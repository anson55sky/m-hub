#!/usr/bin/env node
// 客户端 ↔ 服务端**路径契约**的对账。
//
// ## 为什么要这个脚本
//
// 约定 47：真相源只有 `src-tauri/src/api_spec.rs`（Rust 侧有测试锁住路径与
// 蛇形 body 键名）。但服务端在另一个语言、另一个仓库形态下，**必然是第二份
// 拷贝** —— 而这层的失败模式极其恶劣：两端各自的测试全绿，**只有真联调才
// 404**，而失败现场往往离改动点很远（打错一个字 → 路由不匹配 → 404；
// 少写一段前缀 → 同样 404；服务端注册了但忘了挂 → 同样 404。三种病因一个症状）。
//
// 所以不能靠「抄的时候小心」，要让漂移**在构建期就响**。本脚本从 Rust 源码
// 里机械抽出全部路径，与 `server/src/lib/paths.ts` 逐条比对。
//
// ## 它守的四件事
//
// ① 路径集合一致（Rust 抽出的 ⊆ 服务端表，且服务端表无多余项）
// ② `MOUNTED`（真挂载的）== `DYNAMIC`（应该有的）—— 防「加了表忘了挂」
// ③ `index.ts` 里 `r.add(...)` 的字面路径 == 表里的路径 —— 防「表改了、
//    路由代码没改」（这正是 404 的最常见成因）
// ④ `public/` 下确实存在那 4 个静态签名文件 —— 防「以为部署了其实没有」
//    （静态文件不存在时静态资源不报错，只是请求落进 Worker 然后 404，
//     症状与路由写错一模一样）
//
// 另有两条**具名断言**（无法从源码自动推出的语义约定）：
//  · `/me` 走根路径而非 `/api/v1/me`（约定 52 记的坑：按直觉「修正」就 404）
//  · `POST /v1/chat/completions` 必须存在（约定 33；它由 `chat.rs` 拼出来，
//    **不在** api_spec.rs 里，所以前四条的机械对账抓不到它）

import { existsSync, readFileSync, readdirSync, statSync } from 'node:fs'
import { createPublicKey, verify } from 'node:crypto'
import { fileURLToPath } from 'node:url'
import { dirname, join, relative } from 'node:path'

const ROOT = join(dirname(fileURLToPath(import.meta.url)), '..')
const SPEC_RS = join(ROOT, 'src-tauri/src/api_spec.rs')
const ACCOUNT_RS = join(ROOT, 'src-tauri/src/account.rs')
const CHAT_RS = join(ROOT, 'src-tauri/src/chat.rs')
const CONFIG_RS = join(ROOT, 'src-tauri/src/config.rs')
const PATHS_TS = join(ROOT, 'server/src/lib/paths.ts')
// 路由表在 `handle.ts`（Workers 入口与 Pages Functions 共用同一份）。
// 曾指向 `index.ts` —— 那是把路由搬进 handle.ts 之后遗留的，守卫当场报
// 「16 条全部没有对应注册」，正好证明它在守真东西。
const INDEX_TS = join(ROOT, 'server/src/handle.ts')
const PUBLIC = join(ROOT, 'server/public')

/**
 * Pages Functions 的三个入口文件。
 *
 * ⚠️ **必须存在且必须是这三个前缀**：Pages 的路由优先级与 Workers 相反
 *    （Functions 优先于静态资产），所以 Functions 入口既不能少（少了那条前缀
 *    的 API 全部 404）也不能多（多一个 `functions/api/[[path]].ts` 之类的
 *    catch-all 会把签名清单也吃掉，导致验签失败 —— 那是静默的）。
 *
 * 少一个的失败现场很远（部署后客户端才报 404/验签失败），故在这里拦。
 */
/**
 * Pages Functions 覆盖的前缀 —— **从 `functions/` 目录推导**，不手写列表。
 *
 * ⚠️ 原来是一份手写的 `{ file, covers, why }[]`。那是本脚本自己在别处警告过的
 *   东西（「靠人维护的对照表会悄悄漂移，而它守的恰恰就是漂移」），而这里守的
 *   正是漂移：漏一个前缀 = 一整段路由在 Pages 上变成**死路由**。
 *
 *   它真的漏过：`/packages/:extId/:version/:file`（2026-10-02 加的包体直发）
 *   在 `handle.ts` 里注册了、Workers 上跑得好好的，而 Pages 上因为没有
 *   `functions/packages/` 目录，请求根本不进 Function —— 由静态资产应答了
 *   `public/packages/` 下**重新打包**的那份字节（7853 字节，sha 不符）。
 *   症状是「HTTP 200、能下载、只有客户端校验 sha256 时才失败」，而服务端无报错。
 *
 * 推导规则：文件相对 `functions/` 的路径去掉 `.ts` 与 `[[path]]` 段，
 * 即为它覆盖的前缀（`api/v1/[[path]].ts` → `/api/v1/`、`me.ts` → `/me`）。
 */
function pagesFunctionPrefixes() {
  const base = join(ROOT, 'server', 'functions')
  if (!existsSync(base)) return []
  /** @type {{ file: string; covers: string }[]} */
  const out = []
  const walk = (dir) => {
    for (const name of readdirSync(dir)) {
      const full = join(dir, name)
      if (statSync(full).isDirectory()) {
        walk(full)
        continue
      }
      if (!name.endsWith('.ts')) continue
      // ⚠️ 相对 **functions/ 自己**取，不是相对 server/ —— 否则前缀会变成
      //   /functions/api/v1，与注册路径 /api/v1/... 永远匹配不上（第一版就这么写的，
      //   结果整份守卫把 20 条路由全报成死路由）。
      const rel = relative(base, full)
      const covers =
        '/' +
        rel
          .replace(/\.ts$/, '')
          .split('/')
          .filter((seg) => seg !== '[[path]]')
          .join('/')
      // file 相对 **server/**（下游会 join(ROOT, 'server', file)），
      // 而 covers 相对 **functions/**（那才是路由前缀）。两个基准不同，
      // 第一版把两个都用同一个基准，害得整份守卫把 20 条路由报成死路由。
      out.push({ file: join('functions', rel), covers })
    }
  }
  walk(base)
  return out
}

const PAGES_FUNCTIONS = pagesFunctionPrefixes()

const problems = []
const fail = (m) => problems.push(m)

// ---------------------------------------------------------------- 读源

const stripTests = (src) => {
  const i = src.indexOf('#[cfg(test)]')
  return i === -1 ? src : src.slice(0, i)
}

/**
 * 抽路径字面量。
 *
 * 要处理的写法（都出现在 api_spec.rs 里）：
 *   "/api/v1/dev/apply"                                  裸字符串
 *   format!("/api/v1/dev/submissions/{id}")              `{id}` 是动态段
 *   format!("{}/api/v1/auth/github/device/start", ...)   `{}` 是 base 拼接点
 *   "https://x.example/api/v1/..."                       只出现在测试里，已剥掉
 *
 * ⚠️ `(?:\{\})?` 这个可选前缀是**必需的**，不是锦上添花：
 *   漏了它 → 15 条路由里有 7 条（全部 `{}` 拼接形式）根本抽不出来，
 *   而「客户端 ⊆ 服务端」这个方向对**缺失的项一律通过** —— 守卫会一片绿地
 *   放行「把这 7 条从服务端删掉」。实测踩过（第一版就是这么写的）。
 *   `MUST_EXTRACT` 那组自检就是为堵这个洞存在的。
 *
 * 归一化：`{id}` → `:id`（`:` 加名字，**不能**把每个花括号各换一个冒号 ——
 * 那样 `{id}` 会变成 `:id:`，实测也踩过）。
 */
function extractPaths(src) {
  const out = new Set()
  // 只认 `/api/v1` 与 `/me`。**不认裸 `/v1`**：`/v1/chat/completions` 是
  // `chat.rs` 拼 `{SERVER}/v1` + `/chat/completions` 得到的，字面量里从不出现；
  // 把它写进正则只会从 `format!("{}/v1", …)` 里捞出一个 `/v1` 前缀当路由
  // （实测踩过：报「客户端会请求 /v1，但服务端没有」）。
  const re = /"(?:\{\})?(?:https?:\/\/[^"]*?)?(?:\/api\/v1|\/me)[^"]*"/g
  let m
  while ((m = re.exec(src)) !== null) {
    let p = m[0].slice(1, -1)
    p = p.replace(/^https?:\/\/[^/]+/, '')
    p = p.replace(/\/\d+(\/|$)/, '$1') // 丢掉测试夹具的纯数字段（`.../9`）
    p = p.replace(/\{([a-z_]*)\}/g, (_, name) => (name ? `:${name}` : ''))
    if (!p.startsWith('/')) continue
    out.add(p)
  }
  return out
}

/**
 * 解析器自检：这些路径**必须**能被抽出来。
 *
 * 没有这组自检，解析器坏掉时守卫会「因为抽得少而全绿」—— 一个静默失效的
 * 守卫比没有守卫更糟（约定 75 的精神：绿不等于没坏）。这组是从 api_spec.rs
 * 的实际内容抄的；任何一条抽不到就报「解析器坏了」，而不是继续比对。
 */
const MUST_EXTRACT = [
  '/api/v1/auth/github/device/start',
  '/api/v1/auth/github/device/poll',
  '/api/v1/auth/email/send',
  '/api/v1/auth/email/verify',
  '/api/v1/me/redeem',
  '/api/v1/me/device-tokens',
  '/api/v1/me/device-tokens/:id/revoke',
  '/api/v1/dev/apply',
  '/api/v1/dev/submissions',
  '/api/v1/dev/submissions/:id',
  '/api/v1/dev/submissions/:id/withdraw',
  '/api/v1/ai/models',
]

/** 从 paths.ts 抽出各张表。 */
function extractTsTables(src) {
  const grab = (name) => {
    const re = new RegExp(`export const ${name} = \\{([\\s\\S]*?)\\n\\} as const`)
    const m = re.exec(src)
    if (!m) return []
    const entries = []
    const er = /(\w+):\s*'([^']+)'/g
    let e
    while ((e = er.exec(m[1])) !== null) entries.push([e[1], e[2]])
    return entries
  }
  return {
    STATIC: grab('STATIC'),
    DYNAMIC: grab('DYNAMIC'),
    OPENAI_COMPAT: grab('OPENAI_COMPAT'),
    MOUNTED: (() => {
      const re = /export const MOUNTED = \[([\s\S]*?)\] as const/
      const m = re.exec(src)
      if (!m) return []
      return [...m[1].matchAll(/'([^']+)'/g)].map((x) => x[1])
    })(),
  }
}

const specSrc = stripTests(readFileSync(SPEC_RS, 'utf8'))
const accountSrc = stripTests(readFileSync(ACCOUNT_RS, 'utf8'))
const chatSrc = stripTests(readFileSync(CHAT_RS, 'utf8'))
// 两条静态清单地址**不在** api_spec.rs —— 它们是 config.rs 的常量
// （约定 52：市场/升级地址都由 DEFAULT_SERVER_URL 拼出，且地址是编译期常量）
const configSrc = stripTests(readFileSync(CONFIG_RS, 'utf8'))
const tables = extractTsTables(readFileSync(PATHS_TS, 'utf8'))
const indexSrc = readFileSync(INDEX_TS, 'utf8')

// ---------------------------------------------------------------- 0a. Pages Functions 入口

for (const { file, covers, why } of PAGES_FUNCTIONS) {
  const p = join(ROOT, 'server', file)
  if (!existsSync(p)) {
    fail(
      `Pages Functions 入口缺失：server/${file}（覆盖 ${covers} —— ${why}）。\n` +
        `    Pages 的 Functions 优先于静态资产，该前缀下没有 Function 就会 404；\n` +
        `    而清单与包在 public/ 里，签名清单被 Function 抢走则客户端验签失败（静默）。`,
    )
    continue
  }
  // 入口必须真的转给共用路由，而不是自己实现一套 —— 两套实现必然漂移
  const src = readFileSync(p, 'utf8')
  if (!/handleRequest/.test(src)) {
    fail(
      `server/${file} 没有调用 handleRequest。\n` +
        `    Functions 入口必须转给共用的路由表；自己实现一套会让两个运行时行为漂移，` +
        `而漂移的失败现场在运行期（404 / 验签失败），离改动点很远。`,
    )
  }
  // catch-all 必须用双方括号，否则只匹配一段路径
  if (file.includes('[[path]]') && !/\[\[path\]\]/.test(p)) {
    fail(`server/${file} 的 catch-all 语法不对。`)
  }
}

// ---------------------------------------------------------------- 0. 解析器自检

const specOnly = extractPaths(specSrc)
for (const p of MUST_EXTRACT) {
  if (!specOnly.has(p)) {
    fail(
      `解析器自检失败：api_spec.rs 里没能抽出 ${p}。\n` +
        `    守卫会因为「抽得少」而全绿 —— 那比没有守卫更糟。\n` +
        `    先修 scripts/check-api-spec-conformance.mjs 的 extractPaths，别改断言。`,
    )
  }
}

// ---------------------------------------------------------------- ① 路径集合

// 客户端侧：api_spec.rs 的路径 + account.rs 里的 `/me` + config.rs 的两条静态清单
const clientPaths = new Set([
  ...specOnly,
  ...extractPaths(accountSrc),
  ...extractPaths(configSrc),
])
// chat.rs 拼的是 `{SERVER}/v1` + `/chat/completions`，路径不在字面量里
const chatBaseIsV1 = /format!\("\{\}\/v1"/.test(chatSrc)
if (chatBaseIsV1) clientPaths.add('/v1/chat/completions')

const serverPaths = new Set([
  ...tables.DYNAMIC.map(([, p]) => p),
  ...tables.STATIC.map(([, p]) => p),
  ...tables.OPENAI_COMPAT.map(([, p]) => p),
])

for (const p of clientPaths) {
  if (!serverPaths.has(p)) {
    fail(`客户端会请求 ${p}，但 server/src/lib/paths.ts 的表里没有它 → 运行期 404`)
  }
}
// 静态那两条客户端只在 Rust 里经 format! 拼（config.rs），故 clientPaths 已含
for (const [k, p] of tables.STATIC) {
  if (!clientPaths.has(p)) {
    fail(`paths.ts::STATIC.${k} = ${p}，但客户端没有任何地方请求它（多余项，或客户端改了拼法）`)
  }
}

// ---------------------------------------------------------------- ② MOUNTED == DYNAMIC

const dynKeys = tables.DYNAMIC.map(([k]) => k)
for (const k of dynKeys) {
  if (!tables.MOUNTED.includes(k)) fail(`paths.ts::DYNAMIC 有 ${k}，但没登记进 MOUNTED（忘了挂？）`)
}
for (const k of tables.MOUNTED) {
  if (!dynKeys.includes(k)) fail(`paths.ts::MOUNTED 有 ${k}，但 DYNAMIC 里没有（清单与实际不符）`)
}

// ---------------------------------------------------------------- ③ index.ts 真挂了

// r.add('GET', '/api/v1/me', …) 或 r.add('POST', OPENAI_COMPAT.post_chat_completions, …)
const registered = [...indexSrc.matchAll(/r\.add\(\s*'(\w+)'\s*,\s*([^,]+),/g)].map(([, method, expr]) => ({
  method,
  // 字面量直接取值；`TABLE.key` 形式回查表 —— 两种都归一成实际路径字符串
  path: resolveExpr(expr.trim(), tables),
}))

// ⚠️ 必须按 **(方法, 路径)** 成对比对，不能只比路径：`/api/v1/dev/apply` 与
// `/api/v1/dev/submissions` 都是**同时**有 GET 和 POST 的（申请状态 / 我的提交），
// 只比路径会拿第一条去比方法、报出「期望 GET、注册成 POST」这种假警报（实测踩过）。
for (const [k, p] of tables.DYNAMIC) {
  const want = expectedMethod(k)
  if (!registered.some((r) => r.path === p && r.method === want)) {
    const samePath = registered.filter((r) => r.path === p).map((r) => r.method)
    const hint = samePath.length
      ? `（该路径已注册了 ${samePath.join('/')}，但缺 ${want}）`
      : ''
    fail(`paths.ts::DYNAMIC.${k} = ${want} ${p}，但 src/index.ts 里没有对应注册${hint} → 运行期 404/405`)
  }
}
/**
 * 故意**不在** `paths.ts` 里的路径前缀（2026-10-01 起）。
 *
 * 管理端（`/api/v1/admin/*`）没有**客户端消费者** —— 没有管理界面，
 * 走的是 CLI（`npm run review`）。把它塞进 `DYNAMIC` 会让本守卫要求
 * `api_spec.rs` 也有对应项，而客户端永远不会调它，那条契约是假的。
 *
 * 所以这里列一个**显式豁免**，而不是放宽整个「注册了但表里没有」的检查。
 * 理由：放宽会让**任何**新端点都能悄悄逃过契约对账（那才是真问题）。
 * 豁免是**窄的、具名的**，且下面另有一条断言检查「它确实没有客户端」。
 */
const NO_CLIENT_PREFIXES = [
  {
    prefix: '/api/v1/admin/',
    why: '管理端：没有客户端消费者（无管理界面，走 CLI）。放进去会要求 api_spec.rs 有一条客户端永不调用的假契约。',
  },
]

/**
 * `.sig` 是**客户端按 `{url}.sig` 拼出来**的，不是独立调用的路径。
 *
 * ⚠️ 这不是「随便加个端点」的例外，而是一条真实存在的约定：
 *   `market.rs::fetch_market_once` 取 `{endpoint}` 与 `format!("{endpoint}.sig")`。
 *   把它列进 `paths.ts` 反而是错的 —— 客户端代码里**没有**这个字面量，
 *   一旦有人照着表去生成调用就会发现对不上。
 *
 * 双向断言在下面：`.sig` 必须在客户端源码里真的以「`{endpoint}.sig`」的形态出现。
 */
const DERIVED_SIG_SUFFIX = '.sig'

/**
 * `/packages/*` 的 URL 形状是**服务端自己的事**，不是客户端契约。
 *
 * ⚠️ 客户端从来不拼这个路径：它只取清单里的 `entry.downloadUrl`，
 *   而那个值由服务端写进已签名的清单。所以把它列进 `paths.ts` 反而要求
 *   `api_spec.rs` 有一条「客户端代码里根本没有这个字面量」的假契约。
 *
 * 双向断言在下面：客户端源码里必须**找不到** `/packages/` 字面量 ——
 * 哪天有人在 Rust 侧硬编码了这个路径，这条会红，
 * 因为那意味着下载地址不再由清单决定（而清单是验过签的）。
 */
const SERVER_SHAPED_PREFIXES = [
  {
    prefix: '/packages/',
    why: '扩展包分发：地址来自已签名清单里的 downloadUrl，客户端只取该值、不拼路径。',
  },
]

for (const r of registered) {
  if (serverPaths.has(r.path)) continue
  if (r.path.endsWith(DERIVED_SIG_SUFFIX)) continue
  if (SERVER_SHAPED_PREFIXES.some((e) => r.path.startsWith(e.prefix))) continue
  const exempt = NO_CLIENT_PREFIXES.find((e) => r.path.startsWith(e.prefix))
  if (exempt) continue
  fail(`src/handle.ts 注册了 ${r.path}，但它不在 paths.ts 的任何表里（表与代码漂移）`)
}

/** 反向：每个 `.sig` 端点都必须有一个对应的非 `.sig` 端点，否则客户端取不到签名 */
for (const r of registered) {
  if (!r.path.endsWith(DERIVED_SIG_SUFFIX)) continue
  const base = r.path.slice(0, -DERIVED_SIG_SUFFIX.length)
  if (!registered.some((x) => x.path === base)) {
    fail(`${r.path} 有签名端点却没有对应的清单端点 ${base} —— 客户端会取到签名却取不到内容`)
  }
}

for (const { prefix, why } of SERVER_SHAPED_PREFIXES) {
  if (specSrc.includes(prefix)) {
    fail(`api_spec.rs 里出现了 ${prefix} —— ${why}\n    一旦客户端硬编码这个路径，下载地址就不再由已签名的清单决定。`)
  }
}

/**
 * **死路由检查**：每条注册在 `handle.ts` 里的路径，都必须落在某个
 * `functions/` 目录覆盖的前缀下。
 *
 * ⚠️ Pages 的 Functions 是**按目录结构**声明的，没有全局 catch-all。
 *   没有对应目录 → 请求不进 Function → 由静态资产应答 → `handle.ts` 里
 *   那条 `r.add` 在 Pages 上是**死代码**，Workers 上却正常工作。
 *
 * 实测踩过（2026-10-02）：`/packages/:extId/:version/:file` 加进去时忘了建
 * `functions/packages/`，于是 Pages 仍在发 `public/packages/` 下**重新打包**的
 * 字节（7853 vs 清单声明的 7775）。症状是 HTTP 200、能下载、
 * 只有客户端校验 sha256 时失败 —— 服务端日志一片正常。
 */
for (const r of registered) {
  // 需要的 functions 目录 = 路径里**第一个参数段之前**的那段字面量。
  // ⚠️ 不要「过滤掉所有参数段再拼」—— 那样 `/a/:id/b` 会变成 `/a/b`，
  //   然后错误地要求建 `functions/a/b/`（实测第一版就写成了这样，
  //   结果把 `/api/v1/admin/submissions/:id/package` 报成缺 `functions/api/v1/admin/submissions/package/`）。
  const segs = r.path.split('/')
  const firstParam = segs.findIndex((seg) => seg.startsWith(':'))
  // ⚠️ `r.path` 本身以 `/` 开头，所以 split 的第 0 段是空串。必须原样保留，
  //   否则拼出来是 `api/v1/...`（无前导斜杠）而 `/api/v1/` 匹配不上 ——
  //   报错信息里那条 `//api/v1/...` 的双斜杠正是这个来源。
  const literal = segs.slice(0, firstParam === -1 ? undefined : firstParam).join('/')
  const covered = PAGES_FUNCTIONS.some((f) => literal.startsWith(f.covers))
  if (!covered) {
    fail(
      `src/handle.ts 注册了 ${r.path}，但 functions/ 下没有任何目录覆盖 ${literal} —— ` +
        `它在 Pages 上是**死路由**（请求不进 Function，由静态资产应答，而 Workers 上却正常）。\n` +
        `    修法：建 server/functions/${literal.replace(/^\//, '')}/[[path]].ts 并转给 handleRequest。`,
    )
  }
}

/**
 * 豁免是**双向**的：上面说「不要求它在契约里」，这里就要求
 * **客户端确实没有在调它**。少了这条，豁免就变成了「一个可以随手加端点
 * 而不用对账的后门」—— 将来真给管理端做了界面，这条会红，
 * 提醒你把它挪进正式契约（那时它就是有消费者的真端点了）。
 */
for (const { prefix, why } of NO_CLIENT_PREFIXES) {
  if (specSrc.includes(prefix)) {
    fail(
      `api_spec.rs 里出现了 ${prefix}* —— 它已经有客户端消费者了，\n` +
        `    请把它加进 paths.ts::DYNAMIC（以及本文件的注册对账），别留在豁免里。\n` +
        `    豁免理由是：${why}`,
    )
  }
}

/** 把 `'/a/b'` 或 `OPENAI_COMPAT.post_chat_completions` 解析成实际路径。 */
function resolveExpr(expr, t) {
  const lit = expr.replace(/^['"]|['"]$/g, '')
  if (!/^[A-Z_]+\./.test(lit)) return lit
  const [table, key] = lit.split('.')
  const t2 = t[table.toLowerCase()] ?? t[table] ?? []
  const found = (Array.isArray(t2) ? t2 : []).find(
    (e) => Array.isArray(e) && (e[0] === key || e[1] === key),
  )
  return found ? found[1] : lit
}

/** 表的键名前缀就是方法（`get_me` → GET）。写错方法会 405，故这里钉住。 */
function expectedMethod(k) {
  return k.split('_')[0].toUpperCase()
}

// ---------------------------------------------------------------- ④ 静态文件在位 + 验签

/**
 * 不只查「文件存在」，还要查「**签名真的验得过客户端内嵌的那把公钥**」。
 *
 * 「文件存在」抓到的只是最粗的漏（压根没生成）。真正阴的那一类是**签名过期**：
 * 轮换过公钥、或改了清单却没重签 —— 文件照样在、路径照样对，可客户端
 * `signing.rs` 验不过 → **静默回退本地缓存** → 界面表现是「市场一直是空的 /
 * 更新永远没有」，而服务端 `curl` 一切正常。那种故障能查一整天。
 *
 * 所以用 Node 内置的 Ed25519（`crypto.verify` + SPKI DER，零依赖）真跑一遍。
 * 公钥取 `src-tauri/keys/market_public.key` —— 与
 * `signing.rs::MARKET_PUBLIC_KEY_B64` 同一个文件。
 */
const PUBKEY = join(ROOT, 'src-tauri/keys/market_public.key')
/** Ed25519 SPKI 的 DER 前缀，后接 32 字节裸公钥。 */
const ED25519_SPKI_PREFIX = Buffer.from('302a300506032b6570032100', 'hex')

function verifyDetached(pubB64, content, sigB64) {
  const raw = Buffer.from(pubB64.trim(), 'base64')
  if (raw.length !== 32) throw new Error(`公钥长度非法（${raw.length}，应为 32 字节）`)
  const key = createPublicKey({
    key: Buffer.concat([ED25519_SPKI_PREFIX, raw]),
    format: 'der',
    type: 'spki',
  })
  // Ed25519 的算法参数传 null（签名自带算法标识）
  return verify(null, content, key, Buffer.from(sigB64.trim(), 'base64'))
}

const pubB64 = readFileSync(PUBKEY, 'utf8')

/**
 * `/api/v1/market/registry` 不再是静态文件（2026-10-02 起运行期签名）。
 *
 * 原来它由 `scripts/seed-manifests.mjs` 写进 `public/` 再由 CDN 直发。
 * 改成运行期签名后由 `routes/market.ts` 从 D1 取已签名字节 —— 因为上架
 * 必须能在审核台点一下就生效，而构建期脚本要求有人在持有私钥的机器上跑命令
 * （用户报的就是「没有上架按钮、市场还是旧版」）。
 *
 * ⚠️ 这里**显式列出**而不是「从 STATIC 表里减掉」：`tables.STATIC` 是从
 *   `paths.ts` 读的，改那里会连带影响客户端契约的判定。而「哪些清单是静态的」
 *   是一个**语义决定**，写在这里并配注释，比让它悄悄从表里消失更安全。
 *
 * 少了这条豁免的话，下一个人会把静态文件路径加回 `paths.ts`，
 * 于是又出现「构建期一份、运行期一份」的清单 —— 两份签名不同，
 * 而客户端缓存哪份就决定了市场显示哪个版本，且没有任何报错。
 */
const RUNTIME_SIGNED = new Set(['/api/v1/market/registry'])

for (const [, p] of tables.STATIC) {
  if (RUNTIME_SIGNED.has(p)) continue
  const base = join(PUBLIC, p.replace(/^\//, ''))
  const sigFile = base + '.sig'
  if (!existsSync(base)) {
    fail(`静态资源 ${p} 不存在（server/public${p}）——静态文件缺失时请求会落进 Worker 然后 404，症状与路由写错一模一样`)
    continue
  }
  if (!existsSync(sigFile)) {
    fail(`签名 ${p}.sig 不存在 —— 客户端取不到签名 → 整条功能静默死`)
    continue
  }
  let ok
  try {
    ok = verifyDetached(pubB64, readFileSync(base), readFileSync(sigFile, 'utf8'))
  } catch (e) {
    fail(`${p} 验签抛错：${e}`)
    continue
  }
  if (!ok) {
    fail(
      `${p} 的签名**通不过** src-tauri/keys/market_public.key 内嵌的那把公钥。\n` +
        `    两种原因：① 轮换过公钥但没重新签（在 server/ 下跑 npm run seed:manifests）\n` +
        `             ② 签完又改了清单内容（签的是原始字节，改了就废了）\n` +
        `    症状是客户端静默回退本地缓存：市场空白 / 更新永远没有，而服务端 curl 一切正常。`,
    )
  }
}

// ---------------------------------------------------------------- 具名语义断言

// `/me` 在根路径，不在 /api/v1 下（约定 52：按直觉「修正」成 /api/v1/me 会立刻 404）
if (!clientPaths.has('/me')) {
  fail("客户端的 /me 没抽出来 —— 解析器坏了，或 account.rs 改了取法（约定 52：/me 走根路径，别搬进 /api/v1）")
}
if (serverPaths.has('/api/v1/me')) {
  fail('服务端把 /me 写成了 /api/v1/me —— 约定 52 明确记了这个坑，客户端只取根路径 /me')
}
// 平台对话面（约定 33）
if (!serverPaths.has('/v1/chat/completions')) {
  fail('缺 POST /v1/chat/completions —— 约定 33：平台对话是这个端点，不补的话开了额度一发消息就失败')
}
if (!chatBaseIsV1) {
  fail("chat.rs 的 platform_base_url 不再是 {SERVER}/v1 —— 那 paths.ts::OPENAI_COMPAT 的 /v1/chat/completions 就对不上了")
}

// ---------------------------------------------------------------- 结论

if (problems.length) {
  console.error('✗ 客户端 ↔ 服务端路径契约不一致：\n')
  for (const p of problems) console.error(`  · ${p}`)
  console.error(`\n  真相源是 src-tauri/src/api_spec.rs。改契约时先看这里的报错，再改两边。`)
  process.exit(1)
}

console.log(
  `✓ 路径契约一致：客户端 ${clientPaths.size} 条 ↔ 服务端 ${serverPaths.size} 条（含静态 ${tables.STATIC.length} / 动态 ${tables.DYNAMIC.length} / OpenAI 兼容 ${tables.OPENAI_COMPAT.length}），MOUNTED 与 r.add 均已对账`,
)
