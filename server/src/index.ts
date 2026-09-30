// Worker 入口。
//
// ## 两条路径刻意分成两类
//
// **静态**（`public/` 下的文件，由 Workers Static Assets 直接应答主域）
//   `/api/v1/market/registry{,.sig}`、`/api/v1/app/update{,.sig}`
//   —— 走 CDN 边缘，不过 Worker、不计 CPU，且**保证客户端验的原始字节与发布时
//   签的字节逐字节一致**。这条一致性是签名机制的全部意义：一旦中间有任何人
//   重新序列化（gzip 重压缩、JSON 美化、尾随换行），验签就过不了。
//   故静态资产**绝不能**经 Worker 转发或加工。
//
// **动态**（本文件）
//   登录 / 账号 / 开发者 / 发布 / 平台 AI
//
// 静态那两条**不在** `MOUNTED` 表里 —— 它们不是「注册」出来的，
// 而是构建期由 `public/` 目录决定的。`check-api-spec-conformance.mjs`
// 会断言 `public/` 下确实存在这四个文件，防止「以为部署了其实没有」。

import { Router, fail, type Ctx, type Env } from './lib/http'
import { OPENAI_COMPAT } from './lib/paths'
import { devicePoll, deviceStart } from './routes/github'
import { deviceRevoke, deviceTokens, me, redeem } from './routes/me'
import { chatCompletions, models } from './routes/ai'
import { send, verify } from './routes/email'
import { apply, applyStatus, mySubmissions, submissionDetail, submit, withdraw } from './routes/submissions'

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

export default {
  async fetch(req: Request, env: Env): Promise<Response> {
    const url = new URL(req.url)
    const method = req.method.toUpperCase()

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
  },
} satisfies ExportedHandler<Env>
