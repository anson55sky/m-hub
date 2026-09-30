// 客户端 ↔ 服务端的**路径契约**。
//
// ## 为什么不直接抄一份就算
//
// 约定 47：真相源只有客户端的 `src-tauri/src/api_spec.rs`（Rust 侧有测试锁住
// 路径与蛇形 body 键名）。本文件是它在 Worker 侧的对应物，**必然是第二份拷贝**
// —— 两端各自的测试都绿、一对接就 404/400，正是这层最容易出的事。
//
// 所以本文件不做「我抄对了」的自我声明，而是被机械对账：
// `scripts/check-api-spec-conformance.mjs` 从 `api_spec.rs` 抽出全部路径，
// 逐条与 `ROUTES` 比对，不一致直接让 `npm run build` 失败。
// **改契约时先看那个脚本的报错，再改两边。**
//
// ⚠️ 两份清单刻意分开，因为它们的失败后果不同：
//   · `STATIC` —— 客户端启动即拉、验签不过就整条功能死
//   · `DYNAMIC` —— 登录/发布等交互路径，错了才有影响
// 静态那两条放在最前面是有意的：它们是唯一「不写对就全局失效」的。

/** 静态托管的清单资源。`.sig` 由客户端按 `{url}.sig` 拼接后取，故无需单列。 */
export const STATIC = {
  /** 扩展市场清单。客户端验 Ed25519 分离签名，验不过 → 回退本地缓存。 */
  marketRegistry: '/api/v1/market/registry',
  /** 应用升级清单。客户端验签 + semver 比较 + 平台条目匹配。 */
  appUpdate: '/api/v1/app/update',
} as const

/**
 * 动态路由。
 *
 * 命名一律「方法_路径」，路径用 `:param` 标动态段。`check-api-spec-conformance.mjs`
 * 按这套命名抽出并与 `api_spec.rs` 对账，故**改这里必须同步改那边**。
 */
export const DYNAMIC = {
  // ---- 登录（GitHub 设备码）----
  post_auth_github_device_start: '/api/v1/auth/github/device/start',
  post_auth_github_device_poll: '/api/v1/auth/github/device/poll',

  // ---- 登录（邮箱验证码）----
  post_auth_email_send: '/api/v1/auth/email/send',
  post_auth_email_verify: '/api/v1/auth/email/verify',

  // ---- 账号与权益 ----
  /**
   * 登录态。**走根路径 `/me` 而非 `/api/v1/me`** —— 这是上游的历史形状，
   * 客户端 `account.rs::get_me` 就按 `/me` 取。按直觉「修正」成 `/api/v1/me`
   * 会立刻 404（约定 52 记了这个坑）。conformance 脚本对此单列一条断言。
   */
  get_me: '/me',
  post_me_redeem: '/api/v1/me/redeem',
  get_me_device_tokens: '/api/v1/me/device-tokens',
  post_me_device_tokens_revoke: '/api/v1/me/device-tokens/:id/revoke',

  // ---- 平台 AI 额度 ----
  get_ai_models: '/api/v1/ai/models',

  // ---- 开发者申请 ----
  post_dev_apply: '/api/v1/dev/apply',
  get_dev_apply: '/api/v1/dev/apply',

  // ---- 扩展发布 ----
  post_dev_submissions: '/api/v1/dev/submissions',
  get_dev_submissions: '/api/v1/dev/submissions',
  get_dev_submissions_id: '/api/v1/dev/submissions/:id',
  post_dev_submissions_id_withdraw: '/api/v1/dev/submissions/:id/withdraw',
} as const

export type DynamicKey = keyof typeof DYNAMIC

/**
 * OpenAI 兼容面。**不在 `api_spec.rs` 里** —— 它由 `chat.rs::platform_base_url`
 * 拼 `{SERVER}/v1` 再接 `/chat/completions` 得到，所以没有进那份「请求规格」。
 *
 * 正因为不在，conformance 脚本无法从 `api_spec.rs` 抽到它，也就**没有任何机制
 * 会提醒你它存在** —— 而它一旦漏实现，症状是「开了平台额度，一发消息就失败」，
 * 且 404 现场离改动点很远（约定 47 的原话）。故在此显式登记，并由脚本断言
 * 客户端的拼法确实等于本路径。
 */
export const OPENAI_COMPAT = {
  post_chat_completions: '/v1/chat/completions',
} as const

/**
 * 「我实际注册了哪些路由」—— 与 `DYNAMIC` 刻意分开。
 *
 * 分开的原因：`DYNAMIC` 是「应该有哪些」，这里是「真的挂上了哪些」。
 * 二者漂移时（本文件加了路由却忘了在 index.ts 里挂）单靠读代码看不出来，
 * 而症状是运行期 404 —— 约定 47 说的「失败现场离改动点很远」。
 * `conformance` 脚本会断言两者集合相等。
 */
export const MOUNTED = [
  'post_auth_github_device_start',
  'post_auth_github_device_poll',
  'post_auth_email_send',
  'post_auth_email_verify',
  'get_me',
  'post_me_redeem',
  'get_me_device_tokens',
  'post_me_device_tokens_revoke',
  'get_ai_models',
  'post_dev_apply',
  'get_dev_apply',
  'post_dev_submissions',
  'get_dev_submissions',
  'get_dev_submissions_id',
  'post_dev_submissions_id_withdraw',
] as const satisfies readonly DynamicKey[]
