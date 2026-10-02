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

async function api(path, wantBytes = false) {
  const res = await fetch(`${BASE}${path}`, { headers: { authorization: `Bearer ${adminToken()}` } })
  if (!res.ok) die(`取 ${path} 失败：HTTP ${res.status}`)
  return wantBytes ? new Uint8Array(await res.arrayBuffer()) : res.json()
}

async function main() {
  console.log('取已批准的提交…')
  const { items } = await api('/api/v1/admin/submissions')
  const approved = items.filter((s) => s.status === 'approved')

  if (approved.length === 0) {
    console.log('没有已批准的提交，无需发布。')
    return
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
}

main().catch((e) => {
  console.error(e)
  process.exit(1)
})