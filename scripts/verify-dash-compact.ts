/**
 * `dashLayoutGeometry` 的验证脚本（不进 prebuild，手动跑）。
 *
 * 跑法：node --experimental-strip-types scripts/verify-dash-compact.ts
 *
 * ## 为什么核心断言是「整段」而不是「单个模块」
 *
 * 第一版几何是「把每个空模块压扁 + 逐个上吸」，用户看着不整齐：空洞、错边。
 * 现在改成**整段折叠 + 各段重排**。所以断言的重点从「压扁了几个」变成了
 * 「折叠后版面还是否整齐」——
 *   · 段内各模块**等高**（顶边齐、底边齐）
 *   · 各段**依次紧贴**（没有空洞）
 *   · 折叠只整段发生（不会出现一段里有的缩了有的没缩）
 *
 * 这三样正是「整齐」的可校验形式，也是用户会直接看到的东西。
 *
 * 验的是**代码本身**而非复制一份逻辑：直接跑真实的 .ts 源文件
 * （Node 22 自带类型擦除），PRESET 也是从源码里解析出来的。
 */
import {
  effectiveLayout,
  toBands,
  bandCollapses,
  DEGRADABLE_IDS,
  type Rect,
} from '../src/composables/dashLayoutGeometry.ts'
import { readFileSync } from 'node:fs'

/** 从真实源码里解析出 PRESET（不复制一份，避免两边漂移） */
function loadPreset(): { id: string; variant: string; x: number; y: number; w: number; h: number }[] {
  const src = readFileSync(
    new URL('../src/composables/useDashboardLayout.ts', import.meta.url),
    'utf8',
  )
  const block = src.slice(src.indexOf('const PRESET'), src.indexOf('function defaultPlacements'))
  return [...block.matchAll(
    /\{\s*id:\s*'([a-z0-9]+)',\s*variant:\s*'([a-z]+)',\s*x:\s*(\d+),\s*y:\s*(\d+),\s*w:\s*(\d+),\s*h:\s*(\d+)\s*\}/g,
  )].map((m) => ({ id: m[1], variant: m[2], x: +m[3], y: +m[4], w: +m[5], h: +m[6] }))
}

let failed = 0
function check(name: string, cond: boolean, detail = '') {
  if (cond) {
    console.log(`  ✓ ${name}`)
  } else {
    console.error(`  ✗ ${name}${detail ? '  ' + detail : ''}`)
    failed++
  }
}

/** 逐行覆盖的列数；返回 [最短的一行, 覆盖数] */
function worstRow(ps: Rect[]): { row: number; cols: number } {
  const rows = Math.max(...ps.map((p) => p.y + p.h))
  let worst = { row: -1, cols: 99 }
  for (let y = 0; y < rows; y++) {
    const used = new Set<number>()
    for (const p of ps) {
      if (p.y <= y && y < p.y + p.h) for (let x = p.x; x < p.x + p.w; x++) used.add(x)
    }
    if (used.size < worst.cols) worst = { row: y, cols: used.size }
  }
  return worst
}

/** 某排内是否有高度不一致（不齐 = 错边） */
function raggedBands(ps: Rect[]): number[] {
  const byY = new Map<number, Rect[]>()
  for (const p of ps) {
    if (!byY.has(p.y)) byY.set(p.y, [])
    byY.get(p.y)!.push(p)
  }
  return [...byY.entries()].filter(([, b]) => new Set(b.map((p) => p.h)).size > 1).map(([y]) => y)
}

const maxRow = (ps: Rect[]) => Math.max(...ps.map((p) => p.y + p.h))

const PRESET = loadPreset()
const PRESET_BANDS = toBands(PRESET)
console.log(
  `PRESET 载入 ${PRESET.length} 个模块 / ${PRESET_BANDS.length} 段：` +
    PRESET_BANDS.map((b) => `y=${b[0].y}(${b.map((p) => p.id).join(',')})`).join('  '),
)
console.log()

// ---- ① 折叠前：版面本来就是整齐的（守卫 check-dash-preset 也守着这条）----
console.log('① 折叠前 PRESET 本身逐行铺满、段内等高')
{
  check('每行都覆盖 12 列', worstRow(PRESET).cols === 12, `最差的是第 ${worstRow(PRESET).row} 行，只覆盖 ${worstRow(PRESET).cols} 列`)
  check('没有高度不一致的段', raggedBands(PRESET).length === 0, `y=${raggedBands(PRESET)}`)
}

// ---- ② 什么都不空时：逐格不变 --------------------------------------------
console.log('\n② 全部模块都有内容 → 版面必须逐格不变')
{
  const { placements, compacted } = effectiveLayout(PRESET, () => false)
  check('compacted = false', !compacted)
  const same =
    placements.length === PRESET.length &&
    placements.every((p, i) => {
      const q = PRESET[i]
      return p.id === q.id && p.x === q.x && p.y === q.y && p.w === q.w && p.h === q.h
    })
  check('每个模块的 x/y/w/h 全部不变', same)
}

// ---- ③ 核心：折叠后仍然整齐 ----------------------------------------------
console.log('\n③ 折叠后仍然整齐（用户直接看到的就是这两条）')
{
  const degradables = PRESET.filter((p) => DEGRADABLE_IDS.has(p.id)).map((p) => p.id)
  // 穷举各种空/满组合（本 PRESET 里可降级模块 5 个 → 32 种）
  let combos = 0
  let holes = 0
  let ragged = 0
  let partial = 0
  for (let mask = 0; mask < 1 << degradables.length; mask++) {
    const empty = new Set(degradables.filter((_, i) => mask & (1 << i)))
    const { placements } = effectiveLayout(PRESET, (id) => empty.has(id))
    combos++
    const w = worstRow(placements)
    if (w.cols !== 12) {
      holes++
      if (holes === 1) console.error(`      首例空洞：第 ${w.row} 行只覆盖 ${w.cols}/12（空模块 ${[...empty].join(',') || '无'}）`)
    }
    const r = raggedBands(placements)
    if (r.length) {
      ragged++
      if (ragged === 1) console.error(`      首例错边：y=${r.join(',')}（空模块 ${[...empty].join(',') || '无'}）`)
    }
    // 折叠必须整段：同一段里不能有的 h=1 有的 h>1
    for (const band of toBands(placements)) {
      const hs = new Set(band.map((p) => p.h))
      if (hs.size !== 1) { partial++; break }
    }
  }
  check(`${combos} 种空/满组合：逐行都铺满 12 列（无空洞）`, holes === 0, `${holes} 种有空洞`)
  check(`${combos} 种组合：段内始终等高（无错边）`, ragged === 0, `${ragged} 种有错边`)
  check(`${combos} 种组合：折叠只整段发生`, partial === 0, `${partial} 种出现半段折叠`)
}

// ---- ④ 折叠确实发生，且只发生在该折的段 --------------------------------
console.log('\n④ 折叠只发生在「全部成员可降级且全空」的段')
{
  // 第 2 段 = 待办 / 倒计时 / 提示词，三样全空才折
  const band2 = PRESET_BANDS[1]
  const band2Ids = band2.map((p) => p.id)
  console.log(`      第 2 段成员：${band2Ids.join('、')}`)

  const allEmpty = new Set(band2Ids)
  const folded = effectiveLayout(PRESET, (id) => allEmpty.has(id))
  // ⚠️ toBands 每次调用都返回**新的数组**，所以只能取一次、复用。
  // 之前这里调了两次再拿 `b !== b2` 去排除，引用恒不相等 → 第 2 段自己也被
  // 当成「其余段」检查，而它已经是 h=1，于是报出一个并不存在的失败。
  const foldedBands = toBands(folded.placements)
  const b2 = foldedBands.find((b) => b[0].id === band2Ids[0])!
  check('三样全空 → 整段压成 1 行', b2.every((p) => p.h === 1))
  check(
    '其余段高度不变',
    foldedBands.filter((b) => b[0].id !== band2Ids[0]).every((b) => b.every((p) => p.h > 1)),
  )
  check('总行数随之下缩', maxRow(folded.placements) < maxRow(PRESET), `${maxRow(PRESET)} → ${maxRow(folded.placements)}`)

  // 只空一个 → **不折**（这就是「部分折叠不做」的落点）
  const oneEmpty = new Set([band2Ids[0]])
  const partial = effectiveLayout(PRESET, (id) => oneEmpty.has(id))
  check(
    `只空「${band2Ids[0]}」→ 整段不折（部分折叠会造出错边，所以不做）`,
    !partial.compacted,
    'compacted 应为 false',
  )
  check('版面逐格不变', partial.placements.every((p, i) => p.y === PRESET[i].y && p.h === PRESET[i].h))

  // 段里有不参与降级的成员 → 永不折叠
  for (const band of PRESET_BANDS) {
    const hasFixed = band.some((p) => !DEGRADABLE_IDS.has(p.id))
    if (!hasFixed) continue
    const r = effectiveLayout(PRESET, () => true) // 全部判空，最激进
    const b = toBands(r.placements).find((x) => x[0].id === band[0].id)!
    check(
      `含固定成员的段（${band.map((p) => p.id).join('、')}）即使全空也不折`,
      b.every((p) => p.h > 1),
    )
  }
}

// ---- ⑤ bandCollapses 的判定边界 ----------------------------------------
console.log('\n⑤ bandCollapses 的判定')
{
  const band = [
    { id: 'todo', x: 0, y: 0, w: 4, h: 4 },
    { id: 'countdown', x: 4, y: 0, w: 4, h: 4 },
  ]
  check('全部可降级且全空 → 可折', bandCollapses(band, () => true))
  check('有一个不空 → 不可折', !bandCollapses(band, (id) => id !== 'todo'))
  check('含不可降级成员 → 不可折（哪怕全空）', !bandCollapses([...band, { id: 'clock', x: 8, y: 0, w: 4, h: 4 }], () => true))
  check('空段 → 不可折', !bandCollapses([], () => true))
}

// ---- ⑥ 边界 -------------------------------------------------------------
console.log('\n⑥ 边界')
{
  check('空布局不炸', effectiveLayout([], () => true).placements.length === 0)
  const single = [{ id: 'recent', x: 0, y: 0, w: 12, h: 3 }]
  const r = effectiveLayout(single, () => true)
  check('单模块布局：空时压成 1 行', r.placements[0].h === 1)
  check('单模块布局：非空时不动', effectiveLayout(single, () => false).placements[0].h === 3)
}

console.log()
console.log(failed ? `失败 ${failed} 条` : '全部通过')
process.exit(failed ? 1 : 0)
