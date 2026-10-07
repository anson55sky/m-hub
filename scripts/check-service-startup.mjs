// 构建期守卫：service 扩展的「懒启动」不许占住主线程。
//
// ## 为什么要有这个守卫
//
// 2026-10-06 实机事故：用户装了一个 `runtime: service` 的扩展，一打开就**整窗卡死**。
// 成因链是三件事叠在一起，每一件单看都「像合理的写法」：
//
//   ① `read_extension_entry` 是**同步** `#[tauri::command]` → Tauri 派它上主线程；
//   ② 它内联调 `start_service`（懒启动后端，约定 45），而 `resolve_node` 里
//      `block_on(download_builtin_node(app))` —— **在主线程上下载**；
//   ③ 那个下载用 `reqwest::get`，**既无连接超时也无读取超时**。
//
// 实测国内链路 nodejs.org 可达但只有 ~340KB/s，48.8MB 的运行时 = 2.5 分钟冻结；
// 连接一旦半死，没有读取超时就是**永久冻结**（用户只能强杀）。
//
// 单测抓不到这类问题（它是「签名 + 调用方式 + HTTP 客户端配置」三者的组合），
// 而每一处都可能被下一个人「顺手改回去」——尤其 ③：把建好的 client 换回
// `reqwest::get` 看起来完全无害。所以这里做机械检查。
//
// 三条规则都做过变异验证：逐条破坏后本守卫必须报错。

import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { dirname, join } from 'node:path'

const root = join(dirname(fileURLToPath(import.meta.url)), '..')
const read = (p) => readFileSync(join(root, p), 'utf8')

const extensionRs = read('src-tauri/src/extension.rs')
const runtimeRs = read('src-tauri/src/runtime.rs')
const serviceRs = read('src-tauri/src/service.rs')

let failed = 0
function fail(msg) {
  console.error(`  ✗ ${msg}`)
  failed++
}

/**
 * 规则 ①：`read_extension_entry` 必须是 `async fn`。
 *
 * ⚠️ 只认 `pub async fn read_extension_entry(` 这一种写法。写成
 *   `pub fn` 就是主线程跑，卡死立刻复现。
 */
if (!/pub async fn read_extension_entry\s*\(/.test(extensionRs)) {
  fail(
    'src/extension.rs：read_extension_entry 必须是 `pub async fn`。\n' +
      '    同步命令跑在主线程上，而它内联的 service 懒启动会下载内置 Node 运行时 ——\n' +
      '    实测整窗冻结 2.5 分钟（nodejs.org 国内 ~340KB/s × 48.8MB）。\n' +
      '    改成 async 后 Tauri 把它派到异步运行时，主线程不再被占。',
  )
}

/** 规则 ②：service 启动必须整段抛到后台，且不得 await 它。 */
const spawnIdx = extensionRs.indexOf('spawn_blocking(move || {\n            if let Err(e) = crate::service::start_service')
if (spawnIdx === -1) {
  fail(
    'src/extension.rs：service 懒启动必须放进 `spawn_blocking`（整段，不 await）。\n' +
      '    它是「下载运行时 + 起进程 + 探活」的阻塞活；抛出去主线程与异步 worker 都不被占。\n' +
      '    注意契约本来就允许不等：后端没就绪时前端仍能打开，runtime.info 返回 serviceReady=false。',
  )
} else if (/spawn_blocking\([^)]*\)\s*\.await/.test(extensionRs)) {
  fail(
    'src/extension.rs：`spawn_blocking(...).await` 又把后台活拉回当前线程等了 ——\n' +
      '    那就等于没抛出去。请去掉 `.await`。',
  )
}

/**
 * 规则 ③：内置 Node 的下载**不许**用 `reqwest::get`。
 *
 * ⚠️ `reqwest::get` 用默认 client：无连接超时、无读取超时。慢链路能下，
 *   卡住就是永久。口径与约定 35 的下载器同款（连接 15s / 空闲读 30s / 不设总超时）。
 */
if (/reqwest::get\(/.test(runtimeRs)) {
  fail(
    'src-tauri/src/runtime.rs：下载内置运行时不能用 `reqwest::get`。\n' +
      '    它用默认 client，既无连接超时也无读取超时 —— 连接半死就是永久挂起。\n' +
      '    请自建 Client：connect_timeout(15s) + read_timeout(30s)，不设总超时。',
  )
}
if (!/\.connect_timeout\(/.test(runtimeRs)) {
  fail(
    'src-tauri/src/runtime.rs：下载内置运行时的 client 必须设 connect_timeout。\n' +
      '    连不上时要有明确的失败，而不是无限等。',
  )
}
if (!/\.read_timeout\(/.test(runtimeRs)) {
  fail(
    'src-tauri/src/runtime.rs：下载内置运行时的 client 必须设 read_timeout。\n' +
      '    ⚠️ 仍然**不要**设总超时：慢链路传 48MB 要几分钟，设了会误杀正在进行的慢下载。',
  )
}

/**
 * 规则 ④：`ServiceState` 必须有独立的 `starting` 集合。
 *
 * ⚠️ 这是改成后台后**新引入**的洞：幂等检查读 `running`，而端口要等
 *   「解析 node → 起进程」之后才写进去。改成 `spawn_blocking` 后两次调用
 *   真并发，两次都能通过那个检查 → 两个 Node 进程，而后写的覆盖掉前一个的
 *   `child` 句柄，那个进程再也无人回收。
 */
if (!/pub starting\s*:\s*Mutex<HashSet<String>>/.test(serviceRs)) {
  fail(
    'src-tauri/src/service.rs：ServiceState 必须有独立的 `starting` 集合。\n' +
      '    只有 `running` 的话，并发启动会起两个 Node 进程，且前一个的 child 句柄被覆盖、进程成孤儿。',
  )
}

if (failed) {
  console.error(
    `\n[service-startup] ${failed} 条违规。\n` +
      '  真相源是实机事故记录（2026-10-06，装 service 扩展后整窗卡死）。\n' +
      '  改这三处之前请先想一遍「主线程被占住时用户看到什么」。',
  )
  process.exit(1)
}
console.log(
  '  [service-startup] read_extension_entry 已 async ✓ service 启动已 spawn_blocking ✓ ' +
    '下载超时已设 ✓ starting 守卫在位 ✓',
)