// 运行期生成并签名市场清单。
//
// ## 为什么要有这个模块（2026-10-02）
//
// 原来上架**必须**跑构建期脚本 `scripts/publish-approved.mjs`：读 D1 → 解包 →
// 重签 → 部署 Pages。理由是市场清单是 Ed25519 **签过的静态文件**，签名覆盖
// 精确字节，而 D1 是运行期数据库 —— 改它不会改动 CDN 上那个字节。
//
// 这条设计在**安全上是对的**，在**体验上是坏的**：审核通过之后没有任何按钮
// 能让清单更新，必须有人在那台持有私钥的机器上跑一条命令。用户报的是
// 「重新发布新版本后市场还是旧版，而且没有上架按钮」—— 不是他不会用，
// 是这条路径**根本没有按钮可按**。
//
// 改成运行期签名：私钥作为 Pages 加密 secret（`MARKET_PRIVATE_KEY`）；
// 「上架」动作在运行期重建清单 → 签 → **把字节与签名一起存进 D1 单行**。
//
// ## 为什么签名结果必须存进 D1，而不是每次请求现签
//
// 客户端取的是**两个独立请求**：`/registry` 与 `/registry.sig`，然后验签。
// 现签的话两次请求可能落在**不同的 D1 快照 / 不同的边缘节点**，只要中间
// 恰好有一次重建，两边字节就��不上 → 验签失败。
// 存成一行，两个端点读同一个值，这个竞态从根上不存在。
//
// ## 纯逻辑与运行期逻辑分开
//
// `buildRegistry()` 是纯函数（可单测）。`signEd25519()` 依赖 WebCrypto —
// Node 22 的 WebCrypto **还没有** Ed25519（Node 24 才加），所以单测覆盖不了
// 它，别假装能覆盖。
import type { Ctx } from './http.ts'
import { parseShotIds } from './shotIds.ts'

export interface RegistryEntry {
  id: string
  name: string
  version: string
  description?: string
  homepage?: string
  /** 作者署名（发布弹窗填的，或 manifest 里的） */
  author?: string
  permissions?: string[]
  screenshots?: string[]
  downloadUrl: string
  sha256: string
  size: number
  publisherId?: string
  verified?: boolean
}

export interface Registry {
  schemaVersion: number
  updatedAt: string
  revoked: string[]
  extensions: RegistryEntry[]
}

/** 已上架提交里，清单要用到的那几列 */
export interface PublishedRow {
  extId: string
  version: string
  pkgSha256: string | null
  pkgSize: number | null
  manifestJson: string | null
  username: string | null
  /** 作者署名（2026-10-04）。来自发布弹窗，优先于 manifest 里的 author */
  author: string | null
  /** 作者在发布弹窗上传的截图（资产行 id 数组的 JSON），2026-10-04 */
  shotsJson: string | null
}

// ---------------------------------------------------------------- 纯逻辑

/** 三段式数字版本比较（与客户端 `market.rs::version_cmp`、服务端 `compareSemver` 同口径） */
export function cmpVersion(a: string, b: string): number {
  const pa = String(a).split('.')
  const pb = String(b).split('.')
  for (let i = 0; i < 3; i++) {
    const d = (Number(pa[i]) || 0) - (Number(pb[i]) || 0)
    if (d !== 0) return d < 0 ? -1 : 1
  }
  return 0
}

/** 包内 manifest 的最小读法：只要清单要用的那几个字段 */
function readManifest(raw: string | null): Record<string, unknown> | null {
  if (!raw) return null
  try {
    const v = JSON.parse(raw)
    return v && typeof v === 'object' ? (v as Record<string, unknown>) : null
  } catch {
    return null
  }
}

function str(v: unknown): string | undefined {
  return typeof v === 'string' && v.trim() ? v.trim() : undefined
}

function strArray(v: unknown): string[] | undefined {
  if (!Array.isArray(v)) return undefined
  const out = v.filter((x): x is string => typeof x === 'string' && !!x)
  return out.length ? out : undefined
}

/**
 * 从已上架的行构建清单对象。
 *
 * 同一扩展只保留**版本号最高**的那一条 —— 上架语义就是「每个扩展一个当前版本」，
 * 历史版本不在清单里并存（靠旧包体与 `revoked` 保留）。
 *
 * ⚠️ 三条「宁可少一条也不给错数据」的取舍，理由写在各自注释里。
 */
export function buildRegistry(
  rows: PublishedRow[],
  opts: { revoked: string[]; baseUrl: string },
): Registry {
  const best = new Map<string, PublishedRow>()
  for (const r of rows) {
    const prev = best.get(r.extId)
    if (!prev || cmpVersion(r.version, prev.version) > 0) best.set(r.extId, r)
  }

  const extensions: RegistryEntry[] = []
  for (const r of [...best.values()].sort((a, b) => a.extId.localeCompare(b.extId))) {
    // ⚠️ 没有包体的行**必须跳过**，不能塞一个空 downloadUrl：
    //   那会让用户点安装得到 404，而服务端一切正常。
    if (!r.pkgSha256 || !r.pkgSize) continue
    // ⚠️ 没有 manifest 的行同样跳过。manifest 缺失意味着 name/description 全空，
    //   用户在市场里看到一个只有 id 和下载链接的空壳条目 —— 比不显示更糟。
    //   （上架动作会保证写 manifest_json；这里是最后一道关。）
    const mf = readManifest(r.manifestJson)
    if (!mf) continue

    const id = str(mf.id) ?? r.extId
    const version = str(mf.version) ?? r.version
    const entry: RegistryEntry = {
      id,
      // 回落到 extId 而不是空串：名字缺失时至少还能在列表里认出来
      name: str(mf.name) ?? id,
      version,
      downloadUrl: `${opts.baseUrl}/packages/${id}/${version}/${id}-${version}.xhpack`,
      sha256: r.pkgSha256,
      size: r.pkgSize,
    }
    const desc = str(mf.description)
    if (desc) entry.description = desc
    const home = str(mf.homepage)
    if (home) entry.homepage = home
    const perms = strArray(mf.permissions)
    if (perms) entry.permissions = perms
    // 截图：**上传的优先于 manifest 里声明的**。
    //
    // ⚠️ 此前这里只读 `mf.screenshots` —— 而 manifest 里那个字段是作者手写的
    //   URL 数组，从发布弹窗选图选出来的字节**压根没进过清单**（服务端收了
    //   就丢）。于是作者精心选了 5 张图，市场上仍然显示「作者未提供截图」。
    //
    //   回落顺序也是刻意的：上传的字节由我们自己判过文件头、存在自己的库里，
    //   manifest 里写的地址可能指向已经死掉的外链 —— 前者优先。
    const uploaded = parseShotIds(r.shotsJson).map(
      (sid) => `${opts.baseUrl}/api/v1/market/shot/${sid}`,
    )
    const shots = uploaded.length ? uploaded : strArray(mf.screenshots)
    if (shots) entry.screenshots = shots
    // 作者署名：发布弹窗填的优先（那是作者**这次**署名给人看的），
    // 没填则回落到 manifest 里的 `author`（老扩展也有）。
    // ⚠️ 之前压根没输出 author —— 客户端 `MarketExtension.author` 与详情页那一栏
    //   是**一直存在**的，只是运行期清单里永远没有它，于是那一栏永远不显示。
    const author = (r.author ?? '').trim() || str(mf.author)
    if (author) entry.author = author
    if (r.username) entry.publisherId = r.username
    // 只追加字段、不抬 schemaVersion（约定 46）：老客户端对未知字段宽容，
    // 抬版本号会让所有老客户端的市场直接变空
    entry.verified = true
    extensions.push(entry)
  }

  return {
    schemaVersion: 2,
    updatedAt: new Date().toISOString(),
    revoked: opts.revoked,
    extensions,
  }
}

/**
 * 序列化成**签名覆盖的那份字节**。
 *
 * ⚠️ 缩进必须是 2 空格且带尾随换行 —— 与 `seed-manifests.mjs` 写的
 *   `JSON.stringify(registry, null, 2)` + 换行**逐字一致**。
 *   不一致的话「构建期写的」与「运行期写的」两份清单签名不同，
 *   而客户端缓存的是哪一份就决定了验签成败。这个字节约定不是格式偏好。
 */
export function registryBytes(reg: Registry): Uint8Array {
  return new TextEncoder().encode(JSON.stringify(reg, null, 2) + '\n')
}

// ---------------------------------------------------------------- 运行期签名

/** PEM → 裸字节 */
export function pemToDer(pem: string): Uint8Array {
  const b64 = pem
    .replace(/-----BEGIN [^-]+-----/, '')
    .replace(/-----END [^-]+-----/, '')
    .replace(/\s+/g, '')
  const bin = atob(b64)
  const out = new Uint8Array(bin.length)
  for (let i = 0; i < bin.length; i++) out[i] = bin.charCodeAt(i)
  return out
}

/**
 * Ed25519 签名（Worker 的 WebCrypto）。
 *
 * ⚠️ 不能用 `node:crypto`：这里是 Worker 运行期，没有 node 模块。
 * ⚠️ 也不能沿用构建期那套 `openssl pkeyutl -rawin`：那要 spawn 子进程。
 */
export async function signEd25519(pkcs8: Uint8Array, data: Uint8Array): Promise<string> {
  const key = await crypto.subtle.importKey('pkcs8', toBuf(pkcs8), { name: 'Ed25519' }, false, ['sign'])
  const sig = await crypto.subtle.sign({ name: 'Ed25519' }, key, toBuf(data))
  const bytes = new Uint8Array(sig)
  let bin = ''
  for (const b of bytes) bin += String.fromCharCode(b)
  return btoa(bin)
}

function toBuf(u: Uint8Array): ArrayBuffer {
  const copy = new Uint8Array(u.byteLength)
  copy.set(u)
  return copy.buffer
}

// ---------------------------------------------------------------- D1 存取

/**
 * 读出「当前生效的清单字节 + 签名」。
 *
 * ⚠️ 两个端点必须调**这一个**函数取各自的半边，绝不能各自重建 ——
 *   那是字节与签名对不上的唯一来源（见文件头）。
 */
export async function loadRegistryRecord(
  ctx: Ctx,
): Promise<{ bytes: string; sig: string } | null> {
  const row = await ctx.env.DB.prepare(`SELECT bytes, sig FROM market_registry WHERE id = 1`)
    .first<{ bytes: string; sig: string }>()
  if (!row || !row.bytes || !row.sig) return null
  return row
}

/** 对外可达的基地址（清单里必须是**绝对**地址，客户端要拿它下载包体） */
export function publicBase(ctx: Ctx): string {
  return `${ctx.url.protocol}//${ctx.url.host}`
}

/**
 * 重建清单：读 D1 已上架的行 → 构建 → 签 → **存回单行**。
 *
 * 幂等：同样的输入产生同样的字节，只有 `updatedAt` 会变。
 */
export async function rebuildRegistry(ctx: Ctx): Promise<{ extensions: number; bytes: number }> {
  const rows = await ctx.env.DB.prepare(
    `SELECT s.ext_id AS extId, s.version, s.pkg_sha256 AS pkgSha256, s.pkg_size AS pkgSize,
            s.manifest_json AS manifestJson, s.author, u.username
       FROM submissions s
       LEFT JOIN users u ON u.id = s.user_id
      WHERE s.status = 'published'`,
  )
    .all<PublishedRow>()

  const reg = buildRegistry(rows.results ?? [], { revoked: [], baseUrl: publicBase(ctx) })
  const bytes = registryBytes(reg)
  const priv = ctx.env.MARKET_PRIVATE_KEY
  if (!priv) {
    // ⚠️ 没有私钥就**不要**写一份没签名的清单上去：
    //   旧的那份是验签通过的，换成没签的会让客户端从「能用」变成「全部失效」——
    //   一次配置失误换来全量不可用。宁可让上架失败并保留旧清单。
    throw new Error('MARKET_PRIVATE_KEY 未配置，无法重建市场清单')
  }
  const sig = await signEd25519(pemToDer(priv), bytes)
  await ctx.env.DB.prepare(
    `INSERT INTO market_registry (id, bytes, sig, updated_at) VALUES (1, ?1, ?2, ?3)
     ON CONFLICT(id) DO UPDATE SET bytes = ?1, sig = ?2, updated_at = ?3`,
  )
    .bind(new TextDecoder().decode(bytes), sig, Date.now())
    .run()
  return { extensions: reg.extensions.length, bytes: new TextDecoder().decode(bytes).length }
}