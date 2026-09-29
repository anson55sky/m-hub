/**
 * 平台判定与快捷键显示格式化的**单一真相源**。
 *
 * 内部一律存 Tauri 能解析的写法（`CommandOrControl+Control+V`），只有**显示**时
 * 才转成符号。转换散落在各处是重复劳动，也是 bug 温床：同一个快捷键在标题栏
 * 显示成 `⌃⌘V`、在设置页显示成 `CommandOrControl+Control+V`，用户会以为是两个键。
 * 改格式只改这里一处。
 */

/**
 * 是否 macOS。`navigator.platform` 在新版 Safari 已废弃，故与 userAgent 双判。
 *
 * ⚠️ 必须容忍 `navigator` **不存在**。本模块原先假定自己只在浏览器里跑，
 * 于是裸读 `navigator`；而 `tests/security.test.mjs` 是在 `node:vm` 沙箱里
 * 加载模块的（没有 `navigator`），`stores/workbench.ts` 一旦 import 本文件
 * 就直接把那条测试打挂（`ReferenceError: navigator is not defined`）。
 * 判平台本身就该对「拿不到 navigator」稳健 —— 缺失时按非 mac 处理。
 */
const NAV = typeof navigator !== 'undefined' ? navigator : undefined
export const IS_MAC =
  !!NAV && (/Mac|iPhone|iPad/.test(NAV.userAgent) || /Mac|iPhone|iPad/.test(NAV.platform))

/** 别名：Vue 模板里读起来比全大写常量顺 */
export const isMac = IS_MAC

/** 系统级术语：托盘在 macOS 上叫「菜单栏」，别让用户去找一个不存在的「托盘」 */
export const TRAY_TERM = IS_MAC ? '菜单栏' : '系统托盘'

/** 「最小化到 X」里的那个 X —— 标题栏 / toast / 设置页共用 */
export function trayLabel(): string {
  return IS_MAC ? `最小化到${TRAY_TERM}` : '最小化到系统托盘'
}

/**
 * 四个全局快捷键的默认值（内部写法）——`src-tauri/src/shortcut.rs` 四个
 * `DEFAULT_*` 常量的**镜像**。改默认值必须两边一起改，故有
 * `scripts/check-default-shortcuts.mjs` 在 prebuild 锁死。
 *
 * ## 为什么要镜像
 *
 * 设置页的 placeholder、搜索弹窗的 `<kbd>`、AI 对话空视图的提示，都要回答
 * 「用户还没改过快捷键时该显示什么」——那时配置里可能还是空串，前端读不到
 * Rust 的常量。
 *
 * ## 为什么**每个键存两个平台的值**
 *
 * `shortcut.rs` 的常量是 `#[cfg(target_os = …)]` **分叉**的，且两个平台
 * **本来就允许不同**（剪贴板就是：macOS 用 `⌃⌘V`、Windows 用 `Ctrl+\``，
 * 因为 `⌘⌥V` 撞系统键而 `Ctrl+Shift+V` 是 Windows 终端的无格式粘贴）。
 * 早先这里只存一份 macOS 的值，于是「一份镜像服务两个平台」这个前提不成立：
 * Windows 用户看到的占位文案会是 macOS 的键。
 */
const DEFAULT_SHORTCUT_VARIANTS = {
  /** 主窗显隐。macOS 用 `⌘⇧Space`（不带 Shift 的 `⌘Space` 是 Spotlight，不冲突） */
  toggle: { mac: 'CommandOrControl+Shift+Space', other: 'Ctrl+Shift+Space' },
  /**
   * 剪贴板浮层。
   *
   * ⚠️ macOS 刻意**不是** `CommandOrControl+Alt+V`（⌘⌥V）：那是 macOS 自带的
   * 「粘贴并匹配样式」，属系统级注册，第三方应用抢不到 —— 抢不到的表现是
   * **静默失效**（注册失败没人看）。`Control+V`（⌃⌘V）无冲突，且是 Raycast /
   * Pastebot 一系的行业惯例。
   */
  clipboard: { mac: 'CommandOrControl+Control+V', other: 'Ctrl+`' },
  /** 全局搜索 */
  search: { mac: 'CommandOrControl+K', other: 'Ctrl+K' },
  /** AI 对话 */
  chat: { mac: 'CommandOrControl+Shift+K', other: 'Ctrl+Shift+K' },
  /**
   * 统一捕获（2026-09-29 新增）：从任何地方一行记下东西，自动路由到
   * 速记/待办/提示词/倒计时/速达。
   *
   * 键位与 shortcut.rs 的 `DEFAULT_CAPTURE_SHORTCUT` 对齐，由
   * `scripts/check-default-shortcuts.mjs` 锁死。
   */
  capture: { mac: 'CommandOrControl+Shift+U', other: 'Ctrl+Shift+U' },
} as const

export type ShortcutKey = keyof typeof DEFAULT_SHORTCUT_VARIANTS

/** 当前平台生效的默认值 */
export const DEFAULT_SHORTCUTS = Object.fromEntries(
  (Object.keys(DEFAULT_SHORTCUT_VARIANTS) as ShortcutKey[]).map((k) => [
    k,
    IS_MAC ? DEFAULT_SHORTCUT_VARIANTS[k].mac : DEFAULT_SHORTCUT_VARIANTS[k].other,
  ]),
) as Record<ShortcutKey, string>

/**
 * 取某个快捷键的**显示**文本（内部写法 → 符号/展开）。
 * 用户改过就用改后的，没改（或读到空串）回落到默认值。
 */
export function shortcutLabel(
  key: keyof typeof DEFAULT_SHORTCUTS,
  configured?: string | null,
): string {
  return prettyShortcut(configured, prettyShortcut(DEFAULT_SHORTCUTS[key]))
}

/**
 * 右键这个词在 macOS 上的说法。
 *
 * Mac 触控板没有右键键：两指点按或 ⌃+点按 才是「右键」，而没有任何界面会
 * 告诉用户这件事。直接把「右键」两个字摆出来，用户会在触控板上试半天才发现。
 */
export const RIGHT_CLICK_TERM = IS_MAC ? '右键（触控板双指点按 / ⌃ 点按）' : '右键'

/**
 * 「以管理员身份运行」的说法。
 *
 * macOS 没有 UAC：那是一次**显式授权**（系统弹密码框）。说成「管理员身份运行」
 * 会让用户以为是 Windows 那个几乎无感的 UAC 提权。
 */
export const ADMIN_LAUNCH_TERM = IS_MAC ? '以管理员权限打开（需输入密码）' : '以管理员身份运行'

/**
 * 把内部写法转成给人看的字符串。
 *
 * - macOS：`CommandOrControl+Control+V` → `⌃⌘V`，空格 → `␣`，回车 → `↩`
 * - 其他平台：保持 `CommandOrControl` 展开成 `Ctrl`（Windows 用户不认 ⌘）
 *
 * 顺序按 macOS 习惯（⌃⌥⇧⌘）重排，不是简单替换——`Control+Command+V` 念出来是
 * 「⌃⌘V」而不是「⌘⌃V」。
 */
export function prettyShortcut(raw: string | null | undefined, fallback = ''): string {
  if (!raw) return fallback
  const parts = raw.split('+').map((p) => p.trim()).filter(Boolean)
  if (!parts.length) return fallback

  if (!IS_MAC) {
    return parts.map((p) => (p === 'CommandOrControl' ? 'Ctrl' : p)).join('+')
  }

  const ALIASES: Record<string, string> = {
    CommandOrControl: '⌘',
    CmdOrCtrl: '⌘',
    Cmd: '⌘',
    Command: '⌘',
    Meta: '⌘',
    Super: '⌘',
    Control: '⌃',
    Ctrl: '⌃',
    Alt: '⌥',
    Option: '⌥',
    Shift: '⇧',
    Space: '␣',
    Enter: '↩',
    Return: '↩',
    Backspace: '⌫',
    Delete: '⌦',
    Escape: '⎋',
    Up: '↑',
    Down: '↓',
    Left: '←',
    Right: '→',
  }
  const MOD_ORDER = ['⌃', '⌥', '⇧', '⌘']
  const mods: string[] = []
  const keys: string[] = []
  for (const p of parts) {
    const sym = ALIASES[p] ?? p
    if (MOD_ORDER.includes(sym)) mods.push(sym)
    else keys.push(sym)
  }
  return [...MOD_ORDER.filter((m) => mods.includes(m)), ...keys].join('')
}
