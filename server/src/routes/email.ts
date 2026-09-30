// 邮箱验证码登录。
//
// ## 明确取舍：Workers 上没有 SMTP
//
// 约定 53 记着上游用自建 SMTP 发码。**Workers 没有出站 SMTP**（只给
// HTTP/HTTPS），所以这条路不存在。可选替代：
//   · Resend 等 HTTP 邮件 API（有免费额度）—— 本实现走这条
//   · 放弃邮箱登录，只留 GitHub 登录
//
// 倾向后者：m-hub 是个桌面工具，GitHub 一条路已经够用；多一套邮箱就多一个
// 收不到验证码的失败模式（垃圾邮件、SMTP 被限），而客户端界面得为此多出一整块
// （约定 53 那一长串：每小时 5 封、60s 重发冷却、就地常驻反馈……）。
// 所以本文件的定位是**能力齐全但默认关闭**：`RESEND_API_KEY` 没配就明确
// 报「未配置」，不静默失败也不假装成功。
//
// 客户端（`AccountPanel.vue`）目前仍渲染邮箱入口。若不打算启用，在
// `AccountPanel.vue` 里把该分区摘掉（约定 53 提醒过 skip 标记的字面文本
// 不能出现在附近注释里，否则 `gen-settings-index.mjs` 会报「标记不成对」挡住构建）。

import { fail, json, readJson, type Ctx } from '../lib/http'
import { randomId } from '../lib/ids'

/** 验证码。6 位数字，与客户端输入框的 maxlength 对齐。 */
const CODE_TTL_MS = 10 * 60_000
const RESEND_COOLDOWN_MS = 60_000
const MAX_PER_HOUR = 5

const codes = new Map<string, { code: string; at: number; tries: number }>()

/**
 * 每邮箱的发信时刻队列（只留最近一小时）。
 *
 * ⚠️ 这条限额原来只是**声明了常量、没执行** —— 客户端界面写着「同一邮箱每小时
 * 最多 5 封」（约定 53），服务端却只查 60s 冷却，于是第 6 封会一路走到
 * 邮件服务商那里被拒，用户看到的是**服务商的错误**而不是「你发太多了」。
 * 声明了不执行 = 比不声明更坏：文档和界面都在承诺一件服务端没做的事。
 */
const sentAt = new Map<string, number[]>()

function sweep(now: number) {
  for (const [k, v] of codes) if (now - v.at > CODE_TTL_MS) codes.delete(k)
  for (const [k, arr] of sentAt) {
    const kept = arr.filter((t) => now - t < 3600_000)
    if (kept.length) sentAt.set(k, kept)
    else sentAt.delete(k)
  }
}

/** POST /api/v1/auth/email/send —— body `{ email }` */
export async function send(ctx: Ctx) {
  const { env } = ctx
  const body = await readJson(ctx.req)
  const email = String(body.email ?? '').trim().toLowerCase()
  if (!/^[^\s@]+@[^\s@]+\.[^\s@]+$/.test(email)) {
    return fail(400, 'bad_request', '邮箱格式不正确')
  }
  if (!env.RESEND_API_KEY || !env.EMAIL_FROM) {
    return fail(501, 'not_configured', '服务端未启用邮箱登录，请用 GitHub 登录')
  }

  const now = Date.now()
  sweep(now)
  // ⚠️ 冷却/限流的措辞必须带上限（约定 53）：只写「发送过于频繁」，
  // 用户会立刻再点、越点越久，最终锁死自己。
  const inHour = (sentAt.get(email) ?? []).filter((t) => now - t < 3600_000)
  if (inHour.length >= MAX_PER_HOUR) {
    const waitMin = Math.ceil((3600_000 - (now - inHour[0]!)) / 60_000)
    return fail(
      429,
      'rate_limited',
      `同一邮箱每小时最多 ${MAX_PER_HOUR} 封，请 ${waitMin} 分钟后再试。` +
        `（已发 ${inHour.length} 封）`,
    )
  }
  const prev = codes.get(email)
  if (prev && now - prev.at < RESEND_COOLDOWN_MS) {
    const wait = Math.ceil((RESEND_COOLDOWN_MS - (now - prev.at)) / 1000)
    return fail(
      429,
      'rate_limited',
      `发送过于频繁，请 ${wait} 秒后再试（同一邮箱 60 秒只能发一封，每小时最多 ${MAX_PER_HOUR} 封）`,
    )
  }

  const code = String(Math.floor(100000 + Math.random() * 900000))
  codes.set(email, { code, at: now, tries: 0 })

  const res = await fetch('https://api.resend.com/emails', {
    method: 'POST',
    headers: {
      authorization: `Bearer ${env.RESEND_API_KEY}`,
      'content-type': 'application/json',
    },
    body: JSON.stringify({
      from: env.EMAIL_FROM,
      to: [email],
      subject: 'm-hub 登录验证码',
      text: `你的验证码是 ${code}，10 分钟内有效。如果这不是你本人的操作，忽略这封信即可。`,
    }),
  })
  if (!res.ok) {
    // 邮件没发出去就**必须**清掉验证码：留着会让用户输一个永远收不到的码
    codes.delete(email)
    return fail(502, 'email_send_failed', `发信失败（HTTP ${res.status}），请稍后重试`)
  }

  inHour.push(now)
  sentAt.set(email, inHour)

  return json({ ok: true, cooldown_ms: RESEND_COOLDOWN_MS, expires_in: CODE_TTL_MS / 1000 })
}

/** POST /api/v1/auth/email/verify —— body `{ email, code }` */
export async function verify(ctx: Ctx) {
  const env = ctx.env
  const body = await readJson(ctx.req)
  const email = String(body.email ?? '').trim().toLowerCase()
  const code = String(body.code ?? '').trim()
  if (!email || !code) return fail(400, 'bad_request', '缺少邮箱或验证码')

  const entry = codes.get(email)
  if (!entry) return fail(400, 'code_invalid', '验证码无效或已过期')
  if (Date.now() - entry.at > CODE_TTL_MS) {
    codes.delete(email)
    return fail(400, 'code_expired', '验证码已过期，请重新获取')
  }
  // 限尝试次数：否则可以无限猜
  entry.tries += 1
  if (entry.tries > 5) {
    codes.delete(email)
    return fail(429, 'too_many_tries', '尝试次数过多，请重新获取验证码')
  }
  if (entry.code !== code) return fail(400, 'code_invalid', '验证码不正确')

  codes.delete(email) // 一次性

  const githubId = null
  const username = email.split('@')[0]!
  await env.DB.prepare(
    `INSERT INTO users (github_id, username, email) VALUES (?1, ?2, ?3)
     ON CONFLICT(email) DO UPDATE SET username = excluded.username`,
  )
    .bind(githubId, username, email)
    .run()

  const user = await env.DB.prepare(
    `SELECT id, github_id, username, email, avatar, role, is_developer, developer_status, invite_redeemed
       FROM users WHERE email = ?1`,
  )
    .bind(email)
    .first<import('../lib/http').User>()
  if (!user) return fail(500, 'server_error', '账号创建失败')

  const token = `mhub_${randomId().replace(/-/g, '')}${randomId().replace(/-/g, '')}`
  await env.DB.prepare(
    `INSERT INTO sessions (token, user_id, device, expires_at) VALUES (?1, ?2, ?3, ?4)`,
  )
    .bind(token, user.id, '邮箱登录', Date.now() + 30 * 24 * 3600_000)
    .run()

  return json({ status: 'ok', token, user: {
    username: user.username, email: user.email, avatar: user.avatar,
    role: user.role, is_developer: !!user.is_developer,
    developer_status: user.developer_status, invite_redeemed: !!user.invite_redeemed,
  } })
}
