import { test } from 'node:test'
import assert from 'node:assert/strict'
import { filterNotesByFolder } from './noteFolderFilter.ts'

/**
 * 速记文件夹筛选（v0.8.0 发布说明 ①）。
 *
 * 这三条状态的错误形态都是**同一句话**：界面选中 A、列表里是 B —— 没有任何
 * 报错，所以只能靠用例守。全部做过变异验证（见提交说明）。
 */

/** 只造筛选用得着的字段，其余按接口默认 —— 用例不关心时间戳内容。 */
const n = (id, folder_id) => ({
  id,
  title: `笔记${id}`,
  content: '',
  folder_id,
  deleted_at: null,
  icon: '',
  created_at: '2026-10-08 00:00:00.000000',
  updated_at: '2026-10-08 00:00:00.000000',
})

const ALL = [n(1, null), n(2, 10), n(3, 20), n(4, 11)]

test('未选目标 → 原样返回（不筛）', () => {
  assert.equal(filterNotesByFolder(ALL, null, null), ALL)
  // 子树还没取回来时也绝不能筛：拿旧子树或空集筛都会闪一下错内容
  assert.equal(filterNotesByFolder(ALL, 10, null), ALL)
})

test('未归类 = folder_id 为 null 的那些', () => {
  assert.deepEqual(
    filterNotesByFolder(ALL, 'unfiled', []).map((x) => x.id),
    [1],
  )
})

test('某文件夹 = 自身 + 全部后代 id，且不含未归类', () => {
  // 10 是父，11 是它的后代
  assert.deepEqual(
    filterNotesByFolder(ALL, 10, [10, 11]).map((x) => x.id),
    [2, 4],
  )
})

/**
 * ⚠️ 本条守的是一个真 bug：我第一版写成
 *   `if (ids.length === 0) return 未归类`
 *   —— 「数字目标 + 空子树」被当成了未归类。而 `subtree_ids(id)` 对有效 id
 *   永远非空（文件夹总在自己的子树里），所以真出现空集只可能是异常态，
 *   那时该显示空，不是把未归类的笔记顶上来。
 */
test('数字目标 + 空子树 → 空列表，绝不是未归类', () => {
  const got = filterNotesByFolder(ALL, 10, [])
  assert.deepEqual(got.map((x) => x.id), [])
})

/**
 * ⚠️ **契约用例，不是变异守卫** —— 它抓不到「不筛直接返回全部」这个错：
 * 输入为空时「返回全部」与「返回筛选结果」都是 `[]`，`deepEqual` 分辨不出。
 * 实测把返回条件改成「空列表就返回全部」，本条照样绿。
 * 保留它是因为它记录了契约（空输入必须给空输出），但**别把它算进覆盖率**。
 */
test('空笔记列表 → 空列表，不返回全部', () => {
  assert.deepEqual(filterNotesByFolder([], 'unfiled', []), [])
  assert.deepEqual(filterNotesByFolder([], 10, [10]), [])
})

/**
 * ⚠️ 同为**契约用例**：`Set` 去重是语言保证，重复 id 与单次 id 的结果恒等，
 *   所以任何「破坏集合语义」的变异在这里都不可观察。写它是为了让
 *   「集合语义」这件事在用例里有名有姓，而非为了抓变异。
 */
test('子树 id 集合是集合语义：重复 id 不产生重复行', () => {
  assert.deepEqual(
    filterNotesByFolder(ALL, 10, [10, 10, 10]).map((x) => x.id),
    [2],
  )
})
