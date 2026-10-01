// 分块存储的纯函数边界测试（不碰 DB）。
//
// ## 为什么这组边界必须测
//
// 分块最容易错的不是「算法」，是**边界**：
// · 空包 —— `sliceChunks` 返回 0 块还是 1 块空块？两者 `putBlob` 行为不同
// · 正好整除 —— 1 块 / 2 块整除时，最后一块**不能**是空的
// · 尾块 —— subarray 的长度算错会让读回来的字节少一截，而**字节数对得上
//   但内容错**的情况更隐蔽（错在拼接顺序）
//
// 用 128KB 的假数据在单测里造是可行的（Uint8Array 分配很快），
// 不必真构造一个大文件。
import { test } from 'node:test'
import assert from 'node:assert/strict'
import { sliceChunks, joinChunks, toHex, CHUNK_SIZE } from './pkgStore.ts'

test('空包：切片返回 0 块，拼接回 0 字节', () => {
  assert.deepEqual(sliceChunks(new Uint8Array(0)), [], '空包不该产生一块空块')
  assert.equal(joinChunks([], 0).length, 0)
})

test('单块（小于一块）原样往返', () => {
  const b = new Uint8Array([1, 2, 3, 4, 5])
  const parts = sliceChunks(b)
  assert.equal(parts.length, 1)
  assert.deepEqual([...joinChunks(parts, b.length)], [...b])
})

test('正好整除：不产生多余的空尾块', () => {
  const b = new Uint8Array(CHUNK_SIZE * 2)
  const parts = sliceChunks(b)
  assert.equal(parts.length, 2, '2 块整除就该是 2 块，不能多出第 3 块空块')
  assert.ok(parts.every((p) => p.length === CHUNK_SIZE), '每块都应满')
  assert.equal(joinChunks(parts, b.length).length, b.length)
})

test('跨块往返：内容与顺序完全一致', () => {
  const size = CHUNK_SIZE * 2 + 7 // 故意非整除，逼出尾块
  const b = new Uint8Array(size)
  for (let i = 0; i < size; i++) b[i] = i % 251 // 非重复模式，顺序错乱立刻可见
  const parts = sliceChunks(b)
  assert.equal(parts.length, 3)
  assert.equal(parts[2]!.length, 7, '尾块长度不对')
  const out = joinChunks(parts, size)
  assert.equal(out.length, size)
  for (let i = 0; i < size; i++) {
    if (out[i] !== b[i]) assert.fail(`第 ${i} 字节不一致：${out[i]} ≠ ${b[i]}`)
  }
})

test('joinChunks 会丢弃超出 total 的尾部数据（防御库里多出的块）', () => {
  const parts = [new Uint8Array([1, 2]), new Uint8Array([3, 4])]
  assert.deepEqual([...joinChunks(parts, 3)], [1, 2, 3], 'total 之外的字节必须被丢弃')
})

test('toHex 小端十六进制、定长补零', () => {
  assert.equal(toHex(new Uint8Array([0, 1, 15, 16, 255])), '00010f10ff')
  assert.equal(toHex(new Uint8Array([])), '')
})