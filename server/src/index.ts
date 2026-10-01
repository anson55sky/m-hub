// Worker 入口（Cloudflare Workers）。
//
// ⚠️ **Pages Functions 的入口不在这里** —— 见 `functions/`。
// 两者共用 `src/handle.ts` 的路由表，不重复实现。
//
// 两条路径刻意分成两类：
//
// **静态**（`public/` 下的文件，由 Workers Static Assets 直接应答主域）
//   `/api/v1/market/registry{,.sig}`、`/api/v1/app/update{,.sig}`
//   —— 走 CDN 边缘，不过 Worker、不计 CPU，且**保证客户端验的原始字节与发布时
//   签的字节逐字节一致**。这条一致性是签名机制的全部意义：一旦中间有任何人
//   重新序列化（gzip 重压缩、JSON 美化、尾随换行），验签就过不了。
//   故静态资产**绝不能**经 Worker 转发或加工。
//
//   ⚠️ Workers 与 Pages 的**优先级相反**：Workers 是静态资产优先（匹配到就
//   根本不执行 Worker），Pages 是 Functions 优先。同一批路径在 Pages 上会被
//   Function 抢走，故 `handle.ts::is_static_asset` 那边必须显式放行。
//
// **动态**（`src/handle.ts`）
//   登录 / 账号 / 开发者 / 发布 / 平台 AI
//
// 静态那两条**不在**路由表里 —— 它们不是「注册」出来的，
// 而是构建期由 `public/` 目录决定的。`check-api-spec-conformance.mjs`
// 会断言 `public/` 下确实存在这四个文件，防止「以为部署了其实没有」。

import { handleRequest, type Env } from './handle.ts'

export type { Env }

export default {
  fetch: handleRequest,
} satisfies ExportedHandler<Env>
