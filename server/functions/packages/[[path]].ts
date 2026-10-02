// Pages Functions 入口：`/packages/*` —— 扩展包体。
//
// ## 为什么必须有这个文件
//
// Pages 的路由是**按目录结构**声明的，没有全局 catch-all：
// 只有 `functions/api/v1/`、`functions/me.ts`、`functions/v1/` 这几个入口。
// 于是 `/packages/<ext>/<version>/<file>` **不匹配任何 Function**，
// 请求直接由静态资产应答 —— 也就是 `public/packages/` 下那份**重新打包**的文件。
//
// 这个失败是**静默**的：`handle.ts` 里那条 `r.add('GET', '/packages/…')`
// 在 Workers 上能跑，在 Pages 上**永远不会被调用**（该路径压根没进 Function）。
// 症状是「包能下载、HTTP 200、大小也对不上」—— 客户端校验 sha256 时才失败，
// 而清单与签名全都合法、服务端日志一片正常。
//
// ⚠️ 加任何 `handle.ts` 里的新路径到 Pages 时都要问一句：
//   **这个前缀有对应的 functions/ 目录吗？** 少了就是一条死路由。
//   本文件的存在正是为了让「扩展包从 D1 直发」这件事在 Pages 上真的发生。

import { handleRequest, type Env } from '../../src/handle.ts'

export const onRequest = (ctx: { request: Request; env: Env }): Promise<Response> =>
  handleRequest(ctx.request, ctx.env)