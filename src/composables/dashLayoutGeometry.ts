/**
 * 工作台「有效版面」的几何计算（纯函数，无 Vue 依赖，便于日后单测）
 *
 * ## 要解决的问题
 *
 * 默认布局里，空模块照样占着整块地。实测截图：`prompts` 是 5×8 = 40 格，
 * 而它的 ideal 只有 4×3 = 12 格，于是「暂无提示词」长期占着全板最大的一块。
 * 全板算下来，空内容模块占掉约四成面积。
 *
 * 一个没话说的模块不该占这么大地方 —— 但也不能直接改用户存下来的布局：
 * 那是他的版面，等他加了内容还得能原样长回去。所以这里的做法是
 * **派生**一份「有效版面」：保存的布局不动，渲染时按当前内容量算一份。
 *
 * ## 两条规则
 *
 * ① **压扁**：可降级且当前为空的模块，高度压到 1 行（够放一条单行条）。
 * ② **上吸**：压扁留下的空洞由上方模块下落填补，让整块板随内容收缩。
 *
 * ## 为什么②只在真的压扁过时才做
 *
 * 上吸会**关掉用户刻意留的空白**。若无论有没有空模块都吸，用户在
 * 「布局编辑器」里排好的呼吸感会莫名其妙消失。所以：只有当①真的压扁了
 * 至少一个模块时才吸；板子是满的、或用户一个空模块都没有时，
 * 有效版面与保存布局**逐格相同**，不产生任何视觉变化。
 */

/**
 * 几何计算只需要「矩形」这一件事。
 *
 * 刻意**不**在这里复制一份完整的 `DashPlacement`（那会让本模块与
 * `useDashboardLayout` 里的类型各写一份、字段一改就两边不一致 ——
 * 正是本工程反复在治的那类「同一份东西的第二份拷贝」）。
 * 改用泛型约束：调用方把自己的 `DashPlacement` 传进来即可，
 * 本模块只要求它满足 `Rect`。顺带把几何算法与「模块有哪些字段」解耦。
 */
export interface Rect {
  x: number
  y: number
  w: number
  h: number
}

/** 空内容时压到的高度（1 行，够放一条单行条） */
export const EMPTY_ROW_H = 1

/**
 * 哪些模块在「没有内容」时值得降级。
 *
 * 只列**列表型**模块：它们的内容为空时整块地确实没有意义。
 * 不列的（及其理由）：
 *   · `clock` / `weather` / `sysmon` —— 它们展示的是环境信息，
 *     「空」不是它们的状态（无网络时天气会显示占位而非空白）。
 *   · `sticky1/2` —— 便签本来就是用来写字的，空着才该让人写字，
 *     压扁成一行等于不让写。
 *   · `suda1-4` —— 自定义速达槽位，同理。
 *   · 扩展 module（`ext:` 前缀）—— 不知道扩展内部会渲染什么，
 *     越俎代庖地替它决定「空」是不安全的。
 */
export const DEGRADABLE_IDS = new Set([
  'prompts',
  'todo',
  'todo_overview',
  'countdown',
  'notes',
  'recent',
  'resources',
])

/** 两个矩形是否相交（栅格是离散的整数格，所以用整数比较） */
export function rectsOverlap(a: Rect, b: Rect): boolean {
  return a.x < b.x + b.w && a.x + a.w > b.x && a.y < b.y + b.h && a.y + a.h > b.y
}

/**
 * 把每个模块**尽量往上挪**，直到再往上一行就会与已定位的模块相撞。
 *
 * 按 `y` 升序处理：先处理的一定在原布局里更靠上，于是「已定位」的那批
 * 天然都在当前模块之上，不会出现「挪下去之后又被下面的顶回来」。
 * 相对顺序与 `x` 坐标完全保持，只动 `y`。
 */
export function pullUp<T extends Rect>(items: T[]): T[] {
  const placed: T[] = []
  for (const p of [...items].sort((a, b) => a.y - b.y || a.x - b.x)) {
    let cur: T = { ...p }
    while (cur.y > 0) {
      const up: T = { ...cur, y: cur.y - 1 }
      if (placed.some((q) => rectsOverlap(up, q))) break
      cur = up
    }
    placed.push(cur)
  }
  return placed
}

/**
 * 由保存布局派生「有效版面」。
 *
 * @param saved 用户保存（或默认预设）的布局
 * @param isEmpty 判定「某模块当前是否为空」的函数；返回 false 即不降级
 * @returns `{ placements, compacted }` —— `compacted` 供调用方决定要不要提示用户
 */
export function effectiveLayout<T extends { id: string } & Rect>(
  saved: T[],
  isEmpty: (id: string) => boolean,
): { placements: T[]; compacted: boolean } {
  const degradableEmpty = new Set(
    saved.filter((p) => DEGRADABLE_IDS.has(p.id) && isEmpty(p.id)).map((p) => p.id),
  )
  if (degradableEmpty.size === 0) {
    // 一个都没压扁 → 逐格返回原样，绝不改动用户的版面
    return { placements: saved.map((p) => ({ ...p })), compacted: false }
  }
  const shrunk = saved.map((p) =>
    degradableEmpty.has(p.id) ? { ...p, h: Math.min(p.h, EMPTY_ROW_H) } : { ...p },
  )
  return { placements: pullUp(shrunk), compacted: true }
}
