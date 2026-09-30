#!/usr/bin/env node
// 生成 `server/public/` 下的四份静态文件：两份清单 + 两份分离签名。
//
// ## 为什么这个脚本存在
//
// 客户端的签名机制验的是**下载到的原始字节**，所以「发布」这件事有不可省的手工部分：
// 签名必须在**字节确定之后**做，且签完不能再改。本脚本把那串步骤固定下来，
// 避免出现「改了 JSON 又忘了重签 → 线上验签失败 → 客户端静默回退缓存」。
//
// ## 私钥从哪来
//
// `~/.m-hub-signing/market_private.pem`（由 `bash scripts/market-keygen.sh` 生成，
// 在仓库之外）。**本脚本不生成私钥、不打印私钥、不把它带出那台机器。**
// 没找到就报错并告诉你先跑 keygen —— 不静默用别的密钥，那会签出验不过的东西。
//
// ## 用法
//
//   # 只发市场清单（扩展），不带应用更新
//   node scripts/seed-manifests.mjs
//
//   # 连应用更新一起发（需要已构建好的 .dmg）
//   node scripts/seed-manifests.mjs --dmg "src-tauri/target/release/bundle/dmg/m-hub_0.7.2_aarch64.dmg"

import { readFileSync, writeFileSync, existsSync, mkdirSync, readdirSync, statSync } from 'node:fs'
import { execFileSync } from 'node:child_process'
import { createHash } from 'node:crypto'
import { fileURLToPath } from 'node:url'
import { dirname, join } from 'node:path'

// 本脚本在 server/scripts/ 下，故 dirname/.. = server 目录。两层根要分清：
// 产物落 server/，而版本号要读**客户端**的 tauri.conf.json（在上一层）。
// 混成一个 ROOT 时会拼出 `server/server/public` 这种路径（写的时候不报错，
// 部署时才发现清单没上线）。
const SERVER_ROOT = join(dirname(fileURLToPath(import.meta.url)), '..')
const CLIENT_ROOT = join(SERVER_ROOT, '..')
const PUBLIC = join(SERVER_ROOT, 'public')
const EXT_SRC = join(SERVER_ROOT, 'manifests/extensions')
const KEY_DIR = process.env.MHUB_SIGNING_DIR || join(process.env.HOME, '.m-hub-signing')
const PRIV = join(KEY_DIR, 'market_private.pem')

// ---------------------------------------------------------------- 平台键
//
// 必须与 Rust 侧 `updater::platform_key()` 逐字一致 —— 那是客户端与服务端之间
// 唯一的契约（约定 47 的精神）。这里手写而不是从 Rust import，因为脚本是
// 独立的 Node 进程；`check-api-spec-conformance.mjs` 负责在构建期对账。
const PLATFORM_KEY =
  process.platform === 'darwin' && process.arch === 'arm64' ? 'macos-aarch64'
  : process.platform === 'darwin' ? 'macos-x86_64'
  : 'unknown-unsupported'

// ---------------------------------------------------------------- 工具

const die = (m) => { console.error(`✗ ${m}`); process.exit(1) }

function sha256File(p) {
  return createHash('sha256').update(readFileSync(p)).digest('hex')
}

/**
 * 签名并写出 `.sig`。
 *
 * ⚠️ `-rawin` 的**输入**必须是普通文件，不能是管道（管道下 openssl 拿不到
 * 文件长度，报 "unable to determine file size for oneshot operation"）。踩过一次。
 * 管道放在**输出**侧（openssl stdout → base64）没问题。
 */
function sign(file) {
  execFileSync(
    'sh',
    ['-c', `openssl pkeyutl -sign -inkey ${shq(PRIV)} -rawin -in ${shq(file)} | base64 | tr -d '\\n' > ${shq(file + '.sig')}`],
  )
}

/** shell 单引号转义。路径里可能有空格（`/Users/sky/Desktop/项目/CODE/…` 就有）。 */
function shq(s) {
  return `'${s.replace(/'/g, `'\\''`)}'`
}

// ---------------------------------------------------------------- 1. 校验私钥

if (!existsSync(PRIV)) {
  die(
    `找不到私钥：${PRIV}\n` +
      `  先跑：bash scripts/market-keygen.sh\n` +
      `  （它会把公钥写进 src-tauri/keys/market_public.key —— **改了公钥必须重新构建客户端**，\n` +
      `   否则客户端内嵌的还是旧公钥，验签必然失败）`,
  )
}

// ---------------------------------------------------------------- 2. 市场清单

mkdirSync(join(PUBLIC, 'api/v1/market'), { recursive: true })
mkdirSync(join(PUBLIC, 'api/v1/app'), { recursive: true })

const extensions = []
if (existsSync(EXT_SRC)) {
  for (const id of readdirSync(EXT_SRC)) {
    const dir = join(EXT_SRC, id)
    if (!statSync(dir).isDirectory()) continue
    const mpath = join(dir, 'manifest.json')
    if (!existsSync(mpath)) {
      console.error(`  跳过 ${id}：没有 manifest.json`)
      continue
    }
    const manifest = JSON.parse(readFileSync(mpath, 'utf8'))

    // 打包：.xhpack 就是 zip，manifest 必须在包根，排除 node_modules 与 . 开头项
    const xhpack = join(SERVER_ROOT, 'manifests', `${id}-${manifest.version}.xhpack`)
    execFileSync('zip', [
      '-r', '-q', '-X', xhpack, '.',
      '-x', 'node_modules/*', '*/node_modules/*', '.*', '*/.*',
    ], { cwd: dir })

    extensions.push({
      id: manifest.id || id,
      name: manifest.name || id,
      version: manifest.version,
      description: manifest.description || '',
      runtime: manifest.type || manifest.runtime || 'module',
      author: manifest.author || '',
      // ⚠️ downloadUrl 指向 GitHub Releases，不指向本服务器 —— 省掉整块文件托管
      // （约定：包本体不必自建，客户端只管按 URL 下载）
      downloadUrl: `https://github.com/${process.env.MHUB_RELEASE_REPO || 'anson55sky/m-hub-extensions'}` +
        `/releases/download/${manifest.id || id}-v${manifest.version}/${manifest.id || id}-${manifest.version}.xhpack`,
      sha256: sha256File(xhpack),
      size: statSync(xhpack).size,
      icon: '',
    })
    console.log(`  ✓ ${id} v${manifest.version} → ${(statSync(xhpack).size / 1024).toFixed(0)} KB`)
  }
}

const registry = {
  schemaVersion: 2, // ⚠️ 约定 46：加字段只追加，**不抬版本号**（抬了老客户端市场全空）
  updatedAt: new Date().toISOString(),
  revoked: [], // 下架只往这里加 `id@version`，**绝不**改 submissions.status（约定 60）
  extensions,
}

const registryPath = join(PUBLIC, 'api/v1/market/registry')
writeFileSync(registryPath, JSON.stringify(registry, null, 2))
sign(registryPath)
console.log(`\n✓ 市场清单 ${extensions.length} 个扩展 → server/public${"/" + registryPath.slice(PUBLIC.length + 1)}`)

// ---------------------------------------------------------------- 3. 应用更新清单

const dmgIdx = process.argv.indexOf('--dmg')
const version = (process.env.MHUB_VERSION || JSON.parse(readFileSync(join(CLIENT_ROOT, 'src-tauri/tauri.conf.json'), 'utf8')).version)
const platforms = {}

if (dmgIdx > -1) {
  const dmg = process.argv[dmgIdx + 1]
  if (!dmg || !existsSync(dmg)) die(`--dmg 指定的文件不存在：${dmg}`)
  platforms[PLATFORM_KEY] = {
    url: `https://github.com/${process.env.MHUB_RELEASE_REPO || 'anson55sky/m-hub'}` +
         `/releases/download/v${version}/m-hub_${version}_aarch64.dmg`,
    sha256: sha256File(dmg),
    size: statSync(dmg).size,
  }
  console.log(`✓ 更新清单 v${version} · ${PLATFORM_KEY} · ${(statSync(dmg).size / 1048576).toFixed(1)} MB`)
} else {
  // ⚠️ 空 platforms 是**故意**的：它让客户端明确报「清单没有 macos-aarch64 条目」，
  // 而不是谎报「已是最新版本」（那正是本轮修掉的谎报，约定 47 附近的注释）。
  // 想真发版就带 --dmg。
  console.log(
    `\n! 未提供 --dmg，更新清单的 platforms 为空。\n` +
      `  这不是 bug：客户端会明确报「清单没有 ${PLATFORM_KEY} 条目」而不是谎报已是最新。\n` +
      `  真要发版：先 npm run tauri:build，再 --dmg "src-tauri/target/release/bundle/dmg/....dmg"`,
  )
}

const update = {
  schemaVersion: 1, // ⚠️ 升级清单是 v1，市场清单才是 v2 —— 两份清单各有一套版本号
  version,
  minimumUpgradable: process.env.MHUB_MIN_UPGRADABLE || '',
  notes: process.env.MHUB_NOTES || '',
  platforms,
}
const updatePath = join(PUBLIC, 'api/v1/app/update')
writeFileSync(updatePath, JSON.stringify(update, null, 2))
sign(updatePath)
console.log(`✓ 更新清单 → server/public${"/" + updatePath.slice(PUBLIC.length + 1)}`)

console.log(
  `\n⚠️  签完的字节就是线上字节。这两个 .json 现在可以直接 deploy，\n` +
    `   但**任何进一步修改都会让验签失败** —— 改了必须重跑本脚本。`,
)
