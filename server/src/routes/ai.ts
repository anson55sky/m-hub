// 平台 AI 额度：模型清单 + OpenAI 兼容的流式代理。
//
// ## 两条来自客户端的硬约束（约定 33，都是实测踩出来的）
//
// ① **平台只实现 `POST /v1/chat/completions`，没有 `GET /v1/models`。**
//    上游实测 `/v1/models`、`/v1/models/`、`/models`、`/v1/` 全部 404
//    `{"error":"not_found"}`，而 `POST /v1/chat/completions` 不带凭据回 401
//    —— 即对话链路本身是好的，404 只说明列表端点不存在。
//    客户端因此走 `loadPlatformModels()`（`/api/v1/ai/models`）拿清单。
//    **照 OpenAI 惯例补一个 `GET /v1/models` 是错的方向**：客户端的通用探测
//    拿不到 Key（平台条目钥匙串里只有占位符），补了也会被那条路径判失败。
//
// ② 地址**一律**指向内置服务端（`chat.rs::platform_base_url`），
//    不读用户可改的配置。

import {
  currentUser,
  fail,
  json,
  unauthorized,
  type Ctx,
} from '../lib/http.ts'
import { OPENAI_COMPAT } from '../lib/paths.ts'

/** 平台侧真模型的上游地址。做成 secret，客户端永远看不到。 */
const UPSTREAM_BASE = 'https://api.openai.com'
const DAILY_GRANT = 20

/** GET /api/v1/ai/models —— 「使用平台免费额度」开关拉的就是这份清单。 */
export async function models(ctx: Ctx) {
  const user = await currentUser(ctx.env, ctx.token)
  if (!user) return unauthorized()
  if (!user.invite_redeemed) {
    return fail(403, 'invite_required', '需先兑换邀请码才能使用平台额度')
  }
  return json({
    models: [
      { id: 'platform:gpt-4o-mini', name: 'm-hub 平台 · gpt-4o-mini' },
      { id: 'platform:gpt-4o', name: 'm-hub 平台 · gpt-4o' },
    ],
    quota: await todayQuota(ctx, user.id),
  })
}

/** POST /v1/chat/completions —— OpenAI 兼容，SSE 流式透传。 */
export async function chatCompletions(ctx: Ctx) {
  const user = await currentUser(ctx.env, ctx.token)
  if (!user) return unauthorized()
  if (!user.invite_redeemed) {
    return fail(403, 'invite_required', '需先兑换邀请码才能使用平台额度')
  }

  const remaining = await consumeQuota(ctx, user.id)
  if (remaining <= 0) {
    return fail(429, 'quota_exhausted', '今日平台额度已用完，明天再来')
  }

  // 客户端用 `bearer_auth` 发平台 Key（约定 33：钥匙串里存的是占位符，
  // 真凭据是**账号登录态**，由服务端在请求时现取）。故这里的 Authorization
  // 就是会话 token，上面已验过。
  const body = await ctx.req.text()
  const upstream = await fetch(`${UPSTREAM_BASE}/v1/chat/completions`, {
    method: 'POST',
    headers: {
      'content-type': 'application/json',
      authorization: `Bearer ${ctx.env.OPENAI_API_KEY ?? ''}`,
    },
    body,
  })

  if (!upstream.ok) {
    const text = await upstream.text().catch(() => '')
    // 上游原文可能含内部信息，只回状态码 + 截断文本
    return fail(upstream.status === 429 ? 429 : 502, 'upstream_error',
      `上游返回 ${upstream.status}：${text.slice(0, 200)}`)
  }

  // ⚠️ 流式必须**透传 body 且不缓冲**：客户端按 SSE 增量渲染，
  // 任何 `await resp.text()` 都会把流吃干、表现为「一直转圈然后一次性蹦出全文」。
  return new Response(upstream.body, {
    status: 200,
    headers: {
      'content-type': upstream.headers.get('content-type') ?? 'text/event-stream',
      'cache-control': 'no-store',
      'x-accel-buffering': 'no', // 反代/浏览器侧的缓冲关掉
    },
  })
}

/** 取今日额度（不消耗）。跨 UTC 日切。 */
async function todayQuota(ctx: Ctx, userId: number) {
  const day = new Date().toISOString().slice(0, 10)
  const r = await ctx.env.DB.prepare(
    `SELECT granted, used FROM ai_quota WHERE user_id = ?1 AND day = ?2`,
  )
    .bind(userId, day)
    .first<{ granted: number; used: number }>()

  // 按需发放：不在日切时批量写行，读时补零即可（省一次定时任务）
  const granted = r?.granted ?? DAILY_GRANT
  const used = r?.used ?? 0
  if (!r) {
    await ctx.env.DB.prepare(
      `INSERT OR IGNORE INTO ai_quota (user_id, day, granted, used) VALUES (?1, ?2, ?3, 0)`,
    )
      .bind(userId, day, DAILY_GRANT)
      .run()
  }
  return { granted, used, remaining: Math.max(0, granted - used), day }
}

/** 消耗一次额度，返回剩余。 */
async function consumeQuota(ctx: Ctx, userId: number): Promise<number> {
  const day = new Date().toISOString().slice(0, 10)
  await ctx.env.DB.prepare(
    `INSERT OR IGNORE INTO ai_quota (user_id, day, granted, used) VALUES (?1, ?2, ?3, 0)`,
  )
    .bind(userId, day, DAILY_GRANT)
    .run()

  // 单条 UPDATE 里做条件递增：省掉「先读后写」的竞态（并发下会超发）
  const r = await ctx.env.DB.prepare(
    `UPDATE ai_quota SET used = used + 1
      WHERE user_id = ?1 AND day = ?2 AND used < granted
      RETURNING granted - used AS remaining`,
  )
    .bind(userId, day)
    .first<{ remaining: number }>()

  return r?.remaining ?? 0
}

export { OPENAI_COMPAT }
