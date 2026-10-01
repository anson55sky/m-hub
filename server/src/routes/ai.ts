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

/**
 * GET /api/v1/ai/models —— 「使用平台免费额度」开关拉的就是这份清单。
 *
 * ## ⚠️ 上游未配置时**必须明确说不可用**（2026-10-01 改）
 *
 * 原实现在 `OPENAI_API_KEY` 没配时**照样**返回
 * `{ models: [gpt-4o, gpt-4o-mini], quota: { granted: 20 } }`。
 * 后果是界面显示「每日 20 次」，用户开开关、选模型、发消息 —— 然后失败。
 * **显示能用却不能用，比明确禁用糟得多**：用户会反复重试，
 * 而且报出来的错（上游 401）完全指不到真正原因（服务端没配 Key）。
 *
 * 现在上游缺失时 `available: false` + 空清单 + `reason`，
 * 客户端据此把开关置灰并说明该用自备供应商。
 *
 * 顺带把模型名**从配置读**而不是写死 —— 写死的清单在换模型时要改代码重新部署，
 * 而换模型本该是件小事。
 */
export async function models(ctx: Ctx) {
  const user = await currentUser(ctx.env, ctx.token)
  if (!user) return unauthorized()
  if (!user.invite_redeemed) {
    return fail(403, 'invite_required', '需先兑换邀请码才能使用平台额度')
  }

  // ⚠️ 顺序很重要：**先**判上游，再谈额度。
  // 反过来（先给额度）就又回到了「显示 20 次然后失败」。
  if (!ctx.env.OPENAI_API_KEY) {
    return json({
      available: false,
      reason: '平台 AI 未配置上游 Key，无法使用。你的费用不该由平台承担 —— 请改用自备供应商。',
      models: [],
      quota: { granted: 0, used: 0, remaining: 0, day: '' },
    })
  }

  return json({
    available: true,
    models: platformModelIds().map((id) => ({ id: `platform:${id}`, name: `m-hub 平台 · ${id}` })),
    quota: await todayQuota(ctx, user.id),
  })
}

/**
 * 平台侧可选的模型。
 *
 * 之前是写死的两个 OpenAI 模型名。**但 `api.openai.com` 从国内实测连不上**
 * （curl 直接 000），所以写死它等于给一个用不了的入口 —— 这也是本轮
 * 决定走「自备 Key」的现实原因之一。故改为可配置，留空即不可用。
 */
function platformModelIds(): string[] {
  return (process.env.MHUB_PLATFORM_MODELS || '')
    .split(',')
    .map((s) => s.trim())
    .filter(Boolean)
}

/** POST /v1/chat/completions —— OpenAI 兼容，SSE 流式透传。 */
export async function chatCompletions(ctx: Ctx) {
  const user = await currentUser(ctx.env, ctx.token)
  if (!user) return unauthorized()
  if (!user.invite_redeemed) {
    return fail(403, 'invite_required', '需先兑换邀请码才能使用平台额度')
  }

  // ⚠️ 与 `models()` 同一个前置，且**必须在扣额度之前** ——
  //   否则会白扣一次额度然后失败，用户看着「用了 1 次却没得到回复」。
  if (!ctx.env.OPENAI_API_KEY) {
    return fail(
      503,
      'platform_unavailable',
      '平台 AI 未配置上游 Key，无法使用。请改用自备供应商。',
    )
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
      // ⚠️ `ON CONFLICT DO NOTHING` 是 PostgreSQL 版的「INSERT OR IGNORE」。
      // 数据层会把 `INSERT OR IGNORE` 改写成 `INSERT`，但**补不出 ON CONFLICT**
      // （那需要知道冲突目标），所以子句必须显式写在这里。
      // 没有它的话 PG 会直接语法报错 —— 属「应用自身问题」，看日志尾即可定位。
      `INSERT INTO ai_quota (user_id, day, granted, used) VALUES (?1, ?2, ?3, 0)
       ON CONFLICT (user_id, day) DO NOTHING`,
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
    // 同上：ON CONFLICT 子句必须显式写（PG 无「OR IGNORE」等价物）
    `INSERT INTO ai_quota (user_id, day, granted, used) VALUES (?1, ?2, ?3, 0)
     ON CONFLICT (user_id, day) DO NOTHING`,
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
