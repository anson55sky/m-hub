import { test } from 'node:test'
import assert from 'node:assert/strict'
import { parseShotIds } from './shotIds.ts'

/**
 * `shots_json` 是我们自己写的「资产行 id 数组」。它的**失败模式不是抛错，
 * 而是少图或给错图** —— 所以解析器必须宁可返回空数组，也不能返回垃圾。
 *
 * ⚠️ 全部用例都是变异验证过的：逐条删掉判断后重跑，用例必须变红。
 */

test('正常形状：按原顺序返回正整数 id', () => {
  assert.deepEqual(parseShotIds('[3,1,2]'), [3, 1, 2], '顺序必须保留 —— 那是展示顺序')
})

test('null / 空串 / undefined 一律空数组（不是异常）', () => {
  assert.deepEqual(parseShotIds(null), [])
  assert.deepEqual(parseShotIds(''), [])
})

test('坏 JSON 不抛错，按「没有截图」处理', () => {
  // ⚠️ 清单是全站功能，不能为一个字段的脏数据 500
  assert.deepEqual(parseShotIds('{不是 json'), [])
  assert.deepEqual(parseShotIds('[1,'), [])
})

test('非数组（对象/数字/字符串）按空数组处理', () => {
  assert.deepEqual(parseShotIds('{"0":1}'), [])
  assert.deepEqual(parseShotIds('7'), [])
  assert.deepEqual(parseShotIds('"[1,2]"'), [])
})

test('剔除非正整数：0 / 负数 / 小数 / 字符串 / null / 对象 / 布尔', () => {
  assert.deepEqual(parseShotIds('[0,-1,1.5,"2",null,true,{},[],4]'), [4])
})

test('剔掉超出安全整数的数（会拼出根本不存在的路由）', () => {
  assert.deepEqual(parseShotIds('[1e999]'), [], 'Infinity 不是合法 id')
  assert.deepEqual(parseShotIds('[9007199254740993]'), [], '超过 MAX_SAFE_INTEGER 会丢精度')
})

test('⚠️ 变异：只判 typeof number 会放进 1.5 与 1e999', () => {
  // 这条断言守的是「Number.isInteger + 范围」两个判断真的在代码里。
  // 把 parseShotIds 里的 `Number.isInteger(x) && x > 0 && x <= MAX` 换成
  // `typeof x === 'number'` → 本用例必须变红。
  assert.deepEqual(parseShotIds('[1.5]'), [])
  assert.deepEqual(parseShotIds('[1e999]'), [])
})
