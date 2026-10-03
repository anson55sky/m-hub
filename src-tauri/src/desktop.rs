// 扫描桌面：把桌面上的快捷方式 / 文件 / 文件夹挑着加入速达。
//
// ## 为什么只删「快捷方式」
//
// 用户勾了「加入后清理桌面」之后，桌面上的东西**不能**跟着消失 ——
// 那是用户的文件，不是速达的记录。所以本模块把每一条都标了
// `removable`：**只有快捷方式**为 true。
//
// macOS 上的「快捷方式」有两类：
// · `.alias` —— Finder 替身（双击跳到目标），删掉不影响目标
// · `.webloc` —— 网页快捷方式（一个 plist），删掉不影响任何东西
//
// 而桌面上的 `.app`、普通文件夹、文档**都不是快捷方式**：删掉它们等于
// 删用户的文件。所以哪怕用户勾了清理，这些也会被原样保留，并在返回结果里
// 带 `removable: false` 供界面说明。
//
// 还有一个容易漏的点：**软链接也是快捷方式**，但 `symlink_metadata` 才能
// 判出来 —— `metadata()` 会跟着链接走到目标上去（于是桌面上的
// 「指向文稿夹的软链接」会被误判成文件夹，`removable` 变 false）。
use std::path::{Path, PathBuf};

use serde::Serialize;

const MAX_ENTRIES: usize = 500;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DesktopEntry {
    pub id: String,
    /// 显示名（不带扩展名）
    pub name: String,
    /// 绝对路径
    pub path: String,
    /// `app` / `folder` / `file` / `alias` / `webloc`
    pub kind: String,
    /// 可否作为速达条目加入（`.desktop` 之类的隐藏系统文件不行）
    pub addable: bool,
    /// 可否在加入后**从桌面删掉**（只有快捷方式为 true）
    pub removable: bool,
}

/// 桌面目录。
///
/// ⚠️ 不写死 `~/Desktop`：macOS 允许在 Finder 里改桌面位置
///   （系统设置 → 桌面与程序坞，或 iCloud「桌面与文稿」开启后桌面变成
///   `~/Library/Mobile Documents/com~apple~CloudDocs/Desktop`）。
///   写死的话，在改过位置或开了 iCloud 的机器上扫出来是空的 ——
///   而症状是「扫不到东西」，用户只会以为桌面上真的什么都没有。
///
/// 判据顺序：
///   ① `$HOME/Desktop` 存在就用它
///   ② 否则找 `~/Library/Mobile Documents/com~apple~CloudDocs/Desktop`
///      （iCloud「桌面与文稿」开启时的实际位置）
fn desktop_dir() -> Option<PathBuf> {
    let home = std::env::var_os("HOME")?;
    let candidates = [
        PathBuf::from(&home).join("Desktop"),
        PathBuf::from(&home)
            .join("Library")
            .join("Mobile Documents")
            .join("com~apple~CloudDocs")
            .join("Desktop"),
    ];
    candidates.into_iter().find(|p| p.is_dir())
}

#[tauri::command]
pub async fn scan_desktop() -> Result<Vec<DesktopEntry>, String> {
    let Some(dir) = desktop_dir() else {
        return Err("找不到桌面目录（$HOME/Desktop 不存在）".into());
    };
    let read = tokio::task::spawn_blocking(move || scan_dir(&dir))
        .await
        .map_err(|e| e.to_string())??;
    Ok(read)
}

fn scan_dir(dir: &Path) -> Result<Vec<DesktopEntry>, String> {
    let rd = std::fs::read_dir(dir).map_err(|e| e.to_string())?;
    let mut out: Vec<DesktopEntry> = Vec::new();
    for e in rd.flatten() {
        if out.len() >= MAX_ENTRIES {
            break;
        }
        let path = e.path();
        let name_os = e.file_name();
        let name = name_os.to_string_lossy().into_owned();

        // 隐藏文件（`.DS_Store`、`.localized`、`.VolumeIcon.icns`）不列
        if name.starts_with('.') {
            continue;
        }
        // 扩展名转小写再比：`.ALIAS` / `.WEBLOC` 在某些系统上是可能的
        let ext = Path::new(&name)
            .extension()
            .map(|x| x.to_string_lossy().to_ascii_lowercase())
            .unwrap_or_default();

        // ⚠️ `symlink_metadata` 而不是 `metadata`：后者会**跟着软链接走到目标**，
        //   于是指向文件夹的软链接被误判成「文件夹」→ removable 变 false →
        //   用户勾了清理却没清掉，而且界面显示的是「文件夹」，看不出为什么。
        let meta = match std::fs::symlink_metadata(&path) {
            Ok(m) => m,
            Err(_) => continue,
        };
        let is_symlink = meta.file_type().is_symlink();

        let (kind, removable, addable) = if ext == "alias" {
            ("alias", true, true)
        } else if ext == "webloc" {
            ("webloc", true, true)
        } else if is_symlink {
            // 指向文件的软链接也算快捷方式；指向目录的算文件夹更贴近直觉
            let target_is_dir = std::fs::metadata(&path).map(|m| m.is_dir()).unwrap_or(false);
            (if target_is_dir { "folder" } else { "alias" }, true, true)
        } else if meta.is_dir() {
            ("folder", false, true)
        } else if ext == "app" {
            ("app", false, true)
        } else {
            ("file", false, true)
        };

        out.push(DesktopEntry {
            id: format!("{}:{}", dir.display(), name),
            name: name.trim_end_matches(".alias").trim_end_matches(".webloc").to_string(),
            path: crate::paths::simplify_path(&path).to_string_lossy().into_owned(),
            kind: kind.to_string(),
            addable,
            removable,
        });
    }
    // 排序稳定（同名项目在不同文件系统顺序里可能不同，而 UI 的「全选」会按序追加）
    out.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    Ok(out)
}

/// 删除桌面上的快捷方式。
///
/// ⚠️ **再次校验 removable**：界面上那个勾是「我认为它可删」，
///   而这条命令可能来自任何调用方。所以这里独立再判一次 ——
///   这是唯一拦住「删掉用户文件」的地方，绝不能只靠界面上的复选框。
/// 「这条路径允许删吗」的**纯**判断。抽出来是为了能直接测它。
///
/// ⚠️ 这是整个功能里唯一拦住「删掉用户文件」的地方。它必须是纯函数且被测试
///   直接覆盖 —— 第一版它内联在 `#[tauri::command]` 里，而所有测试都只测
///   `scan_dir`，于是「把 `removable` 直接写成 `true`」这个变异**测试全绿**。
///   命令体只负责取参数、调用它、执行删除。
fn can_remove_desktop_path(
    path: &Path,
    meta: &std::fs::Metadata,
    desktop: &Path,
) -> Result<(), String> {
    // 三道判据**按「提示的具体程度」排序**，不是按安全性排序 ——
    // 三道都拒绝，安全性不受顺序影响，而用户看到的是**第一条**。
    // 放错顺序的后果很具体：`.DS_Store` 不是快捷方式，会被第①道拦下并提示
    // 「只删快捷方式」—— 而真实原因是「这是隐藏文件」（第一版就是这样，
    // 测试直接暴露了它）。最具体的那条放最前，用户才看得到真正的原因。

    // ① 隐藏文件：`.DS_Store` / `.localized` 之类被误传进来
    let short = path
        .file_name()
        .map(|x| x.to_string_lossy().to_string())
        .unwrap_or_default();
    if short.starts_with('.') {
        return Err("不删隐藏文件".into());
    }
    // ② 必须直接在桌面目录下：路径参数来自外部，不能只靠界面上的复选框
    let parent = path.parent().unwrap_or(Path::new(""));
    if !same_dir(parent, desktop) {
        return Err("只能删除桌面目录下的快捷方式".into());
    }
    // ③ 必须是快捷方式：`.alias` / `.webloc` / 软链接
    let ext = path
        .extension()
        .map(|x| x.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    let is_shortcut = ext == "alias" || ext == "webloc" || meta.file_type().is_symlink();
    if !is_shortcut {
        return Err("只删快捷方式（.alias / .webloc / 软链接），文件与文件夹不动".into());
    }
    Ok(())
}

/// 取「判定用的」元数据。
///
/// ⚠️ 必须是 `symlink_metadata`：`metadata()` 会**跟着软链接走到目标**，
///   于是真快捷方式拿到的 `file_type().is_symlink()` 是 false → 被守卫拒掉。
///   症状是「桌面上的软链接点清理，提示只删快捷方式」—— 一个让人以为
///   桌面坏了的假象，而它不会伤到任何文件（漏判，不是危险）。
///
/// 单独抽出来是为了能测：第一版这句写在命令体里，测试全都直接构造
/// `symlink_metadata` 的结果，于是「把它换成 `metadata()`」**测试全绿**。
fn load_meta_for_removal(path: &Path) -> Result<std::fs::Metadata, String> {
    std::fs::symlink_metadata(path).map_err(|e| format!("找不到：{e}"))
}

#[tauri::command]
pub async fn remove_desktop_shortcut(path: String) -> Result<bool, String> {
    let p = PathBuf::from(&path);
    let meta = load_meta_for_removal(&p)?;
    let Some(desktop) = desktop_dir() else {
        return Err("找不到桌面目录".into());
    };
    can_remove_desktop_path(&p, &meta, &desktop)?;
    std::fs::remove_file(&p).map_err(|e| e.to_string())?;
    Ok(true)
}

#[cfg(target_os = "windows")]
fn same_dir(a: &Path, b: &Path) -> bool {
    a == b
}

/// macOS 上 `/Users/sky/Desktop` 与 `/Users/sky/Desktop/` 是同一个目录，
/// 而 macOS 默认文件系统**大小写不敏感** —— 直接 `==` 会让
/// 「路径大小写对不上」被当成「不在桌面目录下」，于是命令永远拒绝。
/// 这里用 `fs::canonicalize` 归一（解析软链接 + 拿到系统认可的大小写）。
#[cfg(target_os = "macos")]
fn same_dir(a: &Path, b: &Path) -> bool {
    match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        (Ok(x), Ok(y)) => x == y,
        _ => a == b,
    }
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn same_dir(a: &Path, b: &Path) -> bool {
    a == b
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn desktop_dir_is_resolved_not_hardcoded() {
        // 只断言「能找到某个目录」，不断言具体路径 ——
        // iCloud「桌面与文稿」开启时它就在 Mobile Documents 下
        let d = desktop_dir().expect("开发机上应能解析出桌面目录");
        assert!(d.is_dir());
    }

    #[test]
    fn hidden_files_are_skipped() {
        let dir = tempfile::tempdir().unwrap();
        for name in [".DS_Store", ".localized", "可见文件.txt"] {
            std::fs::write(dir.path().join(name), b"x").unwrap();
        }
        let got = scan_dir(dir.path()).unwrap();
        assert_eq!(got.len(), 1, "{got:?}");
        assert_eq!(got[0].name, "可见文件.txt");
    }

    /// 核心安全性质：**只有快捷方式 removable=true**。
    ///
    /// ⚠️ 这一条守的是「勾了清理之后会不会删掉用户的文件」。判据是
    ///   `.alias` / `.webloc` / 软链接三类；普通文件、文件夹、`.app`
    ///   一律 removable=false —— 哪怕界面上那个复选框被勾了。
    #[test]
    fn only_shortcuts_are_removable() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path();

        std::fs::write(p.join("普通文件.txt"), b"x").unwrap();
        std::fs::create_dir_all(p.join("普通文件夹")).unwrap();
        std::fs::write(p.join("别名.alias"), b"x").unwrap();
        std::fs::write(p.join("网页.webloc"), b"x").unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(p.join("普通文件.txt"), p.join("指向文件的链接")).unwrap();

        let got = scan_dir(p).unwrap();
        let by = |n: &str| {
            got.iter()
                .find(|e| e.name == n)
                .unwrap_or_else(|| panic!("没扫到 {n}：{got:?}"))
        };

        assert!(by("普通文件.txt").removable == false, "普通文件绝不能可删");
        assert!(by("普通文件夹").removable == false, "文件夹绝不能可删");
        assert!(by("别名").removable, ".alias 是快捷方式");
        assert!(by("网页").removable, ".webloc 是快捷方式");
        #[cfg(unix)]
        assert!(by("指向文件的链接").removable, "软链接也是快捷方式");

        // 所有条目都应可加入速达（除了未来可能出现的类型）
        assert!(got.iter().all(|e| e.addable));
    }

    /**
     * 指向**文件夹**的软链接必须判成 `folder` 而不是 `alias`。
     *
     * ⚠️ 这条要用 `symlink_metadata` 才判得对：用 `metadata()` 会跟着链接
     *   走到目标，于是指向文件夹的链接被当成文件夹 → removable 变 false →
     *   用户勾了「清理桌面」却没清掉，界面显示「文件夹」，看不出为什么。
     */
    #[cfg(unix)]
    #[test]
    fn symlink_to_folder_is_a_folder() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path();
        std::fs::create_dir_all(p.join("目标文件夹")).unwrap();
        std::os::unix::fs::symlink(p.join("目标文件夹"), p.join("指向文件夹的链接")).unwrap();

        let got = scan_dir(p).unwrap();
        let link = got.iter().find(|e| e.name == "指向文件夹的链接").unwrap();
        assert_eq!(link.kind, "folder");
        assert!(link.removable, "软链接本身仍可删 —— 删的是链接，不是目标");
    }

    #[test]
    fn output_order_is_stable() {
        let dir = tempfile::tempdir().unwrap();
        for n in ["c.txt", "a.txt", "b.txt"] {
            std::fs::write(dir.path().join(n), b"x").unwrap();
        }
        let first = scan_dir(dir.path()).unwrap();
        let second = scan_dir(dir.path()).unwrap();
        assert_eq!(first.len(), 3);
        assert_eq!(
            first.iter().map(|e| &e.name).collect::<Vec<_>>(),
            vec!["a.txt", "b.txt", "c.txt"],
            "应按名称排序且与 readdir 顺序无关"
        );
        assert_eq!(second[0].id, first[0].id, "id 要稳定：全选操作按 id 累加");
    }

    /**
     * 守卫 `can_remove_desktop_path` 的三条判据，逐条**直接**测。
     *
     * ⚠️ 这组测试是被「变异测试逼出来的」：第一版守卫内联在命令体里、
     *   测试只覆盖 `scan_dir`，于是把 `removable` 直接写成 `true`
     *   （= 任何文件都能删）**测试全绿** —— 一条守着重��安全性的守卫
     *   当时等于不存在。抽出纯函数 + 直测才把它变成真守卫。
     */
    #[test]
    fn guard_refuses_plain_files_folders_and_apps() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path();
        std::fs::write(p.join("文档.txt"), b"x").unwrap();
        std::fs::create_dir_all(p.join("文件夹")).unwrap();

        for name in ["文档.txt", "文件夹"] {
            let full = p.join(name);
            let meta = std::fs::symlink_metadata(&full).unwrap();
            let err = can_remove_desktop_path(&full, &meta, p).unwrap_err();
            assert!(err.contains("只删快捷方式"), "{name} 应被拒：{err}");
        }
    }

    #[test]
    fn guard_allows_alias_webloc_and_symlinks_in_desktop() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path();
        std::fs::write(p.join("a.alias"), b"x").unwrap();
        std::fs::write(p.join("b.webloc"), b"x").unwrap();

        for name in ["a.alias", "b.webloc"] {
            let full = p.join(name);
            let meta = std::fs::symlink_metadata(&full).unwrap();
            assert!(
                can_remove_desktop_path(&full, &meta, p).is_ok(),
                "{name} 是快捷方式，应当允许删"
            );
        }
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(p.join("a.alias"), p.join("c")).unwrap();
            let full = p.join("c");
            let meta = std::fs::symlink_metadata(&full).unwrap();
            assert!(can_remove_desktop_path(&full, &meta, p).is_ok(), "软链接应当允许删");
        }
    }

    /// 路径参数来自外部 —— 不能只靠界面上的复选框。
    #[test]
    fn guard_refuses_paths_outside_the_desktop_dir() {
        let desk = tempfile::tempdir().unwrap();
        let elsewhere = tempfile::tempdir().unwrap();
        let victim = elsewhere.path().join("别人的.alias");
        std::fs::write(&victim, b"x").unwrap();
        let meta = std::fs::symlink_metadata(&victim).unwrap();

        let err = can_remove_desktop_path(&victim, &meta, desk.path()).unwrap_err();
        assert!(err.contains("只能删除桌面目录下"), "{err}");
    }

    /// 判定用的元数据必须来自 `symlink_metadata` —— 见 `load_meta_for_removal` 的说明。
    /// 换成 `metadata()` 时真快捷方式会被误拒（漏判），而这一条守的就是它。
    #[cfg(unix)]
    #[test]
    fn removal_metadata_follows_symlinks_not_their_targets() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path();
        std::fs::write(p.join("目标.txt"), b"x").unwrap();
        let link = p.join("快捷方式");
        std::os::unix::fs::symlink(p.join("目标.txt"), &link).unwrap();

        let meta = load_meta_for_removal(&link).unwrap();
        assert!(
            meta.file_type().is_symlink(),
            "必须拿到链接自身的元数据；跟着目标走的话软链接会被误判成普通文件"
        );
        assert!(
            can_remove_desktop_path(&link, &meta, p).is_ok(),
            "真快捷方式必须可删"
        );
        // 对照：目标是普通文件，若拿到的是目标的元数据就会被拒
        let wrong = std::fs::metadata(&link).unwrap();
        assert!(!wrong.file_type().is_symlink());
    }

    #[test]
    fn guard_refuses_hidden_files() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path();
        let hidden = p.join(".DS_Store");
        std::fs::write(&hidden, b"x").unwrap();
        let meta = std::fs::symlink_metadata(&hidden).unwrap();
        // 即使它是软链接也不删：隐藏文件一律放过
        let err = can_remove_desktop_path(&hidden, &meta, p).unwrap_err();
        assert!(err.contains("不删隐藏文件"), "{err}");
    }

    #[test]
    fn same_dir_normalizes_trailing_slash_and_case_on_macos() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path();
        assert!(same_dir(p, p));
        let with_slash = PathBuf::from(format!("{}/", p.display()));
        assert!(same_dir(p, &with_slash), "带尾斜杠仍应判定为同一目录");
        assert!(!same_dir(p, Path::new("/")));
    }
}