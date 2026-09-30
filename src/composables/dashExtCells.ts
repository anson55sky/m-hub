/**
 * 工作台「扩展格子」的对账纯函数。
 *
 * ## 为什么单独成文件
 *
 * 这个函数必须能被**单测直接跑生产代码本身**，而 `useDashboardLayout.ts` 在模块
 * 顶层就 `useStore()` 并 import 了 Tauri API —— Node 里 import 它会直接炸，
 * 于是测试只能「照着逻辑抄一份」，而抄出来的那份会跟实现漂移（本工程已经吃过
 * 一次亏：扩展 id 校验被写了两份实现，两边悄悄分叉）。
 *
 * 所以规则本身放在这里：零依赖、零副作用、可直接 import。
 */

/**
 * 已保存布局里**当前注册表里找不到**的 `ext:` 格子 id。
 *
 * ## 这条规则在防什么（一次真实的数据丢失）
 *
 * 恢复布局时 `parsePlacements` 会按 `dashModuleDef(id)` 过滤格子。而
 * `loadExtensionModules()` 里的 `listExtensions()` 在启动早期可能**还没就绪**而抛错，
 * 此时扩展表是空的 —— 用户布局里的 `ext:` 格子被**静默丢掉**。
 *
 * 丢掉本身还只是这一次显示不对；致命的是它紧接着会被**永久写掉**：
 * `persist()` 写的是当前 `placements`，所以用户之后任何一次无关改动
 * （挪一张卡、删一条倒计时）都会把整份列表连同这些格子一起存回去，删得干干净净。
 * 表现为「装好的本机扩展卡片某次重启后自己从工作台消失了」，且没有任何报错。
 *
 * 所以「布局里有 ext: 格子却没解析出来」必须能被识别出来并触发重试，
 * 而不是当成「这些扩展已经被卸载了」。
 *
 * @param raw        `dashboard_layout` 的原始 JSON 字符串
 * @param registered 当前已注册的扩展模块 id（如 `ext:local.calculator`）
 */
export function unresolvedExtIds(raw: string, registered: string[]): string[] {
  let parsed: unknown
  try {
    parsed = JSON.parse(raw)
  } catch {
    // 布局 JSON 坏了不是这里要管的事（parsePlacements 会回退到默认布局）
    return []
  }
  if (!Array.isArray(parsed)) return []
  const have = new Set(registered)
  const out: string[] = []
  for (const item of parsed) {
    const id = item && typeof item === 'object' ? (item as { id?: unknown }).id : undefined
    if (typeof id !== 'string' || !id.startsWith('ext:')) continue
    if (!have.has(id) && !out.includes(id)) out.push(id)
  }
  return out
}

/**
 * 扩展注册表重试间隔（毫秒）。
 *
 * 覆盖的是「后端命令注册晚于前端首帧」这一种竞态：主窗很快就开始恢复布局，
 * 而扩展扫描/代理要更晚才就绪。间隔递增而不是等长，是因为越晚越接近就绪；
 * 最后一档 3s 已经足够，再长只会让「扩展真被卸载了」的用户白等。
 */
export const EXT_REGISTRY_RETRY_DELAYS_MS = [150, 400, 900, 1800, 3000]
