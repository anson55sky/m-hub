// 扩展包安全关卡。
//
// ## 为什么关卡必须在「审核通过」**之前**
//
// 没有关卡的审核 = 一个点一下的按钮，审核人只能凭文件名判断。
// 用户装上的是**别人的代码**，会在自己的机器上跑、还持有宿主给的桥 API 权限。
// 所以关卡是审核这件事**有意义**的前提，不是可选的加固。
//
// ## 每条规则的取舍
//
// 每条下面都写了「它防什么」。这不是注释洁癖：后来加的检查如果说不出
// 防的是什么，多半是凭想象加的，反而会变成误拒、让人绕过真正的检查。
//
// ## 只读中央目录，不解压
//
// 关卡只看**文件清单 + manifest.json 的内容**，不解压任何用户代码。
// `listZipEntries` 已经把清单拿出来；唯一要读内容的是 manifest.json，
// 而它必须限制解压后的大小（zip bomb：几 KB 压缩包能解出几 GB）。

import type { ZipEntry } from './zipdir.ts'

/** 包内允许出现的文件扩展名（小写，含点） */
const ALLOWED_EXT = new Set([
  '.json', '.html', '.htm', '.css', '.js', '.mjs', '.cjs',
  '.txt', '.md', '.svg', '.png', '.jpg', '.jpeg', '.gif', '.webp',
  '.ico', '.woff', '.woff2', '.ttf', '.otf',
  // 约定 46：包内无扩展名的 LICENSE 在打包时会被改名成 LICENSE.txt，
  // 所以 .txt 覆盖得到；但**不要**因此放宽成「允许无扩展名」——
  // 那会让任意可执行文件（无扩展名的 sh/py）混进包里。
])

/** 一律禁掉的路径片段：这些目录不进包，也不该出现在包里 */
const DENY_DIRS = ['node_modules/', '.git/', '.DS_Store']

/** manifest.json 解压后的大小上限（防 zip bomb） */
const MAX_MANIFEST_BYTES = 256 * 1024

/** 扩展包允许申请的权限全集 —— 超出即拒（约定 32 的能力表） */
const ALLOWED_PERMISSIONS = new Set([
  'storage', 'config', 'shared-storage', 'network', 'system', 'clipboard',
  'open-url', 'files', 'notify', 'window', 'events', 'theme',
])

export interface GateResult {
  ok: boolean
  /** 不通过的原因（第一条）—— 直接写进 review_note 给审核人看 */
  reason?: string
  /** 全部问题（给人看的完整清单，不只第一条） */
  problems: string[]
  /** 清单里用到的权限（审核人要知道这个包要什么） */
  permissionsUsed: string[]
  fileCount: number
  totalSize: number
}

/**
 * 跑关卡。
 *
 * @param entries 中央目录条目
 * @param readFile 按路径取解压后内容（只在需要读 manifest 时才调用）；
 *                 返回 null 表示取不到（关卡据此判定「无法核对」）
 * @param manifestId 若给出，manifest.json 的 id 必须与它一致（防「审核的是 A、装的是 B」）
 */
export async function runGate(
  entries: ZipEntry[],
  readFile: (path: string) => Promise<Uint8Array | null>,
  manifestId?: string,
): Promise<GateResult> {
  const problems: string[] = []
  const files = entries.filter((e) => !e.path.endsWith('/'))

  // ---- ① 加密包一律拒 ----
  // 解不开就无法审核；且加密包常被用来藏东西。
  const enc = files.find((e) => e.encrypted)
  if (enc) problems.push(`包内有加密条目，无法审核：${enc.path}`)

  // ---- ② 路径穿越 ----
  // 服务端**不会**把包解到磁盘（发布时由 seed 脚本在本机解），
  // 但关卡要挡在前面：一旦将来加了「服务端解压」或「审核页展示内容」，
  // 一个 `../../.ssh/authorized_keys` 就是任意写。
  for (const e of files) {
    if (e.path.includes('..') || e.path.startsWith('/') || /^[A-Za-z]:/.test(e.path)) {
      problems.push(`包内路径非法（可能越权写入）：${e.path}`)
      break
    }
  }

  // ---- ③ 不该存在的目录 ----
  // 约定 46：打包时就该排除 node_modules。出现在包里说明打包有 bug 或
  // 有人手改了包 —— 两种都不该上架。
  for (const d of DENY_DIRS) {
    const hit = files.find((e) => e.path.includes(d))
    if (hit) {
      problems.push(`包内不该包含 ${d}（约定 46 打包时就应排除）：${hit.path}`)
      break
    }
  }

  // ---- ④ 扩展名白名单 ----
  const bad = files.find((e) => {
    const base = e.path.slice(e.path.lastIndexOf('/') + 1)
    if (!base.includes('.')) return true // 无扩展名 → 拒（见 ALLOWED_EXT 注释）
    const i = base.lastIndexOf('.')
    return !ALLOWED_EXT.has(base.slice(i).toLowerCase())
  })
  if (bad) {
    problems.push(
      `包内文件扩展名不在白名单内：${bad.path}。` +
        `允许：${[...ALLOWED_EXT].join(' ')}`,
    )
  }

  // ---- ⑤ manifest 必须在包根 ----
  // 约定 46：manifest 在根，否则客户端扫不到 → 装了也是「不可用」。
  const rootManifest = files.find((e) => e.path === 'manifest.json')
  if (!rootManifest) {
    problems.push('包根缺少 manifest.json（约定 46：它必须在包根，否则客户端扫不到这个扩展）')
  } else if (rootManifest.size > MAX_MANIFEST_BYTES) {
    problems.push(`manifest.json 过大（${rootManifest.size} 字节 > ${MAX_MANIFEST_BYTES}）`)
  }

  // ---- ⑥ 体积 ----
  const totalSize = files.reduce((n, e) => n + e.size, 0)
  const huge = files.find((e) => e.size > 32 * 1024 * 1024)
  if (huge) problems.push(`单个文件过大：${huge.path}（${(huge.size / 1048576).toFixed(1)}MB）`)

  // ---- ⑦ 权限：读 manifest 核对 ----
  let permissionsUsed: string[] = []
  if (rootManifest && rootManifest.size <= MAX_MANIFEST_BYTES) {
    const raw = await readFile('manifest.json').catch(() => null)
    if (!raw) {
      problems.push('读不出 manifest.json 的内容，无法核对权限（包可能已损坏）')
    } else {
      let mf: { id?: unknown; permissions?: unknown }
      try {
        mf = JSON.parse(new TextDecoder().decode(raw)) as typeof mf
      } catch (e) {
        // ⚠️ 这里原来**没有** try：manifest 是乱码/截断时 JSON.parse 直接抛，
        //    异常一路冒到 handle.ts 的兜底 → 客户端收到 `server_error` 500，
        //    **完全看不出是「包坏了」**。而提交者上传的包坏了是很常见的事，
        //    必须给出「manifest 不是合法 JSON」这种能照着改的说法。
        problems.push(`manifest.json 不是合法 JSON：${String(e).slice(0, 120)}`)
        return {
          ok: false,
          reason: problems[0],
          problems,
          permissionsUsed: [],
          fileCount: files.length,
          totalSize,
        }
      }
      const decl = Array.isArray(mf.permissions) ? mf.permissions.filter((p) => typeof p === 'string') : []
      const over = decl.filter((p) => !ALLOWED_PERMISSIONS.has(p))
      if (over.length) {
        // ⚠️ 这条对应 AGENTS.md 里的 `gate.ts::permissionsUsedInSource` 家族。
        // 超出全集比「多要了权限」更严重：那是**不存在**的能力，
        // 通常意味着 manifest 写错了、或想装一个更宽松的宿主。
        problems.push(`manifest 申请了不存在的权限：${over.join(', ')}`)
      }
      permissionsUsed = decl
      if (manifestId && typeof mf.id === 'string' && mf.id !== manifestId) {
        problems.push(`manifest 里的 id（${mf.id}）与提交的扩展 id（${manifestId}）不一致`)
      }
    }
  }

  return {
    ok: problems.length === 0,
    reason: problems[0],
    problems,
    permissionsUsed,
    fileCount: files.length,
    totalSize,
  }
}