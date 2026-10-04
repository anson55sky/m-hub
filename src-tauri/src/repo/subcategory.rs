//! 速达小类（ADR 0012）：大类（resources.kind）下单归属的小类库，每条资源恰好归属
//! 一个小类（单归属），不是多选标签。`resources.category` 语义升级为「所属大类小类库中
//! 的一个小类名」——文件大类沿用原 7 个内置值作为初始小类（建表种子见 db.rs），
//! 应用/网页初始为空、按需新建；各大类一套、允许同名不同义（UNIQUE(kind, name)）。
//! 存量条目不回填：category 为 NULL 即「未归类」，与默认小类区分。
//!
//! ## 层级（2026-10-04）：`name` 存**全路径**，段间用 `/` 分隔
//!
//! 「速达的小类支持层级了」这条需求**没有动表结构**，全部落在 `name` 的语义上：
//! `开发` 是顶层，`开发/前端` 是它下面的一级，`开发/前端/Vue` 再下一级。
//!
//! ### 为什么是路径而不是 `parent_id`
//!
//! ① **零迁移**：老数据的小类名里没有 `/`，天然就是顶层。新增一列 `parent_id`
//!    还得回填，而「回填错了」表现为条目挂到不存在的父级下——比不改更糟。
//! ② **不用改 `resources.category`**：它继续存全路径，筛��、统计、扩展桥
//!    （`data.resources.create`）全都不必改语义。这一列如果改成存 id，
//!    十几处读它的地方都要跟着改，且改错任何一处都是「条目从所有视图消失」。
//! ③ **`UNIQUE(kind, name)` 继续有效**：唯一性落在全路径上，本来就是我们要的
//!    （同一父级下不能重名，不同父级下可以重名 —— 路径不同即可）。
//!
//! ### 代价与边界
//!
//! · 一个层级段里**不能含 `/`**（`validate_segment` 拦掉）。否则「A/B」既可能是
//!   「A 下的 B」也可能是「名叫 A/B 的顶层项」，路径本身就歧义了。
//! · 改名只改**自己那一段**，后代整条路径跟着改（`rename` 级联）。
//! · 删除**连子树一起删**，子树里的条目改挂大类默认小类（`delete`）。

use crate::models::{ResourceSubcategory, SubcategoryNode};
use rusqlite::{params, Connection};

use super::now;

pub const VALID_KINDS: [&str; 3] = ["app", "web", "file"];

fn row_to_sub(row: &rusqlite::Row) -> rusqlite::Result<ResourceSubcategory> {
    Ok(ResourceSubcategory {
        id: row.get(0)?,
        kind: row.get(1)?,
        name: row.get(2)?,
        sort_order: row.get(3)?,
        is_default: row.get::<_, i64>(4)? != 0,
    })
}

const COLS: &str = "id, kind, name, sort_order, is_default";

pub fn list(conn: &Connection) -> rusqlite::Result<Vec<ResourceSubcategory>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {COLS} FROM resource_subcategories ORDER BY kind ASC, sort_order ASC, id ASC"
    ))?;
    let rows = stmt.query_map([], row_to_sub)?;
    rows.collect()
}

/// 大类的小类名是否可用（同大类内唯一；rename 时排除自身）
pub fn is_name_free(
    conn: &Connection,
    kind: &str,
    name: &str,
    exclude_id: Option<i64>,
) -> rusqlite::Result<bool> {
    let n: i64 = conn.query_row(
        "SELECT COUNT(*) FROM resource_subcategories WHERE kind = ?1 AND name = ?2 AND id != ?3",
        params![kind, name, exclude_id.unwrap_or(-1)],
        |r| r.get(0),
    )?;
    Ok(n == 0)
}

pub fn create(conn: &Connection, kind: &str, name: &str) -> rusqlite::Result<ResourceSubcategory> {
    // 大类的第一个小类自动成为默认小类（用户随后可在设置里改）
    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM resource_subcategories WHERE kind = ?1",
        params![kind],
        |r| r.get(0),
    )?;
    let is_default: i64 = if count == 0 { 1 } else { 0 };
    let max: i64 = conn.query_row(
        "SELECT COALESCE(MAX(sort_order), -1) FROM resource_subcategories WHERE kind = ?1",
        params![kind],
        |r| r.get(0),
    )?;
    conn.execute(
        "INSERT INTO resource_subcategories (kind, name, sort_order, is_default) VALUES (?1, ?2, ?3, ?4)",
        params![kind, name, max + 1, is_default],
    )?;
    Ok(ResourceSubcategory {
        id: conn.last_insert_rowid(),
        kind: kind.to_string(),
        name: name.to_string(),
        sort_order: max + 1,
        is_default: is_default != 0,
    })
}

/// 改名：**只改自己那一段**，后代整条路径跟着改（单事务，资源层无感知）。
///
/// ⚠️ 老实现是「把 `resources.category = old` 全改成 `new`」—— 层级下这是错的：
///   改「开发」→「研发」时，「开发/前端」必须变成「研发/前端」而不是留在原地，
///   否则它在树上就成了「开发」那个**已经不存在的**节点的子级，界面上会显示成
///   一个找不到父亲的缩进行。
pub fn rename(conn: &mut Connection, id: i64, seg: &str) -> Result<(), String> {
    validate_segment(seg)?;
    let tx = conn.transaction().map_err(|e| e.to_string())?;
    let (kind, old_path): (String, String) = tx
        .query_row(
            "SELECT kind, name FROM resource_subcategories WHERE id = ?1",
            params![id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .map_err(|_| format!("NOT_FOUND: 小类 {id} 不存在"))?;

    let parent = parent_path(&old_path);
    let new_path = join_path(parent, seg);
    if new_path == old_path {
        return Ok(());
    }
    let free = is_name_free(&tx, &kind, &new_path, Some(id)).map_err(|e| e.to_string())?;
    if !free {
        return Err(format!("DUP: 该层级下已有名为「{seg}」的小类"));
    }

    let subtree: Vec<(i64, String)> = {
        let mut stmt = tx
            .prepare("SELECT id, name FROM resource_subcategories WHERE kind = ?1")
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map(params![kind], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))
            .map_err(|e| e.to_string())?;
        let mut out: Vec<(i64, String)> = Vec::new();
        for row in rows {
            let (sid, name) = row.map_err(|e| e.to_string())?;
            if is_within(&name, &old_path) {
                out.push((sid, name));
            }
        }
        out
    };
    // ⚠️ 这里**不需要**「自己先改」的排序。第一版写过，变异验证时删掉它测试
    //   全绿 —— 查下来原因是  已经把「新路径已被占用」挡在门外，
    //   于是任何一行 UPDATE 的中间态都不会撞 UNIQUE(kind, name)。
    //   留着它只会让人以为这里有个要靠顺序躲开的坑（约定 75 的推论：
    //   死代码比没有代码更贵）。
    for (sid, name) in &subtree {
        let np = reparent(&old_path, &new_path, name)
            .ok_or_else(|| format!("INTERNAL: 路径 {name} 不在 {old_path} 之下"))?;
        tx.execute(
            "UPDATE resource_subcategories SET name = ?1 WHERE id = ?2",
            params![np, sid],
        )
        .map_err(|e| e.to_string())?;
        // 资源条目按同样的映射改挂
        tx.execute(
            "UPDATE resources SET category = ?1, updated_at = ?2 WHERE kind = ?3 AND category = ?4",
            params![np, now(), kind, name],
        )
        .map_err(|e| e.to_string())?;
    }
    tx.commit().map_err(|e| e.to_string())
}

/// 删除小类：**连整棵子树一起删**，子树里的条目批量改挂大类默认小类
/// （该大类一个小类不剩时回「未归类」NULL）；若删的是默认小类，把排序最前的
/// 剩余小类提为默认。单事务完成。
///
/// ⚠️ 删子树而不是只删自己：留着孤儿节点的话，界面上会出现一行缩进很深、
///   父亲却不存在的小类，点它只能看到空列表，而用户没有任何办法把它收拾掉
///   （改不了名字、拖不动、删不到）。
pub fn delete(conn: &mut Connection, id: i64) -> Result<(), String> {
    let tx = conn.transaction().map_err(|e| e.to_string())?;
    let (kind, name, was_default): (String, String, i64) = tx
        .query_row(
            "SELECT kind, name, is_default FROM resource_subcategories WHERE id = ?1",
            params![id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .map_err(|_| format!("NOT_FOUND: 小类 {id} 不存在"))?;

    // 先算默认：⚠️ 必须在**删除之前**取 —— 删完之后再查就只剩别人了，
    //   那时挑出来的「新的默认」其实是「本来就排第二的那个」，而不是
    //   「因为默认被删了才该顶上来的那个」。两者在只剩一个小类时结果相同，
    //   在两个以上时就选错了。
    let new_default: Option<String> = tx
        .query_row(
            "SELECT name FROM resource_subcategories WHERE kind = ?1 AND is_default = 1 AND id != ?2 LIMIT 1",
            params![kind, id],
            |r| r.get::<_, String>(0),
        )
        .ok()
        .or_else(|| {
            tx.query_row(
                "SELECT name FROM resource_subcategories WHERE kind = ?1 AND id != ?2
                 ORDER BY sort_order ASC, id ASC LIMIT 1",
                params![kind, id],
                |r| r.get::<_, String>(0),
            )
            .ok()
        });

    // 子树里所有路径（自己 + 后代），逐条改挂后才删 —— 顺序反了就没法再按名匹配
    let subtree: Vec<String> = {
        let mut stmt = tx
            .prepare("SELECT name FROM resource_subcategories WHERE kind = ?1")
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map(params![kind], |r| r.get::<_, String>(0))
            .map_err(|e| e.to_string())?;
        let mut out: Vec<String> = Vec::new();
        for row in rows {
            let n = row.map_err(|e| e.to_string())?;
            if is_within(&n, &name) {
                out.push(n);
            }
        }
        out
    };

    match &new_default {
        Some(dn) => {
            for n in &subtree {
                tx.execute(
                    "UPDATE resources SET category = ?1, updated_at = ?2 WHERE kind = ?3 AND category = ?4",
                    params![dn, now(), kind, n],
                )
                .map_err(|e| e.to_string())?;
            }
        }
        None => {
            for n in &subtree {
                tx.execute(
                    "UPDATE resources SET category = NULL, updated_at = ?1 WHERE kind = ?2 AND category = ?3",
                    params![now(), kind, n],
                )
                .map_err(|e| e.to_string())?;
            }
        }
    }

    for n in &subtree {
        tx.execute("DELETE FROM resource_subcategories WHERE kind = ?1 AND name = ?2", params![kind, n])
            .map_err(|e| e.to_string())?;
    }

    if was_default != 0 {
        // ⚠️ 晋升成默认的必须是**剩余**里排序最前的那个，且不能是子树里的
        //   （子树已经删完了，这条查询天然只在剩余行里选）
        tx.execute(
            "UPDATE resource_subcategories SET is_default = 1 WHERE id = (
               SELECT id FROM resource_subcategories WHERE kind = ?1
               ORDER BY sort_order ASC, id ASC LIMIT 1)",
            params![kind],
        )
        .map_err(|e| e.to_string())?;
    }
    tx.commit().map_err(|e| e.to_string())
}

/// 组内拖拽排序：按传入 id 顺序写 sort_order
pub fn reorder(conn: &Connection, kind: &str, ids: &[i64]) -> rusqlite::Result<()> {
    // 整批排序必须是**一个**事务（2026-09-29 补）。原先逐条独立提交，
    // 拖拽排序写到一半失败（例如 ids 里有已被删掉的 id、或中途被取消）
    // 就留下「半新半旧」的顺序：用户看到的分类顺序是乱的，而且
    // `default_name()` 的兜底排序（ORDER BY is_default DESC, sort_order ASC）
    // 也跟着一起错，后续新建的资源会被挂到错误的小类上。
    // 同文件的 rename / delete 早就用了事务，只有这里漏了。
    let tx = conn.unchecked_transaction()?;
    for (idx, id) in ids.iter().enumerate() {
        tx.execute(
            "UPDATE resource_subcategories SET sort_order = ?1 WHERE id = ?2 AND kind = ?3",
            params![idx as i64, id, kind],
        )?;
    }
    tx.commit()
}

pub fn set_default(conn: &Connection, id: i64) -> Result<(), String> {
    let kind: String = conn
        .query_row(
            "SELECT kind FROM resource_subcategories WHERE id = ?1",
            params![id],
            |r| r.get(0),
        )
        .map_err(|_| format!("NOT_FOUND: 小类 {id} 不存在"))?;
    // 两条 UPDATE 必须同事务（2026-09-29 补）：它们表达的是「先清掉旧的默认、
    // 再把新的设为默认」这一个原子操作。分开提交时若第二条失败，该大类就
    // **一个默认小类都没有** —— 而 `default_name()` 靠 ORDER BY 兜底不会崩，
    // 于是用户看不到任何报错，只发现新建的资源全被挂到了「排序最前」那个小类。
    let tx = conn.unchecked_transaction().map_err(|e| e.to_string())?;
    tx.execute(
        "UPDATE resource_subcategories SET is_default = 0 WHERE kind = ?1",
        params![kind],
    )
    .map_err(|e| e.to_string())?;
    tx.execute(
        "UPDATE resource_subcategories SET is_default = 1 WHERE id = ?1",
        params![id],
    )
    .map_err(|e| e.to_string())?;
    tx.commit().map_err(|e| e.to_string())
}

/// 大类的默认小类名：is_default=1 的行优先，否则取排序最前的行；该大类还没有小类时 None。
/// 新建资源未指定小类时经此自动归入默认（ADR 0012 决策 3）。
pub fn default_name(conn: &Connection, kind: &str) -> rusqlite::Result<Option<String>> {
    match conn.query_row(
        "SELECT name FROM resource_subcategories WHERE kind = ?1
         ORDER BY is_default DESC, sort_order ASC, id ASC LIMIT 1",
        params![kind],
        |r| r.get::<_, String>(0),
    ) {
        Ok(n) => Ok(Some(n)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::init_in_memory;
    use crate::models::ResourceKind;
    use crate::repo::resource;

    fn kinds_eq(a: &ResourceSubcategory, kind: &str, name: &str) -> bool {
        a.kind == kind && a.name == name
    }

    #[test]
    fn seed_file_categories_and_default_other() {
        let conn = init_in_memory().unwrap();
        let subs = list(&conn).unwrap();
        assert_eq!(subs.len(), 7);
        assert!(subs.iter().all(|s| s.kind == "file"));
        let other = subs.iter().find(|s| s.name == "其他").unwrap();
        assert!(other.is_default);
        assert_eq!(default_name(&conn, "file").unwrap().as_deref(), Some("其他"));
        assert_eq!(default_name(&conn, "app").unwrap(), None);
    }

    #[test]
    fn create_auto_defaults_first_of_kind() {
        let conn = init_in_memory().unwrap();
        let s = create(&conn, "app", "开发工具").unwrap();
        assert!(s.is_default);
        let s2 = create(&conn, "app", "常用").unwrap();
        assert!(!s2.is_default);
        assert_eq!(default_name(&conn, "app").unwrap().as_deref(), Some("开发工具"));
    }

    #[test]
    fn create_resource_auto_assigns_default_subcategory() {
        let conn = init_in_memory().unwrap();
        // 应用大类还没有小类 → 保持「未归类」
        let r = resource::create(&conn, ResourceKind::App, "a", "t", None, None, None).unwrap();
        assert_eq!(r.category, None);
        // 建了小类之后 → 自动归默认
        create(&conn, "app", "开发工具").unwrap();
        let r2 = resource::create(&conn, ResourceKind::App, "b", "t", None, None, None).unwrap();
        assert_eq!(r2.category.as_deref(), Some("开发工具"));
        // 显式指定不受影响
        let r3 = resource::create(&conn, ResourceKind::App, "c", "t", Some("常用"), None, None).unwrap();
        assert_eq!(r3.category.as_deref(), Some("常用"));
    }

    #[test]
    fn rename_cascades_to_resources() {
        let conn = init_in_memory().unwrap();
        // 文件种子小类已存在，这里新建一个专属小类验证改名级联
        let sub = create(&conn, "file", "临时分类").unwrap();
        let r = resource::create(&conn, ResourceKind::File, "d", "t", Some("临时分类"), None, None).unwrap();
        let mut conn = conn;
        rename(&mut conn, sub.id, "资料").unwrap();
        let after = resource::get(&conn, r.id).unwrap();
        assert_eq!(after.category.as_deref(), Some("资料"));
        let names: Vec<String> = list(&conn).unwrap().into_iter().map(|s| s.name).collect();
        assert!(names.contains(&"资料".to_string()));
        assert!(!names.contains(&"临时分类".to_string()));
    }

    #[test]
    fn delete_reassigns_entries_to_default_and_promotes_successor() {
        let mut conn = init_in_memory().unwrap();
        let doc = create(&conn, "app", "文档类").unwrap(); // 第一个 → 默认
        let dev = create(&conn, "app", "开发工具").unwrap();
        let r_doc = resource::create(&conn, ResourceKind::App, "x", "t", Some("文档类"), None, None).unwrap();
        let r_dev = resource::create(&conn, ResourceKind::App, "y", "t", Some("开发工具"), None, None).unwrap();
        // 删默认小类 → 其条目改挂新默认（晋升的开发工具）
        delete(&mut conn, doc.id).unwrap();
        assert_eq!(
            resource::get(&conn, r_doc.id).unwrap().category.as_deref(),
            Some("开发工具")
        );
        assert_eq!(
            resource::get(&conn, r_dev.id).unwrap().category.as_deref(),
            Some("开发工具")
        );
        let subs = list(&conn).unwrap();
        let dev_sub = subs.iter().find(|s| kinds_eq(s, "app", "开发工具")).unwrap();
        assert!(dev_sub.is_default);
        // 删到只剩默认再删 → 条目回「未归类」
        delete(&mut conn, dev.id).unwrap();
        assert_eq!(resource::get(&conn, r_doc.id).unwrap().category, None);
        assert!(list(&conn).unwrap().iter().all(|s| s.kind != "app"));
    }

    #[test]
    fn set_default_and_reorder() {
        let conn = init_in_memory().unwrap();
        let a = create(&conn, "web", "收藏").unwrap();
        let b = create(&conn, "web", "工作").unwrap();
        set_default(&conn, b.id).unwrap();
        assert_eq!(default_name(&conn, "web").unwrap().as_deref(), Some("工作"));
        reorder(&conn, "web", &[b.id, a.id]).unwrap();
        let subs = list(&conn).unwrap();
        let web: Vec<&ResourceSubcategory> = subs.iter().filter(|s| s.kind == "web").collect();
        assert_eq!(web[0].name, "工作");
        assert_eq!(web[1].name, "收藏");
    }
}

// ==================== 层级：纯函数（可离线回归） ====================
//
// ⚠️ 这几个函数是**唯一的**路径规则出处。前端也有一份（拼筛选项、算缩进），
//   那份只做展示、且刻意不参与判定 —— 真不一致时以这里为准。
//   「同一规则写两遍」是这类改动的头号故障源，所以下面每条都配了负例。

/// 层级段之间的分隔符。选 `/` 是因为：用户写不进小类名（见 `validate_segment`），
/// 也不是文件系统字符，且比任何中文符号都更不容易和内容撞上。
pub const SEP: char = '/';

/// 段是否合法：非空、不超长、**不含分隔符**、不含首尾空白
pub fn validate_segment(seg: &str) -> Result<(), String> {
    let t = seg.trim();
    if t.is_empty() {
        return Err("小类名称不能为空".into());
    }
    if t.chars().count() > 20 {
        return Err("小类名称需为 1–20 个字符".into());
    }
    if t.contains(SEP) {
        // ⚠️ 必须拦：分隔符进了段名，「A/B」就有两种解释，路径本身歧义化，
        //   而歧义会在改名/删除时静默算错子树（删「开发」把「开发/前端」也删了，
        //   用户以为是两件独立的事）。
        return Err(format!("小类名称不能包含「{SEP}」（那是层级分隔符）"));
    }
    if t != seg {
        return Err("小类名称首尾不能有空格".into());
    }
    Ok(())
}

/// 拼子级全路径
pub fn join_path(parent: &str, seg: &str) -> String {
    if parent.is_empty() {
        seg.to_string()
    } else {
        format!("{parent}{SEP}{seg}")
    }
}

/// 全路径的最后一段（显示用）
pub fn leaf_of(path: &str) -> &str {
    path.rsplit(SEP).next().unwrap_or(path)
}

/// 全路径去掉最后一段（= 父路径）；顶层返回空串
pub fn parent_path(path: &str) -> &str {
    match path.rfind(SEP) {
        Some(i) => &path[..i],
        None => "",
    }
}

/// 层级深度：顶层 0
pub fn depth_of(path: &str) -> usize {
    if path.is_empty() {
        0
    } else {
        path.split(SEP).count() - 1
    }
}

/// `path` 是否在 `ancestor` 之下（**含自身**）。
///
/// ⚠️ 必须是**按段**比，不是字符串前缀比：`开发` 不是 `开发者` 的祖先，
///   而 `starts_with("开发")` 会说是 —— 于是「删掉『开发者』顺手带走了
///   『开发』下所有条目」，而界面上这两行看着毫无关系。
pub fn is_within(path: &str, ancestor: &str) -> bool {
    if ancestor.is_empty() {
        return true;
    }
    path == ancestor
        || (path.len() > ancestor.len()
            && path.starts_with(ancestor)
            && path.as_bytes()[ancestor.len()] == SEP as u8)
}

/// 把一批全路径改挂到新父路径下（改名 / 删除级联用）
pub fn reparent(from: &str, to: &str, path: &str) -> Option<String> {
    if path == from {
        return Some(to.to_string());
    }
    let rest = path.strip_prefix(from)?.strip_prefix(SEP)?;
    Some(join_path(to, rest))
}

/// 层级树的**唯一**构造处：按大类产出嵌套节点。
///
/// ⚠️ 前端也有一份「按 `/` 切路径拼树」的代码，但那份**只做展示**
///   （渲染缩进、算筛选项），判定一律以这里为准。两份不一致时改这里。
pub fn tree(conn: &Connection, kind: &str) -> rusqlite::Result<Vec<SubcategoryNode>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {COLS} FROM resource_subcategories WHERE kind = ?1
         ORDER BY sort_order ASC, id ASC"
    ))?;
    let rows = stmt.query_map(params![kind], row_to_sub)?;
    let subs: Vec<ResourceSubcategory> = rows.collect::<rusqlite::Result<_>>()?;

    let mut roots: Vec<SubcategoryNode> = Vec::new();
    // 自顶向下挂：每层先找到父节点再 push。
    //
    // ⚠️ 用 `sort_order` 升序遍历是有意的：父节点的 sort_order 一定早于
    //   它的孩子吗？**不一定** —— 老数据全是 sort_order 0..n，后来新建的子级
    //   拿到的 sort_order 更大，但用户可以把子级拖到父级之前（排序是全组的）。
    //   所以真正的保证来自「建子级时它的 sort_order 一定 ≥ 当前最大值」，
    //   而 delete 之后 `sort_order` 会有空洞 —— 空洞不影响单调性。
    for s in subs {
        let full_path = s.name.clone();
        let node = SubcategoryNode {
            id: s.id,
            name: leaf_of(&full_path).to_string(),
            depth: depth_of(&full_path),
            full_path: full_path.clone(),
            is_default: s.is_default,
            children: Vec::new(),
        };
        let parent = parent_path(&full_path).to_string();
        if parent.is_empty() {
            roots.push(node);
        } else {
            let target = find_mut(&mut roots, &parent).ok_or_else(|| {
                rusqlite::Error::InvalidParameterName(format!(
                    "小类「{full_path}」的父级「{parent}」不存在（孤儿节点）"
                ))
            })?;
            target.children.push(node);
        }
    }
    Ok(roots)
}

/// 在已建好的树里按**全路径**找节点（逐层下钻，故天然只命中真正的祖先）
fn find_mut<'a>(nodes: &'a mut [SubcategoryNode], path: &str) -> Option<&'a mut SubcategoryNode> {
    let (head, rest) = match path.find(SEP) {
        Some(i) => (&path[..i], &path[i + 1..]),
        None => (path, ""),
    };
    for n in nodes.iter_mut() {
        if n.full_path != path && n.name != head {
            continue;
        }
        return if rest.is_empty() {
            Some(n)
        } else {
            find_mut(&mut n.children, rest)
        };
    }
    None
}

#[cfg(test)]
mod hierarchy_tests {
    use super::*;
    use crate::db::init_in_memory;
    use crate::models::ResourceKind;
    use crate::repo::resource;

    // ---- 纯函数 ----

    #[test]
    fn path_helpers_roundtrip() {
        assert_eq!(join_path("", "开发"), "开发");
        assert_eq!(join_path("开发", "前端"), "开发/前端");
        assert_eq!(leaf_of("开发/前端"), "前端");
        assert_eq!(leaf_of("开发"), "开发");
        assert_eq!(parent_path("开发/前端"), "开发");
        assert_eq!(parent_path("开发/前端/Vue"), "开发/前端");
        assert_eq!(parent_path("开发"), "");
        assert_eq!(depth_of("开发"), 0);
        assert_eq!(depth_of("开发/前端"), 1);
        assert_eq!(depth_of("a/b/c"), 2);
    }

    /// ⚠️ 这是**整条层级逻辑里最容易错的一处**：`starts_with` 会把
    ///   「开发者」当成「开发」的子树，于是「删掉『开发者』」会连带
    ///   删掉「开发」下的全部条目 —— 界面上这两行看着毫无关系。
    #[test]
    fn is_within_compares_by_segment_not_by_prefix() {
        assert!(is_within("开发", "开发"));
        assert!(is_within("开发/前端", "开发"));
        assert!(is_within("开发/前端/Vue", "开发"));
        assert!(!is_within("开发者", "开发"), "「开发者」不是「开发」的子树");
        assert!(!is_within("开发x/前端", "开发"));
        assert!(!is_within("开发", "开发/前端"), "祖先不是后代");
        assert!(is_within("任意", ""), "空祖先 = 根，容纳一切");
    }

    #[test]
    fn reparent_moves_subtree_and_keeps_rest() {
        assert_eq!(reparent("开发", "研发", "开发"), Some("研发".into()));
        assert_eq!(reparent("开发", "研发", "开发/前端"), Some("研发/前端".into()));
        assert_eq!(reparent("开发", "研发", "开发/前端/Vue"), Some("研发/前端/Vue".into()));
        assert_eq!(reparent("开发", "研发", "开发者"), None, "不在其下则不动");
        assert_eq!(reparent("开发", "研发", "生活"), None);
    }

    #[test]
    fn validate_segment_rejects_the_separator() {
        assert!(validate_segment("开发").is_ok());
        assert!(validate_segment("a/b").is_err(), "段里不能有分隔符");
        assert!(validate_segment("").is_err());
        assert!(validate_segment("  ").is_err());
        assert!(validate_segment(" 开发").is_err(), "首尾空白");
        assert!(validate_segment(&"x".repeat(21)).is_err());
    }

    // ---- 真库 ----

    #[test]
    fn rename_moves_the_whole_subtree_not_just_itself() {
        let mut conn = init_in_memory().unwrap();
        let dev = create(&conn, "web", "开发").unwrap();
        create(&conn, "web", "开发/前端").unwrap();
        create(&conn, "web", "开发/前端/Vue").unwrap();
        create(&conn, "web", "开发者").unwrap();
        let r_child = resource::create(&conn, ResourceKind::Web, "c", "t", Some("开发/前端"), None, None).unwrap();
        let r_grand = resource::create(&conn, ResourceKind::Web, "g", "t", Some("开发/前端/Vue"), None, None).unwrap();
        let r_sibling = resource::create(&conn, ResourceKind::Web, "s", "t", Some("开发者"), None, None).unwrap();

        rename(&mut conn, dev.id, "研发").unwrap();

        let names: Vec<String> = list(&conn).unwrap().into_iter().map(|s| s.name).collect();
        assert!(names.contains(&"研发".to_string()));
        assert!(names.contains(&"研发/前端".to_string()), "子级应跟着换父");
        assert!(names.contains(&"研发/前端/Vue".to_string()), "孙级也应跟着换");
        assert!(names.contains(&"开发者".to_string()), "只是前缀相同，不该被动");
        assert_eq!(resource::get(&conn, r_child.id).unwrap().category.as_deref(), Some("研发/前端"));
        assert_eq!(resource::get(&conn, r_grand.id).unwrap().category.as_deref(), Some("研发/前端/Vue"));
        assert_eq!(resource::get(&conn, r_sibling.id).unwrap().category.as_deref(), Some("开发者"));
    }

    #[test]
    fn delete_removes_subtree_and_reassigns_all_its_entries() {
        let mut conn = init_in_memory().unwrap();
        let keep = create(&conn, "web", "收藏").unwrap(); // 第一个 → 默认
        let dev = create(&conn, "web", "开发").unwrap();
        create(&conn, "web", "开发/前端").unwrap();
        let r_top = resource::create(&conn, ResourceKind::Web, "t", "t", Some("开发"), None, None).unwrap();
        let r_child = resource::create(&conn, ResourceKind::Web, "c", "t", Some("开发/前端"), None, None).unwrap();
        let r_keep = resource::create(&conn, ResourceKind::Web, "k", "t", Some("收藏"), None, None).unwrap();

        delete(&mut conn, dev.id).unwrap();

        // ⚠️ `list` 返回**所有大类**，文件大类的 7 个种子也在里面 ——
        //   断言必须按 kind 过滤，否则它测的是「文件种子还在」而不是「子树没了」
        let names: Vec<String> = list(&conn)
            .unwrap()
            .into_iter()
            .filter(|s| s.kind == "web")
            .map(|s| s.name)
            .collect();
        assert_eq!(names, vec!["收藏".to_string()], "子树应连根带叶一起消失");
        assert_eq!(resource::get(&conn, r_top.id).unwrap().category.as_deref(), Some("收藏"));
        assert_eq!(resource::get(&conn, r_child.id).unwrap().category.as_deref(), Some("收藏"), "子级的条目也要改挂");
        assert_eq!(resource::get(&conn, r_keep.id).unwrap().category.as_deref(), Some("收藏"));
        let _ = keep;
    }

    #[test]
    fn deleting_a_non_default_parent_promotes_the_real_default() {
        let mut conn = init_in_memory().unwrap();
        let dev = create(&conn, "web", "开发").unwrap(); // 默认
        let fav = create(&conn, "web", "收藏").unwrap();
        let r = resource::create(&conn, ResourceKind::Web, "x", "t", Some("开发"), None, None).unwrap();

        delete(&mut conn, dev.id).unwrap();
        assert_eq!(resource::get(&conn, r.id).unwrap().category.as_deref(), Some("收藏"));
        let subs = list(&conn).unwrap();
        let fav_sub = subs.iter().find(|s| s.name == "收藏").unwrap();
        assert!(fav_sub.is_default, "删了默认，剩下的排序最前者应晋升");
        let _ = fav;
    }

    #[test]
    fn tree_nests_by_path_and_keeps_sibling_order() {
        let conn = init_in_memory().unwrap();
        let a = create(&conn, "web", "开发").unwrap();
        let _a2 = create(&conn, "web", "开发/前端").unwrap();
        let _a3 = create(&conn, "web", "开发/后端").unwrap();
        let _b = create(&conn, "web", "生活").unwrap();
        reorder(&conn, "web", &[a.id]).unwrap();

        let t = tree(&conn, "web").unwrap();
        assert_eq!(t.len(), 2, "两个顶层");
        assert_eq!(t[0].full_path, "开发");
        assert_eq!(t[0].children.len(), 2);
        assert_eq!(t[0].children[0].name, "前端", "展示名是末级");
        assert_eq!(t[0].children[0].full_path, "开发/前端");
        assert_eq!(t[0].children[0].depth, 1);
        assert_eq!(t[1].full_path, "生活");
        assert!(t[1].children.is_empty());
        assert!(t[0].is_default, "第一个小类是默认");
    }

    #[test]
    fn tree_reports_an_orphan_instead_of_silently_dropping_it() {
        // 人工造一个孤儿：只有子级没有父级
        let conn = init_in_memory().unwrap();
        // ⚠️ 必须带分隔符才是孤儿：名字里没有「/」的话它就是个普通顶层节点，
        //   tree() 会正常返回 —— 那条用例就变成在测「顶层节点能进树」，
        //   测不到「孤儿被吞掉」这件事。
        conn.execute(
            "INSERT INTO resource_subcategories (kind, name, sort_order, is_default) VALUES ('web', '查无此父/孤儿子级', 0, 0)",
            [],
        )
        .unwrap();
        // 正常路径不该报错
        let t = tree(&conn, "file").unwrap();
        assert_eq!(t.len(), 7, "另一个大类不受影响");
        assert!(tree(&conn, "web").is_err(), "孤儿小类必须报错而不是被吞掉");
    }
}
