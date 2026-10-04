/**
 * 把后端抛出的错误翻成人能读的一句话。
 *
 * ## 为什么需要它
 *
 * Rust 侧 `account.rs::api_error` 统一产出 `CODE: 说明` 形态
 * （`QUOTA_EXHAUSTED: 今日平台额度已用完，明天再来`）—— 这个形态**对日志友好、
 * 对人不友好**：界面若直接渲染它，用户看到的是「一串错误代码」外加一句人话，
 * 而真正想让他知道的是后半句。
 *
 * 历史上各处自己剥前缀（`AccountPanel.vue` 的 `authErrorText` /
 * `emailErrorText` 各一份），于是只有账号那两条链路读得懂，AI 额度、
 * 邀请码、发布这些地方仍在把 `QUOTA_EXHAUSTED:` 直接怼到界面上。
 *
 * ## 口径
 *
 * 1. **剥掉大写代码前缀**（`^[A-Z][A-Z0-9_]*:\s*`）—— 它属于日志，不属于界面。
 *    原始串仍可在 `title` 里备查（多处 UI 已有这个做法）。
 * 2. **给已知的几个码补「下一步」**。光有「今日额度已用完」用户不知道该干嘛；
 *    「明天再来」「换个账号」「重新获取验证码」才是能照着做的下一步。
 * 3. **认不出就原样给 message**，绝不编：猜一个原因比不解释更糟。
 *
 * ## 不要在这里塞业务判断
 *
 * 「服务端是不是挂了」「要不要重试」这类判断各调用点不一样（发布要拦住、
 * 对话要提示重试），由调用点决定。这里只负责把错误码与说明变成一句话。
 */

/** 已知错误码 → 「发生了什么 + 接下来怎么办」。取自服务端实际发出的码。 */
const EXPLAINED: Record<string, string> = {
  // ---- 服务端实际发出的码（grep `fail(<状态>, '<码>'` 得到全集）----
  QUOTA_EXHAUSTED: '今天的 AI 额度已经用完了，明天再来',
  INVITE_REQUIRED: '要先兑换邀请码才能用这个功能，去「设置 → 账号」兑换',
  ALREADY_REDEEMED: '这个账号已经兑换过了',
  INVALID_CODE: '兑换码无效',
  SERVER_MISCONFIGURED: '服务端还没配好这项功能，等管理员处理',
  CODE_INVALID: '验证码不正确，重新输入一次',
  CODE_EXPIRED: '验证码已过期，点「重新获取」再输',
  TOO_MANY_TRIES: '尝试次数太多了，等一分钟再来',
  RATE_LIMITED: '操作太频繁了，等一会儿再来',
  UNAUTHORIZED: '登录状态过期了，去「设置 → 账号」重新登录',
  NOT_DEVELOPER: '要先成为开发者才能做这个操作，去「设置 → 账号」申请',
  ALREADY_DEVELOPER: '你已经是开发者了',
  ALREADY_PENDING: '你已有一份申请在审核中，等结果就行',
  NOT_APPROVED: '开发者申请还在审核中，通过之后才能做这个操作',
  NOT_CONFIGURED: '服务端还没启用这项功能，换个方式试试',
  GATE_FAILED: '扩展没通过发布前的自动检查，按列出来的问题改完再发',
  SCREENSHOT_TOO_LARGE: '单张截图超过 2MB，换一张小一点的',
  SHOT_NOT_IMAGE: '截图不是 png/jpg/webp，换个格式',
  PACKAGE_MISSING: '服务端找不到这个扩展包，重新提交一次',
  TOO_LARGE: '文件超出大小上限',
  PACKAGE_TOO_LARGE: '安装包太大，试着去掉扩展里的多余文件',
  BLOCKED_BY_OPEN_SUBMISSION: '这个扩展还有一份提交没走完流程，先撤回它或等审核结果',
  SERVER_ERROR: '服务端出错了，稍后再试',
  GITHUB_UNAVAILABLE: '登录服务连不上 GitHub（服务端网络问题，与你的网络无关）',

  // ---- 客户端自造的码（Rust 侧直接 format!("CODE: …")）----
  NETWORK_ERROR: '连不上服务器，先检查网络或代理',
  SERVER_TIMEOUT: '服务器没有响应，稍后再试',
  SERVER_ADDRESS_INVALID: '服务地址无法解析（不是你的网络问题，重试也不会好）',
  PLATFORM_UNAVAILABLE: '平台服务暂时不可用，稍后再试',
  IO_ERROR: '读写文件失败，检查权限后重试',
  INVALID_ARGUMENT: '参数不合法',
  CONFLICT: '操作与当前状态冲突，刷新后重试',
  ALREADY_EXISTS: '已经存在了',
  RECURRENCE_INVALID_TIME: '周期规则的时间不合法',
  RECURRENCE_ITER_LIMIT: '周期规则算不出下一轮，请检查规则',
  RECURRENCE_RANGE_TOO_LARGE: '周期跨度太大，请缩小范围',
}

/** `CODE: 说明` 里的代码部分；不是这个形态就返回 null */
function codeOf(raw: string): string | null {
  const m = /^\s*([A-Z][A-Z0-9_]*):\s?/.exec(raw)
  return m ? m[1]! : null
}

/** 剥掉代码前缀，取说明部分 */
function bodyOf(raw: string): string {
  return raw.replace(/^\s*[A-Z][A-Z0-9_]*:\s*/, '').trim()
}

/**
 * 把任意 catch 到的值翻成一句话。
 *
 * ⚠️ 认不出的错误**原样返回原串**（含代码）：那说明服务端发了一个我们没收录的码，
 *   界面显示出来用户至少能截图反馈，编一句「请稍后再试」反而把他引向错误方向。
 */
export function errorText(e: unknown): string {
  // ⚠️ `Error` 实例要取 `.message`：`String(new Error('网络断了'))` 是
  //   `'Error: 网络断了'` —— 界面上就多出一个「Error:」前缀，
  //   与我们要消灭的「一串错误代码」是同一种毛病。
  const raw =
    e instanceof Error ? e.message : typeof e === 'string' ? e : String(e)
  const code = codeOf(raw)
  const body = bodyOf(raw)
  // ⚠️ 优先用收录过的解释：它比裸 message 多一个「下一步」。
  //   但解释里要带上原始说明（作者可能写得比我们细）—— 故两者拼接而非取舍。
  if (code && EXPLAINED[code]) {
    return body ? `${EXPLAINED[code]}（${body}）` : EXPLAINED[code]!
  }
  return body || raw
}

/** 原始串（给 `title` 备查用）。界面上不该出现它，但排查时需要它。 */
export function errorRaw(e: unknown): string {
  return e instanceof Error ? `${e.name}: ${e.message}` : typeof e === 'string' ? e : String(e)
}