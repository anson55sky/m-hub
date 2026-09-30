// Node 入口（PocketBay / 本地开发）。
//
// ## 与 Workers 入口的关系
//
// 业务逻辑（`routes/*.ts`）与路由表（`lib/paths.ts`）**完全共用**，只有
// 「怎么把 HTTP 请求变成 Ctx、怎么把返回值变成 Response」这一层不同。
// 那层薄到几乎不需要抽象，所以这里不复用 `lib/http.ts` 的 Worker 版 Router —
// 它返回 Web 标准 `Response`，Node 侧要转成 `ServerResponse`。
// 复用 `lib/paths.ts` 的 `DYNAMIC` 表才是关键：它保证**两条入口注册的路由
// 不会漂**（`check-api-spec-conformance.mjs` 逐条对账）。
//
// ## ⚠️ 协议硬要求（部署协议 §3.4 第 1 类「运行约定」）
//
// 必须读 `process.env.PORT` 并绑定 **`0.0.0.0`**，不是 `127.0.0.1`。
// 绑回环会让平台探活连不上，症状是 `app_not_listening` / `app_port_mismatch`。
// 协议明确要求：**只改绑定，不要顺手改入口文件**。

import { createServer, type IncomingMessage, type ServerResponse } from 'node:http'
import { readFile } from 'node:fs/promises'
import { existsSync } from 'node:fs'
import { join, dirname } from 'node:path'
import { fileURLToPath } from 'node:url'

import { DYNAMIC, STATIC } from './lib/paths.ts'
import { SqliteDb } from './lib/sqlite.ts'
import { PostgresDb } from './lib/postgres.ts'
import { D1Shape } from './lib/d1shape.ts'
import type { Env, User } from './lib/http.ts'
import { devicePoll, deviceStart } from './routes/github.ts'
import { deviceRevoke, deviceTokens, me, redeem } from './routes/me.ts'
import { chatCompletions, models } from './routes/ai.ts'
import { send, verify } from './routes/email.ts'
import {
  apply,
  applyStatus,
  mySubmissions,
  submissionDetail,
  submit,
  withdraw,
} from './routes/submissions.ts'

const HERE = dirname(fileURLToPath(import.meta.url))
/** 静态清单目录（与 Workers 的 `public/` 同一份文件） */
const PUBLIC = join(HERE, '..', 'public')

const PORT = Number(process.env.PORT ?? 8787)
const HOST = '0.0.0.0' // 协议硬要求：不能绑 127.0.0.1

// ---------------------------------------------------------------- 环境

const dbPath = process.env.POCKETBAY_DATA_DIR
  ? join(process.env.POCKETBAY_DATA_DIR, 'm-hub.db')
  : join(HERE, '..', 'data', 'm-hub.db')

/** 仅 SQLite 路径需要（PG 由池管理，退出时 end 即可） */
let sqliteDb: SqliteDb | null = null
let env: Env | null = null

async function boot(): Promise<Env> {
  if (env) return env

  // ---- 选哪个库：看**意图**，不看「有没有 DATABASE_URL」 ----
  //
  // ⚠️ 这里踩过一次真实的坑：最初写成「有 DATABASE_URL 就用 PG，否则 SQLite」。
  // 但 `DATABASE_URL` 是平台在**配对页选过托管库时注入**的，且会**持续存在** ——
  // 即使用户后来改选 D（文件库放 /data），那个变量也还在。
  // 结果：用户明确选了 SQLite，应用却连了 PostgreSQL，而那些表**从未建过**
  // （dump 被平台以「仅支持 pg_dump -Fc」拒绝），于是每个查询都失败。
  // 而日志还打印 `db = /data/m-hub.db`（无条件打印的假信息），看着一切正常。
  //
  // 两条教训：① **配置项存在 ≠ 用户想要它**，选型必须由显式意图决定；
  // ② 日志里每行都必须是真的 —— 假日志比没日志更坏（它会让人查错方向）。
  //
  // 现在的口径：
  //   `MHUB_DB=postgres` → 托管库（须由用户在配对页选 A 并完成 dump 导入）
  //   其它（含未设置）  → SQLite 文件库，落 `POCKETBAY_DATA_DIR`（默认 ./data）
  //
  // 选了 PG 但表不存在 → **明确报错并退出**，不静默降级、不假装正常。
  const wantPostgres = (process.env.MHUB_DB ?? '').toLowerCase() === 'postgres'
  const dbUrl = process.env.DATABASE_URL
  let db: D1Shape
  let sqliteDb2: SqliteDb | null = null

  if (wantPostgres) {
    if (!dbUrl) {
      console.error(
        '[m-hub] ✗ MHUB_DB=postgres 但没有 DATABASE_URL。' +
          '请在配对页选托管库，或把 MHUB_DB 去掉改用 SQLite。',
      )
      process.exit(1)
    }
    const pg = new PostgresDb(dbUrl)
    // 验库：托管库的表是否存在。**不验的后果**是每个接口都返回 500，
    // 而日志里只有一句「数据库 = 平台托管库」，看不出是空库。
    let probe: { n: number } | null = null
    try {
      probe = await pg.first<{ n: number }>(
        `SELECT count(*) AS n FROM information_schema.tables
          WHERE table_schema = 'public' AND table_name IN (?1, ?2, ?3, ?4, ?5, ?6)`,
        'users',
        'sessions',
        'submissions',
        'dev_applications',
        'submission_assets',
        'ai_quota',
      )
    } catch (e) {
      // 连不上托管库要**说清是谁的问题**：否则用户看到 ECONNREFUSED 会去查
      // 自己的网络，而故障在平台侧。
      console.error(
        `[m-hub] ✗ 连不上平台托管库：${(e as Error).message}\n` +
          '        这不是你的应用问题，是平台数据库侧未就绪。\n' +
          '        去掉 MHUB_DB=postgres 可改用 SQLite 文件库（落 POCKETBAY_DATA_DIR）。',
      )
      process.exit(1)
    }
    if ((probe?.n ?? 0) < 6) {
      console.error(
        `[m-hub] ✗ 托管库里只有 ${probe?.n ?? 0}/6 张表。` +
          '结构未导入 —— 平台只接受 pg_dump -Fc 格式的 dump。' +
          '要么导入结构，要么去掉 MHUB_DB 改用 SQLite。',
      )
      process.exit(1)
    }
    db = new D1Shape(pg)
    console.log('[m-hub] 数据库 = 平台托管库（PostgreSQL），6 张表已就位')
  } else {
    const dir = dirname(dbPath)
    const { mkdirSync } = await import('node:fs')
    mkdirSync(dir, { recursive: true })
    sqliteDb2 = new SqliteDb(dbPath)
    db = new D1Shape(sqliteDb2)
    const schemaPath = join(HERE, '..', 'schema.sql')
    if (existsSync(schemaPath)) {
      await sqliteDb2.exec(await readFile(schemaPath, 'utf8'))
      console.log(`[m-hub] 数据库 = SQLite（${dbPath}），schema 已应用`)
    } else {
      console.error(`[m-hub] ⚠️ 找不到 schema.sql（${schemaPath}），数据库可能未初始化`)
    }
    if (dbUrl) {
      console.log('[m-hub] 注：检测到 DATABASE_URL，但未设 MHUB_DB=postgres，按 SQLite 处理')
    }
  }
  sqliteDb = sqliteDb2

  env = {
    // 路线层用的是 D1 形状（`ctx.env.DB.prepare().bind()`），
    // 故把包装后的对象塞进 Env 的 DB 位
    DB: db as unknown as Env['DB'],
    GITHUB_CLIENT_ID: process.env.GITHUB_CLIENT_ID,
    INVITE_CODE: process.env.INVITE_CODE,
    OPENAI_API_KEY: process.env.OPENAI_API_KEY,
    RESEND_API_KEY: process.env.RESEND_API_KEY,
    EMAIL_FROM: process.env.EMAIL_FROM,
  }
  console.log(
    `[m-hub] 配置：GITHUB_CLIENT_ID=${env.GITHUB_CLIENT_ID ? '已配置' : '未配置'} ` +
      `INVITE_CODE=${env.INVITE_CODE ? '已配置' : '未配置'} ` +
      `OPENAI_API_KEY=${env.OPENAI_API_KEY ? '已配置' : '未配置'}`,
  )
  return env
}

// ---------------------------------------------------------------- 静态清单

/** 扩展包在静态资源里的落点：`/packages/<id>/<version>/<file>` */
const PACKAGE_PREFIX = '/packages/'

/** 应用安装包在静态资源里的落点：`/downloads/<版本>/<文件>` */
const DOWNLOAD_PREFIX = '/downloads/'

/**
 * 静态资源直接读文件应答，**不做任何加工**。
 *
 * ⚠️ 清单走这里时，这是签名机制的全部意义所在：客户端 `signing.rs` 验的是
 * **下载到的原始字节**。任何重新序列化（gzip 重压缩 / JSON 美化 / 多一个
 * 尾随换行）都会让验签失败，症状是客户端静默回退缓存 —— 市场空白、
 * 更新永远没有，而接口全部 200。故用 `Buffer` 原样写出。
 *
 * ## 扩展包也走这里（`/packages/…`）
 *
 * 踩过一次真实的坑：最初为了「省掉文件托管」，把清单里的 `downloadUrl`
 * 指向 `github.com/<某仓库>/releases/download/…` —— **那个 release 并不存在**，
 * 症状是用户点「安装」报 `下载失败: HTTP 404`，而服务端一切正常。
 * 写进清单的 URL **必须真的取得到**（发版前逐条 HEAD 验一遍）。
 *
 * 包由本服务端托管，比指去 GitHub 更好：
 * · 不依赖第二个仓库、不依赖第三方发版流程
 * · 与清单同源，`seed:manifests` 一次就产出全部可下载文件
 * · 上游 `x-hub` 本来也是服务端托管（`/api/v1/market/asset/packages/…`）
 */
async function serveStatic(pathname: string, res: ServerResponse): Promise<boolean> {
  const manifests = new Set<string>([
    ...Object.values(STATIC),
    ...Object.values(STATIC).map((p) => p + '.sig'),
  ])
  // 包（扩展 .xhpack）与安装包（.dmg）都放行 —— 二者都是清单里指向的 URL
  const isPackage =
    pathname.startsWith(PACKAGE_PREFIX) || pathname.startsWith(DOWNLOAD_PREFIX)
  if (!manifests.has(pathname) && !isPackage) return false

  const file = isPackage ? join(PUBLIC, pathname.replace(/^\//, '')) : join(PUBLIC, pathname.replace(/^\//, ''))
  if (!file.startsWith(PUBLIC)) {
    // 目录穿越（包路径来自用户输入，这里是唯一的防线，必须严）
    res.writeHead(400).end()
    return true
  }
  try {
    const buf = await readFile(file)
    res.writeHead(200, {
      'content-type': isPackage
        ? 'application/octet-stream'
        : pathname.endsWith('.sig')
          ? 'text/plain; charset=utf-8'
          : 'application/json; charset=utf-8',
      'content-length': String(buf.byteLength),
      // 短缓存：发布后要能较快生效，但别每次都回源
      'cache-control': 'public, max-age=60',
    })
    res.end(buf)
  } catch {
    // 文件不存在时**明确 404 并说明**，不能静默落到路由层 ——
    // 症状会变成「请求进了 Worker 然后 404」，与路径写错完全一样
    res.writeHead(404, { 'content-type': 'application/json; charset=utf-8' })
    res.end(
      JSON.stringify({
        error: 'not_found',
        message: isPackage
          ? `扩展包 ${pathname} 不在部署产物里。请确认 server/public/packages/ 下有该文件（seed:manifests 会生成）。`
          : `静态清单 ${pathname} 未部署。请在 server/ 下跑 npm run seed:manifests 重新生成并连同 .sig 一起上传。`,
      }),
    )
  }
  return true
}

// ---------------------------------------------------------------- 路由

type Handler = (ctx: import('./lib/http.ts').Ctx) => Promise<Response> | Response

const routes: Record<string, Handler> = {
  "POST_/api/v1/auth/github/device/start": (c) => deviceStart(c),
  "POST_/api/v1/auth/github/device/poll": (c) => devicePoll(c),
  "POST_/api/v1/auth/email/send": (c) => send(c),
  "POST_/api/v1/auth/email/verify": (c) => verify(c),
  "GET_/me": (c) => me(c),
  "POST_/api/v1/me/redeem": (c) => redeem(c),
  "GET_/api/v1/me/device-tokens": (c) => deviceTokens(c),
  "POST_/api/v1/me/device-tokens/:id/revoke": (c) => deviceRevoke(c, c.params),
  "GET_/api/v1/ai/models": (c) => models(c),
  "POST_/api/v1/dev/apply": (c) => apply(c),
  "GET_/api/v1/dev/apply": (c) => applyStatus(c),
  "POST_/api/v1/dev/submissions": (c) => submit(c),
  "GET_/api/v1/dev/submissions": (c) => mySubmissions(c),
  "GET_/api/v1/dev/submissions/:id": (c) => submissionDetail(c, c.params),
  "POST_/api/v1/dev/submissions/:id/withdraw": (c) => withdraw(c, c.params),
  // 约定 33：平台对话是 OpenAI 兼容面，**不在** DYNAMIC 表里（见 paths.ts::OPENAI_COMPAT）
  "POST_/v1/chat/completions": (c) => chatCompletions(c),
}

/** 把 `GET /api/v1/dev/submissions/:id` 编成匹配器。 */
function match(method: string, pathname: string): { handler: Handler; params: Record<string, string> } | '404' | '405' {
  const segs = pathname.split('/').filter(Boolean)
  const pathMatched = new Set<string>()
  for (const [key, handler] of Object.entries(routes)) {
    // ⚠️ 只能切**第一个**下划线：路径本身可能含下划线
    // （如 `platform:<模型名>` 之类将来可能出现），`split('_')` 会把路径切碎。
    const sep = key.indexOf('_')
    const m = key.slice(0, sep)
    const pattern = key.slice(sep + 1)
    const parts = pattern.split('/').filter(Boolean)
    if (parts.length !== segs.length) continue
    const params: Record<string, string> = {}
    let ok = true
    for (let i = 0; i < parts.length; i++) {
      const p = parts[i]!
      if (p.startsWith(':')) {
        if (!segs[i]) { ok = false; break }
        params[p.slice(1)] = segs[i]!
      } else if (p !== segs[i]) { ok = false; break }
    }
    if (!ok) continue
    pathMatched.add(m)
    if (m === method) return { handler, params }
  }
  return pathMatched.size > 0 ? '405' : '404'
}

// ---------------------------------------------------------------- 服务器

const server = createServer(async (req: IncomingMessage, res: ServerResponse) => {
  const started = Date.now()
  try {
    // 根路径 + 健康检查。
    //
    // 平台部署时会探测「首页」与「健康路径」，两者都没有响应就判 `probe_failed`
    // （首次部署实测：`error_category: probe_failed`，而 runtime 日志显示服务
    // **已经正常启动** —— 纯粹是探活没处可探）。
    //
    // ⚠️ 本服务是**纯接口 + 静态清单**，没有前端页面。故根路径返回一份
    // 自述 JSON 而不是 404，也算「有响应」。这比另配 `pocketbay.yaml`
    // 声明检查路径更省 —— 后者要额外维护一份与代码不同步的配置。
    if (req.url === '/' || req.url === '/health' || req.url === '/healthz') {
      const body = JSON.stringify({
        service: 'm-hub-server',
        status: 'ok',
        // 列出真实能力，省得排查时逐个试
        endpoints: {
          static: Object.values(STATIC),
          api: Object.values(DYNAMIC).length + 1 + ' 条（登录 / 账号 / 开发者 / 发布 / 平台 AI）',
        },
        db: process.env.DATABASE_URL ? 'postgresql' : 'sqlite',
      })
      res.writeHead(200, { 'content-type': 'application/json; charset=utf-8' })
      res.end(body)
      return
    }

    const e = await boot()
    const url = new URL(req.url ?? '/', `http://${req.headers.host ?? 'localhost'}`)
    const method = req.method ?? 'GET'

    if (await serveStatic(url.pathname, res)) {
      console.log(`[m-hub] ${method} ${url.pathname} static ${Date.now() - started}ms`)
      return
    }

    const hit = match(method, url.pathname)
    if (hit === '404') {
      res.writeHead(404, { 'content-type': 'application/json; charset=utf-8' })
      res.end(JSON.stringify({ error: 'not_found', message: `没有这个接口：${method} ${url.pathname}` }))
      return
    }
    if (hit === '405') {
      res.writeHead(405, { 'content-type': 'application/json; charset=utf-8', allow: 'GET, POST' })
      res.end(JSON.stringify({ error: 'method_not_allowed', message: `${url.pathname} 不接受 ${method}` }))
      return
    }

    // 把 Node 的请求包成 Web 标准 Request，路线层因此不必知道自己在 Node 上
    const headers = new Headers()
    for (const [k, v] of Object.entries(req.headers)) {
      if (typeof v === 'string') headers.set(k, v)
      else if (Array.isArray(v)) headers.set(k, v.join(', '))
    }
    const hasBody = method !== 'GET' && method !== 'HEAD'
    const request = new Request(url.toString(), {
      method,
      headers,
      body: hasBody ? await readBody(req) : undefined,
      // Node 的 fetch 实现要求 duplex:'half' 才能带流式 body
      ...(hasBody ? { duplex: 'half' } : {}),
    } as RequestInit)

    const ctx = {
      req: request,
      env: e,
      url,
      params: hit.params,
      token: req.headers.authorization?.replace(/^Bearer\s+/i, '').trim() || undefined,
    }

    const out = await hit.handler(ctx)
    const buf = Buffer.from(await out.arrayBuffer())
    res.writeHead(out.status, {
      'content-type': out.headers.get('content-type') ?? 'application/json; charset=utf-8',
      'content-length': String(buf.byteLength),
      ...(out.headers.get('cache-control') ? { 'cache-control': out.headers.get('cache-control')! } : {}),
      ...(out.headers.get('allow') ? { allow: out.headers.get('allow')! } : {}),
    })
    res.end(buf)
    console.log(`[m-hub] ${method} ${url.pathname} → ${out.status} ${Date.now() - started}ms`)
  } catch (err) {
    // 协议 §3.4 第 3 类：应用自身问题看日志尾。故这里必须打完整堆栈到 stdout
    console.error(`[m-hub] ${req.method} ${req.url} 未处理异常:`, err)
    if (!res.headersSent) {
      res.writeHead(500, { 'content-type': 'application/json; charset=utf-8' })
    }
    res.end(JSON.stringify({ error: 'server_error', message: '服务端内部错误' }))
  }
})

/** 读完整请求体为 ArrayBuffer（multipart 与 JSON 都要）。 */
function readBody(req: IncomingMessage): Promise<ArrayBuffer> {
  return new Promise((resolve, reject) => {
    const chunks: Buffer[] = []
    req.on('data', (c: Buffer) => chunks.push(c))
    req.on('end', () => {
      const b = Buffer.concat(chunks)
      resolve(b.buffer.slice(b.byteOffset, b.byteOffset + b.byteLength) as ArrayBuffer)
    })
    req.on('error', reject)
  })
}

server.listen(PORT, HOST, () => {
  console.log(`[m-hub] listening on ${HOST}:${PORT}（pid ${process.pid}）`)
  // ⚠️ 这里**不能**无条件打印 dbPath —— 用托管库时那行是假信息，
  // 而假日志比没日志更坏（实测踩过：日志说 db=/data/m-hub.db，实际连的是 PG）。
  // 库的实际选择已在 boot() 里如实打印，这里不再重复。
  // 协议 §3.4：不要求健康接口，但有一个成本极低且便于排查
  console.log(`[m-hub] 注册路由 ${Object.keys(routes).length} 条 + 静态 ${Object.keys(STATIC).length} 份清单`)
})

// 优雅退出：让 WAL 落盘
for (const sig of ['SIGTERM', 'SIGINT'] as const) {
  process.on(sig, () => {
    console.log(`[m-hub] 收到 ${sig}，关闭中…`)
    server.close(() => {
      sqliteDb?.close()
      process.exit(0)
    })
    // 兜底：3s 内没关干净就强退，避免平台等太久判定启动失败
    setTimeout(() => {
      sqliteDb?.close()
      process.exit(0)
    }, 3000).unref()
  })
}

export type { User }
