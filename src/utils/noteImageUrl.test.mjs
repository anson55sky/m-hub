import { test } from 'node:test'
import assert from 'node:assert/strict'
import {
  NOTE_IMAGE_HTTP_PREFIX,
  NOTE_IMAGE_NATIVE_PREFIX,
  normalizeNoteImageUrls,
} from './noteImageUrl.ts'

/**
 * 笔记图片 URL 的平台归一（macOS 移植修复）。
 *
 * 这条守卫守的是**「图到底能不能显示」**：判据来自 Tauri/wry 的源码 ——
 * Windows 用 `http://<scheme>.localhost/`，macOS 用裸 scheme 的 `<scheme>://`，
 * 两种形态在对方的平台上都不是「打不开的本地文件」而是一次真实的网络请求。
 * 症状是图裂，而任何类型检查与后端测试都看不出正文里的地址形态。
 */

/* ── macOS：Windows 形态 → 原生形态 ─────────────────────────────── */

test('macOS 上 Windows 形态的地址会被改写', () => {
  const src = `正文 ![图](${NOTE_IMAGE_HTTP_PREFIX}0123456789abcdef.png)`
  const out = normalizeNoteImageUrls(src, true)
  assert.ok(out.includes(`${NOTE_IMAGE_NATIVE_PREFIX}0123456789abcdef.png`), out)
  assert.ok(!out.includes('http://'), out)
})

test('macOS 上已经是原生形态的地址原样保留（幂等）', () => {
  const src = `![图](${NOTE_IMAGE_NATIVE_PREFIX}fedcba9876543210.jpg)`
  assert.equal(normalizeNoteImageUrls(src, true), src)
  assert.equal(normalizeNoteImageUrls(normalizeNoteImageUrls(src, true), true), src)
})

/* ── Windows：原生形态 → Windows 形态 ───────────────────────────── */

test('Windows 上原生形态的地址会被改写成 http 形态', () => {
  const src = `![图](${NOTE_IMAGE_NATIVE_PREFIX}0123456789abcdef.png)`
  const out = normalizeNoteImageUrls(src, false)
  assert.ok(out.includes(`${NOTE_IMAGE_HTTP_PREFIX}0123456789abcdef.png`), out)
})

test('Windows 上 Windows 形态的地址原样保留（幂等）', () => {
  const src = `![图](${NOTE_IMAGE_HTTP_PREFIX}0123456789abcdef.png)`
  assert.equal(normalizeNoteImageUrls(src, false), src)
})

/* ── 绝不能碰的 ─────────────────────────────────────────────────── */

test('正文里的其它 http 地址、data URL 一律不动', () => {
  const src = [
    '![外链图](https://example.com/a.png)',
    '![本站图](http://mhub-note.example.com/a.png)',
    '![内联图](data:image/png;base64,AAAA)',
    '```',
    `![代码块里的](${NOTE_IMAGE_HTTP_PREFIX}0123456789abcdef.png)`,
    '```',
    '[普通链接](http://example.com)',
  ].join('\n')
  const out = normalizeNoteImageUrls(src, true)
  // 代码块内的也一并改写是有意的：编辑器载入后整篇就是当前平台的形态，
  // 下次保存即自愈；留着不改反而是「一半能显示一半不能」的薛定谔状态。
  // 真正不能碰的是**不匹配的**地址：
  assert.ok(out.includes('https://example.com/a.png'), out)
  assert.ok(out.includes('data:image/png;base64,AAAA'), out)
  assert.ok(out.includes('http://mhub-note.example.com/a.png'), out)
  assert.ok(out.includes('[普通链接](http://example.com)'), out)
})

test('空串与纯文本原样返回', () => {
  assert.equal(normalizeNoteImageUrls('', true), '')
  assert.equal(normalizeNoteImageUrls('就一段文字', true), '就一段文字')
  assert.equal(normalizeNoteImageUrls('就一段文字', false), '就一段文字')
})

test('多张图全部改写，不只第一张', () => {
  const src = [
    `![1](${NOTE_IMAGE_HTTP_PREFIX}0123456789abcdef.png)`,
    `![2](${NOTE_IMAGE_HTTP_PREFIX}fedcba9876543210.png)`,
  ].join('\n\n')
  const out = normalizeNoteImageUrls(src, true)
  assert.equal(out.split(NOTE_IMAGE_NATIVE_PREFIX).length - 1, 2, out)
})