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
        "SELECT id, title, content, created_at, updated_at FROM notes WHERE id = ?1",
        params![id],
        row_to_note,
    )
}

pub fn list(conn: &Connection) -> Result<Vec<Note>> {
    let mut stmt = conn.prepare(
        "SELECT id, title, content, created_at, updated_at FROM notes ORDER BY updated_at DESC, id DESC",
    )?;
    let rows = stmt.query_map([], row_to_note)?;
    rows.collect()
}

/// 笔记列表（仅元信息，不拉 content）：用于外部保存速记后主窗口刷新列表，
/// 避免每次刷新都全量读取正文，数据量大时省内存省 IO。
pub fn list_meta(conn: &Connection) -> Result<Vec<Note>> {
    let mut stmt = conn.prepare(
        "SELECT id, title, '', created_at, updated_at FROM notes ORDER BY updated_at DESC, id DESC",
    )?;
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
        "SELECT id, title, content, created_at, updated_at FROM notes \
         ORDER BY updated_at DESC, id DESC",
    )?;
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
        "SELECT id, title, content, created_at, updated_at FROM notes \
         WHERE title LIKE ?1 ESCAPE '\\' OR content LIKE ?1 ESCAPE '\\' \
         ORDER BY updated_at DESC, id DESC",
    )?;
    let rows = stmt.query_map(params![pattern], row_to_note)?;
    rows.collect()
}

pub fn row_to_note(row: &rusqlite::Row) -> Result<Note> {
    Ok(Note {
        id: row.get(0)?,
        title: row.get(1)?,
        content: row.get(2)?,
        created_at: row.get(3)?,
        updated_at: row.get(4)?,
    })
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
