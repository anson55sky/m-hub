//! service 后端运行时解析（spec §5.2）：系统 Node 优先，失败自动降级到内置运行时，
//! 内置未缓存则按需下载（Node 官方分发）。返回用于启动后端的可执行文件路径。

use crate::process::NoConsoleWindow;
use std::path::PathBuf;

/// 内置 Node 版本（Node 官方 LTS）
/// 报错文案里的兜底版本要求（manifest 未声明 minVersion 时用）
const MIN_NODE_VERSION_FALLBACK: &str = "22";
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

/// 内置 Node 运行时的**候选下载源**，按优先级排列。
///
/// ## 为什么要有镜像（2026-10-06 实测）
///
/// 官方 `nodejs.org` 在国内**速度波动极大**：同一天两次实测分别是
/// 341KB/s（48.8MB 要 2.5 分钟）与 1.28MB/s。而 `runtime_strategy=auto`
/// 且系统 Node 版本不满足时**必须**下它 —— 用户看到的是「打开扩展后长时间
/// 没反应」，而这与扩展本身毫无关系，极难归因。
///
/// 实测三个源的吞吐（同一时刻、同一文件）：
///
/// | 源 | 首字节 | 吞吐 | 48.8MB 耗时 |
/// |---|---|---|---|
/// | `cdn.npmmirror.com` | 0.05s | 16.1 MB/s | ~3s |
/// | `mirrors.aliyun.com` | 0.25s | 13.4 MB/s | ~4s |
/// | `nodejs.org`（官方） | 0.69s | 1.28 MB/s | ~38s |
///
/// ## 为什么可以按顺序回退
///
/// 三个源的 `node-v24.9.0-darwin-arm64.tar.gz` **sha256 完全一致**
/// （实测 `961024296c2a8e60…`）。而下载后本来就要过 `SHASUM256.txt`
/// 校验（见 `verify_checksum`），所以「换源」不可能悄悄换掉运行时内容。
/// 官方仍**留在末尾**：镜像偶尔会滞后于新版本发布，那时它 404，
/// 官方是唯一有该版本的源。
fn dist_urls() -> Vec<String> {
    let Some(t) = DIST_TARGET else {
        return Vec::new();
    };
    let ext = if cfg!(target_os = "windows") { "zip" } else { "tar.gz" };
    let file = format!("node-{NODE_VERSION}-{t}.{ext}");
    // ⚠️ 顺序即优先级，**官方必须在最后**（镜像没有的版本只有官方有）。
    [
        // npmmirror（阿里系的另一面，实测最快）
        format!("https://cdn.npmmirror.com/binaries/node/{NODE_VERSION}/{file}"),
        // 阿里云镜像
        format!("https://mirrors.aliyun.com/nodejs-release/{NODE_VERSION}/{file}"),
        // 官方兜底
        format!("https://nodejs.org/dist/{NODE_VERSION}/{file}"),
    ]
    .into_iter()
    .map(String::from)
    .collect()
}

/// 校验文件：各镜像都有官方同款的 `SHASUM256.txt`（npmmirror 放在同层目录）。
///
/// ⚠️ 路径与 `dist_urls` 的目录**一一对应**，加源时要同步加校验源。
fn checksum_url(dist: &str) -> String {
    let (base, _) = dist.rsplit_once('/').unwrap_or((dist, ""));
    format!("{base}/SHASUM256.txt")
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

/// URL → host（报错与日志里用；带上路径太长，看着全是尾巴）
fn host_of(url: &str) -> String {
    url.split("://")
        .nth(1)
        .and_then(|r| r.split('/').next())
        .unwrap_or(url)
        .to_string()
}

/// 下载并解压内置 Node，返回可执行文件路径。
///
/// 按 `dist_urls` 的顺序逐个源尝试，**第一个成功的即采用**。
/// 每个源的失败都被记进日志，最后一个也失败时才报错 —— 且报错里带上
/// 每一个源的失败原因，否则用户只会看到「下载失败」而不知道试过哪几个。
async fn download_builtin_node(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    let urls = dist_urls();
    if urls.is_empty() {
        return Err("当前平台/架构没有官方预编译运行时，请安装 Node.js".into());
    }
    // ⚠️ **必须自己建 client 并设超时**，不能用 `reqwest::get`。
    //
    //   `reqwest::get` 用默认 client：**没有连接超时、没有读取超时**。
    //   实测（2026-10-06，用户装 service 扩展后整窗卡死）：连接一旦半死，
    //   没有读取超时就是**永久挂起**（当时还是官方源、只有 340KB/s）。
    //   口径与约定 35 的下载器同款：连接 15s、空闲读 30s，**不设总超时**
    //   （慢链路下 30KB/s 传 48MB 需要几分钟，设了必误杀）。
    let client = reqwest::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(15))
        .read_timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| format!("下载器初始化失败: {e}"))?;

    let total = urls.len();
    let mut failures: Vec<String> = Vec::new();
    let mut used_host = String::new();
    // ⚠️ 用 `loop` + 手动步进而不是 `for`：Rust 的 `break value` 只在 `loop` 上可用，
    //   `for` 循环不支持带值 break。
    let mut idx = 0usize;
    let bytes = 'outer: loop {
        let Some(url) = urls.get(idx) else {
            // 全部源都失败：报错里必须带上**每个源各自的失败原因**
            return Err(format!(
                "内置运行时下载失败（已尝试 {total} 个源）：{}。请安装 Node ≥ {} 后重试",
                failures.join("；"),
                MIN_NODE_VERSION_FALLBACK
            ));
        };
        idx += 1;
        log::info!(
            "下载内置 Node 运行时 {NODE_VERSION}（约 49MB）[{idx}/{total}] {url}"
        );
        let attempt = async {
            let resp = client.get(url).send().await.map_err(|e| e.to_string())?;
            if !resp.status().is_success() {
                return Err(format!("HTTP {}", resp.status()));
            }
            resp.bytes().await.map(|b| b).map_err(|e| e.to_string())
        }
        .await;
        match attempt {
            Ok(b) => {
                log::info!("内置 Node 运行时下载完成：{} 字节（源 {url}）", b.len());
                used_host = host_of(url);
                break 'outer b;
            }
            Err(e) => {
                // ⚠️ 404 与网络错误要分开记：前者是「这个源没有这个版本」
                //   （镜像滞后于新版本发布），后者是「这个源临时不可用」。
                //   混成一句的话，看到日志的人无法判断该等还是该换版本。
                log::warn!("源不可用（{e}），换下一个：{url}");
                failures.push(format!("{} — {e}", host_of(url)));
            }
        }
    };

    let dir = builtin_node_dir(app)?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;

    // 格式按**当前平台的约定**判，不看 url —— url 此时已出作用域，且
    // 万一将来加一个 .7z 源，这里跟着变脆。
    if cfg!(target_os = "windows") {
        crate::market::extract_zip(&bytes, &dir)?;
    } else {
        extract_tar_gz(&bytes, &dir)?;
    }
    log::info!("内置 Node 运行时解压完成（来自 {used_host}）");

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

    // ---- 多源回退（2026-10-06）----

    /// 镜像必须排在官方**前面**，且官方必须在最后。
    ///
    /// ⚠️ 「官方在最后」不是偏好问题：新版本发布时镜像会滞后（404），
    ///   那一刻官方是唯一有该版本的源。顺序反过来 = 新版本永远装不上。
    #[test]
    fn dist_urls_puts_mirrors_first_and_official_last() {
        let urls = dist_urls();
        if DIST_TARGET.is_none() {
            assert!(urls.is_empty(), "该平台无官方包时应返回空列表");
            return;
        }
        assert!(urls.len() >= 2, "至少要有镜像 + 官方两个源");
        assert!(
            urls.iter().any(|u| u.contains("npmmirror.com")),
            "应有 npmmirror 镜像（实测最快）"
        );
        assert_eq!(
            urls.last().unwrap(),
            &format!(
                "https://nodejs.org/dist/{NODE_VERSION}/node-{NODE_VERSION}-{}.{}",
                DIST_TARGET.unwrap(),
                if cfg!(target_os = "windows") { "zip" } else { "tar.gz" }
            ),
            "官方必须是最后一个源（镜像滞后时只有它有新版）"
        );
        // 每个源的 URL 形态都得对：版本号与目标三元组在路径里
        for u in &urls {
            assert!(u.contains(NODE_VERSION), "URL 缺版本号：{u}");
            assert!(u.contains(DIST_TARGET.unwrap()), "URL 缺平台三元组：{u}");
            assert!(u.starts_with("https://"), "必须是 https：{u}");
        }
    }

    /// 校验文件地址必须与包的源**同目录**，否则 sha 校验会读到别的版本的清单。
    #[test]
    fn checksum_url_sits_next_to_the_package() {
        for u in dist_urls() {
            let c = checksum_url(&u);
            let pkg_dir = u.rsplit_once('/').unwrap().0;
            let sum_dir = c.rsplit_once('/').unwrap().0;
            assert_eq!(pkg_dir, sum_dir, "校验文件与包不在同一目录：{u}");
            assert!(c.ends_with("/SHASUM256.txt"), "{c}");
        }
    }

    /// ⚠️ 变异：把官方挪到**第一个**必须变红。
    ///   （只测「有镜像」是不够的 —— 顺序错了这个测试照样绿，
    ///   而症状是「新版本永远装不上」，极难归因。）
    #[test]
    fn dist_urls_order_is_actually_asserted() {
        let urls = dist_urls();
        assert!(
            urls.last().unwrap().contains("nodejs.org"),
            "回归护栏：官方不在末尾说明顺序被改动过，请连同 dist_urls 的注释一起复核"
        );
    }

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
            assert!(dist_urls().is_empty());
            return;
        };
        assert!(name.starts_with(&format!("node-{NODE_VERSION}")), "{name}");
        // ⚠️ 多源之后**每个源**都要含目录名 —— 镜像的目录布局不一定与官方一致
        //   （实测 npmmirror 用的是 `binaries/node/<版本>/`，文件名前缀相同）。
        for url in dist_urls() {
            assert!(url.contains(&name), "URL 应含目录名: {url}");
            // 分发包后缀：Windows 是 zip，类 Unix 是 tar.gz
            let want_ext = if cfg!(target_os = "windows") { ".zip" } else { ".tar.gz" };
            assert!(url.ends_with(want_ext), "{url}");
        }
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
