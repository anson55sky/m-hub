// 账号与权益：/me、兑换邀请码、设备列表。
//
// 响应字段名由客户端 `account.rs` 的解析代码决定，**不能随手改**：
//   username / email / avatar / role / is_developer / developer_status /
//   invite_redeemed / application / can_apply_developer
// 见 client `account.rs::parse_me` 一带的 `.get(...)`。

import {
  currentUser,
  fail,
  json,
  readJson,
  unauthorized,
  type Ctx,
} from '../lib/http'

/** GET /me —— 注意是**根路径**，不是 /api/v1/me（约定 52 记了这个坑）。 */
export async function me(ctx: Ctx) {
  const user = await currentUser(ctx.env, ctx.token)
  if (!user) return unauthorized()
  return json(publicUser(user))
}

/**
 * 对外的用户形状。
 *
 * ⚠️ 刻意**不**回 `id` 与 `sessions`：客户端不需要，泄露了等于给攻击者
 * 「枚举用户」和「看懂会话表结构」两样没用的东西。
 * 客户端 `AccountPanel.vue` 只读 username / email / avatar。
 */
function publicUser(u: import('../lib/http').User) {
  return {
    username: u.username,
    email: u.email,
    avatar: u.avatar,
    role: u.role,
    is_developer: !!u.is_developer,
    developer_status: u.developer_status,
    invite_redeemed: !!u.invite_redeemed,
  }
}

/** POST /api/v1/me/redeem —— body `{ code }`（蛇形，约定 47） */
export async function redeem(ctx: Ctx) {
  const user = await currentUser(ctx.env, ctx.token)
  if (!user) return unauthorized()
  const body = await readJson(ctx.req)

  const code = typeof body.code === 'string' ? body.code.trim() : ''
  if (!code) return fail(400, 'bad_request', '缺少兑换码')

  // 未配置时**明确报未配置**，不静默通过 —— 静默通过会让所有人以为兑换成功
  if (!ctx.env.INVITE_CODE) {
    return fail(500, 'server_misconfigured', '服务端未配置邀请码')
  }
  if (code !== ctx.env.INVITE_CODE) {
    return fail(400, 'invalid_code', '兑换码无效')
  }
  if (user.invite_redeemed) {
    return fail(409, 'already_redeemed', '该账号已兑换过')
  }

  await ctx.env.DB.prepare(`UPDATE users SET invite_redeemed = 1 WHERE id = ?1`)
    .bind(user.id)
    .run()

  return json({ ok: true, invite_redeemed: true })
}

/** GET /api/v1/me/device-tokens */
export async function deviceTokens(ctx: Ctx) {
  const user = await currentUser(ctx.env, ctx.token)
  if (!user) return unauthorized()
  const rows = await ctx.env.DB.prepare(
    `SELECT token, device, created_at AS createdAt, expires_at AS expiresAt
       FROM sessions WHERE user_id = ?1 ORDER BY created_at DESC`,
  )
    .bind(user.id)
    .all<{ token: string; device: string | null; createdAt: number; expiresAt: number }>()

  const now = Date.now()
  return json({
    items: (rows.results ?? []).map((r) => ({
      // token 只给前 8 位：够用户认出是哪台设备，又不足以当凭证用
      id: r.token.slice(0, 8),
      device: r.device,
      created_at: r.createdAt,
      expires_at: r.expiresAt,
      expired: r.expiresAt <= now,
      current: r.token === ctx.token,
    })),
  })
}

/** POST /api/v1/me/device-tokens/:id/revoke */
export async function deviceRevoke(ctx: Ctx, params: Record<string, string>) {
  const user = await currentUser(ctx.env, ctx.token)
  if (!user) return unauthorized()

  // 客户端只传前 8 位，故按前缀匹配。**必须同时限定 user_id**，
  // 否则改个 id 就能撤别人的会话。
  const r = await ctx.env.DB.prepare(
    `DELETE FROM sessions WHERE user_id = ?1 AND token LIKE ?2`,
  )
    .bind(user.id, `${params.id}%`)
    .run()

  if (!r.meta.changes) return fail(404, 'not_found', '找不到该设备')
  return json({ ok: true })
}
