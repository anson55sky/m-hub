//! 枚举本机已安装浏览器
//!
//! - **Windows**：读注册表 `SOFTWARE\Clients\StartMenuInternet`（HKLM + HKCU，
//!   另查 Wow6432Node 兼容 32 位注册项）——系统「默认应用」列表即来源于此。
//!   每个子键为一条浏览器注册项：默认值是显示名，`shell\open\command` 是启动命令
//!   （首段引号内即 exe 路径）。按 exe 路径去重后按显示名排序。
//! - **macOS**：**向 LaunchServices 要答案**
//!   （`-[NSWorkspace URLsForApplicationsToOpenURL:]`，即「哪些应用能处理这个 https URL」）
//!   ——这与「系统默认浏览器设置」那个列表同源，一个不漏。再辅以目录扫描兜底
//!   （LS 数据库偶尔漏索引手工拷贝的 `.app`）。
//!
//! ## 为什么不能只扫目录（2026-09-29 改）
//!
//! 早先的实现只扫 `/Applications`、`/System/Applications`、`~/Applications`
//! 的**一级**，并注释说「这与 LaunchServices 的判据一致，不会漏」。那句话是错的：
//! 一级扫描漏掉整个 `/Applications/Setapp/*.app`（Setapp 用户在 mac 上装机量很大，
//! 他们的 Chrome/Arc/Brave/Firefox 全在这里），也漏掉 Xcode 的
//! `…/Developer/Applications/`、Homebrew 改过 `--appdir` 的情况。
//! 症状是右键菜单里「用 XX 打开」分组**空掉或只剩 Safari**，而 `browsers.rs`
//! 的模块注释还写着「不会漏」——文档的承诺与实现相反，比不写更糟。
//!
//! ## 用的哪个 API（一个容易踩的坑）
//!
//! 直觉上会去找 `LSCopyAllHandlersForURLScheme`。**不要用**：它有两个问题，
//! ① 自 10.10 起 deprecated；② 它的签名是 `LSCopyAllHandlersForURLScheme(CFStringRef inURLScheme)`
//! —— **只有一个参数**。按「bundleID + scheme」两参数的印象去调，会把 NULL
//! 当成 scheme 传进去，返回值是 NULL（「no handlers available」），
//! 而这与「系统真的没装任何浏览器」**无法区分**，症状是浏览器列表静默变空。
//!
//! 正确做法是公开 API `-[NSWorkspace URLsForApplicationsToOpenURL:]`（10.15+）：
//! 传一个 `https://` URL，直接拿回**所有**能打开它的应用 URL（不是 bundle id，
//! 省掉一次 id → 路径的转换），且非 deprecated。
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
    let mut merged: Vec<InstalledBrowser> = Vec::new();

    // ① 权威来源：LaunchServices 认定的「能打开 https 链接」的应用
    for (name, path) in launchservices_http_handlers() {
        merged.push(InstalledBrowser { name, exe: path });
    }

    // ② 目录扫描兜底：LS 数据库偶尔漏索引（手工拷贝的 `.app`、刚装还没注册完）
    for dir in search_dirs() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("app") {
                continue;
            }
            let Some(info) = read_info_plist(&path) else {
                continue;
            };
            if !declares_web_scheme(&info) {
                continue;
            }
            let Some(name) = display_name(&path, &info) else {
                continue;
            };
            merged.push(InstalledBrowser {
                name,
                exe: path.to_string_lossy().into_owned(),
            });
        }
    }

    let mut result = normalize(merged);
    result.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    result
}

/// 把合并后的原始列表收拾干净。
///
/// LaunchServices 给的是**系统注册表的全量**，里面有前端菜单不该出现的两类噪声
/// （实测本机：13 条 → 收拾后 6 条）：
///
/// 1. **嵌套的 helper `.app`**：如
///    `/Applications/Doubao.app/Contents/Helpers/trampoline/Doubao Browser Trampoline.app`
///    ——它是某个 App 的内部组件、也会注册 http。列进「用 XX 浏览器打开」纯属噪音。
/// 2. **同名的多路径**：macOS 11+ 上 `Safari.app` 同时以 `/Applications/Safari.app`
///    和 `/System/Volumes/Preboot/Cryptexes/App/System/Applications/Safari.app`
///    两条注册（后者是密封系统卷里的副本）。用户看到两个 Safari 只会困惑。
///
/// 去重按**显示名**（不是路径），并优先保留「像用户会装的那个」：
/// 先排除 Cryptex 密封卷，再比路径深度（浅的优先）。
#[cfg(target_os = "macos")]
fn normalize(mut items: Vec<InstalledBrowser>) -> Vec<InstalledBrowser> {
    // ① 丢掉嵌套在别的 bundle 里的 helper
    items.retain(|b| !b.exe.contains(".app/Contents/"));

    // ② 按显示名去重，保留「更好」的那条
    let mut by_name: std::collections::HashMap<String, InstalledBrowser> =
        std::collections::HashMap::new();
    for item in items {
        let key = item.name.to_lowercase();
        let replace = match by_name.get(&key) {
            Some(existing) => prefer_candidate(&item, existing),
            None => true,
        };
        if replace {
            by_name.insert(key, item);
        }
    }
    by_name.into_values().collect()
}

/// 同名时是否该用 `candidate` 替换 `existing`
#[cfg(target_os = "macos")]
fn prefer_candidate(candidate: &InstalledBrowser, existing: &InstalledBrowser) -> bool {
    fn score(path: &str) -> (u8, usize) {
        // Cryptex 密封系统卷里的副本排最后；其余比路径深度，浅的更像用户装的
        let cryptex = u8::from(path.starts_with("/System/Volumes/Preboot"));
        (cryptex, path.matches('/').count())
    }
    score(&candidate.exe) < score(&existing.exe)
}

/// LaunchServices 认定的「能打开 https 链接」的应用（显示名 + `.app` 路径）。
///
/// 用公开 API `-[NSWorkspace URLsForApplicationsToOpenURL:]`（10.15+）：
/// 传一个探针 URL，拿回**全部**处理者的 `.app` 路径（已经是路径，
/// 不需要 bundle id → 路径的二次转换）。非 deprecated。
///
/// 返回空 vec 表示调用失败或系统一个处理者都没有 —— 此时调用方退化为
/// 纯目录扫描，标准位置的浏览器仍能列出，不至于整个功能消失。
#[cfg(target_os = "macos")]
fn launchservices_http_handlers() -> Vec<(String, String)> {
    use objc2_app_kit::NSWorkspace;
    use objc2_foundation::{NSArray, NSString, NSURL};

    // 探针 URL：只看**能不能被处理**，不真的发请求。用 https 而非 http
    // 是为了避开那些只注册了 http 的老应用？—— 恰恰相反，我们要的是全部，
    // 故取 https（现代浏览器的默认注册项）；两者差异由下面的目录扫描兜底。
    let Some(probe) = NSURL::URLWithString(&NSString::from_str("https://www.example.com/")) else {
        log::warn!("构造探针 URL 失败，浏览器枚举退化为目录扫描");
        return Vec::new();
    };
    let ws = NSWorkspace::sharedWorkspace();
    let urls: &NSArray<NSURL> = &*ws.URLsForApplicationsToOpenURL(&probe);
    let mut out = Vec::new();
    for url in urls.iter() {
        let Some(path) = url.path() else {
            continue;
        };
        let path = path.to_string();
        if !path.ends_with(".app") {
            continue;
        }
        // 显示名：优先 AppKit 的「本地化名」（用户可能改过，如「Visual Studio Code」），
        // 读不到就退回包名。不用 Info.plist —— LS 给的路径可能不在扫描目录里。
        let name = crate::mac::app_display_name(std::path::Path::new(&path))
            .unwrap_or_else(|| {
                std::path::Path::new(&path)
                    .file_stem()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_else(|| path.clone())
            });
        out.push((name, path));
    }
    out
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

#[cfg(all(test, target_os = "macos"))]
mod ls_tests {
    use super::*;

    /// LaunchServices 应当至少认出 Safari（macOS 自带的 http 处理者）。
    /// 这条守住「LS 调用真的能用」——它是 deprecated 的私有 API，
    /// 哪天系统升级改了行为，这里会第一个红。
    #[test]
    fn launchservices_returns_at_least_safari() {
        let handlers = launchservices_http_handlers();
        assert!(!handlers.is_empty(), "LaunchServices 没返回任何 http 处理者（枚举已退化为目录扫描）");
        assert!(
            handlers.iter().any(|(_, path)| path.contains("Safari")),
            "LaunchServices 结果里没有 Safari，实际拿到：{handlers:?}"
        );
    }

    /// 目录扫描必须仍然能独立工作（LS 数据库不可用时的兜底路径）。
    #[test]
    fn directory_scan_alone_still_finds_safari() {
        let list = list_installed();
        assert!(!list.is_empty(), "浏览器枚举结果为空");
    }

    /// 结果按显示名排序且不含重复路径（前端菜单直接渲染这份列表）。
    #[test]
    fn result_is_sorted_and_deduped() {
        let list = list_installed();
        let mut sorted = list.clone();
        sorted.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
        assert_eq!(
            list.iter().map(|b| &b.name).collect::<Vec<_>>(),
            sorted.iter().map(|b| &b.name).collect::<Vec<_>>(),
            "结果未按显示名排序"
        );
        let mut paths: Vec<String> = list.iter().map(|b| b.exe.to_lowercase()).collect();
        let before = paths.len();
        paths.sort();
        paths.dedup();
        assert_eq!(paths.len(), before, "同一个 .app 被列了多次（前端菜单会出现重复项）");
    }
}


#[cfg(all(test, target_os = "macos"))]
mod normalize_tests {
    use super::*;

    fn b(name: &str, exe: &str) -> InstalledBrowser {
        InstalledBrowser { name: name.into(), exe: exe.into() }
    }

    /// helper `.app`（嵌在别的 bundle 里）不该出现在「用 XX 浏览器打开」里
    #[test]
    fn drops_nested_helper_apps() {
        let items = vec![
            b("Doubao", "/Applications/Doubao.app"),
            b("Doubao Browser Trampoline", "/Applications/Doubao.app/Contents/Helpers/trampoline/Doubao Browser Trampoline.app"),
        ];
        let out = normalize(items);
        assert_eq!(out.len(), 1, "helper 混进了列表：{out:?}");
        assert_eq!(out[0].name, "Doubao");
    }

    /// macOS 11+ 上 Safari 会以两条路径注册（Applications + 密封系统卷），
    /// 只保留用户会装的那条
    #[test]
    fn dedupes_same_name_keeping_user_facing_path() {
        let items = vec![
            b("Safari", "/System/Volumes/Preboot/Cryptexes/App/System/Applications/Safari.app"),
            b("Safari", "/Applications/Safari.app"),
        ];
        let out = normalize(items);
        assert_eq!(out.len(), 1, "Safari 出现了两次：{out:?}");
        assert_eq!(out[0].exe, "/Applications/Safari.app");
    }

    /// 顺序不应影响结果：反序输入也必须收敛到同一条
    #[test]
    fn dedupe_is_order_independent() {
        let deep = b("Safari", "/System/Volumes/Preboot/Cryptexes/App/System/Applications/Safari.app");
        let shallow = b("Safari", "/Applications/Safari.app");
        assert_eq!(
            normalize(vec![deep.clone(), shallow.clone()])[0].exe,
            normalize(vec![shallow, deep])[0].exe
        );
    }

    /// 去重按显示名（大小写不敏感），不同名的同名路径各留一条
    #[test]
    fn different_names_are_kept() {
        let items = vec![b("Google Chrome", "/a/Google Chrome.app"), b("Chromium", "/b/Chromium.app")];
        assert_eq!(normalize(items).len(), 2);
    }
}
