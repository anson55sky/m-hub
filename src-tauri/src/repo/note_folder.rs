//! 速记文件夹的数据访问层（v0.8.0，发布说明 ①）。
//!
//! 数据结构与速达小类（`repo/subcategory.rs`）**同构**，路径纯函数共用
//! `repo/treepath.rs`。两者的差别只有三处，都在本文件里：
//!
//! | | 速达小类 | 速记文件夹 |
//! |---|---|---|
//! | 归属 | `resources.category`（**名字**，无外键） | `notes.folder_id`（**id**，有外键） |
//! | 删除时 | 条目批量改挂默认小类 | 笔记**改挂「未归类」**（`folder_id = NULL`） |
//! | 默认值 | 大类的第一个小类自动成为默认 | **无默认**，顶层就是「未归类」 |
//!
//! 第三条是有意的：速记的「未归类」是一等公民（大量笔记本来就该留在根），
//! 强加一个默认小类会让新建笔记全部掉进某个用户没建过的文件夹里。

use super::treepath::{
    build_tree, is_within, join_path, leaf_of, parent_path, reparent, TreeNode,
};
use rusqlite::{params, Connection, Result};

const COLS: &str = "id, name, sort_order";

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct NoteFolder {
    pub id: i64,
    pub name: String,
    pub sort_order: i64,
}

/// 层级树节点。判定真源是 `repo::note_folder::tree`，前端那份只做展示。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct FolderNode {
    pub id: i64,
    pub name: String,
    pub depth: usize,
    pub full_path: String,
    /// 该文件夹**及其全部后代**里的笔记数。
    ///
    /// ⚠️ 是含后代的总数，不是「直属」。界面上父分类右边那个数字如果只算直属，
    /// 用户点进去发现数量对不上，会以为筛选漏了东西。
    /// 这也是筛选的口径（`subtreeFilterSql`）—— 两处必须一致。
    pub note_count: i64,
    pub children: Vec<FolderNode>,
}

fn row_to_folder(row: &rusqlite::Row) -> Result<NoteFolder> {
    Ok(NoteFolder {
        id: row.get(0)?,
        name: row.get(1)?,
        sort_order: row.get(2)?,
    })
}

pub fn list(conn: &Connection) -> Result<Vec<NoteFolder>> {
    let mut stmt =
        conn.prepare(&format!("SELECT {COLS} FROM note_folders ORDER BY sort_order ASC, id ASC"))?;
    let rows = stmt.query_map([], row_to_folder)?;
    rows.collect()
}

/// 段是否合法（业务名词 = 「文件夹」）。
#[inline]
pub fn validate_segment(seg: &str) -> Result<(), String> {
    crate::repo::treepath::validate_segment(seg, "文件夹")
}

/// 该名字（含其所有后代）是否已被占用。建同级/子级都要查。
pub fn is_name_free(conn: &Connection, name: &str) -> Result<bool> {
    let n: i64 = conn.query_row(
        "SELECT COUNT(*) FROM note_folders WHERE name = ?1",
        params![name],
        |r| r.get(0),
    )?;
    Ok(n == 0)
}

/// 在 `parent` 下新建一级。`parent` 空串 = 顶层。
///
/// 返回 `Result<_, String>` 而不是 `rusqlite::Result`：要报「已存在」「名称不能为空」
/// 这类**业务**错误，套进 `rusqlite::Error` 只能借 `InvalidParameterName` 硬塞，
/// 那是给「SQL 用错」用的，混进业务错会让调用点的错误文案处理形同虚设。
pub fn create(conn: &Connection, parent: &str, seg: &str) -> Result<NoteFolder, String> {
    validate_segment(seg)?;
    let full = join_path(parent, seg);
    if !is_name_free(conn, &full).map_err(db_err)? {
        return Err(format!("文件夹「{full}」已存在"));
    }
    // ⚠️ 新建时 sort_order 取「当前最大值 + 1」：排序是全组的，新项一律排最后。
    //   取 max 而不是 count —— delete 之后 count 会和实际项数不符（中间有空洞），
    //   而 sort_order 只要求单调，max+1 永远安全。
    let next: i64 = conn
        .query_row(
            "SELECT COALESCE(MAX(sort_order), -1) + 1 FROM note_folders",
            [],
            |r| r.get(0),
        )
        .map_err(db_err)?;
    conn.execute(
        "INSERT INTO note_folders (name, sort_order) VALUES (?1, ?2)",
        params![full, next],
    )
    .map_err(db_err)?;
    Ok(NoteFolder {
        id: conn.last_insert_rowid(),
        name: full,
        sort_order: next,
    })
}

/// 改名：**只改自己那一段**，整棵子树跟着换父。
///
/// ⚠️ 新路径由这里算，前端不拼 —— 前端拼的话两边规则一旦不同步，
///   表现是「界面上叫 A、库里叫 B」，而筛选按库里那份算，于是新建的子级
///   筛不到（约定 79 同款）。
pub fn rename(conn: &mut Connection, id: i64, seg: &str) -> Result<(), String> {
    validate_segment(seg)?;
    let tx = conn.transaction().map_err(db_err)?;
    let old: String = tx
        .query_row(
            "SELECT name FROM note_folders WHERE id = ?1",
            params![id],
            |r| r.get(0),
        )
        .map_err(|_| format!("文件夹不存在（id={id}）"))?;
    let new_self = join_path(parent_path(&old), seg);

    // ⚠️ 冲突判定必须**排除自己**：改回原名是合法的（改个名又改回来），
    //   而 `UNIQUE(name)` 会让「新旧同名」直接报约束错误 —— 那是错的语义。
    let clash: i64 = tx
        .query_row(
            "SELECT COUNT(*) FROM note_folders WHERE name = ?1 AND id <> ?2",
            params![new_self, id],
            |r| r.get(0),
        )
        .map_err(db_err)?;
    if clash > 0 {
        return Err(format!("文件夹「{new_self}」已存在"));
    }
    // ⚠️ 这里**曾经**有一个「不能挪进自己子树」的检查，是**死代码**，已删。
    //   推理：`new_self = join_path(parent_path(old), seg)`，而 `validate_segment`
    //   已保证 `seg` 不含 `/` —— 所以 `new_self` 的深度**恒等于** `old` 的深度。
    //   而「严格后代」的深度必然更大，两者不可能同时成立。环在数学上就形成不了。
    //   我原来那条测试是**假守卫**：它断言 `rename(seg="工作/会议")` 报错，
    //   而那句实际被 `validate_segment`（含分隔符）拦下，删掉环检查照样绿。
    //   换成了下面那条真正的不变式断言。

    // 先收集整棵子树，再逐条换父。收集必须**先于**写入：
    // 边查边写会在同一层里撞到自己刚改过的行。
    let subtree: Vec<(i64, String)> = {
        let mut stmt = tx
            .prepare("SELECT id, name FROM note_folders ORDER BY sort_order ASC, id ASC")
            .map_err(db_err)?;
        let all = stmt
            .query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))
            .map_err(db_err)?
            .collect::<Result<Vec<(i64, String)>>>()
            .map_err(db_err)?;
        all.into_iter()
            .filter(|(_, p)| is_within(p, &old))
            .collect()
    };
    for (fid, path) in &subtree {
        let new_path = reparent(&old, &new_self, path).ok_or_else(|| {
            format!("内部错误：子树换父失败（{path} 不在 {old} 之下）")
        })?;
        tx.execute(
            "UPDATE note_folders SET name = ?1 WHERE id = ?2",
            params![new_path, fid],
        )
        .map_err(db_err)?;
    }
    tx.commit().map_err(db_err)
}

/// 删除：**连子树一起删**，子树里的笔记一并改挂「未归类」。
///
/// ⚠️ 「笔记改挂未归类」而不是「连带删笔记」—— 用户建了层级结构是为了整理，
///   删一个文件夹就把他那一堆笔记一起删掉是不可接受的（约定 80）。
pub fn delete(conn: &mut Connection, id: i64) -> Result<(), String> {
    let tx = conn.transaction().map_err(db_err)?;
    let name: String = tx
        .query_row(
            "SELECT name FROM note_folders WHERE id = ?1",
            params![id],
            |r| r.get(0),
        )
        .map_err(|_| format!("文件夹不存在（id={id}）"))?;

    let ids: Vec<i64> = {
        let mut stmt = tx.prepare("SELECT id, name FROM note_folders").map_err(db_err)?;
        let all = stmt
            .query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))
            .map_err(db_err)?
            .collect::<Result<Vec<(i64, String)>>>()
            .map_err(db_err)?;
        all.into_iter()
            .filter(|(_, p)| is_within(p, &name))
            .map(|(i, _)| i)
            .collect()
    };
    // 先把笔记摘出来再删文件夹。外键是 ON DELETE SET NULL，理论上删文件夹
    // 就会自动置空 —— 但那条子句只在 `PRAGMA foreign_keys = ON` 时生效，
    // 而本工程**没有**开启它（历史原因）。显式改挂不依赖那个 pragma，
    // 两边行为一致，不给「将来有人开了 pragma」埋一个行为差异。
    for fid in &ids {
        tx.execute(
            "UPDATE notes SET folder_id = NULL WHERE folder_id = ?1",
            params![fid],
        )
        .map_err(db_err)?;
    }
    for fid in &ids {
        tx.execute("DELETE FROM note_folders WHERE id = ?1", params![fid])
            .map_err(db_err)?;
    }
    tx.commit().map_err(db_err)
}

/// 整组重排（拖拽排序落点后写回）。
pub fn reorder(conn: &Connection, ids: &[i64]) -> Result<()> {
    for (i, id) in ids.iter().enumerate() {
        conn.execute(
            "UPDATE note_folders SET sort_order = ?1 WHERE id = ?2",
            params![i as i64, id],
        )?;
    }
    Ok(())
}

fn db_err(e: rusqlite::Error) -> String {
    e.to_string()
}

/// 层级树 + 每个节点的笔记数（含后代）。
pub fn tree(conn: &Connection) -> Result<Vec<FolderNode>> {
    let folders = list(conn)?;
    let flat: Vec<(i64, String)> = folders.iter().map(|f| (f.id, f.name.clone())).collect();
    let nodes = build_tree(&flat, "文件夹")
        .map_err(rusqlite::Error::InvalidParameterName)?;

    // 直属计数一次查完，再沿树累加出「含后代」的总数
    let mut direct: std::collections::HashMap<i64, i64> = std::collections::HashMap::new();
    {
        let mut stmt = conn.prepare(
            "SELECT folder_id, COUNT(*) FROM notes WHERE folder_id IS NOT NULL AND deleted_at IS NULL GROUP BY folder_id",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?))
        })?;
        for row in rows {
            let (k, v) = row?;
            direct.insert(k, v);
        }
    }

    fn to_nodes(
        nodes: Vec<TreeNode>,
        direct: &std::collections::HashMap<i64, i64>,
    ) -> Vec<FolderNode> {
        nodes
            .into_iter()
            .map(|n| {
                let children = to_nodes(n.children, direct);
                // 含后代：自己 + 全部孩子（孩子那边已经算好了）
                let count = children.iter().map(|c| c.note_count).sum::<i64>()
                    + direct.get(&n.id).copied().unwrap_or(0);
                FolderNode {
                    id: n.id,
                    name: leaf_of(&n.full_path).to_string(),
                    depth: n.depth,
                    full_path: n.full_path,
                    note_count: count,
                    children,
                }
            })
            .collect()
    }
    Ok(to_nodes(nodes, &direct))
}

/// 该文件夹及其全部后代的 id 集合 —— 「筛选某层 = 含全部后代」。
pub fn subtree_ids(conn: &Connection, id: i64) -> Result<Vec<i64>> {
    let name: String = conn.query_row(
        "SELECT name FROM note_folders WHERE id = ?1",
        params![id],
        |r| r.get(0),
    )?;
    let mut stmt = conn.prepare("SELECT id, name FROM note_folders")?;
    let all = stmt
        .query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))?
        .collect::<Result<Vec<(i64, String)>>>()?;
    Ok(all
        .into_iter()
        .filter(|(_, p)| is_within(p, &name))
        .map(|(i, _)| i)
        .collect())
}

/// 笔记数（**未删除**的）—— 与 `tree` 的计数口径必须一致。
pub fn count_live_notes(conn: &Connection) -> Result<i64> {
    conn.query_row(
        "SELECT COUNT(*) FROM notes WHERE deleted_at IS NULL",
        [],
        |r| r.get(0),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::init_in_memory;

    #[test]
    fn create_top_level_and_nested() {
        let conn = init_in_memory().unwrap();
        let a = create(&conn, "", "工作").unwrap();
        assert_eq!(a.name, "工作");
        let b = create(&conn, "工作", "会议").unwrap();
        assert_eq!(b.name, "工作/会议");
        assert_eq!(list(&conn).unwrap().len(), 2);
    }

    #[test]
    fn create_rejects_duplicate_and_bad_segment() {
        let conn = init_in_memory().unwrap();
        create(&conn, "", "工作").unwrap();
        create(&conn, "工作", "会议").unwrap();
        assert!(create(&conn, "", "工作").is_err(), "同名顶层");
        assert!(create(&conn, "工作", "会议").is_err(), "同一父下的同名子级");
        // ⚠️「工作/工作」**是合法的**独立路径，不是重名 —— 层级只由路径决定，
        //   同名不同父是两个文件夹（这与速达小类一致）。
        assert!(create(&conn, "工作", "工作").is_ok(), "同名不同父应当允许");
        assert!(create(&conn, "", "含/斜杠").is_err(), "含分隔符");
        assert!(create(&conn, "", "  ").is_err(), "纯空白");
        assert!(create(&conn, "", &"x".repeat(21)).is_err(), "超长");
    }

    /// 新建时 sort_order 必须取 max+1 而不是 count：delete 之后 count 与实际
    /// 项数不符（中间有空洞），而 sort_order 只要求单调。
    #[test]
    fn create_uses_max_plus_one_not_count() {
        let conn = init_in_memory().unwrap();
        let a = create(&conn, "", "甲").unwrap();
        let b = create(&conn, "", "乙").unwrap();
        let c = create(&conn, "", "丙").unwrap();
        let mut conn = conn;
        assert_eq!((a.sort_order, b.sort_order, c.sort_order), (0, 1, 2));

        // ⚠️ 必须删**第一个**（甲）而不是最后一个。
        //   删最后一个时空洞在**尾部**：`MAX` 与 `COUNT-1` 相等，两种实现给同一个数，
        //   断言再严也区分不出来（我第一版就是这么写的，红灯是假的）。
        //   删中间/头部才会分叉：删甲后剩 乙(1)、丙(2) ——
        //     · `COUNT(*)`      = 2 → 丁拿到 2，与丙**同级**，次序退化成按 id 决胜
        //     · `MAX(sort)+1`   = 3 → 丁真正排在丙之后
        //   症状是「新建的文件夹插到了某个老文件夹前面」。
        delete(&mut conn, a.id).unwrap();
        let d = create(&conn, "", "丁").unwrap();
        assert_eq!(d.sort_order, 3, "必须取 max+1，而不是 count");

        // 顺带钉住：删掉的那条不该被「补回来」，丁也不能占掉它的位置
        assert_eq!(b.sort_order, 1);
        assert_eq!(c.sort_order, 2);
        assert_ne!(d.sort_order, c.sort_order, "与存量同级 = 次序不可预期");
    }

    #[test]
    fn rename_only_own_segment_and_moves_subtree() {
        let mut conn = init_in_memory().unwrap();
        let work = create(&conn, "", "工作").unwrap();
        let meet = create(&conn, "工作", "会议").unwrap();
        let weekly = create(&conn, "工作/会议", "周会").unwrap();
        rename(&mut conn, work.id, "职场").unwrap();
        let names: Vec<String> = list(&conn).unwrap().into_iter().map(|f| f.name).collect();
        assert_eq!(names, vec!["职场", "职场/会议", "职场/会议/周会"]);
        // 笔记的 folder_id 不变（id 没变），只是路径换父
        assert_eq!(get_id(&conn, meet.id).unwrap(), "职场/会议");
        assert_eq!(get_id(&conn, weekly.id).unwrap(), "职场/会议/周会");
    }

    fn get_id(conn: &Connection, id: i64) -> Result<String> {
        conn.query_row("SELECT name FROM note_folders WHERE id = ?1", params![id], |r| {
            r.get(0)
        })
    }

    /// ⚠️ 改回原名必须合法：`UNIQUE(name)` 会让「新旧同名」直接报约束错误，
    ///   但那不是「名字冲突」，是用户改了个名又想改回来。
    /// ⚠️ 不变式：**改名不改变深度**。
    ///
    /// 这条不是装饰，它是「改名不可能形成环」的**证明**：
    /// `new_self = parent_path(old) + "/" + seg`，而 `validate_segment` 保证 `seg`
    /// 不含 `/`，所以 `depth(new_self) == depth(old)` 恒成立；而严格后代的深度必然
    /// 更大。我原来那条「不能挪进自己子树」的检查因此是死代码（测试也一并删了）。
    /// 一旦有人放宽 `validate_segment`（比如允许段内含 `/`），这条会先红。
    #[test]
    fn rename_preserves_depth_so_cycles_are_impossible() {
        let mut conn = init_in_memory().unwrap();
        let work = create(&conn, "", "工作").unwrap();
        let meet = create(&conn, "工作", "会议").unwrap();
        let deep = create(&conn, "工作/会议", "周会").unwrap();
        rename(&mut conn, work.id, "职场").unwrap();
        assert_eq!(crate::repo::treepath::depth_of(&get_id(&conn, work.id).unwrap()), 0);
        assert_eq!(crate::repo::treepath::depth_of(&get_id(&conn, meet.id).unwrap()), 1);
        assert_eq!(crate::repo::treepath::depth_of(&get_id(&conn, deep.id).unwrap()), 2);
    }

    #[test]
    fn rename_back_to_original_name_is_allowed() {
        let mut conn = init_in_memory().unwrap();
        let a = create(&conn, "", "工作").unwrap();
        create(&conn, "工作", "会议").unwrap();
        rename(&mut conn, a.id, "职场").unwrap();
        rename(&mut conn, a.id, "工作").expect("改回原名应当允许");
        assert_eq!(get_id(&conn, a.id).unwrap(), "工作");
    }

    #[test]
    fn rename_rejects_conflict_and_cycle() {
        let mut conn = init_in_memory().unwrap();
        let work = create(&conn, "", "工作").unwrap();
        create(&conn, "", "生活").unwrap();
        create(&conn, "工作", "会议").unwrap();
        // 撞上另一个顶层名
        assert!(rename(&mut conn, work.id, "生活").is_err(), "与已有顶层同名");
        // 改回同名（同父同段）应当允许 —— 那不是冲突
        rename(&mut conn, work.id, "工作").unwrap();
        assert_eq!(get_id(&conn, work.id).unwrap(), "工作");
    }

    /// 删除连子树一起删，且子树里的笔记改挂「未归类」而不是被删掉。
    #[test]
    fn delete_removes_subtree_but_keeps_notes() {
        let mut conn = init_in_memory().unwrap();
        let work = create(&conn, "", "工作").unwrap();
        let meet = create(&conn, "工作", "会议").unwrap();
        let weekly = create(&conn, "工作/会议", "周会").unwrap();
        let life = create(&conn, "", "生活").unwrap();

        let n1 = insert_note(&conn, work.id, "工作笔记");
        let n2 = insert_note(&conn, weekly.id, "周会笔记");
        let n3 = insert_note(&conn, life.id, "生活笔记");

        delete(&mut conn, work.id).unwrap();

        assert_eq!(list(&conn).unwrap().len(), 1, "整棵子树都没了");
        assert!(get_id(&conn, meet.id).is_err());
        assert!(get_id(&conn, weekly.id).is_err());
        // 子树里的笔记：还在，只是回到未归类
        for n in [n1, n2] {
            assert!(note_exists(&conn, n), "笔记不该被删");
            assert_eq!(note_folder(&conn, n), None, "应改挂未归类");
        }
        // ⚠️ 不在子树里的那本**必须留在原文件夹**。我第一版把它也断言成 None，
        //   于是「生活」明明没被删、它的笔记却被算进「改挂未归类」——
        //   测试名字叫「keeps_notes」，实际却在检查一件没删的事。
        assert!(note_exists(&conn, n3));
        assert_eq!(note_folder(&conn, n3), Some(life.id), "生活没被删，不该改挂");
    }

    fn insert_note(conn: &Connection, folder_id: i64, title: &str) -> i64 {
        conn.execute(
            "INSERT INTO notes (title, folder_id) VALUES (?1, ?2)",
            params![title, folder_id],
        )
        .unwrap();
        conn.last_insert_rowid()
    }
    fn note_exists(conn: &Connection, id: i64) -> bool {
        conn.query_row("SELECT 1 FROM notes WHERE id = ?1", params![id], |_| Ok(()))
            .is_ok()
    }
    fn note_folder(conn: &Connection, id: i64) -> Option<i64> {
        conn.query_row("SELECT folder_id FROM notes WHERE id = ?1", params![id], |r| r.get(0))
            .ok()
    }

    /// 父节点的计数必须是「含后代」，否则界面上点进去数量对不上。
    #[test]
    fn tree_counts_include_descendants() {
        let conn = init_in_memory().unwrap();
        let work = create(&conn, "", "工作").unwrap();
        let meet = create(&conn, "工作", "会议").unwrap();
        create(&conn, "工作", "日常")).unwrap();
        insert_note(&conn, work.id, "直属工作");
        insert_note(&conn, meet.id, "会议里");

        let t = tree(&conn).unwrap();
        assert_eq!(t.len(), 1);
        // 工作 = 直属 1 + 会议 1 + 日常 0 = 2
        assert_eq!(t[0].note_count, 2, "父计数必须含后代");
        assert_eq!(t[0].children[0].full_path, "工作/会议");
        assert_eq!(t[0].children[0].note_count, 1);
    }

    /// 回收站里的笔记**不计入**文件夹数 —— 否则删一条笔记，那天所在的文件夹
    /// 计数就变了，而用户在回收站里点「还原」会看到数字对不上。
    #[test]
    fn tree_count_excludes_trashed_notes() {
        let conn = init_in_memory().unwrap();
        let work = create(&conn, "", "工作").unwrap();
        let n = insert_note(&conn, work.id, "待删");
        conn.execute(
            "UPDATE notes SET deleted_at = '2026-01-01 00:00:00.000' WHERE id = ?1",
            params![n],
        )
        .unwrap();
        assert_eq!(tree(&conn).unwrap()[0].note_count, 0);
        assert_eq!(count_live_notes(&conn).unwrap(), 0);
    }

    /// ⚠️ `subtree_ids` 按**段**比：`开发者` 不是 `开发` 的后代。
    ///   若按字符串前缀，删「开发者」会连带把「开发」下的笔记筛出来。
    #[test]
    fn subtree_ids_compare_by_segment() {
        let conn = init_in_memory().unwrap();
        let dev = create(&conn, "", "开发").unwrap();
        create(&conn, "开发", "前端")).unwrap();
        let dev2 = create(&conn, "", "开发者").unwrap();
        create(&conn, "开发者", "移动端")).unwrap();

        let ids = subtree_ids(&conn, dev.id).unwrap();
        assert_eq!(ids.len(), 2, "开发 + 开发/前端");
        assert!(!ids.contains(&dev2.id), "「开发者」与「开发」无关");
    }

    #[test]
    fn reorder_writes_sequential_order() {
        let conn = init_in_memory().unwrap();
        let a = create(&conn, "", "甲").unwrap();
        let b = create(&conn, "", "乙").unwrap();
        let c = create(&conn, "", "丙").unwrap();
        reorder(&conn, &[c.id, a.id, b.id]).unwrap();
        let names: Vec<String> = list(&conn).unwrap().into_iter().map(|f| f.name).collect();
        assert_eq!(names, vec!["丙", "甲", "乙"]);
    }
}
