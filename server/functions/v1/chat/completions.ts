// Pages Functions 入口：OpenAI 兼容面 `POST /v1/chat/completions`。
//
// ⚠️ 同样在**根路径**下（`/v1/` 而非 `/api/v1/`），且**只实现这一个端点**：
// `GET /v1/models` 等一律 404 —— 这是实测确认过的（见约定 33 的两条坑）。
// 故这里必须是精确文件而非 catch-all：若写成 `functions/v1/[[path]].ts`，
// 那些 404 会被泛化成别的形态，掩盖「这个端点不存在」这个自诊断信号。

import { handleRequest, type Env } from '../../../src/handle.ts'

export const onRequest = (ctx: { request: Request; env: Env }): Promise<Response> =>
  handleRequest(ctx.request, ctx.env)
