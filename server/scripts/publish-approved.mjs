#!/usr/bin/env node
// 发布已批准的扩展到市场。
//
// ## 为什么必须有这个独立步骤（而不能在审核接口里直接上架）
//
// 市场清单是**签过名的静态文件**：`api/v1/market/registry` + `.sig`。
// 签名覆盖精确字节，一旦有任何重新序列化（gzip、JSON 美化、尾随换行），
// 客户端的 Ed25519 验签就失败。而 D1 是运行期数据库，改它**不会**改动
// 已经部署到 CDN 上的那个字节。
//
// 所以上架**必然**是构建期动作：
//   读 D1 里 status='approved' → 还原成扩展源码目录 → seed 重新打包 + 签名
//   → 部署 Pages → 客户端才取得到
//
// 这也是 `approveSubmission` 返回里那句 `next` 的含义：
// **状态改了不等于已上架**，两者差这一步。
//
// ## 为什么经管理端接口取数据，而不是 wrangler d1 直连
//
// 直连 D1 会**绕过门禁与关卡**。审核接口即使只是个「读」，也走同一套
// 认证与校验 —— 这样「审核通过」与「能发布」是同一个信任边界。
//
// ## 用法
//
//   npm run publish:approved            # 只做本地准备并说明下一步（安全默认）
//   npm run publish:approved -- --deploy
//
// ⚠️ 默认**不部署**：「签了名但没部署」的状态最容易被误当成已上架。

import { execFileSync } from 'node:child_process'
import { mkdirSync, rmSync, existsSync, readdirSync, statSync, renameSync, writeFileSync, readFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'

const SERVER_ROOT = join(dirname(fileURLToPath(import.meta.url)), '..')
const EXT_ROOT = join(SERVER_ROOT, 'manifests', 'extensions')
const STAGE = join(SERVER_ROOT, 'manifests', '.staged')
const BASE = (process.env.MHUB_SERVER_URL || 'https://m-hub-server.pages.dev').replace(/\/+$/, '')

const DO_DEPLOY = process.argv.includes('--deploy')

/**
 * 找到要沿用的 .dmg（用于保持更新清单的 platforms 不被清空）。
 *
 * 优先 `MHUB_DMG`，否则在客户端的构建产物目录里找**最新的一个**。
 * 找最新的而不是写死版本号：版本会变，写死的话每次发版都要改这里，
 * 而漏改的后果是「发布扩展时自动更新静默失效」。
 */
function findDmg() {
  if (process.env.MHUB_DMG) return existsSync(process.env.MHUB_DMG) ? process.env.MHUB_DMG : null
  const dir = join(SERVER_ROOT, '..', 'src-tauri', 'target', 'release', 'bundle', 'dmg')
  if (!existsSync(dir)) return null
  const dmgs = readdirSync(dir).filter((f) => f.endsWith('.dmg'))
  if (dmgs.length === 0) return null
  dmgs.sort((a, b) => statSync(join(dir, b)).mtimeMs - statSync(join(dir, a)).mtimeMs)
  return join(dir, dmgs[0])
}

function die(msg) {
  console.error(`\n✗ ${msg}\n`)
  process.exit(1)
}

function adminToken() {
  const t = (process.env.MHUB_ADMIN_TOKEN || '').trim()
  if (!t) die('缺少 MHUB_ADMIN_TOKEN（值在 ~/.m-hub-signing/admin_token）')
  return t
}

/**
 * 与 `api()` 同签名，但**抛错而不是 exit**。
 *
 * ⚠️ `api()` 内部调 `die()` → `process.exit(1)`。这在「取提交列表」那种
 *   必需步骤上是对的（拿不到就该停），但对**部署之后的收尾动作**是错的：
 *   实测 `markWithRetry` 里调 `api()`，第一次 405 就把整个进程干掉了 ——
 *   重试逻辑根本没机会执行，日志里也看不出「它本该重试」。
 *
 *   凡是「失败可以继续、或者要自己重试」的调用，一律走这个。
 */
async function apiSoft(path, init = {}) {
  const res = await fetch(`${BASE}${path}`, {
    ...init,
    headers: {
      authorization: `Bearer ${adminToken()}`,
      ...(init.headers || {}),
    },
  })
  if (!res.ok) throw new Error(`HTTP ${res.status}`)
  return res.json()
}

async function api(path, wantBytes = false) {
  const res = await fetch(`${BASE}${path}`, { headers: { authorization: `Bearer ${adminToken()}` } })
  if (!res.ok) die(`取 ${path} 失败：HTTP ${res.status}`)
  return wantBytes ? new Uint8Array(await res.arrayBuffer()) : res.json()
}

async function main() {
  console.log('取已批准的提交…')
  const { items } = await api('/api/v1/admin/submissions')
  // ⚠️ 候选集 = `approved` ∪ `published`，不是只看 approved。
  //
  //   只看 approved 会造成一个死锁：某扩展最新的那条发完就是 `published`，
  //   于是它不再进候选集；而旧的 approved（同一扩展的更早版本）**单独**发是错的
  //   —— 少了它就没法比版本号，只能把旧版本当最新发上去（实测：v0.1.5 会被
  //   当成「唯一可发的」推上清单，把已经在架的 v0.1.6 顶掉）。
  //
  //   带上 published 是**幂等**的：重跑一次清单字节相同（版本相同、sha 相同），
  //   而比版本号时它天然会赢过那些更早的 approved。
  const candidates = items.filter((s) => s.status === 'approved' || s.status === 'published')

  if (candidates.length === 0) {
    console.log('没有可发布的提交（approved / published 都为空），无需发布。')
    return
  }

  // 同一扩展取最高版本。
  // ⚠️ 注意这里比的是**候选集**（含 published），不是「只看 approved」：
  //   最新那条发完就是 published、不再进「只看 approved」的集合，于是比版本号时
  //   只剩更早的 approved —— 清单会被旧版本顶掉（实测差点发生）。
  const latest = new Map()
  for (const s of candidates) {
    const prev = latest.get(s.extId)
    if (!prev || cmpVersion(s.version, prev.version) > 0) latest.set(s.extId, s)
  }

  // ⚠️ 真正要上架的 = 「最新版里**还没 published** 的那些」。
  //   已经是 published 的最新版**不要重发**：
  //     ① 它已经在清单里，重发的字节相同，没必要；
  //     ② `GET /admin/submissions/:id/package` **只对 approved 发包**，
  //        去取 published 的会拿到 409（`not_approved`）。
  const approved = [...latest.values()].filter((s) => s.status === 'approved')
  // ⚠️ 早期版本在这里直接 return，于是**应用自动更新清单被跳过了**。
  //   而更新清单里存着 DMG 的 sha256 —— 只要重新构建过 DMG，它就过期了。
  //   实测：清单写着 70740a02…，本地 DMG 已经是 c39baca6…，
  //   用户点「立即更新」会卡在 sha256 校验失败，而自动更新不校验就装不上。
  //
  //   所以「没有扩展要上架」≠「什么都不用做」：更新清单照样要重签。
  const NOTHING_TO_LIST = approved.length === 0
  if (NOTHING_TO_LIST) {
    console.log('每个扩展的最新版本都已上架，无需重新上架；但仍需重签更新清单（DMG 可能变了）。')
  }
  const superseded = candidates.length - approved.length
  if (superseded > 0) {
    console.log(
      `  ${candidates.length} 条候选里，${superseded} 条被更高版本取代或已在架，不会上架`,
    )
  }


  console.log(`\n已批准 ${approved.length} 条：`)
  for (const s of approved) {
    console.log(`  #${s.id}  ${s.extId} v${s.version}  by ${s.username}${s.hasPackage ? '' : '  ⚠ 包体缺失'}`)
  }

  // ⚠️ 「已批准但没有字节」是最坏的状态：清单会指向一个不存在的文件，
  //    用户点安装得到 404，而服务端一切正常。必须在发布前拦住。
  const missing = approved.filter((s) => !s.hasPackage)
  if (missing.length) {
    die(
      `${missing.length} 条的包体在库里找不到，发不出去。\n` +
        `    处理：让作者重新提交（旧的 gate_failed/approved 记录用 DELETE 清掉）。`,
    )
  }

  console.log('\n解包…')
  rmSync(STAGE, { recursive: true, force: true })
  mkdirSync(STAGE, { recursive: true })
  for (const s of approved) {
    const bytes = await api(`/api/v1/admin/submissions/${s.id}/package`, true)
    const xhpack = join(STAGE, `pkg-${s.id}.xhpack`)
    writeFileSync(xhpack, bytes)
    const dir = join(STAGE, s.extId)
    rmSync(dir, { recursive: true, force: true })
    mkdirSync(dir, { recursive: true })
    execFileSync('unzip', ['-q', '-o', xhpack, '-d', dir])
    rmSync(xhpack, { force: true })

    // ⚠️ 这里必须**校验 manifest.json 在根**并直接报错。
    //    `seed-manifests.mjs` 遇到没有根 manifest 的目录只会「跳过」并继续 ——
    //    那意味着「这个扩展没上架」被静默吞掉，而前面所有环节都显示成功。
    //
    //    实测踩过：我用 `zip ext/` 造了个包（多一层 `ext/`），发布脚本
    //    照样跑完、照样重签了清单，只是清单里没有那个扩展。
    //
    //    约定 46：manifest 必须在包根（客户端扫目录时只看根）。
    const rootManifest = join(dir, 'manifest.json')
    if (!existsSync(rootManifest)) {
      const got = readdirSync(dir).slice(0, 8).join(', ')
      die(
        `#${s.id}（${s.extId} v${s.version}）的包里没有根级 manifest.json。\n` +
          `    解开后顶层是：${got}\n` +
          `    约定 46 要求 manifest.json 在**包根**；多包一层目录的包无法上架。\n` +
          `    这是包本身的问题，不是发布脚本的 —— 请让作者用正确的打包方式重发。`,
      )
    }
    console.log(`  ✓ ${s.extId} v${s.version}`)
  }

  // 移进 seed 认识的目录；同名旧版本被覆盖（「最新已批准」就是该上架的那版）
  mkdirSync(EXT_ROOT, { recursive: true })
  for (const id of readdirSync(STAGE)) {
    const src = join(STAGE, id)
    if (!statSync(src).isDirectory()) continue
    const dest = join(EXT_ROOT, id)
    rmSync(dest, { recursive: true, force: true })
    renameSync(src, dest)
  }
  rmSync(STAGE, { recursive: true, force: true })

  // ⚠️ 必须把现有 DMG 一起传下去。
  //
  // `seed-manifests.mjs` 在没有 `--dmg` 时会写 `platforms: {}` ——
  // 那是**故意的**（约定 47：空 platforms 让客户端明确报「清单没有该平台条目」，
  // 而不是谎报已是最新）。但对**扩展发布**这个场景，它等于
  // 「连带把应用自动更新写坏了」，而两边毫无关联。
  //
  // 实测踩过：跑一次发布脚本 → 更新清单 platforms 空了 → 自动更新失效，
  // 而日志里每一步都显示成功。
  const dmg = findDmg()
  if (!dmg) {
    die(
      '找不到构建好的 .dmg，无法在重签时保留更新清单。\n' +
        '    （不传 --dmg 会把 platforms 写成空，等于连带搞坏应用自动更新。）\n' +
        '    先跑 `npm run tauri:build`（client 目录），或用 MHUB_DMG 指定路径。',
    )
  }
  console.log(`\n重打包并签名清单（沿用 ${dmg.split('/').pop()} 以保留更新清单）…`)
  execFileSync('node', [join(SERVER_ROOT, 'scripts', 'seed-manifests.mjs'), '--dmg', dmg], {
    stdio: 'inherit',
    env: { ...process.env, MHUB_SERVER_URL: BASE },
  })

  // 复核：重签后更新清单必须仍有平台条目
  const upd = JSON.parse(readFileSync(join(SERVER_ROOT, 'public/api/v1/app/update'), 'utf8'))
  if (Object.keys(upd.platforms || {}).length === 0) {
    die('重签后更新清单的 platforms 是空的 —— 应用自动更新会失效，已中止部署')
  }

  if (!existsSync(join(SERVER_ROOT, 'public', 'api', 'v1', 'market', 'registry'))) {
    die('seed 跑完却没有产出 registry —— 清单没生成，别部署')
  }

  if (!DO_DEPLOY) {
    console.log(
      '\n本地准备完成。**尚未部署** —— 清单改动要等 pages deploy 才生效。\n' +
        '确认后加 --deploy 再跑，或手动 npm run deploy:pages。',
    )
    return
  }

  console.log('\n部署到 Pages…')
  execFileSync(
    'npx',
    ['--yes', 'wrangler@latest', 'pages', 'deploy', 'public', '--project-name', 'm-hub-server', '--branch', 'main'],
    { stdio: 'inherit', cwd: SERVER_ROOT },
  )
  console.log('\n✓ 已部署。注意 Pages 有边缘传播：刚部署完立刻请求可能拿到旧内容或 522。')

  await markPublished(approved)
}

/**
 * 部署成功后，把这批提交标记为 `published`。
 *
 * ## 为什么**必须**有这一步（2026-10-02 实测）
 *
 * 状态机缺了这一环，`published` 永远到不了，于是：
 *
 *   ① 作者侧的提交记录永远显示「已通过」，而不是「已上架」——
 *      而约定 60 的整段逻辑（已下架是**客户端派生**状态，判据是
 *      `status === 'published'` 命中清单 `revoked`）根本不会触发。
 *   ② 管理端的删除接口按「`published` 不许删」保护在架版本，
 *      但因为状态永远停在 `approved`，**那条保护是死代码** ——
 *      正在市场里服役的版本可以被直接删掉。
 *      「死代码比没有更糟」：它让人以为这里有防护。
 *
 * ## 顺序：部署**之后**才改状态
 *
 * 反过来的话，清单部署失败而状态已改成 published，作者侧就会显示
 * 「已上架」而用户根本装不到 —— 与事实相反的那类谎报。
 * 部署成功是唯一能让 `published` 成真的前提。
 */
async function markPublished(approved) {
  const ids = approved.map((s) => s.id)
  if (!ids.length) return
  const done = []
  for (const s of approved) {
    try {
      await markWithRetry(s.id)
      done.push(s.id)
    } catch (e) {
      // 状态没改成不影响「已经上架」这个事实 —— 部署已完成。
      // 但必须说出来：作者侧会继续显示「已通过」，且在架版本失去删除保护。
      console.warn(`⚠ #${s.id} 没能标记为 published：${e.message ?? e}`)
    }
  }
  if (done.length) {
    console.log(`✓ 已标记 published：#${done.join(' #')}（作者侧会显示「已上架」）`)
  } else {
    console.warn(
      '⚠ 一条都没标记成功。清单已是新的，但提交状态仍是 approved ——\n' +
        '  作者侧会显示「已通过」，且在架版本不受删除保护。\n' +
        '  等两分钟后重跑本命令即可补上（发布本身是幂等的）。',
    )
  }
}

main().catch((e) => {
  console.error(e)
  process.exit(1)
})
/**
 * 标记 published，并对「新版本还没生效」重试。
 *
 * ## 为什么必须等
 *
 * `wrangler pages deploy` 报 "Deployment complete" **只代表上传完成**，
 * 边缘节点还在跑旧 worker —— 此刻调 mark-published 会拿到 **404/405**。
 * 踩过两次：一次是 405（路由不在旧版本里），一次是 522。
 * 「部署成功」与「已生效」是两件事。
 *
 * ## 为什么**重试真正的标记调用**、而不是先探活
 *
 * 试过两条错路：
 * ① 拿 `POST .../mark-published` 当**前置**探针 → 探针本身就是改数据的动作。
 *    新代码先到时，探针会把某条提交真的标成 published —— 而那次调用的目的
 *    并不是标记它。等价于「用副作用去做判断」，判据就不可信了。
 * ② 拿 `GET /admin/health` 当探针 → 它**上一次部署就已经有了**，
 *    于是探针立刻通过，而本次部署还没生效。探针必须**每次部署都不同**，
 *    而打包进函数的常量做不到。
 *
 * 直接重试真正的调用就没有这些问题：失败时**什么也没发生**（旧代码里
 * 路由不存在），成功时**正是想要的结果**。等待与副作用合成了同一次动作。
 *
 * 只对 404/405 重试：其它状态码（400/401/403/409）说明新版本已经在服务了，
 * 是这条调用本身被拒，再等也没用。
 */
async function markWithRetry(id, timeoutMs = 150_000) {
  const started = Date.now()
  let shown = false
  while (true) {
    try {
      return await apiSoft(`/api/v1/admin/submissions/${id}/mark-published`, { method: 'POST' })
    } catch (e) {
      const msg = String(e?.message ?? e)
      const stale = /HTTP (404|405|502|522)/.test(msg)
      if (!stale || Date.now() - started > timeoutMs) throw e
      if (!shown) {
        console.log('  等新部署在边缘节点生效（Pages 传播有延迟，通常 30~60s）…')
        shown = true
      }
      process.stdout.write('.')
      await new Promise((r) => setTimeout(r, 6000))
    }
  }
}

/** 三段式数字版本比较（与客户端 `parseXyz`、服务端 `compareSemver` 同口径） */
function cmpVersion(a, b) {
  const pa = String(a).split('.')
  const pb = String(b).split('.')
  for (let i = 0; i < 3; i++) {
    const d = (Number(pa[i]) || 0) - (Number(pb[i]) || 0)
    if (d !== 0) return d < 0 ? -1 : 1
  }
  return 0
}
