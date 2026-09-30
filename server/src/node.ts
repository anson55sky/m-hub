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

import { STATIC } from './lib/paths.ts'
import { SqliteDb } from './lib/sqlite.ts'
import { D1Shape, type Db } from './lib/d1shape.ts'
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

let sqlite: SqliteDb | null = null
let env: Env | null = null

async function boot(): Promise<Env> {
  if (env) return env
  const dir = dirname(dbPath)
  const { mkdirSync } = await import('node:fs')
  mkdirSync(dir, { recursive: true })

  sqlite = new SqliteDb(dbPath)
  const db = new D1Shape(sqlite as unknown as Db)

  // 建表：schema.sql 是多语句脚本，SQLite 语法，与 D1 共用同一份
  const schemaPath = join(HERE, '..', 'schema.sql')
  if (existsSync(schemaPath)) {
    await sqlite.exec(await readFile(schemaPath, 'utf8'))
    console.log(`[m-hub] schema 已应用（${dbPath}）`)
  } else {
    console.error(`[m-hub] ⚠️ 找不到 schema.sql（${schemaPath}），数据库可能未初始化`)
  }

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

/**
 * 静态清单直接读文件应答，**不做任何加工**。
 *
 * ⚠️ 这是签名机制的全部意义所在：客户端 `signing.rs` 验的是**下载到的原始字节**。
 * 任何重新序列化（gzip 重压缩 / JSON 美化 / 多一个尾随换行）都会让验签失败，
 * 症状是客户端静默回退缓存 —— 市场空白、更新永远没有，而接口全部 200。
 *
 * 故这里用 `Buffer` 原样写出，且**不设 `content-encoding`**（由 Node 自行决定，
 * 我们不主动压缩）。
 */
async function serveStatic(pathname: string, res: ServerResponse): Promise<boolean> {
  const wanted = new Set<string>([...Object.values(STATIC), ...Object.values(STATIC).map((p) => p + '.sig')])
  if (!wanted.has(pathname)) return false

  const file = join(PUBLIC, pathname.replace(/^\//, ''))
  if (!file.startsWith(PUBLIC)) {
    // 目录穿越（`pathname` 已被白名单挡掉，这里是第二道）
    res.writeHead(400).end()
    return true
  }
  try {
    const buf = await readFile(file)
    res.writeHead(200, {
      'content-type': pathname.endsWith('.sig') ? 'text/plain; charset=utf-8' : 'application/json; charset=utf-8',
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
        message: `静态清单 ${pathname} 未部署。请在 server/ 下跑 npm run seed:manifests 重新生成并连同 .sig 一起上传。`,
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
  console.log(`[m-hub] db = ${dbPath}`)
  // 协议 §3.4：不要求健康接口，但有一个成本极低且便于排查
  console.log(`[m-hub] 注册路由 ${Object.keys(routes).length} 条 + 静态 ${Object.keys(STATIC).length} 份清单`)
})

// 优雅退出：让 WAL 落盘
for (const sig of ['SIGTERM', 'SIGINT'] as const) {
  process.on(sig, () => {
    console.log(`[m-hub] 收到 ${sig}，关闭中…`)
    server.close(() => {
      sqlite?.close()
      process.exit(0)
    })
    // 兜底：3s 内没关干净就强退，避免平台等太久判定启动失败
    setTimeout(() => {
      sqlite?.close()
      process.exit(0)
    }, 3000).unref()
  })
}

export type { User }
