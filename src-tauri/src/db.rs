use rusqlite::{Connection, Result};
use std::path::Path;

pub fn init(path: &Path) -> Result<Connection> {
    let conn = Connection::open(path)?;
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    migrate(&conn)?;
    Ok(conn)
}

#[cfg(test)]
pub fn init_in_memory() -> Result<Connection> {
    let conn = Connection::open_in_memory()?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    migrate(&conn)?;
    Ok(conn)
}

fn table_exists(conn: &Connection, name: &str) -> bool {
    conn.query_row(
        "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?1",
        rusqlite::params![name],
        |row| row.get::<_, i64>(0),
    )
    .map(|n| n > 0)
    .unwrap_or(false)
}

fn migrate(conn: &Connection) -> Result<()> {
    // ⚠️ 先修历史遗留的半截状态（2026-09-29）。**必须放在下面那句
    // `CREATE TABLE IF NOT EXISTS resources` 之前**。
    //
    // 下面那段 resources 重建原先是 4 条**独立提交**的 DDL
    // （RENAME → CREATE → INSERT…SELECT → DROP）。若进程在 RENAME 之后、
    // DROP 之前被杀（崩溃 / 断电 / 磁盘满），库就停在「只剩 resources_old」的
    // 中间态。旧代码对此毫无察觉：`PRAGMA table_info(resources)` 查的是
    // **已不存在的表**，SQLite 对不存在的表返回**空集且不报错**，
    // 于是 `cols.iter().any(|c| c == "group_id")` 判假、重建被跳过，
    // 数据永远卡在 `resources_old` 里，而界面上速达列表全空、**没有任何报错**。
    //
    // 位置很关键：若放在那句 CREATE TABLE 之后，`resources` 必然已经存在，
    // 就只能走「把旧表内容并回新表」这条**有损**的路（要逐列对齐、可能因列不匹配而
    // 整条失败）。放在最前面，则「崩在 RENAME 之后」这种最危险的情形走的是
    // **无损的改名回填**，之后自然被下面的 group_id 检测接手、完成剩下的迁移。
    if table_exists(conn, "resources_old") {
        if table_exists(conn, "resources") {
            // 新表也在（崩在 CREATE 之后、DROP 之前）：把旧表内容并回来。
            // ⚠️ 丢弃旧表的 `DROP` **必须**只在并回成功之后才做。
            // 否则并回因列不匹配等原因失败时，旧表连同数据一起被删掉 ——
            // 那就从「能救」变成「彻底没了」。
            match conn.execute(
                "INSERT OR IGNORE INTO resources
                   (id, kind, name, target, category, icon, args, sort_order,
                    last_launched_at, created_at, updated_at)
                 SELECT id, kind, name, target, category, icon, args, sort_order,
                        last_launched_at, created_at, updated_at
                   FROM resources_old",
                [],
            ) {
                Ok(_) => {
                    let _ = conn.execute("DROP TABLE IF EXISTS resources_old", []);
                    log::warn!("已修复上次迁移遗留的 resources_old（速达数据已并回）");
                }
                Err(e) => log::error!(
                    "检测到上次迁移遗留的 resources_old，但并回失败：{e}。**已保留该表未删**，\
                     速达数据仍在其中，需人工处理（导出 resources_old 表内容）"
                ),
            }
        } else {
            // 新表根本不在（崩在 RENAME 之后、CREATE 之前）：直接改回原名，无损
            match conn.execute("ALTER TABLE resources_old RENAME TO resources", []) {
                Ok(_) => log::warn!("已从 resources_old 恢复 resources 表（速达数据已救回）"),
                Err(e) => log::error!("resources 缺失且 resources_old 存在，改名恢复失败: {e}"),
            }
        }
    }

    // 全新安装：直接建合一的 resources 表（kind 含 app/web/file，不再有分组）
    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS resources (
          id INTEGER PRIMARY KEY AUTOINCREMENT,
          kind TEXT NOT NULL CHECK (kind IN ('app', 'web', 'file')),
          name TEXT NOT NULL,
          target TEXT NOT NULL,
          category TEXT,
          icon TEXT,
          args TEXT,
          sort_order INTEGER NOT NULL DEFAULT 0,
          last_launched_at TEXT,
          created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%d %H:%M:%f','now')),
          updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%d %H:%M:%f','now'))
        );

        CREATE TABLE IF NOT EXISTS notes (
          id INTEGER PRIMARY KEY AUTOINCREMENT,
          title TEXT NOT NULL DEFAULT '',
          content TEXT NOT NULL DEFAULT '',
          created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%d %H:%M:%f','now')),
          updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%d %H:%M:%f','now'))
        );

        CREATE TABLE IF NOT EXISTS tags (
          id INTEGER PRIMARY KEY AUTOINCREMENT,
          name TEXT NOT NULL UNIQUE,
          created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%d %H:%M:%f','now'))
        );

        CREATE TABLE IF NOT EXISTS note_tags (
          note_id INTEGER NOT NULL REFERENCES notes(id) ON DELETE CASCADE,
          tag_id INTEGER NOT NULL REFERENCES tags(id) ON DELETE CASCADE,
          PRIMARY KEY (note_id, tag_id)
        );

        CREATE TABLE IF NOT EXISTS todos (
          id INTEGER PRIMARY KEY AUTOINCREMENT,
          title TEXT NOT NULL,
          done INTEGER NOT NULL DEFAULT 0,
          priority INTEGER NOT NULL DEFAULT 0,
          -- 截止 / 提醒时刻（毫秒时间戳，均可空）；remind_fired 防重复提醒
          due_at INTEGER,
          remind_at INTEGER,
          remind_fired INTEGER NOT NULL DEFAULT 0,
          -- 子待办父级（删除父条目时经外键级联删除子条目）
          parent_id INTEGER REFERENCES todos(id) ON DELETE CASCADE,
          -- 手动拖拽排序位（分组内升序；NULL = 未手动排序，组内按创建时间倒序）
          sort_order INTEGER,
          -- 乐观锁版本号（局域网同步冲突检测：每次写回 +1）
          version INTEGER NOT NULL DEFAULT 0,
          created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%d %H:%M:%f','now')),
          updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%d %H:%M:%f','now')),
          completed_at TEXT,
          -- 轻量 Markdown 正文（勾选一律用子待办，正文不放 checklist）
          description TEXT NOT NULL DEFAULT '',
          -- 置顶：脱离日期分组，固定排在列表最顶部「置顶」区
          pinned INTEGER NOT NULL DEFAULT 0,
          -- 周期规则总开关：once 一次性 / daily / weekly / monthly / yearly / weekdays / custom
          repeat_mode TEXT NOT NULL DEFAULT 'once',
          -- custom：每 N 个 repeat_unit
          repeat_every INTEGER,
          repeat_unit TEXT,
          -- 位掩码 bit0=周一 … bit6=周日（weekly 多选、custom+week 用）
          repeat_weekdays INTEGER,
          -- monthly：1..31，-1 = 月末
          repeat_month_day INTEGER,
          -- monthly：第几个（1..5，-1 = 最后一个），非空时与 repeat_weekdays 组合表达「第几个星期几」
          repeat_month_nth INTEGER,
          -- 结束条件：never / until / count
          repeat_end_mode TEXT,
          repeat_end_at INTEGER,
          repeat_count INTEGER,
          -- 累计完成次数与上次完成时间（统计用，不逐次留历史）
          repeat_done_count INTEGER NOT NULL DEFAULT 0,
          repeat_last_done_at TEXT
        );

        -- 待办标签：与笔记标签（tags / note_tags）是两套独立定义，互不同步
        CREATE TABLE IF NOT EXISTS todo_tags (
          id INTEGER PRIMARY KEY AUTOINCREMENT,
          name TEXT NOT NULL UNIQUE,
          -- '' = 用默认色；否则 #rrggbb
          color TEXT NOT NULL DEFAULT '',
          sort_order INTEGER NOT NULL DEFAULT 0,
          created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%d %H:%M:%f','now'))
        );

        CREATE TABLE IF NOT EXISTS todo_tag_links (
          todo_id INTEGER NOT NULL REFERENCES todos(id) ON DELETE CASCADE,
          tag_id INTEGER NOT NULL REFERENCES todo_tags(id) ON DELETE CASCADE,
          PRIMARY KEY (todo_id, tag_id)
        );

        CREATE TABLE IF NOT EXISTS stickies (
          id INTEGER PRIMARY KEY AUTOINCREMENT,
          slot INTEGER NOT NULL UNIQUE CHECK (slot IN (1, 2)),
          content TEXT NOT NULL DEFAULT '',
          -- 乐观锁版本号
          version INTEGER NOT NULL DEFAULT 0,
          created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%d %H:%M:%f','now')),
          updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%d %H:%M:%f','now'))
        );

        CREATE TABLE IF NOT EXISTS detached_stickies (
          id INTEGER PRIMARY KEY AUTOINCREMENT,
          slot INTEGER NOT NULL UNIQUE CHECK (slot IN (1, 2)),
          content TEXT NOT NULL DEFAULT '',
          x REAL,
          y REAL,
          always_on_top INTEGER NOT NULL DEFAULT 1,
          -- 乐观锁版本号
          version INTEGER NOT NULL DEFAULT 0,
          created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%d %H:%M:%f','now')),
          updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%d %H:%M:%f','now'))
        );

        CREATE TABLE IF NOT EXISTS snippets (
          id INTEGER PRIMARY KEY AUTOINCREMENT,
          title TEXT NOT NULL,
          content TEXT NOT NULL,
          is_pinned INTEGER NOT NULL DEFAULT 0,
          copy_count INTEGER NOT NULL DEFAULT 0,
          last_copied_at TEXT NOT NULL DEFAULT '',
          created_at TEXT NOT NULL,
          updated_at TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS countdowns (
          id INTEGER PRIMARY KEY AUTOINCREMENT,
          name TEXT NOT NULL,
          -- 重复模式：once 一次性 / daily 每天固定时刻 / interval 每隔 N 分钟
          repeat_mode TEXT NOT NULL DEFAULT 'once' CHECK (repeat_mode IN ('once', 'daily', 'interval')),
          -- 下一次到点时刻（毫秒时间戳）；once 为绝对时刻，daily 为当天 HH:MM，interval 为当前轮结束时刻
          end_at INTEGER NOT NULL,
          -- 周期总长（毫秒）：once 为创建时长，daily 为 24h，interval 为 interval_minutes*60000；用于水位进度计算
          total_ms INTEGER NOT NULL DEFAULT 0,
          -- interval 专用：间隔分钟数
          interval_minutes INTEGER,
          -- 暂停：once 冻结剩余时长，daily/interval 到点不提醒
          paused INTEGER NOT NULL DEFAULT 0,
          -- once 暂停时冻结的剩余毫秒（恢复时 end_at = now + paused_remaining_ms）
          paused_remaining_ms INTEGER,
          -- 工作台卡片不可见导致的自动冻结（区别于手动暂停；卡片恢复显示或浮窗浮起时自动恢复）
          auto_paused INTEGER NOT NULL DEFAULT 0,
          -- once 到点后置 1（卡片灰态，等手动删除）；daily/interval 永不置 1
          finished INTEGER NOT NULL DEFAULT 0,
          -- 浮窗状态与位置（浮起时创建独立透明圆窗，位置随拖动持久化）
          floated INTEGER NOT NULL DEFAULT 0,
          float_x REAL,
          float_y REAL,
          created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%d %H:%M:%f','now')),
          updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%d %H:%M:%f','now'))
        );

        CREATE TABLE IF NOT EXISTS chat_sessions (
          id INTEGER PRIMARY KEY AUTOINCREMENT,
          title TEXT NOT NULL DEFAULT '新对话',
          model_name TEXT NOT NULL DEFAULT '',
          created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%d %H:%M:%f','now')),
          updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%d %H:%M:%f','now')),
          -- 会话级累计 token（每轮回复完成后累加，用于面板顶部实时统计）
          tokens_input INTEGER NOT NULL DEFAULT 0,
          tokens_output INTEGER NOT NULL DEFAULT 0,
          tokens_cache_read INTEGER NOT NULL DEFAULT 0,
          tokens_reasoning INTEGER NOT NULL DEFAULT 0,
          -- 会话级累计生成耗时（毫秒），用于计算 TPS（输出 token / 秒）
          elapsed_ms INTEGER NOT NULL DEFAULT 0
        );

        CREATE TABLE IF NOT EXISTS chat_messages (
          id INTEGER PRIMARY KEY AUTOINCREMENT,
          session_id INTEGER NOT NULL REFERENCES chat_sessions(id) ON DELETE CASCADE,
          role TEXT NOT NULL CHECK (role IN ('user', 'assistant')),
          content TEXT NOT NULL DEFAULT '',
          created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%d %H:%M:%f','now'))
        );

        CREATE TABLE IF NOT EXISTS clipboard_history (
          id INTEGER PRIMARY KEY AUTOINCREMENT,
          content TEXT NOT NULL,
          html TEXT,
          source_app TEXT,
          is_pinned INTEGER NOT NULL DEFAULT 0,
          kind TEXT NOT NULL DEFAULT 'text',
          image_path TEXT,
          file_paths TEXT,
          created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%d %H:%M:%f','now')),
          updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%d %H:%M:%f','now'))
        );

        CREATE INDEX IF NOT EXISTS idx_notes_updated ON notes(updated_at DESC);
        CREATE INDEX IF NOT EXISTS idx_note_tags_tag ON note_tags(tag_id);
        CREATE INDEX IF NOT EXISTS idx_todos_created ON todos(created_at DESC);
        -- 注意：idx_todos_parent / idx_todos_remind 不能放进本批次——老库的 todos 表
        -- 尚无 parent_id/remind_at 列，CREATE INDEX 会在补列迁移前失败；
        -- 两者统一在 migrate() 尾部（ALTER 补列之后）创建。
        CREATE INDEX IF NOT EXISTS idx_countdowns_end ON countdowns(end_at);
        CREATE INDEX IF NOT EXISTS idx_chat_messages_session ON chat_messages(session_id, id);
        CREATE INDEX IF NOT EXISTS idx_clipboard_updated ON clipboard_history(updated_at DESC);
        -- 剪贴板去重索引（2026-09-29 补）。每次复制入库都会跑
        --   SELECT id ... WHERE content = ?1 AND kind = ?2 AND is_pinned = 0
        -- 而 content 上**原本没有任何索引**，所以每按一次 ⌘C 就是一次全表扫。
        -- 索引列顺序按查询的等值条件排：content 是选择性最高的，kind 次之。
        -- 代价可控的理由：索引项存的是 content 本身，所以索引体积 ≤ 表体积 ——
        -- 它换来的是把 O(行数) 的扫描换成 O(log n) 的查找，而表本身已经在那儿了。
        CREATE INDEX IF NOT EXISTS idx_clipboard_dedup ON clipboard_history(kind, content);
        ",
    )?;

    // ---- 旧版本迁移（分组模型 -> 合一模型） ----

    // 旧 resources 表含 group_id 列：重建为合一表，去掉分组外键
    let cols: Vec<String> = conn
        .prepare("PRAGMA table_info(resources)")?
        .query_map([], |row| row.get(1))?
        .collect::<rusqlite::Result<Vec<String>>>()?;
    if cols.iter().any(|c| c == "group_id") {
        // 旧表可能缺 last_launched_at（极早期版本），重建前先补上，保证 SELECT 不失败
        if !cols.iter().any(|c| c == "last_launched_at") {
            conn.execute("ALTER TABLE resources ADD COLUMN last_launched_at TEXT", [])?;
        }
        // ⚠️ 这 4 步必须同事务（2026-09-29 补）。SQLite 的 DDL 是事务性的，
        // 所以这能保证「要么全做完、要么一步没做」，从根上消除上面那种半截状态。
        // `execute_batch` 不接受事务作用域（它自己管理隐式事务），故逐条走 tx。
        //
        // 源表行数必须在**批处理之前**取：批次最后一条是 `DROP TABLE resources_old`，
        // 之后再去查它就会得到 "no such table"。
        let expected: i64 = conn.query_row("SELECT COUNT(*) FROM resources", [], |r| r.get(0))?;
        let tx = conn.unchecked_transaction()?;
        tx.execute_batch(
            "
            ALTER TABLE resources RENAME TO resources_old;
            CREATE TABLE resources (
              id INTEGER PRIMARY KEY AUTOINCREMENT,
              kind TEXT NOT NULL CHECK (kind IN ('app', 'web', 'file')),
              name TEXT NOT NULL,
              target TEXT NOT NULL,
              category TEXT,
              icon TEXT,
              args TEXT,
              sort_order INTEGER NOT NULL DEFAULT 0,
              last_launched_at TEXT,
              created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%d %H:%M:%f','now')),
              updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%d %H:%M:%f','now'))
            );
            INSERT INTO resources (id, kind, name, target, icon, args, sort_order, last_launched_at, created_at, updated_at)
              SELECT id, kind, name, target, icon, args, sort_order, last_launched_at, created_at, updated_at FROM resources_old;
            DROP TABLE resources_old;
            ",
        )
        .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
        // 校验行数没丢再提交：INSERT…SELECT 是静默的，少拷了行也不会报错
        let copied: i64 = tx.query_row("SELECT COUNT(*) FROM resources", [], |r| r.get(0))?;
        if copied != expected {
            // 不提交：回滚后仍是原表，用户数据毫发无损
            return Err(rusqlite::Error::ToSqlConversionFailure(Box::new(
                std::io::Error::other(format!(
                    "resources 重建行数不符：期望 {expected} 实得 {copied}，已回滚"
                )),
            )));
        }
        tx.commit()?;
        log::info!("resources 表已由分组模型迁移为合一模型（{copied} 行）");
    }

    // 旧 files 表：全部并入 resources（kind='file'，target=path，category 保留），然后删除
    if table_exists(conn, "files") {
        conn.execute(
            "INSERT INTO resources (kind, name, target, category, created_at, updated_at)
             SELECT 'file', name, path, category, created_at, updated_at FROM files",
            [],
        )?;
        conn.execute("DROP TABLE files", [])?;
    }

    // 旧 groups 表及其索引：数据层已合一，不再使用
    if table_exists(conn, "groups") {
        conn.execute("DROP TABLE groups", [])?;
    }
    conn.execute("DROP INDEX IF EXISTS idx_resources_group", [])?;
    conn.execute("DROP INDEX IF EXISTS idx_files_category", [])?;

    // 旧 ai_usage 表：AI 用量统计已拆分为扩展（com.m-hub.token-stats），宿主不再使用，删表清残留
    if table_exists(conn, "ai_usage") {
        conn.execute("DROP TABLE ai_usage", [])?;
    }
    conn.execute("DROP INDEX IF EXISTS idx_ai_usage_time", [])?;

    // 兜底：resources 表缺 last_launched_at 列时补充
    let cols: Vec<String> = conn
        .prepare("PRAGMA table_info(resources)")?
        .query_map([], |row| row.get(1))?
        .collect::<rusqlite::Result<Vec<String>>>()?;
    if !cols.iter().any(|c| c == "last_launched_at") {
        conn.execute("ALTER TABLE resources ADD COLUMN last_launched_at TEXT", [])?;
    }

    // 索引建立在迁移完成之后（旧表重建前没有 category 列，不能提前建）
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_resources_category ON resources(category)",
        [],
    )?;

    // 速达小类（ADR 0012）：大类（resources.kind）下单归属的小类库。
    // 文件大类的 7 个内置 category 直接作为初始小类并入（名称与前端 utils/categories.ts
    // 逐字一致），「其他」暂承接默认小类（细则待用户确认）；应用/网页初始为空、按需新建。
    // INSERT OR IGNORE + UNIQUE(kind,name) 保证幂等，老库升级与新装库都只补缺、不覆盖。
    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS resource_subcategories (
          id INTEGER PRIMARY KEY AUTOINCREMENT,
          kind TEXT NOT NULL CHECK (kind IN ('app', 'web', 'file')),
          name TEXT NOT NULL,
          sort_order INTEGER NOT NULL DEFAULT 0,
          is_default INTEGER NOT NULL DEFAULT 0,
          UNIQUE (kind, name)
        );

        INSERT OR IGNORE INTO resource_subcategories (kind, name, sort_order, is_default) VALUES
          ('file', '文件夹', 0, 0),
          ('file', '文档', 1, 0),
          ('file', '图片', 2, 0),
          ('file', '视频', 3, 0),
          ('file', '音频', 4, 0),
          ('file', '压缩包', 5, 0),
          ('file', '其他', 6, 1);
        ",
    )?;

    // 旧 chat_sessions 表缺 token 累计列：逐列补齐（ALTER TABLE ADD COLUMN 幂等）
    let chat_cols: Vec<String> = conn
        .prepare("PRAGMA table_info(chat_sessions)")?
        .query_map([], |row| row.get(1))?
        .collect::<rusqlite::Result<Vec<String>>>()?;
    for (col, def) in [
        ("tokens_input", "INTEGER NOT NULL DEFAULT 0"),
        ("tokens_output", "INTEGER NOT NULL DEFAULT 0"),
        ("tokens_cache_read", "INTEGER NOT NULL DEFAULT 0"),
        ("tokens_reasoning", "INTEGER NOT NULL DEFAULT 0"),
        ("elapsed_ms", "INTEGER NOT NULL DEFAULT 0"),
    ] {
        if !chat_cols.iter().any(|c| c == col) {
            conn.execute(
                &format!("ALTER TABLE chat_sessions ADD COLUMN {col} {def}"),
                [],
            )?;
        }
    }

    // 剪贴板历史从「纯文本」升级为「文本/图片/文件」三类型：逐列补齐（ALTER TABLE ADD COLUMN 幂等）
    let clip_cols: Vec<String> = conn
        .prepare("PRAGMA table_info(clipboard_history)")?
        .query_map([], |row| row.get(1))?
        .collect::<rusqlite::Result<Vec<String>>>()?;
    for (col, def) in [
        ("kind", "TEXT NOT NULL DEFAULT 'text'"),
        ("image_path", "TEXT"),
        ("file_paths", "TEXT"),
    ] {
        if !clip_cols.iter().any(|c| c == col) {
            conn.execute(
                &format!("ALTER TABLE clipboard_history ADD COLUMN {col} {def}"),
                [],
            )?;
        }
    }

    // 倒计时表补 auto_paused 列（工作台卡片不可见时的自动冻结标记，ALTER TABLE ADD COLUMN 幂等）
    let cd_cols: Vec<String> = conn
        .prepare("PRAGMA table_info(countdowns)")?
        .query_map([], |row| row.get(1))?
        .collect::<rusqlite::Result<Vec<String>>>()?;
    if !cd_cols.iter().any(|c| c == "auto_paused") {
        conn.execute(
            "ALTER TABLE countdowns ADD COLUMN auto_paused INTEGER NOT NULL DEFAULT 0",
            [],
        )?;
    }

    // 待办表补截止/提醒/子待办列（v0.3.4，ALTER TABLE ADD COLUMN 幂等；
    // parent_id 带 REFERENCES 子句时 SQLite 要求默认值为 NULL，恰好就是所需默认）
    let todo_cols: Vec<String> = conn
        .prepare("PRAGMA table_info(todos)")?
        .query_map([], |row| row.get(1))?
        .collect::<rusqlite::Result<Vec<String>>>()?;
    if !todo_cols.iter().any(|c| c == "due_at") {
        conn.execute("ALTER TABLE todos ADD COLUMN due_at INTEGER", [])?;
    }
    if !todo_cols.iter().any(|c| c == "remind_at") {
        conn.execute("ALTER TABLE todos ADD COLUMN remind_at INTEGER", [])?;
    }
    if !todo_cols.iter().any(|c| c == "remind_fired") {
        conn.execute(
            "ALTER TABLE todos ADD COLUMN remind_fired INTEGER NOT NULL DEFAULT 0",
            [],
        )?;
    }
    if !todo_cols.iter().any(|c| c == "parent_id") {
        conn.execute(
            "ALTER TABLE todos ADD COLUMN parent_id INTEGER REFERENCES todos(id) ON DELETE CASCADE",
            [],
        )?;
    }
    // 待办手动排序位（v0.4.3，拖拽排序；NULL = 未手动排序）
    if !todo_cols.iter().any(|c| c == "sort_order") {
        conn.execute("ALTER TABLE todos ADD COLUMN sort_order INTEGER", [])?;
    }
    // 乐观锁版本号（局域网同步冲突检测：写回时 WHERE version 比对，每次 +1）
    if !todo_cols.iter().any(|c| c == "version") {
        conn.execute(
            "ALTER TABLE todos ADD COLUMN version INTEGER NOT NULL DEFAULT 0",
            [],
        )?;
    }
    // 待办 v1（标签 / 置顶 / 周期）：描述 / 置顶 / 周期规则列。全部可空或带 DEFAULT，
    // 老库启动即补齐，零数据改造（与 v0.3.4 补列的既有模式一致）。
    if !todo_cols.iter().any(|c| c == "description") {
        conn.execute(
            "ALTER TABLE todos ADD COLUMN description TEXT NOT NULL DEFAULT ''",
            [],
        )?;
    }
    if !todo_cols.iter().any(|c| c == "pinned") {
        conn.execute(
            "ALTER TABLE todos ADD COLUMN pinned INTEGER NOT NULL DEFAULT 0",
            [],
        )?;
    }
    if !todo_cols.iter().any(|c| c == "repeat_mode") {
        conn.execute(
            "ALTER TABLE todos ADD COLUMN repeat_mode TEXT NOT NULL DEFAULT 'once'",
            [],
        )?;
    }
    for col in [
        "repeat_every",
        "repeat_weekdays",
        "repeat_month_day",
        "repeat_month_nth",
        "repeat_end_at",
        "repeat_count",
    ] {
        if !todo_cols.iter().any(|c| c == col) {
            conn.execute(&format!("ALTER TABLE todos ADD COLUMN {col} INTEGER"), [])?;
        }
    }
    for col in ["repeat_unit", "repeat_end_mode", "repeat_last_done_at"] {
        if !todo_cols.iter().any(|c| c == col) {
            conn.execute(&format!("ALTER TABLE todos ADD COLUMN {col} TEXT"), [])?;
        }
    }
    if !todo_cols.iter().any(|c| c == "repeat_done_count") {
        conn.execute(
            "ALTER TABLE todos ADD COLUMN repeat_done_count INTEGER NOT NULL DEFAULT 0",
            [],
        )?;
    }
    // 便签/浮窗便签表补乐观锁版本号
    let sticky_cols: Vec<String> = conn
        .prepare("PRAGMA table_info(stickies)")?
        .query_map([], |row| row.get(1))?
        .collect::<rusqlite::Result<Vec<String>>>()?;
    if !sticky_cols.iter().any(|c| c == "version") {
        conn.execute(
            "ALTER TABLE stickies ADD COLUMN version INTEGER NOT NULL DEFAULT 0",
            [],
        )?;
    }
    let detached_cols: Vec<String> = conn
        .prepare("PRAGMA table_info(detached_stickies)")?
        .query_map([], |row| row.get(1))?
        .collect::<rusqlite::Result<Vec<String>>>()?;
    if !detached_cols.iter().any(|c| c == "version") {
        conn.execute(
            "ALTER TABLE detached_stickies ADD COLUMN version INTEGER NOT NULL DEFAULT 0",
            [],
        )?;
    }
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_todos_parent ON todos(parent_id)",
        [],
    )?;
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_todos_remind ON todos(remind_at)",
        [],
    )?;
    // 待办升级新增索引：同样必须在补列之后创建（老库此时才具备这些列）
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_todo_tag_links_tag ON todo_tag_links(tag_id)",
        [],
    )?;
    // 曾建的 idx_todos_repeat 已移除：全工程没有按 repeat_mode 等值过滤的查询
    // （周期展开用 `repeat_mode <> 'once'`，SQLite 不走索引），它只让每次写 todos
    // 多维护一棵 B 树。老库在这里顺手删掉。
    conn.execute("DROP INDEX IF EXISTS idx_todos_repeat", [])?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::params;

    #[test]
    fn fresh_db_has_unified_resources() {
        let conn = init_in_memory().unwrap();
        conn.execute(
            "INSERT INTO resources (kind, name, target, category) VALUES ('app', 'VS Code', '/x/code.exe', NULL)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO resources (kind, name, target, category) VALUES ('file', '报告', 'C:/docs/report.pdf', '文档')",
            [],
        )
        .unwrap();
        let n: i64 = conn
            .query_row("SELECT COUNT(*) FROM resources", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 2);
        assert!(!table_exists(&conn, "groups"));
        assert!(!table_exists(&conn, "files"));
    }

    #[test]
    fn legacy_group_files_schema_migrates() {
        // 构造旧版库：groups + 带 group_id 的 resources + files
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "
            CREATE TABLE groups (
              id INTEGER PRIMARY KEY AUTOINCREMENT,
              name TEXT NOT NULL,
              sort_order INTEGER NOT NULL DEFAULT 0,
              created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%d %H:%M:%f','now')),
              updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%d %H:%M:%f','now'))
            );
            CREATE TABLE resources (
              id INTEGER PRIMARY KEY AUTOINCREMENT,
              group_id INTEGER NOT NULL REFERENCES groups(id) ON DELETE CASCADE,
              kind TEXT NOT NULL CHECK (kind IN ('app', 'web')),
              name TEXT NOT NULL,
              target TEXT NOT NULL,
              icon TEXT,
              args TEXT,
              sort_order INTEGER NOT NULL DEFAULT 0,
              created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%d %H:%M:%f','now')),
              updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%d %H:%M:%f','now'))
            );
            CREATE TABLE files (
              id INTEGER PRIMARY KEY AUTOINCREMENT,
              name TEXT NOT NULL,
              path TEXT NOT NULL,
              category TEXT NOT NULL DEFAULT '其他',
              created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%d %H:%M:%f','now')),
              updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%d %H:%M:%f','now'))
            );
            INSERT INTO groups (id, name) VALUES (1, '开发');
            INSERT INTO resources (id, group_id, kind, name, target) VALUES (1, 1, 'app', 'VS Code', '/x/code.exe');
            INSERT INTO resources (id, group_id, kind, name, target) VALUES (2, 1, 'web', 'GitHub', 'https://github.com');
            INSERT INTO files (id, name, path, category) VALUES (1, '报告', 'C:/docs/report.pdf', '文档');
            INSERT INTO files (id, name, path, category) VALUES (2, '照片', 'C:/pics/a.png', '图片');
            ",
        )
        .unwrap();

        migrate(&conn).unwrap();

        let n: i64 = conn
            .query_row("SELECT COUNT(*) FROM resources", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 4);
        assert!(!table_exists(&conn, "groups"));
        assert!(!table_exists(&conn, "files"));

        // 文件已并入，kind='file' 且分类保留
        let files: Vec<(String, String, Option<String>)> = conn
            .prepare("SELECT name, kind, category FROM resources WHERE kind = 'file' ORDER BY id")
            .unwrap()
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap();
        assert_eq!(files.len(), 2);
        assert_eq!(files[0].1, "file");
        assert_eq!(files[0].2.as_deref(), Some("文档"));
        assert_eq!(files[1].2.as_deref(), Some("图片"));

        // 原 app/web 资源保留
        let apps: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM resources WHERE kind IN ('app', 'web')",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(apps, 2);
    }

    #[test]
    fn legacy_todos_schema_migrates() {
        // 构造旧版库：todos 表无 due_at/remind_at/remind_fired/parent_id 列（v0.3.3 及更早），
        // 且已有数据。迁移必须先补列、后建引用新列的索引——若 CREATE INDEX 混在
        // migrate() 开头的建表批次里，老库会因「no such column: parent_id」启动崩溃。
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "
            CREATE TABLE todos (
              id INTEGER PRIMARY KEY AUTOINCREMENT,
              title TEXT NOT NULL,
              done INTEGER NOT NULL DEFAULT 0,
              priority INTEGER NOT NULL DEFAULT 0,
              created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%d %H:%M:%f','now')),
              updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%d %H:%M:%f','now')),
              completed_at TEXT
            );
            INSERT INTO todos (title, done) VALUES ('迁移前的老待办', 0);
            ",
        )
        .unwrap();

        migrate(&conn).unwrap();

        // 新列已补齐
        let cols: Vec<String> = conn
            .prepare("PRAGMA table_info(todos)")
            .unwrap()
            .query_map([], |row| row.get(1))
            .unwrap()
            .collect::<rusqlite::Result<Vec<String>>>()
            .unwrap();
        for col in ["due_at", "remind_at", "remind_fired", "parent_id", "sort_order"] {
            assert!(cols.iter().any(|c| c == col), "缺列 {col}");
        }

        // 引用新列的索引已创建
        for idx in ["idx_todos_parent", "idx_todos_remind"] {
            let n: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM sqlite_master WHERE type='index' AND name=?1",
                    params![idx],
                    |r| r.get(0),
                )
                .unwrap();
            assert_eq!(n, 1, "缺索引 {idx}");
        }

        // 老数据完好，且新列取默认值
        let (title, due, parent): (String, Option<i64>, Option<i64>) = conn
            .query_row(
                "SELECT title, due_at, parent_id FROM todos WHERE id = 1",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        assert_eq!(title, "迁移前的老待办");
        assert_eq!(due, None);
        assert_eq!(parent, None);
    }

    #[test]
    fn legacy_v033_full_schema_migrates_twice_and_works() {
        // v0.3.3 → v0.4.1 直接升级路径：0.3.3 与 0.4.1 的建表差异只有两处——
        // todos 缺 due_at/remind_at/remind_fired/parent_id，countdowns 缺 auto_paused；
        // 其余表结构相同（由 IF NOT EXISTS 兜底）。本测试构造这两张差异表 + 存量数据，
        // 验证：migrate 连跑两次都成功（幂等）、老数据无损、业务层 repo 正常读写、级联删除生效。
        let conn = Connection::open_in_memory().unwrap();
        conn.pragma_update(None, "foreign_keys", "ON").unwrap();
        conn.execute_batch(
            "
            CREATE TABLE todos (
              id INTEGER PRIMARY KEY AUTOINCREMENT,
              title TEXT NOT NULL,
              done INTEGER NOT NULL DEFAULT 0,
              priority INTEGER NOT NULL DEFAULT 0,
              created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%d %H:%M:%f','now')),
              updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%d %H:%M:%f','now')),
              completed_at TEXT
            );
            CREATE TABLE countdowns (
              id INTEGER PRIMARY KEY AUTOINCREMENT,
              name TEXT NOT NULL,
              repeat_mode TEXT NOT NULL DEFAULT 'once' CHECK (repeat_mode IN ('once', 'daily', 'interval')),
              end_at INTEGER NOT NULL,
              total_ms INTEGER NOT NULL DEFAULT 0,
              interval_minutes INTEGER,
              paused INTEGER NOT NULL DEFAULT 0,
              paused_remaining_ms INTEGER,
              finished INTEGER NOT NULL DEFAULT 0,
              floated INTEGER NOT NULL DEFAULT 0,
              float_x REAL,
              float_y REAL,
              created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%d %H:%M:%f','now')),
              updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%d %H:%M:%f','now'))
            );
            INSERT INTO todos (title, done) VALUES ('升级前待办A', 0);
            INSERT INTO todos (title, done, completed_at) VALUES ('升级前待办B', 1, '2026-08-01 10:00:00.000');
            INSERT INTO countdowns (name, end_at, total_ms) VALUES ('升级前倒计时', 9999999999999, 60000);
            ",
        )
        .unwrap();

        // 连跑两次：第二次验证已迁移库上的幂等性（不重复加列/建索引报错）
        migrate(&conn).unwrap();
        migrate(&conn).unwrap();

        // 差异列全部补齐
        let cols: Vec<String> = conn
            .prepare("PRAGMA table_info(todos)")
            .unwrap()
            .query_map([], |row| row.get(1))
            .unwrap()
            .collect::<rusqlite::Result<Vec<String>>>()
            .unwrap();
        for col in ["due_at", "remind_at", "remind_fired", "parent_id", "sort_order"] {
            assert!(cols.iter().any(|c| c == col), "todos 缺列 {col}");
        }
        let cd_cols: Vec<String> = conn
            .prepare("PRAGMA table_info(countdowns)")
            .unwrap()
            .query_map([], |row| row.get(1))
            .unwrap()
            .collect::<rusqlite::Result<Vec<String>>>()
            .unwrap();
        assert!(cd_cols.iter().any(|c| c == "auto_paused"), "countdowns 缺 auto_paused");

        // 存量数据无损
        let todo_n: i64 = conn
            .query_row("SELECT COUNT(*) FROM todos", [], |r| r.get(0))
            .unwrap();
        assert_eq!(todo_n, 2);
        let cd_n: i64 = conn
            .query_row("SELECT COUNT(*) FROM countdowns", [], |r| r.get(0))
            .unwrap();
        assert_eq!(cd_n, 1);

        // 业务层读取正常（SELECT 引用全部新列，迁移不完整会在此暴露）
        let todos = crate::repo::todo::list(&conn).unwrap();
        assert_eq!(todos.len(), 2);
        assert!(todos.iter().all(|t| t.due_at.is_none() && t.parent_id.is_none()));

        // 新能力可用：父子级联删除（外键 ON DELETE CASCADE 经 ALTER 补列后生效）
        let parent = crate::repo::todo::create(&conn, "父待办", None, None).unwrap();
        let child = crate::repo::todo::create(&conn, "子待办", Some(parent.id), None).unwrap();
        let removed = crate::repo::todo::delete(&conn, parent.id).unwrap();
        assert_eq!(removed, vec![child.id]);
        let left: i64 = conn
            .query_row("SELECT COUNT(*) FROM todos WHERE id IN (?1, ?2)", params![parent.id, child.id], |r| r.get(0))
            .unwrap();
        assert_eq!(left, 0);
    }

    #[test]
    fn ai_usage_table_is_dropped_on_migrate() {
        // 构造旧版库：含 ai_usage 表（旧版 token 统计专用）及其索引，模拟老用户升级
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "
            CREATE TABLE ai_usage (
              id INTEGER PRIMARY KEY AUTOINCREMENT,
              session_id TEXT NOT NULL DEFAULT '',
              provider TEXT,
              model TEXT,
              tokens_input INTEGER NOT NULL DEFAULT 0,
              tokens_cache_read INTEGER NOT NULL DEFAULT 0,
              tokens_cache_write INTEGER NOT NULL DEFAULT 0,
              tokens_output INTEGER NOT NULL DEFAULT 0,
              tokens_reasoning INTEGER NOT NULL DEFAULT 0,
              cost REAL NOT NULL DEFAULT 0,
              time_created INTEGER NOT NULL,
              source TEXT NOT NULL DEFAULT 'remote'
            );
            CREATE INDEX idx_ai_usage_time ON ai_usage(time_created);
            INSERT INTO ai_usage (session_id, tokens_input, tokens_output, time_created)
              VALUES ('s1', 100, 200, 1);
            ",
        )
        .unwrap();

        migrate(&conn).unwrap();

        // ai_usage 表与索引被清除，核心表正常建立（其它数据不受影响）
        assert!(!table_exists(&conn, "ai_usage"));
        assert!(table_exists(&conn, "resources"));
        assert!(table_exists(&conn, "notes"));
        let idx: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='index' AND name='idx_ai_usage_time'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(idx, 0);
    }

    /// 旧代码的非原子重建会在崩溃/断电后留下 `resources_old`。
    /// 那种库必须能**自动救回**，否则速达数据永远卡在旧表里、界面全空且无报错。
    ///
    /// 这里刻意不模拟崩溃，而是直接构造出崩溃后的**终态**（两种都测）：
    ///   ① 有 `resources_old` 但**没有** `resources`（崩在 RENAME 之后、CREATE 之前）
    ///   ② 两者都有（新表已建好但没 DROP 完）
    ///
    /// ① 尤其关键：那种状态下 `PRAGMA table_info(resources)` 返回**空集且不报错**
    /// （查的是不存在的表），所以光靠列名检测会判假、跳过一切恢复 ——
    /// 这正是旧代码把数据丢干净的那一步。
    #[test]
    fn recovers_from_half_renamed_resources_table() {
        // ① 崩在 RENAME 之后：只剩 resources_old
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE resources_old (
               id INTEGER PRIMARY KEY AUTOINCREMENT,
               kind TEXT NOT NULL,
               name TEXT NOT NULL,
               target TEXT NOT NULL,
               category TEXT,
               icon TEXT, args TEXT,
               sort_order INTEGER NOT NULL DEFAULT 0,
               last_launched_at TEXT,
               created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%d %H:%M:%f','now')),
               updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%d %H:%M:%f','now'))
             );
             INSERT INTO resources_old (kind, name, target) VALUES
               ('app', '速达甲', 'https://a.example'),
               ('app', '速达乙', 'https://b.example');",
        )
        .unwrap();
        migrate(&conn).unwrap();

        assert!(table_exists(&conn, "resources"), "resources 表应被救回");
        assert!(!table_exists(&conn, "resources_old"), "resources_old 应被清理");
        let names: Vec<String> = conn
            .prepare("SELECT name FROM resources ORDER BY id")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap();
        assert_eq!(names, vec!["速达甲", "速达乙"], "速达数据必须一条不少地救回");

        // ② 新表已在、残留 resources_old：内容并回，已有的行不被覆盖
        let conn2 = Connection::open_in_memory().unwrap();
        conn2.execute_batch(
            "CREATE TABLE resources_old (
               id INTEGER PRIMARY KEY AUTOINCREMENT,
               kind TEXT NOT NULL, name TEXT NOT NULL, target TEXT NOT NULL,
               category TEXT, icon TEXT, args TEXT,
               sort_order INTEGER NOT NULL DEFAULT 0, last_launched_at TEXT,
               created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%d %H:%M:%f','now')),
               updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%d %H:%M:%f','now'))
             );
             INSERT INTO resources_old (id, kind, name, target) VALUES (1,'app','旧行','x');
             CREATE TABLE resources (
               id INTEGER PRIMARY KEY AUTOINCREMENT,
               kind TEXT NOT NULL CHECK (kind IN ('app','web','file')),
               name TEXT NOT NULL, target TEXT NOT NULL, category TEXT, icon TEXT, args TEXT,
               sort_order INTEGER NOT NULL DEFAULT 0, last_launched_at TEXT,
               created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%d %H:%M:%f','now')),
               updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%d %H:%M:%f','now'))
             );
             INSERT INTO resources (id, kind, name, target) VALUES (9,'app','新行','y');",
        )
        .unwrap();
        migrate(&conn2).unwrap();

        assert!(!table_exists(&conn2, "resources_old"), "残留的 resources_old 应被清理");
        let mut names: Vec<String> = conn2
            .prepare("SELECT name FROM resources ORDER BY id")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap();
        names.sort();
        assert_eq!(
            names,
            vec!["新行".to_string(), "旧行".to_string()],
            "新表已有行要保住，残留旧表的行要并回来"
        );
    }

    /// 并回**失败**时必须保留 `resources_old`，不能顺手把它删掉。
    ///
    /// 这条是被变异测试逼出来的：把 `DROP TABLE resources_old` 挪到
    /// `if let Err(..)` 分支里（无论成败都删）后，`recovers_from_half_renamed_resources_table`
    /// **照样通过** —— 因为它构造的旧表列是全对的、合并根本不会失败。
    /// 于是「失败时也删」这个真实存在的数据丢失写法，没有任何测试拦得住。
    ///
    /// 这里的做法是**故意让合并失败**：旧表缺 `icon` 列，而并回的 SELECT
    /// 显式列出了 `icon`，SQLite 直接报 "no such column: icon"。
    /// 正确行为是：报错、但**表还在**、数据一条不少。
    #[test]
    fn failed_merge_keeps_resources_old() {
        // 必须**两张表都在**才能走到「并回」分支：只有 resources_old 时，
        // 恢复逻辑会走更前面的「改名回填」（无损，同样保住了数据），
        // 根本到不了这里。要构造的是「崩在 CREATE 之后、DROP 之前」那个中间态。
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE resources (
               id INTEGER PRIMARY KEY AUTOINCREMENT,
               kind TEXT NOT NULL CHECK (kind IN ('app','web','file')),
               name TEXT NOT NULL, target TEXT NOT NULL, category TEXT,
               icon TEXT, args TEXT, sort_order INTEGER NOT NULL DEFAULT 0,
               last_launched_at TEXT,
               created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%d %H:%M:%f','now')),
               updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%d %H:%M:%f','now'))
             );
             CREATE TABLE resources_old (
               id INTEGER PRIMARY KEY AUTOINCREMENT,
               kind TEXT NOT NULL, name TEXT NOT NULL, target TEXT NOT NULL,
               category TEXT,
               -- 故意缺 icon / args / sort_order / last_launched_at / 时间戳，
               -- 好让并回的 SELECT 必然失败（no such column: icon）
               junk TEXT
             );
             INSERT INTO resources_old (kind, name, target, junk) VALUES ('app','救我','t', 'j');",
        )
        .unwrap();
        migrate(&conn).unwrap();

        assert!(
            table_exists(&conn, "resources_old"),
            "并回失败时**必须**保留 resources_old —— 删掉就从「能救」变成「彻底没了」"
        );
        let n: i64 = conn
            .query_row("SELECT COUNT(*) FROM resources_old", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 1, "数据必须一条不少地留在旧表里");
    }

    /// 重建后行数必须与源表一致（少拷了行 INSERT…SELECT 不会报错）。
    /// 校验放在提交前 —— 不一致就回滚，用户数据毫发无损。
    #[test]
    fn resources_rebuild_preserves_row_count() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE groups (id INTEGER PRIMARY KEY, name TEXT NOT NULL);
             CREATE TABLE resources (
               id INTEGER PRIMARY KEY AUTOINCREMENT,
               group_id INTEGER NOT NULL REFERENCES groups(id),
               kind TEXT NOT NULL CHECK (kind IN ('app','web')),
               name TEXT NOT NULL, target TEXT NOT NULL,
               icon TEXT, args TEXT, sort_order INTEGER NOT NULL DEFAULT 0,
               created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%d %H:%M:%f','now')),
               updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%d %H:%M:%f','now'))
             );",
        )
        .unwrap();
        for i in 0..25 {
            conn.execute(
                "INSERT INTO groups (id, name) VALUES (?1, ?2)",
                rusqlite::params![i, format!("组{i}")],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO resources (group_id, kind, name, target) VALUES (?1,'app',?2,?3)",
                rusqlite::params![i, format!("速达{i}"), format!("t{i}")],
            )
            .unwrap();
        }
        migrate(&conn).unwrap();
        let n: i64 = conn
            .query_row("SELECT COUNT(*) FROM resources", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 25, "重建后行数必须与源表一致");
    }

    /// 剪贴板去重索引必须**真被查询计划用上**，而不只是建了个索引。
    ///
    /// 建了却没走上的索引是纯粹的装饰品：磁盘多占一份、每次复制照样全表扫，
    /// 而没有任何症状能暴露它。`EXPLAIN QUERY PLAN` 里出现
    /// `USING INDEX idx_clipboard_dedup` 才算真的用上了。
    #[test]
    fn clipboard_dedup_index_is_actually_used() {
        let conn = init_in_memory().unwrap();
        crate::repo::clipboard::insert(&conn, "重复内容", None, None).unwrap();
        let plan: String = conn
            .query_row(
                "EXPLAIN QUERY PLAN
                 SELECT id FROM clipboard_history
                  WHERE content = ?1 AND kind = ?2 AND is_pinned = 0
                  ORDER BY updated_at DESC, id DESC LIMIT 1",
                params!["重复内容", "text"],
                // EXPLAIN QUERY PLAN 的列是 (id, parent, notused, detail)，描述在第 3 列
                |r| r.get::<_, String>(3),
            )
            .unwrap();
        assert!(
            plan.contains("idx_clipboard_dedup"),
            "去重查询没有走 idx_clipboard_dedup（索引白建了）: {plan}"
        );
    }
}
