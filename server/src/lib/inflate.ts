// zip 单条目解压（只给关卡读 manifest.json 用）。
//
// ## 为什么现在才做
//
// 关卡唯一需要读**内容**的地方是 manifest.json（核对 id 与权限），
// 其余全靠中央目录。所以不引入 zip 库、不做完整解压，只实现这一条路径。
//
// ## 用运行时的 DecompressionStream
//
// Workers/Pages 内置 `DecompressionStream('deflate-raw')`，
// 不需要 polyfill 也不需要 WASM。zip 的 method 8 正好对应 deflate-raw。
//
// ## 两个必须防的坑
//
// ① **zip bomb**：压缩后 300 字节的 manifest.json 能解出几 GB。
//    所以 `maxOutputBytes` 是**硬上限**，超了立即中止 —���
//    不能「先解完再检查大小」。
// ② **method 0（store）**：未压缩条目不需要解压，直接复制字节。
//    只支持这两种 method；遇到其他（bzip2/lzma/分卷）明确报错，
//    因为「看不懂」绝不能被当成「没问题」。

export async function inflateRaw(data: Uint8Array, maxOutputBytes: number): Promise<Uint8Array> {
  const ds = new DecompressionStream('deflate-raw')
  const writer = ds.writable.getWriter()

  // ⚠️ 写入侧的 promise **必须**显式接住，否则超限时 cancel 掉 readable
  //    会让 writable 以 AbortError 拒绝，而这个拒绝**没人等** →
  //    Node 报 unhandledRejection，把真正的错误（超限）盖掉。
  //    实测踩过：测试看到的是 AbortError 而不是「超过上限」。
  const feed = (async () => {
    await writer.write(data as unknown as BufferSource)
    await writer.close()
  })()
  feed.catch(() => {
    /* 超限路径下这是预期的；真正的错误由下面的 throw 报出 */
  })

  const reader = ds.readable.getReader()
  const chunks: Uint8Array[] = []
  let total = 0
  for (;;) {
    const { done, value } = await reader.read()
    if (done) break
    if (!value) continue
    total += value.byteLength
    // ⚠️ 边读边判，而不是读完再判 —— 见文件头坑 ①
    if (total > maxOutputBytes) {
      await reader.cancel().catch(() => {})
      await feed.catch(() => {})
      throw new Error(`解压后超过 ${maxOutputBytes} 字节上限（可能是 zip bomb）`)
    }
    chunks.push(value as Uint8Array)
  }

  const out = new Uint8Array(total)
  let off = 0
  for (const c of chunks) {
    out.set(c, off)
    off += c.byteLength
  }
  return out
}