# m-hub-server · PocketBay 部署
#
# 协议明确支持 `dockerfile` 部署类型。用 Dockerfile 而非「node」类型的理由：
# **必须锁定 Node 版本**。生产靠 `--experimental-strip-types` 直接跑 .ts，
# 而该 flag 的可用性与默认行为随 Node 版本变化（22.6+ 需显式开启，
# 22.18+ 默认开启）。让平台自己挑版本 = 把「能否启动」交给运气。
FROM node:24-alpine

WORKDIR /app

# ⚠️ 顺序有讲究：package.json 单独先拷，让依赖层能被缓存。
# 改业务代码不会触发依赖重跑 —— 构建更快，且平台日志更干净。
#
# **刻意不跑 `npm ci` / `npm install`**：本服务零运行时依赖（全部 Node 内置
# 模块：node:http / node:sqlite / node:crypto / node:fs / node:url）。
# devDependencies 只有 typescript（本地类型检查用），生产不需要。
# 少一步 = 少一个构建失败点 —— 协议 §3.4 把「依赖安装失败」列为独立的
# 故障类别，零依赖就等于把这一整类故障从可能变成不可能。
COPY package.json ./

COPY tsconfig.json ./
COPY schema.sql ./
COPY scripts ./scripts
COPY src ./src
# 静态清单（registry / update + 各自 .sig）。**已签名的字节就是线上字节** ——
# 任何后续修改都会让验签失败，故这里原样拷贝，不在镜像里重新生成。
COPY public ./public

# 协议 §3.4 第 1 类「运行约定」：必须读 PORT、绑 0.0.0.0。
# node.ts 里已经这么做了，这里只把它显式写出来，避免被误改。
ENV PORT=8080
ENV NODE_ENV=production
# 平台会为动态应用挂载 /data 并注入 POCKETBAY_DATA_DIR（协议 §一）。
# 不设时 node.ts 回落到 ./data（本地开发用）。
ENV POCKETBAY_DATA_DIR=/data

EXPOSE 8080

# node:sqlite 在 Node 22/23 带 ExperimentalWarning，生产日志不该被它淹没。
# 用 --no-warnings 精确屏蔽这一条（而不是 --no-warnings 全关，那会藏掉真警告）。
CMD ["node", "--experimental-strip-types", "--no-warnings=ExperimentalWarning", "src/node.ts"]
