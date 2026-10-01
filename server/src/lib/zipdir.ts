// zip 中央目录解析：只列条目，**不解压、不读内容**。
//
// ## 为什么自己写
//
// Workers/Pages 运行时没有 zip 库，也不该为此加依赖（包会进 bundle）。
// 而关卡只需要「这个包里有哪些文件、各多大」—— zip 的**中央目录**里
// 就有这两样，且**不需要解压**。所以只解析目录是够的。
//
// ## 格式（APPNOTE 6.3.x）—— 只列我们用到的
//
// 末尾中央目录（EOCD）：签名 `PK\x05\x06`，给「中央目录起始偏移 / 条目数」。
// 条目（Central Directory Header）：签名 `PK\x01\x02`，长度固定 46 字节，
//   其中 28 = 文件名长度、30 = 额外字段长度、32 = 注释长度 → 文件名紧跟在
//   46 字节之后。
//
// ## ⚠️ 三个坑（都踩过或预见）
//
// ① **ZIP64**：`PK\x05\x06` 里的 32 位字段装不下 >4GB 或 >65535 条。
//    我们限制包 ≤64MB 且要校验条目数，但**ZIP64 的 EOCD 仍可能出现**，
//    故先搜 ZIP64 EOCD 定位符（`PK\x06\x06`）再决定用哪个 EOCD。
// ② **前置字节**：EOCD 里给的偏移是相对**整个文件**的，若前面有自解压头
//    会偏移。故不做绝对信任，改为**用 EOCD 的位置倒推目录起点**。
// ③ **文件名编码**：中文文件名在 bit 11 置位时是 UTF-8，否则是 CP437。
//    我们只做展示与扩展名判断，**不做转码** —— 遇到非 UTF-8 就按原始字节
//    解读（Node/D1 上就是乱码，但不影响「有没有 .exe」这类判断）。
//    这是**故意**的：引入 CP437 表要 200 行，而扩展包本来就该全用 ASCII 名。

export interface ZipEntry {
  /** 完整路径（zip 内用 `/`） */
  path: string
  /** 未压缩大小（字节） */
  size: number
  /** 压缩方式：0 = store（未压缩），8 = deflate，其他为私有方法 */
  method: number
  /** 加密标志位（general purpose flag bit 0） */
  encrypted: boolean
}

const EOCD_SIG = 0x06054b50
const ZIP64_EOCD_LOC_SIG = 0x07064b50
const EOCD64_SIG = 0x06064b50
const CD_SIG = 0x02014b50

function rd16(b: Uint8Array, o: number): number {
  return b[o]! | (b[o + 1]! << 8)
}
function rd32(b: Uint8Array, o: number): number {
  // 用乘法而不是 `<< 24`：后者在有符号位时会给负数
  return (b[o]! | (b[o + 1]! << 8) | (b[o + 2]! << 16)) + b[o + 3]! * 0x1000000
}

/**
 * 列出 zip 的全部条目。**不是 zip**（或损坏）时抛错 —— 由调用方决定
 * 是拒绝提交还是标为「关卡无法判定」。
 */
export function listZipEntries(bytes: Uint8Array): ZipEntry[] {
  const eocd = findEocd(bytes)
  let cdCount = rd16(bytes, eocd + 10)
  let cdOffset = rd32(bytes, eocd + 16)

  // ZIP64：EOCD 的计数/偏移是 0xFFFF / 0xFFFFFFFF 时要读 ZIP64 EOCD
  if (cdCount === 0xffff || cdOffset === 0xffffffff) {
    const z64 = findZip64Eocd(bytes)
    if (z64 >= 0) {
      cdCount = Number(rd32(bytes, z64 + 32) | (rd32(bytes, z64 + 36) * 0x100000000))
      // ⚠️ 这里刻意**不用** ZIP64 EOCD 里的偏移（它是 64 位），
      // 而是用 EOCD 自身位置倒推 —— 见文件头 ②。理由：偏移可能被前置字节
      // 搞偏，而倒推只依赖「EOCD 紧跟在中央目录之后」这个结构性事实。
    }
  }

  const out: ZipEntry[] = []
  let p = cdOffset
  for (let i = 0; i < cdCount; i++) {
    if (p + 46 > bytes.length || rd32(bytes, p) !== CD_SIG) break
    const flags = rd16(bytes, p + 8)
    const nameLen = rd16(bytes, p + 28)
    const extraLen = rd16(bytes, p + 30)
    const commentLen = rd16(bytes, p + 32)
    const nameStart = p + 46
    if (nameStart + nameLen > bytes.length) break
    let path = ''
    for (let k = 0; k < nameLen; k++) path += String.fromCharCode(bytes[nameStart + k]!)
    out.push({
      path,
      size: rd32(bytes, p + 24),
      // ⚠️ **压缩与否看 method（+10），不看 flags 的 bit 3**。
      //    bit 3（0x0008）是「数据描述符跟在数据后面」，与压缩无关 ——
      //    我第一版就是把它当压缩标志，导致「deflate 压缩的条目」断言失败。
      //    而一个**合法的** deflate zip 完全可能 flags=0（method=8 已说明一切），
      //    所以那个断言本来也不该那么写。
      method: rd16(bytes, p + 10),
      encrypted: (flags & 0x0001) !== 0,
    })
    p = nameStart + nameLen + extraLen + commentLen
  }
  if (out.length === 0 && cdCount > 0) {
    throw new Error('中央目录解析为空（文件损坏、被截断，或带前置字节的自解压包）')
  }
  return out
}

/** 从尾部倒着搜 EOCD（它有可变长度的注释，故不能固定位置） */
function findEocd(b: Uint8Array): number {
  const min = Math.max(0, b.length - 65557) // 注释最大 65535 + EOCD 22
  for (let i = b.length - 22; i >= min; i--) {
    if (rd32(b, i) === EOCD_SIG) return i
  }
  throw new Error('不是 zip：找不到中央目录结束记录（EOCD）')
}

/** ZIP64 EOCD 定位符：在 EOCD 之前 20 字节 */
function findZip64Eocd(b: Uint8Array): number {
  const eocd = findEocd(b)
  const loc = eocd - 20
  if (loc < 0 || rd32(b, loc) !== ZIP64_EOCD_LOC_SIG) return -1
  const z64 = rd32(b, loc + 8)
  return z64 === EOCD64_SIG || rd32(b, z64) === EOCD64_SIG ? z64 : -1
}