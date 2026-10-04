import { test } from 'node:test'
import assert from 'node:assert/strict'
import { parseNotes, parseInline } from './notesParse.ts'

const SAMPLE = `# v0.7.4 发布说明
## 新增
- 速达的小类支持层级了
- 扩展发布弹窗可以填「作者署名」了
## 修复
- 关闭「自动检查更新」后不再打扰
`

test('标题 / 列表 / 段落三类分得开', () => {
  const b = parseNotes(SAMPLE)
  assert.deepEqual(
    b.map((x) => x.kind),
    ['h', 'h', 'li', 'li', 'h', 'li'],
    '空行不该产生空段落',
  )
  assert.equal(b[0].text, 'v0.7.4 发布说明')
  assert.equal(b[0].level, 1)
  assert.equal(b[1].level, 2)
  assert.equal(b[2].text, '速达的小类支持层级了')
  assert.equal(b[2].level, 0)
})

test('⚠️ 变异：把列表缩进忽略掉必须变红（子项会与父项同级）', () => {
  const b = parseNotes('- 顶层\n  - 子项')
  assert.equal(b[0].level, 0)
  assert.equal(b[1].level, 1, '缩进两格 = 子级')
  const deep = parseNotes('    - 四格缩进')
  assert.equal(deep[0].level, 2, '四格 = 二级')
})

test('认不出的行按段落原样显示，不猜它是标题', () => {
  const b = parseNotes('普通一行\n### 三级标题\n#没有空格不是标题')
  assert.deepEqual(b.map((x) => x.kind), ['p', 'h', 'p'])
  assert.equal(b[2].text, '#没有空格不是标题')
})

test('层级封顶三级：#### 及更深都当三级（再深没有视觉层级可用）', () => {
  assert.equal(parseNotes('###### 六级')[0].level, 3)
})

test('段内加粗：成对的 ** 才算加粗', () => {
  assert.deepEqual(parseInline('**重点**事项'), [
    { text: '重点', bold: true },
    { text: '事项', bold: false },
  ])
  assert.deepEqual(parseInline('没有加粗'), [{ text: '没有加粗', bold: false }])
  // ⚠️ 单个 ** 不补：补了就把「a**b」这种正文改坏了
  assert.deepEqual(parseInline('a**b'), [{ text: 'a**b', bold: false }])
})

test('⚠️ 变异：把成对检查放宽（找最近的 ** 就闭合）必须变红', () => {
  // 现实现用 `[^*]+`（不允许加粗段里再出现 **），放宽成 `[\s\S]+?` 后
  // 'a ** b ** c' 会被误判成一段加粗
  assert.deepEqual(parseInline('**重点**'), [{ text: '重点', bold: true }])
  assert.deepEqual(parseInline('*单个星号*'), [{ text: '*单个星号*', bold: false }])
  // ⚠️ `****` 是这条的分水岭：`[^*]+` 要求加粗段里**至少一个非星号字符**，
  //   于是它原样保留。放宽成 `[\s\S]+?` 后会匹配出「空内容的加粗段」，
  //   界面上那一行就变成空白 —— 只测 `**重点**` 是测不出这个差别的。
  assert.deepEqual(parseInline('****'), [{ text: '****', bold: false }])
  assert.deepEqual(parseInline('a ** b ** c'), [
    { text: 'a ', bold: false },
    { text: ' b ', bold: true },
    { text: ' c', bold: false },
  ])
})
