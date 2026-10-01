<script setup lang="ts">
// 账号大类（登录 / 额度 / 开发者申请 / 我的设备）
//
// 平台服务端地址是内置常量（`config::DEFAULT_SERVER_URL`），设置里**不提供地址入口**：
// 正式域名启用后用户无需也无法改地址（改地址只会让账号功能指向失效的服务端，见约定 52）。
//
// 从 SettingsView.vue 拆出（见该文件顶部说明）：设置页按大类按需加载，
// 首次打开只需外壳 + 当前大类的代码，切大类时才加载对应面板。
import { inject, onBeforeUnmount, onMounted, ref } from 'vue';
import { Copy } from 'lucide-vue-next';
import { isTauri, tauriApi } from '../../api/tauri';
import type {
  AccountDevice,
  AccountStatus,
  GithubDeviceStart,
  GithubLocalDeviceStart,
  GithubLocalStatus,
} from '../../api/tauri';
import { reportClientError } from '../../utils/error-report';

const showToast = inject<(msg: string) => void>('showToast', () => {})

// ---- 平台账号（登录 / 额度 / 开发者申请） ----
// 登录只服务「申请开发者 + 发布扩展」：装扩展、用自带 API Key 的 AI 对话都不需要账号。
const account = ref<AccountStatus | null>(null)
const accountBusy = ref(false)
const ghDevice = ref<GithubDeviceStart | null>(null)
/** 发起登录的进行中态：服务端要代调 GitHub（自带 15s 超时），期间按钮必须立刻给出可见反馈 */
const ghStarting = ref(false)

/**
 * 邮箱验证码登录入口**已开启**（2026-09-18）：服务端 SMTP 就绪，`POST /api/v1/auth/email/send`
 * 正常发码（6 位数字、10 分钟有效、同一邮箱每小时最多 5 封）。
 *
 * 它是国内网络下的**兜底登录方式**：GitHub 链路要服务端出网到 github.com / api.github.com，
 * 国内机房会间歇性失败，邮箱收码不依赖任何境外链路。
 *
 * 曾以 `EMAIL_LOGIN_ENABLED: boolean = false` + 模板里的 settings-index skip 标记
 * 临时隐藏（服务端没配发信时，入口只会让用户点一次撞一次 503）；现在两者一并移除 ——
 * 若哪天服务端又没有发信，界面上会就地显示「服务器尚未配置发信」，不再是静默失效。
 */

/**
 * 登录区的就地反馈（busy = 正在联系服务端；warn = 需要用户自己动手；error = 失败原因）。
 *
 * 为什么不用 toast 报错：服务端要**代调 GitHub**（它自己有 15s 超时），发起失败实测要 10s 上下才返回，
 * 而 toast 只显示 2.2s —— 用户点完「开始登录」干等十几秒，期间界面只有按钮文案变化，
 * 体感就是「点了没反应」（实测：服务端连不上 GitHub 时稳定 ~10.5s 返回 502，
 * toast 一闪即过，用户完全没看见）。所以登录结果就地在按钮下方常驻，直到下一次操作。
 */
const loginNotice = ref<{ kind: 'busy' | 'warn' | 'error'; text: string; raw: string } | null>(null)

/**
 * 平台服务（m-hub 服务器）确认不通。
 *
 * 只在**真的试过一次并失败**之后才置位 —— 不靠猜、也不靠启动时探测：
 * 未登录时 `accountStatus` 根本不联网（直接返回 signed_out），拿它探测不出通不通。
 *
 * 置位后的行为：平台那组按钮置灰 + 一条**中性**说明。
 * 之前是让用户反复点、每次吃一条红字，而红字说的是「发起登录失败」——
 * 于是「GitHub 已经登录成功了」和「登录失败」同屏出现，用户无从判断哪个是哪个
 * （2026-09-30 用户实际困惑过：截图里两条登录都叫「用 GitHub 登录」，
 * 成功的那条报错的那条各占一行）。
 */
const platformDown = ref(false)
/** 服务端**主动拒绝**（不是连不上），且属于「配置/未部署」而非「临时故障」 */
const platformUnconfigured = ref<string>('')
/**
 * 平台 AI 上游未配置（服务端没有 `OPENAI_API_KEY`）。
 *
 * 与 `platformDown` 的区别很重要：**这不是故障，也不是临时不可用** ——
 * 它不会自己恢复，重新登录也不会变。故不能用「请稍后重试」那套话术。
 * 而 `platformUnconfigured`（服务端没配 GitHub 凭证）也不对：那是登录侧的问题，
 * 账号本身是好着的（你已经能看到自己名字了）。
 *
 * 判据：**服务端明确说了「上游没配」**（`available:false` / `PLATFORM_UNAVAILABLE`）
 * 才置位；连不上仍算 `platformDown`。
 */
const platformAiUnavailable = ref<string>('')

/**
 * 传输层错误码 = 「服务不可用」，而不是「你密码填错了」这类可重试失败。
 *
 * ⚠️ `SERVER_MISCONFIGURED` 是**第三类**，原先两类都没覆盖它，实机表现：
 * 服务端活着、但没配 `GITHUB_CLIENT_ID` → 点「开始登录」弹红色 toast
 * 「服务端未配置 GITHUB_CLIENT_ID」，而同屏上方本地 GitHub 登录是**绿的**
 * （GlassPad 已登录）。两条并存，用户只会以为自己白登了。
 *
 * 它与 `platformDown` 的区别很重要：传输故障是**临时**的、该重试；
 * 未配置是**配置状态**、重试多少次都一样，故单独一个 state 且文案不同。
 */
function isPlatformDown(e: unknown): boolean {
  const raw = String(e)
  return (
    raw.startsWith('SERVER_ADDRESS_INVALID') ||
    raw.startsWith('SERVER_TIMEOUT') ||
    raw.startsWith('NETWORK_ERROR') ||
    raw.startsWith('GITHUB_UNAVAILABLE')
  )
}

/** 从错误里认出「服务端未配置某项」，返回人话说明；不是这一类则返回空串 */
function unconfiguredReason(e: unknown): string {
  const raw = String(e)
  const m = /SERVER_MISCONFIGURED:\s*([^"'\\]+)/.exec(raw)
  if (!m) return ''
  const what = (m[1] || '').trim()
  if (what.includes('GITHUB_CLIENT_ID')) {
    return '服务端尚未配置 GitHub 登录凭证，平台登录暂不可用。' +
      '这不影响上面的本地 GitHub 登录，也不影响扩展市场与自动更新。'
  }
  if (what.includes('INVITE_CODE')) {
    return '服务端尚未配置兑换码，兑换与开发者申请暂不可用。'
  }
  return `服务端尚未配置${what || '所需配置'}，该功能暂不可用。`
}

/**
 * 后端错误是 `CODE: 说明` 形态（account.rs::api_error）：界面只展示说明部分，编码留在 title 里备查。
 * 少数几个码额外补一句「该找谁处理」——尤其 GITHUB_UNAVAILABLE 是**服务端出网问题**，
 * 不加说明用户只会以为是自己网络的问题，反复重试。
 */
function authErrorText(e: unknown): { text: string; raw: string } {
  const raw = String(e)
  const body = raw.replace(/^[A-Z_]+:\s*/, '')
  if (raw.startsWith('GITHUB_UNAVAILABLE')) {
    return { text: '登录服务暂时连不上 GitHub，请稍后再试（服务端网络问题，与你的网络无关）', raw }
  }
  if (raw.startsWith('SERVER_ADDRESS_INVALID')) {
    // 服务地址本身不可解析 —— 归因明确写清「不是你的网络」，否则用户会去查自己的网
    return {
      text:
        '登录服务地址无法解析（域名不存在）。这不是你的网络问题，重试也不会好 —— ' +
        '该服务当前不可用，登录与验证码等功能需要先有一个可用的服务地址。',
      raw,
    }
  }
  if (raw.startsWith('SERVER_TIMEOUT')) {
    return { text: '登录服务没有响应（域名能解析，但服务无响应，可能已停机）', raw }
  }
  if (raw.startsWith('NETWORK_ERROR')) {
    return { text: '连不上登录服务器（域名能解析，但连接失败）。可稍后重试。', raw }
  }
  // 轮询阶段的两个常见结果：用户在 GitHub 页点了「取消」，或等待窗口过期
  if (raw.includes('access_denied')) return { text: '你取消了 GitHub 授权', raw }
  if (raw.includes('expired_token') || raw.includes('session_expired')) {
    return { text: '这次登录等待已超时，请重新发起', raw }
  }
  // 服务端建会话失败（500 login_failed）：拿到的 message 是「登录失败，请稍后再试」，
  // 从这句话看不出是服务端出的问题 —— 补上归因与线索位置，否则用户只会以为是自己操作错了。
  if (raw.startsWith('LOGIN_FAILED')) {
    return { text: '服务端建立登录会话时出错（500），请稍后再试（服务端日志里有 [auth] 开头的详情）', raw }
  }
  return { text: body || raw, raw }
}

/**
 * 邮箱链路的错误文案：服务端已经把 message 写成可读句子（`ERROR_CODE: 说明`），
 * 这里只补几条「该怎么办」，否则用户对同一句话只会反复点重试。
 *
 * ⚠️ `RATE_LIMITED`（同一邮箱每小时 5 封）是最容易踩的一条：文案只说「发送过于频繁」时，
 * 用户的本能是立刻再点一次 —— 越点越久，而且这一小时内都没法登录。
 */
function emailErrorText(e: unknown): { text: string; raw: string } {
  const raw = String(e)
  if (raw.startsWith('RATE_LIMITED')) {
    return { text: '这个邮箱一小时内发得太多了（上限 5 封），请稍后再试，或换一个邮箱', raw }
  }
  if (raw.startsWith('EMAIL_SEND_FAILED')) {
    return { text: '邮件发送失败，请稍后再试（服务端发信出错，与你的邮箱无关）', raw }
  }
  if (raw.startsWith('EMAIL_NOT_CONFIGURED')) {
    return { text: '服务器尚未配置发信，暂时无法用邮箱登录', raw }
  }
  return authErrorText(e)
}

/** 发送成功后的重发冷却：连点不但会撞服务端限流，还会把「最新一封」的验证码换掉 */
function startEmailCooldown(sec: number) {
  emailCooldown.value = sec
  if (emailTimer !== null) clearInterval(emailTimer)
  emailTimer = window.setInterval(() => {
    emailCooldown.value -= 1
    if (emailCooldown.value <= 0) {
      emailCooldown.value = 0
      if (emailTimer !== null) {
        clearInterval(emailTimer)
        emailTimer = null
      }
    }
  }, 1000)
}

const emailInput = ref('')
const emailCode = ref('')
const emailSent = ref(false)
/** 发信请求在途（accountBusy 与 GitHub 链路共用，按钮文案要更精确的这一个） */
const emailSending = ref(false)
/** 重发冷却秒数：服务端按邮箱限流（每小时 5 封），连点会把配额一次耗光 */
const emailCooldown = ref(0)
let emailTimer: number | null = null
/** 邮箱链路的就地反馈（同 loginNotice：发信要走 SMTP，可能等数秒，只挂 toast 等于没有反馈） */
const emailNotice = ref<{ kind: 'busy' | 'warn' | 'error'; text: string; raw: string } | null>(null)
const redeemInput = ref('')
const showApply = ref(false)
const applyReason = ref('')
let ghTimer: number | null = null
/** GitHub 要求降低轮询频率（slow_down）时置位：界面据此换一句提示，别让用户以为卡死了 */
const ghSlow = ref(false)

/** 复制文本（剪贴板 API 不可用时回退 execCommand，与 AiProviders 同款做法） */
async function copyText(text: string, okMsg: string): Promise<void> {
  if (!text) return
  try {
    await navigator.clipboard.writeText(text)
    showToast(okMsg)
    return
  } catch {
    // 继续尝试 execCommand 回退
  }
  const ta = document.createElement('textarea')
  ta.value = text
  ta.style.position = 'fixed'
  ta.style.opacity = '0'
  document.body.appendChild(ta)
  ta.select()
  let ok = false
  try {
    ok = document.execCommand('copy')
  } catch {
    ok = false
  }
  document.body.removeChild(ta)
  showToast(ok ? okMsg : '复制失败')
}

async function loadAccount() {
  if (!isTauri()) return
  try {
    account.value = await tauriApi.accountStatus()
    if (account.value.loggedIn) void loadDevices()
  } catch {
    // 后端不可用（浏览器预览）时静默
  }
}

// ---- 在线设备（多设备登录：换机不被顶下线，丢失时可单独撤销） ----
const devices = ref<AccountDevice[]>([])
const devicesMax = ref(5)
const devicesBusy = ref(false)

async function loadDevices() {
  if (!isTauri()) return
  try {
    const r = await tauriApi.accountListDevices()
    devices.value = r.devices ?? []
    devicesMax.value = r.max ?? 5
  } catch {
    devices.value = []
  }
}

/**
 * 会话已失效（401）时的统一收尾：清过的 token 由 Rust 侧清掉，这里只需刷新界面。
 * 场景：在别处撤销了本机 / 服务端重置了 token —— 不刷新的话界面会一直显示「已登录」。
 */
async function handleAuthError(e: unknown): Promise<boolean> {
  if (String(e).includes('UNAUTHORIZED')) {
    await loadAccount()
    showToast('登录已失效，请重新登录')
    return true
  }
  return false
}

async function revokeDevice(id: number) {
  devicesBusy.value = true
  try {
    await tauriApi.accountRevokeDevice(id)
    await loadDevices()
    showToast('已撤销该设备')
  } catch (e) {
    if (await handleAuthError(e)) return
    showToast(`撤销失败：${e}`)
  } finally {
    devicesBusy.value = false
  }
}

function fmtDeviceTime(ms: number): string {
  const d = new Date(ms)
  const p = (n: number) => String(n).padStart(2, '0')
  return `${d.getMonth() + 1}-${p(d.getDate())} ${p(d.getHours())}:${p(d.getMinutes())}`
}

function stopGithubPolling() {
  if (ghTimer !== null) {
    clearTimeout(ghTimer)
    ghTimer = null
  }
}

// ---------------- 客户端直连 GitHub（不经平台服务端）----------------
//
// GitHub 的 Device Flow **不需要 client_secret**（它就是为「设备无法安全保存密钥」
// 而设计的），所以客户端可以自己走完全程 —— 不经平台服务端。
//
// ⚠️ 两条链路仍然并存、互不替代（2026-09-30 复核）：平台服务端已迁到
// `m-hub-server.pocketbay.app`，而本地直连这条路**依然保留**——
// 它不依赖服务端可用性，服务端挂掉时仍能登录看身份。
//
// 与服务端那条链路的区别必须写在界面上：
//   · 本地登录 = 真的 GitHub 身份（用户名/头像/邮箱）+ 一个可用的 GitHub 凭据
//   · 平台能力（AI 额度 / 申请开发者 / 发布扩展 / 市场）**仍然要服务器**，登录后依旧不可用
// 所以下面那段文案要说清这件事，不能让用户以为登录了就全能。
const ghLocal = ref<GithubLocalStatus>({ loggedIn: false, needsClientId: false, identity: null })
const ghLocalStarting = ref(false)
const ghLocalDevice = ref<GithubLocalDeviceStart | null>(null)
const ghLocalNotice = ref<{ kind: 'busy' | 'warn' | 'error' | 'ok'; text: string } | null>(null)
let ghLocalTimer: number | undefined

async function loadGhLocal() {
  if (!isTauri()) return
  try {
    ghLocal.value = await tauriApi.githubStatus()
  } catch (e) {
    // 读状态失败不该让整页报错：只当没登录，登录按钮仍然可用
    ghLocal.value = { loggedIn: false, needsClientId: false, identity: null }
  }
}

function stopGhLocalPolling() {
  if (ghLocalTimer !== undefined) {
    clearTimeout(ghLocalTimer)
    ghLocalTimer = undefined
  }
}

function cancelGhLocalLogin() {
  stopGhLocalPolling()
  ghLocalDevice.value = null
  ghLocalStarting.value = false
  ghLocalNotice.value = null
}

/**
 * 轮询本地授权结果。
 *
 * ⚠️ `slow_down` 由 Rust 侧并进了 pending（我们把间隔取 ≥ 5s 已经够保守），
 * 但仍要按服务端给的 interval 走，不能更快 —— GitHub 收到过快轮询会一直回
 * slow_down，现象是「浏览器已授权、客户端永远等待」，而日志一切正常。
 */
function startGhLocalPolling(dev: GithubLocalDeviceStart) {
  stopGhLocalPolling()
  const intervalMs = Math.max(5, dev.interval) * 1000
  const tick = async () => {
    try {
      const r = await tauriApi.githubDevicePoll(dev.deviceCode)
      if (r.status === 'pending') {
        ghLocalTimer = window.setTimeout(() => void tick(), intervalMs)
        return
      }
      stopGhLocalPolling()
      ghLocalDevice.value = null
      if (r.status === 'done') {
        ghLocalNotice.value = { kind: 'ok', text: '登录成功' }
        showToast('登录成功')
        await loadGhLocal()
        // 平台状态也重拉一次：登录态变了，界面别停在旧值
        void loadAccount()
      } else {
        ghLocalNotice.value = { kind: 'error', text: `登录失败：${r.message}` }
        showToast('登录失败')
      }
    } catch (e) {
      stopGhLocalPolling()
      ghLocalDevice.value = null
      ghLocalNotice.value = { kind: 'error', text: `登录失败：${String(e)}` }
      void reportClientError('GitHub 本地登录轮询失败', { error: String(e) })
    }
  }
  ghLocalTimer = window.setTimeout(() => void tick(), intervalMs)
}

async function startGhLocalLogin() {
  if (ghLocalStarting.value || ghLocalDevice.value) return
  ghLocalStarting.value = true
  ghLocalNotice.value = { kind: 'busy', text: '正在向 GitHub 申请验证码…' }
  try {
    const dev = await tauriApi.githubDeviceStart()
    ghLocalDevice.value = dev
    ghLocalNotice.value = null
    // 顺手开浏览器；打不开就明说（码在界面上、按钮也在，静默失败会被当成「点了没反应」）
    void tauriApi.openExternal(dev.verificationUri).catch(() => {
      if (ghLocalDevice.value === dev) {
        ghLocalNotice.value = { kind: 'warn', text: '没能自动打开浏览器，请点下面的「打开浏览器」' }
      }
    })
    startGhLocalPolling(dev)
  } catch (e) {
    const text = String(e)
    if (ghLocal.value.needsClientId) {
      // 缺 Client ID 属于「还没配好」而不是「登录失败」：静态黄块已写清做法，
      // 这里再报一次红字只会让人以为程序坏了（实测界面上黄字红字重复出现）
      ghLocalNotice.value = null
    } else {
      ghLocalNotice.value = { kind: 'error', text }
      void reportClientError('GitHub 本地登录发起失败', { error: text })
    }
  } finally {
    ghLocalStarting.value = false
  }
}

async function ghLocalLogout() {
  stopGhLocalPolling()
  ghLocal.value = await tauriApi.githubLogout()
  ghLocalNotice.value = null
  showToast('已退出 GitHub 登录')
}

/**
 * 轮询 GitHub 授权结果（按服务端给的 interval，最少 3 秒一次）。
 *
 * ⚠️ `slow_down` 必须被尊重：GitHub 收到过快轮询后会回它，并要求「之后所有请求间隔 +5s」。
 * 早期实现由服务端把它并进 pending 吞掉，于是客户端永远按原速轮询、永远不会减速——
 * 用户看到的现象是「浏览器里已经授权成功，客户端却一直等待授权」，而且服务端日志一片正常。
 */
function startGithubPolling(dev: GithubDeviceStart) {
  stopGithubPolling()
  let intervalMs = Math.max(3, dev.interval) * 1000
  const tick = async () => {
    try {
      const r = await tauriApi.accountLoginGithubPoll(dev.pollId)
      if (r.status === 'pending') {
        ghTimer = window.setTimeout(() => void tick(), intervalMs)
        return
      }
      if (r.status === 'slow_down') {
        intervalMs += 5000
        ghSlow.value = true
        ghTimer = window.setTimeout(() => void tick(), intervalMs)
        return
      }
      stopGithubPolling()
      ghDevice.value = null
      if (r.status === 'ok') {
        loginNotice.value = null
        showToast('登录成功')
        await loadAccount()
      } else {
        const { text, raw } = authErrorText(r.message ?? '未知原因')
        loginNotice.value = { kind: 'error', text: `登录失败：${text}`, raw }
        showToast(`登录失败：${text}`)
      }
    } catch (e) {
      stopGithubPolling()
      ghDevice.value = null
      const { text, raw } = authErrorText(e)
      loginNotice.value = { kind: 'error', text: `登录失败：${text}`, raw }
      showToast(`登录失败：${text}`)
      // 同发起失败：轮询请求本身报错（如服务端 500）要在客户端留痕
      void reportClientError('账号登录轮询失败', { error: raw })
    }
  }
  ghTimer = window.setTimeout(() => void tick(), intervalMs)
}

async function startGithubLogin() {
  if (ghStarting.value) return
  ghStarting.value = true
  accountBusy.value = true
  // 先落一条「正在联系」，否则从点击到服务端回话这段（最长 20s）界面没有任何变化
  loginNotice.value = { kind: 'busy', text: '正在联系登录服务…（最长约 20 秒）', raw: '' }
  const startedAt = Date.now()
  try {
    const dev = await tauriApi.accountLoginGithubStart()
    ghSlow.value = false
    ghDevice.value = dev
    loginNotice.value = null
    // 顺手把浏览器打开（用户仍可手动复制地址）；打不开就明说 —— 验证码在界面上、按钮也在，
    // 静默失败会让人以为「点了没反应」（和发起失败同一类体感问题）
    void tauriApi.openExternal(dev.verificationUri).catch(() => {
      if (ghDevice.value === dev) {
        loginNotice.value = { kind: 'warn', text: '没能自动打开浏览器，请点下面的「打开浏览器」继续', raw: '' }
      }
    })
    startGithubPolling(dev)
  } catch (e) {
    // 服务端代调 GitHub 失败时给的是可读文案（如「GITHUB_UNAVAILABLE: GitHub 暂时不可用，请稍后再试」）
    const { text, raw } = authErrorText(e)
    if (unconfiguredReason(e)) {
      platformUnconfigured.value = unconfiguredReason(e)
    }
    if (isPlatformDown(e)) {
      // 平台服务不通：不要再摆一条红字「发起登录失败」——
      // 同一个页面上方「GitHub 登录已成功」是绿的，下面一条红的「登录失败」
      // 只会让人以为刚才白登了。改成置位 + 中性说明 + 按钮置灰。
      platformDown.value = true
      loginNotice.value = null
      showToast('平台服务当前不可用')
    } else if (unconfiguredReason(e)) {
      // 同理：「服务端未配置 X」也**不是**一次登录失败。
      // 摆红字 + 弹 toast 的后果实测很糟：上方本地 GitHub 登录是绿的
      // （已登录），下方一条红的「登录失败」让人以为白登了。
      // 改成按钮置灰 + 常驻中性说明（已由 platformUnconfigured 渲染）。
      loginNotice.value = null
      showToast('该功能需服务端配置后才可用')
    } else {
      loginNotice.value = { kind: 'error', text: `发起登录失败：${text}`, raw }
      showToast(`发起登录失败：${text}`)
    }
    // 落本地日志（含耗时）：账号链路横跨客户端 / 服务端 / GitHub 三方，
    // 没有这条记录时「服务端返回 500」在客户端侧不留任何痕迹，只能靠服务端日志——
    // 那台机器不一定够得着。耗时还能区分「超时」与「立刻报错」两种完全不同的故障。
    void reportClientError('账号登录发起失败', { error: raw, elapsedMs: Date.now() - startedAt })
  } finally {
    accountBusy.value = false
    ghStarting.value = false
  }
}

/** 取消 GitHub 登录等待：停掉轮询并收起验证码（否则点错了只能干等 15 分钟） */
function cancelGithubLogin() {
  stopGithubPolling()
  ghDevice.value = null
  ghSlow.value = false
  loginNotice.value = null
}

async function sendEmailCode() {
  const email = emailInput.value.trim()
  if (!email) {
    emailNotice.value = { kind: 'warn', text: '请先填写邮箱', raw: '' }
    return
  }
  if (emailSending.value || emailCooldown.value > 0) return
  emailSending.value = true
  accountBusy.value = true
  // 先落一条「正在发送」：发信要过 SMTP，弱网/对方服务器慢时要数秒，期间界面必须有变化
  emailNotice.value = { kind: 'busy', text: '正在发送验证码…', raw: '' }
  const startedAt = Date.now()
  try {
    const r = await tauriApi.accountLoginEmailSend(email)
    if (!r.ok) {
      // 服务端未配置发信（Rust 侧把 503 转成 ok=false + 说明）：显示出来而不是假装成功
      emailNotice.value = { kind: 'error', text: r.message ?? '发送失败', raw: '' }
      return
    }
    emailSent.value = true
    emailNotice.value = null
    showToast('验证码已发送，请查收邮件')
    startEmailCooldown(60)
  } catch (e) {
    const { text, raw } = emailErrorText(e)
    // 与平台 GitHub 那条同口径：传输层失败 = 服务不通，置位并撤掉红字，
    // 别让邮箱区也摆一条「发送失败」，与上方绿色的「GitHub 登录已成功」打架
    if (unconfiguredReason(e)) {
      platformUnconfigured.value = unconfiguredReason(e)
    }
    if (isPlatformDown(e)) {
      platformDown.value = true
      emailNotice.value = null
    } else {
      emailNotice.value = { kind: 'error', text, raw }
    }
    showToast(`发送失败：${text}`)
    // 与 GitHub 链路同款留痕：发信失败横跨客户端 / 服务端 / 邮件服务商三方，客户端不留痕就无从排查
    void reportClientError('邮箱验证码发送失败', { error: raw, elapsedMs: Date.now() - startedAt })
  } finally {
    emailSending.value = false
    accountBusy.value = false
  }
}

async function verifyEmailCode() {
  const email = emailInput.value.trim()
  const code = emailCode.value.trim()
  if (!code) {
    emailNotice.value = { kind: 'warn', text: '请填写邮件里的 6 位验证码', raw: '' }
    return
  }
  accountBusy.value = true
  emailNotice.value = { kind: 'busy', text: '正在校验验证码…', raw: '' }
  try {
    account.value = await tauriApi.accountLoginEmailVerify(email, code)
    emailSent.value = false
    emailCode.value = ''
    emailNotice.value = null
    showToast('登录成功')
  } catch (e) {
    const { text, raw } = emailErrorText(e)
    // 与平台 GitHub 那条同口径：传输层失败 = 服务不通，置位并撤掉红字，
    // 别让邮箱区也摆一条「发送失败」，与上方绿色的「GitHub 登录已成功」打架
    if (unconfiguredReason(e)) {
      platformUnconfigured.value = unconfiguredReason(e)
    }
    if (isPlatformDown(e)) {
      platformDown.value = true
      emailNotice.value = null
    } else {
      emailNotice.value = { kind: 'error', text, raw }
    }
    showToast(`登录失败：${text}`)
  } finally {
    accountBusy.value = false
  }
}

async function doLogout() {
  accountBusy.value = true
  try {
    stopGithubPolling()
    ghDevice.value = null
    account.value = await tauriApi.accountLogout()
    showToast('已退出登录')
  } catch (e) {
    showToast(`退出失败：${e}`)
  } finally {
    accountBusy.value = false
  }
}

async function doRedeem() {
  if (!redeemInput.value.trim()) {
    showToast('请填写邀请码')
    return
  }
  accountBusy.value = true
  try {
    account.value = await tauriApi.accountRedeem(redeemInput.value.trim())
    redeemInput.value = ''
    showToast('兑换成功，权益已到账')
  } catch (e) {
    showToast(`兑换失败：${e}`)
  } finally {
    accountBusy.value = false
  }
}

async function doApply() {
  if (applyReason.value.trim().length < 10) {
    showToast('请至少写 10 个字说明你想做什么扩展')
    return
  }
  accountBusy.value = true
  try {
    await tauriApi.devApply(applyReason.value.trim())
    showApply.value = false
    applyReason.value = ''
    showToast('申请已提交，等待审核')
    await loadAccount()
  } catch (e) {
    if (await handleAuthError(e)) return
    showToast(`提交失败：${e}`)
  } finally {
    accountBusy.value = false
  }
}

function accountSummary(a: AccountStatus): string {
  if (a.error) return `暂时连不上服务器：${a.error}`
  if (a.developerStatus === 'approved') return '扩展开发者'
  if (a.developerStatus === 'pending') return '开发者申请审核中'
  if (a.developerStatus === 'rejected') return '开发者申请未通过'
  return a.inviteRedeemed ? '已兑换邀请码' : '尚未兑换邀请码'
}

onMounted(() => {
  void loadAccount()
  void loadGhLocal()
})
onBeforeUnmount(() => {
  stopGithubPolling()
  // 本地 GitHub 轮询也要停：面板切走时定时器若还活着，会在别的视图里继续发请求
  stopGhLocalPolling()
  if (emailTimer !== null) clearInterval(emailTimer)
})
</script>

<template>
        <section id="sv-sec-account" class="sv-sec" aria-label="账号">
          <h3 class="sv-sec-title">账号</h3>
          <!-- ============ 客户端直连 GitHub（不需要平台服务端） ============ -->
          <div v-if="isTauri()" class="setting-row">
            <div class="setting-info">
              <span class="setting-name">用 GitHub 登录</span>
              <span class="setting-desc">
                在浏览器里输入验证码即可，不用记密码。这一步由本机直接和 GitHub 通信，不经过 m-hub 服务器
              </span>
            </div>
            <button
              v-if="!ghLocal.loggedIn"
              class="ghost-btn data-btn"
              type="button"
              :disabled="ghLocalStarting || !!ghLocalDevice || ghLocal.needsClientId"
              :title="ghLocal.needsClientId ? '还没配置 GitHub Client ID，见下方说明' : ''"
              @click="startGhLocalLogin"
            >
              {{ ghLocalStarting ? '正在发起…' : ghLocalDevice ? '等待授权…' : '开始登录' }}
            </button>
            <button
              v-else
              class="ghost-btn data-btn"
              type="button"
              @click="ghLocalLogout"
            >
              退出登录
            </button>
          </div>

          <!-- 没配 Client ID 时说清是「缺配置」而不是「登录失败」 -->
          <p v-if="isTauri() && ghLocal.needsClientId" class="account-notice warn">
            还没配置 GitHub Client ID，所以暂时登不了。到 GitHub →
            Settings → Developer settings → OAuth Apps → New OAuth App 建一个
            （Authorization callback URL 填
            <code>https://github.com/login/oauth/callback</code>），
            把 Client ID 填进 <code>src-tauri/src/github_auth.rs</code> 的
            <code>GITHUB_CLIENT_ID</code> 后重新构建。设备码登录不需要 client_secret。
          </p>

          <!-- 已登录：显示身份，并说清哪些能力仍然不可用 -->
          <div v-if="isTauri() && ghLocal.loggedIn && ghLocal.identity" class="account-device">
            <img
              v-if="ghLocal.identity.avatarUrl"
              :src="ghLocal.identity.avatarUrl"
              class="account-avatar"
              alt=""
              referrerpolicy="no-referrer"
            />
            <div class="setting-info">
              <span class="setting-name">{{ ghLocal.identity.name }}</span>
              <span class="setting-desc">
                @{{ ghLocal.identity.login }}<template v-if="ghLocal.identity.email">
                  · {{ ghLocal.identity.email }}</template
                >
              </span>
            </div>
            <button class="ghost-btn" type="button" @click="tauriApi.openExternal(ghLocal.identity!.htmlUrl)">
              GitHub 主页
            </button>
          </div>

          <!-- 授权中：码 + 复制 + 打开浏览器 + 取消 -->
          <div v-if="ghLocalDevice" class="account-device">
            <span>浏览器里输入验证码</span>
            <b class="account-code">{{ ghLocalDevice.userCode }}</b>
            <button
              class="account-copy"
              type="button"
              title="复制验证码"
              aria-label="复制验证码"
              @click="copyText(ghLocalDevice.userCode, '验证码已复制')"
            >
              <Copy :size="13" :stroke-width="2" />
            </button>
            <button class="ghost-btn" type="button" @click="tauriApi.openExternal(ghLocalDevice.verificationUri)">
              打开浏览器
            </button>
            <button class="ghost-btn" type="button" @click="cancelGhLocalLogin">取消</button>
            <span class="dev-dir-warn">等待授权…</span>
          </div>

          <p
            v-if="ghLocalNotice"
            class="account-notice"
            :class="ghLocalNotice.kind"
            role="status"
          >
            {{ ghLocalNotice.text }}
          </p>

          <!-- 说清边界：GitHub 登录能给什么、不能给什么。宁可现在讲清，
               也不要让用户登录成功后去逐个试出「原来这些还是用不了」。 -->
          <p v-if="isTauri()" class="account-platform-note">
            <b>GitHub 登录能得到</b>：你的 GitHub 身份，以及一个可用的 GitHub 凭据。<br />
            <b>仍然需要 m-hub 服务器</b>：平台 AI 额度、申请扩展开发者、发布扩展、扩展市场。
            这些要由服务器签发会话，服务器不在线时无法使用。
          </p>

          <template v-if="account && !account.loggedIn">
            <!-- 下面这组是**平台**登录（经 m-hub 服务器），与上面那组必须分清：
                 上面拿到的是 GitHub 身份，这里拿的是平台会话（额度/开发者/发布）。
                 两者都叫「用 GitHub 登录」很容易被当成同一个功能。 -->
            <h4 class="account-subtitle">平台登录（需要 m-hub 服务器）</h4>
            <!-- 试过一次确认不通就置灰并说清原因：让用户反复点、每次吃一条红字，
                 而同屏上方「GitHub 登录已成功」是绿的，两条并存只会让人以为白登了。 -->
            <!-- ⚠️ 「未配置」与「服务不可用」分开说：前者重试无用（是配置状态），
                 后者该重试。混成一条会让用户以为反复点就能成。 -->
            <p v-if="platformUnconfigured" class="account-notice warn">
              {{ platformUnconfigured }}
            </p>
            <p v-else-if="platformDown" class="account-notice warn">
              平台服务当前不可用，平台登录已停用（不影响上面的 GitHub 登录）。平台能力包括
              AI 额度、申请扩展开发者、发布扩展、扩展市场 —— 这些都要服务器签发会话。
            </p>
            <div class="setting-row">
              <div class="setting-info">
                <span class="setting-name">用 GitHub 登录</span>
                <span class="setting-desc">在浏览器里输入验证码即可，不用记密码</span>
              </div>
              <button
                class="ghost-btn data-btn"
                type="button"
                :disabled="accountBusy || !!ghDevice || platformDown || !!platformUnconfigured"
                :title="
                  platformUnconfigured
                    ? '服务端未配置该功能所需凭证'
                    : platformDown
                      ? '平台服务当前不可用'
                      : ''
                "
                @click="startGithubLogin"
              >
                {{ ghStarting ? '正在发起…' : ghDevice ? '等待授权…' : '开始登录' }}
              </button>
            </div>
            <!-- 登录结果就地常驻（服务端代调 GitHub 要 10s 上下，只靠 2.2s 的 toast 等于没有反馈，见 loginNotice 注释） -->
            <p
              v-if="loginNotice"
              class="account-notice"
              :class="loginNotice.kind"
              :title="loginNotice.raw"
              role="status"
            >
              {{ loginNotice.text }}
            </p>
            <div v-if="ghDevice" class="account-device">
              <span>浏览器里输入验证码</span>
              <b class="account-code">{{ ghDevice.userCode }}</b>
              <button
                class="account-copy"
                type="button"
                title="复制验证码"
                aria-label="复制验证码"
                @click="copyText(ghDevice.userCode, '验证码已复制')"
              >
                <Copy :size="13" :stroke-width="2" />
              </button>
              <button
                class="ghost-btn"
                type="button"
                @click="tauriApi.openExternal(ghDevice.verificationUri)"
              >
                打开浏览器
              </button>
              <button
                class="ghost-btn"
                type="button"
                @click="cancelGithubLogin"
              >
                取消
              </button>
              <span class="dev-dir-warn">
                {{ ghSlow ? 'GitHub 要求降低频率，已自动放慢…' : '等待授权…' }}
              </span>
            </div>

            <!-- 邮箱验证码登录：国内网络下的兜底方式（GitHub 链路要服务端出网到境外，会间歇性失败）。
                 服务端发信已就绪；万一哪天又没配，Rust 侧会把 503 转成下面那条就地提示，不静默失败。 -->
            <div class="setting-row">
              <div class="setting-info">
                <span class="setting-name">用邮箱验证码登录</span>
                <span class="setting-desc">验证码 10 分钟内有效，收不到时先看看垃圾邮件</span>
              </div>
              <div class="account-inline">
                <input v-model="emailInput" class="account-input" placeholder="you@example.com" />
                <button
                  class="ghost-btn data-btn"
                  type="button"
                  :disabled="accountBusy || emailCooldown > 0 || platformDown"
                  :title="platformDown ? '平台服务当前不可用' : ''"
                  @click="sendEmailCode"
                >
                  {{ emailCooldown > 0 ? `${emailCooldown}s 后可重发` : emailSending ? '正在发送…' : '发验证码' }}
                </button>
              </div>
            </div>
            <div v-if="emailSent" class="account-inline account-inline-end">
              <input
                v-model="emailCode"
                class="account-input account-input-sm"
                placeholder="6 位验证码"
                maxlength="6"
              />
              <button class="ghost-btn data-btn" type="button" :disabled="accountBusy" @click="verifyEmailCode">
                登录
              </button>
            </div>
            <!-- 邮箱链路的结果就地常驻（发信要过 SMTP，且服务端按邮箱限流每小时 5 封，只靠 toast 会「点了没反应」） -->
            <p
              v-if="emailNotice"
              class="account-notice"
              :class="emailNotice.kind"
              :title="emailNotice.raw"
              role="status"
            >
              {{ emailNotice.text }}
            </p>
          </template>

          <template v-else-if="account?.loggedIn">
            <div class="setting-row">
              <div class="setting-info">
                <span class="setting-name">{{ account.username }}</span>
                <span class="setting-desc">{{ accountSummary(account) }}</span>
              </div>
              <button class="ghost-btn data-btn" type="button" :disabled="accountBusy" @click="doLogout">
                退出登录
              </button>
            </div>

            <div class="setting-row">
              <div class="setting-info">
                <span class="setting-name">AI 额度</span>
                <span class="setting-desc">
                  {{
                    platformAiUnavailable
                      ? '平台 AI 未配置上游 Key，暂不可用 —— 请改用「AI 助手」里自备供应商'
                      : account.inviteRedeemed
                        ? `剩余 ${account.quotaRemaining} 次（共 ${account.quotaTotal} 次）`
                        : '尚未兑换邀请码，当前没有额度'
                  }}
                </span>
              </div>
            </div>

            <div v-if="!account.inviteRedeemed" class="setting-row">
              <div class="setting-info">
                <span class="setting-name">兑换邀请码</span>
                <!--
                  ⚠️ 原文案「兑换后才发放额度」与实现不符：服务端兑换只置
                  `invite_redeemed = 1`，**不发任何额度**。实际额度是每天懒发放
                  （`ai.ts` 的 DAILY_GRANT，读时补行），不必兑换也每天有 —— 
                  兑换的作用是**准入**。
                  「显示能用却不能用 / 说错了却让人以为有」都比明确说清更糟。
                -->
                <span class="setting-desc">用于解锁平台 AI 额度与「申请成为开发者」；兑换本身不发额度</span>
              </div>
              <div class="account-inline">
                <input v-model="redeemInput" class="account-input" placeholder="邀请码" />
                <button class="ghost-btn data-btn" type="button" :disabled="accountBusy" @click="doRedeem">
                  兑换
                </button>
              </div>
            </div>

            <div class="setting-row">
              <div class="setting-info">
                <span class="setting-name">扩展开发者</span>
                <span class="setting-desc">
                  {{
                    account.developerStatus === 'approved'
                      ? '已通过：可在扩展中心发布扩展'
                      : account.developerStatus === 'pending'
                        ? '申请审核中'
                        : account.developerStatus === 'rejected'
                          ? '申请未通过（可在扩展中心重新提交）'
                          : account.canApplyDeveloper
                            ? '还没有申请'
                            : '先兑换邀请码才能申请'
                  }}
                </span>
              </div>
              <button
                v-if="account.canApplyDeveloper"
                class="ghost-btn data-btn"
                type="button"
                :disabled="accountBusy"
                @click="showApply = !showApply"
              >
                申请成为开发者
              </button>
            </div>

            <div v-if="showApply" class="account-apply">
              <textarea
                v-model="applyReason"
                class="account-textarea"
                rows="3"
                placeholder="说说你想做什么扩展、为什么需要相应权限（审核时看这段）"
              ></textarea>
              <div class="account-inline account-inline-end">
                <button class="ghost-btn" type="button" @click="showApply = false">取消</button>
                <button class="ghost-btn data-btn" type="button" :disabled="accountBusy" @click="doApply">
                  提交申请
                </button>
              </div>
            </div>

            <div class="setting-row">
              <div class="setting-info">
                <span class="setting-name">我的设备</span>
                <span class="setting-desc">
                  同一账号最多 {{ devicesMax }} 台同时在线（换机不会被顶下线）；设备丢失时可单独撤销
                </span>
              </div>
              <button class="ghost-btn" type="button" :disabled="devicesBusy" @click="loadDevices">
                刷新
              </button>
            </div>
            <p v-if="!devices.length" class="account-device-empty">暂无设备记录</p>
            <div v-for="d in devices" :key="d.id" class="account-device-item">
              <span class="account-device-name">
                {{ d.label || '未命名设备' }}
                <b v-if="d.current" class="account-device-self">本机</b>
              </span>
              <span class="account-device-meta">最近使用 {{ fmtDeviceTime(d.last_seen_at) }}</span>
              <button
                v-if="!d.current"
                class="ghost-btn"
                type="button"
                :disabled="devicesBusy"
                @click="revokeDevice(d.id)"
              >
                撤销
              </button>
              <span v-else class="account-device-meta">下线请用「退出登录」</span>
            </div>
          </template>
        </section>
</template>
