#!/usr/bin/env node
// 审核 CLI —— 「在哪里审核」的答案。
//
// ## 为什么是 CLI 而不是应用里做个管理界面
//
// 管理端只有一个使用者（平台所有者），而为了一个人做界面意味着：
// 多一套路由、多一套鉴权态、多一份要维护的前端。
// CLI 的另一个好处是**输出可复制**（申请理由、包清单、关卡报告都能粘出来）。
//
// 代价要说清：**没有人在浏览器里点**，所以审核是「跑一条命令」。
// 将来要给别人开放管理权限时，这里要补「谁能跑」的判断
// （现在靠 Cloudflare secret，是唯一的凭据）。
//
// ## 认证从哪来
//
// 用一个只在本地环境变量里的 `MHUB_ADMIN_TOKEN`，服务端比对。
// **不是**账号 token —— 管理端要用 `role='admin'` 的**会话**，
// 但会话 token 存在应用里（`account_token.json`），CLI 不该去读应用的私钥目录。
//
// 用法：
//   MHUB_ADMIN_TOKEN=xxx npm run review -- list
//   MHUB_ADMIN_TOKEN=xxx npm run review -- approve-dev 3 --note "理由"
//
// ## 为什么不用 wrangler d1 直改
//
// 直改 SQL 能改状态，但**跳过关卡**：包体缺失、关卡不过的提交照样能被改成
// approved，然后卡在发布脚本里。用真接口走一遍，审核动作才有意义。
import { readFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'

const SERVER_ROOT = join(dirname(fileURLToPath(import.meta.url)), '..')

const BASE = (
  process.env.MHUB_SERVER_URL || 'https://m-hub-server.pages.dev'
).replace(/\/+$/, '')

const TOKEN = process.env.MHUB_ADMIN_TOKEN || ''
const SECRET_FILE = join(process.env.HOME || '', '.m-hub-signing/admin_token')

/** 读 token：优先环境变量，其次本地文件（600 权限，且从不进版本库） */
function token() {
  if (TOKEN) return TOKEN
  try {
    return readFileSync(SECRET_FILE, 'utf8').trim()
  } catch {
    console.error(
      '缺少管理端 token。两种方式任选：\n' +
        '  MHUB_ADMIN_TOKEN=<值> npm run review -- <命令>\n' +
        `  或写进 ${SECRET_FILE}`,
    )
    process.exit(2)
  }
}

async function api(path, init = {}) {
  const res = await fetch(`${BASE}${path}`, {
    ...init,
    headers: {
      authorization: `Bearer ${token()}`,
      'content-type': 'application/json',
      ...(init.headers ?? {}),
    },
  })
  const text = await res.text()
  let body
  try {
    body = JSON.parse(text)
  } catch {
    body = { error: 'not_json', message: text.slice(0, 200) }
  }
  if (!res.ok) {
    console.error(`✗ HTTP ${res.status} ${body.error ?? ''}：${body.message ?? text.slice(0, 200)}`)
    process.exit(1)
  }
  return body
}

const fmtBytes = (n) => (n >= 1048576 ? `${(n / 1048576).toFixed(1)}MB` : `${Math.round(n / 1024)}KB`)

async function listDev() {
  const { items } = await api('/api/v1/admin/dev-applications')
  if (!items.length) return console.log('（没有开发者申请）')
  console.log('开发者申请：')
  for (const a of items) {
    console.log(`  #${a.id}  ${a.username}  [${a.status}]  user_status=${a.developerStatus}`)
    if (a.reason) console.log(`      理由：${String(a.reason).slice(0, 100)}`)
    if (a.reviewNote) console.log(`      审核备注：${a.reviewNote}`)
  }
}

async function listSubs() {
  const { items } = await api('/api/v1/admin/submissions')
  if (!items.length) return console.log('（没有扩展提交）')
  console.log('扩展提交：')
  for (const s of items) {
    console.log(
      `  #${s.id}  ${s.extId} v${s.version}  [${s.status}]  by ${s.username}  ` +
        `${fmtBytes(s.pkgSize ?? 0)}  包体${s.hasPackage ? '在' : '缺失 ⚠ 发不出去'}`,
    )
    if (s.reviewNote) console.log(`      备注：${s.reviewNote}`)
  }
}

const [, , cmd, ...rest] = process.argv
const flag = (n, d = '') => {
  const i = rest.indexOf(`--${n}`)
  return i > -1 ? (rest[i + 1] ?? d) : d
}
const pos = rest.filter((a, i) => !a.startsWith('--') && !(i > 0 && rest[i - 1].startsWith('--')))

switch (cmd) {
  case 'list':
  case undefined:
    await listDev()
    await listSubs()
    break
  case 'list-dev':
    await listDev()
    break
  case 'list-subs':
    await listSubs()
    break
  case 'approve-dev':
    await api(`/api/v1/admin/dev-applications/${pos[0]}/approve`, {
      method: 'POST',
      body: JSON.stringify({ note: flag('note') }),
    }).then((r) => console.log(`✓ 申请 #${r.id} 已通过（已发布扩展需再跑 npm run publish:approved）`))
    break
  case 'reject-dev':
    await api(`/api/v1/admin/dev-applications/${pos[0]}/reject`, {
      method: 'POST',
      body: JSON.stringify({ note: flag('note') }),
    }).then((r) => console.log(`✓ 申请 #${r.id} 已驳回`))
    break
  case 'approve-sub':
    await api(`/api/v1/admin/submissions/${pos[0]}/approve`, {
      method: 'POST',
      body: JSON.stringify({ note: flag('note') }),
    }).then((r) => console.log(`✓ 提交 #${r.id} 已通过。${r.next}`))
    break
  case 'reject-sub':
    await api(`/api/v1/admin/submissions/${pos[0]}/reject`, {
      method: 'POST',
      body: JSON.stringify({ note: flag('note') }),
    }).then((r) => console.log(`✓ 提交 #${r.id} 已驳回`))
    break
  case 'delete-sub':
    // ⚠️ 二次确认：删除**不可撤销**，且没有回收站。
    //   服务端只允许删已走完流程的（approved / rejected / withdrawn）——
    //   published 的必须先下架，否则市场清单会指向一个不存在的字节。
    if (!rest.includes('--yes')) {
      console.error(
        `这会**永久删除**提交 #${pos[0]}（不可撤销，没有回收站）。\n` +
          '确认无误请加 --yes 重跑。',
      )
      process.exit(2)
    }
    await api(`/api/v1/admin/submissions/${pos[0]}`, { method: 'DELETE' }).then((r) =>
      console.log(
        `✓ 提交 #${r.id} 已删除（原状态 ${r.deleted}）` +
          (r.blobFreed ? '，包体一并清理' : '，包体仍被其它提交引用故保留'),
      ),
    )
    break
  default:
    console.error(
      `未知命令：${cmd}\n` +
        '可用：list / list-dev / list-subs / approve-dev <id> / reject-dev <id> / approve-sub <id> / reject-sub <id>\n' +
        '      delete-sub <id> --yes（永久删除已走完流程的提交，需二次确认）\n' +
        '      可选 --note "理由"（驳回时必填）',
    )
    process.exit(2)
}