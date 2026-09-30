#!/usr/bin/env node
// 清单自检：里面每一个 URL 都必须**真的取得到**。
//
// ## 为什么需要它
//
// 踩过一次真实的坑：最初为了「省掉文件托管」，把清单里的 `downloadUrl`
// 指向 `github.com/<某仓库>/releases/download/…`，而**那个 release 并不存在**。
// 后果是用户点「安装」报 `下载失败: HTTP 404`，而：
//   · 服务端一切正常（清单、签名、接口全 200）
//   · 签名验证通过（sha256 是对的，只是那个文件不存在）
//   · 客户端版本、日志都看不出问题
//
// 也就是说**清单本身合法、验签通过，只是里面的 URL 指向了一个不存在的东西**。
// 这类错误没有任何现有守卫能抓到 —— 直到真人点一次「安装」。
//
// 故本脚本在**构建期**逐条验证：
//   ① downloadUrl 的**路径**能在 `public/` 下找到对应文件（离线、快、不依赖网络）
//   ② 该文件的 sha256 与清单里写的一致（顺带抓「清单与包版本不匹配」）
//   ③ size 一致
//
// 另有两条结构断言：schemaVersion 必须是 2（抬版本会让老客户端市场全空）、
// `revoked` 必须是数组（约定 60：下架只往这里加 `id@version`）。

import { readFileSync, existsSync, statSync } from 'node:fs'
import { createHash } from 'node:crypto'
import { fileURLToPath } from 'node:url'
import { dirname, join, normalize } from 'node:path'

const ROOT = join(dirname(fileURLToPath(import.meta.url)), '..')
const PUBLIC = join(ROOT, 'public')
const REGISTRY = join(PUBLIC, 'api/v1/market/registry')
const UPDATE = join(PUBLIC, 'api/v1/app/update')

const problems = []
const fail = (m) => problems.push(m)

if (!existsSync(REGISTRY)) {
  console.error('✗ 市场清单不存在。请先在 server/ 下跑 npm run seed:manifests')
  process.exit(1)
}
const registry = JSON.parse(readFileSync(REGISTRY, 'utf8'))

// ---------------------------------------------------------------- 结构

if (registry.schemaVersion !== 2) {
  fail(
    `schemaVersion 必须是 2，当前 ${registry.schemaVersion}。` +
      '约定 46：加字段只追加，**不抬版本号** —— 抬了老客户端的市场会直接变空。',
  )
}
if (!Array.isArray(registry.revoked)) {
  fail('revoked 必须是数组（约定 60：平台下架只往这里加 `id@version`，绝不改 submissions.status）')
}
if (!Array.isArray(registry.extensions)) fail('extensions 必须是数组')

// ---------------------------------------------------------------- downloadUrl

for (const e of registry.extensions ?? []) {
  const where = `扩展 ${e.id}@${e.version}`
  if (!e.downloadUrl) {
    fail(`${where}：缺 downloadUrl`)
    continue
  }
  if (/^https?:\/\//.test(e.downloadUrl)) {
    let u
    try {
      u = new URL(e.downloadUrl)
    } catch {
      fail(`${where}：downloadUrl 不是合法 URL（${e.downloadUrl}）`)
      continue
    }
    // 只在**本服务端**托管时才做离线校验；外链（如对象存储 CDN）跳过，
    // 但要提醒 —— 外链的存在性无法在构建期保证。
    if (!u.pathname.startsWith('/packages/')) {
      fail(
        `${where}：downloadUrl 指向外部主机（${u.host}）。\n` +
          `    本项目已改为服务端托管（/packages/…）。指外部地址时那个 URL 必须由你` +
          `真的创建出来 —— 指向不存在的 GitHub release 是踩过的坑，症状是用户点安装报 404。`,
      )
      continue
    }
    const rel = normalize(u.pathname.replace(/^\//, ''))
    const file = join(PUBLIC, rel)
    if (!file.startsWith(PUBLIC)) {
      fail(`${where}：downloadUrl 路径越界（${u.pathname}）`)
      continue
    }
    if (!existsSync(file)) {
      fail(
        `${where}：清单指向 ${rel}，但该文件不在 server/public/ 下。\n` +
          `    包不会被部署 → 用户点安装报 HTTP 404。跑 npm run seed:manifests 重新生成。`,
      )
      continue
    }
    // sha256 / size 一致性：抓到「清单与包版本不匹配」——
    // 症状是客户端 sha256 校验失败（ERR），比 404 更难查
    const buf = readFileSync(file)
    const sha = createHash('sha256').update(buf).digest('hex')
    if (e.sha256 && sha !== e.sha256) {
      fail(
        `${where}：包内容与清单里的 sha256 不一致。\n` +
          `    清单 ${e.sha256.slice(0, 16)}… / 实际 ${sha.slice(0, 16)}…\n` +
          `    客户端会在下载后校验 sha256，不一致即安装失败。`,
      )
    }
    const size = statSync(file).size
    if (typeof e.size === 'number' && e.size !== size) {
      fail(`${where}：包大小与清单不符（清单 ${e.size} / 实际 ${size}）`)
    }
    // 清单里的 sha256/size 必须是**本机算出来的**，不是手抄的
    if (!e.sha256) fail(`${where}：缺 sha256（客户端会拒绝安装）`)
  }
}

// ---------------------------------------------------------------- 更新清单

if (existsSync(UPDATE)) {
  const u = JSON.parse(readFileSync(UPDATE, 'utf8'))
  if (u.schemaVersion !== 1) {
    fail(
      `更新清单的 schemaVersion 应为 1，当前 ${u.schemaVersion}。` +
        '（升级清单是 v1，市场清单才是 v2 —— 两份清单各有一套版本号）',
    )
  }
  if (!u.version) fail('更新清单缺 version')
  if (!u.platforms || typeof u.platforms !== 'object') {
    fail('更新清单缺 platforms（空对象是合法的 —— 客户端会明确报「无本平台条目」而不是谎报已是最新）')
  } else {
    const want =
      process.platform === 'darwin' && process.arch === 'arm64'
        ? 'macos-aarch64'
        : 'macos-x86_64'
    if (!u.platforms[want]) {
      fail(
        `更新清单没有 ${want} 条目（本机是 ${process.platform}/${process.arch}）。\n` +
          `    客户端会报「清单没有 ${want} 条目」—— 这是刻意行为，不是谎报。`,
      )
    } else {
      const entry = u.platforms[want]
      // 与扩展包同一类坑：URL 指向的东西必须真的取得到
      let pu
      try {
        pu = new URL(entry.url)
      } catch {
        fail(`${want}：url 不是合法 URL（${entry.url}）`)
      }
      if (pu && pu.pathname.startsWith('/downloads/')) {
        const rel = normalize(pu.pathname.replace(/^\//, ''))
        const file = join(PUBLIC, rel)
        if (!file.startsWith(PUBLIC)) {
          fail(`${want}：url 路径越界（${pu.pathname}）`)
        } else if (!existsSync(file)) {
          fail(
            `${want}：更新清单指向 ${rel}，但该文件不在 server/public/ 下。\n` +
              `    安装包不会被部署 → 用户点「立即更新」报 HTTP 404。` +
              `重跑 seed:manifests --dmg <路径>。`,
          )
        } else {
          const buf = readFileSync(file)
          const sha = createHash('sha256').update(buf).digest('hex')
          if (entry.sha256 && sha !== entry.sha256) {
            fail(`${want}：安装包 sha256 与清单不符（清单 ${entry.sha256.slice(0, 12)}… / 实际 ${sha.slice(0, 12)}…）`)
          }
          if (typeof entry.size === 'number' && entry.size !== statSync(file).size) {
            fail(`${want}：安装包大小与清单不符`)
          }
        }
      } else if (pu) {
        fail(
          `${want}：url 指向外部主机（${pu.host}）。该地址必须真的存在 ——\n` +
            `    指向不存在的 GitHub release 是踩过的坑，症状是用户点更新报 404。`,
        )
      }
    }
  }
}

// ---------------------------------------------------------------- 输出

if (problems.length) {
  console.error('✗ 清单自检未通过：\n')
  for (const p of problems) console.error(`  · ${p}`)
  console.error('\n  这类错误在部署后**看不出来**：清单与签名都合法、服务端全 200，')
  console.error('  只有真人点一次「安装」才会暴露（HTTP 404 或 sha256 校验失败）。')
  process.exit(1)
}

const n = registry.extensions?.length ?? 0
console.log(`✓ 清单自检通过：${n} 个扩展的 downloadUrl 全部指向已部署的包、sha256 与 size 一致；更新清单平台条目齐备`)
