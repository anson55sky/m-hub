import { test } from 'node:test'
import assert from 'node:assert/strict'
import { sniffShot } from './submissions.ts'

/**
 * 截图类型必须**按文件头**判，不信扩展名、不信 multipart 的 `File.type`。
 *
 * ⚠️ 这道校验以前没有 —— 因为上传的字节**压根没被存下来**。
 *   存下来之后它就是一道真闸门：一张 `text/html` 被原样回给浏览器，
 *   就是存储型 XSS（截图是公开地址，任何人拿到 id 都能取）。
 *
 * 三条都是变异验证过的。
 */

test('png / jpg / webp 按文件头认出', () => {
  assert.equal(sniffShot(new Uint8Array([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0, 0]))?.ext, 'png')
  assert.equal(sniffShot(new Uint8Array([0xff, 0xd8, 0xff, 0xe0, 0, 0]))?.ext, 'jpg')
  assert.equal(
    sniffShot(
      new Uint8Array([0x52, 0x49, 0x46, 0x46, 0, 0, 0, 0, 0x57, 0x45, 0x42, 0x50, 0, 0]),
    )?.ext,
    'webp',
  )
})

test('⚠️ html / svg / 脚本一律认不出（宁可拒绝，不给「猜一个类型」的机会）', () => {
  const html = new TextEncoder().encode('<!DOCTYPE html><script>alert(1)</script>')
  assert.equal(sniffShot(html), null, 'HTML 绝不能被当图片收下')
  const svg = new TextEncoder().encode('<svg xmlns="http://www.w3.org/2000/svg"><script/></svg>')
  assert.equal(sniffShot(svg), null, 'SVG 能内联脚本，同样不放行')
  assert.equal(sniffShot(new TextEncoder().encode('GIF89a')), null, '不在白名单里就是拒绝')
})

test('⚠️ 截断到只剩文件头前缀的短字节不认（长度下限是判定的一部分）', () => {
  // 只有 2 字节的 \x89P —— 足以通过前两个字节的比较，但不是一张图。
  // 判据是「头部完整匹配」，不是「前缀匹配」。
  assert.equal(sniffShot(new Uint8Array([0x89, 0x50])), null)
  assert.equal(sniffShot(new Uint8Array([0xff, 0xd8])), null)
  assert.equal(sniffShot(new Uint8Array([0x52, 0x49, 0x46, 0x46])), null)
  assert.equal(sniffShot(new Uint8Array([])), null)
})

test('jpeg 的 FF D8 FF 三字节都要在（只有 SOI 两个字节不算）', () => {
  assert.equal(sniffShot(new Uint8Array([0xff, 0xd8, 0x00, 0x01])), null, '缺第三个 FF 不认')
  assert.equal(sniffShot(new Uint8Array([0xff, 0xd8, 0xff, 0x00]))?.ext, 'jpg')
})
