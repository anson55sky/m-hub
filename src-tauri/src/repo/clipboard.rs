use crate::models::ClipboardItem;
use crate::repo::now;
use rusqlite::{params, Connection, Result};
use std::sync::atomic::{AtomicU64, Ordering};

/// 单条文本最大存储长度（超过截断，防数据库膨胀）
pub const MAX_ITEM_LEN: usize = 20000;
/// 单条 html 片段最大长度（复制整页/大表格时 html 可能达数百 KB，超限丢弃 html 只留纯文本）
pub const MAX_HTML_LEN: usize = 64 * 1024;
/// 保留策略清理最小间隔：连续复制时每次入库都跑两个 DELETE（大表可能耗时数十 ms），
/// 节流到至少间隔该时长才执行一次，避免高频复制时 UI/监听被锁拖慢。
const CLEANUP_INTERVAL_MS: u64 = 60_000;
/// 上次清理时间戳（毫秒，进程内单调），初始为 0 表示首条就清理一次
static LAST_CLEANUP_MS: AtomicU64 = AtomicU64::new(0);

/// 节流后的保留策略清理：距离上次执行不足 CLEANUP_INTERVAL_MS 时直接跳过。
/// 保留策略允许延迟执行（临时多存几条无碍），换取热路径不频繁锁表。
pub fn cleanup_throttled(conn: &Connection) {
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    let last = LAST_CLEANUP_MS.load(Ordering::Relaxed);
    if now_ms.saturating_sub(last) < CLEANUP_INTERVAL_MS {
        return;
    }
    if LAST_CLEANUP_MS
        .compare_exchange(last, now_ms, Ordering::Relaxed, Ordering::Relaxed)
        .is_ok()
    {
        let _ = cleanup(conn);
    }
}

/// 记录文本（可能来自剪贴板）：
/// - 超过 MAX_ITEM_LEN 截断（html 同步丢弃，避免与截断文本不一致）
/// - html 超过 MAX_HTML_LEN 丢弃（只留纯文本，防止 app.db 暴涨）
/// - 相同内容（非置顶）已存在 → 刷新 updated_at/source_app 挪到最前，不新增重复条目
/// - 写入后执行保留策略清理
pub fn insert(conn: &Connection, content: &str, html: Option<&str>, source: Option<&str>) -> Result<()> {
    let truncated = content.chars().take(MAX_ITEM_LEN).collect::<String>();
    let html = if content.len() == truncated.len() {
        html.and_then(|h| {
            if h.len() > MAX_HTML_LEN {
                None
            } else {
                Some(h.to_string())
            }
        })
    } else {
        None
    };
    insert_typed(conn, "text", &truncated, html.as_deref(), None, None, source).map(|_| ())
}

/// 记录图片：`dedup_key` 为图片字节哈希（用于「相同图片只留一条」去重），
/// `image_path` 为快照落盘路径。content 列复用为去重键，前端展示走缩略图。
/// 返回 true 表示新插入，false 表示命中去重（调用方可据此删除本次落盘的冗余快照）。
pub fn insert_image(
    conn: &Connection,
    dedup_key: &str,
    image_path: &str,
    source: Option<&str>,
) -> Result<bool> {
    insert_typed(conn, "image", dedup_key, None, Some(image_path), None, source)
}

/// 记录文件列表：`file_paths` 为原始路径（不拷贝文件内容），content 列存路径连接便于搜索与去重。
pub fn insert_files(conn: &Connection, file_paths: &[String], source: Option<&str>) -> Result<()> {
    let content = file_paths.join("\n");
    let json = serde_json::to_string(file_paths).unwrap_or_default();
    insert_typed(conn, "file", &content, None, None, Some(&json), source).map(|_| ())
}

/// 按类型入库的统一实现：content 作为去重键，kind 隔离各类型命名空间，
/// 相同 content（非置顶）已存在 → 刷新时间与来源挪到最前，不新增重复条目。
/// 返回 true 表示新插入，false 表示去重更新。
fn insert_typed(
    conn: &Connection,
    kind: &str,
    content: &str,
    html: Option<&str>,
    image_path: Option<&str>,
    file_paths: Option<&str>,
    source: Option<&str>,
) -> Result<bool> {
    let ts = now();

    let recent_id: Option<i64> = conn
        .query_row(
            "SELECT id FROM clipboard_history
             WHERE content = ?1 AND kind = ?2 AND is_pinned = 0
             ORDER BY updated_at DESC, id DESC LIMIT 1",
            params![content, kind],
            |r| r.get(0),
        )
        .ok();

    if let Some(id) = recent_id {
        conn.execute(
            "UPDATE clipboard_history SET updated_at = ?1, source_app = COALESCE(?2, source_app) WHERE id = ?3",
            params![ts, source, id],
        )?;
        cleanup_throttled(conn);
        Ok(false)
    } else {
        conn.execute(
            "INSERT INTO clipboard_history (content, html, source_app, kind, image_path, file_paths, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7)",
            params![content, html, source, kind, image_path, file_paths, ts],
        )?;
        cleanup_throttled(conn);
        Ok(true)
    }
}

/// 使用记录：刷新 updated_at 把该条目挪到列表最前（粘贴/复制历史项时调用）。
pub fn touch(conn: &Connection, id: i64) -> Result<()> {
    conn.execute(
        "UPDATE clipboard_history SET updated_at = ?1 WHERE id = ?2",
        params![now(), id],
    )?;
    Ok(())
}

/// 保留策略（Q5/Q12 决策）：
/// - 非置顶项超过 ttl_days 自动删除
/// - 总条数（含置顶）超过 max_items 时删最旧
pub fn cleanup(conn: &Connection) -> Result<()> {
    let cfg = crate::config::load();
    cleanup_with(conn, cfg.clipboard_max_items, cfg.clipboard_ttl_days)
}

/// 保留策略实际执行（参数化便于测试，避免污染真实配置）
pub fn cleanup_with(conn: &Connection, max_items: i64, ttl_days: i64) -> Result<()> {
    let max_items = max_items.max(1);

    let cutoff = chrono::Utc::now()
        .checked_sub_signed(chrono::Duration::days(ttl_days.max(0)))
        .unwrap_or_else(chrono::Utc::now)
        .format("%Y-%m-%d %H:%M:%S%.6f")
        .to_string();

    // 删除前先收集将被清掉的图片快照路径，联动删除磁盘文件（防孤儿快照泄漏）
    let mut doomed: Vec<String> = Vec::new();
    let mut stmt = conn.prepare(
        "SELECT image_path FROM clipboard_history
         WHERE is_pinned = 0 AND updated_at < ?1 AND kind = 'image' AND image_path IS NOT NULL",
    )?;
    for p in stmt.query_map(params![cutoff], |r| r.get::<_, String>(0))? {
        doomed.push(p?);
    }
    let mut stmt = conn.prepare(
        "SELECT image_path FROM clipboard_history WHERE id IN (
           SELECT id FROM clipboard_history
           ORDER BY updated_at DESC, id DESC
           LIMIT -1 OFFSET ?1
         ) AND kind = 'image' AND image_path IS NOT NULL",
    )?;
    for p in stmt.query_map(params![max_items], |r| r.get::<_, String>(0))? {
        doomed.push(p?);
    }

    // 两条 DELETE 必须同事务（2026-09-29 补），而且它们**不是独立的**：
    // 第二条的 `OFFSET ?1` 是「跳过最新的 max_items 条后全删」，这个集合是在
    // 第一条（TTL 清理）**已经生效**的前提下算出来的。分开提交时若第一条成功、
    // 第二条失败，历史就被按错误的边界裁掉了 —— 而且不可逆（删掉的就是历史记录）。
    let tx = conn.unchecked_transaction()?;
    tx.execute(
        "DELETE FROM clipboard_history WHERE is_pinned = 0 AND updated_at < ?1",
        params![cutoff],
    )?;
    tx.execute(
        "DELETE FROM clipboard_history WHERE id IN (
           SELECT id FROM clipboard_history
           ORDER BY updated_at DESC, id DESC
           LIMIT -1 OFFSET ?1
         )",
        params![max_items],
    )?;
    tx.commit()?;

    // 删磁盘文件**必须在 commit 之后**。`doomed` 是提交前按「即将删除」算出的，
    // 若先删文件再提交、而提交失败回滚了，就会留下「记录还在、图片没了」的行
    // —— 用户点开一条有缩略图的历史，图片却是裂的，且因为记录还在、后续清理
    // 不会再来删这个孤儿文件，永久残留。
    for p in doomed {
        let _ = std::fs::remove_file(&p);
    }
    Ok(())
}

/// 置顶优先，其次最近复制；可选关键字搜索（内容/来源应用）；offset 支持滚动加载后续页
pub fn list(conn: &Connection, keyword: Option<&str>, limit: i64, offset: i64) -> Result<Vec<ClipboardItem>> {
    let kw = keyword.map(|k| k.trim()).filter(|k| !k.is_empty());
    let mut sql = String::from(
        "SELECT id, content, html, source_app, is_pinned, kind, image_path, file_paths, created_at, updated_at
         FROM clipboard_history ",
    );
    if let Some(k) = kw {
        let pattern = format!("%{}%", k);
        sql.push_str(
            "WHERE content LIKE ?1 OR html LIKE ?1 OR COALESCE(source_app,'') LIKE ?1 ",
        );
        sql.push_str("ORDER BY is_pinned DESC, updated_at DESC, id DESC LIMIT ?2 OFFSET ?3");
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(params![pattern, limit, offset], row_to_item)?;
        rows.collect()
    } else {
        sql.push_str("ORDER BY is_pinned DESC, updated_at DESC, id DESC LIMIT ?1 OFFSET ?2");
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(params![limit, offset], row_to_item)?;
        rows.collect()
    }
}

pub fn get(conn: &Connection, id: i64) -> Result<ClipboardItem> {
    conn.query_row(
        "SELECT id, content, html, source_app, is_pinned, kind, image_path, file_paths, created_at, updated_at
         FROM clipboard_history WHERE id = ?1",
        params![id],
        row_to_item,
    )
}

pub fn toggle_pin(conn: &Connection, id: i64) -> Result<ClipboardItem> {
    conn.execute(
        "UPDATE clipboard_history SET is_pinned = CASE WHEN is_pinned = 1 THEN 0 ELSE 1 END, updated_at = ?1 WHERE id = ?2",
        params![now(), id],
    )?;
    get(conn, id)
}

pub fn delete(conn: &Connection, id: i64) -> Result<()> {
    conn.execute("DELETE FROM clipboard_history WHERE id = ?1", params![id])?;
    Ok(())
}

/// 合并多条**文本**为一条新的，并删掉原来那几条。
///
/// ## 三个关键决定
///
/// **① 顺序 = 列表顺序**，即 `list()` 的 `ORDER BY is_pinned DESC, updated_at DESC, id DESC`。
/// 用户勾选的先后**不影响**结果——他看到的是面板上的顺序，合并出来就该是那个顺序。
/// 这条不能靠调用方把 id 排好再传进来：那等于让前端复制一份排序口径，两边必然漂移。
///
/// **② 永远 INSERT 新行，不走 `insert_typed` 的去重。**
/// 去重那套是给「后台监听剪贴板」用的：同一段内容再次被复制，视作同一条刷新时间。
/// 而合并是用户的**一次性显式动作**，他说「合成一条新的」。若在这里走去重，
/// 拼出来的内容一旦与某条已有记录相同，就会去**顶替那条无关记录**（改它的
/// 时间戳和来源），而原条目照样被删——用户看到的是「我明明新建了一条，
/// 结果有条不相干的记录被改了」。两种语义不能混用。
///
/// **③ 置顶取「任一为置顶」。** 置顶是用户对这批内容价值的判断，合并掉其中一条
/// 不该把那个判断悄悄丢掉。
///
/// ## 原子性
///
/// 建新与删旧在**同一个事务**里。中途失败（磁盘满、锁冲突）必须整体回滚：
/// 只建不删 = 多出一条重复；只删不建 = **内容直接丢了**，后者是不可接受的。
///
/// 事务内**不做** `cleanup_throttled`（它会连带清理图片文件，跨文件操作无法随
/// 事务回滚）；交由调用方在提交后触发，与普通写入同一套节奏。
///
/// ⚠️ **本函数验不到什么**（写明以免误以为已被覆盖）：
/// 「建新成功、删旧中途失败」需要故障注入（磁盘写满 / 锁冲突 / 在两条 DELETE
/// 之间 kill 进程）才能造出来，本工程的测试环境造不出来。已覆盖的是**能造出来**
/// 的那些失败路径：少于两条、id 已消失、含非文本 —— 三种都断言「一条都没被删」。
/// 事务原子性本身靠 rusqlite 保证，未经测试证明。
///
/// 返回合并后的新条目。`ids` 少于 2 条、含非文本、或含已不存在的 id 都直接报错
/// —— 宁可不做，也不要「悄悄少合了几条」。
pub fn merge_texts(conn: &Connection, ids: &[i64]) -> Result<ClipboardItem> {
    if ids.len() < 2 {
        return Err(rusqlite::Error::InvalidParameterName(
            "至少要选两条才能合并".into(),
        ));
    }
    let tx = conn.unchecked_transaction()?;

    // 按列表顺序取出（与 list() 同一条 ORDER BY）
    let placeholders = std::iter::repeat("?")
        .take(ids.len())
        .collect::<Vec<_>>()
        .join(",");
    let sql = format!(
        "SELECT id, content, source_app, is_pinned FROM clipboard_history
         WHERE id IN ({placeholders})
         ORDER BY is_pinned DESC, updated_at DESC, id DESC"
    );
    // 语句必须**先于 commit 析构**（它借着 tx），所以全部圈进块里。
    // 直接把 prepare 的结果 collect 完再 commit 会报 E0505「不能 move 出 tx」。
    let rows: Vec<(i64, String, Option<String>, bool)> = {
        let mut stmt = tx.prepare(&sql)?;
        // 先把 query_map 的结果绑成变量再 collect（同 list() 的写法）：
        // 直接写成链式表达式会让迭代器活到块尾，和 stmt 一起析构 → E0597。
        let mapped = stmt.query_map(rusqlite::params_from_iter(ids.iter()), |r| {
            Ok((
                r.get(0)?,
                r.get::<_, String>(1)?,
                r.get(2)?,
                r.get::<_, i64>(3)? != 0,
            ))
        })?;
        let collected = mapped.collect::<std::result::Result<_, _>>()?;
        collected
    };

    if rows.len() != ids.len() {
        // 有 id 已不存在（可能刚被 TTL 清理）。报明确错，而不是少合几条——
        // 少合的结果是「我明明选了 5 条，它只用了 4 条」且无任何提示。
        return Err(rusqlite::Error::InvalidParameterName(format!(
            "有 {} 条已不在剪贴板历史里（可能刚被过期清理），请重新选择",
            ids.len() - rows.len()
        )));
    }

    // 非文本条目（图片/文件）无法按行拼接：把二进制当文本合并出来的只会是乱码。
    let non_text: Vec<(i64, String)> = {
        let mut stmt = tx.prepare(&format!(
            "SELECT id, kind FROM clipboard_history WHERE id IN ({placeholders}) AND kind <> 'text'"
        ))?;
        let mapped = stmt.query_map(rusqlite::params_from_iter(ids.iter()), |r| {
            Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?))
        })?;
        // 每行出错就跳过：这条查询只用来「告诉用户哪几条不能合」，
        // 单行读不出来不该让整个合并失败（下面的条数对账仍会兜住少合的问题）
        let collected = mapped.filter_map(|r| r.ok()).collect::<Vec<_>>();
        collected
    };
    if !non_text.is_empty() {
        let kinds: Vec<String> = non_text
            .iter()
            .map(|(_, k)| match k.as_str() {
                "image" => "图片".to_string(),
                "file" => "文件".to_string(),
                other => other.to_string(),
            })
            .collect();
        return Err(rusqlite::Error::InvalidParameterName(format!(
            "只能合并文本条目，其中含：{}",
            kinds.join("、")
        )));
    }

    let content: String = rows
        .iter()
        .map(|(_, c, _, _)| c.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    let truncated: String = content.chars().take(MAX_ITEM_LEN).collect();
    let pinned = rows.iter().any(|(_, _, _, p)| *p);
    // 来源取列表里最靠前那条（它通常是用户最后复制的那条）
    let source = rows.first().and_then(|(_, _, s, _)| s.clone());
    let ts = now();

    tx.execute(
        "INSERT INTO clipboard_history (content, html, source_app, kind, is_pinned, created_at, updated_at)
         VALUES (?1, NULL, ?2, 'text', ?3, ?4, ?4)",
        params![truncated, source, pinned as i64, ts],
    )?;
    let new_id = tx.last_insert_rowid();

    for (id, _, _, _) in &rows {
        tx.execute("DELETE FROM clipboard_history WHERE id = ?1", params![id])?;
    }
    tx.commit()?;

    get(conn, new_id)
}

pub fn clear(conn: &Connection) -> Result<()> {
    conn.execute("DELETE FROM clipboard_history", [])?;
    Ok(())
}

/// 所有图片快照路径（清空历史时联动删除磁盘文件，避免孤儿快照泄漏）
pub fn image_paths(conn: &Connection) -> Result<Vec<String>> {
    let mut stmt = conn.prepare(
        "SELECT image_path FROM clipboard_history WHERE kind = 'image' AND image_path IS NOT NULL",
    )?;
    let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
    rows.collect()
}

pub fn count(conn: &Connection) -> Result<i64> {
    conn.query_row("SELECT COUNT(*) FROM clipboard_history", [], |r| r.get(0))
}

fn row_to_item(row: &rusqlite::Row) -> Result<ClipboardItem> {
    let file_paths: Vec<String> = row
        .get::<_, Option<String>>(7)?
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default();
    Ok(ClipboardItem {
        id: row.get(0)?,
        content: row.get(1)?,
        html: row.get(2)?,
        source_app: row.get(3)?,
        is_pinned: row.get(4)?,
        kind: row.get(5)?,
        image_path: row.get(6)?,
        file_paths,
        created_at: row.get(8)?,
        updated_at: row.get(9)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::init_in_memory;

    fn setup() -> Connection {
        init_in_memory().unwrap()
    }

    /// 插入 n 条并返回它们的 id（按插入顺序）
    fn seed(conn: &Connection, items: &[&str]) -> Vec<i64> {
        items
            .iter()
            .map(|t| {
                insert(conn, t, None, None).unwrap();
                last_id(conn)
            })
            .collect()
    }

    fn last_id(conn: &Connection) -> i64 {
        conn.last_insert_rowid()
    }

    fn contents(conn: &Connection) -> Vec<String> {
        list(conn, None, 50, 0).unwrap().into_iter().map(|i| i.content).collect()
    }

    /// 合并后只剩一条，内容按**列表顺序**（新→旧）换行拼接
    #[test]
    fn merge_joins_in_list_order_and_removes_originals() {
        let conn = setup();
        let ids = seed(&conn, &["A", "B", "C"]);
        // 列表顺序是 C,B,A（updated_at DESC）
        let merged = merge_texts(&conn, &ids).unwrap();
        assert_eq!(merged.content, "C\nB\nA");
        let left = contents(&conn);
        assert_eq!(left.len(), 1, "原来三条必须都没了，只剩合并结果");
        assert_eq!(left[0], "C\nB\nA");
    }

    /// 传进去的 id 顺序**不该**影响结果：用户勾选的先后与看到的列表顺序无关
    #[test]
    fn merge_result_is_independent_of_id_argument_order() {
        // 两个独立库：同一批 id 不能合并两次（第一次就把它们删了）
        let c1 = setup();
        let c2 = setup();
        let ids1 = seed(&c1, &["A", "B", "C"]);
        let ids2 = seed(&c2, &["A", "B", "C"]);
        let forward = merge_texts(&c1, &ids1).unwrap();
        let shuffled = merge_texts(&c2, &[ids2[2], ids2[0], ids2[1]]).unwrap();
        assert_eq!(forward.content, shuffled.content);
    }

    /// 置顶的那条排在列表最前，所以合并结果应当以它开头；
    /// 且合并结果继承「任一为置顶」。
    #[test]
    fn merge_puts_pinned_first_and_keeps_pinned_flag() {
        let conn = setup();
        let ids = seed(&conn, &["A", "B"]);
        toggle_pin(&conn, ids[0]).unwrap();
        let merged = merge_texts(&conn, &ids).unwrap();
        assert_eq!(merged.content, "A\nB", "置顶项应排在最前");
        assert!(merged.is_pinned, "任一为置顶 → 合并结果置顶");
    }

    /// 合并是显式动作，**不走** insert_typed 的去重：
    /// 拼出来的内容若与某条已有记录相同，必须新建一条，而不是顶替它。
    #[test]
    fn merge_always_inserts_even_if_content_already_exists() {
        let conn = setup();
        seed(&conn, &["A", "B"]);
        // 先放一条内容恰好等于合并结果的记录
        insert(&conn, "B\nA", None, Some("原记录")).unwrap();
        let before = list(&conn, None, 50, 0).unwrap();
        let victim = before.iter().find(|i| i.content == "B\nA").unwrap().clone();

        // 只选 A、B 两条 —— 「原记录」不参与，否则拼出来是 "B\nA\nB\nA"
        let picked: Vec<i64> = before
            .iter()
            .filter(|i| i.content == "A" || i.content == "B")
            .map(|i| i.id)
            .collect();
        let merged = merge_texts(&conn, &picked).unwrap();

        assert_eq!(merged.content, "B\nA");
        // 原来那条「原记录」必须**原封不动**还在（没被顶替：时间戳/来源不变）
        let still = get(&conn, victim.id).unwrap();
        assert_eq!(still.source_app.as_deref(), Some("原记录"));
        assert!(merged.id != victim.id, "必须是一条新行，而不是复用旧行");
    }

    #[test]
    fn merge_rejects_single_id() {
        let conn = setup();
        let ids = seed(&conn, &["only"]);
        assert!(merge_texts(&conn, &ids).is_err());
        assert_eq!(contents(&conn).len(), 1, "失败时不得改动任何数据");
    }

    /// id 少一条时要**报错**，不能悄悄少合几条
    #[test]
    fn merge_rejects_when_some_id_vanished() {
        let conn = setup();
        let ids = seed(&conn, &["A", "B"]);
        let missing = 999_999;
        let err = merge_texts(&conn, &[ids[0], ids[1], missing]).unwrap_err();
        assert!(err.to_string().contains("已不在剪贴板历史"), "实际: {err}");
        assert_eq!(contents(&conn).len(), 2, "失败时原条目必须都还在");
    }

    /// 图片/文件不能按行拼：报错，且一条都不许删
    #[test]
    fn merge_rejects_non_text_and_keeps_everything() {
        let conn = setup();
        insert(&conn, "文本", None, None).unwrap();
        let text_id = last_id(&conn);
        insert_image(&conn, "img-dedup-key", "/tmp/shot.png", None).unwrap();
        let img_id = last_id(&conn);
        let err = merge_texts(&conn, &[text_id, img_id]).unwrap_err();
        assert!(err.to_string().contains("只能合并文本"), "实际: {err}");
        assert_eq!(contents(&conn).len(), 2, "失败时文本与图片都必须还在");
    }

    /// 超出 MAX_ITEM_LEN 时截断，且**不留半截尾巴**
    #[test]
    fn merge_truncates_overlong_content() {
        let conn = setup();
        let long = "x".repeat(MAX_ITEM_LEN + 500);
        let ids = seed(&conn, &["short", &long]);
        let merged = merge_texts(&conn, &ids).unwrap();
        assert_eq!(merged.content.chars().count(), MAX_ITEM_LEN);
    }

    /// 新条目排在最前（用户合并完就该在顶部看到它）
    #[test]
    fn merged_item_appears_at_top_of_list() {
        let conn = setup();
        let ids = seed(&conn, &["A", "B"]);
        insert(&conn, "更新的内容", None, None).unwrap();
        // 期望值取自**合并前**的列表顺序，而不是写死 —— 同一毫秒插入的条目
        // 靠 `id DESC` 定序，写死 "A\nB" 是在断言一个我没验证过的猜测。
        let expected = list(&conn, None, 50, 0)
            .unwrap()
            .iter()
            .filter(|i| i.content == "A" || i.content == "B")
            .map(|i| i.content.clone())
            .collect::<Vec<_>>()
            .join("\n");
        let merged = merge_texts(&conn, &ids).unwrap();
        assert_eq!(merged.content, expected);
        assert_eq!(contents(&conn)[0], expected, "合并结果应出现在列表最前");
    }

    #[test]
    fn insert_then_list_newest_first() {
        let conn = setup();
        insert(&conn, "first", None, None).unwrap();
        insert(&conn, "second", Some("<b>html</b>"), Some("浏览器")).unwrap();
        let list = list(&conn, None, 50, 0).unwrap();
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].content, "second");
        assert_eq!(list[0].html.as_deref(), Some("<b>html</b>"));
        assert_eq!(list[0].source_app.as_deref(), Some("浏览器"));
        assert_eq!(list[1].content, "first");
    }

    #[test]
    fn dedup_within_window_refreshes_not_inserts() {
        let conn = setup();
        insert(&conn, "same", None, Some("A")).unwrap();
        let first: i64 = conn
            .query_row("SELECT id FROM clipboard_history", [], |r| r.get(0))
            .unwrap();
        insert(&conn, "same", None, Some("B")).unwrap();
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM clipboard_history", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 1);
        let all = list(&conn, None, 50, 0).unwrap();
        assert_eq!(all[0].source_app.as_deref(), Some("B"));
        assert!(first > 0);
    }

    #[test]
    fn long_content_is_truncated_and_drops_html() {
        let conn = setup();
        let long = "x".repeat(MAX_ITEM_LEN + 100);
        insert(&conn, &long, Some("<b>html</b>"), None).unwrap();
        let all = list(&conn, None, 50, 0).unwrap();
        assert_eq!(all[0].content.chars().count(), MAX_ITEM_LEN);
        assert!(all[0].html.is_none());
    }

    #[test]
    fn dedup_anytime_refreshes_existing_row() {
        let conn = setup();
        insert(&conn, "same", None, Some("A")).unwrap();
        // 模拟很久以前的旧条目（远超原 2s 去重窗口），再复制相同内容
        conn.execute(
            "UPDATE clipboard_history SET updated_at = '2000-01-01 00:00:00.000000'",
            [],
        )
        .unwrap();
        insert(&conn, "same", None, Some("B")).unwrap();
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM clipboard_history", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 1, "相同内容不新增条目");
        let all = list(&conn, None, 50, 0).unwrap();
        assert_eq!(all[0].source_app.as_deref(), Some("B"));
        assert!(all[0].updated_at.as_str() > "2000-01-01");
    }

    #[test]
    fn touch_moves_item_to_front() {
        let conn = setup();
        insert(&conn, "a", None, None).unwrap();
        insert(&conn, "b", None, None).unwrap();
        let a_id = list(&conn, None, 50, 0).unwrap()[1].id;
        std::thread::sleep(std::time::Duration::from_millis(5));
        touch(&conn, a_id).unwrap();
        let all = list(&conn, None, 50, 0).unwrap();
        assert_eq!(all[0].id, a_id, "touch 后条目挪到最前");
    }

    #[test]
    fn pin_exempts_from_ttl_cleanup() {
        let conn = setup();
        let old = chrono::Utc::now()
            .checked_sub_signed(chrono::Duration::days(30))
            .unwrap()
            .format("%Y-%m-%d %H:%M:%S%.6f")
            .to_string();
        conn.execute(
            "INSERT INTO clipboard_history (content, is_pinned, created_at, updated_at) VALUES ('pinned', 1, ?1, ?1)",
            params![old],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO clipboard_history (content, is_pinned, created_at, updated_at) VALUES ('plain', 0, ?1, ?1)",
            params![old],
        )
        .unwrap();
        cleanup(&conn).unwrap();
        let all = list(&conn, None, 50, 0).unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].content, "pinned");
    }

    #[test]
    fn max_items_caps_total_including_pinned() {
        let conn = setup();
        // 直接参数化调用，避免污染真实用户配置
        insert(&conn, "a", None, None).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(5));
        insert(&conn, "b", None, None).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(5));
        insert(&conn, "c", None, None).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(5));
        insert(&conn, "d", None, None).unwrap();

        cleanup_with(&conn, 3, 7).unwrap();
        let all = list(&conn, None, 50, 0).unwrap();
        assert_eq!(all.len(), 3);
        assert!(!all.iter().any(|i| i.content == "a"));
    }

    #[test]
    fn search_matches_content() {
        let conn = setup();
        insert(&conn, "https://github.com/tauri", None, None).unwrap();
        insert(&conn, "别的文本", None, None).unwrap();
        let found = list(&conn, Some("github"), 50, 0).unwrap();
        assert_eq!(found.len(), 1);
        assert!(found[0].content.contains("github"));
    }

    #[test]
    fn toggle_pin_and_delete() {
        let conn = setup();
        insert(&conn, "x", None, None).unwrap();
        let item = list(&conn, None, 50, 0).unwrap().remove(0);
        let pinned = toggle_pin(&conn, item.id).unwrap();
        assert!(pinned.is_pinned);
        delete(&conn, item.id).unwrap();
        assert!(get(&conn, item.id).is_err());
        assert_eq!(count(&conn).unwrap(), 0);
    }

    #[test]
    fn insert_image_and_files_roundtrip() {
        let conn = setup();
        insert_image(&conn, "aabbccdd11223344", "C:/m-hub/clipboard/images/1.png", Some("画图")).unwrap();
        insert_files(
            &conn,
            &["C:/a.txt".to_string(), "C:/b.pdf".to_string()],
            Some("资源管理器"),
        )
        .unwrap();
        let all = list(&conn, None, 50, 0).unwrap();
        assert_eq!(all.len(), 2);
        // 后插入的文件在最前
        assert_eq!(all[0].kind, "file");
        assert_eq!(
            all[0].file_paths,
            vec!["C:/a.txt".to_string(), "C:/b.pdf".to_string()]
        );
        assert_eq!(all[0].content, "C:/a.txt\nC:/b.pdf");
        assert_eq!(all[1].kind, "image");
        assert_eq!(all[1].image_path.as_deref(), Some("C:/m-hub/clipboard/images/1.png"));
        assert_eq!(all[1].content, "aabbccdd11223344");
    }

    #[test]
    fn image_dedup_refreshes_not_inserts() {
        let conn = setup();
        insert_image(&conn, "samehash", "C:/img/a.png", Some("A")).unwrap();
        insert_image(&conn, "samehash", "C:/img/b.png", Some("B")).unwrap();
        assert_eq!(count(&conn).unwrap(), 1, "相同图片哈希不新增条目");
        let all = list(&conn, None, 50, 0).unwrap();
        assert_eq!(all[0].source_app.as_deref(), Some("B"));
        assert_eq!(all[0].image_path.as_deref(), Some("C:/img/a.png"));
    }

    /// `cleanup_with` 的整体行为：按 max_items 裁剪、联动删掉被裁掉记录的图片文件、
    /// 且个别文件已不存在时不能整个失败。
    ///
    /// ## 这条测试**验不到**什么（别把它当守卫用）
    ///
    /// 加事务时定下的关键顺序约束是「图片文件必须在 commit **之后**才 unlink」
    /// —— `doomed` 是提交前算出的，若先删文件再提交、而提交失败回滚，就会留下
    /// 「记录还在、图片没了」的行（用户看到裂图，且记录还在、后续清理不会再删
    /// 这个孤儿文件，永久残留）。
    ///
    /// 但**本测试证明不了这一点**：我把 unlink 挪到 commit 之前跑过，
    /// 这条用例照样通过 —— 因为无论顺序如何，成功路径结束时文件都没了。
    /// 要区分这两种顺序必须在两者之间注入一次提交失败，而当前代码没有可注入的
    /// 故障点。所以这条只是 `cleanup_with` 的行为回归，**顺序约束靠的是代码里
    /// 那段注释与 review，不是它**。若日后 `cleanup_with` 引入依赖注入或返回错误
    /// 的分支，那时才补得上一条真正能区分顺序的用例。
    #[test]
    fn cleanup_removes_image_files_only_after_commit() {
        let conn = setup();
        // 两条图片记录 + 一个真实存在的文件；另有两条纯文本
        let dir = std::env::temp_dir().join("mhub_clip_cleanup_test");
        std::fs::create_dir_all(&dir).unwrap();
        let img = dir.join("a.png");
        std::fs::write(&img, b"x").unwrap();

        insert_image(&conn, "h1", img.to_str().unwrap(), None).unwrap();
        insert_image(&conn, "h2", "does-not-exist.png", None).unwrap();
        insert(&conn, "文本一", None, None).unwrap();
        insert(&conn, "文本二", None, None).unwrap();
        assert_eq!(count(&conn).unwrap(), 4);

        // max_items=2 → 只留最新 2 条（文本一/文本二），两条图片记录被裁掉
        cleanup_with(&conn, 2, 3650).unwrap();

        let left = list(&conn, None, 50, 0).unwrap();
        assert_eq!(left.len(), 2, "应按 max_items 裁到 2 条");
        assert!(
            !img.exists(),
            "图片文件应在提交后被删除（否则回滚会留下「记录在、图片没了」的行）"
        );
        // 不存在的路径被删也不应报错（整个清理流程不能因为一个孤儿文件就失败）
        assert!(cleanup_with(&conn, 2, 3650).is_ok());

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 置顶的记录不参与任何一条清理（TTL 与超量都跳过它）。
    /// 加事务后要确认两条 DELETE 都没把 pin 住的记录误删。
    #[test]
    fn cleanup_keeps_pinned_items() {
        let conn = setup();
        insert(&conn, "会被裁掉", None, None).unwrap();
        insert(&conn, "置顶保留", None, None).unwrap();
        let pinned = list(&conn, None, 50, 0)
            .unwrap()
            .into_iter()
            .find(|i| i.content == "置顶保留")
            .unwrap();
        toggle_pin(&conn, pinned.id).unwrap();

        cleanup_with(&conn, 1, 3650).unwrap();
        let left = list(&conn, None, 50, 0).unwrap();
        assert_eq!(left.len(), 1);
        assert_eq!(left[0].content, "置顶保留");
    }
}
