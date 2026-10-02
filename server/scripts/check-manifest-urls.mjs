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

import { readFileSync, existsSync, writeFileSync, rmSync } from 'node:fs'
import { createHash } from 'node:crypto'
import { fileURLToPath } from 'node:url'
import { dirname, join, normalize } from 'node:path'
import { tmpdir } from 'node:os'

const ROOT = join(dirname(fileURLToPath(import.meta.url)), '..')
const PUBLIC = join(ROOT, 'public')
const REGISTRY = join(PUBLIC, 'api/v1/market/registry')
const UPDATE = join(PUBLIC, 'api/v1/app/update')

const problems = []
const fail = (m) => problems.push(m)

/**
 * 清单里 URL 预期使用的主机。
 *
 * ⚠️ 为什么要钉：迁移托管处（Workers → Pages → PocketBay）时，**最容易漏的
 * 就是这个**。清单里的 `downloadUrl` 是签名前写死的字节，改了地址忘了重签 /
 * 忘了重跑 seed 脚本，就会出现「清单取自新部署、包却指向旧部署」的跨部署混用。
 *
 * 症状很有欺骗性：清单验签**通过**（它确实被正确签过）、包也能下载，
 * 只是版本对不上 —— 或者旧部署被关掉后变成 404。这类问题要等到
 * 真实安装或版本替换时才暴露。
 */
const EXPECTED_HOST = (process.env.MHUB_SERVER_URL || 'https://m-hub-server.pages.dev')
  .replace(/\/+$/, '')
  .replace(/^https?:\/\//, '')

/**
 * ⚠️ 清单现在有**两个来源**，本脚本要明确检查的是哪一个。
 *
 * 2026-10-02 起 `/api/v1/market/registry` 改成**运行期签名**：
 * 字节存在 D1 的 `market_registry` 单行里，由 `POST /api/v1/admin/market/rebuild`
 * 生成 —— 上架必须能在审核台点一下就生效，而构建期脚本要求有人在持有私钥的
 * 机器上跑命令（用户报的就是「没有上架按钮、市场还是旧版」）。
 *
 * 于是本地 `public/api/v1/market/registry` **不再存在**，那是正常的。
 * 本脚本改为：优先读本地文件（构建期脚本刚跑过时它在），没有就读线上。
 * 两者都没有 → 报「还没上架过任何东西」，那才是真问题。
 */
const ONLINE = `https://${EXPECTED_HOST}/api/v1/market/registry`

/** 清单字节：本地优先（构建期脚本刚跑过），没有就读线上 */
let registryText = existsSync(REGISTRY) ? readFileSync(REGISTRY, 'utf8') : null
if (registryText === null) {
  try {
    const res = await fetch(ONLINE)
    if (res.ok) registryText = await res.text()
  } catch {
    /* 下面统一报错 */
  }
}
if (registryText === null) {
  console.error(
    `✗ 市场清单拿不到（本地 public/ 没有，线上 ${ONLINE} 也没有）。\n` +
      `    说明清单从没被生成过 —— 在审核台把某个扩展点「上架」，或跑：\n` +
      `      npm run review -- rebuild-market`,
  )
  process.exit(1)
}
const registry = JSON.parse(registryText)

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
    if (u.host !== EXPECTED_HOST) {
      fail(
        `${where}：downloadUrl 的主机是 ${u.host}，预期 ${EXPECTED_HOST}。\n` +
          `    改了托管处必须重跑 seed-manifests（清单是签名前的字节），\n` +
          `    否则会出现「清单来自新部署、包却指向旧部署」的跨部署混用。`,
      )
      continue
    }
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

    /**
     * 包字节取自**那个 URL 本身**，不是 `server/public/` 下的文件。
     *
     * ⚠️ 2026-10-02 起扩展包改成运行期从 D1 直发（routes/pkg.ts），
     *   `public/packages/` 已删除。查磁盘会永远报「文件不在 public/ 下」，
     *   而真正的故障（URL 取不到 / 字节对不上）反而看不见。
     *
     *   而且**取线上**比查磁盘更强：它验的是「客户端真正会下载到的那份字节」，
     *   包含部署漏了、Functions 没生效、路由写错这些**磁盘检查全都看不到**的问题。
     *   本地文件（构建期脚本刚跑过时）只作为离线兜底 —— CI 里没有网络。
     */
    let buf = null
    if (existsSync(file)) {
      buf = readFileSync(file)
    } else {
      try {
        const res = await fetch(u.href)
        if (res.ok) buf = Buffer.from(await res.arrayBuffer())
        else fail(`${where}：包地址取不到 —— HTTP ${res.status}（${u.host}${u.pathname}）`)
      } catch (err) {
        fail(`${where}：包地址取不到 —— ${String(err)}`)
      }
    }
    if (!buf) continue
    const sha = createHash('sha256').update(buf).digest('hex')
    if (e.sha256 && sha !== e.sha256) {
      fail(
        `${where}：包内容与清单里的 sha256 不一致。\n` +
          `    清单 ${e.sha256.slice(0, 16)}… / 实际 ${sha.slice(0, 16)}…\n` +
          `    客户端会在下载后校验 sha256，不一致即安装失败。`,
      )
    }
    const size = buf.byteLength
    if (typeof e.size === 'number' && e.size !== size) {
      fail(`${where}：包大小与清单不符（清单 ${e.size} / 实际 ${size}）`)
    }
    // sha256 与清单一致**不等于**包可用：两者可以「一致地错」（打包时打错了、
    // sha256 也是对着错文件算的）。而客户端的失败点在解包，症状是
    // 「下载成功 → 解压失败」，与 404 是完全不同的现象、却同样只在实机暴露。
    // 故这里真的解一遍，并要求 manifest.json 在**包根**（客户端的硬要求）。
    if (rel.endsWith('.xhpack')) {
      try {
        const { execFileSync } = await import('node:child_process')
        // ⚠️ 必须解**手上那份 buf**，不能解磁盘路径：包已经是运行期从 D1 直发的，
        //   `public/packages/` 早就删了，原来的 `file` 现在指向一个不存在的路径。
        //   （症状是 `unzip: cannot find or open …` —— 看着像包坏了，其实是取错了源。）
        const tmp = join(tmpdir(), `mhub-check-${Date.now()}.xhpack`)
        writeFileSync(tmp, buf)
        let raw
        try {
          raw = execFileSync('unzip', ['-Z1', tmp], { encoding: 'utf8' })
        } finally {
          rmSync(tmp, { force: true })
        }
        const list = raw
          .split('\n')
          .map((x) => x.trim())
          .filter(Boolean)
        if (!list.includes('manifest.json')) {
          fail(
            `${where}：包根没有 manifest.json（实际条目：${list.join(', ') || '空'}）。\n` +
              '    客户端要求 manifest 在包根，否则安装后扩展不可用。',
          )
        }
      } catch (e) {
        fail(`${where}：扩展包无法解开（${String(e).slice(0, 60)}）—— 客户端会在解包阶段失败`)
      }
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
          if (typeof entry.size === 'number' && entry.size !== buf.byteLength) {
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
