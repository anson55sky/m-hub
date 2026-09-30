#!/usr/bin/env node
// 「我现在到哪一步了」—— 按顺序检查上站的每个前置条件。
//
// 为什么用脚本而不是 README 里的清单：清单是静态的，你做完一步不会有人
// 告诉你「第 3 步好了」。这个脚本每跑一次都重新判断，所以随时可以跑，
// 不用回忆自己走到哪了。
//
//   node scripts/onboard-status.mjs
//
// 只做**只读**检查，不改任何东西、不联网部署。

import { existsSync, readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { dirname, join } from 'node:path'
import { createPublicKey, verify } from 'node:crypto'

const SERVER = join(dirname(fileURLToPath(import.meta.url)), '..')
const CLIENT = join(SERVER, '..')
const read = (p) => readFileSync(p, 'utf8')

const steps = []
const step = (name, ok, detail, hint) => steps.push({ name, ok, detail, hint })

// ---------------------------------------------------------------- 1. Node
step(
  'Node.js ≥ 20',
  Number(process.versions.node.split('.')[0]) >= 20,
  `当前 ${process.version}`,
  'brew install node',
)

// ---------------------------------------------------------------- 2. wrangler
const wranglerInstalled = existsSync(join(SERVER, 'node_modules/wrangler'))
step(
  'wrangler 已装',
  wranglerInstalled,
  wranglerInstalled ? 'server/node_modules/wrangler' : '缺失',
  'cd server && npm install',
)

// ---------------------------------------------------------------- 3. 已登录
// wrangler 的凭据位置**分平台**：macOS 走 ~/Library/Preferences/，
// Linux/Windows 走 ~/.wrangler/。第一版只写了后者，于是 macOS 上明明
// 登录成功了、脚本却报「未登录」—— 误报比不报更坏，它会让人去重登一遍。
// 优先用 `wrangler whoami` 判定（那是权威），文件路径只作快速路径。
const credPaths = [
  join(process.env.HOME, 'Library/Preferences/.wrangler/config/default.toml'),
  join(process.env.HOME, '.wrangler/config/default.toml'),
  join(process.env.HOME, '.config/.wrangler/config/default.toml'),
]
const credPath = credPaths.find((p) => existsSync(p))
const loggedIn = !!credPath
step(
  'wrangler 已登录',
  loggedIn,
  loggedIn ? credPath : '没找到 ~/.wrangler/config',
  'cd server && npx wrangler login（浏览器点 Allow）',
)

// ---------------------------------------------------------------- 4. database_id 已填
const wranglerRaw = read(join(SERVER, 'wrangler.jsonc'))
// 去掉 // 注释再取 JSON（jsonc）
const wrangler = JSON.parse(
  wranglerRaw
    .split('\n')
    .map((l) => l.replace(/^\s*\/\/.*$/, ''))
    .join('\n'),
)
const db = wrangler.d1_databases?.[0]
const dbId = db?.database_id ?? ''
const dbFilled = !!dbId && !dbId.startsWith('REPLACE_ME')
step(
  'D1 的 database_id 已填进 wrangler.jsonc',
  dbFilled,
  dbFilled ? `${db.name} → ${dbId}` : `当前是占位符：${dbId}`,
  'npx wrangler d1 create m-hub，把输出的 database_id 填进 wrangler.jsonc',
)

// ---------------------------------------------------------------- 5. 密钥
// secret 的值读不出来（Cloudflare 不回传），所以只能查「配过没有」——
// 通过 deploy 时的 manifest 判断。这里改成检查 vars 里是否还是空串。
const vars = wrangler.vars ?? {}
const emptyVars = Object.entries(vars)
  .filter(([k, v]) => v === '' && k !== 'GITHUB_CLIENT_ID')
  .map(([k]) => k)
step(
  'wrangler.jsonc 的 vars 无空串残留',
  emptyVars.length === 0,
  emptyVars.length ? `还是空的：${emptyVars.join(', ')}` : 'OK',
  '这些走 secret put（值不回传、密钥不进仓库）',
)

// ---------------------------------------------------------------- 6. 静态清单已生成
const pubDir = join(SERVER, 'public')
const statics = [
  '/api/v1/market/registry',
  '/api/v1/app/update',
]
const missing = []
for (const p of statics) {
  for (const suf of ['', '.sig']) {
    if (!existsSync(join(pubDir, p.replace(/^\//, '') + suf))) missing.push(p + suf)
  }
}
step(
  '两份签名清单已生成',
  missing.length === 0,
  missing.length ? `缺：${missing.join(', ')}` : '4 个文件都在',
  'cd server && npm run seed:manifests',
)

// ---------------------------------------------------------------- 6.5 workers.dev 子域已注册
//
// 这一步**独立于** `wrangler.jsonc`：那里的 `workers_dev: true` 只是声明
// 「发布到 workers.dev」，不负责**创建**那个子域。子域是账号级一次性设置。
// 不注册的症状很有迷惑性：Worker 明明 `Uploaded m-hub-server` 成功了，
// 整条 deploy 却仍失败 —— 所以不能拿「上传成功」当成功判据。
const deployedUrlFile = join(SERVER, '.deployed-url')
const deployedUrl = existsSync(deployedUrlFile) ? read(deployedUrlFile).trim() : ''
step(
  'workers.dev 子域已注册（否则 deploy 整体失败）',
  !!deployedUrl,
  deployedUrl || '未知 —— 查不到已部署地址',
  '网页注册：dash.cloudflare.com/<账号ID>/workers/onboarding → Workers & Pages → 你的 Worker → Your subdomain → Change',
)

// ---------------------------------------------------------------- 7. 签名验得过
const pubKeyFile = join(CLIENT, 'src-tauri/keys/market_public.key')
const SPKI = Buffer.from('302a300506032b6570032100', 'hex')
let sigOk = false
let sigDetail = ''
if (!missing.length) {
  const raw = Buffer.from(read(pubKeyFile).trim(), 'base64')
  const key = createPublicKey({ key: Buffer.concat([SPKI, raw]), format: 'der', type: 'spki' })
  for (const p of statics) {
    const f = join(pubDir, p.replace(/^\//, ''))
    const ok = verify(null, readFileSync(f), key, Buffer.from(read(f + '.sig').trim(), 'base64'))
    if (!ok) {
      sigOk = false
      sigDetail = `${p} 验不过`
      break
    }
    sigOk = true
    sigDetail = '两份都验得过内嵌公钥'
  }
} else {
  sigDetail = '跳过（清单还没生成）'
}
step('清单签名验得过客户端内嵌公钥', sigOk, sigDetail, '重跑 npm run seed:manifests（公钥轮换过就必须重签）')

// ---------------------------------------------------------------- 8. 客户端常量
const configSrc = read(join(CLIENT, 'src-tauri/src/config.rs'))
const m = /DEFAULT_SERVER_URL:\s*&str\s*=\s*"([^"]+)"/.exec(configSrc)
const currentUrl = m?.[1] ?? '(没找到)'
const stillOld = currentUrl.includes('xfactor.top')
step(
  '客户端地址常量',
  !stillOld,
  stillOld ? `仍是 ${currentUrl}（该域名 NXDOMAIN）` : currentUrl,
  '部署成功后改成你的 workers.dev 地址，然后重新构建客户端',
)

// ---------------------------------------------------------------- 9. 客户端构建
const builtBefore = existsSync(join(CLIENT, 'src-tauri/target/release'))
step(
  '客户端曾构建过 release',
  builtBefore,
  builtBefore ? 'target/release 存在' : '没构建过（会自动更新，不能建 .app 包）',
  'cd .. && npm run tauri:build',
)

// ---------------------------------------------------------------- 输出

const done = steps.filter((s) => s.ok).length
console.log('\nm-hub-server 上站进度\n' + '='.repeat(52))
for (const [i, s] of steps.entries()) {
  console.log(`${s.ok ? '✓' : '·'} ${String(i + 1).padStart(2)}. ${s.name}`)
  console.log(`      ${s.detail}`)
  if (!s.ok && s.hint) console.log(`      → ${s.hint}`)
}
console.log('='.repeat(52))
console.log(`${done}/${steps.length} 项就绪。`)

const nextIdx = steps.findIndex((s) => !s.ok)
if (nextIdx === -1) {
  console.log('\n全部就绪 → cd server && npm run deploy\n')
} else {
  const s = steps[nextIdx]
  console.log(`\n下一个要做：第 ${nextIdx + 1} 步 —— ${s.name}`)
  if (s.hint) console.log(`  ${s.hint}\n`)
}
