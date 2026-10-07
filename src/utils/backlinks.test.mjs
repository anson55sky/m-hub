import { test } from 'node:test'
import assert from 'node:assert/strict'
import {
  extractWikiLinks,
  buildBacklinkIndex,
  snippetAround,
  titleAmbiguity,
  normalizeTitle,
} from './backlinks.ts'

/**
 * 速记双链（发布说明 ②）。
 *
 * ⚠️ 这批测试里有一半是「边界」—— 因为双链的失败模式全是**静默的**：
 * 多匹配一点、少匹配一点，界面上都只是「反链少了一条」，没有报错。
 */

/* ── 抽出 ───────────────────────────────────────────────────────── */

test('抽出基本形式', () => {
  const l = extractWikiLinks('见 [[会议记录]] 了解')
  assert.equal(l.length, 1)
  assert.equal(l[0].target, '会议记录')
  assert.equal(l[0].label, '')
  assert.equal(l[0].anchor, '')
  assert.equal(l[0].offset, 2, '「见」占 0、空格占 1、`[` 在 2')
})

test('|显示文字 与 #小节 后缀', () => {
  const l = extractWikiLinks('[[会议记录|那次会]]和[[购物清单#第二页]]')
  assert.equal(l.length, 2)
  assert.equal(l[0].target, '会议记录')
  assert.equal(l[0].label, '那次会')
  assert.equal(l[1].target, '购物清单')
  assert.equal(l[1].anchor, '第二页')
})

test('⚠️ [[A#B]] 按 wiki 惯例拆成「A 的小节 B」；标题含 # 要用带引号形式', () => {
  // ⚠️ 这是**刻意**选的取舍，不是 bug：`[[A#B]]` 本身歧义（见 parseInner 注释）。
  //    判据是「可预测 > 猜得准」—— 按惯例拆，标题真含 # 时用引号形式。
  const plain = extractWikiLinks('[[C# 学习笔记]]')
  assert.equal(plain[0].target, 'C')
  assert.equal(plain[0].anchor, '学习笔记')

  // 引号形式：整体当标题，不拆
  const quoted = extractWikiLinks('[["C# 学习笔记"]]')
  assert.equal(quoted[0].target, 'C# 学习笔记')
  assert.equal(quoted[0].anchor, '', '引号内不拆锚点')
  assert.equal(quoted[0].label, '')
})

test('⚠️ 标题含多个 # 时取**最后一个**当锚点', () => {
  // `lastIndexOf` 与 `indexOf` 只在有多个 # 时有区别 —— 没有这条用例的话，
  // 把 lastIndexOf 改成 indexOf 测试照样绿（实测）。
  // 判据：`[[A#B#C]]` 读成「标题 A#B 的小节 C」比「标题 A 的小节 B#C」更合理，
  // 因为锚点通常写在末尾。
  const l = extractWikiLinks('[[A#B#C]]')
  assert.equal(l[0].target, 'A#B')
  assert.equal(l[0].anchor, 'C')
})

test('引号包整个 inner（不能只包标题部分）', () => {
  // ⚠️ 这是**刻意**的简化：引号 + 显示文字同时需要时，语法会出现
  //   「引号可以只包一部分」的歧义。要显示文字就别用引号形式。
  const l = extractWikiLinks('[["C# 学习笔记"]]')
  assert.equal(l[0].target, 'C# 学习笔记')
  assert.equal(l[0].label, '')
  assert.equal(l[0].anchor, '')
})

test('⚠️ 不跨行匹配（打了一半的标记不是链接）', () => {
  // 跨行匹配会把中间整段正文吞成「标题」，用户看到的是「某条链接的标题变成一整段话」
  assert.deepEqual(extractWikiLinks('前[[\n中间一整段正文\n]]后'), [])
})

test('⚠️ 空目标被跳过（[[]] 是打了一半，不是链接）', () => {
  assert.deepEqual(extractWikiLinks('[[]]'), [])
  assert.deepEqual(extractWikiLinks('[[|只有显示文字]]'), [], '目标为空')
})

test('⚠️ 标题含 `]` 时不支持 —— 匹配到第一个 ]]，产生一条可见的坏链', () => {
  // 正则 `[^\]\n]+` 会在**第一个** `]]` 处收口。这不是「宁可漏也不错」，
  // 而是「漏不掉」：`[[数组 Array[]] 笔记]]` 必然匹配成 `数组 Array[`。
  //
  // 为什么接受它而不是改成不匹配：一条指向不存在笔记的链接在界面上是
  // **可见的**（「引用了不存在的笔记」），用户能看到自己写错了标记；
  // 而整条不匹配的话，用户看到一段原样文本，不知道问题出在哪。
  //
  // 真要支持标题含 `]`，得引入转义（`\[\`）—— 那是另一套语法，不做。
  const l = extractWikiLinks('[[数组 Array[]] 笔记]]')
  assert.equal(l.length, 1)
  assert.equal(l[0].target, '数组 Array[')
  // 而 `[[a](b)]]` 这种「`[[` 后面紧跟 `]`」的确实匹配不上（内层为空）
  assert.deepEqual(extractWikiLinks('[[a](b)]]'), [])
})

test('多次抽出无状态残留（正则 /g 的 lastIndex）', () => {
  const a = extractWikiLinks('[[甲]]')
  const b = extractWikiLinks('[[甲]]')
  assert.deepEqual(a, b)
  assert.equal(a.length, 1)
  assert.equal(b.length, 1)
})

/* ── 索引 ───────────────────────────────────────────────────────── */

const notes = [
  { id: 1, title: '会议记录', content: '见 [[购物清单]] 与 [[周报]]' },
  { id: 2, title: '购物清单', content: '买菜' },
  { id: 3, title: '周报', content: '本周引用了 [[会议记录]]' },
]

test('建出链与反链', () => {
  const ix = buildBacklinkIndex(notes)
  assert.equal((ix.outgoing.get(1) ?? []).length, 2)
  const inc = ix.incoming.get(2) ?? []
  assert.equal(inc.length, 1)
  assert.equal(inc[0].id, 1, '会议记录引用了购物清单')
  assert.equal((ix.incoming.get(1) ?? []).length, 1, '周报被引用')
  assert.equal(ix.dangling.size, 0)
})

test('同一篇多次引用同一目标 → 合并成一条、记多个偏移', () => {
  const ix = buildBacklinkIndex([
    { id: 1, title: '甲', content: '开头 [[乙]] 中间 [[乙]] 结尾' },
    { id: 2, title: '乙', content: '' },
  ])
  const inc = ix.incoming.get(2) ?? []
  assert.equal(inc.length, 1, '不该出现「乙 ×2」')
  assert.equal(inc[0].offsets.length, 2, '两处偏移都要记')
})

test('引用不存在的笔记要记成 dangling（不能静默丢弃）', () => {
  const ix = buildBacklinkIndex([{ id: 1, title: '甲', content: '见 [[不存在的]]' }])
  assert.equal(ix.dangling.get('不存在的'), 1)
  assert.equal(ix.incoming.size, 0)
})

test('标题匹配忽略首尾空白与大小写', () => {
  const ix = buildBacklinkIndex([
    { id: 1, title: 'Meeting Notes', content: '' },
    { id: 2, title: '甲', content: '[[  meeting notes  ]]' },
  ])
  assert.equal(ix.dangling.size, 0, '首尾空白与大小写不该影响匹配')
  assert.equal((ix.incoming.get(1) ?? []).length, 1)
})

test('⚠️ 折叠空白不等于删除空白（[[会议记录]] ≠ [[会议 记录]]）', () => {
  // 这条是**故意**分开的：normalizeTitle 折叠**连续**空白为一个，
  // 但不删除词内空格。否则「会议 记录」和「会议记录」会被当成同一篇，
  // 而那是两篇用户明确分开写的笔记。
  const ix = buildBacklinkIndex([
    { id: 1, title: '会议 记录', content: '' },
    { id: 2, title: '甲', content: '[[会议记录]]' },
  ])
  assert.equal(ix.dangling.size, 1, '不该匹配上')
  assert.equal(ix.incoming.size, 0)
})

test('重名 → 指向第一篇，且给出提示文案', () => {
  const ix = buildBacklinkIndex([
    { id: 1, title: '重复', content: '' },
    { id: 2, title: '重复', content: '' },
    { id: 3, title: '引用方', content: '[[重复]]' },
  ])
  assert.deepEqual(ix.byTitle.get('重复'), [1, 2])
  assert.equal((ix.incoming.get(1) ?? []).length, 1, '指向第一篇')
  assert.equal((ix.incoming.get(2) ?? []).length, 0)
  assert.match(titleAmbiguity(ix, '重复'), /2 篇同名/)
  assert.equal(titleAmbiguity(ix, '唯一'), '')
})

test('未命名笔记不作为链接目标', () => {
  const ix = buildBacklinkIndex([
    { id: 1, title: '   ', content: '' },
    { id: 2, title: '甲', content: '[[无标题笔记]]' },
  ])
  // ⚠️ 必须断言 **byTitle 里没有空键**，而不是断言 dangling.size。
  //    空标题进不进 byTitle 对 dangling 计数没影响（`[[无标题笔记]]` 的 key 是
  //    「无标题笔记」，与空键不匹配，照样落进 dangling）—— 我第一版就是这么写的，
  //    于是「空标题进索引」这个变异照样绿（实测）。
  assert.ok(!ix.byTitle.has(''), '空标题不该进索引')
  // 而 `[[]]`（空目标）被跳过，所以也不会有人「链接到空标题」
  assert.equal(ix.dangling.size, 1)
})

test('摘要里的双链标记被去掉（界面上不显示 [[ ]]）', () => {
  const ix = buildBacklinkIndex([
    { id: 1, title: '甲', content: '前面铺垫文字见 [[乙|那次会]] 后面' },
    { id: 2, title: '乙', content: '' },
  ])
  const inc = ix.incoming.get(2) ?? []
  assert.ok(!inc[0].snippet.includes('[['), `摘要里有标记：${inc[0].snippet}`)
  assert.ok(inc[0].snippet.includes('那次会'), '应显示自定义文字')
})

test('normalizeTitle 折叠空白', () => {
  assert.equal(normalizeTitle('  a   b  '), 'a b')
  assert.equal(normalizeTitle('a\n\tb'), 'a b')
})

test('snippetAround 在正文两端加省略号', () => {
  const c = 'x'.repeat(100) + 'TARGET' + 'y'.repeat(100)
  const i = c.indexOf('TARGET')
  const s = snippetAround(c, i, 10)
  assert.ok(s.startsWith('…') && s.endsWith('…'), s)
  const s2 = snippetAround('短', 0, 40)
  assert.ok(!s2.startsWith('…') && !s2.endsWith('…'), s2)
})
