// 「真 zip → 读出 manifest.json → 过关卡」的端到端测试。
//
// ## 为什么单独一层
//
// gate.test.ts 用的是**构造出来的条目对象**（专注测规则），
// zipdir.test.ts 用真 zip 但只列目录。中间那一步
// ——**从真 zip 里按路径取到内容**—— 谁都没测过。
//
// 而那一步正是最容易错的：步长算错、偏移用错列、方法判断错，
// 症状都是「读不出 manifest」→ 关卡误拒好包，或者更糟：读到了错的内容。
import { test } from 'node:test'
import assert from 'node:assert/strict'
import { deflateRawSync } from 'node:zlib'
import { listZipEntries } from './zipdir.ts'
import { readZipEntry } from './zipread.ts'
import { runGate } from './gate.ts'

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

/** 真 zip：可指定哪些条目用 store(0)、哪些用 deflate(8) */
function zip(files: Record<string, string>, store: string[] = []): Uint8Array {
  const locals: Buffer[] = []
  const cds: Buffer[] = []
  let off = 0
  for (const [name, content] of Object.entries(files)) {
    const data = Buffer.from(content)
    const useStore = store.includes(name)
    const comp = useStore ? data : deflateRawSync(data)
    const method = useStore ? 0 : 8
    const nameB = Buffer.from(name)
    const crc = crc32(data)
    const lh = Buffer.alloc(30)
    lh.writeUInt32LE(0x04034b50, 0)
    lh.writeUInt16LE(method, 8)
    lh.writeUInt32LE(crc, 14)
    lh.writeUInt32LE(comp.length, 18)
    lh.writeUInt32LE(data.length, 22)
    lh.writeUInt16LE(nameB.length, 26)
    locals.push(lh, nameB, comp)
    const cd = Buffer.alloc(46)
    cd.writeUInt32LE(0x02014b50, 0)
    cd.writeUInt16LE(method, 10)
    cd.writeUInt32LE(crc, 16)
    cd.writeUInt32LE(comp.length, 20)
    cd.writeUInt32LE(data.length, 24)
    cd.writeUInt16LE(nameB.length, 28)
    cd.writeUInt32LE(off, 42)
    cds.push(cd, nameB)
    off += lh.length + nameB.length + comp.length
  }
  const cdBuf = Buffer.concat(cds)
  const eo = Buffer.alloc(22)
  eo.writeUInt32LE(0x06054b50, 0)
  eo.writeUInt16LE(Object.keys(files).length, 8)
  eo.writeUInt16LE(Object.keys(files).length, 10)
  eo.writeUInt32LE(cdBuf.length, 12)
  eo.writeUInt32LE(off, 16)
  return new Uint8Array(Buffer.concat([...locals, cdBuf, eo]))
}

test('从真 zip 里读出 deflate 的 manifest.json', async () => {
  const mf = JSON.stringify({ id: 'com.demo', permissions: ['storage', 'config'] })
  const z = zip({ 'manifest.json': mf, 'index.html': '<h1>hi</h1>' })
  const entries = listZipEntries(z)
  const raw = await readZipEntry(z, entries, 'manifest.json', 256 * 1024)
  assert.ok(raw, '应读得到')
  assert.deepEqual(JSON.parse(new TextDecoder().decode(raw!)), {
    id: 'com.demo',
    permissions: ['storage', 'config'],
  })
})

test('store（未压缩）条目同样读得出 —— 两种 method 都要支持', async () => {
  // ⚠️ 只测 deflate 的话，「method=0 走的是另一条分支」就没人验过，
  //    而 .xhpack 完全可能含未压缩条目。
  const mf = JSON.stringify({ id: 'com.store', permissions: [] })
  const z = zip({ 'manifest.json': mf, 'a.txt': 'plain' }, ['manifest.json', 'a.txt'])
  const entries = listZipEntries(z)
  assert.equal(entries.find((e) => e.path === 'manifest.json')!.method, 0)
  const raw = await readZipEntry(z, entries, 'manifest.json', 256 * 1024)
  assert.equal(JSON.parse(new TextDecoder().decode(raw!)).id, 'com.store')
})

test('多个条目时读第二个也要对（步长/偏移不能只对第一个）', async () => {
  // 这是「靠扫描定位」最容易错的地方：第一个对了不代表第二个也对。
  const files: Record<string, string> = {}
  for (let i = 0; i < 12; i++) files[`pad/f${i}.txt`] = `content-${i}-${'x'.repeat(i * 37)}`
  files['manifest.json'] = JSON.stringify({ id: 'com.last', permissions: [] })
  const z = zip(files)
  const entries = listZipEntries(z)
  assert.equal(entries.length, 13)
  for (let i = 0; i < 12; i++) {
    const raw = await readZipEntry(z, entries, `pad/f${i}.txt`, 1024 * 1024)
    assert.equal(new TextDecoder().decode(raw!), files[`pad/f${i}.txt`], `第 ${i} 个条目读错`)
  }
  const mf = await readZipEntry(z, entries, 'manifest.json', 256 * 1024)
  assert.equal(JSON.parse(new TextDecoder().decode(mf!)).id, 'com.last', '最后一个条目也要对')
})

test('不存在的路径返回 null（不是抛错）', async () => {
  const z = zip({ 'manifest.json': '{}' })
  assert.equal(await readZipEntry(z, listZipEntries(z), 'nope.html', 1024), null)
})

test('zip bomb：解压超上限时抛错而不是吃满内存', async () => {
  const big = 'A'.repeat(2 * 1024 * 1024) // 2MB 高度可压
  const z = zip({ 'manifest.json': '{}', 'bomb.txt': big })
  await assert.rejects(
    () => readZipEntry(z, listZipEntries(z), 'bomb.txt', 64 * 1024),
    /超过|上限|zip bomb/,
  )
  // 同一个包，给足上限就该读得出 —— 证明是上限在起作用，不是功能坏了
  const ok = await readZipEntry(z, listZipEntries(z), 'bomb.txt', 4 * 1024 * 1024)
  assert.equal(ok!.length, big.length)
})

test('端到端：真 zip → 关卡通过', async () => {
  const mf = JSON.stringify({ id: 'com.e2e', permissions: ['storage'] })
  const z = zip({ 'manifest.json': mf, 'index.html': '<h1>hi</h1>', 'assets/a.css': 'body{}' })
  const entries = listZipEntries(z)
  const r = await runGate(
    entries,
    (p) => readZipEntry(z, entries, p, 256 * 1024),
    'com.e2e',
  )
  assert.equal(r.ok, true, `不该被拒：${r.problems.join('；')}`)
  assert.equal(r.fileCount, 3)
})

test('端到端：真 zip 里塞了 .sh → 关卡拒（证明关卡看到的是真实内容而非构造对象）', async () => {
  const mf = JSON.stringify({ id: 'com.bad', permissions: [] })
  const z = zip({ 'manifest.json': mf, 'evil.sh': 'rm -rf ~' })
  const entries = listZipEntries(z)
  const r = await runGate(
    entries,
    (p) => readZipEntry(z, entries, p, 256 * 1024),
    'com.bad',
  )
  assert.equal(r.ok, false)
  assert.match(r.reason ?? '', /白名单/)
})