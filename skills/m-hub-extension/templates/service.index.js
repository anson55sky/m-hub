// service/index.js —— 零依赖 Node 后端，监听宿主注入的 PORT
// 落地时把 TODO 换成真实逻辑；前端一律用 window.mhub.service.request('/api/...') 访问，
// 不要直接 fetch 端口（宿主的 /svc/<extId>/* 反向代理统一解决 CORS）。
const http = require('http')

const port = parseInt(process.env.PORT || '0', 10)
const HOST = '127.0.0.1' // 铁律：只监听本机回环

const server = http.createServer((req, res) => {
  res.setHeader('Content-Type', 'application/json; charset=utf-8')

  // 健康检查路径需与 manifest.backend.health 一致
  if (req.url === '/healthz') {
    res.end(JSON.stringify({ ok: true }))
    return
  }

  if (req.url === '/api/hello') {
    res.end(JSON.stringify({ message: 'hi', extId: process.env.XHUB_EXT_ID }))
    return
  }

  // TODO: 在这里挂真实路由（读取 body 时注意分段拼接，不要假设一次到齐）
  res.statusCode = 404
  res.end(JSON.stringify({ error: 'NOT_FOUND', path: req.url }))
})

server.listen(port, HOST, () => {
  // 端口由宿主分配（manifest.backend.port = 0），实际端口从 server.address() 取
  console.log(`[service] listening on http://${HOST}:${server.address().port}`)
})

// 宿主停止扩展时发 SIGTERM，优雅收尾
process.on('SIGTERM', () => server.close(() => process.exit(0)))
