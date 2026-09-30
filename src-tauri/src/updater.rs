//! 应用自动升级（方案 A：自研 update.json + 摘要签名 + rename 自替换）。
//!
//! 升级清单 `releases/update.json` 紧随市场清单之后扩展：发布侧用同一把
//! Ed25519 私钥对原始字节做分离签名（`.sig` 文本文件并列上传），客户端以
//! 内嵌公钥验签（`signing::verify_detached`），通过才信任清单内容——
//! 清单是唯一安全根，下载物（新版本 zip）的 sha256 均由签名清单背书。
//!
//! 流程（对应文档 §6.2）：
//!   ① `check_for_update`：拉取 update.json + .sig → 验签 → semver 比较 +
//!      `minimumUpgradable` 跳级保护 → 平台匹配（键名见 platform_key；便携版优先 portableUrl）→
//!      广播 `update-available`（版本/说明/大小）。
//!   ② `download_update`：按清单下载新版本 zip → 边下边算 sha256（与清单
//!      比对）→ 落 `data_root()/updates/<version>/m-hub.zip` →
//!      写 `data_root()/updates/.pending.json` 标记，广播 `update-ready`。
//!   ③ `apply_pending_update`：每次启动早期调用（幂等）。无标记直接跳过；
//!      待应用版本不高于当前版本（如已手动装了更高版）→ 清理过期包防降级；
//!      有标记 → 解包 → `exe → exe.old` / `新 exe → exe` 两步就位
//!      （Windows 允许 rename 正在运行的 exe；数据根与 exe 跨盘时 rename
//!      报 os error 17，move_file 退化为复制）→ 校验新 exe 具名 → 删 .old。
//!      就位成功后立即以新 exe 拉起子进程接管启动、当前进程退出——当前
//!      进程镜像已被改名成 .old，原地继续跑只会是旧版本。任一步失败
//!      回滚并保留标记，下次启动重试。启动时顺手清理上次升级残留的 .old。
//!      拉起后、退出前必须先 `tauri_plugin_single_instance::destroy` 释放单
//!      实例互斥与隐藏窗口：否则新实例启动时插件 setup 里 CreateMutexW 会撞
//!      上旧进程未释放的互斥（旧进程 std::process::exit 不触发插件的 Exit
//!      清理），新实例被判为「第二实例」自杀退出——表现为升级后应用没有自动
//!      重启，需用户手动再开（历史反复出现的现象，顺序见 `relaunch_app` 注释：
//!      先 spawn 成功再 destroy，失败路径不触碰互斥以防双实例）。
//!
//! 事件：`update-available`、`update-download-progress`、`update-ready`。

use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;
use tauri::Emitter;
use tauri::Manager;

/// 更新清单 schema 版本。
const SCHEMA_VERSION: u32 = 1;

/// 待应用更新标记文件：`data_root()/updates/.pending.json`
const PENDING_FILE: &str = ".pending.json";

/// 下载互斥标志：同一时刻只允许一个 download_update 在跑。
/// 并发触发会各自 File::create 截断同一临时文件互踩，必须拒绝而非排队
static DOWNLOAD_IN_FLIGHT: AtomicBool = AtomicBool::new(false);

/// 「稍后再提示」补检去重：每点一次递增代数，只有代数仍最新的那个补检任务
/// 到期才真正执行检查。连点「稍后再提示」时暂停窗口本来就顺延到最后一次点击
/// （`update_snooze_until_ms` 覆盖写），这里再让旧的补检任务到点后自行退出，
/// 避免排出一串同一时刻的重复检查。
static SNOOZE_GENERATION: AtomicU64 = AtomicU64::new(0);

/// download_update 的守卫：任何退出路径（含 ? / panic 展开）都复位互斥标志
struct DownloadGuard;
impl Drop for DownloadGuard {
    fn drop(&mut self) {
        DOWNLOAD_IN_FLIGHT.store(false, Ordering::Release);
    }
}

/// 下载根目录：`data_root()/updates/<version>/`
fn updates_root() -> Result<PathBuf, String> {
    Ok(crate::paths::data_root().join("updates"))
}

fn pending_file() -> Result<PathBuf, String> {
    Ok(updates_root()?.join(PENDING_FILE))
}

/// 更新清单（远端 `update.json`），字段与文档 §6.1 一致。
#[derive(Debug, Clone, Deserialize)]
struct UpdateManifest {
    #[serde(rename = "schemaVersion")]
    schema_version: u32,
    /// 新版本号（对比当前版本决定是否可更新）
    #[serde(default)]
    version: String,
    /// 可升级的最低版本下限（跳级保护）
    #[serde(default, rename = "minimumUpgradable")]
    minimum_upgradable: String,
    /// 更新说明摘要（下载前给用户看）
    #[serde(default)]
    notes: String,
    /// 平台条目：`macos-aarch64` 等，键名见 [`platform_key`]
    #[serde(default)]
    platforms: std::collections::HashMap<String, PlatformEntry>,
}

impl Default for UpdateManifest {
    fn default() -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            version: String::new(),
            minimum_upgradable: String::new(),
            notes: String::new(),
            platforms: std::collections::HashMap::new(),
        }
    }
}

/// 单个平台的下载信息。
#[derive(Debug, Clone, Deserialize)]
struct PlatformEntry {
    /// 标准版下载地址（zip）
    #[serde(default)]
    url: String,
    /// 便携版下载地址（zip，便携版优先）
    #[serde(default, rename = "portableUrl")]
    portable_url: String,
    /// 标准版 zip 的 sha256（hex 小写）
    #[serde(default)]
    sha256: String,
    /// 便携版 zip 的 sha256（hex 小写）
    #[serde(default, rename = "portableSha256")]
    portable_sha256: String,
    /// zip 字节大小（0 = 未知，仅作进度参考）
    #[serde(default)]
    size: u64,
    /// 便携版 zip 字节大小（0 = 未知）
    #[serde(default, rename = "portableSize")]
    portable_size: u64,
}

impl Default for PlatformEntry {
    fn default() -> Self {
        Self {
            url: String::new(),
            portable_url: String::new(),
            sha256: String::new(),
            portable_sha256: String::new(),
            size: 0,
            portable_size: 0,
        }
    }
}

/// 前端查询/收到的更新信息（`get_update_status` / `update-available` 负载）。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInfo {
    /// 是否有可用更新（已命中版本且未下载）
    pub available: bool,
    /// 目标版本号（空 = 无目标版本）
    pub version: String,
    /// 更新说明摘要
    pub notes: String,
    /// 本次更新 zip 大小（0 = 未知）
    pub size: u64,
    /// 该更新是否为便携版专属（决定下载哪个 URL / 校验哪个 sha256）
    pub portable: bool,
    /// 是否已就绪待重启应用（下载完成并写好标记）
    pub ready: bool,
    /// 当前应用的版本号
    pub current: String,
}

impl UpdateInfo {
    fn none(current: &str) -> Self {
        Self {
            available: false,
            version: String::new(),
            notes: String::new(),
            size: 0,
            portable: false,
            ready: false,
            current: current.to_string(),
        }
    }
}

fn current_version(app: &tauri::AppHandle) -> String {
    app.package_info().version.to_string()
}

/// 解析更新清单。
fn parse_manifest(bytes: &[u8]) -> Result<UpdateManifest, String> {
    let m: UpdateManifest = serde_json::from_slice(bytes).map_err(|e| format!("清单解析失败: {e}"))?;
    if m.schema_version > SCHEMA_VERSION {
        return Err(format!(
            "升级清单 schemaVersion={} 高于宿主支持的 v{SCHEMA_VERSION}",
            m.schema_version
        ));
    }
    if m.version.is_empty() {
        return Err("升级清单缺少 version 字段".to_string());
    }
    Ok(m)
}

/// 一次性拉取 URL 内容（字节），非 2xx 视为失败。
async fn fetch_bytes(client: &reqwest::Client, url: &str) -> Result<Vec<u8>, String> {
    let resp = client
        .get(url)
        .send()
        .await
        .map_err(|e| format!("通信失败：{e}"))?;
    if !resp.status().is_success() {
        return Err(format!("HTTP {}", resp.status()));
    }
    let bytes = resp.bytes().await.map_err(|e| format!("读取响应失败: {e}"))?;
    Ok(bytes.to_vec())
}

/// 脱敏更新源错误信息：不向用户暴露具体 URL。
fn sanitize_update_error(mut msg: String, endpoint: &str, sig_url: &str) -> String {
    for url in [endpoint, sig_url] {
        msg = msg.replace(url, "(更新源地址)");
    }
    msg
}

/// 更新清单地址（内置常量：平台服务端接口，服务端再代理 COS）。
fn update_endpoint() -> String {
    crate::config::update_manifest_url()
}

/// 拉取更新清单并验签（未验签通过一律不信任）。返回 `(清单, 原字节)`。
async fn fetch_manifest(client: &reqwest::Client) -> Result<(UpdateManifest, Vec<u8>), String> {
    let endpoint = update_endpoint();
    let sig_url = format!("{endpoint}.sig");
    let content = fetch_bytes(client, &endpoint)
        .await
        .map_err(|e| sanitize_update_error(format!("拉取更新清单失败：{e}"), &endpoint, &sig_url))?;
    let sig = fetch_bytes(client, &sig_url)
        .await
        .map_err(|e| sanitize_update_error(format!("拉取清单签名失败：{e}"), &endpoint, &sig_url))?;
    let sig = String::from_utf8_lossy(&sig).into_owned();
    crate::signing::verify_detached(&content, &sig)
        .map_err(|e| format!("更新清单验签失败：{e}"))?;
    let manifest = parse_manifest(&content)?;
    Ok((manifest, content))
}

fn to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn version_cmp(a: &str, b: &str) -> std::cmp::Ordering {
    crate::market::version_cmp(a, b)
}

/// 本构建对应的清单平台键，形如 `macos-aarch64` / `macos-x86_64`。
///
/// ## ⚠️ 这里原先硬编码 `windows-x86_64`（2026-09-30 修）
///
/// 上游是 Windows 独占，平台键是写死的常量。移植到 macOS 后**它没跟着换**，
/// 于是 `platforms.get("windows-x86_64")` 在 macOS 上永远取不到条目 ——
/// 清单里就算发了 macOS 的包也读不到，实机表现是日志一行
/// 「更新源无当前平台条目（windows-x86_64）」且**永远不提示更新**。
///
/// 这属于约定 P9 点名的形态：「上游换了数据源，下游的匹配代码没跟着换」，
/// 而且**没有论证注释**（= 不属于有意取舍，是移植遗漏）。
///
/// 键名由**构建目标**算出而不是写死：换架构（Apple Silicon ↔ Intel）时
/// 服务端发哪个键就认哪个，不需要再改代码。清单里的键名是这个函数
/// 决定的唯一契约，改它要同步告知服务端。
// `concat!` 只接受字面量、不能拼 const，所以这里用「cfg 挂在 return 上」这个惯用法：
// 每个分支在编译期就定死，函数体里不会留下任何运行时字符串拼接。
//
// 键名带 OS 段：同一个 `aarch64` 在 macOS 与其它平台含义不同，
// 只按架构索引会让跨平台的清单互相串味。
pub fn platform_key() -> &'static str {
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    return "macos-aarch64";
    #[cfg(all(target_os = "macos", target_arch = "x86_64"))]
    return "macos-x86_64";
    #[cfg(all(target_os = "windows", target_arch = "x86_64"))]
    return "windows-x86_64";
    #[cfg(all(target_os = "windows", target_arch = "aarch64"))]
    return "windows-aarch64";
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    return "linux-x86_64";
    #[cfg(all(target_os = "linux", target_arch = "aarch64"))]
    return "linux-aarch64";
    #[allow(unreachable_code)]
    "unknown-unknown"
}

/// 「清单里没有本平台的条目」的唯一口径文案。
///
/// 两个调用点（检查更新、下载前复查）曾经各写一份且含义不同：
/// 检查那处返回 `none`（→ 前端显示「已是最新版本」，**谎报成功**），
/// 下载那处返回 `Err`。口径必须只有一份，否则将来只会漂移。
///
/// 措辞要点：说清「是服务端没发本平台的包」而不是「没有新版本」——
/// 两者对用户的行动建议完全相反。
fn no_platform_entry_reason(manifest: &UpdateManifest) -> String {
    let mut keys: Vec<&str> = manifest.platforms.keys().map(|k| k.as_str()).collect();
    keys.sort_unstable(); // HashMap 迭代序不定，报错文案要稳定可复现
    if keys.is_empty() {
        return format!(
            "更新源的 platforms 为空，没有任何平台的包（本构建需要 {} 条目）",
            platform_key()
        );
    }
    format!(
        "更新源无当前平台条目：清单只发布了 [{}]，本构建是 {}。服务端需在 platforms 下补 {} 条目",
        keys.join(", "),
        platform_key(),
        platform_key()
    )
}

/// 解析平台条目：取本构建对应的平台键；便携版优先 portableUrl。
fn platform_entry(manifest: &UpdateManifest) -> Option<(PlatformEntry, bool)> {
    let entry = manifest.platforms.get(platform_key())?;
    let portable = crate::paths::is_portable();
    let available = if portable {
        !entry.portable_url.is_empty()
    } else {
        !entry.url.is_empty()
    };
    if !available {
        return None;
    }
    Some((entry.clone(), portable))
}

/// 一份更新清单对当前构建意味着什么。
///
/// ## 为什么要三分而不是 `is_newer() -> bool`
///
/// 原来是一个 bool，把**三件不同的事**压成同一个 `false`：
/// ① 清单版本不比当前新（真的已是最新）② 被 `minimumUpgradable` 跳级保护拦下
/// ③ 清单里没有本平台的包。三者都走 `UpdateInfo::none` → 前端一律显示
/// 「已是最新版本」。
///
/// ②③ 是**谎报成功**：确实有新版，只是这台机器升不了。它和 ① 给用户的
/// 行动建议完全相反（① 什么都不用做，②③ 得去换台机器 / 找作者发包）。
/// 站着「有新版但你拿不到」时唯一该做的就是把它说出来。
///
/// 纯函数、不碰网络与配置，故可完整单测——这正是原来那条 bool 掩盖掉的地方。
#[derive(Debug)]
enum ManifestVerdict {
    /// 没有比当前更新的版本
    UpToDate,
    /// 有更新，但当前构建拿不到；附可直接展示给用户的原因
    Unreachable(String),
    /// 有更新且当前构建可升
    Upgradeable {
        entry: PlatformEntry,
        portable: bool,
    },
}

fn judge(manifest: &UpdateManifest, current: &str) -> ManifestVerdict {
    use std::cmp::Ordering;
    if manifest.version.is_empty() {
        return ManifestVerdict::UpToDate;
    }
    if version_cmp(&manifest.version, current) != Ordering::Greater {
        return ManifestVerdict::UpToDate;
    }
    // 到这里已经确定「有新版」。以下两种是「有新版但拿不到」。
    if !manifest.minimum_upgradable.is_empty()
        && version_cmp(current, &manifest.minimum_upgradable) == Ordering::Less
    {
        return ManifestVerdict::Unreachable(format!(
            "更新源 v{} 要求最低可升级 v{}，当前 v{} 低于下限，被跳级保护拦下（不是「已是最新」：确有新版，但需先手动升级到 v{} 以上的版本）",
            manifest.version, manifest.minimum_upgradable, current, manifest.minimum_upgradable
        ));
    }
    match platform_entry(manifest) {
        Some((entry, portable)) => ManifestVerdict::Upgradeable { entry, portable },
        None => ManifestVerdict::Unreachable(no_platform_entry_reason(manifest)),
    }
}

/// 检查是否有可用更新。验签失败 / 通信失败时**静默**返回"无更新"
/// （记日志不打扰用户），只有真正命中才广播 `update-available`。
///
/// `manual`：是否由用户手动触发（About 页「检查更新」）。手动检查时
/// **忽略**「跳过此版本」记录——用户主动查看，应能再次看到该版本。
#[tauri::command]
pub async fn check_for_update(
    app: tauri::AppHandle,
    manual: Option<bool>,
) -> Result<UpdateInfo, String> {
    let manual = manual.unwrap_or(false);
    let current = current_version(&app);
    // 更新清单来自平台服务端（国内）：强制直连，别被用户本地代理带沟里（见 crate::net）
    let client = crate::net::direct()
        .timeout(Duration::from_secs(20))
        .build()
        .map_err(|e| format!("HTTP 客户端初始化失败: {e}"))?;

    let (manifest, _) = match fetch_manifest(&client).await {
        Ok(m) => m,
        Err(e) => {
            log::warn!("更新检查失败（静默）: {e}");
            // 手动检查（About 页）应向上报错，让前端提示失败原因；
            // 仅自动检查静默降级为"无更新"
            if manual {
                return Err(e);
            }
            return Ok(UpdateInfo::none(&current));
        }
    };

    // 先问「有没有新版」，再问「这台机器能不能升」——两者必须分开，见 ManifestVerdict。
    // 顺序保持原样：跳级/缺条目都在「跳过此版本」与「稍后再提示」之后判定，
    // 免得暂停窗口把一条真实故障也吞掉（那正是原来查不出来的原因之一）。
    if version_cmp(&manifest.version, &current) != std::cmp::Ordering::Greater
        || manifest.version.is_empty()
    {
        log::info!("已是最新版本（当前 v{current}，源 v{}）", manifest.version);
        return Ok(UpdateInfo::none(&current));
    }
    // 用户「跳过此版本」：与清单目标版本一致时不再提示（记录到 config）。
    // 仅自动检查时生效——手动检查更新应能再次看到并选择升级。
    if !manual && crate::config::load().skipped_update_version == manifest.version {
        log::info!("版本 v{} 已被用户跳过，不再提示", manifest.version);
        return Ok(UpdateInfo::none(&current));
    }
    // 「稍后再提示」：暂停窗口内自动检查不弹（见 snooze_update；手动检查不受影响）。
    // 到期后由 snooze_update 安排的补检或 4h 周期循环再次提示。
    if !manual {
        let snooze_until = crate::config::load().update_snooze_until_ms;
        if snooze_until > 0 && now_epoch_ms() < snooze_until {
            log::info!("更新提示处于「稍后再提示」暂停窗口内，本轮不弹");
            return Ok(UpdateInfo::none(&current));
        }
    }
    let (entry, portable) = match judge(&manifest, &current) {
        ManifestVerdict::Upgradeable { entry, portable } => (entry, portable),
        // 「拿不到」不是「没有」：报出来，别谎报已是最新。手动检查由 AboutSection
        // 的 catch 展示原因；自动检查的调用方本就 `let _ =` 吞掉（与拉取失败同口径），
        // 差异只体现在日志。
        ManifestVerdict::Unreachable(e) => {
            log::warn!("{e}");
            return Err(e);
        }
        // 上面已用 version_cmp 判过「无新版」，走到这里只可能是判据漂移。
        // 宁可报错也不要悄悄说「已是最新」——那条路正是本函数要消灭的谎报。
        ManifestVerdict::UpToDate => {
            return Err(format!(
                "更新判定内部不一致：源 v{} 不比当前 v{} 新，却走到了可升级分支",
                manifest.version, current
            ));
        }
    };

    let info = UpdateInfo {
        available: true,
        version: manifest.version.clone(),
        notes: manifest.notes.clone(),
        size: if portable { entry.portable_size } else { entry.size },
        portable,
        ready: false,
        current,
    };
    log::info!(
        "发现新版本 v{}（{}，{:.1} MB，便携版={}）",
        manifest.version,
        if portable { "portable" } else { "standard" },
        (if portable { entry.portable_size } else { entry.size }) as f64 / 1048576.0,
        portable
    );
    let _ = app.emit("update-available", &info);
    Ok(info)
}

fn now_epoch_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// 「稍后再提示」：把更新弹窗暂停 30 分钟（写 `update_snooze_until_ms`），到点后
/// 主动补一次自动检查。为什么补检：静默检查循环是 4h 一跳，不补检的话「稍后」
/// 实际会变成「最多 4 小时后」。手动检查（About 页）不受暂停窗口影响。
#[tauri::command]
pub fn snooze_update(app: tauri::AppHandle) -> Result<(), String> {
    const SNOOZE_MINUTES: i64 = 30;
    {
        let _guard = crate::config::lock();
        let mut cfg = crate::config::load();
        cfg.update_snooze_until_ms = now_epoch_ms() + SNOOZE_MINUTES * 60_000;
        crate::config::save(&cfg)?;
    }
    // 取一个本次点击专属的代数；期间又点了「稍后再提示」的话代数会变大，
    // 本任务到点后据此直接退出，只留最新一次去补检
    let generation = SNOOZE_GENERATION.fetch_add(1, Ordering::AcqRel) + 1;
    let handle = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(Duration::from_secs((SNOOZE_MINUTES * 60) as u64)).await;
        if SNOOZE_GENERATION.load(Ordering::Acquire) != generation {
            log::info!("「稍后再提示」期间有更新的点击，本次补检跳过（已由最新那次顺延）");
            return;
        }
        // 两条分支都要落日志：`if let Ok(..)` 会把 Err 整条吞掉，补检既没提示
        // 也没更新时，日志上会留一段**静默缺口**——而这正是「自动更新看着一切
        // 正常、其实早就不工作」最隐蔽的一种形态（check_for_update 内部已记
        // WARN，这里补的是「补检本身没跑成」这一层）。
        match check_for_update(handle, None).await {
            Ok(info) => log::info!(
                "「稍后再提示」到期补检：{}",
                if info.available { "仍有更新" } else { "无更新" }
            ),
            Err(e) => log::warn!("「稍后再提示」到期补检未能完成：{e}"),
        }
    });
    log::info!("更新提示已推迟 {} 分钟", SNOOZE_MINUTES);
    Ok(())
}

/// 下载可用更新（`check_for_update` 命中后调用）。
/// 重新拉取清单并验签（保证下载依据仍是最新签名清单）→ 取对应平台条目 →
/// 流式下载 → 边下边算 sha256 校验 → 写 `updates/<version>/m-hub.zip` →
/// 写 `.pending.json` → 广播 `update-ready`。
///
/// 前端传入 `version` 仅为目标版本校验：下载时若清单中的目标版本与请求不符
/// 则中止（防并发/竞态下下载旧条目）。
#[tauri::command]
pub async fn download_update(
    app: tauri::AppHandle,
    version: String,
) -> Result<UpdateInfo, String> {
    // 互斥进入：慢链路下载会持续数分钟，期间重复触发只会截断重下
    if DOWNLOAD_IN_FLIGHT
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return Err("已有下载任务进行中，请等待其完成（或重启应用中断）".to_string());
    }
    let _download_guard = DownloadGuard;
    let current = current_version(&app);
    // 不设总超时：慢链路（~30KB/s）下载 8.7MB 需数分钟，总超时必然误杀慢而活跃的下载；
    // 改为连接超时 + 空闲读超时（30s 收不到新数据才断），下面的流式读取同样吃 read_timeout
    // 安装包由我们自己的分发（COS 国内）提供：同样强制直连（见 crate::net）
    let client = crate::net::direct()
        .connect_timeout(Duration::from_secs(15))
        .read_timeout(Duration::from_secs(30))
        .build()
        .map_err(|e| format!("HTTP 客户端初始化失败: {e}"))?;

    // 断点续传 + 自动重试：慢链路上长连接易被中途掐断（reqwest 统一报
    // "error decoding response body"），每次尝试都从 tmp 已有字节数 Range 续传（R2 支持 206），
    // 失败退避后自动再试直到成功或重试额度用完；残片不小于预期总大小时视为脏文件丢弃
    const MAX_ATTEMPTS: u32 = 8;
    const RETRY_DELAY: Duration = Duration::from_secs(2);

    // 清单 + 签名两次 GET：接入层瞬时异常（一次 404/超时）不该让整单失败，轻量重试兜底
    const MANIFEST_ATTEMPTS: u32 = 3;
    let (manifest, _) = {
        let mut last_err = String::new();
        let mut got = None;
        for attempt in 1..=MANIFEST_ATTEMPTS {
            match fetch_manifest(&client).await {
                Ok(v) => {
                    got = Some(v);
                    break;
                }
                Err(e) => {
                    last_err = e;
                    log::warn!("更新清单拉取第 {attempt}/{MANIFEST_ATTEMPTS} 次失败: {last_err}");
                    if attempt < MANIFEST_ATTEMPTS {
                        tokio::time::sleep(RETRY_DELAY).await;
                    }
                }
            }
        }
        match got {
            Some(v) => v,
            None => return Err(last_err),
        }
    };
    if !manifest.version.eq(&version) {
        return Err(format!(
            "更新源已变更（目标 v{version}，清单 v{}），请重新检查更新",
            manifest.version
        ));
    }
    let (entry, portable) = platform_entry(&manifest).ok_or_else(|| {
        format!("{}（本次目标 v{version}）", no_platform_entry_reason(&manifest))
    })?;
    let (url, sha256) = if portable {
        (entry.portable_url.clone(), entry.portable_sha256.clone())
    } else {
        (entry.url.clone(), entry.sha256.clone())
    };
    if url.is_empty() {
        return Err("更新清单缺少下载地址".to_string());
    }

    // 目录准备：updates/<version>/
    let ver_dir = updates_root()?.join(&version);
    std::fs::create_dir_all(&ver_dir).map_err(|e| format!("创建更新目录失败: {e}"))?;
    let zip_path = ver_dir.join("m-hub.zip");
    let tmp_zip = ver_dir.join("m-hub.zip.tmp");

    // 失败退避后自动再试直到成功或重试额度用完；残片不小于预期总大小时视为脏文件丢弃
    let manifest_size = if portable { entry.portable_size } else { entry.size };
    let mut attempt: u32 = 0;
    let downloaded;
    loop {
        attempt += 1;
        // 每次尝试重新读残片长度（上次尝试可能又推进了一些）
        let mut offset: u64 = 0;
        if tmp_zip.is_file() {
            let existing = std::fs::metadata(&tmp_zip).map(|m| m.len()).unwrap_or(0);
            if existing > 0 && (manifest_size == 0 || existing < manifest_size) {
                offset = existing;
            } else if existing > 0 {
                let _ = std::fs::remove_file(&tmp_zip);
            }
        }

        let mut request = client.get(&url);
        if offset > 0 {
            request = request.header("Range", format!("bytes={offset}-"));
        }
        let resp = match request.send().await {
            Ok(r) => r,
            Err(e) => {
                if attempt < MAX_ATTEMPTS {
                    log::warn!(
                        "更新下载第 {attempt} 次尝试失败（{e}），{}s 后从 {} 字节处续传",
                        RETRY_DELAY.as_secs(),
                        offset
                    );
                    tokio::time::sleep(RETRY_DELAY).await;
                    continue;
                }
                return Err(format!("下载失败: {e}"));
            }
        };
        // 残片与服务端文件不一致（如清单与服务端包不同步）→ Range 越界 416：丢弃残片重下
        if resp.status() == reqwest::StatusCode::RANGE_NOT_SATISFIABLE {
            let _ = std::fs::remove_file(&tmp_zip);
            if attempt < MAX_ATTEMPTS {
                log::warn!("更新续传偏移越界（416），已丢弃残片重新下载");
                continue;
            }
            return Err("更新残片与服务端文件不一致，已重置下载，请重试".to_string());
        }
        if !resp.status().is_success() {
            // 非 2xx（含 404/403 瞬态）同样退避重试：接入层抖动一次不该让用户手点失败
            if attempt < MAX_ATTEMPTS {
                log::warn!(
                    "更新下载第 {attempt} 次尝试返回 HTTP {}，{}s 后重试（从 {offset} 字节处续传）",
                    resp.status(),
                    RETRY_DELAY.as_secs()
                );
                tokio::time::sleep(RETRY_DELAY).await;
                continue;
            }
            return Err(format!("下载失败: HTTP {}", resp.status()));
        }
        // 服务器不支持 Range（回 200 全量而非 206）时清零从头下
        let resumed = offset > 0 && resp.status() == reqwest::StatusCode::PARTIAL_CONTENT;
        if offset > 0 && !resumed {
            offset = 0;
        }
        let total = if manifest_size > 0 {
            Some(manifest_size)
        } else {
            resp.content_length().map(|l| l + offset)
        };
        let mut file = if resumed {
            std::fs::OpenOptions::new()
                .append(true)
                .open(&tmp_zip)
                .map_err(|e| format!("打开续传文件失败: {e}"))?
        } else {
            std::fs::File::create(&tmp_zip).map_err(|e| format!("创建文件失败: {e}"))?
        };
        let mut stream = resp.bytes_stream();
        let mut done: u64 = offset;
        let mut last_emit: u64 = offset;
        let mut interrupted: Option<String> = None;
        while let Some(chunk) = stream.next().await {
            match chunk {
                Ok(bytes) => {
                    file.write_all(&bytes).map_err(|e| format!("写入失败: {e}"))?;
                    done += bytes.len() as u64;
                    if done - last_emit >= 262_144 || total == Some(done) {
                        last_emit = done;
                        let _ = app.emit(
                            "update-download-progress",
                            crate::market::DownloadProgress {
                                id: "m-hub".to_string(),
                                received: done,
                                total,
                            },
                        );
                    }
                }
                Err(e) => {
                    // 连接被掐断：保留已写入部分，退避后续传
                    interrupted = Some(format!("下载中断: {e}"));
                    break;
                }
            }
        }
        file.flush().map_err(|e| format!("落盘失败: {e}"))?;
        drop(file);
        match interrupted {
            Some(msg) => {
                if attempt < MAX_ATTEMPTS {
                    log::warn!(
                        "更新下载第 {attempt} 次尝试中断于 {} 字节，{}s 后续传",
                        done,
                        RETRY_DELAY.as_secs()
                    );
                    tokio::time::sleep(RETRY_DELAY).await;
                    continue;
                }
                return Err(msg);
            }
            None => {
                if last_emit != done {
                    let _ = app.emit("update-download-progress", crate::market::DownloadProgress {
                        id: "m-hub".to_string(),
                        received: done,
                        total,
                    });
                }
                downloaded = done;
                break;
            }
        }
    }
    if manifest_size != 0 && downloaded != manifest_size {
        // 流正常结束但长度与清单不符：服务端包与清单可能不同步，续传救不了系统性偏差
        return Err(format!(
            "下载不完整: 收到 {downloaded} 字节，预期 {manifest_size} 字节（更新源与清单可能不同步）"
        ));
    }
    // 校验完整性（清单背书）。续传时前段字节未经 hasher，全量从盘上文件重算（8MB 级瞬间完成）
    if !sha256.is_empty() {
        let whole = std::fs::read(&tmp_zip).map_err(|e| format!("读取下载文件失败: {e}"))?;
        let mut hasher = Sha256::new();
        hasher.update(&whole);
        let actual = to_hex(&hasher.finalize());
        drop(whole);
        if !actual.eq_ignore_ascii_case(&sha256) {
            let _ = std::fs::remove_file(&tmp_zip);
            return Err(format!(
                "下载校验失败（sha256 不匹配）\n期望: {sha256}\n实际: {actual}\n更新包可能被篡改或损坏，已中止。"
            ));
        }
    }
    // 就位（rename 原子）
    std::fs::rename(&tmp_zip, &zip_path).map_err(|e| format!("更新包就位失败: {e}"))?;

    // 写待应用标记
    let pending = serde_json::json!({
        "version": manifest.version.clone(),
        "zipPath": zip_path.to_string_lossy().to_string(),
        "portable": portable,
    });
    std::fs::write(pending_file()?, serde_json::to_vec_pretty(&pending).unwrap_or_default())
        .map_err(|e| format!("写入更新标记失败: {e}"))?;

    log::info!(
        "更新已下载就绪: v{} -> v{}（{} 字节，便携版={}）",
        current,
        manifest.version,
        downloaded,
        portable
    );
    let info = UpdateInfo {
        available: true,
        version: manifest.version.clone(),
        notes: manifest.notes.clone(),
        size: if portable { entry.portable_size } else { entry.size },
        portable,
        ready: true,
        current,
    };
    let _ = app.emit("update-ready", &info);
    Ok(info)
}

/// 当前更新状态（不发起网络请求）：读取本地标记与配置，供前端展示。
#[tauri::command]
pub fn get_update_status(app: tauri::AppHandle) -> Result<UpdateInfo, String> {
    let current = current_version(&app);
    let mut info = UpdateInfo::none(&current);
    if let Some(pending) = read_pending() {
        info.available = true;
        info.version = pending.version;
        info.portable = pending.portable;
        info.ready = true;
    }
    Ok(info)
}

/// 记录用户「跳过此版本」：将该版本号持久化到 config，后续检查更新时不再提示该版本。
#[tauri::command]
pub fn skip_update_version(version: String) -> Result<(), String> {
    let version = version.trim().to_string();
    if version.is_empty() {
        return Err("版本号不能为空".to_string());
    }
    let _guard = crate::config::lock();
    let mut config = crate::config::load();
    config.skipped_update_version = version.clone();
    crate::config::save(&config)?;
    log::info!("已跳过版本 v{}，后续检查不再提示", version);
    Ok(())
}

/// 待应用更新标记。
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct PendingUpdate {
    version: String,
    #[serde(default)]
    zip_path: String,
    #[serde(default)]
    portable: bool,
}

fn read_pending() -> Option<PendingUpdate> {
    let path = pending_file().ok()?;
    if !path.is_file() {
        return None;
    }
    let content = std::fs::read_to_string(&path).ok()?;
    serde_json::from_str::<PendingUpdate>(&content).ok()
}

/// 应用待更新版本（每次启动早期调用，幂等）。
/// Windows 允许 rename 正在运行的 exe：`exe → exe.old`、`新 exe → exe`。
/// 任一步失败回滚（.old 还原）并保留标记，下次启动重试。
/// 成功则删除标记 + 更新包，并立即以新 exe 拉起子进程、当前进程退出——
/// 当前进程镜像已被改名成 .old，原地继续跑的仍是旧版本（「点了重启
/// 还是旧版」的根因）。新实例再进这里时无标记，直接跳过，不会循环。
/// 另外每次进入先清理上次升级残留的 .old（此刻已无进程占用，可安全删）。
/// 拉起走 `relaunch_app`（先释放 single-instance 互斥，见其注释）。
pub fn apply_pending_update(app: &tauri::AppHandle, current_version: &str) {
    // 清理上次自替换残留的 .old：升级时运行中进程的镜像被改名成了
    // exe.old，Windows 不允许删除运行中进程的镜像文件，当时必删失败；
    // 等到下一次启动它已无进程占用，这里统一清掉，避免 exe 目录长期躺着 .old
    if let Ok(exe_path) = std::env::current_exe() {
        if let Some(exe_dir) = exe_path.parent() {
            let _ = std::fs::remove_file(exe_dir.join(exe_file_name_with_old(&exe_path)));
        }
    }
    let Some(pending) = read_pending() else {
        return;
    };
    // 待应用版本不高于当前版本（用户可能已手动装了更高版本）：清掉过期
    // 更新包与标记，防止启动时旧包把新装的 exe 又覆盖回旧版（降级）
    if version_cmp(&pending.version, current_version) != std::cmp::Ordering::Greater {
        log::info!(
            "待应用更新 v{} 不高于当前版本 v{}，清理过期更新包",
            pending.version,
            current_version
        );
        discard_pending(&pending);
        return;
    }
    let zip_path = PathBuf::from(&pending.zip_path);
    if !zip_path.is_file() {
        log::warn!("待应用更新包不存在（{}），清理标记", zip_path.display());
        discard_pending(&pending);
        return;
    }

    let exe_path = match std::env::current_exe() {
        Ok(p) => p,
        Err(e) => {
            log::warn!("无法定位当前 exe（{e}），跳过本次应用更新");
            return;
        }
    };
    let exe_dir = match exe_path.parent() {
        Some(d) => d.to_path_buf(),
        None => {
            log::warn!("无法定位 exe 目录，跳过本次应用更新");
            return;
        }
    };

    // 解包新版本到同一目录下的隐藏暂存目录
    let version_dir = match updates_root() {
        Ok(d) => d.join(&pending.version),
        Err(_) => return,
    };
    let staging = version_dir.join(".staging");
    if staging.exists() {
        let _ = std::fs::remove_dir_all(&staging);
    }
    let staging_tmp = version_dir.join(".staging.tmp");
    if staging_tmp.exists() {
        let _ = std::fs::remove_dir_all(&staging_tmp);
    }
    if let Err(e) = crate::market::extract_zip_read(&zip_path, &staging_tmp) {
        log::warn!("更新包解包失败（{}），跳过本次应用更新，稍后重试", e);
        let _ = std::fs::remove_dir_all(&staging_tmp);
        return;
    }

    // 解包结果里找到真正要安装的产物。**两平台的产物形态不同**：
    // - Windows：可执行文件 `.exe`（根或一层子目录，产物名不定，取体积最大者）；
    // - macOS：整个 `m-hub.app` 包（签名/Info.plist/Frameworks 都在里面，
    //   只换内层二进制会让包签名失效并被 Gatekeeper 拒启）。
    #[cfg(target_os = "macos")]
    let found = locate_new_app(&staging_tmp);
    #[cfg(target_os = "macos")]
    let found_desc = ".app 包";
    #[cfg(target_os = "windows")]
    let found = locate_new_exe(&staging_tmp);
    #[cfg(target_os = "windows")]
    let found_desc = "可执行文件（.exe）";

    let Some(found) = found else {
        log::warn!("更新包内未找到{found_desc}，跳过应用更新");
        let _ = std::fs::remove_dir_all(&staging_tmp);
        return;
    };
    // 校验新产物不是当前正在运行的自己（防更新包误打包旧版）
    if found == exe_path
        || current_app_bundle_target().is_some_and(|cur| cur == found)
    {
        log::warn!("更新包产物与当前一致，疑似打包异常，跳过");
        let _ = std::fs::remove_dir_all(&staging_tmp);
        return;
    }

    if let Err(e) = std::fs::rename(&staging_tmp, &staging) {
        log::warn!("更新暂存目录就位失败（{e}），跳过本次应用更新");
        let _ = std::fs::remove_dir_all(&staging_tmp);
        return;
    }
    // 新产物相对暂存根目录的偏移（可能在子目录），rename 后同样相对 staging 拼接
    let rel = found
        .strip_prefix(&staging_tmp)
        .unwrap_or_else(|_| std::path::Path::new(found.file_name().unwrap_or_default()));
    let incoming = staging.join(rel);

    let target = build_replace_target(&exe_path, &exe_dir, &incoming);
    if !target.commit() {
        let _ = std::fs::remove_dir_all(&staging);
        return;
    }
    let _ = std::fs::remove_dir_all(&staging);
    let _ = std::fs::remove_file(&zip_path);
    let _ = remove_pending_file();

    log::info!("应用已自替换升级到 v{}", pending.version);

    // 4) 立即以新 exe 拉起子进程并退出：当前进程镜像已被改名成 .old，
    //    原地继续跑的仍是旧版本——不换进程的话，用户重启后看到的还是
    //    旧版界面（旧缺陷）。新实例启动时无标记，直接跳过，不会循环。
    //    注意：新实例插件 setup（CreateMutexW）前必须释放 single-instance
    //    互斥与隐藏窗口，否则被判第二实例自杀退出——升级后「没有自动重启」
    //    的根因（relaunch_app 内部「先 spawn 成功再 destroy」保证顺序）。
    relaunch_app(app);
}

/// 重启当前进程（以磁盘上的 exe 重新拉起后退出当前进程）。
/// 供三处共用：① updater 自替换成功后接管启动；② 「立即重启」按钮
/// （restart_app 命令，数据目录迁移/更新就绪后重启）。
///
/// single-instance 插件的 mutex（`m-hub-sim`）与隐藏窗口在插件 setup 时创建，
/// 正常由插件的 `RunEvent::Exit` on_event 销毁。但这里是用 `std::process::exit(0)`
/// 硬退（不触发 RunEvent::Exit / 事件循环），句柄只能靠 OS 在进程终止时回收——
/// 若退出前不释放，spawn 的新实例启动到插件 setup（CreateMutexW）往往先于旧进程
/// 句柄清理完成，被判为「第二实例」自杀退出，升级/重启表现为「没有自动重启」
/// （历史反复出现的现象）。
///
/// 因此顺序必须是：**先 spawn 成功，再 destroy（释放互斥与隐藏窗口），最后退出**：
/// - spawn 成功到新进程真正执行到插件 setup 需要数十毫秒（进程创建 + CRT/运行时
///   初始化），而 destroy 紧跟 spawn 返回之后（微秒级两条 Win32 调用），必然先于
///   新实例的 CreateMutexW——新实例必然成为主实例；
/// - 反过来「先 destroy 再 spawn」一旦 spawn 失败（如被安全软件拦截），本进程
///   继续运行但互斥与窗口已释放：插件判定第二实例依赖「mutex 存在 + 能找到隐藏
///   窗口」，两者都没了之后再手动开一个实例不会被拦截——出现双实例（明确不接受）。
///   所以失败路径绝不触碰 destroy，本进程仍是受保护的唯一主实例。
pub fn relaunch_app(app: &tauri::AppHandle) {
    // 直接 exit(0) 不会触发 RunEvent::Exit，service 后端子进程需在此手动停掉，
    // 否则 Node 子进程残留（标准退出路径由 lib.rs RunEvent::Exit → stop_all 兜底）。
    // setup 早期（apply_pending_update 路径）ServiceState 尚未 manage：try_state 判空跳过。
    if app.try_state::<crate::service::ServiceState>().is_some() {
        crate::service::stop_all(app);
    }
    let Ok(exe) = std::env::current_exe() else {
        log::error!("重启失败：无法定位当前 exe");
        return;
    };
    // macOS 走 LaunchServices（`open <包路径>`）而不是直接 spawn 包内二进制：
    // 直接 spawn 虽也能跑，但会绕过 LaunchServices 的应用注册流程，表现为
    // Dock 图标延迟出现、窗口不自动前置、Apple 事件（打开 URL 等）丢失。
    // `-n` 强制新实例：单实例插件的互斥还在旧进程里，`open` 默认会「激活已运行
    // 的那个」而不是新建，那样新版本永远起不来。
    #[cfg(target_os = "macos")]
    let spawned: Result<(), ()> = match current_app_bundle() {
        Some(bundle) => std::process::Command::new("open")
            .arg("-n")
            .arg(&bundle)
            .status()
            .ok()
            .filter(|st| st.success())
            .map(|_| ())
            .ok_or(()),
        // 开发态（不在 .app 内）：没有包路径可交给 LaunchServices，直接 spawn 二进制
        None => std::process::Command::new(&exe)
            .args(std::env::args_os().skip(1))
            .spawn()
            .ok()
            .map(|_| ())
            .ok_or(()),
    };
    #[cfg(not(target_os = "macos"))]
    let spawned: Result<(), ()> = std::process::Command::new(&exe)
        .args(std::env::args_os().skip(1))
        .spawn()
        .ok()
        .map(|_| ())
        .ok_or(());

    match spawned {
        Ok(()) => {
            // 子进程已创建：此刻释放互斥与隐藏窗口（幂等），保证新实例的
            // CreateMutexW 拿到的是全新互斥、必然成为主实例；随后本进程退出。
            tauri_plugin_single_instance::destroy(app);
            // 托盘图标同样要补发 NIM_DELETE：relaunch 走硬退不触发 RunEvent::Exit，
            // 不清理的话自动重启/手动重启后托盘会留一个悬停才消失的幽灵图标。
            // 新实例按自己的 hwnd+id 重新 NIM_ADD，二者互不���响。
            let _ = app.remove_tray_by_id("main-tray");
            log::info!("已拉起新进程接管启动，当前进程退出");
            std::process::exit(0);
        }
        Err(()) => {
            log::error!("重启拉起失败，请手动重启应用");
        }
    }
}

fn remove_pending_file() -> std::io::Result<()> {
    if let Ok(p) = pending_file() {
        let _ = std::fs::remove_file(&p);
    }
    Ok(())
}

/// 跨卷安全移动：同卷走原子 rename；跨卷（数据根与 exe 不同盘）rename 报
/// os error 17「系统无法将文件移到不同的磁盘驱动器」，退化为复制——调用点
/// 已把旧 exe 挪走、目标路径空出，运行中的 exe 不锁定新建文件，复制可成功。
fn move_file(src: &Path, dst: &Path) -> std::io::Result<()> {
    match std::fs::rename(src, dst) {
        Ok(()) => Ok(()),
        Err(rename_err) => std::fs::copy(src, dst).map(|_| ()).map_err(|copy_err| {
            log::warn!("rename 失败（{rename_err}），复制兜底也失败（{copy_err}）");
            copy_err
        }),
    }
}

/// 清理一份待应用更新（zip、解包目录与标记）。
fn discard_pending(pending: &PendingUpdate) {
    if !pending.zip_path.is_empty() {
        let _ = std::fs::remove_file(PathBuf::from(&pending.zip_path));
    }
    if let Ok(root) = updates_root() {
        let _ = std::fs::remove_dir_all(root.join(&pending.version));
    }
    let _ = remove_pending_file();
}

fn exe_file_name_with_old(exe: &Path) -> std::ffi::OsString {
    let mut name = exe.file_name().unwrap_or_default().to_os_string();
    name.push(".old");
    name
}

/// 当前正在运行的 `.app` 包根目录（macOS）。
///
/// `current_exe()` 给出的是包**内部**的二进制
/// （`/Applications/m-hub.app/Contents/MacOS/m-hub`），而 macOS 的分发单元是整个
/// `.app`：只换内层二进制会留下过期的 `Info.plist` / `Frameworks` / 图标，
/// **并且会破坏代码签名**（签名覆盖整个包，改一个字节签名即失效，
/// Gatekeeper 随即拒绝启动）。所以自替换的单位必须是包。
///
/// 从当前 exe 向上找到第一个以 `.app` 结尾的祖先目录即包根；
/// 开发态（`cargo run`）不落在 .app 里，返回 None —— 此时走「就地换二进制」的降级路径。
#[cfg(target_os = "macos")]
pub fn current_app_bundle() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    exe.ancestors()
        .find(|p| {
            p.extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| e.eq_ignore_ascii_case("app"))
        })
        .map(|p| p.to_path_buf())
}

/// 在解包目录（根或一层）里找 `.app` 包（macOS 的分发单元）。
/// 有多个时取体积最大的（防误取只读的小样例包）。
#[cfg(target_os = "macos")]
fn locate_new_app(dir: &Path) -> Option<PathBuf> {
    let mut best: Option<(PathBuf, u64)> = None;
    let mut consider = |p: &Path| {
        let is_app = p
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| e.eq_ignore_ascii_case("app"));
        if is_app && p.is_dir() {
            let size = dir_size(p);
            if best.as_ref().map(|(_, s)| size > *s).unwrap_or(true) {
                best = Some((p.to_path_buf(), size));
            }
        }
    };
    if let Ok(entries) = std::fs::read_dir(dir) {
        for e in entries.flatten() {
            consider(&e.path());
        }
    }
    best.map(|(p, _)| p)
}

/// 目录总字节数（递归一层即可：.app 的重量级内容在 Contents/ 子目录里）
fn dir_size(dir: &Path) -> u64 {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return 0;
    };
    entries
        .flatten()
        .map(|e| {
            let p = e.path();
            std::fs::metadata(&p)
                .map(|m| {
                    if m.is_file() {
                        m.len()
                    } else {
                        dir_size(&p)
                    }
                })
                .unwrap_or(0)
        })
        .sum()
}

/// 自替换的落地目标：macOS 是 `.app` 包目录，其余是 exe 文件。
///
/// 抽象成「目标路径 + 备份名 + 一次移动」三件事，两平台共用上面那套
/// 解包 / 校验 / 回滚流程——差别只有移动的对象。
struct ReplaceTarget {
    /// 要被替换掉的那个路径（.app 目录或 exe 文件）
    current: PathBuf,
    /// 备份名后缀（Windows `.old` 是文件，macOS 是同名目录）
    backup: PathBuf,
    /// 新版本解包后的对应产物
    incoming: PathBuf,
}

impl ReplaceTarget {
    /// 提交替换。成功返回 true，失败时**已回滚**、磁盘保持旧版本可启动。
    fn commit(&self) -> bool {
        if self.current == self.incoming {
            log::warn!("更新目标与当前一致，疑似打包异常，跳过");
            return false;
        }
        let _ = remove_path(&self.backup);
        if let Err(e) = std::fs::rename(&self.current, &self.backup) {
            log::warn!("备份当前安装目录失败（{e}），跳过本次应用更新");
            return false;
        }
        if let Err(e) = move_file(&self.incoming, &self.current) {
            log::error!("安装新版本失败（{e}），回滚到旧版本");
            let _ = std::fs::rename(&self.backup, &self.current);
            return false;
        }
        // 备份残留：Windows 上运行中的镜像删不掉（预期），macOS 上是旧包目录，
        // 同样留着——下次启动开头统一清理
        let _ = remove_path(&self.backup);
        true
    }
}

/// 当前应被替换的「分发单元」路径：macOS 是 `.app` 包，Windows 是 exe。
/// 仅用于「更新包产物 == 自己」的重合检测（防止把旧包当新包发出去）。
#[cfg(target_os = "macos")]
fn current_app_bundle_target() -> Option<PathBuf> {
    current_app_bundle()
}
#[cfg(not(target_os = "macos"))]
fn current_app_bundle_target() -> Option<PathBuf> {
    None
}

/// 组装自替换目标。macOS 换整个 `.app` 包，Windows 换单个 exe。
#[cfg(target_os = "macos")]
fn build_replace_target(exe: &Path, _exe_dir: &Path, incoming: &Path) -> ReplaceTarget {
    match current_app_bundle() {
        // 正常分发态：换包。备份名 `m-hub.app.old` 与包同级，
        // 便于用户在更新失败时自己把它改回 .app（并被文档告知）
        Some(bundle) => ReplaceTarget {
            backup: bundle.with_extension("app.old"),
            current: bundle,
            incoming: incoming.to_path_buf(),
        },
        // 开发态（cargo run / 直接跑二进制）：没有包可换，
        // 退化成换内层二进制——此时没有签名问题，也够用
        None => ReplaceTarget {
            backup: exe_dir(exe).join(exe_file_name_with_old(exe)),
            current: exe.to_path_buf(),
            incoming: incoming.to_path_buf(),
        },
    }
}

#[cfg(target_os = "windows")]
fn build_replace_target(exe: &Path, exe_dir: &Path, incoming: &Path) -> ReplaceTarget {
    ReplaceTarget {
        backup: exe_dir.join(exe_file_name_with_old(exe)),
        current: exe.to_path_buf(),
        incoming: incoming.to_path_buf(),
    }
}

fn exe_dir(exe: &Path) -> PathBuf {
    exe.parent().map(|p| p.to_path_buf()).unwrap_or_default()
}

/// 删除文件或目录（`remove_file` 对目录、`remove_dir_all` 对文件都会失败）
fn remove_path(p: &Path) -> std::io::Result<()> {
    if p.is_dir() {
        std::fs::remove_dir_all(p)
    } else {
        std::fs::remove_file(p)
    }
}

/// 在解包目录（根或一层）里找 `.exe` 文件；有多个时取体积最大的（防误取只读小工具）。
#[cfg(target_os = "windows")]
fn locate_new_exe(dir: &Path) -> Option<PathBuf> {
    let mut best: Option<(PathBuf, u64)> = None;
    let mut consider = |p: &Path| {
        if let Some(name) = p.file_name() {
            let lower = name.to_string_lossy().to_lowercase();
            if lower.ends_with(".exe") && std::fs::metadata(p).map(|m| m.is_file()).unwrap_or(false) {
                let size = std::fs::metadata(p).map(|m| m.len()).unwrap_or(0);
                if best.as_ref().map(|(_, s)| size > *s).unwrap_or(true) {
                    best = Some((p.to_path_buf(), size));
                }
            }
        }
    };
    if let Ok(entries) = std::fs::read_dir(dir) {
        for e in entries.flatten() {
            let p = e.path();
            consider(&p);
            if p.is_dir() {
                if let Ok(inner) = std::fs::read_dir(&p) {
                    for ie in inner.flatten() {
                        consider(&ie.path());
                    }
                }
            }
        }
    }
    best.map(|(p, _)| p)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_pending_camel_case() {
        // download_update 写入的是 camelCase 的 zipPath，读取必须能映射回 zip_path
        let json = serde_json::json!({
            "version": "0.3.1",
            "zipPath": "C:/x/updates/0.3.1/m-hub.zip",
            "portable": false,
        });
        let p: PendingUpdate = serde_json::from_value(json).unwrap();
        assert_eq!(p.version, "0.3.1");
        assert_eq!(p.zip_path, "C:/x/updates/0.3.1/m-hub.zip");
        assert!(!p.portable);
    }

    #[test]
    fn move_file_works_and_overwrites() {
        let dir = std::env::temp_dir().join(format!("mhub-move-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();

        // 目标已存在：Windows 上 rename 必败 → 退化复制并覆盖（跨卷场景的等效路径）
        let src = dir.join("src.exe");
        let dst = dir.join("dst.exe");
        std::fs::write(&src, b"new").unwrap();
        std::fs::write(&dst, b"old").unwrap();
        move_file(&src, &dst).unwrap();
        assert_eq!(std::fs::read(&dst).unwrap(), b"new");

        // 目标不存在：rename 直达（同卷主路径）
        let src2 = dir.join("src2.exe");
        let dst2 = dir.join("dst2.exe");
        std::fs::write(&src2, b"v2").unwrap();
        move_file(&src2, &dst2).unwrap();
        assert_eq!(std::fs::read(&dst2).unwrap(), b"v2");
        assert!(!src2.exists());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn parses_manifest() {
        let json = serde_json::json!({
            "schemaVersion": 1,
            "version": "0.4.0",
            "minimumUpgradable": "0.1.0",
            "notes": "v0.4.0: 新增更新中心",
            "platforms": {
                platform_key(): {
                    "url": "https://dist/m-hub-0.4.0.zip",
                    "portableUrl": "https://dist/m-hub-0.4.0-portable.zip",
                    "sha256": "abc",
                    "portableSha256": "def",
                    "size": 1024
                }
            }
        });
        let m = parse_manifest(&serde_json::to_vec(&json).unwrap()).unwrap();
        assert_eq!(m.schema_version, 1);
        assert_eq!(m.version, "0.4.0");
        assert_eq!(m.minimum_upgradable, "0.1.0");
        let e = m.platforms.get(platform_key()).expect("有平台条目");
        assert_eq!(e.portable_url, "https://dist/m-hub-0.4.0-portable.zip");
    }

    /// 平台键必须是「本构建真的能认出来」的那个。
    ///
    /// 这条是补一个真实的漏测：移植到 macOS 后 `platform_entry` 仍按
    /// `windows-x86_64` 取条目，于是 macOS 构建永远匹配不到自己 ——
    /// 而**没有任何测试覆盖这件事**（`parses_manifest` 只测 JSON 解析，
    /// 用的是 Windows 键当夹具，看起来一切正常）。
    #[test]
    fn current_build_matches_its_own_platform_entry() {
        // 升级清单的 schema 是 v1（市场清单才是 v2 —— 两份清单各有一套版本号）
        let json = serde_json::json!({
            "schemaVersion": 1,
            "version": "9.9.9",
            "platforms": {
                platform_key(): {
                    "url": "https://dist/m-hub.dmg",
                    "sha256": "abc",
                    "size": 1
                }
            }
        });
        let m = parse_manifest(&serde_json::to_vec(&json).unwrap()).unwrap();
        let (entry, _portable) = platform_entry(&m).expect("本构建应能认出自己的平台条目");
        assert_eq!(entry.url, "https://dist/m-hub.dmg");
    }

    /// 键名带 OS 段：本工程是 macOS，键必须是 `macos-*`。
    /// 键名是客户端与服务端之间**唯一的契约**，写错就两边永远对不上。
    #[test]
    fn platform_key_names_the_running_os() {
        let k = platform_key();
        if cfg!(target_os = "macos") {
            assert!(
                k.starts_with("macos-"),
                "macOS 构建的平台键必须以 macos- 开头，实际: {k}"
            );
            assert!(
                k.ends_with("-aarch64") || k.ends_with("-x86_64"),
                "键名必须带架构段，否则换机器就认不出来，实际: {k}"
            );
        }
        assert!(!k.contains("unknown"), "未识别的平台组合不该产出 unknown 键: {k}");
    }

    /// 只有 Windows 条目时，macOS 构建必须**取不到**（而不是误取到 Windows 的包，
    /// 那会下载错平台的安装包）。这一条钉住「宁可无更新，不可错更新」。
    #[test]
    fn does_not_fall_back_to_a_foreign_platform_entry() {
        if !cfg!(target_os = "macos") {
            return; // 只在 macOS 上有意义
        }
        let json = serde_json::json!({
            "schemaVersion": 1,
            "version": "9.9.9",
            "platforms": {
                "windows-x86_64": { "url": "https://dist/m-hub.exe", "sha256": "abc", "size": 1 }
            }
        });
        let m = parse_manifest(&serde_json::to_vec(&json).unwrap()).unwrap();
        assert!(
            platform_entry(&m).is_none(),
            "macOS 构建不得取用 Windows 平台条目"
        );
    }

    #[test]
    fn rejects_future_schema() {
        let json = serde_json::json!({
            "schemaVersion": 99,
            "version": "0.4.0",
            "platforms": {}
        });
        assert!(parse_manifest(&serde_json::to_vec(&json).unwrap()).is_err());
    }

    #[test]
    fn rejects_missing_version() {
        let json = serde_json::json!({ "schemaVersion": 1, "platforms": {} });
        assert!(parse_manifest(&serde_json::to_vec(&json).unwrap()).is_err());
    }

    /// ⚠️ 本组测试走的是**发布侧的真实产物形态**：
    /// 用临时密钥对现场签一份 update.json（与 `scripts/market-sign.sh` 做的事
    /// 同构），再依次过 `parse_manifest` → `signing::verify_detached_with` →
    /// `platform_entry` → `judge`。
    ///
    /// 为什么要这么写：以往那条手抄的 `TEST_SIGNATURE` 常量只能证明
    /// 「常量与自己自洽」——把整条链上任何一环改坏（平台键取错、schema 判错、
    /// 验签接错字节），它照样绿。**整条链从未被真实签名跑通过一次。**
    fn signed_manifest(version: &str, keys: &[&str]) -> (String, Vec<u8>, String) {
        use base64::engine::general_purpose::STANDARD as B64;
        use base64::Engine as _;
        use ed25519_dalek::{Signer as _, SigningKey};

        let mut platforms = serde_json::Map::new();
        for k in keys {
            platforms.insert(
                k.to_string(),
                serde_json::json!({
                    "url": format!("https://dist/m-hub-{k}.dmg"),
                    "sha256": "abc",
                    "size": 1
                }),
            );
        }
        let json = serde_json::json!({
            "schemaVersion": 1,
            "version": version,
            "minimumUpgradable": "0.1.0",
            "notes": "端到端联调",
            "platforms": platforms
        });
        // to_vec 而非 to_string：客户端验的是**原始字节**，签的也必须是同一份字节
        let content = serde_json::to_vec(&json).unwrap();

        let sk = SigningKey::from_bytes(&[7u8; 32]);
        let sig = sk.sign(&content).to_bytes();
        let pub_b64 = B64.encode(sk.verifying_key().as_bytes());
        (pub_b64, content, B64.encode(sig))
    }

    /// 端到端：真实签名 → 解析 → 验签 → 平台匹配 → 判为可升级。
    /// 每一环都走生产代码，不是把中间结果抄进断言。
    #[test]
    fn end_to_end_signed_update_manifest_reaches_upgradeable() {
        let (pub_b64, content, sig) =
            signed_manifest("9.9.9", &[platform_key()]);
        crate::signing::verify_detached_with(&pub_b64, &content, &sig)
            .expect("真实签名应通过生产验签逻辑");
        let m = parse_manifest(&content).expect("真实清单应能解析");
        match judge(&m, "0.7.2") {
            ManifestVerdict::Upgradeable { entry, .. } => {
                assert_eq!(entry.url, format!("https://dist/m-hub-{}.dmg", platform_key()));
            }
            other => panic!("应判为可升级，实际: {other:?}"),
        }
    }

    /// 公钥参数真的被用上了：同一份签名，正确公钥过、别的公钥不过。
    ///
    /// ⚠️ 这条一开始只断言「换公钥后验不过」——那是**弱守卫**：若实现忽略传入
    /// 公钥、始终用内嵌常量那把，验签照样失败、`is_err()` 照样成立、测试照样绿，
    /// 而「公钥参数被忽略」这个安全缺陷完全测不出（实测：把实现改成忽略参数后，
    /// 本条不红，是另外两条 happy-path 测试红的）。
    /// 判据：**要证明「用上了这个参数」，必须同时证明「用它时成功」**。
    #[test]
    fn end_to_end_uses_the_passed_public_key() {
        use base64::engine::general_purpose::STANDARD as B64;
        use base64::Engine as _;
        use ed25519_dalek::SigningKey;
        let (pub_b64, content, sig) = signed_manifest("9.9.9", &[platform_key()]);
        let other_pub = B64.encode(
            SigningKey::from_bytes(&[8u8; 32])
                .verifying_key()
                .as_bytes(),
        );
        assert_ne!(other_pub, pub_b64, "两把公钥必须真的不同");
        assert!(
            crate::signing::verify_detached_with(&pub_b64, &content, &sig).is_ok(),
            "用签名者对应的公钥必须通过——否则说明传入的公钥被忽略了"
        );
        assert!(
            crate::signing::verify_detached_with(&other_pub, &content, &sig).is_err(),
            "换一把公钥后必须验不过"
        );
    }

    /// 清单内容改一个字节（模拟传输中被改）→ 验不过。
    /// 钉住「验的是原始字节」：若实现改成验解析后的结构，这���会绿。
    #[test]
    fn end_to_end_rejects_a_single_flipped_byte_in_the_manifest() {
        let (pub_b64, content, sig) = signed_manifest("9.9.9", &[platform_key()]);
        let mut tampered = content.clone();
        let n = tampered.len();
        tampered[n - 2] ^= 0x01;
        assert!(
            crate::signing::verify_detached_with(&pub_b64, &tampered, &sig).is_err(),
            "改一个字节即须验不过"
        );
    }

    /// 清单里没有本平台条目时，整条链的终点是 Unreachable（可展示的原因），
    /// 而不是 UpToDate。这是发布事故最常见的一种，必须端到端成立。
    #[test]
    fn end_to_end_windows_only_manifest_ends_unreachable_not_up_to_date() {
        let (pub_b64, content, sig) = signed_manifest("9.9.9", &["windows-x86_64"]);
        crate::signing::verify_detached_with(&pub_b64, &content, &sig).unwrap();
        let m = parse_manifest(&content).unwrap();
        match judge(&m, "0.7.2") {
            ManifestVerdict::Unreachable(reason) => {
                assert!(reason.contains(platform_key()));
            }
            other => panic!(
                "Windows-only 清单在 macOS 上必须判为拿不到，实际: {other:?}"
            ),
        }
    }

    /// 造一份「版本比当前新、且带本平台条目」的可升级清单。
    fn upgradable_manifest(version: &str) -> UpdateManifest {
        UpdateManifest {
            schema_version: 1,
            version: version.to_string(),
            platforms: {
                let mut p = std::collections::HashMap::new();
                p.insert(
                    platform_key().to_string(),
                    PlatformEntry {
                        url: "https://dist/m-hub.dmg".to_string(),
                        sha256: "abc".to_string(),
                        size: 1,
                        ..PlatformEntry::default()
                    },
                );
                p
            },
            ..UpdateManifest::default()
        }
    }

    /// 真的没有新版 —— 唯一该显示「已是最新版本」的情形。
    #[test]
    fn judge_reports_up_to_date_only_when_there_is_genuinely_no_newer_version() {
        let m = upgradable_manifest("0.4.0");
        assert!(matches!(judge(&m, "0.4.0"), ManifestVerdict::UpToDate));
        assert!(matches!(judge(&m, "0.5.0"), ManifestVerdict::UpToDate));
        // 空版本 = 源不可用，不得被判成「有新版但拿不到」
        let mut empty = upgradable_manifest("");
        empty.version = String::new();
        assert!(matches!(judge(&empty, "0.3.0"), ManifestVerdict::UpToDate));
    }

    #[test]
    fn judge_reports_upgradeable_with_the_entry() {
        let m = upgradable_manifest("0.4.0");
        match judge(&m, "0.3.0") {
            ManifestVerdict::Upgradeable { entry, portable } => {
                assert_eq!(entry.url, "https://dist/m-hub.dmg");
                let _ = portable;
            }
            other => panic!("应判为可升级，实际: {other:?}"),
        }
    }

    /// 跳级保护拦下时**不是**「已是最新」：确有新版，只是这台机器升不了。
    /// 这条在原来那个 bool 里和「已是最新」同为 false，界面因此显示「已是最新版本」。
    #[test]
    fn judge_separates_jump_protection_from_up_to_date() {
        let mut m = upgradable_manifest("0.4.0");
        m.minimum_upgradable = "0.3.0".to_string();
        match judge(&m, "0.2.0") {
            ManifestVerdict::Unreachable(reason) => {
                assert!(
                    reason.contains("0.2.0") && reason.contains("0.3.0"),
                    "原因里应写清当前版本与下限，用户才知道该往哪走: {reason}"
                );
                // 判据不是「文案里不能出现『已是最新』」——那句话以否定形式出现
                // （「不是『已是最新』：确有新版…」）恰恰最能帮用户理解出了什么事。
                // 真正要钉住的是：**必须明确告诉用户存在新版**，否则与谎报无异。
                assert!(
                    reason.contains("确有新版"),
                    "原因必须点明确有新版（这正是与「已是最新」的分界）: {reason}"
                );
            }
            other => panic!("应判为拿不到而非已是最新，实际: {other:?}"),
        }
        // 恰好在下限上则放行
        assert!(matches!(judge(&m, "0.3.0"), ManifestVerdict::Upgradeable { .. }));
    }

    /// 清单无本平台条目时**不是**「已是最新」。移植后这条曾对每个 macOS 构建
    /// 无条件成立（平台键写死 windows-x86_64），界面一律显示「已是最新版本」——
    /// 自动更新通道看起来完全正常，永远不会有人去查。
    #[test]
    fn judge_separates_missing_platform_entry_from_up_to_date() {
        let mut m = upgradable_manifest("9.9.9");
        m.platforms = std::collections::HashMap::new();
        m.platforms.insert(
            "windows-x86_64".to_string(),
            PlatformEntry {
                url: "https://dist/m-hub.exe".to_string(),
                ..PlatformEntry::default()
            },
        );
        match judge(&m, "0.7.2") {
            ManifestVerdict::Unreachable(reason) => {
                assert!(reason.contains(platform_key()), "原因应写明本构建要哪个键: {reason}");
                assert!(
                    reason.contains("windows-x86_64"),
                    "原因应列出服务端实际发布了什么，否则无法自查: {reason}"
                );
            }
            other => panic!("应判为拿不到而非已是最新，实际: {other:?}"),
        }
    }

    /// 空 platforms 与「发了别的平台」要说不同的话——前者是源没配好，
    /// 后者是发错了平台的包，排查方向不同。
    #[test]
    fn no_platform_entry_reason_distinguishes_empty_from_wrong_platform() {
        let mut m = UpdateManifest::default();
        m.platforms = std::collections::HashMap::new();
        let empty = no_platform_entry_reason(&m);
        assert!(empty.contains("platforms 为空"), "空清单应有独立措辞: {empty}");

        m.platforms.insert("windows-x86_64".to_string(), PlatformEntry::default());
        let wrong = no_platform_entry_reason(&m);
        assert!(wrong.contains("windows-x86_64"), "应列出实际发布的键: {wrong}");
    }

    /// 报错文案必须可复现：platforms 是 HashMap，迭代序随机，
    /// 若不排序，同一份清单两次报错内容会不同（用户截图给作者时对不上）。
    /// 报错文案必须只取决于清单内容，不取决于 HashMap 的迭代序。
    ///
    /// ⚠️ 这条一开始写成「同一份清单反复调用 20 次比对」——那是**假守卫**：
    /// `HashMap` 的迭代序在**同一个进程内是稳定的**，去掉排序后照样全绿
    /// （约定 73 的典型形态：写完就绿不代表它是守卫）。
    /// 真正能造出来的失败是：两个 `HashMap` 走不同的 `RandomState` 种子，
    /// 迭代序通常不同 —— 内容相同的清单因插入顺序不同而报错不同，才是真的漂了。
    #[test]
    fn no_platform_entry_reason_ignores_insertion_order() {
        let keys = ["windows-x86_64", "macos-x86_64", "linux-aarch64", "linux-x86_64"];
        let build = |order: &[&str]| {
            let mut m = UpdateManifest::default();
            for k in order {
                m.platforms.insert(k.to_string(), PlatformEntry::default());
            }
            m
        };
        let forward = no_platform_entry_reason(&build(&keys));
        let mut reversed = keys;
        reversed.reverse();
        let backward = no_platform_entry_reason(&build(&reversed));
        assert_eq!(
            forward, backward,
            "同一份清单（仅插入顺序不同）应给出同一句报错，否则用户截图给作者时对不上"
        );
        // 且必须是排好序的（可复现的稳定输出），不是碰巧一致
        assert!(
            forward.contains("[linux-aarch64, linux-x86_64, macos-x86_64, windows-x86_64]"),
            "键名应按字典序输出: {forward}"
        );
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn locate_exe_picks_largest() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("config.json"), "{}").unwrap();
        std::fs::write(dir.path().join("m-hub.exe"), vec![0u8; 100]).unwrap();
        std::fs::write(dir.path().join("helper.exe"), vec![0u8; 10]).unwrap();
        let found = locate_new_exe(dir.path()).unwrap();
        assert_eq!(found.file_name().unwrap().to_string_lossy(), "m-hub.exe");
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn locate_exe_finds_nested() {
        let dir = tempfile::tempdir().unwrap();
        let sub = dir.path().join("res");
        std::fs::create_dir_all(&sub).unwrap();
        std::fs::write(sub.join("m-hub.exe"), vec![0u8; 50]).unwrap();
        let found = locate_new_exe(dir.path()).unwrap();
        assert_eq!(found.file_name().unwrap().to_string_lossy(), "m-hub.exe");
    }

    /// macOS 的对应件：更新包里认的是 `.app` 包，且要取**体积最大**的那个
    /// （分发包里常同时躺着源码/文档目录，误取会让自替换装上一个空壳）。
    #[cfg(target_os = "macos")]
    #[test]
    fn locate_app_picks_largest_bundle() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("README.txt"), b"whatever").unwrap();
        let small = dir.path().join("helper.app");
        let big = dir.path().join("m-hub.app");
        for p in [&small, &big] {
            std::fs::create_dir_all(p.join("Contents/MacOS")).unwrap();
        }
        std::fs::write(small.join("Contents/MacOS/helper"), vec![0u8; 10]).unwrap();
        std::fs::write(big.join("Contents/MacOS/m-hub"), vec![0u8; 100]).unwrap();

        let found = locate_new_app(dir.path()).unwrap();
        assert_eq!(found.file_name().unwrap().to_string_lossy(), "m-hub.app");
    }

    /// 非 `.app` 的目录绝不能被当成更新产物——否则会拿一个普通文件夹去替换应用包
    #[cfg(target_os = "macos")]
    #[test]
    fn locate_app_ignores_plain_directories() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("docs")).unwrap();
        std::fs::write(dir.path().join("notes.txt"), b"x").unwrap();
        assert!(locate_new_app(dir.path()).is_none());
    }

    /// `replace_file` 语义：文件 → 备份 → 新文件落地 → 备份清理。
    /// macOS 上换成目录版（`.app` → `.app.old`），两者共享 `ReplaceTarget::commit`。
    #[cfg(target_os = "macos")]
    #[test]
    fn replace_target_commits_and_cleans_backup() {
        let dir = tempfile::tempdir().unwrap();
        let current = dir.path().join("m-hub.app");
        let incoming = dir.path().join("incoming/m-hub.app");
        std::fs::create_dir_all(current.join("Contents/MacOS")).unwrap();
        std::fs::create_dir_all(incoming.join("Contents/MacOS")).unwrap();
        std::fs::write(current.join("Contents/MacOS/m-hub"), b"old").unwrap();
        std::fs::write(incoming.join("Contents/MacOS/m-hub"), b"new").unwrap();

        let target = ReplaceTarget {
            current: current.clone(),
            backup: current.with_extension("app.old"),
            incoming,
        };
        assert!(target.commit(), "替换应成功");
        assert_eq!(
            std::fs::read_to_string(current.join("Contents/MacOS/m-hub")).unwrap(),
            "new"
        );
        assert!(!current.with_extension("app.old").exists(), "备份应被清理");
    }

    /// 替换失败必须**回滚到旧版本**，不能留下「半个应用」——
    /// macOS 上这尤其致命：包签名一旦不完整，整个 app 直接无法启动。
    #[cfg(target_os = "macos")]
    #[test]
    fn replace_target_rolls_back_when_incoming_missing() {
        let dir = tempfile::tempdir().unwrap();
        let current = dir.path().join("m-hub.app");
        std::fs::create_dir_all(current.join("Contents/MacOS")).unwrap();
        std::fs::write(current.join("Contents/MacOS/m-hub"), b"old").unwrap();

        let target = ReplaceTarget {
            current: current.clone(),
            backup: current.with_extension("app.old"),
            incoming: dir.path().join("does-not-exist.app"),
        };
        assert!(!target.commit(), "产物缺失时不应成功");
        assert!(current.join("Contents/MacOS/m-hub").exists(), "旧版本应已回滚就位");
        assert_eq!(
            std::fs::read_to_string(current.join("Contents/MacOS/m-hub")).unwrap(),
            "old"
        );
    }
}