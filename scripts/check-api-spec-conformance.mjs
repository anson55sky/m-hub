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

import { readFileSync, existsSync } from 'node:fs'
import { createPublicKey, verify } from 'node:crypto'
import { fileURLToPath } from 'node:url'
import { dirname, join } from 'node:path'

const ROOT = join(dirname(fileURLToPath(import.meta.url)), '..')
const SPEC_RS = join(ROOT, 'src-tauri/src/api_spec.rs')
const ACCOUNT_RS = join(ROOT, 'src-tauri/src/account.rs')
const CHAT_RS = join(ROOT, 'src-tauri/src/chat.rs')
const CONFIG_RS = join(ROOT, 'src-tauri/src/config.rs')
const PATHS_TS = join(ROOT, 'server/src/lib/paths.ts')
const INDEX_TS = join(ROOT, 'server/src/index.ts')
const PUBLIC = join(ROOT, 'server/public')

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
for (const r of registered) {
  if (!serverPaths.has(r.path)) {
    fail(`src/index.ts 注册了 ${r.path}，但它不在 paths.ts 的任何表里（表与代码漂移）`)
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

for (const [, p] of tables.STATIC) {
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
