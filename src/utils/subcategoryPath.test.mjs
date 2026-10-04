import { test } from 'node:test'
import assert from 'node:assert/strict'
import { subcategoryContains, leafOf, parentPath, depthOf, reparentPath } from './subcategoryPath.ts'

/**
 * 前端这份路径规则与 Rust 侧一一对应。任何一侧改宽/改窄，另一侧的用例就会红。
 * 全部做过变异验证（见提交记录）。
 */

test('末级名 / 父路径 / 深度', () => {
  assert.equal(leafOf('开发/前端'), '前端')
  assert.equal(leafOf('开发'), '开发')
  assert.equal(parentPath('开发/前端/Vue'), '开发/前端')
  assert.equal(parentPath('开发'), '')
  assert.equal(depthOf('开发'), 0)
  assert.equal(depthOf('开发/前端'), 1)
  assert.equal(depthOf('a/b/c'), 2)
})

/**
 * ⚠️ 本条是层级功能里最容易错的一处，也是「筛选看起来有 bug」的唯一来源：
 *   `startsWith` 会把「开发者」当成「开发」的子树。
 */
test('含后代：含自身与各级后代，不含只是前缀相同的名字', () => {
  assert.equal(subcategoryContains('开发', '开发'), true)
  assert.equal(subcategoryContains('开发/前端', '开发'), true)
  assert.equal(subcategoryContains('开发/前端/Vue', '开发'), true)
  assert.equal(subcategoryContains('开发者', '开发'), false, '「开发者」不是「开发」的子树')
  assert.equal(subcategoryContains('开发x/前端', '开发'), false)
  assert.equal(subcategoryContains('开发', '开发/前端'), false, '祖先不是后代')
  assert.equal(subcategoryContains(null, '开发'), false, '未归类不属于任何小类')
  assert.equal(subcategoryContains('任意', ''), true, '空祖先 = 根')
})

test('改挂：整棵子树跟着换父，非子树原样返回 null', () => {
  assert.equal(reparentPath('开发', '研发', '开发'), '研发')
  assert.equal(reparentPath('开发', '研发', '开发/前端'), '研发/前端')
  assert.equal(reparentPath('开发', '研发', '开发/前端/Vue'), '研发/前端/Vue')
  assert.equal(reparentPath('开发', '研发', '开发者'), null)
  assert.equal(reparentPath('开发', '研发', '生活'), null)
})
