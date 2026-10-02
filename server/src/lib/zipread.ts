// 从 zip 里取**单个**条目的内容（只给关卡读 manifest.json 用）。
//
// ## 为什么不完整解压
//
// 关卡只需要 manifest.json（核对 id 与权限）。完整解压要把用户的全部代码
// 拉进内存 —— 那既是 zip bomb 的入口，也是不必要的风险面。
//
// ## 定位方式：用中央目录给的 localOffset，不扫描
//
// 我第一版写成「从第一个局部头一路扫过去」，那在 **data descriptor**
// （局部头里 compSize=0、flag bit 3 置位）下会算错步长，越扫越歪。
// 中央目录每个条目都记着它局部头的精确偏移（+42），直接用即可 ——
// 这是 zip 格式本身提供的，不是猜的。

import type { ZipEntry } from './zipdir.ts'
import { inflateRaw } from './inflate.ts'

const LFH_SIG = 0x04034b50

function rd16(b: Uint8Array, o: number): number {
  return b[o]! | (b[o + 1]! << 8)
}
function rd32(b: Uint8Array, o: number): number {
  return (b[o]! | (b[o + 1]! << 8) | (b[o + 2]! << 16)) + b[o + 3]! * 0x1000000
}

/**
 * 取指定条目内容；条目不存在返回 null。
 *
 * @param maxBytes 解压后上限，防 zip bomb（关卡唯一会读的是 manifest，
 *                 256KB 足够；不给上限的话 300 字节的包能解出几 GB）
 * @throws 条目损坏 / 用了不支持的压缩方法 —— **绝不静默返回 null**：
 *         null 会被关卡当成「读不出 manifest」，那与「这个包用了
 *         我看不懂的压缩法」是两回事，混起来会给出误导性的原因。
 */
export async function readZipEntry(
  bytes: Uint8Array,
  entries: ZipEntry[],
  path: string,
  maxBytes: number,
): Promise<Uint8Array | null> {
  const e = entries.find((x) => x.path === path)
  if (!e) return null
  if (e.encrypted) throw new Error(`${path} 是加密条目，无法读取`)

  const p = e.localOffset
  if (p + 30 > bytes.length || rd32(bytes, p) !== LFH_SIG) {
    throw new Error(`${path} 的局部文件头无效（偏移 ${p}）—— zip 可能损坏或有前置字节`)
  }
  // ⚠️ 名字长度要**读局部头的**（+26），不能用中央目录的：两者可以不同
  //    （中央目录存的是 UTF-8 名，局部头可能是旧式的）。步长必须按局部的算。
  const nameLen = rd16(bytes, p + 26)
  const extraLen = rd16(bytes, p + 28)
  const start = p + 30 + nameLen + extraLen
  const end = start + e.compressedSize
  if (end > bytes.length) throw new Error(`${path} 的数据超出文件末尾（zip 损坏）`)
  const raw = bytes.subarray(start, end)

  if (e.method === 0) {
    // store：未压缩。仍要校验上限 —— 「未压缩」不等于「小」。
    if (raw.length > maxBytes) throw new Error(`${path} 未压缩就有 ${raw.length} 字节，超过 ${maxBytes}`)
    return raw
  }
  if (e.method !== 8) {
    throw new Error(`${path} 使用了不支持的压缩方法 ${e.method}（只支持 0=store / 8=deflate）`)
  }
  return inflateRaw(raw, maxBytes)
}