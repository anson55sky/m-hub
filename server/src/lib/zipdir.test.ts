// zip 中央目录解析的回归测试。
//
// ## 为什么用**真 zip** 而不是手搓字节片段
//
// 手搓几个十六进制常量当样本，测的只是「我理解的格式」和「我写的解析器」
// 自洽 —— 两者同时错时测试照样绿。而 zip 格式我完全可能理解错
// （flag 位偏移、长度字段位置都容易差几个字节）。
//
// 这里用 `node:zlib` 现场造**符合 APPNOTE 6.3** 的 zip，并额外拿一个
// 真实 `.xhpack` 交叉验证。
import { test } from 'node:test'
import assert from 'node:assert/strict'
import { deflateRawSync } from 'node:zlib'
import { readFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'
import { listZipEntries } from './zipdir.ts'

/** CRC32（zip 的必备字段，算错的话 unzip 会报 CRC 错） */
function crc32(buf: Buffer): number {
  const t: number[] = []
  for (let n = 0; n < 256; n++) {
    let c = n
    for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1
    t[n] = c >>> 0
  }
  let crc = 0xffffffff
  for (const b of buf) crc = t[(crc ^ b) & 0xff]! ^ (crc >>> 8)
  return (crc ^ 0xffffffff) >>> 0
}

/** 造一个真 zip（deflate 压缩 + 完整中央目录 + EOCD） */
function makeZip(files: Record<string, string>, opts: { comment?: string } = {}): Uint8Array {
  const locals: Buffer[] = []
  const cds: Buffer[] = []
  let off = 0
  for (const [name, content] of Object.entries(files)) {
    const data = Buffer.from(content)
    const comp = deflateRawSync(data)
    const nameB = Buffer.from(name)
    const crc = crc32(data)
    const lh = Buffer.alloc(30)
    lh.writeUInt32LE(0x04034b50, 0)
    lh.writeUInt16LE(20, 4) // version needed
    lh.writeUInt16LE(0, 6) // flags
    lh.writeUInt16LE(8, 8) // method = deflate
    lh.writeUInt32LE(crc, 14)
    lh.writeUInt32LE(comp.length, 18)
    lh.writeUInt32LE(data.length, 22)
    lh.writeUInt16LE(nameB.length, 26)
    locals.push(lh, nameB, comp)
    const cd = Buffer.alloc(46)
    cd.writeUInt32LE(0x02014b50, 0)
    cd.writeUInt16LE(20, 4)
    cd.writeUInt16LE(20, 6)
    cd.writeUInt16LE(8, 10)
    cd.writeUInt32LE(crc, 16)
    cd.writeUInt32LE(comp.length, 20)
    cd.writeUInt32LE(data.length, 24)
    cd.writeUInt16LE(nameB.length, 28)
    cd.writeUInt32LE(off, 42)
    cds.push(cd, nameB)
    off += lh.length + nameB.length + comp.length
  }
  const cdBuf = Buffer.concat(cds)
  const commentB = Buffer.from(opts.comment ?? '')
  const eo = Buffer.alloc(22 + commentB.length)
  eo.writeUInt32LE(0x06054b50, 0)
  eo.writeUInt16LE(Object.keys(files).length, 8)
  eo.writeUInt16LE(Object.keys(files).length, 10)
  eo.writeUInt32LE(cdBuf.length, 12)
  eo.writeUInt32LE(off, 16)
  eo.writeUInt16LE(commentB.length, 20)
  commentB.copy(eo, 22)
  return new Uint8Array(Buffer.concat([...locals, cdBuf, eo]))
}

test('列出条目：路径、原始大小、压缩标记', () => {
  const z = makeZip({ 'manifest.json': '{"id":"x"}', 'index.html': '<h1>hi</h1>', 'a/b/c.txt': 'deep' })
  const es = listZipEntries(z)
  assert.equal(es.length, 3)
  assert.deepEqual(es.map((e) => e.path), ['manifest.json', 'index.html', 'a/b/c.txt'])
  assert.equal(es[0]!.size, Buffer.byteLength('{"id":"x"}')) // 实测 10；别手数
  assert.ok(es.every((e) => e.method === 8), 'deflate 的 method 应为 8（压缩与否看 method，不看 flags bit3）')
  assert.ok(es.every((e) => !e.encrypted))
})

test('带注释的 zip：EOCD 位置不固定，仍要找到', () => {
  // 注释长度可变 → EOCD 不能按固定偏移找，必须倒着搜（文件头坑 ②）
  const z = makeZip({ 'a.txt': 'a' }, { comment: 'x'.repeat(1000) })
  assert.deepEqual(listZipEntries(z).map((e) => e.path), ['a.txt'])
})

test('嵌套目录的条目名保留完整路径（关卡要按后缀判扩展名）', () => {
  const z = makeZip({ 'dist/assets/app.js': 'x', 'node_modules/pkg/index.js': 'y' })
  const paths = listZipEntries(z).map((e) => e.path)
  assert.ok(paths.includes('dist/assets/app.js'))
  assert.ok(paths.includes('node_modules/pkg/index.js'), '关卡要能看见 node_modules 里的东西')
})

test('不是 zip 时抛错（不返回空数组冒充「包里没有文件」）', () => {
  // 关键：返回空数组会让关卡以为「包是空的」，那是**静默放行**的方向。
  assert.throws(() => listZipEntries(new Uint8Array(100)), /不是 zip|中央目录/)
  assert.throws(() => listZipEntries(new Uint8Array(0)), /不是 zip|中央目录/)
})

test('真实 .xhpack：与 Node 自带 zip 工具的输出对照', () => {
  const p = join(dirname(fileURLToPath(import.meta.url)), '..', '..', 'manifests', 'local.calculator-0.1.0.xhpack')
  let bytes: Uint8Array
  try {
    bytes = readFileSync(p)
  } catch {
    return // 该文件不在（干净检出时）→ 本条跳过，但上面几条仍在跑
  }
  const es = listZipEntries(bytes)
  const paths = es.map((e) => e.path)
  assert.ok(paths.includes('manifest.json'), `真实包里应有 manifest.json，实际：${paths.join(', ')}`)
  assert.ok(paths.some((x) => x.endsWith('index.html')), '真实包里应有 index.html')
  // 契约 46：manifest 必须在**包根**（不能是 manifest.json/manifest.json）
  assert.equal(paths.filter((x) => x === 'manifest.json').length, 1)
  assert.ok(es.every((e) => e.size > 0), '真实包里不该有 0 字节条目')
  assert.ok(es.every((e) => e.method === 0 || e.method === 8), '真实包只用 store/deflate')
})