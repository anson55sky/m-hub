//! service 后端运行时解析（spec §5.2）：系统 Node 优先，失败自动降级到内置运行时，
//! 内置未缓存则按需下载（Node 官方分发）。返回用于启动后端的可执行文件路径。

use crate::process::NoConsoleWindow;
use std::path::PathBuf;

/// 内置 Node 版本（Node 官方 LTS）
const NODE_VERSION: &str = "v24.9.0";

/// 解析 Node 版本号的主版本（"v22.11.0" → 22）
fn node_major(version: &str) -> u32 {
    version
        .trim()
        .trim_start_matches('v')
        .split('.')
        .next()
        .unwrap_or("0")
        .parse()
        .unwrap_or(0)
}

/// 检测系统 Node 是否可用且主版本 ≥ min_version（min_version 形如 "22"）。
/// 成功返回版本号（如 "v22.11.0"）。
fn check_system_node(min_version: Option<&str>) -> Result<String, String> {
    let mut cmd = std::process::Command::new("node");
    cmd.arg("--version");
    cmd.no_console_window();
    let out = cmd
        .output()
        .map_err(|_| "未检测到 Node.js".to_string())?;
    if !out.status.success() {
        return Err("Node.js 不可用".to_string());
    }
    let ver = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if let Some(min) = min_version {
        let min_major = node_major(min);
        let cur_major = node_major(&ver);
        if cur_major < min_major {
            return Err(format!("系统 Node {ver} 低于要求 ≥ {min}"));
        }
    }
    Ok(ver)
}

/// 内置 Node 缓存目录：`data_root()/runtime/node`
/// 必须用 `paths::data_root()`（便携版跟随可执行文件同级的 data/），不能用 `app_data_dir()`。
fn builtin_node_dir(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    let _ = app;
    Ok(crate::paths::data_root().join("runtime").join("node"))
}

/// 本平台的 Node 官方分发标识（`node-<版本>-<平台>-<架构>`）。
///
/// 与 nodejs.org/dist 的目录名一一对应。返回 None = 该平台/架构官方没发预编译包
/// （本工程只覆盖 Windows x64 与 macOS 的两个主流架构），此时降级为「用系统 Node」。
#[cfg(all(target_os = "windows", target_arch = "x86_64"))]
const DIST_TARGET: Option<&str> = Some("win-x64");
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
const DIST_TARGET: Option<&str> = Some("darwin-arm64");
#[cfg(all(target_os = "macos", target_arch = "x86_64"))]
const DIST_TARGET: Option<&str> = Some("darwin-x64");
#[cfg(not(any(
    all(target_os = "windows", target_arch = "x86_64"),
    all(target_os = "macos", target_arch = "aarch64"),
    all(target_os = "macos", target_arch = "x86_64")
)))]
const DIST_TARGET: Option<&str> = None;

/// 分发包在解压后的顶层目录名（`node-v24.9.0-darwin-arm64/`）
fn dist_dir_name() -> Option<String> {
    DIST_TARGET.map(|t| format!("node-{NODE_VERSION}-{t}"))
}

/// 分发包的可执行文件名（Windows 是 `node.exe`，类 Unix 没有后缀）
#[cfg(target_os = "windows")]
const NODE_BIN: &str = "node.exe";
#[cfg(not(target_os = "windows"))]
const NODE_BIN: &str = "node";

/// 内置 Node 可执行文件路径（已缓存则 Some）
fn builtin_node_exe(app: &tauri::AppHandle) -> Option<PathBuf> {
    let dir = builtin_node_dir(app).ok()?;
    let exe = dir.join(dist_dir_name()?).join("bin").join(NODE_BIN);
    exe.is_file().then_some(exe)
}

/// 内置 Node 运行时下载 URL
fn dist_url() -> Option<String> {
    DIST_TARGET.map(|t| {
        let ext = if cfg!(target_os = "windows") { "zip" } else { "tar.gz" };
        format!("https://nodejs.org/dist/{NODE_VERSION}/node-{NODE_VERSION}-{t}.{ext}")
    })
}

/// 解压 `.tar.gz`（macOS 的 Node 分发格式）到 `dst`。
///
/// 安全要点与 `market::extract_zip` 同款：**逐条校验路径**。tar 的 `..` 穿越
/// 比 zip 更危险——`entry.path()` 直接来自归档头、完全由远端内容决定。
/// 遇到越界条目**跳过并记账**而不是 return Err（一个坏条目不该让整个安装失败），
/// 但只要一条都没解出来就报错，避免「装了个空目录却显示就绪」。
fn extract_tar_gz(bytes: &[u8], dst: &std::path::Path) -> Result<usize, String> {
    let decoder = flate2::read::GzDecoder::new(bytes);
    let mut archive = tar::Archive::new(decoder);
    let mut written = 0usize;
    for entry in archive.entries().map_err(|e| format!("解压失败: {e}"))? {
        let mut entry = entry.map_err(|e| format!("解压失败: {e}"))?;
        let path = entry.path().map_err(|e| format!("解压失败: {e}"))?.into_owned();
        // ⚠️ **必须拿「拼好之后的完整路径」去比**：归档里的 `path` 是**相对**的
        // （`node-…/bin/node`），直接 `path.starts_with(dst)` 拿绝对路径去比，
        // 永远为 false —— 结果就是「什么都没解出来」。先 join 再判越界：
        // `dst.join("../x")` 在词法上就不以 `dst` 开头，`..` 与绝对路径都能挡掉。
        // `unpack_in` 是第二道防线（tar crate 内部也会拒），两层都留。
        if !dst.join(&path).starts_with(dst) {
            log::warn!("内置运行时包内条目越界，已跳过: {}", path.display());
            continue;
        }
        if entry.unpack_in(dst).map_err(|e| format!("解压失败: {e}"))? {
            written += 1;
        }
    }
    if written == 0 {
        return Err("解压后没有任何文件".into());
    }
    Ok(written)
}

/// 下载并解压内置 Node，返回可执行文件路径。
async fn download_builtin_node(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    let Some(url) = dist_url() else {
        return Err("当前平台/架构没有官方预编译运行时，请安装 Node.js".into());
    };
    let resp = reqwest::get(&url)
        .await
        .map_err(|e| format!("下载失败: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("下载失败: HTTP {}", resp.status()));
    }
    let bytes = resp.bytes().await.map_err(|e| e.to_string())?;

    let dir = builtin_node_dir(app)?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;

    if url.ends_with(".zip") {
        crate::market::extract_zip(&bytes, &dir)?;
    } else {
        extract_tar_gz(&bytes, &dir)?;
    }

    let exe = dir
        .join(dist_dir_name().unwrap_or_default())
        .join("bin")
        .join(NODE_BIN);
    if !exe.is_file() {
        return Err(format!("解压后未找到 {}", exe.display()));
    }
    // 官方 tar 包里的 node 二进制**不带可执行位**，解压后要自己补上，
    // 否则 service 后端会以「permission denied」起不来（Windows 的 zip 不需要这步）
    #[cfg(not(target_os = "windows"))]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(mut perm) = std::fs::metadata(&exe).map(|m| m.permissions()) {
            perm.set_mode(perm.mode() | 0o755);
            let _ = std::fs::set_permissions(&exe, perm);
        }
    }
    log::info!("内置 Node 运行时已就绪: {}", exe.display());
    Ok(exe)
}

/// 解析用于启动后端的 node 可执行文件。策略（config.runtime_strategy）：
/// - `system`：始终用系统 Node，不降级、不下载；
/// - `builtin`：始终用内置（已缓存直接用，未缓存下载）；
/// - `auto`（默认）：系统优先，版本不符/缺失降级内置，未缓存则下载。
pub fn resolve_node(
    app: &tauri::AppHandle,
    min_version: Option<&str>,
    strategy: &str,
) -> Result<PathBuf, String> {
    match strategy {
        "system" => {
            check_system_node(min_version).map_err(|e| {
                format!(
                    "{e}（运行时策略为「始终系统」，请安装 Node ≥ {}）",
                    min_version.unwrap_or("22")
                )
            })?;
            Ok(PathBuf::from("node"))
        }
        "builtin" => {
            if let Some(exe) = builtin_node_exe(app) {
                return Ok(exe);
            }
            log::info!("运行时策略「始终内置」，下载内置运行时…");
            tauri::async_runtime::block_on(download_builtin_node(app))
        }
        _ => {
            let system_err = match check_system_node(min_version) {
                Ok(_ver) => return Ok(PathBuf::from("node")),
                Err(e) => e,
            };

            if let Some(exe) = builtin_node_exe(app) {
                log::info!("系统 Node 不可用（{system_err}），降级使用内置运行时");
                return Ok(exe);
            }

            log::info!("系统 Node 不可用，下载内置运行时（首次，一次性）…");
            match tauri::async_runtime::block_on(download_builtin_node(app)) {
                Ok(exe) => Ok(exe),
                Err(e) => Err(format!(
                    "系统 Node 不可用（{system_err}），内置运行时下载失败（{e}）。请安装 Node ≥ {}",
                    min_version.unwrap_or("22")
                )),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn node_major_parses() {
        assert_eq!(node_major("v22.11.0"), 22);
        assert_eq!(node_major("v18.20.4"), 18);
        assert_eq!(node_major("22"), 22);
        assert_eq!(node_major("bogus"), 0);
    }

    /// 缓存目录名必须与 nodejs.org 的分发包顶层目录名逐字一致——
    /// 对不上就是「下载成功、解压后找不到 node」这类极难查的失败。
    #[test]
    fn dist_dir_matches_official_layout() {
        let Some(name) = dist_dir_name() else {
            // 官方没发该平台/架构的包：至少要能给出可读的降级提示
            assert!(dist_url().is_none());
            return;
        };
        assert!(name.starts_with(&format!("node-{NODE_VERSION}")), "{name}");
        let url = dist_url().unwrap();
        assert!(url.contains(&name), "URL 应含目录名: {url}");
        // 分发包后缀：Windows 是 zip，类 Unix 是 tar.gz
        let want_ext = if cfg!(target_os = "windows") { ".zip" } else { ".tar.gz" };
        assert!(url.ends_with(want_ext), "{url}");
    }

    /// tar 条目越界必须被挡下：Node 的分发来自公网，`.` / `..` / 绝对路径都不能写出目标目录
    #[test]
    fn extract_tar_gz_rejects_path_traversal() {
        use std::io::Write;
        let mut builder = tar::Builder::new(Vec::new());
        let mut header = tar::Header::new_gnu();
        header.set_size(2);
        header.set_mode(0o644);
        // 先写一个合法路径让 header 成型，再**直接改原始 name 字段**塞进 `..`。
        // tar crate 的 `set_path` 自己就会拒绝 `..`，想测防护就必须绕过它伪造归档头。
        header.set_path("placeholder.txt").unwrap();
        let evil = b"../escaped.txt";
        let old = header.as_old_mut();
        old.name[..evil.len()].copy_from_slice(evil);
        old.name[evil.len()..].fill(0);
        header.set_cksum();
        builder
            .append(&header, &mut std::io::Cursor::new(b"hi".to_vec()))
            .unwrap();
        let tar_bytes = builder.into_inner().unwrap();

        let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        gz.write_all(&tar_bytes).unwrap();
        let gz = gz.finish().unwrap();

        let dst = std::env::temp_dir().join(format!("mhub-tar-probe-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dst);
        std::fs::create_dir_all(&dst).unwrap();

        // 越界条目被跳过 → 视为「什么都没解出来」并报错
        let err = extract_tar_gz(&gz, &dst).unwrap_err();
        assert!(err.contains("没有任何文件"), "实际: {err}");
        assert!(
            !std::env::temp_dir().join("escaped.txt").exists(),
            "越界条目绝不能写到目标目录之外"
        );
        let _ = std::fs::remove_dir_all(&dst);
    }

    /// 正常包必须真的解出来（且目录结构与 node 的官方布局一致）
    #[test]
    fn extract_tar_gz_unpacks_regular_entry() {
        use std::io::Write;
        let mut builder = tar::Builder::new(Vec::new());
        let mut header = tar::Header::new_gnu();
        header.set_size(5);
        header.set_mode(0o644);
        header.set_path("node-v0.1.0-darwin-arm64/bin/node").unwrap();
        header.set_cksum();
        builder
            .append(&header, &mut std::io::Cursor::new(b"hello".to_vec()))
            .unwrap();
        let tar_bytes = builder.into_inner().unwrap();

        let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        gz.write_all(&tar_bytes).unwrap();
        let gz = gz.finish().unwrap();

        let dst = std::env::temp_dir().join(format!("mhub-tar-ok-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dst);
        std::fs::create_dir_all(&dst).unwrap();

        assert_eq!(extract_tar_gz(&gz, &dst).unwrap(), 1);
        let out = dst.join("node-v0.1.0-darwin-arm64/bin/node");
        assert_eq!(std::fs::read_to_string(&out).unwrap(), "hello");
        let _ = std::fs::remove_dir_all(&dst);
    }
}
