//! 枚举本机已安装浏览器
//!
//! - **Windows**：读注册表 `SOFTWARE\Clients\StartMenuInternet`（HKLM + HKCU，
//!   另查 Wow6432Node 兼容 32 位注册项）——系统「默认应用」列表即来源于此。
//!   每个子键为一条浏览器注册项：默认值是显示名，`shell\open\command` 是启动命令
//!   （首段引号内即 exe 路径）。按 exe 路径去重后按显示名排序。
//! - **macOS**：没有「已注册浏览器」清单，改用**能力探测**——扫 `/Applications`、
//!   `~/Applications`、`/System/Applications` 下的 `.app` 包，读 `Info.plist` 的
//!   `CFBundleURLTypes`，凡声明了 `http`/`https` scheme 的即是浏览器。这与 LaunchServices
//!   「能打开 http 链接」的判据一致，因此不会漏掉没在标准目录的用户自装浏览器。
//!
//! 两平台 `InstalledBrowser.exe` 的含义不同：Windows 是 exe 路径，macOS 是 `.app` 包路径
//! （`open -a <app> <url>` 吃的正是包路径）。前端只把它当作一个不透明标识传回，
//! 不解析内部结构，故无需跨平台统一。

use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct InstalledBrowser {
    pub name: String,
    pub exe: String,
}

#[cfg(target_os = "windows")]
pub fn list_installed() -> Vec<InstalledBrowser> {
    use std::collections::HashSet;
    use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE};
    use winreg::RegKey;

    let mut result: Vec<InstalledBrowser> = Vec::new();
    let mut seen = HashSet::new();
    for root in [HKEY_LOCAL_MACHINE, HKEY_CURRENT_USER] {
        let root = RegKey::predef(root);
        for path in [
            "SOFTWARE\\Clients\\StartMenuInternet",
            "SOFTWARE\\Wow6432Node\\Clients\\StartMenuInternet",
        ] {
            let Ok(clients) = root.open_subkey(path) else {
                continue;
            };
            for key_name in clients.enum_keys().flatten() {
                let Ok(client) = clients.open_subkey(&key_name) else {
                    continue;
                };
                let Ok(command) = client
                    .open_subkey("shell\\open\\command")
                    .and_then(|k| k.get_value::<String, _>(""))
                else {
                    continue;
                };
                let Some(exe) = extract_exe(&command) else {
                    continue;
                };
                // 注册表残留（浏览器已卸载）过滤
                if !std::path::Path::new(&exe).is_file() {
                    continue;
                }
                if !seen.insert(exe.to_lowercase()) {
                    continue;
                }
                let name = display_name(&client, &key_name, &exe);
                result.push(InstalledBrowser { name, exe });
            }
        }
    }
    result.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    result
}

#[cfg(target_os = "macos")]
pub fn list_installed() -> Vec<InstalledBrowser> {
    use std::collections::HashSet;

    let mut result: Vec<InstalledBrowser> = Vec::new();
    let mut seen = HashSet::new();
    for dir in search_dirs() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            // 只看一级：浏览器都在 Applications 顶层，Utilities 里的不算
            if path.extension().and_then(|e| e.to_str()) != Some("app") {
                continue;
            }
            let Some(info) = read_info_plist(&path) else {
                continue;
            };
            if !declares_web_scheme(&info) {
                continue;
            }
            let bundle_id = info
                .get("CFBundleIdentifier")
                .and_then(|v| v.as_string())
                .unwrap_or_default()
                .to_string();
            if !seen.insert(bundle_id.to_lowercase()) {
                continue;
            }
            let Some(name) = display_name(&path, &info) else {
                continue;
            };
            result.push(InstalledBrowser {
                name,
                exe: path.to_string_lossy().into_owned(),
            });
        }
    }
    result.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    result
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
pub fn list_installed() -> Vec<InstalledBrowser> {
    Vec::new()
}

// ---------- Windows 私有 ----------

/// 从启动命令中解析 exe 路径：`"C:\...\chrome.exe" --single-argument %1` → 引号内路径
#[cfg(target_os = "windows")]
fn extract_exe(command: &str) -> Option<String> {
    let trimmed = command.trim();
    if let Some(rest) = trimmed.strip_prefix('"') {
        let end = rest.find('"')?;
        let path = &rest[..end];
        return if path.is_empty() {
            None
        } else {
            Some(path.to_string())
        };
    }
    let path = trimmed.split_whitespace().next()?;
    if path.is_empty() {
        None
    } else {
        Some(path.to_string())
    }
}

/// 显示名：优先注册表默认值 / LocalizedString；间接字符串（@...）或缺失时按 exe 文件名兜底
#[cfg(target_os = "windows")]
fn display_name(client: &winreg::RegKey, key_name: &str, exe: &str) -> String {
    let from_registry = client
        .get_value::<String, _>("")
        .ok()
        .or_else(|| client.get_value::<String, _>("LocalizedString").ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty() && !s.starts_with('@'));
    from_registry.unwrap_or_else(|| {
        fallback_name(exe)
            .map(str::to_string)
            .unwrap_or_else(|| key_name.to_string())
    })
}

/// 常见浏览器 exe 文件名 → 显示名兜底表
#[cfg(target_os = "windows")]
fn fallback_name(exe: &str) -> Option<&'static str> {
    let file = std::path::Path::new(exe)
        .file_name()?
        .to_str()?
        .to_lowercase();
    const FALLBACKS: &[(&str, &str)] = &[
        ("chrome.exe", "Google Chrome"),
        ("msedge.exe", "Microsoft Edge"),
        ("firefox.exe", "Firefox"),
        ("brave.exe", "Brave"),
        ("vivaldi.exe", "Vivaldi"),
        ("opera.exe", "Opera"),
        ("360se.exe", "360安全浏览器"),
        ("360chrome.exe", "360极速浏览器"),
        ("qqbrowser.exe", "QQ浏览器"),
        ("sogouexplorer.exe", "搜狗浏览器"),
        ("maxthon.exe", "傲游浏览器"),
    ];
    FALLBACKS
        .iter()
        .find(|(name, _)| *name == file)
        .map(|(_, display)| *display)
}

// ---------- macOS 私有 ----------

/// 浏览器的标准安装位置。按 LaunchServices 实际查找顺序排列。
#[cfg(target_os = "macos")]
fn search_dirs() -> Vec<std::path::PathBuf> {
    let mut dirs = vec![
        std::path::PathBuf::from("/Applications"),
        std::path::PathBuf::from("/System/Applications"),
    ];
    if let Some(home) = dirs::home_dir() {
        dirs.push(home.join("Applications"));
    }
    dirs
}

/// 读 `.app` 包内的 `Contents/Info.plist`。
/// 有的包把 plist 写成二进制格式，`plist` crate 两种都能解析。
#[cfg(target_os = "macos")]
fn read_info_plist(app_path: &std::path::Path) -> Option<plist::Dictionary> {
    let plist_path = app_path.join("Contents").join("Info.plist");
    let data = std::fs::read(&plist_path).ok()?;
    let value = plist::Value::from_reader(std::io::Cursor::new(data)).ok()?;
    value.into_dictionary()
}

/// 该包是否声明了能处理 http/https 的 URL scheme。
///
/// 注意**不能**只看「有 `CFBundleURLTypes`」——大量非浏览器应用也声明了自定义 scheme
/// （邮件客户端、播放器、局域网投屏…）。必须逐项查 `CFBundleURLSchemes` 里含
/// `http` 或 `https`，这才是 LaunchServices 认「它是浏览器」的真实判据。
#[cfg(target_os = "macos")]
fn declares_web_scheme(info: &plist::Dictionary) -> bool {
    let Some(url_types) = info.get("CFBundleURLTypes").and_then(|v| v.as_array()) else {
        return false;
    };
    url_types.iter().any(|entry| {
        entry
            .as_dictionary()
            .and_then(|d| d.get("CFBundleURLSchemes"))
            .and_then(|v| v.as_array())
            .map(|schemes| {
                schemes.iter().filter_map(|s| s.as_string()).any(|s| {
                    let s = s.trim().to_ascii_lowercase();
                    s == "http" || s == "https"
                })
            })
            .unwrap_or(false)
    })
}

/// 显示名：`CFBundleDisplayName` → `CFBundleName` → 包目录名（去 `.app`）。
/// 前两个是本地化过的（`InfoPlist.strings` 未读时用 Info.plist 里的原值，够用）；
/// 都没有时回退目录名，避免出现无名条目。
#[cfg(target_os = "macos")]
fn display_name(app_path: &std::path::Path, info: &plist::Dictionary) -> Option<String> {
    for key in ["CFBundleDisplayName", "CFBundleName"] {
        if let Some(name) = info.get(key).and_then(|v| v.as_string()) {
            let trimmed = name.trim();
            if !trimmed.is_empty() {
                return Some(trimmed.to_string());
            }
        }
    }
    app_path
        .file_stem()
        .and_then(|s| s.to_str())
        .map(str::to_string)
        .filter(|s| !s.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(target_os = "windows")]
    #[test]
    fn extract_exe_parses_quoted_command() {
        assert_eq!(
            extract_exe(
                r#""C:\Program Files\Google\Chrome\Application\chrome.exe" --single-argument %1"#
            ),
            Some(r"C:\Program Files\Google\Chrome\Application\chrome.exe".to_string())
        );
        assert_eq!(
            extract_exe(r"C:\Browsers\firefox.exe %1"),
            Some(r"C:\Browsers\firefox.exe".to_string())
        );
        assert_eq!(extract_exe(r#""#), None);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn web_scheme_requires_http_or_https() {
        let mk = |schemes: Vec<&str>| {
            let entry = plist::Value::Array(
                schemes
                    .iter()
                    .map(|s| plist::Value::String((*s).into()))
                    .collect(),
            );
            let mut d = plist::Dictionary::new();
            d.insert("CFBundleURLSchemes".into(), entry);
            plist::Value::Array(vec![plist::Value::Dictionary(d)])
        };
        let info_of = |v: plist::Value| {
            let mut d = plist::Dictionary::new();
            d.insert("CFBundleURLTypes".into(), v);
            d
        };

        assert!(declares_web_scheme(&info_of(mk(vec!["https", "http"]))));
        assert!(declares_web_scheme(&info_of(mk(vec!["HTTP"]))));
        // 只有自定义 scheme 的不是浏览器（邮件客户端、投屏…）
        assert!(!declares_web_scheme(&info_of(mk(vec!["mailto", "ftp"]))));
        // 声明了 URLTypes 但没有任何 scheme
        assert!(!declares_web_scheme(&info_of(mk(vec![]))));
        // 完全没有 CFBundleURLTypes
        assert!(!declares_web_scheme(&plist::Dictionary::new()));
    }

    #[test]
    fn list_installed_runs_without_panic() {
        // 依赖机器环境（无浏览器时应返回空列表），仅保证枚举过程不 panic
        let _ = list_installed();
    }
}
