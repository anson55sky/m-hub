# m-hub-server 上站 · 逐步操作清单

从零到「客户端能连上」。**全程约 15 分钟，全部免费，不用买域名，不用绑卡。**

任何时候不确定走到哪了，跑：

```bash
cd /Users/sky/Desktop/项目/CODE/m-hub/server
npm run status
```

它会告诉你 9 项里已完成几项、下一个该做什么。

---

## 第 0 步 · 知道最后要什么

部署成功后你会拿到一个这样的地址：

```
https://m-hub-server.<一串字符>.workers.dev
```

这个地址是 Cloudflare 免费给你的，**不用买域名**。后面第 8 步要把它填进客户端。

---

## 第 1 步 · 注册 Cloudflare（免费，不绑卡）

**网址**：https://dash.cloudflare.com/sign-up

1. 填邮箱，设密码
2. 验证邮箱
3. 如果问你选计划 —— 选 **Free**（免费版）
4. **不要**填信用卡。Workers 免费版明确「no credit card required」

> 免费额度（官方）：
> - Workers **10 万次请求/天**
> - 每次请求 **10ms CPU**
> - **超出直接停**（hard stop，不额外扣费）
> - D1 **5GB 存储**
>
> 你现在装机量 0，每 4 小时查一次更新 → 一天几百次请求，离 10 万差三个数量级。
> **这个方案在你「几十个人用」的规模内一直是 ¥0。**

---

## 第 2 步 · 拿 GitHub 的 Client ID

**为什么需要**：服务端要用它代表用户登录（拿你的账号的开发者身份）。
客户端里已有的那个 Client ID 是**本机直连 GitHub** 用的，解不开服务端签发的会话。

**网址**：https://github.com/settings/developers

点击路径：

```
Settings（左上头像 → Settings）
  → Developer settings（左栏最下）
    → OAuth Apps
      → New OAuth App
```

**表单怎么填**：

| 字段 | 填什么 |
|---|---|
| Application name | `m-hub-server` |
| Homepage URL | `https://workers.dev` |
| Authorization callback URL | `https://workers.dev` |

> **callback URL 会不会有安全问题？** 不会。GitHub 从 2019 年起已移除 OAuth 的
> `state` 强制要求，而**设备码流程根本不用回调** —— 用户在浏览器里授权后，
> 你服务端是**轮询** GitHub 拿 token 的，不是 GitHub 回调你。所以这一栏
> 纯粹是表单必填，随便填个合法 URL 即可，它永远不会被用到。

点 **Register application**。

**复制**：页面最上面的 **Client ID**（一串 40 字符的 hex）。

> **Client secret 不用管。** 设备码流程不需要它，这也是为什么这套方案
> 不用 OAuth 回调、不用存 secret 的原因。

---

## 第 3 步 · wrangler 登录

```bash
cd /Users/sky/Desktop/项目/CODE/m-hub/server
npx wrangler login
```

浏览器会自动打开一个授权页 → 点 **Allow**。

**成功的样子**：
```
✔ You have logged in.
```

（凭证会落在 `~/.wrangler/config/default.toml`，不涉及密钥。）

自查：`npm run status` 的第 3 项应该变成 ✓。

---

## 第 4 步 · 建 D1 数据库

```bash
npx wrangler d1 create m-hub
```

**成功的样子**（输出里找 `database_id`）：
```
🌀 Creating D1 database with name "m-hub"...
🌀 Created D1 database with name "m-hub"!
   [[d1_databases]]
   binding = "DB"
   database_name = "m-hub"
   database_id = "xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx"
✔ Success
```

把那串 `database_id` 复制下来。

**填进 `wrangler.jsonc`**：用编辑器打开 `server/wrangler.jsonc`，找到第 57 行：

```jsonc
"database_id": "REPLACE_ME_run_wrangler_d1_create",
```

改成（引号保留，只换里面的字符串）：

```jsonc
"database_id": "你复制的那串",
```

> 这一步我**故意**留成明显的假 id，而不是空字符串 —— 空字符串会让部署报一个
> 含糊的错，假 id 报错时第一眼就告诉你是这一步没做。
> `npm run status` 的第 4 项变成 ✓ 就是对了。

---

## 第 5 步 · 建表

```bash
npx wrangler d1 execute m-hub --file=./schema.sql --remote
```

**成功的样子**：
```
🌀 Executing on remote database m-hub...
🌀 Executed 7 commands on m-hub
```

（`7` 是建表语句的条数。不同版本可能显示别的数字。）

---

## 第 6 步 · 配密钥

三个 `secret put`，每个都会提示你输入值（**输入时不回显，正常**）：

```bash
npx wrangler secret put GITHUB_CLIENT_ID
```
粘贴第 2 步的 Client ID，回车。

```bash
npx wrangler secret put INVITE_CODE
```
**你自己定一个兑换码**，比如 `MHUB-2026`。这串东西决定了谁能兑换额度、
谁能申请开发者 —— 相当于你的内测邀请码，别用太弱的。

> 换个更随机的更好（`openssl rand -base64 12` 可以生成一个）。

```bash
npx wrangler secret put OPENAI_API_KEY
```
**你现在没有这个就先跳过这行** —— 不配的话平台 AI 功能会明确回 401，
不影响其它任何功能。

> `secret` 与 `vars` 共用一个命名空间，**secret 优先**。
> 好处是值不回传、不进 `wrangler.jsonc`、不进仓库。

**成功的样子**：
```
🌀 Creating the secret for the Worker m-hub-server
✔ Success! Uploaded secret GITHUB_CLIENT_ID
```

---

## 第 6.5 步 · ⚠️ 注册 workers.dev 子域（**必须先做，否则第 7 步必失败**）

**这一步很容易被漏掉**，而且它的失败长得像别的问题。

`wrangler.jsonc` 里的 `workers_dev: true` 只是**声明「我要发布到 workers.dev」**，
它**不会替你创建那个子域**。子域是账号级的一次性设置，必须单独注册。

**不注册的症状**（我第一次就踩了）：

```
▲ [WARNING] You need to register a workers.dev subdomain before publishing to workers.dev
? Would you like to register a workers.dev subdomain now?
🤖 Using fallback value in non-interactive context: no      ← 自动答了「不」
✘ [ERROR] You can either deploy your worker to ... or register a workers.dev
           subdomain here: https://dash.cloudflare.com/<你的账号ID>/workers/onboarding
```

注意「Worker 已上传成功」但整条 deploy 仍失败 —— 所以别看到
`Uploaded m-hub-server` 就以为好了，**要看有没有那行 `Your worker has been deployed to …`**。

**怎么做**（`wrangler subdomain` 命令在 wrangler v3 已废弃，官方要求走网页）：

**网址**：https://dash.cloudflare.com/c4b2b5b67ecb5f4cee8cbb586a8f71c3/workers/onboarding

（上面那串是本账号的 ID，你的可能不同 —— 从上面报错信息里复制即可。
也可以从 https://dash.cloudflare.com 进去 → 左侧 **Workers & Pages**。）

点路径：

```
Workers & Pages
  → 点你的 Worker（m-hub-server，第 6 步 secret put 时已自动建好）
    → 找到 "Your subdomain" 这一栏
      → Change
        → 填一个名字（例如 sky-mhub）→ 确认
```

> 那个名字会成为**全局账号级**的子域，之后所有 Worker 都挂在它下面：
> `m-hub-server.sky-mhub.workers.dev`。取完就不能改（要改得重新验证所有权），
> 所以别取太随便的名字。

---

## 第 7 步 · 部署

```bash
npm run deploy
```

这个命令会**先跑契约对账守卫**（路径对不对、签名验不验得过），
通过了才真的部署。守卫报错就是有东西不对，别跳过。

**成功的样子**：
```
✓ 路径契约一致：客户端 16 条 ↔ 服务端 16 条…
🌀 Publishing to Cloudflare…
   Version ID: xxxxx
   Worker Startup Time: 30ms
✔ Uploaded m-hub-server
🌀 Building Worker...
   [conflicting versions] … 
Done in 12.34s
Your worker has been deployed to m-hub-server.<你的子域>.workers.dev
```

**把那行地址记下来**，第 8 步要用。

---

## 第 8 步 · 验证

### 8.1 静态清单（这两条通 = 市场与更新通了）

```bash
curl -i https://m-hub-server.<你的子域>.workers.dev/api/v1/market/registry
curl -i https://m-hub-server.<你的子域>.workers.dev/api/v1/market/registry.sig
```

**期望**：`HTTP/2 200` + 清单 JSON（里面有 1 个扩展 `local.calculator`）
和 88 字符的 base64 签名。

### 8.2 服务端活着（这条不通 = 路径错了）

```bash
curl -i https://m-hub-server.<你的子域>.workers.dev/me
```

**期望**：`HTTP/2 401` + `{"error":"unauthorized",...}`

> **必须是 401，不是 404。** 401 = 路由存在、只是没带 token（正确）；
> 404 = 域名或前缀写错了。这条是约定 52 记的自查法。

### 8.3 看日志（出问题时用）

```bash
npx wrangler tail
```

---

## 第 9 步 · 让客户端用上它

### 9.1 改地址常量

打开 `src-tauri/src/config.rs` 第 456 行：

```rust
pub const DEFAULT_SERVER_URL: &str = "https://m-hub.xfactor.top";
```

改成你的地址：

```rust
pub const DEFAULT_SERVER_URL: &str = "https://m-hub-server.<你的子域>.workers.dev";
```

> 这个地址是**编译期常量**且设置里**故意没有**地址入口（约定 52），
> 所以改完必须**重新构建客户端**才生效。
> 现在装机量 0，重发零代价 —— 这是唯一一次免费的机会。

### 9.2 重新构建

```bash
cd /Users/sky/Desktop/项目/CODE/m-hub
npm run tauri:build
```

产物在 `src-tauri/target/release/bundle/`。

### 9.3 确认改对了

```bash
npm run status     # 第 8 项应该变成 ✓
```

---

## 第 10 步（可选）· 发一个新版本到更新清单

想让客户端真能自动更新的话：

```bash
cd /Users/sky/Desktop/项目/CODE/m-hub/server
npm run seed:manifests -- --dmg "../src-tauri/target/release/bundle/dmg/m-hub_0.7.2_aarch64.dmg"
npm run deploy
```

> **签完的字节就是线上字节。** 之后**任何**对 `public/api/v1/app/update`
> 的修改（包括手动编辑、格式化）都会让验签失败 → 客户端静默回退缓存 →
> 表现为「更新永远没有」，而 `curl` 一切正常。
> 所以：改了就重跑 `seed:manifests`。守卫第 5 项会替你盯着这件事。

---

## 出问题了怎么办

| 现象 | 原因 | 怎么办 |
|---|---|---|
| `wrangler login` 打不开浏览器 | 环境问题 | 跑 `npx wrangler login --browser=false`，会给一个网址手动打开 |
| 部署报 `You need to register a workers.dev subdomain` | **第 6.5 步没做** | 走 https://dash.cloudflare.com/<账号ID>/workers/onboarding 网页注册（`wrangler subdomain` 命令已废弃） |
| 看到 `Uploaded m-hub-server` 但没有 `Your worker has been deployed to …` | 同上，子域未注册 | 别被「上传成功」骗了，deploy 整体是失败的 |
| `wrangler subdomain <名字>` 报 Deprecation | v3 已废弃该命令 | 改走网页（见第 6.5 步） |
| 部署报 `database_id` 无效 / `Invalid uuid` | 第 4 步没填 | 看 `npm run status` 第 4 项 |
| `curl` 清单 404 | 部署没成功，或路径拼错 | `npx wrangler tail` 看有没有请求到 |
| `curl /me` 返回 404 而非 401 | 路径前缀错了 | 确认地址里没有多一层路径 |
| 客户端显示市场空白 | 验签失败（客户端静默回退缓存） | 跑 `npm run status` 第 7 项；再对比客户端内嵌公钥与 `market_public.key` |
| 部署时守卫报错 | 契约漂移 | **别绕过**，看报错文字改对应的那一边 |
| `d1 execute` 报表已存在 | 第 5 步跑过两次 | 无害，schema 全是 `IF NOT EXISTS` |

## 部署静态资源到 Cloudflare Pages（v0.7.3 起的市场与更新清单托管处）

`workers.dev` 在国内被 DNS 污染，`pages.dev` 不被 —— 这是选它的唯一理由。
账号/发布类接口仍走平台服务端；市场清单、扩展包、安装包走 Pages。

```bash
# 建项目（--production-branch 必填；--force 只在**建项目这一次**需要）
npx wrangler pages project create m-hub-server --production-branch main --force

# 上传：先复制到干净暂存目录，确保隐藏文件 _headers 一并带上（拖拽上传最容易漏它）
STAGE=/tmp/mhub-pages-stage && rm -rf "$STAGE" && mkdir -p "$STAGE" && cp -R public/. "$STAGE"/
npx wrangler pages deploy "$STAGE" --project-name m-hub-server --branch main
```

三个坑：

1. **不带 `--force` 会失败**。wrangler 4.x 会把 Pages 委派给 Cloudflare Workers，
   去打 `/workers/services/<name>` 返回 `code: 10013`。
2. **不带 `--production-branch` 也会失败**：`Missing production branch`。
   我们是纯静态上传、没有 Git 仓库，这个分支名只是个标识，用 `main` 即可。
3. **`_headers` 是隐藏文件**，它承载 `Content-Encoding: identity` ——
   清单与签名必须逐字节一致，任何重新压缩都会让 Ed25519 验签失败。
   控制台拖拽上传容易漏掉它，故走命令行 + 干净暂存目录。

⚠️ **改了托管地址必须重跑 `seed-manifests` 并重新部署**：
清单里的 `downloadUrl` 是**签名前写死的字节**，忘了重签会出现
「清单验签通过（它确实被正确签过）但包指向另一个部署」的跨部署混用。
守卫 `check-manifest-urls.mjs` 的主机名断言会拦下这种不一致。

部署后必查（不能只看状态码，要**逐字节对比 + 真验签**）：

```bash
B=https://m-hub-server.pages.dev
for f in api/v1/market/registry api/v1/market/registry.sig api/v1/app/update api/v1/app/update.sig; do
  cmp <(curl -s $B/$f) public/$f && echo "✓ $f 逐字节一致"
done
```

⚠️ 刚部署完的头几分钟偶发 `522`（边缘传播中），连测 10 次即可区分
「偶发抖动」与「稳定故障」—— 单次 522 不要据此下结论。
客户端侧对此有 4 次退避重试兜底（清单与签名是两次独立请求，
一次成功一次失败拼在一起必然验签失败）。

⚠️ `fetch_bytes` 在**读 body 之前**就检查状态码（`updater.rs` / `market.rs` 各一份）：
这样 5xx 会被当成 `HTTP 5xx` 并触发重试，而**不会**被误当成签名内容
去报「签名非法」—— 后者会把一个网络故障说成一次安全事件。
