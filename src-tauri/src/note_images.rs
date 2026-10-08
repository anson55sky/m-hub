//! 笔记图片的孤儿回收（v0.8.1）。
//!
//! 判据只有一条：**`notes/images/` 里没有任何笔记正文引用到的文件**。
//! 三条必须记住的语义，缺一条就是删掉用户还能看到的图：
//!
//! 1. **回收站里的笔记也要算引用**。软删（约定 v0.8.0 ③）之后笔记还在库里，
//!    用户随时能还原 —— 只统计「未删除」的笔记会把「刚删掉的那篇」的图删光，
//!    而还原回来就是一篇图全裂的笔记，且**没有任何报错**。
//! 2. **只删符合命名形状的文件**（`16 位十六进制 + 白名单扩展名`，即
//!    `import_note_image` 的产出形态）。用户自己往这个目录里丢的东西一律不动。
//! 3. **不猜「引用」怎么写**。正文里的地址有两种平台形态（见 `commands::note_image_prefix`），
//!    两种都要认；另外导出/导入期间正文里短暂存在 `images/<名>` 相对形态 ——
//!    那是包内形态，库里不该有，但认了也不亏（多留一张图比删掉一张强）。
//!
//! 触发时机只有两处：**永久删除笔记时**（`purge_note` / `empty_note_trash`）与
//! 设置里的手动清理。**不在启动时自动删** —— 用户没要求过的事，静默删他磁盘上的
//! 文件只会被当成「数据丢了」；这里宁可让孤儿慢慢积累到用户自己点一下。

use rusqlite::Connection;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// 正文中可能出现的图片引用前缀（两种平台形态 + 包内相对形态）。
const REF_PREFIXES: &[&str] = &[
    "http://mhub-note.localhost/",
    "mhub-note://localhost/",
    "images/",
];

/// 清理结果。`bytes` 是**这次清掉的**字节数（dry-run 时是「将会清掉」的）。
#[derive(Debug, Clone, Copy, serde::Serialize, PartialEq, Eq, Default)]
pub struct OrphanStats {
    pub files: usize,
    pub bytes: u64,
}

/// 数据根下的笔记图片目录。
pub fn images_dir() -> PathBuf {
    crate::paths::data_root().join("notes").join("images")
}

/// 是否是本应用产出的图片文件名（内容哈希命名）。
fn is_managed_name(name: &str) -> bool {
    let mut parts = name.split('.');
    let (Some(hash), Some(ext), None) = (parts.next(), parts.next(), parts.next()) else {
        return false;
    };
    let ext = ext.to_ascii_lowercase();
    hash.len() == 16
        && hash.chars().all(|c| c.is_ascii_hexdigit())
        && crate::commands::NOTE_IMAGE_EXTENSIONS.contains(&ext.as_str())
}

/// 全部笔记（含**回收站**）正文里引用到的图片文件名。
pub fn referenced_images(conn: &Connection) -> Result<BTreeSet<String>, String> {
    // ⚠️ 必须含回收站（语义 1）：这里漏了 deleted_at 的笔记，还原它们时就会图全裂。
    let mut stmt = conn
        .prepare("SELECT content FROM notes")
        .map_err(|e| format!("读取笔记正文失败: {e}"))?;
    let rows = stmt
        .query_map([], |r| r.get::<_, String>(0))
        .map_err(|e| format!("读取笔记正文失败: {e}"))?;
    let mut out = BTreeSet::new();
    for content in rows {
        let content = content.map_err(|e| format!("读取笔记正文失败: {e}"))?;
        for name in referenced_in(&content) {
            out.insert(name);
        }
    }
    Ok(out)
}

/// 一段正文里引用到的图片文件名（纯函数，可单测）。
fn referenced_in(content: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for prefix in REF_PREFIXES {
        let mut rest = content;
        while let Some(idx) = rest.find(prefix) {
            let after = &rest[idx + prefix.len()..];
            // 文件名只含 [0-9A-Za-z._-]，遇到别的字符就停 ——
            // 于是 `](...)`、引号、空白自动成为边界，不必逐个枚举标点。
            let end = after
                .find(|c: char| !(c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == '_'))
                .unwrap_or(after.len());
            let name = &after[..end];
            if is_managed_name(name) {
                out.insert(name.to_string());
            }
            rest = &after[end..];
        }
    }
    out
}

/// 列出 `dir` 里未被引用的图片：`(文件名, 字节数)`。
///
/// 与「删」分开（`scan_orphans` / `purge`）是为了**能只算不删** —— 设置里那个按钮
/// 要先把「会清掉几张、多大」报给用户看（约定 53：不可逆的操作要先交代后果）。
pub fn scan_orphans(dir: &Path, referenced: &BTreeSet<String>) -> Vec<(String, u64)> {
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return out; // 目录还没有（用户没用过图片）= 没有孤儿
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        // 语义 2：只动本应用产出的文件名形状
        if !is_managed_name(&name) || referenced.contains(&name) {
            continue;
        }
        let Ok(meta) = entry.metadata() else { continue };
        if !meta.is_file() {
            continue;
        }
        out.push((name, meta.len()));
    }
    out
}

/// 清理孤儿图片。`dry_run` 为真时只统计不删。
pub fn purge_orphans(conn: &Connection, dry_run: bool) -> Result<OrphanStats, String> {
    let dir = images_dir();
    let referenced = referenced_images(conn)?;
    let orphans = scan_orphans(&dir, &referenced);
    let mut stats = OrphanStats {
        files: orphans.len(),
        bytes: orphans.iter().map(|(_, n)| *n).sum(),
    };
    if dry_run {
        return Ok(stats);
    }
    for (name, _) in orphans {
        if let Err(e) = std::fs::remove_file(dir.join(&name)) {
            // 删不掉就跳过并记日志，不让一个坏文件让整批清理失败
            log::warn!("清理孤儿笔记图片失败: {name} ({e})");
            stats.files -= 1;
        }
    }
    Ok(stats)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::init_in_memory;

    fn url(name: &str) -> String {
        format!("{}{name}", crate::commands::note_image_prefix())
    }

    fn write(dir: &Path, name: &str, bytes: &[u8]) {
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(dir.join(name), bytes).unwrap();
    }

    /// 判据的核心：**回收站里的笔记也算引用**。
    ///
    /// 这条一旦漏掉，用户「删一篇 → 清理 → 还原」就会得到一篇图全裂的笔记，
    /// 而界面上没有任何一处能看出原因（正文一个字没变）。
    #[test]
    fn trashed_notes_still_protect_their_images() {
        let conn = init_in_memory().unwrap();
        let a = crate::repo::note::create(&conn, "甲").unwrap();
        crate::repo::note::update(&conn, a.id, "甲", &format!("图：{}", url("0123456789abcdef.png"))).unwrap();
        let b = crate::repo::note::create(&conn, "乙").unwrap();
        crate::repo::note::update(&conn, b.id, "乙", &format!("图：{}", url("fedcba9876543210.png"))).unwrap();
        // 只把乙送进回收站
        crate::repo::note::trash(&conn, b.id).unwrap();

        let referenced = referenced_images(&conn).unwrap();
        assert!(
            referenced.contains("fedcba9876543210.png"),
            "回收站里的笔记引用必须仍算引用，否则还原回来就是图全裂"
        );
        assert!(referenced.contains("0123456789abcdef.png"));
    }

    /// 三种引用形态都要认：两种协议形态 + 包内相对形态（导出期间库里可能出现）。
    #[test]
    fn all_three_reference_forms_are_recognized() {
        let content = format!(
            "![](http://mhub-note.localhost/0123456789abcdef.png)\n\
             ![](mhub-note://localhost/fedcba9876543210.jpg)\n\
             ![](images/aaaabbbbccccdddd.png)\n\
             ![](images/照片.jpg)\n\
             ![](https://example.com/0123456789abcdef.png)"
        );
        let found = referenced_in(&content);
        assert_eq!(found.len(), 3, "{found:?}");
        assert!(found.contains("0123456789abcdef.png"));
        assert!(found.contains("fedcba9876543210.jpg"));
        assert!(found.contains("aaaabbbbccccdddd.png"));
        // 非本应用命名（照片.jpg）与外部站点同名文件都不算引用
        assert!(!found.contains("照片.jpg"), "{found:?}");
    }

    /// 只删本应用命名形状的文件，且不碰仍被引用的。
    #[test]
    fn scan_only_reports_managed_names_that_are_unreferenced() {
        let dir = std::env::temp_dir().join(format!(
            "m-hub-orphan-scan-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        write(&dir, "0123456789abcdef.png", b"12345");
        write(&dir, "fedcba9876543210.png", b"123");
        write(&dir, "我的图片.png", b"1");   // 非命名形状：用户自己丢的，不动
        write(&dir, "README.txt", b"1");    // 同上

        let referenced: BTreeSet<String> = ["0123456789abcdef.png".to_string()].into();
        let orphans = scan_orphans(&dir, &referenced);
        assert_eq!(orphans.len(), 1, "{orphans:?}");
        assert_eq!(orphans[0], ("fedcba9876543210.png".to_string(), 3));

        // dry-run 语义：只统计不动文件
        let dir2 = orphans.clone();
        assert_eq!(dir2.len(), 1);
        assert!(dir.join("fedcba9876543210.png").is_file());

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 目录不存在时是「没有孤儿」，不是错误（用户可能从没插过图）。
    #[test]
    fn missing_dir_yields_no_orphans() {
        let missing = std::env::temp_dir().join("m-hub-orphan-dir-that-does-not-exist");
        let _ = std::fs::remove_dir_all(&missing);
        assert!(scan_orphans(&missing, &BTreeSet::new()).is_empty());
    }
}