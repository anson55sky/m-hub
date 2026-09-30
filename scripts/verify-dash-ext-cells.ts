/**
 * `dashExtCells` 的验证脚本（不进 prebuild，手动跑）。
 *
 * 跑法：node --experimental-strip-types scripts/verify-dash-ext-cells.ts
 *
 * ## 这条规则在防什么
 *
 * 一次真实的数据丢失（用户可见、且当时没有任何报错）：
 * 恢复工作台布局时，`ext:` 格子按 `dashModuleDef(id)` 过滤，而扩展注册表
 * （`listExtensions()`）在启动早期可能还没就绪 → 过滤把用户的扩展卡片丢掉 →
 * 用户之后**任何一次无关改动**触发 `persist()`，把这次丢失写回配置 → 永久消失。
 *
 * `unresolvedExtIds` 就是用来识别「该重试」还是「扩展真被卸载了」的判据。
 * 它必须同时满足：只认 `ext:` 前缀、认得出没注册的、去重、不受坏 JSON 影响。
 *
 * 验的是**生产代码本身**（直接 import 真 .ts 源文件），不是抄一份逻辑。
 */
import { unresolvedExtIds, EXT_REGISTRY_RETRY_DELAYS_MS } from '../src/composables/dashExtCells.ts'

let failed = 0
function check(name: string, ok: boolean) {
  if (ok) console.log(`  ✓ ${name}`)
  else {
    console.error(`  ✗ ${name}`)
    failed++
  }
}

const layout = (items: Array<{ id: string }>) => JSON.stringify(items)

console.log('① 注册表齐全：不该触发重试')
{
  const raw = layout([
    { id: 'clock' },
    { id: 'ext:local.calculator' },
    { id: 'sticky1' },
  ])
  check(
    '已注册的扩展格子不算未解析',
    unresolvedExtIds(raw, ['ext:local.calculator']).length === 0,
  )
}

console.log('\n② 注册表为空（启动竞态）：必须被识别出来，否则格子会被永久写掉')
{
  const raw = layout([{ id: 'clock' }, { id: 'ext:local.calculator' }, { id: 'recent' }])
  const missing = unresolvedExtIds(raw, [])
  check('识别出未解析的扩展格子', missing.length === 1)
  check('id 原样返回（可用于日志/兜底）', missing[0] === 'ext:local.calculator')
}

console.log('\n③ 只认 ext: 前缀')
{
  const raw = layout([{ id: 'clock' }, { id: 'notes' }, { id: 'todo' }])
  check('纯内置布局返回空', unresolvedExtIds(raw, []).length === 0)
  const weird = layout([{ id: 'ext:' }, { id: 'extension:x' }, { id: 'my-ext:1' }])
  // 只认**精确的** `ext:` 前缀 —— 注册表里的 id 一律是 `ext:${扩展id}`。
  // `extension:x` / `my-ext:1` 不是扩展格子（它们连 dashModuleDef 都过不了），
  // 误判成扩展只会让重试白等好几秒 —— 宁可漏判也不能误判。
  check('只有 ext: 前缀算扩展族', unresolvedExtIds(weird, []).join() === 'ext:')
}

console.log('\n④ 边界')
{
  check('坏 JSON 不抛异常', unresolvedExtIds('{不是 json', []).length === 0)
  check('非数组不炸', unresolvedExtIds('{"id":"ext:x"}', []).length === 0)
  check('空字符串', unresolvedExtIds('', []).length === 0)
  const noId = layout([{ x: 0 }, { id: 123 }, { id: null }] as never)
  check('缺 id / id 非字符串：跳过而不炸', unresolvedExtIds(noId, []).length === 0)
  const dup = layout([{ id: 'ext:a' }, { id: 'ext:a' }, { id: 'ext:b' }])
  const d = unresolvedExtIds(dup, [])
  check('重复 id 去重', d.length === 2 && d[0] === 'ext:a' && d[1] === 'ext:b')
  const partial = layout([{ id: 'ext:a' }, { id: 'ext:b' }])
  check('只报没注册的那一个', unresolvedExtIds(partial, ['ext:a']).join() === 'ext:b')
}

console.log('\n⑤ 重试节奏')
{
  check('至少重试 3 次（单次不够：竞态窗口不止一次 tick）', EXT_REGISTRY_RETRY_DELAYS_MS.length >= 3)
  check('间隔递增', EXT_REGISTRY_RETRY_DELAYS_MS.every((v, i, a) => i === 0 || v > a[i - 1]))
  check('总等待不超过 ~6.5s（再久就是让用户白等）', EXT_REGISTRY_RETRY_DELAYS_MS.reduce((a, b) => a + b, 0) <= 6500)
}

console.log()
console.log(failed ? `失败 ${failed} 条` : '全部通过')
process.exit(failed ? 1 : 0)
