-- m-hub-server · D1 schema
--
-- 只放**必须落库**的东西。可静态托管的（市场清单 / 升级清单 + 各自 .sig）
-- 不进库 —— 它们是签过名的静态文件，放 CDN 边缘比过 Worker 更省、更快，
-- 而且能保证「客户端验的原始字节」与「发布时签的字节」逐字节一致。
--
-- 约定 47：客户端 `src-tauri/src/api_spec.rs` 是路径与请求体键名的唯一真相源。
-- 本文件只管**响应**形状，响应字段名由客户端的解析代码决定（见 server/README.md）。

PRAGMA foreign_keys = ON;

-- ---------------------------------------------------------------- 账号

CREATE TABLE IF NOT EXISTS users (
  id                INTEGER PRIMARY KEY AUTOINCREMENT,
  -- GitHub 数字 id。登录只走 GitHub，故此列是登录身份；邮箱登录（可选）另存。
  github_id         TEXT UNIQUE,
  username          TEXT NOT NULL,
  email             TEXT UNIQUE,
  avatar            TEXT,
  -- role: user | admin
  role              TEXT NOT NULL DEFAULT 'user',
  -- 开发者身份。三个字段刻意冗余在 users 上而不另开表：客户端 `/me` 一次就要
  -- 全读，多一次 JOIN 就多一个 404/字段名漂移的现场（约定 47 的教训）。
  is_developer      INTEGER NOT NULL DEFAULT 0,
  -- developer_status: none | pending | approved | rejected
  developer_status  TEXT    NOT NULL DEFAULT 'none',
  -- 邀请码兑换 = 发额度 + 开放开发者申请。客户端 `/me` 直接读这个布尔。
  invite_redeemed   INTEGER NOT NULL DEFAULT 0,
  created_at        INTEGER NOT NULL DEFAULT (unixepoch() * 1000)
);

-- 会话。token 明文存：它本身就是随机不可猜的凭证，加密存储在这个威胁模型下
-- 换不来实质收益（拿到库就能拿到用户会话），却让「查自己的会话」变复杂。
-- 若将来要支持「撤销某台设备」（`/me/device-tokens`）就靠这张表。
CREATE TABLE IF NOT EXISTS sessions (
  token       TEXT PRIMARY KEY,
  user_id     INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  -- 设备名，供「我的在线设备」列表展示
  device      TEXT,
  created_at  INTEGER NOT NULL DEFAULT (unixepoch() * 1000),
  -- 过期时刻（毫秒）。`currentUser` 每次都带 `expires_at > now` 判定，
  -- 故过期是**查询时**生效，不需要定时任务去清理。
  expires_at  INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_sessions_user ON sessions(user_id);

-- 扩展包体（分块存储，内容寻址）。
--
-- ## 为什么分块而不直接存一列 BLOB
--
-- 我想实测 D1 的单值上限，但**测不出来**：`wrangler d1 execute` 把 SQL 当命令行
-- 参数传，实测 1KB 能写、64KB 就 `code: 7500` —— 那是**CLI 的参数长度限制**，
-- 不是 D1 的存储上限。真正的写入走 Function 里的 `env.DB` 绑定，不经命令行，
-- 所以这个上限无法用 CLI 探测。
--
-- 既然不能确认，就不依赖它：128KB 一块，任何可能的上限都够。
-- 真要用 R2 时，`pkgStore.ts` 内部换实现即可，调用方无感。
--
-- ## 为什么按 sha256 内容寻址
--
-- 同一份包重复提交（关卡挂了重提、或两个作者提交了同一个包）只存一份；
-- 且「审核通过的字节」与「发布的字节」靠同一个 sha256 对账，不会错位。
--
-- ⚠️ 删除策略：内容寻址意味着**不能按包删**（可能被多个 submission 引用）。
--    故只在整库清理时按引用计数回收，不做单包删除。
CREATE TABLE IF NOT EXISTS pkg_blobs (
  sha256  TEXT    NOT NULL,
  idx     INTEGER NOT NULL,          -- 块序号，从 0 起
  data    BLOB    NOT NULL,
  PRIMARY KEY (sha256, idx)
);

-- GitHub 设备码流程的中间态：poll_id → GitHub device_code。
--
-- ⚠️ **这张表是 Pages Functions 部署的必要条件**，不是可选优化（2026-10-01 实机）。
-- 原实现用进程内存的 `Map` 存这层状态，注释写「单次登录窗口只有几分钟，
-- 回收概率低到可接受」—— 那是按**常驻进程**算的。Pages Functions 的实例是
-- 短命的、多个 isolate 互不共享、scale-to-zero 后必然冷启动，于是
-- 「换验证码」与「轮询」两次请求大概率落在**不同实例**，轮询必然
-- `poll_gone`（HTTP 410）。
--
-- 实测证据：连发三次 device/start 全部 200 且每次都是新的 user_code，
-- 说明没有任何状态被复用。
--
-- 过期同样是**查询时**判定（`expires_at > now`），不需要定时清理；
-- 真正的删除发生在 poll 成功时（一次性消费）。
CREATE TABLE IF NOT EXISTS device_polls (
  poll_id      TEXT PRIMARY KEY,
  -- GitHub 返回的 device_code。它是**换 token 的凭据**，等价于一次性密码，
  -- 故与 sessions.token 同等对待：明文存 D1、绝不进日志、不进错误信息。
  --
  -- ⚠️ 这里**没有** user_id：device/start 时用户还没授权，压根没有 user 行
  --   （users 是在换到 access_token 之后才建的）。加一个 NOT NULL 外键会直接
  --   让整个登录流程插不进去 —— 我第一版就是这么写的。
  device_code  TEXT NOT NULL,
  -- 当前建议的轮询间隔（秒）。GitHub 回 slow_down 时要**累加**它，
  -- 这个累加是**有状态**的 —— 不落库就等于跨实例丢失退避，
  -- 客户端会以固定频率猛点 GitHub（也正是 GitHub 回 slow_down 的原因）。
  interval     INTEGER NOT NULL DEFAULT 5,
  created_at   INTEGER NOT NULL DEFAULT (unixepoch() * 1000),
  expires_at   INTEGER NOT NULL
);
-- 过期判定是查询时的 `expires_at > now`，不需要定时任务；
-- 真正的删除发生在 poll 成功时（一次性消费）。
CREATE INDEX IF NOT EXISTS idx_device_polls_expiry ON device_polls(expires_at);

-- ---------------------------------------------------------------- 开发者申请

-- 状态机：pending → approved | rejected
-- 约定 58：客户端「已提交未走完流程」的集合 = uploaded/pending_review/gate_failed，
-- 其中 pending_review 对应这里的 pending。同一扩展只允许一条未终结的申请，
-- 由下面的部分唯一索引在**数据库层**保证，而不是靠应用层查一遍再插
-- （并发下会漏，而客户端已经按「有阻塞就拦住提交」处理，见约定 58）。
CREATE TABLE IF NOT EXISTS dev_applications (
  id          INTEGER PRIMARY KEY AUTOINCREMENT,
  user_id     INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  reason      TEXT NOT NULL,
  status      TEXT NOT NULL DEFAULT 'pending',
  review_note TEXT,
  created_at  INTEGER NOT NULL DEFAULT (unixepoch() * 1000),
  reviewed_at INTEGER
);
-- 同一用户只允许一条未终结的申请：status 属于 ('pending') 时唯一。
-- SQLite 部分唯一索引：WHERE 子句限定生效范围，终结后即可再申请。
CREATE UNIQUE INDEX IF NOT EXISTS idx_dev_app_one_pending
  ON dev_applications(user_id) WHERE status = 'pending';
CREATE INDEX IF NOT EXISTS idx_dev_app_user ON dev_applications(user_id, created_at DESC);

-- ---------------------------------------------------------------- 发布

-- 状态机：
--   uploaded → pending_review → published
--                    ↓              ↓
--              gate_failed      （下架不改本表，见约定 60）
--   上述任一未终结状态均可 withdraw → withdrawn
--
-- ⚠️ 约定 60：平台下架只写市场清单的 `revoked`，**绝不 UPDATE 本表**。
-- 否则客户端从提交记录里看不出「已下架」，会一直显示「已上架」。
-- 「已下架」是客户端拿清单 `revoked` 反查出来的派生状态。
CREATE TABLE IF NOT EXISTS submissions (
  id           INTEGER PRIMARY KEY AUTOINCREMENT,
  user_id      INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  ext_id       TEXT NOT NULL,
  version      TEXT NOT NULL,
  status       TEXT NOT NULL DEFAULT 'pending_review',
  -- 审核备注；gate 失败原因也放这里（客户端 `.get("review_note")`）
  review_note  TEXT,
  -- 包体在 `pkg_blobs` 里的键（内容寻址，见该表注释）与总大小。
  -- ⚠️ 这两列是**能上架的前提**：没有包体，审核通过了也发布不出去
  --    （清单里的 downloadUrl 指向一个不存在的字节）。2026-10-01 补。
  pkg_sha256   TEXT,
  pkg_size     INTEGER,
  -- 包内文件清单（JSON: [{path,size}]）。关卡要靠它判断「有没有禁的扩展名 /
  -- 是否漏了 manifest.json」，审核页要靠它展示「这个包会往用户机器上放什么」。
  pkg_files    TEXT,
  created_at   INTEGER NOT NULL DEFAULT (unixepoch() * 1000),
  updated_at   INTEGER NOT NULL DEFAULT (unixepoch() * 1000)
);
-- 同一扩展只允许一条未走完流程的提交（约定 58 的服务端侧兜底）。
-- 未终结集合 = uploaded / pending_review / gate_failed —— 必须与客户端的
-- 可撤回白名单一致，否则会出现「客户端拦住提交、服务端却允许」的错位。
CREATE UNIQUE INDEX IF NOT EXISTS idx_sub_one_open
  ON submissions(user_id, ext_id)
  WHERE status IN ('uploaded', 'pending_review', 'gate_failed');
-- 提交记录按账号全量列出（约定 61：服务端**不支持**按 ext_id 过滤，
-- 客户端因此不做过滤而是给非本扩展的行加标签区分）
CREATE INDEX IF NOT EXISTS idx_sub_user ON submissions(user_id, created_at DESC);

-- 上传的扩展包。放 D1 而不是 R2 的理由：包体小（约定 46 打的 .xhpack
-- 排除 node_modules）、且只在审核期存在。**D1 单值上限 1MB**，
-- 故这里存的是「发布元数据」，包本体走 multipart 直传（见 routes/submissions.ts）。
CREATE TABLE IF NOT EXISTS submission_assets (
  id            INTEGER PRIMARY KEY AUTOINCREMENT,
  submission_id INTEGER NOT NULL REFERENCES submissions(id) ON DELETE CASCADE,
  filename      TEXT NOT NULL,
  size          INTEGER NOT NULL,
  sha256        TEXT NOT NULL,
  -- 包与截图的实际存放位置（GitHub Releases / R2 URL），审核通过后才公开
  storage_key   TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_asset_sub ON submission_assets(submission_id);

-- ---------------------------------------------------------------- 平台 AI 额度

-- 额度按天发放（约定 53：平台额度是「一个开关 + 一个对话入口」，
-- 客户端只拉模型清单，额度在服务端核销）。此处只记发放与消耗流水。
-- 市场清单的**已签名字节**（2026-10-02 加：把上架从构建期搬进运行期）。
--
-- 为什么要存字节而不是每次请求现签：客户端取的是**两个独立请求**
-- （`/registry` 与 `/registry.sig`）然后验签。现签的话两次请求可能落在
-- 不同的 D1 快照 / 不同的边缘节点，中间只要有一次重建，两边字节就对不上
-- → 客户端报「验签失败」。存成**单行**，两个端点读同一个值，竞态从根上不存在。
CREATE TABLE IF NOT EXISTS market_registry (
  id         INTEGER PRIMARY KEY CHECK (id = 1),  -- 恒为一行；CHECK 防误插第二行
  bytes      TEXT    NOT NULL,                  -- 签名前的清单 JSON（**签名覆盖的就是这份字节**）
  sig        TEXT    NOT NULL,                  -- base64 的 Ed25519 签名
  updated_at INTEGER NOT NULL
);

-- submissions 的市场元数据（原本只有构建期解包时能读到）。
--
-- 清单里的 name/description/homepage/permissions 来自**包内 manifest.json**，
-- 而包在 `pkg_blobs` 里是分块存的，读取要解 zip。运行期上架时我们手里已经有
-- 那个包（审核通过那一步就要跑关卡），顺手把 manifest 存下来，
-- 重建清单时就变成一次纯数据库读 —— 不必每次上架都解一遍 zip。
--
-- ⚠️ 只在「上架」那一刻写。approved 的提交**没有**这一列内容，
--   rebuild 会跳过它们（`status='published'` 才进清单）。
ALTER TABLE submissions ADD COLUMN manifest_json TEXT;

CREATE TABLE IF NOT EXISTS ai_quota (
  user_id     INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  day         TEXT    NOT NULL,           -- 'YYYY-MM-DD'（UTC）
  granted     INTEGER NOT NULL DEFAULT 0,
  used        INTEGER NOT NULL DEFAULT 0,
  PRIMARY KEY (user_id, day)
);
