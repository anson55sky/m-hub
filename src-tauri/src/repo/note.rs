use crate::models::Note;
use crate::repo::now;
use rusqlite::{params, Connection, Result};

pub fn create(conn: &Connection, title: &str) -> Result<Note> {
    let ts = now();
    conn.execute(
        "INSERT INTO notes (title, created_at, updated_at) VALUES (?1, ?2, ?2)",
        params![title, ts],
    )?;
    get(conn, conn.last_insert_rowid())
}

pub fn get(conn: &Connection, id: i64) -> Result<Note> {
    conn.query_row(
        &format!("SELECT {NOTE_COLS} FROM notes WHERE id = ?1"),
        params![id],
        row_to_note,
    )
}

/// 全部**未删除**笔记。
///
/// ⚠️ 回收站里的笔记不进这个列表 —— 它们只在「回收站」视图里出现。
///   不过滤的话，用户会在主列表里看到已删除的笔记（软删只是打了个标记），
///   而那是「删了还在」的直观感受。
pub fn list(conn: &Connection) -> Result<Vec<Note>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {NOTE_COLS} FROM notes WHERE deleted_at IS NULL {LIVE_ORDER}"
    ))?;
    let rows = stmt.query_map([], row_to_note)?;
    rows.collect()
}

/// 笔记列表（仅元信息，不拉 content）：用于外部保存速记后主窗口刷新列表，
/// 避免每次刷新都全量读取正文，数据量大时省内存省 IO。
///
/// ⚠️ 同样排除回收站 —— 否则外部浮层保存速记后刷新列表，会把已删除的笔记
///   又带回来一条。
pub fn list_meta(conn: &Connection) -> Result<Vec<Note>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT id, title, '', folder_id, deleted_at, icon, created_at, updated_at
         FROM notes WHERE deleted_at IS NULL {LIVE_ORDER}"
    ))?;
    let rows = stmt.query_map([], row_to_note)?;
    rows.collect()
}

pub fn update(conn: &Connection, id: i64, title: &str, content: &str) -> Result<Note> {
    let affected = conn.execute(
        "UPDATE notes SET title = ?1, content = ?2, updated_at = ?3 WHERE id = ?4",
        params![title, content, now(), id],
    )?;
    if affected == 0 {
        // 带 NOT_FOUND 前缀：扩展桥调用方据此与服务器错误区分，不再盲目重试
        return Err(rusqlite::Error::InvalidParameterName(format!(
            "NOT_FOUND: 笔记 {id} 不存在"
        )));
    }
    get(conn, id)
}

pub fn delete(conn: &Connection, id: i64) -> Result<()> {
    conn.execute("DELETE FROM notes WHERE id = ?1", params![id])?;
    Ok(())
}

/// SQL `LIKE` 的转义：让 `%` / `_` 按字面量参与匹配。
///
/// 不转的话用户搜 `100%` 会变成「以 100 开头、任意字符结尾」，
/// 搜 `a_b` 会匹配到 `axb`。`ESCAPE` 子句指定用哪个字符当转义符。
fn like_pattern(keyword: &str) -> String {
    let escaped: String = keyword
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_");
    format!("%{escaped}%")
}

/// 清掉搜索词里的不可见字符。
///
/// 为什么需要：正文是用户**原样**存进去的，网页复制来的内容常夹带零宽空格
/// （`U+200B` 防断词）、软连字符（`U+00AD`）这类字符。SQL 的 `LIKE` 完全不认
/// 这些码位 —— 它们既不是 `\s`，也不是任何可打印字符，于是
/// `发<U+200B>布计划` 里的「发布计划」四个字在 `LIKE '%发布计划%'` 眼里
/// **根本不是连续的**，一次本该命中的搜索返回空。
///
/// 前端那侧（`src/utils/invisibleChars.ts`）对**摘要与搜索结果片段**做同样的
/// 清理。两边码位清单必须一致 —— 一端清一端不清，就是摘要干净但搜不到。
///
/// 分类规则与前端一致：① 有可见宽度的空白 → 换成普通空格；② 不可见的零宽类
/// 与软连字符 → 删掉。**不在这里替换数据库里的正文** —— 那是改用户的数据，
/// 而搜索只是读取侧的问题。
fn normalize_keyword(keyword: &str) -> String {
    let mut out = String::with_capacity(keyword.len());
    for ch in keyword.chars() {
        match ch {
            // ① 有可见宽度：换成普通空格（它们的宽度不同但都算空白）
            '\u{00A0}' | '\u{1680}' | '\u{2000}'..='\u{200A}'
            | '\u{2028}' | '\u{2029}' | '\u{202F}' | '\u{205F}' | '\u{3000}' => out.push(' '),
            // ② 不可见的零宽类：删掉
            '\u{200B}' | '\u{200C}' | '\u{200E}' | '\u{200F}' | '\u{2060}' | '\u{FEFF}' => {}
            // ③ 软连字符：删掉
            '\u{00AD}' => {}
            // ⚠️ `U+200D`（ZWJ）**刻意不清**：emoji 序列靠它组合
            //    （`👨‍👩‍👧` = 3 个 emoji + 2 个 ZWJ），清掉 emoji 就碎了。
            //    与前端 `invisibleChars.ts` 的取舍一致。
            _ => out.push(ch),
        }
    }
    out
}

/// 文本里是否含需要清理的不可见字符（`U+200D` ZWJ 不算，见 `normalize_keyword`）。
///
/// 单独一个函数是为了让 `matches_normalized` 能**先问一句再决定要不要归一化**：
/// 绝大多数笔记正文一个这类字符都没有，那就不该为它分配一个新 String。
fn has_invisible(s: &str) -> bool {
    s.chars().any(|c| {
        matches!(c,
            '\u{00A0}' | '\u{1680}' | '\u{2000}'..='\u{200A}'
            | '\u{2028}' | '\u{2029}' | '\u{202F}' | '\u{205F}' | '\u{3000}'
            | '\u{200B}' | '\u{200C}' | '\u{200E}' | '\u{200F}'
            | '\u{2060}' | '\u{FEFF}' | '\u{00AD}')
    })
}

/// 「忽略不可见字符」的包含判断。`needle` 必须已归一化。
fn matches_normalized(haystack: &str, needle: &str) -> bool {
    if has_invisible(haystack) {
        normalize_keyword(haystack).contains(needle)
    } else {
        haystack.contains(needle)
    }
}

/// 笔记全文搜索（标题或正文命中）。
///
/// **两段式**，顺序不能反：
///
/// 1. **原样 `LIKE`** —— 走 SQL，正文里的通配符已转义。绝大多数搜索在这里结束。
/// 2. **归一化后逐行比对** —— 只在第一段**一条都没命中**时才跑。
///
/// ⚠️ 第 2 段必须**同时归一化正文**，只归一化关键词是不够的：
/// 不可见字符绝大多数在**正文**里（用户原样粘进来的），而用户记得的词是干净的 ——
/// `发<U+200B>布计划` 配关键词「发布计划」。第一版只清了关键词侧，
/// 测试当场就红了（`search_finds_through_zero_width_characters`）。
///
/// 代价：第 2 段是一次全表读 + 逐行字节扫描。之所以可以接受，有三条：
/// ① 只在第 1 段落空时才发生，而第 1 段落空**通常**意味着「确实没有」——
///    多花一次扫表换正确性，比「搜不到」划算；
/// ② `matches_normalized` 先用 `has_invisible` 问一句，干净的行**不分配**新 String，
///    所以这一段的实际开销接近一次顺序读；
/// ③ 前端搜索有 300ms 防抖（约定 10），是「每次停顿一次」而非「每次击键一次」。
/// 真要再省，就得给正文建一张归一化后的影子表 —— 那是另一件事，不在本轮范围。
///
/// ⚠️ 关键词全由不可见字符组成时**返回空**而不是全部：归一化后是空串，
/// 若继续比对，`contains("")` 对任何字符串都为真 —— 症状是
/// 「搜一个看不见的字符，整个笔记库全出来了」。
pub fn search(conn: &Connection, keyword: &str) -> Result<Vec<Note>> {
    let hits = search_like(conn, &like_pattern(keyword))?;
    if !hits.is_empty() {
        return Ok(hits);
    }
    let needle = normalize_keyword(keyword);
    if needle.is_empty() {
        return Ok(Vec::new());
    }
    let mut stmt = conn.prepare(
        &format!("SELECT {NOTE_COLS} FROM notes \
         ORDER BY updated_at DESC, id DESC",
    ))?;
    let rows = stmt.query_map([], row_to_note)?;
    let mut out = Vec::new();
    for row in rows {
        let n = row?;
        if matches_normalized(&n.title, &needle) || matches_normalized(&n.content, &needle) {
            out.push(n);
        }
    }
    Ok(out)
}

fn search_like(conn: &Connection, pattern: &str) -> Result<Vec<Note>> {
    // ⚠️ `ESCAPE '\\'` 必须**两处都写**（title 与 content）。只写一处的话，
    //    转义符的作用域只覆盖那一个谓词 —— 漏一处就是「搜 `%` 时标题命中、
    //    正文不命中」这种只在特定字段上复现的怪现象。
    //
    // ⚠️ 排序补 `id DESC` 与 `list()` / `list_meta()` 对齐：同一毫秒内新建的多条
    //    笔记 `updated_at` 相同，不加 `id` 兜底时次序由 SQLite 内部决定 ——
    //    「两段式的第 1 段结果」与「直接 list 的结果」会给出不同顺序。
    let mut stmt = conn.prepare(
        &format!("SELECT {NOTE_COLS} FROM notes \
         WHERE title LIKE ?1 ESCAPE '\\' OR content LIKE ?1 ESCAPE '\\' \
         ORDER BY updated_at DESC, id DESC",
    ))?;
    let rows = stmt.query_map(params![pattern], row_to_note)?;
    rows.collect()
}

pub fn row_to_note(row: &rusqlite::Row) -> Result<Note> {
    Ok(Note {
        id: row.get(0)?,
        title: row.get(1)?,
        content: row.get(2)?,
        folder_id: row.get(3)?,
        deleted_at: row.get(4)?,
        icon: row.get(5)?,
        created_at: row.get(6)?,
        updated_at: row.get(7)?,
    })
}

/// 笔记列表的列（含 folder_id / deleted_at / icon）。
const NOTE_COLS: &str =
    "id, title, content, folder_id, deleted_at, icon, created_at, updated_at";

/// 列表查询的 WHERE 与 ORDER。
///
/// ⚠️ 回收站里的笔记**默认不进任何列表** —— 它们只在「回收站」视图里出现。
///   若这里不过滤，用户会在主列表里看到已删除的笔记（因为软删只是打了个标记），
///   而那是「删了还在」的直观感受。
const LIVE_ORDER: &str = "ORDER BY updated_at DESC, id DESC";

/// 按文件夹筛选（含全部后代）的笔记列表。
///
/// `folder_ids` 的三种取值：
/// - `None` → 全部未删除笔记（不按文件夹过滤）
/// - `Some(&[])` → **只要未归类**（`folder_id IS NULL`）
/// - `Some(&[1, 2])` → 这些文件夹（含各自全部后代，由调用方先算好 id 集合）
///
/// ⚠️ 「未归类」必须单独一个条件，不能并进 `folder_id IN (...)`：
///   `IN` 里放不进 `NULL`，而 `folder_id = NULL` 是「未归类」这个**一等公民**，
///   不是「某个 id 为 NULL 的文件夹」。混进 `IN` 的话，选「未归类」会返回空列表，
///   而用户看到的是「这个文件夹里一条笔记都没有」。
pub fn list_by_folder(
    conn: &Connection,
    folder_ids: Option<&[i64]>,
) -> Result<Vec<Note>> {
    let sql = match folder_ids {
        None => format!("SELECT {NOTE_COLS} FROM notes WHERE deleted_at IS NULL {LIVE_ORDER}"),
        Some(ids) if ids.is_empty() => format!(
            "SELECT {NOTE_COLS} FROM notes WHERE deleted_at IS NULL AND folder_id IS NULL {LIVE_ORDER}"
        ),
        Some(ids) => {
            // `IN (?, ?, ...)` 的占位符个数必须与 ids 长度一致 ——
            // 拼错的话 SQLite 会报「参数个数不匹配」，而那种错只在运行时出现。
            let ph = ids.iter().map(|_| "?").collect::<Vec<_>>().join(", ");
            format!(
                "SELECT {NOTE_COLS} FROM notes WHERE deleted_at IS NULL
                 AND folder_id IN ({ph}) {LIVE_ORDER}"
            )
        }
    };
    let mut stmt = conn.prepare(&sql)?;
    let rows = match folder_ids {
        Some(ids) if !ids.is_empty() => {
            // ⚠️ 不能用 `params![ids]`：rusqlite 的 `params!` 对 `&[i64]`
            //   不直接支持（它要 `&[&dyn ToSql]`）。逐个转。
            let refs: Vec<&dyn rusqlite::ToSql> =
                ids.iter().map(|i| i as &dyn rusqlite::ToSql).collect();
            stmt.query_map(&refs[..], row_to_note)?
        }
        _ => stmt.query_map([], row_to_note)?,
    };
    rows.collect()
}

/// 回收站列表（按删除时间倒序 —— 最近删的排最前，用户最可能想还原它）。
pub fn list_trash(conn: &Connection) -> Result<Vec<Note>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {NOTE_COLS} FROM notes WHERE deleted_at IS NOT NULL
         ORDER BY deleted_at DESC, id DESC"
    ))?;
    let rows = stmt.query_map([], row_to_note)?;
    rows.collect()
}

/// 软删：只打标记，不动正文。
pub fn trash(conn: &Connection, id: i64) -> Result<Note> {
    let affected = conn.execute(
        "UPDATE notes SET deleted_at = ?1 WHERE id = ?2 AND deleted_at IS NULL",
        params![now(), id],
    )?;
    if affected == 0 {
        // 两种可能：笔记不存在，或已经在回收站里。都要报，但文案不同 ——
        // 「不存在」是用户点错了，「已在回收站」是重复操作，后者不该报错。
        let exists: bool = conn.query_row(
            "SELECT 1 FROM notes WHERE id = ?1",
            params![id],
            |r| r.get::<_, i32>(0).map(|n| n > 0),
        )?;
        if !exists {
            return Err(rusqlite::Error::InvalidParameterName(format!(
                "NOT_FOUND: 笔记 {id} 不存在"
            )));
        }
        return get(conn, id);
    }
    get(conn, id)
}

/// 从回收站还原。
pub fn restore(conn: &Connection, id: i64) -> Result<Note> {
    let affected = conn.execute(
        "UPDATE notes SET deleted_at = NULL WHERE id = ?1 AND deleted_at IS NOT NULL",
        params![id],
    )?;
    if affected == 0 {
        let exists: bool = conn.query_row(
            "SELECT 1 FROM notes WHERE id = ?1",
            params![id],
            |r| r.get::<_, i32>(0).map(|n| n > 0),
        )?;
        if !exists {
            return Err(rusqlite::Error::InvalidParameterName(format!(
                "NOT_FOUND: 笔记 {id} 不存在"
            )));
        }
        return Err(rusqlite::Error::InvalidParameterName(format!(
            "笔记 {id} 不在回收站里"
        )));
    }
    get(conn, id)
}

/// 彻底删除单条（回收站里「删除」按钮）。**不可逆**，前端必须二次确认。
pub fn purge(conn: &Connection, id: i64) -> Result<()> {
    conn.execute("DELETE FROM notes WHERE id = ?1", params![id])?;
    Ok(())
}

/// 清空回收站。**不可逆**，前端必须二次确认。
///
/// ⚠️ 只删 `deleted_at IS NOT NULL` 的 —— 没有这个条件的话，一个拼写错误
///    （比如将来有人把 `IS NOT NULL` 写成 `IS NULL`）会删掉**全部笔记**。
pub fn empty_trash(conn: &Connection) -> Result<usize> {
    let n = conn.execute("DELETE FROM notes WHERE deleted_at IS NOT NULL", [])?;
    Ok(n)
}

/// 清理超过保留天数的回收站条目，返回**删掉的条数**。
///
/// `keep_days <= 0` = 永久保留，直接返回 0。这一条不能写成「按 0 天算」——
/// 那会在每次启动时把整个回收站清空（`now() - 0 天` 的上界正好是现在）。
///
/// ⚠️ 两个条件缺一不可，且都必须是 `deleted_at IS NOT NULL`：
///    少了它就会按时间删掉**正常笔记**。与 `empty_trash` 同一条防线。
pub fn purge_expired(conn: &Connection, keep_days: i64) -> Result<usize> {
    if keep_days <= 0 {
        return Ok(0);
    }
    let cutoff = chrono::Utc::now() - chrono::Duration::days(keep_days);
    let cutoff = cutoff.format("%Y-%m-%d %H:%M:%S%.6f").to_string();
    let n = conn.execute(
        "DELETE FROM notes
         WHERE deleted_at IS NOT NULL AND deleted_at < ?1",
        params![cutoff],
    )?;
    Ok(n)
}

/// 设置专属小图标。空串 = 恢复默认图标。
pub fn set_icon(conn: &Connection, id: i64, icon: &str) -> Result<Note> {
    let affected = conn.execute(
        "UPDATE notes SET icon = ?1, updated_at = ?2 WHERE id = ?3",
        params![icon, now(), id],
    )?;
    if affected == 0 {
        return Err(rusqlite::Error::InvalidParameterName(format!(
            "NOT_FOUND: 笔记 {id} 不存在"
        )));
    }
    get(conn, id)
}

/// 移动笔记到某个文件夹。`None` = 未归类。
pub fn move_to_folder(conn: &Connection, id: i64, folder_id: Option<i64>) -> Result<Note> {
    if let Some(fid) = folder_id {
        // ⚠️ 必须校验目标存在。不校验的话，一个过期的 folder_id（文件夹刚被删）
        //   会让笔记「消失」—— 它还在库里，但所有列表都按 folder_id 过滤，
        //   于是用户看到的是「笔记不见了」，而它其实在一个不存在的文件夹里。
        let ok: bool = conn.query_row(
            "SELECT 1 FROM note_folders WHERE id = ?1",
            params![fid],
            |r| r.get::<_, i32>(0).map(|n| n > 0),
        )?;
        if !ok {
            return Err(rusqlite::Error::InvalidParameterName(format!(
                "NOT_FOUND: 文件夹 {fid} 不存在"
            )));
        }
    }
    let affected = conn.execute(
        "UPDATE notes SET folder_id = ?1, updated_at = ?2 WHERE id = ?3",
        params![folder_id, now(), id],
    )?;
    if affected == 0 {
        return Err(rusqlite::Error::InvalidParameterName(format!(
            "NOT_FOUND: 笔记 {id} 不存在"
        )));
    }
    get(conn, id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::init_in_memory;

    #[test]
    fn create_and_get_note() {
        let conn = init_in_memory().unwrap();
        let n = create(&conn, "待办事项").unwrap();
        assert_eq!(n.title, "待办事项");
        assert_eq!(n.content, "");
    }

    #[test]
    fn list_notes_ordered_by_updated_desc() {
        let conn = init_in_memory().unwrap();
        let a = create(&conn, "A").unwrap();
        let b = create(&conn, "B").unwrap();
        update(&conn, a.id, "A", "updated later").unwrap();
        let list = list(&conn).unwrap();
        assert_eq!(list.iter().map(|n| n.id).collect::<Vec<_>>(), vec![a.id, b.id]);
    }

    #[test]
    fn update_note_title_and_content() {
        let conn = init_in_memory().unwrap();
        let n = create(&conn, "T").unwrap();
        let updated = update(&conn, n.id, "新标题", "这是内容").unwrap();
        assert_eq!(updated.title, "新标题");
        assert_eq!(updated.content, "这是内容");
    }

    #[test]
    fn delete_note() {
        let conn = init_in_memory().unwrap();
        let n = create(&conn, "T").unwrap();
        delete(&conn, n.id).unwrap();
        assert!(get(&conn, n.id).is_err());
    }

    #[test]
    fn search_notes_by_title_and_content() {
        let conn = init_in_memory().unwrap();
        create(&conn, "购物清单").unwrap();
        let second = create(&conn, "会议记录").unwrap();
        update(&conn, second.id, "会议记录", "讨论了发布计划").unwrap();
        let by_title = search(&conn, "购物").unwrap();
        assert_eq!(by_title.len(), 1);
        let by_content = search(&conn, "发布计划").unwrap();
        assert_eq!(by_content.len(), 1);
        assert_eq!(by_content[0].id, second.id);
    }

    /// 零宽字符夹在词中间时，`LIKE '%词%'` 命不中（`LIKE` 不认这些码位）。
    /// 这是「隐形字符混进搜索」最常见的形态：正文是用户原样存的，
    /// 而用户记得的词里没有那些字符。
    #[test]
    fn search_finds_through_zero_width_characters() {
        let conn = init_in_memory().unwrap();
        let n = create(&conn, "会议记录").unwrap();
        //             ZWSP          ← 网页「防断词」零宽空格
        update(&conn, n.id, "会议记录", "讨论了发\u{200B}布计划").unwrap();
        let hits = search(&conn, "发布计划").unwrap();
        assert_eq!(hits.len(), 1, "正文含零宽空格时仍应搜得到");
        assert_eq!(hits[0].id, n.id);
    }

    #[test]
    fn search_finds_through_soft_hyphen() {
        let conn = init_in_memory().unwrap();
        let n = create(&conn, "resume").unwrap();
        update(&conn, n.id, "简历", "re\u{00AD}sume 的写法").unwrap();
        assert_eq!(search(&conn, "resume").unwrap().len(), 1);
    }

    #[test]
    fn search_finds_through_nbsp() {
        let conn = init_in_memory().unwrap();
        let n = create(&conn, "T").unwrap();
        update(&conn, n.id, "T", "中文\u{00A0}英文").unwrap();
        assert_eq!(search(&conn, "中文 英文").unwrap().len(), 1);
    }

    /// ⚠️ ZWJ（`U+200D`）刻意不清：清掉 emoji 就碎了。
    /// 这条锁住那个取舍 —— 有人「顺手把零宽类都清掉」时会红。
    #[test]
    fn search_keeps_zwj_so_emoji_survive() {
        let family = "👨\u{200D}👩\u{200D}👧";
        assert_eq!(normalize_keyword(family), family);
        // 组合 emoji 作为一个整体搜不到是正常的（LIKE 不做字素簇匹配），
        // 这里只守「关键词原样透传、ZWJ 没被吃掉」。
        assert!(normalize_keyword(&format!("看{family}")).contains('\u{200D}'));
    }

    /// ⚠️ 关键词全由不可见字符组成时**不能**返回全部笔记。
    /// 归一化后是空串 → 若继续走 `like_pattern("")` 会得到 `%%`，
    /// 那匹配**每一行** —— 症状是「搜一个空格，笔记库全出来了」。
    #[test]
    fn all_invisible_keyword_returns_empty_not_everything() {
        let conn = init_in_memory().unwrap();
        create(&conn, "笔记甲").unwrap();
        create(&conn, "笔记乙").unwrap();
        assert!(search(&conn, "\u{200B}\u{00AD}\u{FEFF}").unwrap().is_empty());
        assert!(search(&conn, "\u{00A0}").unwrap().is_empty());
    }

    /// `LIKE` 通配符必须按字面量处理：搜 `100%` 不该变成「100 开头、任意结尾」。
    #[test]
    fn like_wildcards_in_keyword_are_literal() {
        let conn = init_in_memory().unwrap();
        let a = create(&conn, "完成度 100%").unwrap();
        create(&conn, "完成度 100 分").unwrap();
        let hits = search(&conn, "100%").unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].id, a.id);
    }

    #[test]
    fn like_underscore_in_keyword_is_literal() {
        let conn = init_in_memory().unwrap();
        let a = create(&conn, "snake_case 命名").unwrap();
        create(&conn, "snakeXcase 命名").unwrap();
        let hits = search(&conn, "snake_case").unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].id, a.id);
    }

    /// 转义符自身也必须能被搜到（`\` → `\\`）。
    #[test]
    fn like_escape_char_backslash_is_searchable() {
        let conn = init_in_memory().unwrap();
        let a = create(&conn, "路径 C:\\temp").unwrap();
        let hits = search(&conn, "\\temp").unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].id, a.id);
    }

    /// ⚠️ 正文里的零宽字符**不被改写** —— 搜索是读取侧的事，不动用户数据。
    /// 这条若不锁，「顺手在 `update` 里归一化正文」的实现会让它变红，
    /// 而那种实现会让用户导出/复制原文时拿到被改过的内容。
    #[test]
    fn search_does_not_rewrite_stored_content() {
        let conn = init_in_memory().unwrap();
        let n = create(&conn, "T").unwrap();
        let original = "发\u{200B}布\u{00AD}计划";
        update(&conn, n.id, "T", original).unwrap();
        search(&conn, "发布计划").unwrap();
        assert_eq!(get(&conn, n.id).unwrap().content, original);
    }

    /// ⚠️ 上面三条测的是**兜底段**（全表扫 + 归一化比对），而第 1 段的 `LIKE`
    /// 是否真的转义了通配符，它们测不到 —— 第 2 段会把第 1 段的失败兜回来，
    /// 去掉 `ESCAPE '\'` 后测试照样全绿（实测）。
    /// 所以 `search_like` 必须**单独**测：它是第 1 段的真实生产路径。
    #[test]
    fn search_like_escapes_wildcards() {
        let conn = init_in_memory().unwrap();
        let a = create(&conn, "完成度 100%").unwrap();
        create(&conn, "完成度 100 分").unwrap();
        // ⚠️ 绕开 `search`：直接走第 1 段。少写 ESCAPE 时这里是 0 条。
        assert_eq!(search_like(&conn, &like_pattern("100%")).unwrap().len(), 1);
        assert_eq!(search_like(&conn, &like_pattern("100%")).unwrap()[0].id, a.id);
    }

    #[test]
    fn search_like_escapes_underscore() {
        let conn = init_in_memory().unwrap();
        let a = create(&conn, "snake_case 命名").unwrap();
        create(&conn, "snakeXcase 命名").unwrap();
        assert_eq!(search_like(&conn, &like_pattern("snake_case")).unwrap().len(), 1);
        assert_eq!(search_like(&conn, &like_pattern("snake_case")).unwrap()[0].id, a.id);
    }

    /// ⚠️ `ESCAPE` 必须**两个谓词都写**。只给 title 写的话，
    /// 搜正文里的 `%` 就不命中 —— 而这正是 `search_like` 里最容易漏的一半。
    #[test]
    fn search_like_escape_applies_to_content_too() {
        let conn = init_in_memory().unwrap();
        let a = create(&conn, "标题无百分号").unwrap();
        update(&conn, a.id, "标题无百分号", "正文里有 100% 完成").unwrap();
        let by_content = search_like(&conn, &like_pattern("100%")).unwrap();
        assert_eq!(by_content.len(), 1, "content 谓词也要转义");
        assert_eq!(by_content[0].id, a.id);
    }

    #[test]
    fn search_like_finds_backslash() {
        let conn = init_in_memory().unwrap();
        let a = create(&conn, "路径 C:\\temp").unwrap();
        assert_eq!(search_like(&conn, &like_pattern("\\temp")).unwrap().len(), 1);
        assert_eq!(search_like(&conn, &like_pattern("\\temp")).unwrap()[0].id, a.id);
    }

    /* ── 回收站（发布说明 ③）─────────────────────────────────────── */

    fn mk(conn: &Connection, title: &str) -> i64 {
        conn.execute(
            "INSERT INTO notes (title) VALUES (?1)",
            params![title],
        )
        .unwrap();
        conn.last_insert_rowid()
    }

    #[test]
    fn trash_is_soft_delete_and_restore_brings_it_back() {
        let conn = init_in_memory().unwrap();
        let n = mk(&conn, "待删");
        trash(&conn, n).unwrap();
        // 软删：行还在，只是打了标记
        assert!(get(&conn, n).is_ok(), "软删不该让 get 失败");
        assert!(get(&conn, n).unwrap().deleted_at.is_some());
        // 主列表里看不到
        assert!(list(&conn).unwrap().is_empty(), "回收站里的笔记不该进主列表");
        assert_eq!(list_trash(&conn).unwrap().len(), 1);
        // 还原
        restore(&conn, n).unwrap();
        assert!(get(&conn, n).unwrap().deleted_at.is_none());
        assert_eq!(list(&conn).unwrap().len(), 1);
    }

    #[test]
    fn trash_twice_is_idempotent_not_an_error() {
        let conn = init_in_memory().unwrap();
        let n = mk(&conn, "待删");
        trash(&conn, n).unwrap();
        // 重复删不该报错 —— 那是重复操作，不是错误
        trash(&conn, n).unwrap();
        assert_eq!(list_trash(&conn).unwrap().len(), 1);
    }

    #[test]
    fn restore_something_not_in_trash_is_an_error() {
        let conn = init_in_memory().unwrap();
        let n = mk(&conn, "正常");
        assert!(restore(&conn, n).is_err(), "不在回收站的不能还原");
    }

    #[test]
    fn purge_is_hard_delete() {
        let conn = init_in_memory().unwrap();
        let n = mk(&conn, "待删");
        trash(&conn, n).unwrap();
        purge(&conn, n).unwrap();
        assert!(get(&conn, n).is_err(), "彻底删除后 get 必须失败");
    }

    #[test]
    fn empty_trash_only_removes_trashed() {
        let conn = init_in_memory().unwrap();
        let a = mk(&conn, "要删");
        let b = mk(&conn, "留着");
        trash(&conn, a).unwrap();
        let n = empty_trash(&conn).unwrap();
        assert_eq!(n, 1, "只删回收站里的");
        assert!(get(&conn, a).is_err());
        assert!(get(&conn, b).is_ok(), "未删除的不该被清掉");
    }

    #[test]
    fn purge_expired_respects_keep_days_and_spares_live_notes() {
        let conn = init_in_memory().unwrap();
        let old = mk(&conn, "两个月前删的");
        let fresh = mk(&conn, "昨天删的");
        let live = mk(&conn, "从没删过");
        trash(&conn, old).unwrap();
        trash(&conn, fresh).unwrap();
        // 把 old 的删除时间倒推 60 天 —— 用真实时钟写测试只会得到不确定的结论
        conn.execute(
            "UPDATE notes SET deleted_at = ?1 WHERE id = ?2",
            params![
                (chrono::Utc::now() - chrono::Duration::days(60))
                    .format("%Y-%m-%d %H:%M:%S%.6f")
                    .to_string(),
                old
            ],
        )
        .unwrap();

        let n = purge_expired(&conn, 30).unwrap();
        assert_eq!(n, 1, "只清掉超过 30 天的那一条");
        assert!(get(&conn, old).is_err(), "过期的该清");
        assert!(get(&conn, fresh).is_ok(), "还在保留期内的不该动");
        assert!(get(&conn, live).is_ok(), "⚠️ 未删除的必须原样留住");

        // 0 = 永久保留：既不删过期的，也不该退化成「按 0 天清理」
        let conn = init_in_memory().unwrap();
        let a = mk(&conn, "甲");
        trash(&conn, a).unwrap();
        assert_eq!(purge_expired(&conn, 0).unwrap(), 0, "0 天 = 一条都不清");
        assert_eq!(purge_expired(&conn, -1).unwrap(), 0, "负数同 0");
        assert!(get(&conn, a).is_ok());
    }

    /* ── 文件夹 / 图标（发布说明 ①⑨）───────────────────────────── */

    #[test]
    fn move_to_folder_and_back() {
        let conn = init_in_memory().unwrap();
        let n = mk(&conn, "笔记");
        let f = conn.execute(
            "INSERT INTO note_folders (name) VALUES ('工作')",
            [],
        ).unwrap();
        let fid = conn.last_insert_rowid();
        assert_eq!(f, 1);
        let moved = move_to_folder(&conn, n, Some(fid)).unwrap();
        assert_eq!(moved.folder_id, Some(fid));
        let back = move_to_folder(&conn, n, None).unwrap();
        assert_eq!(back.folder_id, None, "None = 未归类");
    }

    #[test]
    fn move_to_nonexistent_folder_is_rejected() {
        let conn = init_in_memory().unwrap();
        let n = mk(&conn, "笔记");
        // ⚠️ 不校验的话，一个过期的 folder_id 会让笔记「消失」—— 它还在库里，
        //   但所有列表都按 folder_id 过滤，用户看到的是「笔记不见了」。
        assert!(move_to_folder(&conn, n, Some(9999)).is_err());
        assert_eq!(get(&conn, n).unwrap().folder_id, None, "失败的移动不该留下痕迹");
    }

    #[test]
    fn list_by_folder_filters_and_excludes_trash() {
        let conn = init_in_memory().unwrap();
        let a = mk(&conn, "甲");
        let b = mk(&conn, "乙");
        let c = mk(&conn, "丙");
        conn.execute("INSERT INTO note_folders (name) VALUES ('工作')", []).unwrap();
        let fid = conn.last_insert_rowid();
        move_to_folder(&conn, a, Some(fid)).unwrap();
        move_to_folder(&conn, b, Some(fid)).unwrap();
        trash(&conn, c).unwrap();

        assert_eq!(list_by_folder(&conn, None).unwrap().len(), 2, "全部未删除");
        assert_eq!(list_by_folder(&conn, Some(&[fid])).unwrap().len(), 2, "工作文件夹");
        assert_eq!(list_by_folder(&conn, Some(&[])).unwrap().len(), 0, "未归类为空");
    }

    /// ⚠️ 「未归类」= `folder_id IS NULL`，**不是** `folder_id = 0`。
    ///
    /// 我第一版只测了「未归类为空列表」—— 而那时恰好一条未归类笔记都没有，
    /// 于是把 `IS NULL` 写成 `= 0` 结果完全相同，这条断言守不住它守的东西
    /// （实测）。必须有一条**真的未归类**的笔记才能区分。
    #[test]
    fn list_by_folder_unfiled_is_null_not_zero() {
        let conn = init_in_memory().unwrap();
        let unfiled = mk(&conn, "未归类的");
        conn.execute("INSERT INTO note_folders (name) VALUES ('工作')", []).unwrap();
        let fid = conn.last_insert_rowid();
        let filed = mk(&conn, "已归类的");
        move_to_folder(&conn, filed, Some(fid)).unwrap();

        let unfiled_hits = list_by_folder(&conn, Some(&[])).unwrap();
        assert_eq!(unfiled_hits.len(), 1, "未归类列表要有那一条");
        assert_eq!(unfiled_hits[0].id, unfiled);
        // 反向：选了「工作」不该把未归类的带出来
        let filed_hits = list_by_folder(&conn, Some(&[fid])).unwrap();
        assert!(filed_hits.iter().all(|n| n.id != unfiled));
    }

    #[test]
    fn set_icon_and_default() {
        let conn = init_in_memory().unwrap();
        let n = mk(&conn, "笔记");
        assert_eq!(get(&conn, n).unwrap().icon, "", "默认空串");
        set_icon(&conn, n, "📌").unwrap();
        assert_eq!(get(&conn, n).unwrap().icon, "📌");
        set_icon(&conn, n, "").unwrap();
        assert_eq!(get(&conn, n).unwrap().icon, "", "空串 = 恢复默认");
    }

    #[test]
    fn new_note_defaults_are_sane() {
        let conn = init_in_memory().unwrap();
        let n = mk(&conn, "笔记");
        let note = get(&conn, n).unwrap();
        assert_eq!(note.folder_id, None, "新笔记未归类");
        assert_eq!(note.deleted_at, None, "新笔记未删除");
        assert_eq!(note.icon, "");
    }

    #[test]
    fn normalize_keyword_classifies_by_visible_width() {
        // ① 有可见宽度 → 换成普通空格
        assert_eq!(normalize_keyword("a\u{00A0}b"), "a b");
        assert_eq!(normalize_keyword("a\u{3000}b"), "a b");
        // ② 不可见 → 删掉（**不是**换空格，否则 `discuss\u{200B}ion` 被劈成两个词）
        assert_eq!(normalize_keyword("a\u{200B}b"), "ab");
        assert_eq!(normalize_keyword("discuss\u{200B}ion"), "discussion");
        // ③ 软连字符 → 删掉
        assert_eq!(normalize_keyword("re\u{00AD}sume"), "resume");
        // 无可清理字符时原样返回（这条让 `search` 的「两段相同就不必重跑」成立）
        assert_eq!(normalize_keyword("普通词"), "普通词");
    }
}
