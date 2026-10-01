// Pages Functions 入口：`/api/v1/*` 全部 14 条动态路由。
//
// ## 为什么必须有这个文件，而不能靠静态资产兜底
//
// Cloudflare Pages 与 Workers 的**路由优先级相反**：
// · Workers：静态资产优先 —— 匹配到文件就由 CDN 应答，Worker 根本不执行
// · Pages  ：**Functions 优先** —— 有匹配的 Function 就先执行它
//
// 而 4 个签名清单恰好就在 `/api/v1/` 下（`/api/v1/market/registry`、
// `/api/v1/app/update` 及其 `.sig`）。没有这个文件时它们会被静态资产正确应答；
// 有了这个 catch-all 之后它们会**先**撞进 Function —— 所以
// `handle.ts::is_static_asset` 那边必须显式把它们交还 `env.ASSETS`。
//
// ⚠️ 这个差异是**静默**的：Workers 上全绿，Pages 部署后客户端才开始报
// 「验签失败」，而服务端看起来一切正常。别删这个显式放行。
//
// `[[path]]` 是 Pages 的 catch-all 语法（双方括号 = 匹配任意多段）。

import { handleRequest, type Env } from '../../../src/handle.ts'

export const onRequest = (ctx: { request: Request; env: Env }): Promise<Response> =>
  handleRequest(ctx.request, ctx.env)
