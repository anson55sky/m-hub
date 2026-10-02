// 请求分发的唯一实现 —— Workers 入口与 Pages Functions **共用这一份**。
//
// ## 为什么要抽出来
//
// 同一套 17 条路由要跑在两个运行时上：
// · Workers（`src/index.ts`）—— `compatibility_date` 2025-09-05 起，
//   **静态资产优先于 Worker**：匹配到的文件由 CDN 直接应答，Worker 根本不执行。
// · Pages Functions（`functions/`）—— **恰好相反，Functions 优先于静态资产**。
//
// 于是同一个 `/api/v1/market/registry` 在两个运行时上的路径完全不同：
// Workers 上永远进不了 Worker；Pages 上会**先**撞进 Function。
// 若不显式放行，签名清单就会被 Function 抢走 —— 而
// `src/index.ts` 开头那条铁律写着「静态资产绝不能经 Worker 转发或加工」：
// 清单与签名是**签名前写死的字节**，任何重新序列化（gzip 重压缩、JSON 美化、
// 多一个尾随换行）都会让 Ed25519 验签失败。
//
// 这个差异是**静默**的：Workers 上测得好好的，Pages 部署后客户端才开始报
// 「验签失败」，而服务端看起来一切正常。故必须在这里显式处理。

import { Router, fail, type Ctx, type Env } from './lib/http.ts'
import { OPENAI_COMPAT } from './lib/paths.ts'
import { devicePoll, deviceStart } from './routes/github.ts'
import { deviceRevoke, deviceTokens, me, redeem } from './routes/me.ts'
import { marketRegistry, marketRegistrySig } from './routes/market.ts'
import { extensionPackage } from './routes/pkg.ts'
import { chatCompletions, models } from './routes/ai.ts'
import { send, verify } from './routes/email.ts'
import { apply, applyStatus, mySubmissions, submissionDetail, submit, withdraw } from './routes/submissions.ts'

// ⚠️ 管理端**不在** `paths.ts::DYNAMIC` 里，因为它**没有客户端消费者** ——
//    没有管理界面，走的是 CLI（`npm run review`）。把它塞进 DYNAMIC 会让
//    `check-api-spec-conformance.mjs` 要求 `api_spec.rs` 也有对应项，而客户端
//    永远不会调它。故单独列在这里，并在下面的守卫里断言「DYNAMIC 里确实没有 admin」。
import {
  approveDevApplication,
  approveSubmission,
  submissionPackage,
  listDevApplications,
  listSubmissions,
  rejectDevApplication,
  rejectSubmission,
  deleteSubmission,
  markSubmissionPublished,
  adminHealth,
  publishSubmission,
  rebuildMarket,
} from './routes/admin.ts'

export type { Env }

const r = new Router()

// ---- 动态路由（表与 paths.ts::MOUNTED 一一对应） ----
r.add('POST', '/api/v1/auth/github/device/start', (c) => deviceStart(c))
r.add('POST', '/api/v1/auth/github/device/poll', (c) => devicePoll(c))
r.add('POST', '/api/v1/auth/email/send', (c) => send(c))
r.add('POST', '/api/v1/auth/email/verify', (c) => verify(c))
r.add('GET', '/me', (c) => me(c))
r.add('POST', '/api/v1/me/redeem', (c) => redeem(c))
r.add('GET', '/api/v1/me/device-tokens', (c) => deviceTokens(c))
r.add('POST', '/api/v1/me/device-tokens/:id/revoke', (c) => deviceRevoke(c, c.params))
r.add('GET', '/api/v1/ai/models', (c) => models(c))
r.add('POST', '/api/v1/dev/apply', (c) => apply(c))
r.add('GET', '/api/v1/dev/apply', (c) => applyStatus(c))
r.add('POST', '/api/v1/dev/submissions', (c) => submit(c))
r.add('GET', '/api/v1/dev/submissions', (c) => mySubmissions(c))
r.add('GET', '/api/v1/dev/submissions/:id', (c) => submissionDetail(c, c.params))
r.add('POST', '/api/v1/dev/submissions/:id/withdraw', (c) => withdraw(c, c.params))
// 约定 33：平台对话是 OpenAI 兼容面，**不在** api_spec.rs 里（见 paths.ts::OPENAI_COMPAT）
r.add('POST', OPENAI_COMPAT.post_chat_completions, (c) => chatCompletions(c))

// ---- 管理端（门禁 role=admin，见 routes/admin.ts::requireAdmin）----
// 没有客户端消费者，走 `npm run review`。故不在 DYNAMIC 里（见上方 import 的注释）。
r.add('GET', '/api/v1/admin/dev-applications', (c) => listDevApplications(c))
r.add('POST', '/api/v1/admin/dev-applications/:id/approve', (c) => approveDevApplication(c, c.params))
r.add('POST', '/api/v1/admin/dev-applications/:id/reject', (c) => rejectDevApplication(c, c.params))
// 市场清单：运行期签名，字节存 D1（见 routes/market.ts 的文件头）
r.add('GET', '/packages/:extId/:version/:file', (c) => extensionPackage(c, c.params))
r.add('GET', '/api/v1/market/registry', (c) => marketRegistry(c))
r.add('GET', '/api/v1/market/registry.sig', (c) => marketRegistrySig(c))

r.add('POST', '/api/v1/admin/submissions/:id/publish', (c) => publishSubmission(c, c.params))
r.add('POST', '/api/v1/admin/market/rebuild', (c) => rebuildMarket(c))
r.add('GET', '/api/v1/admin/health', (c) => adminHealth(c))
r.add('GET', '/api/v1/admin/submissions', (c) => listSubmissions(c))
r.add('POST', '/api/v1/admin/submissions/:id/approve', (c) => approveSubmission(c, c.params))
r.add('POST', '/api/v1/admin/submissions/:id/reject', (c) => rejectSubmission(c, c.params))
r.add('DELETE', '/api/v1/admin/submissions/:id', (c) => deleteSubmission(c, c.params))
r.add('POST', '/api/v1/admin/submissions/:id/mark-published', (c) =>
  markSubmissionPublished(c, c.params),
)
r.add('GET', '/api/v1/admin/submissions/:id/package', (c) => submissionPackage(c, c.params))

/**
 * 必须由静态资产应答、**不得**经本函数转发的路径。
 *
 * 两个运行时的优先级相反（见文件头），所以只有 Pages 侧需要显式放行；
 * Workers 侧这些请求根本到不了这里（静态资产先应答），留着这个集合无副作用。
 *
 * ⚠️ `.sig` 不单列 —— 它由客户端按 `{url}.sig` 拼接，故按前缀匹配一并覆盖。
 *    但前缀匹配必须**精确到目录**（`/api/v1/market/registry`）而不是
 *    `/api/v1/`：后者会把 15 条动态 API 一起吞掉。
 */
/**
 * ⚠️ `/api/v1/market/registry` **已从本集合移出**（2026-10-02）：
 *   清单改成运行期签名（`lib/marketSign.ts`），由 Function 从 D1 取已签名的字节。
 *   它必须经 Function —— 静态资产那份是**构建期**写的，会与运行期重建的
 *   字节不同步：上架了新版而客户端还在验构建期那份的签名，两边都「正常」
 *   而市场永远停在旧版。
 *
 *   `/api/v1/app/update` 仍在集合里：更新清单要**跟着 DMG 走**，
 *   而 DMG 只在构建机上存在，运行期无从取得。
 */
const STATIC_EXACT = new Set(['/api/v1/app/update'])

/** 该路径是否应由静态资产直接应答（含 `.sig` 与 `/packages/`、`/downloads/`） */
export function is_static_asset(pathname: string): boolean {
  if (STATIC_EXACT.has(pathname)) return true
  if (STATIC_EXACT.has(pathname.replace(/\.sig$/, ''))) return true
  // ⚠️ `/packages/` **不再放行**（2026-10-02）：扩展包改成从 D1 的 blob 直发
  //   （routes/pkg.ts 的文件头说明了为什么不再重新打包）。
  //   `/downloads/`（应用安装包 DMG）仍然静态分发 —— 它只存在于构建机上，
  //   运行期拿不到。
  return pathname.startsWith('/downloads/')
}

export async function handleRequest(req: Request, env: Env): Promise<Response> {
  const url = new URL(req.url)
  const method = req.method.toUpperCase()

  // Pages 侧：Functions 优先于静态资产，故这里必须主动交还，否则签名清单被抢。
  // `env.ASSETS` 是 Pages 提供的静态资产绑定，转发过去拿到的**就是原始字节**
  // （不经本函数序列化），语义与 Workers 上「静态资产先应答」一致。
  if (is_static_asset(url.pathname) && 'ASSETS' in env && env.ASSETS) {
    const res = await env.ASSETS.fetch(req)
    // ASSETS 未命中时返回它自己的 404 页；不要把它伪装成「接口不存在」——
    // 那会让「忘部署清单」看起来像「客户端调错了路径」，故障现场离原因很远。
    if (res.status !== 404) return res
    return fail(404, 'static_asset_missing', `静态资源未部署：${url.pathname}（清单与包由 public/ 决定，跑一次 seed-manifests 再部署）`)
  }

  // body **不在这里**解析：Request 的 body 只能读一次，而 multipart 的读法
  // （formData）与 JSON 不同，各 handler 自己按需读自己的那条路径。
  const ctx: Ctx = {
    req,
    env,
    url,
    params: {},
    token: req.headers.get('authorization')?.replace(/^Bearer\s+/i, '').trim() || undefined,
  }

  const hit = r.resolve(method, url.pathname)
  if (hit.kind === 'not_found') {
    return fail(404, 'not_found', `没有这个接口：${method} ${url.pathname}`)
  }
  if (hit.kind === 'method_not_allowed') {
    // 带 Allow 头：这是「路径对、方法错」的唯一可自诊断信号
    return fail(
      405,
      'method_not_allowed',
      `${url.pathname} 不接受 ${method}（接受：${hit.allow.join(', ')}）`,
      {},
      { allow: hit.allow.join(', ') },
    )
  }

  ctx.params = hit.params
  try {
    return await hit.handler(ctx)
  } catch (e) {
    // 未预期异常：记完整堆栈（Workers 的 console 会进 tail logs），
    // 但**不给客户端回堆栈** —— 那会泄露表名/内部结构
    console.error(`[m-hub] ${method} ${url.pathname} 未处理异常:`, e)
    return fail(500, 'server_error', '服务端内部错误')
  }
}
