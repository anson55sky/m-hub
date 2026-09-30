-- m-hub-server · PostgreSQL schema（由 schema.sql 自动转换而来）
--
-- 只放**必须落库**的东西。可静态托管的（市场清单 / 升级清单 + 各自 .sig）
-- 不进库 —— 它们是签过名的静态文件，放 CDN 边缘比过 Worker 更省、更快，
-- 而且能保证「客户端验的原始字节」与「发布时签的字节」逐字节一致。
--
-- 约定 47：客户端 `src-tauri/src/api_spec.rs` 是路径与请求体键名的唯一真相源。
-- 本文件只管**响应**形状，响应字段名由客户端的解析代码决定（见 server/README.md）。

-- PostgreSQL 版本：外键需在**每条连接**上启用（PG 的 foreign_keys 可按会话设置）。
-- 本项目靠应用层保证外键（写入方都是自己的代码），故不在此声明。

-- ---------------------------------------------------------------- 账号

CREATE TABLE IF NOT EXISTS users (
  id                SERIAL PRIMARY KEY,
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
  created_at        INTEGER NOT NULL DEFAULT ((EXTRACT(EPOCH FROM now())*1000)::bigint)
);

-- 会话。token 明文存：它本身就是随机不可猜的凭证，加密存储在这个威胁模型下
-- 换不来实质收益（拿到库就能拿到用户会话），却让「查自己的会话」变复杂。
-- 若将来要支持「撤销某台设备」（`/me/device-tokens`）就靠这张表。
CREATE TABLE IF NOT EXISTS sessions (
  token       TEXT PRIMARY KEY,
  user_id     INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  -- 设备名，供「我的在线设备」列表展示
  device      TEXT,
  created_at  INTEGER NOT NULL DEFAULT ((EXTRACT(EPOCH FROM now())*1000)::bigint),
  -- 过期时刻（毫秒）。`currentUser` 每次都带 `expires_at > now` 判定，
  -- 故过期是**查询时**生效，不需要定时任务去清理。
  expires_at  INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_sessions_user ON sessions(user_id);

-- ---------------------------------------------------------------- 开发者申请

-- 状态机：pending → approved | rejected
-- 约定 58：客户端「已提交未走完流程」的集合 = uploaded/pending_review/gate_failed，
-- 其中 pending_review 对应这里的 pending。同一扩展只允许一条未终结的申请，
-- 由下面的部分唯一索引在**数据库层**保证，而不是靠应用层查一遍再插
-- （并发下会漏，而客户端已经按「有阻塞就拦住提交」处理，见约定 58）。
CREATE TABLE IF NOT EXISTS dev_applications (
  id          SERIAL PRIMARY KEY,
  user_id     INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  reason      TEXT NOT NULL,
  status      TEXT NOT NULL DEFAULT 'pending',
  review_note TEXT,
  created_at  INTEGER NOT NULL DEFAULT ((EXTRACT(EPOCH FROM now())*1000)::bigint),
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
  id           SERIAL PRIMARY KEY,
  user_id      INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  ext_id       TEXT NOT NULL,
  version      TEXT NOT NULL,
  status       TEXT NOT NULL DEFAULT 'pending_review',
  -- 审核备注；gate 失败原因也放这里（客户端 `.get("review_note")`）
  review_note  TEXT,
  created_at   INTEGER NOT NULL DEFAULT ((EXTRACT(EPOCH FROM now())*1000)::bigint),
  updated_at   INTEGER NOT NULL DEFAULT ((EXTRACT(EPOCH FROM now())*1000)::bigint)
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
  id            SERIAL PRIMARY KEY,
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
CREATE TABLE IF NOT EXISTS ai_quota (
  user_id     INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  day         TEXT    NOT NULL,           -- 'YYYY-MM-DD'（UTC）
  granted     INTEGER NOT NULL DEFAULT 0,
  used        INTEGER NOT NULL DEFAULT 0,
  PRIMARY KEY (user_id, day)
);
