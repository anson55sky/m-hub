/**
 * 网页类速达目标的归一化与判定。
 *
 * ## 「网页」速达不只装网页：网络地址也存这里
 *
 * NAS 共享、FTP 这类地址点开是交给系统处理的（Finder / 传输工具），
 * 应用内浏览器打不开它们。所以 `kind='web'` 实际涵盖两类目标：
 *
 * · **普通网页**（`http` / `https`）→ 按设置走应用内浏览器或系统浏览器
 * · **网络地址**（`ftp` / `smb` / `afp` / `nfs` / `webdav` …）→ **只能**交给系统
 *
 * ⚠️ 这两类必须在这里**分清**，因为归一化会破坏它们：原来的实现是
 * 「`scheme://` 前缀一律剥掉、默认补 `http://`」——
 * `ftp://nas.local/files` 会被改成 `http://nas.local/files`，
 * 于是点开变成「用浏览器访问一个不存在的 http 站点」。
 * 症状是「存得进、打不开、报的是网络错误」，而用户存的时候看不出任何异常。
 */

/** 交给系统处理的网络地址协议（macOS 能通过 `open` 直接接管） */
export const NETWORK_SCHEMES = [
  'ftp',
  'ftps',
  'smb',
  'smbv',
  'afp',
  'nfs',
  'webdav',
  'dav',
  'webdavs',
] as const

/** 取协议名（小写、无冒号）。认不出时返回 `null` —— 而不是猜一个。 */
export function schemeOf(input: string): string | null {
  const m = /^\s*([a-z][a-z0-9+.-]*):\/\//i.exec(input)
  return m ? m[1]!.toLowerCase() : null
}

/** 是网络地址（NAS / FTP / …）吗？这类**只能**交给系统打开 */
export function isNetworkAddress(target: string): boolean {
  const s = schemeOf(target)
  return s !== null && (NETWORK_SCHEMES as readonly string[]).includes(s)
}

/** 是应用内浏览器打得开的网页吗 */
export function isHttpUrl(target: string): boolean {
  const s = schemeOf(target)
  return s === 'http' || s === 'https'
}

/**
 * 归一化用户输入的网址。
 *
 * ⚠️ **网络地址原样保留**，不剥协议、不补 `http://`（见文件头）。
 * ⚠️ 其它协议（`javascript:` / `data:` / `file:`）一律**剥掉前缀**按没写协议处理：
 *   那不是网址，剥掉是最不容易出错的做法；`javascript:` 尤其不能留着 ——
 *   它后面会交给 `openExternal`，留着等于给了一个伪协议注入面。
 */
export function normalizeWebUrl(input: string): string {
  const trimmed = input.trim()
  if (/^https?:\/\//i.test(trimmed)) return trimmed
  // 网络地址：原样返回（大小写与路径都不能动）
  if (isNetworkAddress(trimmed)) return trimmed
  const host = trimmed.replace(/^[a-z][a-z0-9+.-]*:\/\//i, '')
  return `http://${host}`
}

/** 从完整网址推导站点 favicon 地址。网络地址没有 favicon → 返回 null */
export function deriveFaviconUrl(target: string): string | null {
  if (isNetworkAddress(target)) return null
  try {
    const url = new URL(target)
    return `${url.origin}/favicon.ico`
  } catch {
    return null
  }
}

/** 表单里的提示文案：网络地址与网页要分开说 */
export function webTargetHint(): string {
  return '如：github.com、nas.local/share（smb://）、ftp://例子.com'
}
