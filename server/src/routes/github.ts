// GitHub 设备码登录（服务端代调）。
//
// ## 为什么 Workers 上不需要代理
//
// 约定 53 记着：国内服务器上 `github.com` 与 `api.github.com` 都会间歇性失败，
// 所以服务端要挂 `GITHUB_PROXY`（且**只设 `HTTPS_PROXY` 对 Node 原生 fetch 无效**，
// 须由代码挂 ProxyAgent）。
//
// **Workers 不在这个坑里**：它跑在全球 300+ 城市、出口就在 GitHub 旁边，
// 裸 fetch 即可。这是选 Workers 相对「国内轻量服务器」的一个实打实的好处。
//
// ## poll_id 是我们自己的，不透传 GitHub 的 device_code
//
// GitHub 的 `device_code` 是可以直接拿来换 token 的凭证。透传出去等于把
// 「能登录这个账号的东西」发给客户端。所以服务端自签一个 `poll_id`，
// `device_code` 只留在 Worker 内存里。
//
// ⚠️ 内存 Map 的代价要说清：**Worker 实例随时可能被回收**（free plan 尤其频繁），
// 于是「刚拿到 user_code 就被回收」→ 轮询必然失败。
// 这是骨架的已知取舍，不是 bug —— 单次登录窗口只有几分钟，回收概率低到可接受；
// 但真要提高成功率，得把 `poll_id → device_code` 落到 D1 或 KV。见 README「已知取舍」。

import { fail, json, readJson, type Ctx, type Env } from '../lib/http.ts'
import { randomId } from '../lib/ids.ts'

const GH_OAUTH = 'https://github.com'
const GH_API = 'https://api.github.com'

/**
 * GitHub 设备码流程两个端点的公共请求头。
 *
 * ⚠️ `Accept: application/json` **不是可选的**（GitHub 文档明确要求）：
 * 缺它时端点返回 HTML / 纯文本（实测：裸串 `Not Found`），而调用方紧跟着
 * `res.json()` —— 于是抛出的是一个与真实原因无关的异常，故障现场离原因很远。
 *
 * 两处各写一遍的话迟早只改一处，故收成常量。
 */
const GH_FORM_HEADERS = {
  'content-type': 'application/x-www-form-urlencoded',
  Accept: 'application/json',
} as const

/**
 * poll_id → GitHub device_code 的存放处。
 *
 * ## ⚠️ 这里原来是一个进程内存的 `Map`（2026-10-01 改掉）
 *
 * 原注释写「单次登录窗口只有几分钟，回收概率低到可接受」—— 那是按**常驻进程**
 * 算的。迁到 Cloudflare Pages Functions 后前提不成立：实例短命、多个 isolate
 * 互不共享内存、scale-to-zero 后必然冷启动。于是「换验证码」与「轮询」两次请求
 * 大概率落在**不同实例**，轮询必然 410 `poll_gone`。
 *
 * 实测证据（改之前）：连发三次 device/start 全部 200，且每次都是**新的**
 * user_code —— 说明没有任何状态被复用；紧接着 poll 就 410。
 *
 * 教训：**「进程内状态」这个前提是随部署目标一起变的**，而它不体现在任何
 * 类型或签名里。迁到任何无状态运行时前，先 grep 一遍 `new Map(` / 模块级
 * 可变量 —— 那都是隐形的单实例假设。
 *
 * 落 D1 的代价是每次轮询多一次查询；换来的是任意实例都能接手。
 */
const POLL_TTL_MS = 15 * 60_000

/** 读一条待轮询记录；已过期顺手删掉（一次性消费，不留垃圾）。 */
async function takePoll(ctx: Ctx, pollId: string) {
  const row = await ctx.env.DB.prepare(
    `SELECT device_code, interval, expires_at FROM device_polls WHERE poll_id = ?1`,
  )
    .bind(pollId)
    .first<{ device_code: string; interval: number; expires_at: number }>()
  if (!row) return null
  if (row.expires_at < Date.now()) {
    await ctx.env.DB.prepare(`DELETE FROM device_polls WHERE poll_id = ?1`).bind(pollId).run()
    return null
  }
  return row
}

/** 删掉一条待轮询记录（授权成功、或这一轮不可能再成功时）。 */
function dropPoll(ctx: Ctx, pollId: string) {
  return ctx.env.DB.prepare(`DELETE FROM device_polls WHERE poll_id = ?1`).bind(pollId).run()
}

/** GitHub 出网失败与「用户还没授权」是两回事，必须分开——前者是服务端问题。 */
async function ghFetch(url: string, init: RequestInit = {}): Promise<Response> {
  // GitHub 要求 User-Agent；不带会直接 403
  const headers = new Headers(init.headers)
  headers.set('user-agent', 'm-hub-server')
  headers.set('accept', 'application/json')
  try {
    return await fetch(url, { ...init, headers })
  } catch (e) {
    throw new GitHubUnreachable(String(e))
  }
}

class GitHubUnreachable extends Error {}

const unreachable = (e: string) =>
  fail(
    502,
    'github_unavailable',
    `服务端连接 GitHub 失败：${e}。这与你的网络无关，请稍后重试。`,
  )

/** POST /api/v1/auth/github/device/start */
export async function deviceStart(ctx: Ctx) {
  const env = ctx.env
  if (!env.GITHUB_CLIENT_ID) {
    return fail(500, 'server_misconfigured', '服务端未配置 GITHUB_CLIENT_ID')
  }
  const now = Date.now()
  // 顺手清一次过期记录。原实现是遍历内存 Map（sweep），迁到 D1 后**不再有**
  // 「遍历全部待处理项」这个能力 —— 故改成按 expires_at 索引定向删，
  // 避免靠一条无界 DELETE 扫全表。
  await env.DB.prepare(`DELETE FROM device_polls WHERE expires_at < ?1`).bind(now).run()

  let res: Response
  try {
    res = await ghFetch(`${GH_OAUTH}/login/device/code`, {
      method: 'POST',
      // ⚠️ `Accept: application/json` **必须**带。
      //   缺它时 GitHub 的设备码端点返回的是 HTML / 纯文本（实测「Not Found」裸串），
      //   而下面 `res.json()` 会直接抛 —— 抛出的又是别的错误，故障现场离原因很远。
      headers: GH_FORM_HEADERS,
      body: new URLSearchParams({
        client_id: env.GITHUB_CLIENT_ID,
        // 只读身份。**不要**加 repo / workflow 等任何写权限：用户看到的是这个 scope。
        scope: 'read:user user:email',
      }),
    })
  } catch (e) {
    return unreachable(String(e))
  }
  if (!res.ok) {
    return fail(502, 'github_unavailable', `GitHub 拒绝签发设备码（HTTP ${res.status}）`)
  }
  const d = (await res.json()) as {
    device_code: string
    user_code: string
    verification_uri: string
    interval?: number
    expires_in?: number
  }

  const pollId = randomId()
  // GitHub 的 expires_in 是权威值，但用本地常量兜一个上限：
  // 异常大的值会让过期记录长期留在表里。
  const expiresAt = now + Math.min(d.expires_in ?? 900, POLL_TTL_MS / 1000) * 1000
  await env.DB.prepare(
    `INSERT INTO device_polls (poll_id, device_code, interval, created_at, expires_at)
     VALUES (?1, ?2, ?3, ?4, ?5)`,
  )
    .bind(pollId, d.device_code, d.interval ?? 5, now, expiresAt)
    .run()
  return json({
    poll_id: pollId,
    user_code: d.user_code,
    verification_uri: d.verification_uri,
    interval: d.interval ?? 5,
    expires_in: d.expires_in ?? 900,
  })
}

/** POST /api/v1/auth/github/device/poll —— body `{ poll_id }`（蛇形，约定 47） */
export async function devicePoll(ctx: Ctx) {
  const env = ctx.env
  const body = await readJson(ctx.req)
  const pollId = typeof body.poll_id === 'string' ? body.poll_id : ''
  if (!pollId) return fail(400, 'bad_request', '缺少 poll_id')

  const entry = await takePoll(ctx, pollId)
  if (!entry) {
    // 查不到有三种可能，都已无法区分（也不必区分）：过期、已被消费过、
    // 或记录不存在。措辞要说清「重新发起」，否则用户会反复点轮询按钮。
    return fail(
      410,
      'poll_gone',
      '登录会话已失效（等待过久），请重新发起登录。',
    )
  }

  let res: Response
  try {
    res = await ghFetch(`${GH_OAUTH}/login/oauth/access_token`, {
      method: 'POST',
      // ⚠️ `Accept: application/json` **必须**带。
      //   缺它时 GitHub 的设备码端点返回的是 HTML / 纯文本（实测「Not Found」裸串），
      //   而下面 `res.json()` 会直接抛 —— 抛出的又是别的错误，故障现场离原因很远。
      headers: GH_FORM_HEADERS,
      body: new URLSearchParams({
        client_id: env.GITHUB_CLIENT_ID ?? '',
        device_code: entry.device_code,
        grant_type: 'urn:ietf:params:oauth:grant-type:device_code',
      }),
    })
  } catch (e) {
    return unreachable(String(e))
  }

  const d = (await res.json()) as {
    error?: string
    access_token?: string
    interval?: number
  }

  // 用户还没在浏览器里点「授权」——这是**正常状态**，不是错误。
  // slow_down 要回传给客户端调整轮询间隔，别让用户以固定频率猛点。
  if (d.error) {
    if (d.error === 'authorization_pending') {
      return json({ status: 'pending', interval: entry.interval })
    }
    if (d.error === 'slow_down') {
      // 退避要**落库**：跨实例时内存里的累加会丢，而丢了就等于客户端
      // 回到固定频率猛点 GitHub（那正是 GitHub 回 slow_down 的原因）。
      const next = entry.interval + 5
      await ctx.env.DB.prepare(`UPDATE device_polls SET interval = ?1 WHERE poll_id = ?2`)
        .bind(next, pollId)
        .run()
      return json({ status: 'pending', interval: next })
    }
    // 过期 / 被拒 / 码无效：这一轮 login 不可能再成功，删掉免得占着
    await dropPoll(ctx, pollId)
    return fail(400, 'github_denied', githubErrorText(d.error))
  }
  if (!d.access_token) {
    return fail(502, 'github_unavailable', 'GitHub 未返回 access_token')
  }

  // 授权成功 → 删 poll（一次性）
  await dropPoll(ctx, pollId)

  let ghUser: GitHubUser
  try {
    const r = await ghFetch(`${GH_API}/user`, {
      headers: { authorization: `Bearer ${d.access_token}` },
    })
    if (!r.ok) {
      return fail(502, 'github_unavailable', `读取 GitHub 用户信息失败（HTTP ${r.status}）`)
    }
    ghUser = (await r.json()) as GitHubUser
  } catch (e) {
    return unreachable(String(e))
  }

  const session = await upsertUserAndSession(env, ghUser, ctx.req.headers.get('user-agent'))
  return json({ status: 'ok', token: session.token, user: session.user })
}

interface GitHubUser {
  id: number
  login: string
  name: string | null
  email: string | null
  avatar_url: string | null
}

function githubErrorText(code: string): string {
  switch (code) {
    case 'expired_token':
    case 'code_expired':
      return '设备码已过期，请重新发起登录。'
    case 'access_denied':
      return '你已拒绝授权。'
    case 'incorrect_device_code':
      return '设备码无效，请重新发起登录。'
    default:
      // ⚠️ 不把 GitHub 原文直接回给用户：它可能含 redirect_uri / client_id 等
      // 本不该出现在客户端界面的内容
      return `GitHub 拒绝了本次授权（${code}），请重新发起登录。`
  }
}

async function upsertUserAndSession(env: Env, gh: GitHubUser, ua: string | null) {
  const githubId = String(gh.id)
  // 幂等：同一 GitHub 账号重复登录只更新资料，不新建用户
  await env.DB.prepare(
    `INSERT INTO users (github_id, username, email, avatar)
     VALUES (?1, ?2, ?3, ?4)
     ON CONFLICT(github_id) DO UPDATE SET
       username = excluded.username,
       email    = COALESCE(excluded.email, users.email),
       avatar   = excluded.avatar`,
  )
    .bind(githubId, gh.login, gh.email, gh.avatar_url)
    .run()

  const user = await env.DB.prepare(
    `SELECT id, github_id, username, email, avatar, role, is_developer, developer_status, invite_redeemed
       FROM users WHERE github_id = ?1`,
  )
    .bind(githubId)
    .first<import('../lib/http').User>()

  if (!user) throw new Error('用户写入后读不回来')

  // 会话 30 天。过期在查询时判定（见 http.ts::currentUser），故不需要清理任务。
  const token = `mhub_${randomId().replace(/-/g, '')}${randomId().replace(/-/g, '')}`
  const expiresAt = Date.now() + 30 * 24 * 3600_000
  await env.DB.prepare(
    `INSERT INTO sessions (token, user_id, device, expires_at) VALUES (?1, ?2, ?3, ?4)`,
  )
    .bind(token, user.id, deviceLabel(ua), expiresAt)
    .run()

  return { token, user }
}

/** 从 UA 粗略取设备名。**不解析 UA**（那要引库），只要够用户认出自己的机器。 */
function deviceLabel(ua: string | null): string {
  if (!ua) return '未知设备'
  const s = ua.toLowerCase()
  if (s.includes('windows')) return 'Windows'
  if (s.includes('android')) return 'Android'
  if (s.includes('iphone')) return 'iPhone'
  if (s.includes('ipad')) return 'iPad'
  if (s.includes('mac os') || s.includes('macintosh')) return 'macOS'
  if (s.includes('linux')) return 'Linux'
  return ua.slice(0, 40)
}
