/**
 * 工作台「有效版面」的几何计算（纯函数，无 Vue 依赖，便于验证）
 *
 * ## 要解决的问题
 *
 * 空模块照样占着整块地。提示词列表绝大多数时候是空的，而它存在的价值是
 * 「有东西时才用」—— 所以那块地长期空着。
 *
 * 但**不能直接改用户存下来的布局**：那是他的版面，等他加了内容还得能原样长回去。
 * 所以这里的做法是**派生**一份「有效版面」：保存的布局不动，渲染时按当前内容量算一份。
 *
 * ## 关键决策：整段折叠，而不是逐个模块折叠
 *
 * 第一版是「把每个空模块压到 1 行，再让上方模块下落填补空洞」（逐个上吸）。
 * 结果用户看着**不整齐**，两处原因都能复现：
 *
 *  · **空洞**。上吸只能把上面的模块往下拉，模块之间原本不齐的底边就露出空缺。
 *  · **错边**。上吸是逐个模块独立找位置的：`countdown` 占着 y=0–4，于是 `sticky1`
 *    挪不动而 `sticky2` 能挪到 y=3 —— **两张便签一高一低**，最刺眼的一处。
 *
 * 「部分折叠」在手工摆放的栅格上**必然**产生这两者：让一个模块变矮，别的模块
 * 要么留下洞、要么也得跟着动，而一动就对不齐。
 *
 * 所以改成：**按横段折叠**。同一个 `y` 上的模块算一段（推荐布局就是四段），
 * 段内**全部**成员都可降级且都为空时，整段一起压成 1 行；然后各段按新高度重排 y。
 *
 * 这样得到的版面永远满足三条：
 *   · 段内各模块等高（顶边齐、底边齐）
 *   · 段与段依次紧贴（没有空洞）
 *   · 折叠是**整段**的（不会出现一段里有的缩了有的没缩）
 *
 * ## 代价（明确的取舍，不是遗漏）
 *
 * 一段里只要有**一个**成员不参与降级，整段就整体不折叠。目前：
 *   · 第 1 段（时钟 / 天气 / 系统资源）全都不降级 → 永远不折叠
 *   · 第 2 段（待办 / 倒计时 / 提示词）全可降级 → 三样都空时整段折叠
 *   · 第 3 段（含两张便签）→ 永远不折叠（便签空着才该让人写字，压扁等于不让写）
 *   · 第 4 段（最近使用）→ 空时折叠
 *
 * 「待办有内容、提示词为空，于是只把提示词压扁」这种**部分**回收是做不到的 ——
 * 代价是可见的错边，收益只是多一点空间。这笔账不划算，所以不做。
 */

/** 空内容时压到的高度（1 行，够放一条单行条） */
export const EMPTY_ROW_H = 1

/**
 * 哪些模块在「没有内容」时值得降级。
 *
 * 只列**列表型**模块：它们的内容为空时整块地确实没有意义。
 * 不列的（及其理由）：
 *   · `clock` / `weather` / `sysmon` —— 环境信息，「空」不是它们的状态。
 *     天气断网时显示的是 `----` 占位而非空白（实测确认过），也就是说它**有**
 *     一个合理的空态，把那个占位压成一行反而丢信息。
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

/**
 * 几何计算只需要「矩形」这一件事。
 *
 * 刻意**不**在这里复制一份完整的 `DashPlacement`（那会让本模块与
 * `useDashboardLayout` 里的类型各写一份、字段一改就两边不一致 —— 正是本工程
 * 反复在治的那类「同一份东西的第二份拷贝」）。改用泛型约束：调用方把自己的
 * `DashPlacement` 传进来即可。顺带把几何算法与「模块有哪些字段」解耦。
 */
export interface Rect {
  x: number
  y: number
  w: number
  h: number
}

/** 两个矩形是否相交（栅格是离散的整数格，所以用整数比较） */
export function rectsOverlap(a: Rect, b: Rect): boolean {
  return a.x < b.x + b.w && a.x + a.w > b.x && a.y < b.y + b.h && a.y + a.h > b.y
}

/**
 * 把布局按 `y` 聚成横段，各段按 `y` 升序。
 *
 * 以 `y` 聚合是刻意的：**同一段 = 顶边对齐的一排**，这正是视觉上的「一行」。
 * 段内高度是否一致不在这里判定（自定义布局可能不齐），折叠时才会用到。
 */
export function toBands<T extends { id: string } & Rect>(items: T[]): T[][] {
  const byY = new Map<number, T[]>();
  for (const p of items) {
    const arr = byY.get(p.y);
    if (arr) arr.push(p);
    else byY.set(p.y, [p]);
  }
  return [...byY.entries()].sort((a, b) => a[0] - b[0]).map(([, v]) => v);
}

/** 某一段是否可以整段折叠：全部成员可降级，且全部为空 */
export function bandCollapses<T extends { id: string } & Rect>(
  band: T[],
  isEmpty: (id: string) => boolean,
): boolean {
  if (band.length === 0) return false;
  return band.every((p) => DEGRADABLE_IDS.has(p.id) && isEmpty(p.id));
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
  const bands = toBands(saved);
  const collapsedBands = bands.filter((b) => bandCollapses(b, isEmpty));
  if (collapsedBands.length === 0) {
    // 一个段都没折 → 逐格返回原样，绝不改动用户的版面
    return { placements: saved.map((p) => ({ ...p })), compacted: false };
  }

  const out: T[] = [];
  let y = 0;
  for (const band of bands) {
    const fold = bandCollapses(band, isEmpty);
    // 段高取段内最大值：非折叠时逐个保留原 h，折叠时一律 1 行。
    // 折叠段里成员原本的 h 不参与 —— 反正要压扁成一样高。
    const height = fold ? EMPTY_ROW_H : Math.max(...band.map((p) => p.h));
    for (const p of band) {
      out.push({ ...p, y, h: fold ? EMPTY_ROW_H : p.h });
    }
    y += height;
  }
  return { placements: out, compacted: true };
}
