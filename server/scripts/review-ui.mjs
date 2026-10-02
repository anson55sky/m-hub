#!/usr/bin/env node
// 本地审核界面：起一个静态服务 + 代理管理端请求。
//
// ## 为什么页面不直接打线上、而是走本地代理
//
// 因为要**浏览器→线上**就得把 token 交给浏览器，那它会进
// localStorage / 历史记录 / 截图 —— 而这个 token 等于管理员权限。
// 故：本服务读 `~/.m-hub-signing/admin_token` 代发，浏览器只看到
// 已经脱敏过的 JSON。
//
// ## 安全边界（三条，都必须成立）
//
// ① **只监听 127.0.0.1** —— 不对外。局域网里别人能打你的管理端不是好事。
// ② **不是开放代理**：只代理 `/api/v1/admin/*` 这一个前缀白名单。
//    否则任何网页都能让浏览器去打 `http://127.0.0.1:8788/api/…`，
//    借你的 token 审核东西（CSRF / DNS rebinding 那类攻击）。
// ③ **Origin 检查**：只接受来自本页面的请求。
//
// ## 用法
//
//   npm run review:ui          # 起服务并提示访问地址
//
// 停止：Ctrl-C。
import { createServer } from 'node:http'
import { readFileSync, existsSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'

const HERE = dirname(fileURLToPath(import.meta.url))
const PORT = Number(process.env.PORT || 8788)
const BASE = (process.env.MHUB_SERVER_URL || 'https://m-hub-server.pages.dev').replace(/\/+$/, '')
const SECRET_FILE = join(process.env.HOME || '', '.m-hub-signing/admin_token')

if (!existsSync(SECRET_FILE)) {
  console.error(`找不到 ${SECRET_FILE}\n  先跑一次 npm run review 生成 token，或手动写入。`)
  process.exit(2)
}
const TOKEN = readFileSync(SECRET_FILE, 'utf8').trim()

/** 只代理这一个前缀 —— 见文件头 ② */
const ALLOW = /^\/api\/v1\/admin\//

const server = createServer(async (req, res) => {
  const send = (code, body, type = 'application/json') => {
    res.writeHead(code, { 'content-type': type, 'cache-control': 'no-store' })
    res.end(body)
  }

  // ③ Origin 检查：只接受本页面的请求（无 Origin 的（同源导航）也放行）
  const origin = req.headers.origin
  if (origin && origin !== `http://127.0.0.1:${PORT}` && origin !== `http://localhost:${PORT}`) {
    return send(403, JSON.stringify({ error: 'forbidden', message: `来源不被接受：${origin}` }))
  }

  // 静态页面
  if (req.url === '/' || req.url === '/index.html') {
    return send(200, readFileSync(join(HERE, 'review-ui.html')), 'text/html; charset=utf-8')
  }

  if (!ALLOW.test(req.url || '')) {
    return send(404, JSON.stringify({ error: 'not_found', message: '只代理 /api/v1/admin/*' }))
  }

  // 收集请求体（审核接口只有小 JSON）
  let body = ''
  if (req.method === 'POST') {
    for await (const c of req) {
      body += c
      if (body.length > 64 * 1024) return send(413, '{"error":"too_large","message":"请求体过大"}')
    }
  }

  try {
    const up = await fetch(`${BASE}${req.url}`, {
      method: req.method,
      headers: {
        authorization: `Bearer ${TOKEN}`,
        ...(body ? { 'content-type': 'application/json' } : {}),
      },
      body: body || undefined,
    })
    const buf = Buffer.from(await up.arrayBuffer())
    const type = up.headers.get('content-type') || 'application/json'
    res.writeHead(up.status, { 'content-type': type, 'cache-control': 'no-store' })
    res.end(buf)
  } catch (e) {
    send(502, JSON.stringify({ error: 'upstream', message: `打线上管理端失败：${e}` }))
  }
})

// 只监听回环，不对外。0.0.0.0 会让同网段的人能借你的 token 审核。
server.listen(PORT, '127.0.0.1', () => {
  console.log(`
m-hub 审核台已启动

  http://127.0.0.1:${PORT}

· 只监听本机，局域网访问不到
· 页面不持有 token（由本服务代发）
· 只代理 /api/v1/admin/*

停止：Ctrl-C
`)
})