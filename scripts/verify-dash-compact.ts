/**
 * `dashLayoutGeometry` 的验证脚本（不进 prebuild，手动跑）。
 *
 * 为什么是脚本而不是 vitest：本工程没有前端测试框架，而为了验证三个纯函数
 * 引入一整套工具链不划算。Node 22 自带类型擦除（--experimental-strip-types），
 * 直接跑真实的 .ts 源文件即可 —— 验的是**代码本身**，不是复制一份逻辑。
 *
 * 覆盖的是「算法会不会把版面弄坏」这一类不变量，这些靠肉眼读版面是读不出来的：
 *   ① 全满时逐格不变（不能因为加了降级功能就动用户的版面）
 *   ② 有空模块时被压扁到 1 行
 *   ③ 上吸之后**不产生任何重叠**（这是唯一真正会出错的地方）
 *   ④ 总行数随内容收缩
 *
 * 跑法：node --experimental-strip-types scripts/verify-dash-compact.ts
 */
import {
  effectiveLayout,
  pullUp,
  rectsOverlap,
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
const maxY = (ps: Rect[]) => Math.max(...ps.map((p) => p.y + p.h))
const hasOverlap = (ps: Rect[]) =>
  ps.some((a, i) => ps.slice(i + 1).some((b) => rectsOverlap(a, b)))

const PRESET = loadPreset()
console.log(`PRESET 载入 ${PRESET.length} 个模块\n`)

// ---- ① 全满：逐格不变 -------------------------------------------------------
console.log('① 全部模块都有内容 → 版面必须逐格不变')
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
  check('顺序不变', placements.map((p) => p.id).join() === PRESET.map((p) => p.id).join())
}

// ---- ② 提示词为空 → 压扁 ----------------------------------------------------
console.log('\n② prompts / recent / countdown 为空 → 压扁到 1 行')
{
  const empty = new Set(['prompts', 'recent', 'countdown'])
  const { placements, compacted } = effectiveLayout(PRESET, (id) => empty.has(id))
  check('compacted = true', compacted)
  for (const id of empty) {
    const p = placements.find((q) => q.id === id)
    check(`${id} 被压到 1 行`, p?.h === 1, `实际 h=${p?.h}`)
  }
  const prompts = placements.find((p) => p.id === 'prompts')!
  check('prompts 的 x 未被改动', prompts.x === PRESET.find((p) => p.id === 'prompts')!.x)
  check('prompts 的 w 未被改动', prompts.w === PRESET.find((p) => p.id === 'prompts')!.w)
}

// ---- ③ 上吸后无重叠（唯一真会错的地方）------------------------------------
console.log('\n③ 上吸之后不得产生任何重叠（穷举各种空/满组合）')
{
  const degradables = PRESET.filter((p) => DEGRADABLE_IDS.has(p.id)).map((p) => p.id)
  let combos = 0
  let bad = 0
  // 2^n 穷举（本工程默认布局里可降级模块只有 3 个，组合数很小）
  for (let mask = 0; mask < 1 << degradables.length; mask++) {
    const empty = new Set(degradables.filter((_, i) => mask & (1 << i)))
    const { placements } = effectiveLayout(PRESET, (id) => empty.has(id))
    combos++
    if (hasOverlap(placements)) {
      bad++
      console.error(`      重叠！空模块 = ${[...empty].join(',') || '(无)'}`)
    }
  }
  check(`${combos} 种空/满组合全部无重叠`, bad === 0, `${bad} 种有重叠`)

  // pullUp 单独再验一次：随机坐标也不能造出重叠
  let pullBad = 0
  for (let n = 0; n < 400; n++) {
    const items = Array.from({ length: 6 }, (_, i) => ({
      id: `m${i}`,
      x: (n * 7 + i * 3) % 12,
      y: (n * 5 + i * 4) % 15,
      w: 1 + ((n + i) % 4),
      h: 1 + ((n * 2 + i) % 3),
    }))
    if (hasOverlap(pullUp(items))) pullBad++
  }
  check('pullUp 对 400 组随机坐标都不产生重叠', pullBad === 0, `${pullBad} 组有重叠`)
}

// ---- ④ 总行数随内容收缩 -----------------------------------------------------
console.log('\n④ 全部可降级模块都空时，总行数应当收缩')
{
  const degradables = new Set(PRESET.filter((p) => DEGRADABLE_IDS.has(p.id)).map((p) => p.id))
  const full = effectiveLayout(PRESET, () => false)
  const empty = effectiveLayout(PRESET, (id) => degradables.has(id))
  check(
    `总行数 ${maxY(empty.placements)} < 全满时 ${maxY(full.placements)}`,
    maxY(empty.placements) < maxY(full.placements),
  )
}

// ---- ⑤ 非降级模块永远不被压扁 ----------------------------------------------
console.log('\n⑤ 时钟/天气/便签/系统资源 等非降级模块，任何情况下都不被压扁')
{
  const never = ['clock', 'weather', 'sticky1', 'sticky2', 'sysmon']
  const { placements } = effectiveLayout(PRESET, () => true) // 全部判空，最激进
  for (const id of never) {
    const before = PRESET.find((p) => p.id === id)!
    const after = placements.find((p) => p.id === id)!
    check(
      `${id} 尺寸不变（${after.w}×${after.h}）`,
      after.w === before.w && after.h === before.h,
    )
  }
}

console.log(failed ? `\n失败 ${failed} 条` : '\n全部通过')
process.exit(failed ? 1 : 0)
