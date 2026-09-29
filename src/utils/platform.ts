/**
 * 平台判定与快捷键显示格式化的**单一真相源**。
 *
 * 内部一律存 Tauri 能解析的写法（`CommandOrControl+Control+V`），只有**显示**时
 * 才转成符号。转换散落在各处是重复劳动，也是 bug 温床：同一个快捷键在标题栏
 * 显示成 `⌃⌘V`、在设置页显示成 `CommandOrControl+Control+V`，用户会以为是两个键。
 * 改格式只改这里一处。
 */

/** 是否 macOS。`navigator.platform` 在新版 Safari 已废弃，故与 userAgent 双判 */
export const IS_MAC =
  /Mac|iPhone|iPad/.test(navigator.userAgent) || /Mac|iPhone|iPad/.test(navigator.platform)

/** 别名：Vue 模板里读起来比全大写常量顺 */
export const isMac = IS_MAC

/** 系统级术语：托盘在 macOS 上叫「菜单栏」，别让用户去找一个不存在的「托盘」 */
export const TRAY_TERM = IS_MAC ? '菜单栏' : '系统托盘'

/** 「最小化到 X」里的那个 X —— 标题栏 / toast / 设置页共用 */
export function trayLabel(): string {
  return IS_MAC ? `最小化到${TRAY_TERM}` : '最小化到系统托盘'
}

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
