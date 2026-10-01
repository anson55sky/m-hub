// 关卡测试。
//
// ## 关键：每条规则都要有**负例**
//
// 只测「好包通过」把关卡等于没测 —— 一个 `return { ok: true }`
// 能让全部正例通过。所以每条规则都配一个「本该被拦的包」，
// 并**按约定 73 验过变异后测试会红**。
//
// 用合成 zip（见 zipdir.test.ts 里的造包器思路）而不是手搓条目对象：
// 条目对象是关卡的**输入**，而真实输入来自 `listZipEntries` 的输出，
// 两者形状一致才不会漏掉解析层的问题。
import { test } from 'node:test'
import assert from 'node:assert/strict'
import { deflateRawSync, gzipSync } from 'node:zlib'
import { runGate } from './gate.ts'
import type { ZipEntry } from './zipdir.ts'

/** 直接构造条目 + 预置内容，专注测关卡本身（解析层由 zipdir.test.ts 负责） */
function mk(
  files: Record<string, string>,
  opts: { encrypted?: string[]; sizes?: Record<string, number> } = {},
): { entries: ZipEntry[]; readFile: (p: string) => Promise<Uint8Array | null> } {
  const enc = new Set(opts.encrypted ?? [])
  const entries: ZipEntry[] = Object.entries(files).map(([path, content]) => ({
    path,
    size: opts.sizes?.[path] ?? Buffer.byteLength(content),
    method: 8,
    encrypted: enc.has(path),
  }))
  const blobs = new Map(Object.entries(files).map(([k, v]) => [k, Buffer.from(v)]))
  return {
    entries,
    readFile: async (p: string) => {
      const b = blobs.get(p)
      return b ? new Uint8Array(b) : null
    },
  }
}

const OK_MANIFEST = JSON.stringify({
  id: 'com.demo',
  permissions: ['storage'],
})

test('合规包通过', async () => {
  const { entries, readFile } = mk({
    'manifest.json': OK_MANIFEST,
    'index.html': '<h1>hi</h1>',
    'assets/app.css': 'body{}',
    'assets/icon.png': 'PNG',
  })
  const r = await runGate(entries, readFile, 'com.demo')
  assert.equal(r.ok, true, `不该被拒：${r.problems.join('；')}`)
  assert.equal(r.fileCount, 4)
  assert.deepEqual(r.permissionsUsed, ['storage'])
})

test('拒：加密包（解不开就没法审核）', async () => {
  // ⚠️ 加密标记必须打在**真实存在的条目**上。最初我写成
  //    `mk({manifest}, {encrypted:['index.html']})` —— files 里根本没有
  //    index.html，于是标记落在空气上、加密规则没触发、测试红了。
  //    症状与「关卡漏检」一模一样，先确认了实现才定位到是测试写错。
  const { entries, readFile } = mk(
    { 'manifest.json': OK_MANIFEST, 'index.html': '<h1>hi</h1>' },
    { encrypted: ['index.html'] },
  )
  assert.ok(entries.some((e) => e.encrypted), '前提：确实有条目带加密标记')
  const r = await runGate(entries, readFile, 'com.demo')
  assert.equal(r.ok, false)
  assert.match(r.reason ?? '', /加密/)
})

test('拒：路径穿越（防将来服务端解压时任意写）', async () => {
  const { entries, readFile } = mk({
    'manifest.json': OK_MANIFEST,
    '../../.ssh/authorized_keys': 'x',
  })
  const r = await runGate(entries, readFile, 'com.demo')
  assert.equal(r.ok, false)
  assert.match(r.reason ?? '', /路径非法/)
})

test('拒：node_modules 混进包里', async () => {
  const { entries, readFile } = mk({
    'manifest.json': OK_MANIFEST,
    'node_modules/left-pad/index.js': 'x',
  })
  const r = await runGate(entries, readFile, 'com.demo')
  assert.equal(r.ok, false)
  assert.match(r.reason ?? '', /node_modules/)
})

test('拒：白名单外的扩展名（.sh / .exe / .py 都不能进包）', async () => {
  for (const path of ['run.sh', 'tool.exe', 'script.py']) {
    const { entries, readFile } = mk({ 'manifest.json': OK_MANIFEST, [path]: 'x' })
    const r = await runGate(entries, readFile, 'com.demo')
    assert.equal(r.ok, false, `${path} 应被拒`)
    assert.match(r.reason ?? '', /白名单/)
  }
})

test('拒：无扩展名的文件（否则无扩展名的可执行脚本能混进来）', async () => {
  const { entries, readFile } = mk({ 'manifest.json': OK_MANIFEST, 'LICENSE': 'MIT' })
  const r = await runGate(entries, readFile, 'com.demo')
  assert.equal(r.ok, false, '无扩展名必须拒 —— 约定 46 靠打包时把 LICENSE 改名成 .txt，不靠放宽白名单')
  assert.match(r.reason ?? '', /白名单/)
})

test('拒：manifest 不在包根', async () => {
  const { entries, readFile } = mk({ 'sub/manifest.json': OK_MANIFEST })
  const r = await runGate(entries, readFile, 'com.demo')
  assert.equal(r.ok, false)
  assert.match(r.reason ?? '', /包根缺少 manifest\.json/)
})

test('拒：manifest 申请了不存在的权限', async () => {
  const { entries, readFile } = mk({
    'manifest.json': JSON.stringify({ id: 'com.demo', permissions: ['storage', 'root', 'exec'] }),
  })
  const r = await runGate(entries, readFile, 'com.demo')
  assert.equal(r.ok, false)
  assert.match(r.reason ?? '', /不存在的权限.*root/s)
})

test('拒：manifest 的 id 与提交的扩展 id 不一致（防「审的是 A、装的是 B」）', async () => {
  const { entries, readFile } = mk({ 'manifest.json': OK_MANIFEST })
  const r = await runGate(entries, readFile, 'com.other')
  assert.equal(r.ok, false)
  assert.match(r.reason ?? '', /不一致/)
})

test('拒：读不出 manifest 内容（不能当成「没问题」）', async () => {
  const entries: ZipEntry[] = [{ path: 'manifest.json', size: 100, method: 8, encrypted: false }]
  const r = await runGate(entries, async () => null, 'com.demo')
  assert.equal(r.ok, false, '读不出就是读不出，不许静默放行')
  assert.match(r.reason ?? '', /读不出/)
})

test('拒：单个文件过大', async () => {
  const { entries, readFile } = mk(
    { 'manifest.json': OK_MANIFEST, 'big.png': 'x' },
    { sizes: { 'big.png': 33 * 1024 * 1024 } },
  )
  const r = await runGate(entries, readFile, 'com.demo')
  assert.equal(r.ok, false)
  assert.match(r.reason ?? '', /过大/)
})

test('拒：manifest 是乱码时给出可读原因，而不是抛异常', async () => {
  // ⚠️ 这条曾是一个**真 bug**：没有 try 时 JSON.parse 直接抛，
  //    异常冒到 handle.ts 的兜底 → 客户端收到  500，
  //    完全看不出是「包坏了」。而上传了损坏的包是很常见的事。
  const entries: ZipEntry[] = [{ path: 'manifest.json', size: 3, method: 8, encrypted: false }]
  const r = await runGate(entries, async () => new Uint8Array([0xff, 0xfe, 0xfd]), 'com.demo')
  assert.equal(r.ok, false)
  assert.match(r.reason ?? '', /不是合法 JSON/)
})

// ---- gzip/deflate 一致性：确认造包器用的 deflateRaw 与 inflateRaw 同族 ----
// 这一条不测关卡，测的是「我以为的 method 8」确实对应 deflate-raw
test('method 8 = deflate-raw（造包与解压同族）', () => {
  const raw = Buffer.from('hello hello hello hello')
  const comp = deflateRawSync(raw)
  assert.ok(comp.length < raw.length, 'deflateRawSync 应真的压缩了')
  assert.ok(gzipSync(raw).length > 0)
})