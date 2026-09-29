//! 数据根目录解析。
//!
//! 数据根是 m-hub 所有持久化数据的统一挂载点：数据库 `app.db`、配置 `app.json`、
//! 图标 `icons/`、剪贴板图片 `clipboard/`、日志 `logs/`、`chat_keys.json` 全部位于其下。
//!
//! 标准版与便携版的判定互不串扰：
//!   - 便携版（可执行文件同级有 `portable` 标志）：数据**固定**跟随
//!     `<可执行文件所在目录>/data/`，不支持改路径（绝对路径无法跨电脑迁移，
//!     改路径会破坏"随身带"的语义）；
//!   - 标准版（无标志）：数据默认落在系统的「用户配置目录」，可在设置中改到任意目录，
//!     改过的路径记录在同级的 `data_path.json`。
//!
//! 各平台的实际位置（`dirs::config_dir()` 的取值）：
//!
//! | 平台 | 默认数据根 |
//! |---|---|
//! | macOS | `~/Library/Application Support/m-hub` |
//! | Windows | `%APPDATA%\m-hub` |
//! | Linux | `~/.config/m-hub` |
//!
//! ⚠️ **便携版在 macOS 上的含义变了**：macOS 的应用必须是 `/Applications/X.app` 包
//! 才能正常从 Dock/Finder 启动，「把 .app 连同 data 拷到 U 盘」得到的副本
//! LaunchServices 不认（双击不启动）。所以 macOS 上的 `portable` 标志只对
//! 「就地放在某个自选目录、允许从命令行用 `open /path/to/m-hub.app` 启动」的用法
//! 有意义；**要真正随身携带应分发完整 .app 包**（.app 本身是自包含的，
//! 双击即用，不依赖安装位置）。

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// 便携标志文件名：可执行文件同级存在该文件即启用便携版
pub const PORTABLE_MARKER: &str = "portable";

/// 便携版数据子目录名（数据落在可执行文件目录下的 data/ 内）
pub const PORTABLE_DATA_DIR: &str = "data";

/// 引导文件名（记录用户改过的数据根路径；仅标准版使用，位于固定锚点目录）
const BOOTSTRAP_FILE: &str = "data_path.json";

static DATA_ROOT: OnceLock<PathBuf> = OnceLock::new();

/// 默认数据根（标准版，见上表）
pub fn default_data_root() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("m-hub")
}

/// 标准版引导文件锚点：`<config_dir>/m-hub/data_path.json`
fn bootstrap_file() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("m-hub")
        .join(BOOTSTRAP_FILE)
}

/// exe 所在目录
fn exe_dir() -> Option<PathBuf> {
    std::env::current_exe()
        .ok()?
        .parent()
        .map(|p| p.to_path_buf())
}

/// 是否便携版：exe 同目录存在 portable 标志文件
pub fn is_portable() -> bool {
    exe_dir()
        .map(|d| d.join(PORTABLE_MARKER).exists())
        .unwrap_or(false)
}

/// 读取指定引导文件记录的绝对路径；文件不存在或非绝对路径时返回 None
fn read_bootstrap_at(file: &Path) -> Option<PathBuf> {
    let content = std::fs::read_to_string(file).ok()?;
    let p = content.trim();
    if p.is_empty() {
        return None;
    }
    let path = PathBuf::from(p);
    if path.is_absolute() { Some(path) } else { None }
}

/// 解析数据根（不缓存；供惰性初始化与测试使用）
pub fn resolve_data_root() -> PathBuf {
    // 1. 便携版：exe 旁有标志 → 数据固定跟随 exe\data（忽略任何改路径记录）
    if let Some(dir) = exe_dir() {
        if dir.join(PORTABLE_MARKER).exists() {
            return dir.join(PORTABLE_DATA_DIR);
        }
    }

    // 2. 标准版：用户改过的路径（%APPDATA% 引导文件，非默认值）
    if let Some(path) = read_bootstrap_at(&bootstrap_file()) {
        if path != default_data_root() {
            return path;
        }
    }

    // 3. 默认：用户级目录，并首次初始化引导文件
    let default = default_data_root();
    let _ = write_bootstrap_at(&bootstrap_file(), &default);
    default
}

/// 数据根（惰性解析并缓存，进程内只解析一次）
pub fn data_root() -> &'static Path {
    DATA_ROOT.get_or_init(resolve_data_root).as_path()
}

/// 数据路径信息：`(路径, 模式)`，模式 = default / custom / portable
pub fn data_path_info() -> (String, &'static str) {
    let root = data_root();
    // 便携版：固定跟随 exe\data
    if is_portable() {
        return (root.to_string_lossy().into_owned(), "portable");
    }
    // 标准版：引导文件非默认 → 用户改过
    if let Some(path) = read_bootstrap_at(&bootstrap_file()) {
        if path != default_data_root() {
            return (root.to_string_lossy().into_owned(), "custom");
        }
    }
    (root.to_string_lossy().into_owned(), "default")
}

/// 剥掉 Windows verbatim（`\\?\`）前缀，返回等价的可交付路径。
///
/// **为什么必须有这个函数**：`std::fs::canonicalize` 在 Windows 上返回 verbatim 形式
/// （`\\?\A:\m-hub\publish-src\lan-share`）。Rust 自己的 fs 调用接受它，但**一交给外部程序就出事**：
/// Node 的 CJS 加载器处理不了这个前缀，会把 `\\?\A:\…\backend\server.js` 拆错，
/// 跑去 `lstat` 盘符 `A:` 得 `EISDIR` 直接 `exit 1`（service 扩展后端「静默起不来」的根因）；
/// `ShellExecute`（`opener::open` / `explorer`）同样不认。所以凡是**要落盘、要展示、
/// 要交给外部进程**的路径都必须先过这里。
///
/// 保守策略（与 `dunce::simplified` 同口径，只有确知语义不变才剥）：
///   - `\\?\C:\...`（盘符形式）→ `C:\...`；
///   - `\\?\UNC\server\share\...` → `\\server\share\...`；
///   - 剥完含 `.` / `..` 组件的不剥（Win32 会规范化这些组件，剥了语义就变了）；
///   - `\\?\Volume{...}` / `\\?\GLOBALROOT` 等设备路径不剥。
/// 非 Windows 平台原样返回。
pub fn simplify_path(p: &Path) -> PathBuf {
    if !cfg!(windows) {
        return p.to_path_buf();
    }
    let s = p.to_string_lossy();
    let stripped = if let Some(rest) = s.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{rest}")
    } else if let Some(rest) = s.strip_prefix(r"\\?\") {
        // 仅盘符形式（`X:` 开头）可剥；`Volume{...}` 等设备路径保持原样
        let mut chars = rest.chars();
        match (chars.next(), chars.next()) {
            (Some(drive), Some(':')) if drive.is_ascii_alphabetic() => rest.to_string(),
            _ => return p.to_path_buf(),
        }
    } else {
        return p.to_path_buf();
    };
    let out = PathBuf::from(&stripped);
    if out
        .components()
        .any(|c| matches!(c, std::path::Component::CurDir | std::path::Component::ParentDir))
    {
        return p.to_path_buf();
    }
    out
}

/// [`simplify_path`] 的存在性安全网：简化后的路径**确实可访问**才采用，否则原样返回。
/// 兜住极端情形——总长超过 260 且系统未开启长路径支持时，verbatim 是唯一能访问的形式。
pub fn simplify_existing(p: &Path) -> PathBuf {
    let s = simplify_path(p);
    if s.as_path() != p && s.exists() {
        s
    } else {
        p.to_path_buf()
    }
}

/// 更新引导文件指向新的数据根（仅标准版改路径时调用，重启后生效）
pub fn set_data_root(path: &Path) -> Result<(), String> {
    write_bootstrap_at(&bootstrap_file(), path)
}

/// 原子写入引导文件（临时文件 + rename）
fn write_bootstrap_at(file: &Path, value: &Path) -> Result<(), String> {
    if let Some(parent) = file.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let tmp = file.with_extension("json.tmp");
    std::fs::write(&tmp, value.to_string_lossy().as_bytes()).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, file).map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_root_is_absolute() {
        assert!(default_data_root().is_absolute());
    }

    #[test]
    fn portable_detection_returns_bool() {
        // 仅验证函数可调用且返回布尔值（测试二进制目录下通常无 portable 标记）
        let _ = is_portable();
    }

    #[test]
    fn simplify_strips_verbatim_prefix() {
        if !cfg!(windows) {
            // 非 Windows 平台一律原样返回
            assert_eq!(
                simplify_path(Path::new("/tmp/m-hub/ext")),
                PathBuf::from("/tmp/m-hub/ext")
            );
            return;
        }
        // 盘符形式：canonicalize 在 Windows 上的真实产物
        assert_eq!(
            simplify_path(Path::new(r"\\?\A:\m-hub\publish-src\1.3.0\lan-share")),
            PathBuf::from(r"A:\m-hub\publish-src\1.3.0\lan-share")
        );
        // UNC 形式：还原成 `\\server\share`
        assert_eq!(
            simplify_path(Path::new(r"\\?\UNC\srv\share\ext")),
            PathBuf::from(r"\\srv\share\ext")
        );
        // 普通路径不动
        assert_eq!(
            simplify_path(Path::new(r"C:\m-hub\extensions\com.a.b")),
            PathBuf::from(r"C:\m-hub\extensions\com.a.b")
        );
        // 设备路径不剥（剥了会变成不存在的盘符路径）
        assert_eq!(
            simplify_path(Path::new(r"\\?\Volume{8f0e1c2a}\x")),
            PathBuf::from(r"\\?\Volume{8f0e1c2a}\x")
        );
        // 含 `..` 组件不剥：Win32 会规范化，语义会变
        assert_eq!(
            simplify_path(Path::new(r"\\?\C:\a\..\b")),
            PathBuf::from(r"\\?\C:\a\..\b")
        );
    }

    #[test]
    fn simplify_existing_keeps_usable_path() {
        let dir = tempfile::tempdir().unwrap();
        let canon = dir.path().canonicalize().unwrap();
        if cfg!(windows) {
            assert!(
                canon.to_string_lossy().starts_with(r"\\?\"),
                "前提不成立：Windows 的 canonicalize 应当返回 verbatim 形式"
            );
        }
        let plain = simplify_existing(&canon);
        assert!(plain.is_dir(), "简化后的路径必须仍可访问");
        assert!(!plain.to_string_lossy().starts_with(r"\\?\"));
        // 已简化 / 不存在的路径原样返回
        assert_eq!(simplify_existing(&plain), plain);
        let ghost = canon.join("__not_here__");
        assert_eq!(simplify_existing(&ghost), ghost);
    }
}
