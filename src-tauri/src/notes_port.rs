//! 速记导出 / 导入（v0.8.0，发布说明 ⑤）。
//!
//! 包是一个 zip，三条硬口径：
//!
//! 1. **`manifest.json` 是导入的唯一数据源**，`notes/` 下的 `.md` 是给人读的副本。
//!    两者由同一次导出写出、内容一致，但导入**不解析 Markdown** —— 解析意味着
//!    标题/标签/文件夹/时间戳要从文件名和正文里猜，猜错就是静默丢数据。
//!    代价：手改 `.md` 再导入不会生效（README.txt 里写明了）。
//! 2. **图片地址在包内改写成相对路径 `images/<文件名>`**，导入时改回协议 URL。
//!    绝不把数据根的绝对路径写进包 —— 换台机器、换数据目录都得能用（约定 72）。
//! 3. **回收站不导出**；导入永远是**追加**，按「标题 + 创建时间」判重跳过，
//!    绝不覆盖库里已有的笔记（导入错一个包的代价只是一堆重复行，可删；
//!    覆盖的代价是原笔记找不回来）。
//!
//!    由此还得到一条免费的好处：导入**不是原子的**（中间要读 zip、写图片、写库，
//!    不适合包在一个事务里），但它是**幂等的** —— 中途失败后重导同一个包，
//!    已成功的那些会被判重跳过，正好把上次没做完的补齐。

use crate::models::{Note, Tag};
use crate::repo::{note, note_folder, tag};
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeSet, HashMap};
use std::io::{Read, Write};
use std::path::Path;

/// 包格式标识。改它 = 老包全部拒绝导入，务必只在不兼容变更时改。
pub const FORMAT: &str = "m-hub-notes";
/// 包格式版本（当前语义见上面第 1 条）。只增不改：老客户端对不认识的版本要能报人话。
pub const FORMAT_VERSION: u32 = 1;

const MANIFEST: &str = "manifest.json";
const README: &str = "README.txt";
const NOTES_DIR: &str = "notes/";
const IMAGES_DIR: &str = "images/";
/// 「未归类」笔记在包内的目录名（它们没有 folder_id，包里必须有个去处）。
const UNFILED_DIR: &str = "未归类";

/// 正文里内嵌图片 URL 的两种形态（**两种都要认**，理由见下）。
///
/// `note_image_url` 写进正文的是「当前平台可渲染的那一种」（见
/// `commands::note_image_prefix`）；另一种是别的平台写下的数据 —— 换台机器、
/// 或从 Windows 导出到 macOS 时正文里就是它。只认一种的话，另一种形态的图片
/// 会在导出后变成死链（而作者完全看不出为什么）。
const URL_PREFIXES: &[&str] = &["http://mhub-note.localhost/", "mhub-note://localhost/"];
/// 包内相对路径前缀（导入时认它改回协议 URL）。
const REL_PREFIXES: &[&str] = &[IMAGES_DIR];

/// 单篇标题在文件名里的长度上限（字符数）。防止超长标题把 zip 路径顶爆。
const TITLE_FILE_LIMIT: usize = 60;

// ---------- 统计 ----------

/// 导出统计。`images_missing` 是**引用了但盘上找不到**的图片数（历史孤儿），
/// 报出来而不是静默丢 —— 用户看到它才会知道「这几张图没带出去」。
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct ExportStats {
    pub notes: usize,
    pub folders: usize,
    pub tags: usize,
    pub images: usize,
    pub images_missing: usize,
}

/// 导入统计。`skipped` = 判重跳过的笔记数（重复导入同一个包时它 = 包内笔记数）。
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct ImportStats {
    pub notes: usize,
    pub skipped: usize,
    pub folders: usize,
    pub tags: usize,
    pub images: usize,
}

// ---------- 包结构 ----------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Manifest {
    pub format: String,
    pub version: u32,
    pub exported_at: String,
    /// 全部文件夹路径（`name` 即全路径，见约定 76），含本包没有笔记的空文件夹。
    pub folders: Vec<String>,
    pub notes: Vec<NoteEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NoteEntry {
    pub title: String,
    /// 正文：图片地址已是 `images/<文件名>` 相对形态。
    pub content: String,
    #[serde(default)]
    pub folder: Option<String>,
    /// emoji 图标，空串 = 默认图标。
    #[serde(default)]
    pub icon: String,
    #[serde(default)]
    pub tags: Vec<String>,
    pub created_at: String,
    pub updated_at: String,
    /// 包内 `.md` 副本的路径（相对 `notes/`）。**只用于给人对照**，导入不读它。
    pub file: String,
}

// ---------- 图片地址改写 ----------

/// 是否是 `import_note_image` 产出的那种文件名（16 位十六进制哈希 + 白名单扩展名）。
///
/// 判这个形状而不只是「`images/` 后面接一段」：正文里偶然出现的
/// `images/别的.jpg`（外部相对链接）被改写成协议 URL 就成了死链，
/// 而这种误伤用户只会当成「我的图没了」。
fn is_image_name(name: &str) -> bool {
    let mut parts = name.split('.');
    let (Some(hash), Some(ext), None) = (parts.next(), parts.next(), parts.next()) else {
        return false;
    };
    let ext = ext.to_ascii_lowercase();
    hash.len() == 16
        && hash.chars().all(|c| c.is_ascii_hexdigit())
        && crate::commands::NOTE_IMAGE_EXTENSIONS.contains(&ext.as_str())
}

fn earliest(hay: &str, needles: &[&str]) -> Option<(usize, usize)> {
    let mut best: Option<(usize, usize)> = None;
    for n in needles {
        if let Some(p) = hay.find(n) {
            if best.map_or(true, |(b, _)| p < b) {
                best = Some((p, n.len()));
            }
        }
    }
    best
}

/// 在「协议 URL」与「包内相对路径」之间改写正文里的图片地址。
///
/// `to_relative = true`：导出（两种协议前缀 → `images/`）；
/// `to_relative = false`：导入（`images/` → 协议 URL）。
/// 返回 (新正文, 命中的文件名集合)。
fn swap_image_urls(content: &str, to_relative: bool) -> (String, BTreeSet<String>) {
    let needles: &[&str] = if to_relative { URL_PREFIXES } else { REL_PREFIXES };
    // 导入时写回**当前平台**的形态（不是固定第一种）—— 在 macOS 上写回 Windows 形态
    // 等于把图片原样搬回「必然裂」的状态，导出的包换个机器打开也照样裂。
    let repl = if to_relative {
        IMAGES_DIR
    } else {
        crate::commands::note_image_prefix()
    };
    let mut out = String::with_capacity(content.len());
    let mut rest = content;
    let mut found = BTreeSet::new();
    while let Some((idx, nlen)) = earliest(rest, needles) {
        let after = &rest[idx + nlen..];
        // 文件名里只有 [0-9A-Za-z._-]，遇到第一个别的字符就停 —— 于是
        // `](...`、空白、`"caption"` 都自动成为边界，不必逐个枚举标点。
        let end = after
            .find(|c: char| !(c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == '_'))
            .unwrap_or(after.len());
        let name = &after[..end];
        if is_image_name(name) {
            out.push_str(&rest[..idx]);
            out.push_str(repl);
            out.push_str(name);
            found.insert(name.to_string());
            rest = &after[end..];
        } else {
            // 不是我们的图片文件名：原样保留，继续往后找。
            // ⚠️ 必须推进指针（至少吃掉 needle），否则死循环。
            let consumed = idx + nlen + end;
            out.push_str(&rest[..consumed]);
            rest = &rest[consumed..];
        }
    }
    out.push_str(rest);
    (out, found)
}

/// 文件名/目录名安全化：换掉各平台都禁止的字符与控制字符，
/// 去掉结尾的点与空格（Windows 上「名字.」是非法的），截断，兜底空名。
fn sanitize_segment(s: &str) -> String {
    let mapped: String = s
        .chars()
        .map(|c| {
            if c.is_control() || matches!(c, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*') {
                '_'
            } else {
                c
            }
        })
        .collect();
    let mut out: String = mapped
        .trim()
        .chars()
        .take(TITLE_FILE_LIMIT)
        .collect::<String>()
        .trim_end_matches(['.', ' '])
        .to_string();
    if out.is_empty() {
        out = "未命名".into();
    }
    out
}

fn images_dir() -> std::path::PathBuf {
    crate::paths::data_root().join("notes").join("images")
}

// ---------- 导出 ----------

/// 把库里的速记打包成 zip 落到 `dest`。
///
/// 同步 IO + 持锁（调用方是 async 命令，跑在 tokio worker 上，不占主线程）。
pub fn export_to_zip(conn: &Connection, dest: &Path) -> Result<ExportStats, String> {
    let folders = note_folder::list(conn).map_err(|e| format!("读取文件夹失败: {e}"))?;
    let folder_names: Vec<String> = folders.iter().map(|f| f.name.clone()).collect();
    let id_to_path: HashMap<i64, String> =
        folders.iter().map(|f| (f.id, f.name.clone())).collect();

    let notes = note::list(conn).map_err(|e| format!("读取笔记失败: {e}"))?;

    // 标签：先取全量名字，再按 (note_id, tag_id) 关联表拼回每篇。
    let tag_name: HashMap<i64, String> = tag::list(conn)
        .map_err(|e| format!("读取标签失败: {e}"))?
        .into_iter()
        .map(|t: Tag| (t.id, t.name))
        .collect();
    let mut tags_of: HashMap<i64, Vec<String>> = HashMap::new();
    for (nid, tid) in tag::list_note_tags(conn).map_err(|e| format!("读取标签关联失败: {e}"))? {
        if let Some(name) = tag_name.get(&tid) {
            tags_of.entry(nid).or_default().push(name.clone());
        }
    }

    let mut images: BTreeSet<String> = BTreeSet::new();
    let mut used_tags: BTreeSet<String> = BTreeSet::new();
    let mut per_dir: HashMap<String, usize> = HashMap::new();
    let mut entries: Vec<NoteEntry> = Vec::new();

    for n in &notes {
        let (content, found) = swap_image_urls(&n.content, true);
        images.extend(found);
        if let Some(ts) = tags_of.get(&n.id) {
            used_tags.extend(ts.iter().cloned());
        }
        let dir = n
            .folder_id
            .and_then(|id| id_to_path.get(&id).cloned())
            .unwrap_or_else(|| UNFILED_DIR.to_string());
        // 目录名逐段安全化；manifest 里的 `folder` 仍是**库里的真路径**（导入要按它建文件夹），
        // 只有 zip 内的文件路径用安全化后的名字。
        let dir_rel = dir.split('/').map(sanitize_segment).collect::<Vec<_>>().join("/");
        let seq = per_dir.entry(dir_rel.clone()).or_insert(0);
        *seq += 1;
        let file = format!("{:03}-{}.md", seq, sanitize_segment(&n.title));
        entries.push(NoteEntry {
            title: n.title.clone(),
            content,
            folder: n.folder_id.and_then(|id| id_to_path.get(&id).cloned()),
            icon: n.icon.clone(),
            tags: tags_of.get(&n.id).cloned().unwrap_or_default(),
            created_at: n.created_at.clone(),
            updated_at: n.updated_at.clone(),
            file: format!("{dir_rel}/{file}"),
        });
    }

    // 引用了但盘上已经没有的图片：从集合里剔掉（打不进去），单独计数报出来。
    let mut missing = 0usize;
    images.retain(|name| {
        let ok = images_dir().join(name).is_file();
        if !ok {
            missing += 1;
        }
        ok
    });

    let manifest = Manifest {
        format: FORMAT.into(),
        version: FORMAT_VERSION,
        exported_at: chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
        folders: folder_names.clone(),
        notes: entries,
    };

    if let Some(parent) = dest.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent).map_err(|e| format!("创建目录失败: {e}"))?;
        }
    }
    let out = std::fs::File::create(dest).map_err(|e| format!("创建导出文件失败: {e}"))?;
    let mut zip = zip::ZipWriter::new(out);
    let opts = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);

    write_zip_str(&mut zip, README, README_TEXT, opts)?;
    let json = serde_json::to_string_pretty(&manifest).map_err(|e| e.to_string())?;
    write_zip_str(&mut zip, MANIFEST, &json, opts)?;

    for e in &manifest.notes {
        let path = format!("{NOTES_DIR}{}", e.file);
        write_zip_str(&mut zip, &path, &e.content, opts)?;
    }
    for name in &images {
        let src = images_dir().join(name);
        let bytes = std::fs::read(&src).map_err(|e| format!("读取图片 {name} 失败: {e}"))?;
        zip.start_file(format!("{IMAGES_DIR}{name}"), opts)
            .map_err(|e| e.to_string())?;
        zip.write_all(&bytes).map_err(|e| e.to_string())?;
    }

    zip.finish().map_err(|e| format!("完成导出失败: {e}"))?;

    Ok(ExportStats {
        notes: manifest.notes.len(),
        folders: folder_names.len(),
        tags: used_tags.len(),
        images: images.len(),
        images_missing: missing,
    })
}

fn write_zip_str(
    zip: &mut zip::ZipWriter<std::fs::File>,
    name: &str,
    text: &str,
    opts: zip::write::SimpleFileOptions,
) -> Result<(), String> {
    zip.start_file(name, opts).map_err(|e| e.to_string())?;
    zip.write_all(text.as_bytes()).map_err(|e| e.to_string())
}

// ---------- 导入 ----------

/// 从 zip 导入速记。**只追加**，重复的（标题 + 创建时间相同）跳过。
///
/// 顺序是有讲究的：图片 → 文件夹 → 笔记。图片先落盘，正文改写回协议 URL 后
/// 打开就能显示；文件夹先建好，笔记的 folder_id 才不会落到「未归类」。
pub fn import_from_zip(conn: &Connection, src: &Path) -> Result<ImportStats, String> {
    if !src.is_file() {
        return Err("文件不存在或不是文件".into());
    }
    let file = std::fs::File::open(src).map_err(|e| format!("打开文件失败: {e}"))?;
    let mut archive =
        zip::ZipArchive::new(file).map_err(|e| format!("压缩包无效或已损坏: {e}"))?;

    let mut stats = ImportStats::default();

    // 1) manifest —— 没有它就不是本格式的包，先拒再做任何写入
    let manifest: Manifest = {
        let mut entry = archive
            .by_name(MANIFEST)
            .map_err(|_| "这不是 m-hub 导出的速记包（缺少 manifest.json）")?;
        let mut s = String::new();
        entry
            .read_to_string(&mut s)
            .map_err(|e| format!("读取 manifest.json 失败: {e}"))?;
        serde_json::from_str(&s).map_err(|e| format!("manifest.json 解析失败: {e}"))?
    };
    if manifest.format != FORMAT {
        return Err(format!("不支持的包格式「{}」", manifest.format));
    }
    if manifest.version > FORMAT_VERSION {
        return Err(format!(
            "这个包由更新版本的 m-hub 导出（格式 v{}），当前版本不支持导入",
            manifest.version
        ));
    }

    // 2) 图片：整块读进内存再写（≤10MB/张），写完校验长度 —— 写半截的图片
    //    会以内容哈希命名永久留在盘上，永远没人再覆盖它（约定 72）。
    stats.images = extract_images(&mut archive)?;

    // 3) 文件夹：父在前（按层级排序），已有的一律复用
    let mut folder_ids: HashMap<String, i64> = note_folder::list(conn)
        .map_err(|e| format!("读取文件夹失败: {e}"))?
        .into_iter()
        .map(|f| (f.name, f.id))
        .collect();
    let mut wanted: Vec<&String> = manifest.folders.iter().collect();
    wanted.sort_by_key(|p| p.matches('/').count());
    for path in wanted {
        ensure_folder(conn, path, &mut folder_ids, &mut stats)?;
    }

    // 4) 标签：名字 → id 的现成映射，新建的就地补进去并计数
    let mut tag_ids: HashMap<String, i64> = tag::list(conn)
        .map_err(|e| format!("读取标签失败: {e}"))?
        .into_iter()
        .map(|t| (t.name, t.id))
        .collect();

    for e in &manifest.notes {
        // 笔记可能指向 manifest.folders 里没有的路径（手改过的包）：照样建出来，
        // 否则它会静默落进「未归类」，而用户在包里明明看到它有文件夹。
        let folder_id = match &e.folder {
            Some(p) => Some(ensure_folder(conn, p, &mut folder_ids, &mut stats)?),
            None => None,
        };

        let title = if e.title.trim().is_empty() {
            "无标题笔记"
        } else {
            e.title.trim()
        };
        // 判重只看「标题 + 创建时间」：这两样是导出包里最不容易被改动的身份。
        // （比对正文太脆 —— 重新导出一次正文完全相同，但用户改过标题就不该算重复。）
        let dup: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM notes WHERE title = ?1 AND created_at = ?2",
                rusqlite::params![title, e.created_at],
                |r| r.get(0),
            )
            .map_err(|e| format!("判重查询失败: {e}"))?;
        if dup > 0 {
            stats.skipped += 1;
            continue;
        }

        let content = swap_image_urls(&e.content, false).0;
        let n: Note = note::import(conn, title, &content, folder_id, &e.icon, &e.created_at, &e.updated_at)
            .map_err(|e| format!("写入笔记「{title}」失败: {e}"))?;

        let mut ids: Vec<i64> = Vec::new();
        for name in &e.tags {
            let name = name.trim();
            if name.is_empty() {
                continue;
            }
            if let Some(id) = tag_ids.get(name) {
                ids.push(*id);
                continue;
            }
            let t = tag::create(conn, name).map_err(|e| format!("创建标签「{name}」失败: {e}"))?;
            tag_ids.insert(t.name.clone(), t.id);
            ids.push(t.id);
            stats.tags += 1;
        }
        if !ids.is_empty() {
            tag::set_note_tags(conn, n.id, &ids)
                .map_err(|e| format!("写入标签失败: {e}"))?;
        }
        stats.notes += 1;
    }

    Ok(stats)
}

/// 确保 `path`（全路径）这棵链路上每个文件夹都存在，返回它的 id。
/// 已存在的一律复用，不新建、不改排序 —— 导入不该动用户现有的文件夹。
fn ensure_folder(
    conn: &Connection,
    path: &str,
    known: &mut HashMap<String, i64>,
    stats: &mut ImportStats,
) -> Result<i64, String> {
    if let Some(id) = known.get(path) {
        return Ok(*id);
    }
    let mut parent = String::new();
    for (i, seg) in path.split('/').enumerate() {
        if seg.trim().is_empty() {
            return Err(format!("包里的文件夹路径不合法：「{path}」"));
        }
        let full = if i == 0 { seg.to_string() } else { format!("{parent}/{seg}") };
        if !known.contains_key(&full) {
            let f = note_folder::create(conn, &parent, seg)
                .map_err(|e| format!("创建文件夹「{full}」失败: {e}"))?;
            known.insert(full.clone(), f.id);
            stats.folders += 1;
        }
        parent = full;
    }
    known
        .get(path)
        .copied()
        .ok_or_else(|| format!("内部错误：文件夹「{path}」建完却查不到 id"))
}

/// 把包里 `images/` 下的图片落到数据根 `notes/images/`。返回**新写入**的张数。
fn extract_images<R: Read + std::io::Seek>(
    archive: &mut zip::ZipArchive<R>,
) -> Result<usize, String> {
    let dir = images_dir();
    let mut written = 0usize;
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).map_err(|e| e.to_string())?;
        if entry.is_dir() {
            continue;
        }
        // enclosed_name 防 zip slip（`..` / 绝对路径），非法条目直接跳过
        let Some(rel) = entry.enclosed_name() else { continue };
        let rel_str = rel.to_string_lossy().replace('\\', "/");
        let Some(name) = rel_str.strip_prefix(IMAGES_DIR) else { continue };
        if name.contains('/') || !is_image_name(name) {
            continue;
        }
        let dst = dir.join(name);
        if dst.is_file() {
            continue; // 内容哈希命名 ⇒ 已存在即同内容
        }
        let mut buf = Vec::new();
        entry
            .read_to_end(&mut buf)
            .map_err(|e| format!("读取图片 {name} 失败: {e}"))?;
        if buf.is_empty() {
            continue;
        }
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        std::fs::write(&dst, &buf).map_err(|e| format!("保存图片 {name} 失败: {e}"))?;
        let written_len = std::fs::metadata(&dst).map(|m| m.len()).unwrap_or(0);
        if written_len != buf.len() as u64 {
            let _ = std::fs::remove_file(&dst);
            return Err(format!("保存图片 {name} 失败：写入的字节数不完整"));
        }
        written += 1;
    }
    Ok(written)
}

const README_TEXT: &str = "\
m-hub 速记导出包
================

manifest.json   导入用的数据源（文件夹 / 标签 / 图标 / 时间戳都在这里）
notes/          每篇笔记一份 Markdown 副本，按文件夹分目录，给人读的
images/         笔记正文内嵌的图片（正文里的地址已改写成 images/<文件名>）

导入须知
--------
· 导入读的是 manifest.json，不读 notes/ 下的 .md —— 手改 .md 再导入不会生效。
· 导入是追加：同一篇（标题 + 创建时间相同）会被跳过，不会覆盖现有笔记。
· 回收站里的笔记不在这个包里。
";

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::init_in_memory;

    fn tmp_zip(tag: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "m-hub-notes-port-{}-{tag}.zip",
            std::process::id()
        ))
    }

    /// 导出 → 导入到一个全新库，逐项对上：文件夹路径、标签、图标、时间戳、正文。
    /// 再导一次必须全部判重跳过（重复导入不是事故）。
    #[test]
    fn round_trip_preserves_everything_and_is_idempotent() {
        let src = init_in_memory().unwrap();
        let top = note_folder::create(&src, "", "工作").unwrap();
        let leaf = note_folder::create(&src, "工作", "会议").unwrap();
        assert_eq!(top.name, "工作");
        assert_eq!(leaf.name, "工作/会议");

        let n = note::create(&src, "周报").unwrap();
        note::update(&src, n.id, "周报", "本周做了两件事。")
            .unwrap();
        note::move_to_folder(&src, n.id, Some(leaf.id)).unwrap();
        let created = note::get(&src, n.id).unwrap().created_at;
        let t = tag::create(&src, "工作").unwrap();
        tag::set_note_tags(&src, n.id, &[t.id]).unwrap();

        // 顶层（未归类）也放一篇，包里必须有它的去处
        let loose = note::create(&src, "随手记").unwrap();
        note::update(&src, loose.id, "随手记", "散的").unwrap();

        let path = tmp_zip("rt");
        let s = export_to_zip(&src, &path).unwrap();
        assert_eq!(s.notes, 2, "回收站为空，两篇都要导出");
        assert_eq!(s.folders, 2);
        assert_eq!(s.tags, 1);
        assert_eq!(s.images, 0);
        assert_eq!(s.images_missing, 0);

        let dst = init_in_memory().unwrap();
        let s2 = import_from_zip(&dst, &path).unwrap();
        assert_eq!(s2.notes, 2, "{}", serde_json::to_string(&s2).unwrap());
        assert_eq!(s2.skipped, 0);
        assert_eq!(s2.folders, 2, "两个文件夹都要建出来");
        assert_eq!(s2.tags, 1);

        let got = note::list(&dst).unwrap();
        assert_eq!(got.len(), 2);
        let by_title = |t: &str| got.iter().find(|x| x.title == t).unwrap().clone();

        let a = by_title("周报");
        assert_eq!(a.content, "本周做了两件事。");
        assert_eq!(a.created_at, created, "导入不许改动创建时间");
        let dst_folder_path = a
            .folder_id
            .and_then(|id| note_folder::list(&dst).unwrap().into_iter().find(|f| f.id == id))
            .map(|f| f.name)
            .unwrap_or_default();
        assert_eq!(dst_folder_path, "工作/会议");
        let a_tags: Vec<String> = tag::tags_of_note(&dst, a.id)
            .unwrap()
            .into_iter()
            .map(|t| t.name)
            .collect();
        assert_eq!(a_tags, vec!["工作"]);

        let b = by_title("随手记");
        assert!(b.folder_id.is_none(), "未归类的导入后仍应是未归类");

        // 同一个包再导一次：全部判重跳过，库里笔记数不变
        let s3 = import_from_zip(&dst, &path).unwrap();
        assert_eq!(s3.notes, 0);
        assert_eq!(s3.skipped, 2);
        assert_eq!(note::list(&dst).unwrap().len(), 2);
        assert_eq!(s3.folders, 0, "文件夹已存在就不该再建一遍");

        let _ = std::fs::remove_file(&path);
    }

    /// 回收站里的笔记不进包 —— 否则「删掉的笔记」会在导入时复活。
    #[test]
    fn export_skips_trash() {
        let src = init_in_memory().unwrap();
        let keep = note::create(&src, "留下").unwrap();
        let gone = note::create(&src, "删掉").unwrap();
        note::trash(&src, gone.id).unwrap();
        assert!(note::get(&src, keep.id).is_ok());

        let path = tmp_zip("trash");
        let s = export_to_zip(&src, &path).unwrap();
        assert_eq!(s.notes, 1);
        let dst = init_in_memory().unwrap();
        import_from_zip(&dst, &path).unwrap();
        let titles: Vec<String> = note::list(&dst).unwrap().into_iter().map(|n| n.title).collect();
        assert_eq!(titles, vec!["留下"]);
        let _ = std::fs::remove_file(&path);
    }

    /// 图片地址改写必须是**可逆**的：导出改相对、导入改回协议 URL，来回一致。
    /// 同时守住「非本格式的 `images/xxx` 不许被改写」。
    #[test]
    fn image_urls_round_trip_without_false_positives() {
        let url = "http://mhub-note.localhost/0123456789abcdef.png";
        let url2 = "mhub-note://localhost/fedcba9876543210.jpg";
        let src = format!(
            "标题\n\n![]( {url} \"说明\")\n和一张 macOS 形态 {url2}\n\n不是我们的：images/照片.jpg、images/notahexname.png\n"
        );

        let (rel, found) = swap_image_urls(&src, true);
        assert!(!rel.contains("mhub-note"), "{rel}");
        assert!(rel.contains(&format!("{IMAGES_DIR}0123456789abcdef.png")), "{rel}");
        assert!(rel.contains(&format!("{IMAGES_DIR}fedcba9876543210.jpg")), "{rel}");
        assert!(
            rel.contains("images/照片.jpg") && rel.contains("images/notahexname.png"),
            "非哈希命名的相对路径不是本应用的图片，不许改写：{rel}"
        );
        assert_eq!(found.len(), 2, "{found:?}");

        let (back, found2) = swap_image_urls(&rel, false);
        assert_eq!(found2.len(), 2);
        // 导入时写回的是**当前平台**的形态（`note_image_prefix`），两种形态都要能被
        // 下一次导出重新认出来 —— 这条「往返」性质比断言某个具体前缀更耐改，
        // 前缀本身由下面那条测试钉住。
        let prefix = crate::commands::note_image_prefix();
        assert!(back.contains(&format!("{prefix}0123456789abcdef.png")), "{back}");
        assert!(back.contains(&format!("{prefix}fedcba9876543210.jpg")), "{back}");
        assert!(URL_PREFIXES.contains(&prefix), "写回的形态必须是自己认得的形态");
        assert!(back.contains("images/照片.jpg"), "误伤会变成死链：{back}");
        assert_eq!(
            back.matches("mhub-note").count(),
            src.matches("mhub-note").count(),
            "协议地址数量必须一致"
        );
    }

    /// 不是本格式的包必须**在任何写入之前**被拒（写一半的导入比不导入更难收拾）。
    #[test]
    fn foreign_manifest_is_rejected_before_writes() {
        let path = tmp_zip("foreign");
        {
            let out = std::fs::File::create(&path).unwrap();
            let mut zip = zip::ZipWriter::new(out);
            let opts = zip::write::SimpleFileOptions::default();
            zip.start_file("manifest.json", opts).unwrap();
            zip.write_all(
                br#"{"format":"someone-elses","version":1,"exported_at":"","folders":[],"notes":[]}"#,
            )
            .unwrap();
            zip.finish().unwrap();
        }
        let dst = init_in_memory().unwrap();
        let err = import_from_zip(&dst, &path).unwrap_err();
        assert!(err.contains("不支持的包格式"), "{err}");
        assert_eq!(note::list(&dst).unwrap().len(), 0);
        let _ = std::fs::remove_file(&path);
    }

    /// 没有 manifest.json 的 zip（比如用户拿别的压缩包来点导入）要给出人话。
    #[test]
    fn zip_without_manifest_is_rejected() {
        let path = tmp_zip("nomanifest");
        {
            let out = std::fs::File::create(&path).unwrap();
            let mut zip = zip::ZipWriter::new(out);
            let opts = zip::write::SimpleFileOptions::default();
            zip.start_file("readme.md", opts).unwrap();
            zip.write_all(b"hi").unwrap();
            zip.finish().unwrap();
        }
        let dst = init_in_memory().unwrap();
        let err = import_from_zip(&dst, &path).unwrap_err();
        assert!(err.contains("manifest.json"), "{err}");
        let _ = std::fs::remove_file(&path);
    }

    /// 文件名安全化：平台非法字符、结尾的点/空格、纯符号标题都要落到合法名字。
    #[test]
    fn sanitize_keeps_file_names_legal() {
        assert_eq!(sanitize_segment("a/b:c*?"), "a_b_c__");
        assert_eq!(sanitize_segment("  报告.  "), "报告");
        assert_eq!(sanitize_segment("..."), "未命名");
        assert_eq!(sanitize_segment(""), "未命名");
        let long = "字".repeat(200);
        assert_eq!(sanitize_segment(&long).chars().count(), TITLE_FILE_LIMIT);
    }
}
