# m-hub-server

m-hub 的平台服务端。**客户端 → 服务端**的路径契约真相源在
`../src-tauri/src/api_spec.rs`（约定 47），本目录是对应的 Worker 实现 +
一份**构建期强制对账**的副本。

跑在 **Cloudflare Workers + D1**，静态托管两份签名清单。
**不需要买域名、不需要服务器、免费额度内 ¥0。**

---

## 一、子域名怎么拿

> **一步步的操作清单在 [`ONBOARD.md`](./ONBOARD.md)** —— 精确到点哪个链接、
> 填什么、期望看到什么输出、以及出错了怎么办。
> 随时不确定走到哪了，跑 `npm run status`（9 项里完成几项、下一步做什么）。

### 你要的那个地址长这样

```
https://m-hub-server.<你的 Cloudflare 子域>.workers.dev
```

`workers.dev` 是 Cloudflare **免费**分配给每个账号的公网子域，
`<name>.<子域>.workers.dev` 自动可用，带 HTTPS 证书、全球可达、**不用买域名**。

客户端里烧的是编译期常量 `src-tauri/src/config.rs`：

```rust
pub const DEFAULT_SERVER_URL: &str = "https://m-hub.xfactor.top";
```

所以部署完拿到实际地址后，把这行换成你的 `workers.dev` 地址、
**重新构建客户端**即可（现在没用户，重发零代价；以后有用户就贵了 ——
轮换一次地址 = 每个用户手动更新一次）。

### 拿到的四步

```bash
# 1. 注册 / 登录 Cloudflare（免费账号即可）
open https://dash.cloudflare.com/sign-up

# 2. 装 wrangler 并登录（一次）
npm install -g wrangler
wrangler login          # 浏览器里点「Allow」，会写 ~/.wrangler/config

# 3. 建库，拿到 database_id
cd server
npm install
npx wrangler d1 create m-hub
#    → 输出里有一行 database_id = "xxxxxxxx-…"
#    把它填进 wrangler.jsonc 的 d1_databases[0].database_id

# 4. 初始化表结构
npx wrangler d1 execute m-hub --file=./schema.sql --remote

# 5. 配密钥（都是 secret，不进仓库）
npx wrangler secret put GITHUB_CLIENT_ID     # GitHub OAuth App 的 Client ID
npx wrangler secret put INVITE_CODE          # 兑换码，你自己定
npx wrangler secret put OPENAI_API_KEY       # 平台 AI 的上游 Key（没有就别开这个功能）
# 可选：npx wrangler secret put RESEND_API_KEY   # 邮箱登录；不配就明确报「未启用」

# 6. 部署。部署完地址就是
#    https://m-hub-server.<你的子域>.workers.dev
npm run deploy
```

验证：

```bash
curl -i https://m-hub-server.<你的子域>.workers.dev/api/v1/market/registry
curl -i https://m-hub-server.<你的子域>.workers.dev/api/v1/market/registry.sig
# 两者都应是 200
```

### GitHub OAuth App 怎么建（拿 GITHUB_CLIENT_ID）

设备码流程**不需要** `client_secret`，只要 Client ID：

1. GitHub → Settings → Developer settings → OAuth Apps → New OAuth App
2. Application name 随便填；Homepage URL 随便填（设备码流程用不上）
3. **不要**勾任何回调 URL
4. 创建后页面顶部就是 Client ID

> 客户端已内嵌一个可用的 Client ID（`Ov23liN7tC1bDfc7Wb0M`，本机直连 GitHub 用，
> 见 `github_auth.rs`）。**服务端登录是另一码事**：它拿到的是「本账号」，
> 能兑换额度、能申请开发者、能发布扩展 —— 必须用你自己注册的 App，
> 否则你的 OAuth App 授权出来的用户，客户端本地那把钥匙解不开。

### 想要自己的域名？

Workers 可以挂自定义域名（需要你在 Cloudflare 托管该域名）。
**不必须** —— 想要 `m-hub.你的域名` 的话：

1. 把域名加进 Cloudflare（改 NS）
2. Worker → Settings → Domains & Routes → Add → Custom domain
3. 改 `config.rs` 的常量，重新构建客户端

**建议先别买。** `workers.dev` 完全够用，等到真需要「m-hub 品牌一致性」时再买不迟。

---

## 二、架构：静态 vs 动态

| | 路径 | 走哪 | 为什么 |
|---|---|---|---|
| **静态** | `/api/v1/market/registry` + `.sig` | 边缘静态资源，**不过 Worker** | 清单验的是**原始字节**。中间任何一次重新序列化（gzip 重压缩 / JSON 美化 / 多一个尾随换行）都会让验签失败。静态路径保证「发布时签的字节」与「客户端拿到的字节」逐字节一致 |
| **静态** | `/api/v1/app/update` + `.sig` | 同上 | 同上 |
| **动态** | 登录 / 账号 / 开发者 / 发布 | Worker | 需要鉴权、写库 |
| **OpenAI 兼容** | `POST /v1/chat/completions` | Worker（流式透传） | 约定 33：平台**只**实现这个端点，**没有** `GET /v1/models` |

`wrangler.jsonc` 里**故意不设** `assets.binding`：不设 binding 时静态资源命中就不进
Worker；设了反而要自己 `env.ASSETS.fetch()`，多一层出错机会。

### 包本体不自建

清单里的 `downloadUrl` 指向 **GitHub Releases**：

```
https://github.com/<你的仓库>/releases/download/<id>-v<版本>/<id>-<版本>.xhpack
```

客户端只管按 URL 下载，所以整块文件托管、带宽、下载目录遍历防护全都不需要。
`seed-manifests.mjs` 生成的就是这种 URL。

---

## 三、发布一次扩展的完整流程

```bash
# 1. 把扩展源码放进 server/manifests/extensions/<id>/
cp -R ~/Documents/m-hub-extensions/calculator server/manifests/extensions/local.calculator

# 2. 生成并签名两份清单（私钥读 ~/.m-hub-signing/market_private.pem）
cd server && npm run seed:manifests

# 3. 把 .xhpack 传到 GitHub Releases（清单里的 downloadUrl 指向它）
#    本地包在 server/manifests/<id>-<版本>.xhpack

# 4. 部署
npm run deploy
```

`seed-manifests.mjs` 每次都重新打包 + 重新签名，所以「改了源码忘了重签」
这个坑被脚本堵死了 —— 改了不重跑，线上那份就是旧的。

### 私钥

`bash ../scripts/market-keygen.sh` 生成，落 `~/.m-hub-signing/`（仓库外、700/600）。

- 私钥**绝不**进仓库、绝不进二进制、绝不经我（agent）的输出
- **轮换公钥的代价**：每个用户手动更新一次。现在装机量 0，所以**这次轮换零代价** ——
  这是唯一一次免费的机会（`signing.rs` 的 `TEST_SIGNATURE` 必须一起换）
- 泄露 = 任何人都能伪造市场清单和更新包，用户会被投毒

---

## 四、契约对账（约定 47）

Rust 的 `api_spec.rs` 与这里的路径表是**两份拷贝** —— 跨语言没法不拷。
这层的失败模式极其恶劣：两端测试全绿，**只有真联调才 404**，
且三种病因（路径写错 / 少段前缀 / 注册了没挂）一个症状。

所以不靠「抄的时候小心」，而是让漂移**在构建期就响**：

```bash
node ../scripts/check-api-spec-conformance.mjs   # 或 cd server && npm run check:spec
```

它守五件事：

1. **路径集合一致** —— 从 `api_spec.rs` 机械抽出，与 `paths.ts` 双向比对
2. **解析器自检** —— 12 条必须抽到的路径。**没有这条，解析器坏掉时守卫会
   因为「抽得少」而全绿**：15 条路由里 7 条是 `{}` 拼接形式，漏了可选前缀就全抽不出来，
   而「客户端 ⊆ 服务端」对缺失项一律通过（实测踩过）
3. **`MOUNTED` == `DYNAMIC`** —— 防「加了表忘了挂」
4. **`index.ts` 的 `r.add` 逐条对上（方法 + 路径）** —— 防「表改了、代码没改」；
   方法错会 405，症状与 404 极像，所以**方法也钉**
5. **静态清单的签名真的验得过 `src-tauri/keys/market_public.key`** ——
   Node 内置 Ed25519 零依赖真跑一遍。这条抓的是最阴的一类：
   文件在、路径对、但签名过期（轮换过公钥没重签 / 签完又改了内容），
   症状是**客户端静默回退缓存**：市场空白、更新永远没有，而服务端 `curl` 一切正常

外加两条**具名语义断言**（源码推不出来的那部分）：

- `/me` 走**根路径**而非 `/api/v1/me` —— 约定 52 记了这个坑：
  按直觉「修正」成 `/api/v1/me` 会立刻 404
- `POST /v1/chat/completions` 必须存在 —— 约定 33；
  它由 `chat.rs` 拼 `{SERVER}/v1` + `/chat/completions` 得到，
  **不在 `api_spec.rs` 里**，所以机械对账抓不到它，只能具名声明

已用 4 个变异验证守卫真的会红：删路由 / 改方法 / 改内容不重签 / 破坏解析器。

---

## 五、已知取舍（骨架阶段）

| 取舍 | 影响 | 什么时候要改 |
|---|---|---|
| **`poll_id → device_code` 放 Worker 内存** | Worker 实例随时可能回收，回收后轮询必失败（回 410「请重新发起」）。free plan 回收更频繁 | 要降失败率就把映射落 D1 或 KV |
| **邮箱登录默认关闭** | Workers **没有出站 SMTP**（只给 HTTP/HTTPS），所以走 Resend 等 HTTP 邮件 API。未配 `RESEND_API_KEY` 时明确回 501「未启用，请用 GitHub 登录」 | 想开就配 secret；不想开就把 `AccountPanel.vue` 的邮箱分区摘掉 |
| **包本体走 GitHub Releases** | 依赖 GitHub 可用性与匿名下载额度 | 想自建就上 R2，清单里换 URL |
| **`platforms` 空的更新清单** | 客户端会明确报「清单没有 macos-aarch64 条目」。**这是刻意的** —— 换成本轮修掉的那种谎报「已是最新版本」更糟 | 真要发版就 `seed:manifests --dmg <路径>` |
| **无邮件找回 / 无密码** | 账号只绑 GitHub。GitHub 账号没了账号就没了 | 现阶段可接受 |
| **无速率限制**（除发信外） | 登录接口可被刷。签名机制保证伪造不出内容，但可被 DDoS | 上线后按需加 |
| **`ai_quota` 按 UTC 日切** | 与用户本地日期差一天 | 想对齐就在 `todayQuota` 里按用户时区算 |

## 六、免费额度

| | Free | 我们要多少 |
|---|---|---|
| Workers 请求 | 100,000 / 天 | 几十个用户、4h 查一次更新 → **每天几百次** |
| Workers CPU | 10 ms / 请求 | 本项目都是查库/转发，**远低于** |
| D1 存储 | 5 GB | 提交包不入库（D1 单值上限 1MB），**几十 MB 够用很久** |
| D1 每日行读写 | 有限额，**2026-09-01 起超额直接失败**（不再静默） | 会话/额度表按天清理就够 |

即：**几十个用户规模下，Workers + D1 是 ¥0**。真超了再说。

## 七、跑起来看看

```bash
npm run dev            # 本地 Worker（wrangler dev，含本地 D1）
npm run db:init:local  # 建本地表
npm run tail           # 看线上日志
npm run typecheck      # tsc --noEmit
npm test               # node --test
```
