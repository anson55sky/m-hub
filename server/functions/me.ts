// Pages Functions 入口：根路径 `/me`（账号状态）。
//
// ⚠️ 它在**根路径**而不是 `/api/v1/` 下 —— 这是 `api_spec.rs` 与
// `paths.ts` 刻意保留的历史形状（见 `check-api-spec-conformance.mjs` 的具名断言）。
// 直觉上会想「修正」成 `/api/v1/me」，那会立刻 404，**别改**。

import { handleRequest, type Env } from '../src/handle.ts'

export const onRequest = (ctx: { request: Request; env: Env }): Promise<Response> =>
  handleRequest(ctx.request, ctx.env)
