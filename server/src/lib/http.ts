// 极简路由 / 响应工具。
//
// 为什么不用 Hono 之类的框架：整站只有 15 条动态路由，且**必须能被机械对账**
// （`paths.ts` 的表 ↔ 实际挂载的表）。框架会把「注册」藏在装饰器或中间件链里，
// 对账脚本就得跟着框架的实现走 —— 那比 60 行手写路由更容易漂。
// 依赖越少，Worker bundle 越小，免费额度下也更稳。

export type Method = 'GET' | 'POST'

export interface Ctx {
  req: Request
  env: Env
  url: URL
  /** 路径参数（`:id` → `{ id: '9' }`） */
  params: Record<string, string>
  /** Bearer token（无则 undefined）。**不在这里校验**，交给各路由决定要不要。 */
  token?: string
}

export type Handler = (ctx: Ctx) => Promise<Response> | Response

/**
 * 切段：丢**一个**前导空段（路径以 `/` 开头）与**一个**尾随空段（`/me/` ≡ `/me`）。
 *
 * 中间的空段**保留** —— `/a//b` 与 `/a/b` 是不同的请求。
 * 尾随斜杠则从宽：它是常规写法，且在这套路由里不可能与别的端点歧义。
 */
function segments(path: string): string[] {
  const out = path.split('/')
  if (out[0] === '') out.shift()
  if (out.length && out[out.length - 1] === '') out.pop()
  return out
}

/** percent 解码。`decodeURIComponent` 对畸形转义（如 `%zz`）会抛，包起来当原样。 */
function safeDecode(s: string): string {
  try {
    return decodeURIComponent(s)
  } catch {
    return s
  }
}

/**
 * 把 `/a/:id/b` 编成匹配器。段数必须相等，`:x` 捕获一段。
 *
 * ⚠️ 中间的空段**不**忽略：`/a//b` 与 `/a/b` 是不同的请求。
 * 早先用 `split('/').filter(Boolean)`，两种写法都归成一样，结果
 * `/api/v1/dev/submissions//withdraw` 被当成 `id='withdraw'` 命中了
 * withdraw 路由 —— 一处 URL 拼接 bug 被静默匹配成**另一个接口**。
 * 双斜杠几乎总是拼 URL 的 bug，应该响亮地 404，而不是猜。
 */
function compile(pattern: string): (path: string) => Record<string, string> | null {
  const parts = segments(pattern)
  return (path) => {
    const segs = segments(path)
    if (segs.length !== parts.length) return null
    const params: Record<string, string> = {}
    for (let i = 0; i < parts.length; i++) {
      const p = parts[i]!
      if (p.startsWith(':')) {
        if (!segs[i]) return null // 动态段不得为空
        params[p.slice(1)] = safeDecode(segs[i]!)
      } else if (p !== segs[i]) {
        return null
      }
    }
    return params
  }
}

export class Router {
  private routes: Array<{ method: Method; match: (p: string) => Record<string, string> | null; handler: Handler }> = []

  add(method: Method, pattern: string, handler: Handler): this {
    this.routes.push({ method, match: compile(pattern), handler })
    return this
  }

  /**
   * 找出匹配的处理函数。
   *
   * ⚠️ 路径匹配与**方法不匹配**必须报出不同的错：
   *  · 路径不存在 → `not_found`
   *  · 路径存在但方法不对 → `method_not_allowed`，并带 `allow` 列出该路径接受的方法
   * 只回 404 的话，「GET 写成了 POST」这种最常见的手误会伪装成「服务端没这个接口」，
   * 排查方向完全跑偏（约定 52 记的就是这类：404 让人以为域名/前缀错）。
   *
   * 返回**可判别联合**而不是裸的 `404 | 405` 数字：数字当返回值时 TypeScript
   * 无法收窄，调用方只能 `as` 强转或先 `typeof` 判断，写错一次就是运行期才炸。
   */
  resolve(
    method: string,
    path: string,
  ):
    | { kind: 'ok'; handler: Handler; params: Record<string, string> }
    | { kind: 'not_found' }
    | { kind: 'method_not_allowed'; allow: Method[] } {
    const pathMatched = new Set<Method>()
    for (const r of this.routes) {
      const params = r.match(path)
      if (params === null) continue
      pathMatched.add(r.method)
      if (r.method === method) return { kind: 'ok', handler: r.handler, params }
    }
    if (pathMatched.size > 0) {
      const allow = [...pathMatched]
      return { kind: 'method_not_allowed', allow }
    }
    return { kind: 'not_found' }
  }
}

// ---------------------------------------------------------------- 响应

export function json(data: unknown, status = 200, headers: Record<string, string> = {}): Response {
  return new Response(JSON.stringify(data), {
    status,
    headers: {
      'content-type': 'application/json; charset=utf-8',
      // 账号/配额类响应绝不能被任何中间层缓存
      'cache-control': 'no-store',
      ...headers,
    },
  })
}

/**
 * 错误响应。
 *
 * 键名是 `error` —— 客户端 `account.rs` 读的就是 `.get("error")`。
 * 另有部分路径读 `.get("message")`（如发布校验失败），故两个都给。
 */
export function fail(
  status: number,
  code: string,
  message: string,
  extra: Record<string, unknown> = {},
  headers: Record<string, string> = {},
): Response {
  return json({ error: code, message, ...extra }, status, headers)
}

export const unauthorized = () => fail(401, 'unauthorized', '未登录或会话已失效')

/** 读 JSON body。空 body 视为 `{}` —— 客户端对无参端点就发 `{}`，但也要容忍 `POST` 无 body。 */
export async function readJson(req: Request): Promise<Record<string, unknown>> {
  try {
    const v = await req.json()
    return v && typeof v === 'object' ? (v as Record<string, unknown>) : {}
  } catch {
    return {}
  }
}

// ---------------------------------------------------------------- 环境变量

export interface Env {
  DB: D1Database
  /**
   * Pages 提供的静态资产绑定（Workers 侧是 Workers Static Assets，同名不同物）。
   *
   * 存在的**唯一**理由：Pages 的 Functions 优先于静态资产，
   * 所以 `/api/v1/market/registry` 这些签名清单会先撞进 Function，
   * 必须靠它在 `handle.ts::is_static_asset` 里主动交还，
   * 才能拿到**未经序列化**的原始字节。少了它，Pages 上的验签必失败。
   *
   * 故标成可选：Workers 侧没有这个绑定，代码要能同时跑在两个运行时上。
   */
  ASSETS?: Fetcher
  /** 邀请码兑换 = 发额度 + 开放开发者申请，故与登录分开存 */
  INVITE_CODE?: string
  /** GitHub OAuth App 的 Client ID。设备码流程不需要 client_secret。 */
  GITHUB_CLIENT_ID?: string
  /** 平台 AI 的上游 Key。留空 = 平台对话回 401（**不**静默降级） */
  OPENAI_API_KEY?: string
  /** 邮件服务商（Resend）API Key；未配置时邮箱登录明确回「未配置」而不是假装成功 */
  RESEND_API_KEY?: string
  /**
   * 管理端**机器凭据**（`npm run review` 用，2026-10-01 加）。
   *
   * 与账号会话 token 是**两条独立**的路：会话 token 存在应用私有目录里，
   * 让 CLI 去读等于让脚本进另一个程序的家目录（耦合 + 读到用户身份 +
   * 重装即失效）。
   *
   * ⚠️ 门禁**仍要求 role='admin'** —— 这只是「用哪把钥匙开门」，
   *    不是「谁可以开门」。
   * ⚠️ 未配置时管理端对机器凭据**一律拒绝**（见 admin.ts::machineOk）。
   */
  ADMIN_TOKEN?: string
  EMAIL_FROM?: string
}

// ---------------------------------------------------------------- 鉴权

/**
 * 从 `Authorization: Bearer <token>` 取 token。
 *
 * 客户端 `account.rs` 用 `bearer_auth()` 发，故只认这一种形式。
 */
export function bearer(req: Request): string | undefined {
  const h = req.headers.get('authorization')
  if (!h) return undefined
  const m = /^Bearer\s+(.+)$/i.exec(h.trim())
  return m?.[1]?.trim() || undefined
}

/**
 * 定长字符串比较。
 *
 * ⚠️ 用它而不用 `a === b`：`===` 会在**第一个不同的字符**处提前返回，
 *   攻击者能用响应时间逐位试探出正确前缀。管理端 token 比的是机密，
 *   这点差别值得一个函数。
 *
 * 长度不同直接返回 false（那种情况本来就无解，长度不是秘密）。
 */
export function timingSafeEqual(a: string, b: string): boolean {
  if (a.length !== b.length) return false
  let diff = 0
  for (let i = 0; i < a.length; i++) diff |= a.charCodeAt(i) ^ b.charCodeAt(i)
  return diff === 0
}

export interface User {
  id: number
  github_id: string | null
  username: string
  email: string | null
  avatar: string | null
  role: string
  is_developer: number
  developer_status: string
  invite_redeemed: number
}

/** 解析 token → 用户。token 无效/过期一律返回 null（各路由决定回 401 还是 404）。 */
export async function currentUser(env: Env, token: string | undefined): Promise<User | null> {
  if (!token) return null
  const row = await env.DB.prepare(
    `SELECT u.id, u.github_id, u.username, u.email, u.avatar, u.role,
            u.is_developer, u.developer_status, u.invite_redeemed
       FROM sessions s
       JOIN users u ON u.id = s.user_id
      WHERE s.token = ?1 AND s.expires_at > ?2`,
  )
    .bind(token, Date.now())
    .first<User>()
  return row ?? null
}
