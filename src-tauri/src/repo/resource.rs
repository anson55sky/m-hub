use crate::models::{Resource, ResourceKind};
use crate::repo::now;
use rusqlite::{params, Connection, Result};

pub fn create(
    conn: &Connection,
    kind: ResourceKind,
    name: &str,
    target: &str,
    category: Option<&str>,
    icon: Option<&str>,
    args: Option<&str>,
) -> Result<Resource> {
    let ts = now();
    conn.execute(
        "INSERT INTO resources (kind, name, target, category, icon, args, sort_order, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, (SELECT COALESCE(MAX(sort_order), 0) + 1 FROM resources), ?7, ?7)",
        params![kind_to_str(&kind), name, target, category, icon, args, ts],
    )?;
    let id = conn.last_insert_rowid();
    // ADR 0012 决策 3：新建未指定小类 → 自动归入该大类的默认小类；
    // 该大类还没有任何小类时保持 NULL（「未归类」）。编辑时显式置 NULL 不走这里。
    //
    // 两条写必须同事务（2026-09-29 补）。原先分开提交，第二条失败时
    // 资源已落库但 category 为 NULL —— 界面上显示「未归类」，不报错也不崩溃，
    // 但那条 ADR 决策就是没生效，而用户完全无从察觉是哪一步出的问题。
    let tx = conn.unchecked_transaction()?;
    if category.is_none() {
        let kind_str = kind_to_str(&kind);
        if let Some(default_cat) = super::subcategory::default_name(&tx, &kind_str)? {
            tx.execute(
                "UPDATE resources SET category = ?1 WHERE id = ?2",
                params![default_cat, id],
            )?;
        }
    }
    tx.commit()?;
    get(conn, id)
}

/// 资源列表的**全部列**。
///
/// ⚠️ 别再各处硬写一串列名（v0.8.0 前有 5 份逐字重复，加一列就得改 5 处，
///   漏一处就是 `row_to_resource` 取列错位 —— 而那种错不报错，
///   表现为「某几个字段串了行」，能排查很久）。
const COLS: &str = "id, kind, name, target, category, icon, args, sort_order, last_launched_at, created_at, updated_at, secret_label";

pub fn get(conn: &Connection, id: i64) -> Result<Resource> {
    conn.query_row(
        &format!("SELECT {COLS} FROM resources WHERE id = ?1"),
        params![id],
        row_to_resource,
    )
}

pub fn list_all(conn: &Connection) -> Result<Vec<Resource>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {COLS} FROM resources ORDER BY sort_order ASC, id ASC"
    ))?;
    let rows = stmt.query_map([], row_to_resource)?;
    rows.collect()
}

pub fn update(
    conn: &Connection,
    id: i64,
    kind: ResourceKind,
    name: &str,
    target: &str,
    category: Option<&str>,
    icon: Option<&str>,
    args: Option<&str>,
) -> Result<Resource> {
    let affected = conn.execute(
        "UPDATE resources SET kind = ?1, name = ?2, target = ?3, category = ?4, icon = ?5, args = ?6, updated_at = ?7 WHERE id = ?8",
        params![kind_to_str(&kind), name, target, category, icon, args, now(), id],
    )?;
    if affected == 0 {
        // 带 NOT_FOUND 前缀：扩展桥调用方据此与服务器错误区分，不再盲目重试
        return Err(rusqlite::Error::InvalidParameterName(format!(
            "NOT_FOUND: 资源 {id} 不存在"
        )));
    }
    get(conn, id)
}

pub fn delete(conn: &Connection, id: i64) -> Result<()> {
    conn.execute("DELETE FROM resources WHERE id = ?1", params![id])?;
    Ok(())
}

/// 记录资源最近启动时间（最近使用排序用）
pub fn touch(conn: &Connection, id: i64) -> Result<()> {
    conn.execute(
        "UPDATE resources SET last_launched_at = ?1 WHERE id = ?2",
        params![now(), id],
    )?;
    Ok(())
}

/// 按新顺序重新排列所有资源
pub fn reorder(conn: &Connection, ids: &[i64]) -> Result<()> {
    let ts = now();
    let tx = conn.unchecked_transaction()?;
    for (order, id) in ids.iter().enumerate() {
        tx.execute(
            "UPDATE resources SET sort_order = ?1, updated_at = ?2 WHERE id = ?3",
            params![order as i64, ts, id],
        )?;
    }
    tx.commit()
}

pub fn search(conn: &Connection, keyword: &str) -> Result<Vec<Resource>> {
    let pattern = format!("%{}%", keyword);
    let mut stmt = conn.prepare(&format!(
        "SELECT {COLS} FROM resources WHERE name LIKE ?1 ORDER BY sort_order ASC"
    ))?;
    let rows = stmt.query_map(params![pattern], row_to_resource)?;
    rows.collect()
}

/// 写/清加密备注（v0.8.0 发布说明 ⑧）。
///
/// `cipher` 为 `None` 或空串 = **清除**（正文置 NULL、名字置空串），两条一起做 ——
/// 留着名字却没了正文，列表上就是一条打不开的空备注，比从来没有更费解。
pub fn set_secret(conn: &Connection, id: i64, label: &str, cipher: Option<&str>) -> Result<()> {
    let affected = conn.execute(
        "UPDATE resources SET secret_label = ?1, secret_note = ?2, updated_at = ?3 WHERE id = ?4",
        params![label, cipher, now(), id],
    )?;
    if affected == 0 {
        return Err(rusqlite::Error::InvalidParameterName(format!(
            "NOT_FOUND: 资源 {id} 不存在"
        )));
    }
    Ok(())
}

/// 读一条资源的加密备注：`(备注名, 密文 Option)`。
///
/// ⚠️ **密文列永远不进任何列表查询**（`COLS` 里没有它）——只有这个函数能取到。
///   列写错一个字符的后果不是报错，而是密文被当明文发给前端。
pub fn get_secret(conn: &Connection, id: i64) -> Result<(String, Option<String>)> {
    conn.query_row(
        "SELECT secret_label, secret_note FROM resources WHERE id = ?1",
        params![id],
        |r| {
            let label: String = r.get(0)?;
            // 空串与 NULL 同义（老数据/命令层的「清空」都可能落成这两种）
            let raw: Option<String> = r.get(1)?;
            let cipher = raw.filter(|s| !s.is_empty());
            Ok((label, cipher))
        },
    )
}

pub fn kind_to_str(kind: &ResourceKind) -> &'static str {
    match kind {
        ResourceKind::App => "app",
        ResourceKind::Web => "web",
        ResourceKind::File => "file",
    }
}

pub fn row_to_resource(row: &rusqlite::Row) -> Result<Resource> {
    let kind: String = row.get(1)?;
    Ok(Resource {
        id: row.get(0)?,
        kind: match kind.as_str() {
            "app" => ResourceKind::App,
            "file" => ResourceKind::File,
            _ => ResourceKind::Web,
        },
        name: row.get(2)?,
        target: row.get(3)?,
        category: row.get(4)?,
        icon: row.get(5)?,
        args: row.get(6)?,
        sort_order: row.get(7)?,
        last_launched_at: row.get(8)?,
        created_at: row.get(9)?,
        updated_at: row.get(10)?,
        secret_label: row.get(11)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::init_in_memory;

    fn setup() -> Connection {
        init_in_memory().unwrap()
    }

    #[test]
    fn create_and_get_resource() {
        let conn = setup();
        let r = create(&conn, ResourceKind::App, "VS Code", "/usr/bin/code", None, Some("icon"), Some("--reuse-window"))
            .unwrap();
        assert_eq!(r.name, "VS Code");
        assert_eq!(r.kind, ResourceKind::App);
        assert_eq!(r.sort_order, 1);
    }

    #[test]
    fn create_and_get_file_resource() {
        let conn = setup();
        let r = create(&conn, ResourceKind::File, "报告", "C:/docs/report.pdf", Some("文档"), None, None)
            .unwrap();
        assert_eq!(r.kind, ResourceKind::File);
        assert_eq!(r.category.as_deref(), Some("文档"));
        assert_eq!(r.target, "C:/docs/report.pdf");
    }

    #[test]
    fn list_all_ordered() {
        let conn = setup();
        let a = create(&conn, ResourceKind::Web, "GitHub", "https://github.com", None, None, None).unwrap();
        let b = create(&conn, ResourceKind::Web, "Google", "https://google.com", None, None, None).unwrap();
        let list = list_all(&conn).unwrap();
        assert_eq!(list.iter().map(|r| r.id).collect::<Vec<_>>(), vec![a.id, b.id]);
    }

    #[test]
    fn update_resource_fields() {
        let conn = setup();
        let r = create(&conn, ResourceKind::App, "Old", "/bin/old", None, None, None).unwrap();
        let updated = update(&conn, r.id, ResourceKind::Web, "New", "https://new.com", None, Some("i"), Some("a"))
            .unwrap();
        assert_eq!(updated.name, "New");
        assert_eq!(updated.kind, ResourceKind::Web);
        assert_eq!(updated.target, "https://new.com");
    }

    #[test]
    fn reorder_resources() {
        let conn = setup();
        let a = create(&conn, ResourceKind::Web, "A", "https://a.com", None, None, None).unwrap();
        let b = create(&conn, ResourceKind::Web, "B", "https://b.com", None, None, None).unwrap();
        reorder(&conn, &[b.id, a.id]).unwrap();
        let list = list_all(&conn).unwrap();
        assert_eq!(list.iter().map(|r| r.id).collect::<Vec<_>>(), vec![b.id, a.id]);
    }

    #[test]
    fn delete_resource() {
        let conn = setup();
        let r = create(&conn, ResourceKind::App, "Temp", "/bin/temp", None, None, None).unwrap();
        delete(&conn, r.id).unwrap();
        assert!(get(&conn, r.id).is_err());
    }

    #[test]
    fn search_resources_by_name() {
        let conn = setup();
        create(&conn, ResourceKind::Web, "GitHub", "https://github.com", None, None, None).unwrap();
        create(&conn, ResourceKind::Web, "Google", "https://google.com", None, None, None).unwrap();
        let found = search(&conn, "git").unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].name, "GitHub");
    }

    // ---- 加密备注（v0.8.0 发布说明 ⑧）----

    #[test]
    fn secret_writes_reads_and_clears() {
        let conn = setup();
        let r = create(&conn, ResourceKind::Web, "GitHub", "https://github.com", None, None, None).unwrap();
        // 没有备注时是空名 + 无密文（不是错误）
        assert_eq!(get_secret(&conn, r.id).unwrap(), (String::new(), None));

        set_secret(&conn, r.id, "账号密码", Some("CIPHER-TEXT")).unwrap();
        assert_eq!(
            get_secret(&conn, r.id).unwrap(),
            ("账号密码".to_string(), Some("CIPHER-TEXT".to_string()))
        );
        // 名字进列表（明文，可见 🔒）
        assert_eq!(get(&conn, r.id).unwrap().secret_label, "账号密码");
        assert_eq!(list_all(&conn).unwrap()[0].secret_label, "账号密码");

        // 清除：名字与密文必须**一起**走，否则列表上留着一条打不开的空备注
        set_secret(&conn, r.id, "", None).unwrap();
        assert_eq!(get_secret(&conn, r.id).unwrap(), (String::new(), None));
        assert_eq!(get(&conn, r.id).unwrap().secret_label, "");

        // 空串密文与 NULL 同义（老数据可能落成这两种）
        conn.execute(
            "UPDATE resources SET secret_label = 'x', secret_note = '' WHERE id = ?1",
            params![r.id],
        )
        .unwrap();
        assert_eq!(get_secret(&conn, r.id).unwrap(), ("x".to_string(), None));
    }

    /// **密文绝不许出现在列表/详情里**。
    ///
    /// 判据用「序列化成前端拿到的那个 JSON」而不是「查字段」：结构体里本来就没有
    /// 密文字段，断言它不存在是废话；真正会出事的是有人把 `secret_note` 加进
    /// `COLS`，那时列表接口就把全部备注原文发给了前端 —— 而这类改动不报任何错。
    #[test]
    fn list_payload_never_carries_the_ciphertext() {
        let conn = setup();
        let r = create(&conn, ResourceKind::Web, "GitHub", "https://github.com", None, None, None).unwrap();
        set_secret(&conn, r.id, "账号密码", Some("SECRET-CIPHERTEXT-DO-NOT-LEAK")).unwrap();
        let json = serde_json::to_string(&list_all(&conn).unwrap()).unwrap();
        assert!(
            !json.contains("SECRET-CIPHERTEXT-DO-NOT-LEAK"),
            "列表接口把密文发出去了：{json}"
        );
        assert!(!COLS.contains("secret_note"), "密文列进了列表查询的列清单");
        // 明文的名字是要在列表里显示的（用户靠它分辨哪条写了备注）
        assert!(json.contains("账号密码"), "{json}");
    }

    /// 改不存在的资源必须报错（否则「备注没保存」会被当成成功）。
    #[test]
    fn secret_on_missing_resource_errors() {
        let conn = setup();
        assert!(set_secret(&conn, 9999, "x", Some("c")).is_err());
        assert!(get_secret(&conn, 9999).is_err());
    }
}
