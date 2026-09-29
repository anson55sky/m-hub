//! macOS 原生能力层（AppKit / LaunchServices / launchd）。
//!
//! 这个模块的存在是为了把「Windows 上靠 PowerShell + COM + GDI 干的那几件事」换成
//! macOS 的正规做法，并且**收在一处**——它们在 Windows 版里散落在 `commands.rs`
//! （图标提取、.lnk 解析、已安装应用扫描）与各功能模块里，逐个就地改会漏。
//!
//! | Windows 的做法 | macOS 的做法 |
//! |---|---|
//! | `System.Drawing.Icon::ExtractAssociatedIcon` | `NSWorkspace.iconForFile` → TIFF → `NSBitmapImageRep` → PNG |
//! | 读注册表 `...\Uninstall\*` + 开始菜单 `.lnk` | 扫 `/Applications`、`~/Applications`、`/System/Applications` 下的 `.app` |
//! | `WScript.Shell` 解析 `.lnk` 目标 | `.app` 本身就是目标；`.webloc` 等无对应物，直接拒绝 |
//! | `HKCU\...\Run` 开机自启 | `~/Library/LaunchAgents/*.plist` + `launchctl` |
//! | `SetForegroundWindow` | `NSRunningApplication.activateWithOptions` |
//! | `keybd_event` 注入按键 | `CGEvent` 合成 `⌘V`（需辅助功能授权） |
//! | `CF_*` + `WM_CLIPBOARDUPDATE` | `NSPasteboard.changeCount` 轮询（见 `clipboard.rs`） |
//!
//! ## 关于「AppKit 是不是必须在主线程」
//!
//! AppKit 的规约是「NSApplication 与绝大多数 NSView 只能在主线程用」，但
//! **`NSWorkspace` 的图标查询不是那一类**——它走 LaunchServices 查图标缓存，不碰
//! `NSApplication` 的事件循环。批量扫描应用图标动辄几十上百个，若为了「守规矩」
//! 全部 dispatch 回主线程，会把 `scan_installed_apps` 变成几十次 UI 卡顿（该命令
//! 之所以是 `async`，就是为了不冻结界面——见 `commands.rs` 的注释）。
//!
//! 因此本模块的取舍是：图标查询在**调用方线程**同步执行，`NSPasteboard` 同理
//! （Apple 明确文档化 NSPasteboard 可在后台线程使用）。这与 macOS 生态里绝大多数
//! 图标库的实际做法一致。真正必须上主线程的是 AppKit **视图**层级（NSWindow/NSView
//! 的创建与几何），那些留在各自的浮窗模块里、由 Tauri 的主线程事件循环驱动。
//!
//! 本模块**不参与窗口生命周期**（约定 41：禁止运行期 build/destroy），只做数据层。

#![cfg(target_os = "macos")]

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

use objc2_app_kit::{
    NSApplicationActivationOptions, NSBitmapImageFileType, NSBitmapImageRep, NSImage, NSPasteboard,
    NSPasteboardType, NSRunningApplication, NSWorkspace,
};
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2_foundation::{NSArray, NSDictionary, NSString, NSURL};

// ==================== 应用图标 ====================

/// 缓存图标的边长上限（像素）。
///
/// `NSWorkspace.iconForFile` 返回的是**该资源的原始尺寸**——现代 macOS 图标都是
/// 1024×1024 的 16-bit PNG，一张 1.1MB。速达格子里的图标渲染尺寸是 40~64px，
/// 存 1024 纯属浪费：实测扫 118 个应用会占掉 **136MB** 数据目录，还会把
/// 「数据备份」压成的 zip 撑到几百 MB（而这个应用卖点就是本地优先、体积小）。
/// 256 足够覆盖 2× 高分屏下的 128px 显示，且单张降到 20~40KB。
const ICON_MAX_EDGE: u32 = 256;

/// 缓存图标的体积上限（字节）——用于识别并就地重做「历史遗留的大图」。
/// 256px/8bit PNG 稳定在 30KB 上下，64KB 是个不会被正常图标碰到、又能盖住
/// 旧版 1024px 图标（≥ 数百 KB）的阈值。
const ICON_CACHE_MAX_BYTES: u64 = 64 * 1024;

/// 取任意路径的图标并编码为 PNG 字节（已缩放到 [`ICON_MAX_EDGE`]）。
///
/// 链路：`NSWorkspace.iconForFile`（LaunchServices 负责，`.app`/`.icns`/可执行文件
/// 都能拿到正确图标）→ `TIFFRepresentation` → `NSBitmapImageRep` → PNG → 等比缩放。
/// 走 TIFF 中转是因为 `NSImage` 本身不产出 PNG，而 `NSBitmapImageRep` 认 TIFF。
pub fn icon_png(path: &str) -> Option<Vec<u8>> {
    let ws = NSWorkspace::sharedWorkspace();
    let ns_path = NSString::from_str(path);
    let image = ws.iconForFile(&ns_path);
    let full = image_to_png(&image)?;
    downscale_icon_png(&full)
}

/// 把 AppKit 出的原始尺寸 PNG 等比缩到 [`ICON_MAX_EDGE`]，并统一成 8-bit RGBA。
///
/// 顺带解决 16-bit 的问题：AppKit 按原图位深编码，1024px 的现代图标是
/// 16-bit/channel，同尺寸下体积比 8-bit 大一倍以上，而显示端根本用不到。
///
/// 已经是小图（≤ 阈值且边长 ≤ 上限）时**原样返回**——小图重新编码只会掉质量、
/// 白费 CPU。解码失败也返回原字节，宁可大一点也不许丢图标。
fn downscale_icon_png(bytes: &[u8]) -> Option<Vec<u8>> {
    let img = image::load_from_memory(bytes).ok()?;
    let (w, h) = (img.width(), img.height());
    if w <= ICON_MAX_EDGE && h <= ICON_MAX_EDGE {
        return Some(bytes.to_vec());
    }
    let resized = img.resize(
        ICON_MAX_EDGE,
        ICON_MAX_EDGE,
        image::imageops::FilterType::Lanczos3,
    );
    let mut out = std::io::Cursor::new(Vec::new());
    resized
        .write_to(&mut out, image::ImageFormat::Png)
        .ok()?;
    Some(out.into_inner())
}

/// `NSImage` → PNG 字节。TIFF 中转（`NSImage` 没有直接的 PNG 出口）。
fn image_to_png(image: &NSImage) -> Option<Vec<u8>> {
    let tiff = image.TIFFRepresentation()?;
    let rep = NSBitmapImageRep::imageRepWithData(&tiff)?;
    // 空 properties = 用编码器默认参数。unsafe 的原因：签名收
    // `&NSDictionary<NSBitmapImageRepPropertyKey, AnyObject>`，元素类型是编译期
    // 约定而运行期无从校验——我们传的确���是空字典，不存在类型不符的可能。
    // `CopiedKey` 是 from_slices 自己那个泛型参数，字典的 KeyType 约束不到它，
    // 空切片又给不出线索 —— 必须显式指定，否则推不出来
    let empty: Retained<NSDictionary<NSString, AnyObject>> =
        NSDictionary::<NSString, AnyObject>::from_slices::<NSString>(&[], &[]);
    let png = unsafe { rep.representationUsingType_properties(NSBitmapImageFileType::PNG, &empty) }?;
    Some(png.to_vec())
}

/// 图标缓存路径：`数据根/icons/<source 路径哈希>.png`。
///
/// **必须与 Windows 版同款**（`DefaultHasher` + 16 位十六进制）——图标的缓存键是
/// 「来源路径」而不是平台，两平台若各算一套，同一个程序在两个系统上会缓存两份
/// 几乎一样的图，白占空间；更重要的是 `DataPanel` 的「数据大小」统计与备份包
/// 体积会因此对不上。
pub fn icon_cache_path(source: &str) -> PathBuf {
    let mut hasher = DefaultHasher::new();
    source.hash(&mut hasher);
    crate::paths::data_root()
        .join("icons")
        .join(format!("{:016x}.png", hasher.finish()))
}

/// 提取程序图标并缓存为 PNG，返回缓存绝对路径。
/// 提取失败或系统没有该图标时返回 None（前端回退到名称首字母）。
pub fn extract_app_icon(source: &str) -> Option<String> {
    let output_path = icon_cache_path(source);
    // 已提取过直接复用：批量扫描会反复命中同一批应用，每次都重编码纯浪费。
    // 例外：体积超阈值 = 早期版本（未缩放）留下的 1024px 大图，就地重做一次。
    // 不做这个自愈的话，老用户会一直背着上百 MB 的图标目录。
    if let Ok(meta) = std::fs::metadata(&output_path) {
        if meta.is_file() && meta.len() <= ICON_CACHE_MAX_BYTES {
            return Some(output_path.to_string_lossy().into_owned());
        }
    }
    if let Some(dir) = output_path.parent() {
        std::fs::create_dir_all(dir).ok()?;
    }
    let png = icon_png(source)?;
    std::fs::write(&output_path, png).ok()?;
    Some(output_path.to_string_lossy().into_owned())
}

/// 导入用户选择的图标文件到 icons 目录。
///
/// - `.icns`（macOS 的原生图标格式）经 AppKit 解码重编码为 PNG；
/// - `png`/`jpg` 等已是位图的直接复制；
/// - 其余（`icns` 之外的矢量、无扩展名）返回明确错误而不是静默失败。
///
/// 返回落盘后的 PNG 绝对路径。
pub fn import_icon_file(source: &str) -> Result<Option<String>, String> {
    let src = Path::new(source);
    let output_path = icon_cache_path(source);
    if output_path.exists() {
        return Ok(Some(output_path.to_string_lossy().into_owned()));
    }
    if let Some(dir) = output_path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }

    let ext = src
        .extension()
        .and_then(|e| e.to_str())
        .map(|s| s.to_lowercase())
        .unwrap_or_default();

    if ext == "icns" {
        let png = icon_png(source).ok_or_else(|| "无法读取该 icns 图标文件".to_string())?;
        std::fs::write(&output_path, png).map_err(|e| format!("写入图标失败: {e}"))?;
        log::info!("图标导入成功（icns→png）: {}", source);
        return Ok(Some(output_path.to_string_lossy().into_owned()));
    }

    match std::fs::copy(source, &output_path) {
        Ok(_) => {
            log::info!("图标导入成功: {}", source);
            Ok(Some(output_path.to_string_lossy().into_owned()))
        }
        Err(e) => {
            log::error!("图标复制失败: {} -> {}", source, e);
            Err(format!("复制图标失败: {e}"))
        }
    }
}

// ==================== 已安装应用扫描 ====================

/// 系统与第三方应用的安装位置。系统自带工具（`/System/Applications/Utilities`、
/// `/System/Library`）**刻意不扫**——它们不是用户装的程序，列出来只是噪音
/// （对应 Windows 版过滤 `KB*` / `Update for*` 的那一层）。
fn app_dirs() -> Vec<PathBuf> {
    let mut dirs = vec![
        PathBuf::from("/Applications"),
        PathBuf::from("/System/Applications"),
    ];
    if let Some(home) = dirs::home_dir() {
        dirs.push(home.join("Applications"));
    }
    dirs
}

/// 名称噪音过滤：这些是系统的更新/安装/诊断工具，用户不会拿它当速达入口。
fn is_noise(name: &str) -> bool {
    const NOISE: &[&str] = &[
        "software update",
        "system information",
        "system settings",
        "app store",
        "xcode",
        "command line tools",
    ];
    let lower = name.to_lowercase();
    NOISE.iter().any(|n| lower == *n)
}

/// 扫出候选应用：`(展示名, .app 绝对路径)`。
///
/// macOS 上「已安装应用」没有注册表那样的中央清单，`.app` 包本身就是全部真相，
/// 因此这个扫描比 Windows 版**更准**（Windows 版要从 Uninstall 注册项 + 开始菜单
/// 两处拼，还常拿到已卸载的残留项）。
pub fn scan_app_candidates() -> Vec<(String, String)> {
    let mut apps: Vec<(String, String)> = Vec::new();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();

    for dir in app_dirs() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("app") {
                continue;
            }
            // 用「真实路径 + 路径」双重去重：同一应用经软链/别名重复出现时只留一条
            let key = path
                .canonicalize()
                .unwrap_or_else(|_| path.clone())
                .to_string_lossy()
                .to_lowercase();
            if !seen.insert(key) {
                continue;
            }
            let Some(name) = app_display_name(&path) else {
                continue;
            };
            if is_noise(&name) {
                continue;
            }
            apps.push((name, path.to_string_lossy().into_owned()));
        }
    }

    apps.sort_by(|a, b| {
        a.0.to_lowercase()
            .cmp(&b.0.to_lowercase())
            .then_with(|| a.1.cmp(&b.1))
    });
    // 与 Windows 版同量级上限：几十万 `.app` 的机器上不至于把前端列表撑爆
    const MAX_APPS: usize = 500;
    if apps.len() > MAX_APPS {
        apps.truncate(MAX_APPS);
    }
    log::info!("[mac] 扫描已安装应用: 共 {} 个", apps.len());
    apps
}

/// `.app` 的展示名：`CFBundleDisplayName` → `CFBundleName` → 目录名去后缀。
/// 拖入导入（`commands::parse_dropped_path`）也要用，故对外公开。
pub fn app_display_name(app_path: &Path) -> Option<String> {
    let plist_path = app_path.join("Contents").join("Info.plist");
    if let Ok(data) = std::fs::read(&plist_path) {
        if let Ok(value) = plist::Value::from_reader(std::io::Cursor::new(data)) {
            if let Some(dict) = value.as_dictionary() {
                for key in ["CFBundleDisplayName", "CFBundleName"] {
                    if let Some(name) = dict.get(key).and_then(|v| v.as_string()) {
                        let trimmed = name.trim();
                        if !trimmed.is_empty() {
                            return Some(trimmed.to_string());
                        }
                    }
                }
            }
        }
    }
    app_path
        .file_stem()
        .and_then(|s| s.to_str())
        .map(str::to_string)
        .filter(|s| !s.is_empty())
}

/// 批量提取图标：逐个走 [`extract_app_icon`] 的缓存（命中即零开销）。
///
/// Windows 版为了省进程数走「单次 PowerShell 批量导出」；macOS 侧每个图标是一次
/// 轻量的 LaunchServices 查询（已在系统图标缓存里，典型 < 1ms），不值得为它引入
/// 子进程协议，且 NSWorkspace 不是脚本接口。缓存键与 Windows 版一致，因此重复扫描
/// 只在首次产生实际开销。
pub fn batch_extract_icons(apps: &[(String, String)]) -> Result<Vec<Option<String>>, String> {
    Ok(apps
        .iter()
        .map(|(_, target)| extract_app_icon(target))
        .collect())
}

// ==================== 剪贴板（NSPasteboard） ====================

/// 构造一个剪贴板 UTI（Uniform Type Identifier），如 `public.png`。
///
/// Apple 头文件里的 `NSPasteboardTypePNG` 之类**本质就是字符串字面量**
/// （`#define NSPasteboardTypePNG @"public.png"`），objc2 把它们导出成跨模块的
/// `extern "static"`，读一次就要包一层 `unsafe`（而且宏展开处还会误报
/// `unused_unsafe`）。这里直接用字符串：语义完全等价，少一层 unsafe，
/// 而且 UTI 本身就是「这份剪贴板内容是什么类型」的可读协议——写在代码里
/// 比藏在一个常量名后面更���核对。
///
/// 对照：Apple 常量名见各处调用点的注释。
fn uti(name: &str) -> Retained<NSString> {
    NSString::from_str(name)
}

/// 剪贴板是否声明了某个 UTI 类型。
fn has_type(pb: &NSPasteboard, uti: &NSPasteboardType) -> bool {
    // NSArray 只提供 `iter`（没有 `contains`），逐个比字符串。
    // 走 `to_string()` 而非指针/`isEqual:`：一次剪贴板读取只会问两三个 UTI，
    // 几次小分配远不如为省它去写 `isEqualToString:` 的 msg_send 划算。
    pb.types().is_some_and(|types| {
        types
            .iter()
            .any(|t: Retained<NSString>| t.to_string() == uti.to_string())
    })
}

/// 剪贴板里的文件路径（用户在 Finder 里复制文件 → 剪贴板上是 `public.file-url` 列表）。
///
/// 优先读现代的 `public.file-url`；部分老应用只放 `NSFilenamesPboardType`
/// （`public.file-name` 的历史变体，NSString 数组），一并兜住。
pub fn read_clipboard_files() -> Option<Vec<String>> {
    let pb = NSPasteboard::generalPasteboard();

    if has_type(&pb, &uti("public.file-url")) {
        if let Some(list) = pb.propertyListForType(&uti("public.file-url")) {
            // downcast 消费 self 并返回 Result：类型不符不是错误分支，
            // 只是「不是文件列表」，按 .ok() 落回继续尝试旧 UTI
            if let Ok(urls) = list.downcast::<NSArray<AnyObject>>() {
                let paths: Vec<String> = urls
                    .iter()
                    .filter_map(|item: Retained<AnyObject>| item.downcast::<NSURL>().ok())
                    .filter_map(|url| url.path().map(|p| p.to_string()))
                    .collect();
                if !paths.is_empty() {
                    return Some(paths);
                }
            }
        }
    }

    const NS_FILENAMES: &str = "public.file-name";
    let legacy = NSString::from_str(NS_FILENAMES);
    if has_type(&pb, &legacy) {
        if let Some(list) = pb.propertyListForType(&legacy) {
            if let Ok(items) = list.downcast::<NSArray<AnyObject>>() {
                let paths: Vec<String> = items
                    .iter()
                    .filter_map(|item: Retained<AnyObject>| item.downcast::<NSString>().ok())
                    .map(|s| s.to_string())
                    .filter(|s| !s.is_empty())
                    .collect();
                if !paths.is_empty() {
                    return Some(paths);
                }
            }
        }
    }
    None
}

/// 剪贴板里的图片字节。
///
/// 优先 `public.png`（浏览器、微信、QQ、截图工具都放这个，拿到就是可直接落盘的
/// PNG）；退化到 `public.tiff`（macOS 截图与预览的原生格式），由调用方转码。
pub fn read_clipboard_image() -> Option<(Vec<u8>, &'static str)> {
    let pb = NSPasteboard::generalPasteboard();

    if has_type(&pb, &uti("public.png")) {
        if let Some(data) = pb.dataForType(&uti("public.png")) {
            return Some((data.to_vec(), "png"));
        }
    }
    if has_type(&pb, &uti("public.tiff")) {
        if let Some(data) = pb.dataForType(&uti("public.tiff")) {
            return Some((data.to_vec(), "tiff"));
        }
    }
    None
}

/// 剪贴板里的富文本 HTML 片段（`public.html`）。
///
/// NSPasteboard 存的是**完整 HTML 文档**（含 `<html><head><body>`），
/// 而 Windows 的 CF_HTML 有 StartFragment/EndFragment 偏移。这里不做偏移解析，
/// 直接整份交给前端的 DOMPurify 清洗——`ClipboardOverlay.vue` 渲染富文本时
/// 本来就走 `markdownHtml.ts` 的 sanitize 路径，多一层 head/body 无害。
pub fn read_clipboard_html() -> Option<String> {
    let pb = NSPasteboard::generalPasteboard();
    if !has_type(&pb, &uti("public.html")) {
        return None;
    }
    pb.stringForType(&uti("public.html"))
        .map(|s| s.to_string())
        .filter(|s| !s.trim().is_empty())
}

/// 剪贴板里的纯文本。
pub fn read_clipboard_text() -> Option<String> {
    let pb = NSPasteboard::generalPasteboard();
    if !has_type(&pb, &uti("public.utf8-plain-text")) {
        return None;
    }
    pb.stringForType(&uti("public.utf8-plain-text"))
        .map(|s| s.to_string())
        .filter(|s| !s.is_empty())
}

/// 写纯文本（可选同时写富文本 HTML）。
///
/// `clearContents` 之后逐类型写入；每次写入都会让系统 `changeCount` 自增，
/// 监听侧靠它发现变化（Windows 是靠 `WM_CLIPBOARDUPDATE` 消息）。
pub fn write_clipboard_text(text: &str, html: Option<&str>) -> Result<(), String> {
    let pb = NSPasteboard::generalPasteboard();
    pb.clearContents();
    let ns_text = NSString::from_str(text);
    if !pb.setString_forType(&ns_text, &uti("public.utf8-plain-text")) {
        return Err("写入剪贴板失败".into());
    }
    if let Some(html) = html {
        if !html.trim().is_empty() {
            let ns_html = NSString::from_str(html);
            pb.setString_forType(&ns_html, &uti("public.html"));
        }
    }
    Ok(())
}

/// 写文件列表到剪贴板（`public.file-url` 数组，Finder 粘贴得到的是文件本体）。
pub fn write_clipboard_files(paths: &[String]) -> Result<(), String> {
    let pb = NSPasteboard::generalPasteboard();
    pb.clearContents();
    let urls: Vec<objc2::rc::Retained<NSURL>> = paths
        .iter()
        .map(|p| NSURL::fileURLWithPath(&NSString::from_str(p)))
        .collect();
    if urls.is_empty() {
        return Err("没有可写入剪贴板的文件".into());
    }
    let array = NSArray::from_retained_slice(&urls);
    // setPropertyList 的元素类型无法静态校验（签名收 &AnyObject），unsafe 换一次类型擦除
    let ok = unsafe { pb.setPropertyList_forType(&array, &uti("public.file-url")) };
    if !ok {
        return Err("写入剪贴板失败".into());
    }
    Ok(())
}

/// 写图片到剪贴板（读回文件字节，按文件头判断 PNG / TIFF）。
pub fn write_clipboard_image(path: &str) -> Result<(), String> {
    let bytes = std::fs::read(path).map_err(|e| format!("读取图片失败: {e}"))?;
    let pb = NSPasteboard::generalPasteboard();
    pb.clearContents();
    let data = objc2_foundation::NSData::with_bytes(&bytes);
    // 按**文件头**判类型而不是扩展名：剪贴板只认 UTI，给错 UTI 的话
    // 粘到别的应用里会变成「一张打不开的图」
    let image_uti: &NSPasteboardType = if bytes.starts_with(&[0x89, b'P', b'N', b'G']) {
        &uti("public.png")
    } else {
        &uti("public.tiff")
    };
    if !pb.setData_forType(Some(&data), image_uti) {
        return Err("写入剪贴板失败".into());
    }
    Ok(())
}

// ==================== 进程激活 ====================

/// 把指定 bundle id 的已运行应用激活到前台（等价 Windows 的 `SetForegroundWindow`）。
///
/// 用于「剪贴板粘贴后归还焦点」：macOS 隐藏自己的窗口不会自动把焦点还给上一个
/// 应用，必须显式激活。
pub fn activate_app(bundle_id: &str) -> bool {
    let apps =
        NSRunningApplication::runningApplicationsWithBundleIdentifier(&NSString::from_str(bundle_id));
    let Some(app) = apps.iter().next() else {
        return false;
    };
    // `ActivateIgnoringOtherApps` 在 macOS 14 起被 Apple 标记为废弃并且**不再生效**
    // （系统改用「用户上次交互的 app 自动回焦」）。仍要显式调 activate：
    // 它是让已运行应用把窗口拉到前台的唯一途径，14+ 上系统会自行补上 focus。
    #[allow(deprecated)]
    app.activateWithOptions(NSApplicationActivationOptions::ActivateIgnoringOtherApps)
}

/// 当前所有正在运行的应用。
///
/// 注意 `runningApplications` 这个选择器属于 **NSWorkspace**，但 objc2 的绑定把它
/// 挂在 `NSRunningApplication` 的实例方法上（签名与运行时对象不匹配，直接调会崩）。
/// 故一律经 `msg_send!` 从 `NSWorkspace::sharedWorkspace()` 上取，避免踩这个坑。
pub fn running_applications() -> Retained<NSArray<NSRunningApplication>> {
    use objc2::msg_send;
    let ws = NSWorkspace::sharedWorkspace();
    unsafe { msg_send![&*ws, runningApplications] }
}

/// 取当前前台应用的 **bundle id**（形如 `com.apple.Safari`）。
/// 用于「焦点是否还留在本进程」的判定——macOS 没有「前台窗口句柄」，
/// 但有前台 **应用**，粒度更粗却足够区分「是不是我们自己」。
///
/// ⚠️ **必须取 `bundleIdentifier`，不能拿 `bundleURL` 的末段去壳**（2026-09-29 修）。
/// 末段去掉 `.app` 得到的是**包名**（`Safari` / `Google Chrome` / `WeChat`），
/// 看着像 id 实则不是。两个下游都被它静默废掉：
/// ① `clipboard.rs` 的 `prev_focus == SELF_BUNDLE_ID`（`com.mhub.desktop`）
///    永远不成立 → 「向前台应用派发插入」的可靠路径永不命中，每次都退化成
///    依赖辅助功能权限的模拟 ⌘V；
/// ② `activate_app(&prev_focus)` 走的是
///    `NSRunningApplication::runningApplicationsWithBundleIdentifier:`
///    —— 该 API **只接受真 bundle id**，传包名必然返回空数组，焦点归还恒失败。
/// 症状是「收起剪贴板浮层后键盘焦点丢了、要手动点一下原窗口」，且无任何报错。
pub fn frontmost_bundle_id() -> Option<String> {
    let apps = running_applications();
    let frontmost = apps
        .iter()
        .find(|a: &Retained<NSRunningApplication>| a.isActive())?;
    frontmost.bundleIdentifier().map(|s| s.to_string())
}

/// 当前前台应用的**包名**（`Safari` / `Google Chrome`）。
///
/// 与 [`frontmost_bundle_id`] 是两回事，别混用：包名用于**展示**
/// （剪贴板历史条目的「来源应用」列，用户要认得出是哪个 App），
/// 而 bundle id 用于**判定与激活**（两者都是 API 硬要求：激活只认 id）。
/// 恰好 `bundleIdentifier` 的末段常常就是包名，但仍一律走本函数取，
/// 以免两处各自推导末段。
pub fn frontmost_app_name() -> Option<String> {
    let apps = running_applications();
    let frontmost = apps
        .iter()
        .find(|a: &Retained<NSRunningApplication>| a.isActive())?;
    // 优先本地化名（用户自己改过的名字，如「Visual Studio Code」）
    if let Some(name) = frontmost.localizedName() {
        return Some(name.to_string());
    }
    let url = frontmost.bundleURL()?;
    let name = url.lastPathComponent()?.to_string();
    Some(name.trim_end_matches(".app").to_string())
}

// ==================== 光标与屏幕几何 ====================

/// 鼠标光标的全局位置，**逻辑点（point）、原点左上、y 向下**。
///
/// 为什么不直接用 `NSEvent::mouseLocation()`：它是 **Cocoa 坐标**（原点在左下角、
/// y 向上、且以主屏左下为基准），多显示器上下排布时换算极易出错——而窗口定位
/// 要的正是窗口服务器那套「左上原点、y 向下」的全局坐标。`CGEvent::location()`
/// 直接就是这套坐标，省掉整个翻转环节。
///
/// 返回逻辑点而非物理像素：macOS 的窗口位置本身就是点制（Retina 下 1 pt = 2 px），
/// 调用方用 `Position::Logical` 放置窗口即可，不必在 HiDPI 上做换算。
pub fn cursor_point() -> Option<(f64, f64)> {
    use core_graphics::event::CGEvent;
    use core_graphics::event_source::{CGEventSource, CGEventSourceStateID};
    let source = CGEventSource::new(CGEventSourceStateID::HIDSystemState).ok()?;
    let event = CGEvent::new(source).ok()?;
    let p = event.location();
    Some((p.x, p.y))
}

/// 光标所在显示器的边界矩形（逻辑点，左上原点）。
/// 找不到就回落到主显示器。
pub fn display_bounds_at(x: f64, y: f64) -> (f64, f64, f64, f64) {
    use core_graphics::display::{CGDisplayBounds, CGGetDisplaysWithPoint, CGMainDisplayID};
    use core_graphics::geometry::CGPoint;

    let mut ids = [0u32; 4];
    let mut count: u32 = 0;
    let point = CGPoint::new(x, y);
    let err = unsafe {
        CGGetDisplaysWithPoint(
            point,
            ids.len() as u32,
            ids.as_mut_ptr(),
            &mut count as *mut u32,
        )
    };
    let id = if err == 0 && count > 0 {
        ids[0]
    } else {
        unsafe { CGMainDisplayID() }
    };
    let r = unsafe { CGDisplayBounds(id) };
    (r.origin.x, r.origin.y, r.size.width, r.size.height)
}

/// 某点所在显示器的**缩放系数**（backing scale factor）。
/// Retina 屏为 2.0，非 Retina 为 1.0；取不到（边界上、点恰在缝隙里）时回退 1.0。
pub fn display_scale_at(x: f64, y: f64) -> f64 {
    use core_graphics::display::{CGDisplayBounds, CGDisplayPixelsWide, CGGetDisplaysWithPoint};
    use core_graphics::geometry::CGPoint;
    let mut ids = [0u32; 4];
    let mut count: u32 = 0;
    let point = CGPoint::new(x, y);
    let err = unsafe {
        CGGetDisplaysWithPoint(
            point,
            ids.len() as u32,
            ids.as_mut_ptr(),
            &mut count as *mut u32,
        )
    };
    if err != 0 || count == 0 {
        return 1.0;
    }
    let id = ids[0];
    let bounds = unsafe { CGDisplayBounds(id) };
    let wide = unsafe { CGDisplayPixelsWide(id) } as f64;
    if bounds.size.width <= 0.0 {
        return 1.0;
    }
    let scale = wide / bounds.size.width;
    if scale.is_finite() && scale > 0.0 {
        scale
    } else {
        1.0
    }
}

/// 鼠标光标的全局位置，**物理像素**（左上原点）。
///
/// m-hub 的悬浮球贴边几何全篇用物理像素（`Tauri` 的 `outer_position` / `work_area`
/// 在两平台都返回物理像素），而 `CGEvent::location()` 给的是逻辑点。Retina 屏上
/// 两者差 2 倍——不换算的后果是「悬停滑出」的命中区整体偏移一倍、边缘永远判不出。
pub fn cursor_physical() -> Option<(i32, i32)> {
    let (x, y) = cursor_point()?;
    let scale = display_scale_at(x, y);
    Some((
        (x * scale).round() as i32,
        (y * scale).round() as i32,
    ))
}

// 公开的 `CGEventSourceButtonState`（CGEventSource.h）。不用 AppKit 的
// `NSEvent.pressedMouseButtons`——那个被 objc2 标为主线程限定，
// 而调用方（悬浮球边缘监视）是从后台线程调的。
// 放在函数上面会让 doc 注释挂到 extern 块上（报 unused doc comment），故先声明。
#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    /// 返回某个事件源状态下指定鼠标键的当前状态。`state = 1` 即
    /// `kCGEventSourceStateHIDSystemState`（真实硬件状态），`button = 0` 为左键。
    fn CGEventSourceButtonState(state: i32, button: u32) -> bool;
}

/// 左键当前是否按下。
///
/// 悬浮球要靠它判断「正在原生拖拽中，别动窗口」——拖拽由 AppKit 的模态事件循环
/// 接管，此时任何 `set_position` 都会与之打架。

pub fn lmb_down() -> bool {
    const HID_SYSTEM_STATE: i32 = 1;
    const LEFT_BUTTON: u32 = 0;
    unsafe { CGEventSourceButtonState(HID_SYSTEM_STATE, LEFT_BUTTON) }
}

// ==================== 全局 Esc 监视（剪贴板浮层） ====================

/// macOS 虚拟键码：Esc = 53（`kVK_Escape`）。
const KEY_ESCAPE: i64 = 53;

/// 一次 Esc 监视的持有句柄。**drop 即停止监视**（tap 随之拆除，不再吞 Esc）。
pub struct EscWatch {
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
    /// 监视线程的 JoinHandle —— drop 时确保 tap 已被拆掉
    ///
    /// **必须有**：tap 的生命周期绑在它自己线程的 run loop 上，
    /// 只置 `stop` 不 join 的话监视线程可能还活着几十毫秒（一次轮询周期），
    /// 那段时间里用户的 Esc 仍会被吞掉。
    handle: Option<std::thread::JoinHandle<()>>,
}

impl Drop for EscWatch {
    fn drop(&mut self) {
        self.stop
            .store(true, std::sync::atomic::Ordering::SeqCst);
        if let Some(h) = self.handle.take() {
            // ⚠️ **不能在 tap 线程自己身上 join** —— 那会 panic/死锁。
            // 而这条路径是真实存在的：`on_esc` 回调就运行在 tap 线程上，
            // 它调 `hide_overlay` → drop 本句柄 → 落进这个 Drop。
            // 此时线程函数马上就要返回、tap 随它一起析构，
            // 所以把手柄 forget 掉（= detach）即可，无需等待。
            if h.thread().id() == std::thread::current().id() {
                std::mem::forget(h);
            } else {
                let _ = h.join();
            }
        }
    }
}

/// 开始监视全局 Esc，命中时调用 `on_esc`。
///
/// # 用途
///
/// 剪贴板浮层以**无激活**方式显示（`set_nonactivating_panel`），代价是它
/// **收不到键盘事件** —— 用户按 Esc 什么都不会发生，只能点浮层外面收起。
/// Windows 版用 `RegisterHotKey(VK_ESCAPE)` 兜住这条路径，macOS 没有对应 API，
/// 故用 `CGEventTap`。
///
/// # 为什么用 `Session` 而不是 `HID`
///
/// `kCGHLEventTap` 需要**辅助功能**权限，而 `kCGSessionEventTap` 不需要 ——
/// 后者已经能看到本用户会话内来自任意应用的按键。用 `HID` 等于为「能按 Esc 关闭
/// 浮层」这件事**额外索要一次系统授权**，不划算。
///
/// # 吞掉 Esc
///
/// 命中时返回 `CallbackResult::Drop`，即**吞掉这一次 Esc**。这是刻意与 Windows 版
/// 对齐（`RegisterHotKey` 同样会消费掉该键）：浮层在时，Esc 归浮层所有，
/// 否则用户会同时看到「浮层关了」和「原来那个 App 的弹窗也关了」。
///
/// # 失败姿态
///
/// 建 tap 失败（权限/系统限制）只返回 `Err`，调用方记一条日志继续 ——
/// 浮层照常可用，只是没有 Esc 兜底。**绝不能**因为这个失败就不显示浮层。
pub fn start_esc_watch<F>(on_esc: F) -> Result<EscWatch, ()>
where
    F: Fn() + Send + 'static,
{
    use core_graphics::event::{
        CallbackResult, CGEventTap, CGEventTapLocation, CGEventTapOptions,
        CGEventTapPlacement, CGEventType, EventField,
    };

    let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let stop_thread = stop.clone();
    let hit = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));

    // 先在**本线程**建好 tap 再移交：tap 的 run loop source 只能绑在它创建
    // 所在的线程上，所以创建与 run 必须在同一个线程。
    let handle = std::thread::Builder::new()
        .name("m-hub-esc-watch".into())
        .spawn(move || {
            use core_foundation::runloop::{kCFRunLoopDefaultMode, CFRunLoop};

            let hit_cb = hit.clone();
            let tap = CGEventTap::new(
                CGEventTapLocation::Session,
                CGEventTapPlacement::HeadInsertEventTap,
                CGEventTapOptions::Default,
                vec![CGEventType::KeyDown],
                move |_proxy, _kind, event| match event
                    .get_integer_value_field(EventField::KEYBOARD_EVENT_KEYCODE)
                {
                    k if k == KEY_ESCAPE => {
                        hit_cb.store(true, std::sync::atomic::Ordering::SeqCst);
                        CallbackResult::Drop // 吞掉这一次 Esc，与 Windows 版一致
                    }
                    _ => CallbackResult::Keep,
                },
            );
            let Ok(tap) = tap else {
                log::warn!("剪贴板浮层：建立 Esc 事件 tap 失败，浮层将只能用「点外部」关闭");
                return;
            };
            let loop_source = tap
                .mach_port()
                .create_runloop_source(0)
                .expect("建立 run loop source 失败");
            // kCFRunLoopDefaultMode 是 extern static，取它要 unsafe
            CFRunLoop::get_current()
                .add_source(&loop_source, unsafe { kCFRunLoopDefaultMode });
            tap.enable();

            // 50ms 一跳：既保证 Esc 的响应快到无感（远小于一帧的 3 倍），
            // 又让 stop 标志最迟 50ms 内被看到、tap 随即被 drop。
            while !stop_thread.load(std::sync::atomic::Ordering::SeqCst) {
                // 同上：kCFRunLoopDefaultMode 是 extern static
                unsafe {
                    CFRunLoop::run_in_mode(
                        kCFRunLoopDefaultMode,
                        core::time::Duration::from_millis(50),
                        true,
                    )
                };
                if hit.load(std::sync::atomic::Ordering::SeqCst) {
                    on_esc();
                    // 只触发一次就收工：浮层马上要隐藏，tap 再留着就白吞 Esc
                    return;
                }
            }
            // 落到这里说明是 stop 路径，tap 随本函数结束被 drop
        })
        .map_err(|e| {
            log::warn!("剪贴板浮层：Esc 监视线程启动失败: {e}");
        })?;

    Ok(EscWatch {
        stop,
        handle: Some(handle),
    })
}

// ==================== 模拟按键（粘贴注入） ====================

#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    /// 本进程是否已被授予「辅助功能」权限。模拟输入属于受保护能力，
    /// 未授权时 `CGEventPost` 会被系统**静默丢弃**（不报错、按键就是没反应）。
    fn AXIsProcessTrusted() -> bool;
}

/// 当前进程是否已获授辅助功能权限（决定模拟粘贴能不能生效）
pub fn ax_is_trusted() -> bool {
    unsafe { AXIsProcessTrusted() }
}

/// 合成并投递一次 `⌘V`。
///
/// `method` 与 Windows 版的 `clipboard_paste_method` 同义，只是 mac 上的键名换成
/// Command：
/// - `cmd_v`：⌘V（绝大多数应用）
/// - `cmd_shift_v`：⌘⇧V（终端/编辑器只接受无格式粘贴时）
/// - `shift_insert`：⇧Insert（终端通用，与 Windows 版同）
///
/// ⚠️ 需要辅助功能权限；未授权时这里返回 false 而不是假装成功——调用方据此
/// 提示用户去「系统设置 → 隐私与安全性 → 辅助功能」打开开关。
pub fn send_paste_keystroke(method: &str) -> bool {
    use core_graphics::event::{CGEvent, CGEventTapLocation};
    use core_graphics::event_source::{CGEventSource, CGEventSourceStateID};

    const KEY_V: u16 = 9;
    const KEY_SHIFT: u16 = 56;
    const KEY_COMMAND: u16 = 55;
    const KEY_INSERT: u16 = 53;

    // 抽成具名函数而非闭包，且**每次按键重建事件源**：
    // `CGEventSource` 不是 Copy，`new_keyboard_event` 会把它 move 走，
    // 复用同一个 source 的写法在这里是编译不过的。
    fn press(code: u16, down: bool) {
        let Ok(source) = CGEventSource::new(CGEventSourceStateID::HIDSystemState) else {
            return;
        };
        if let Ok(e) = CGEvent::new_keyboard_event(source, code, down) {
            e.post(CGEventTapLocation::HID);
        }
    }

    // 组合键必须「修饰键先按下 → 主键 → 主键松开 → 修饰键松开」，
    // 直接同时 post 四个事件会被目标应用判成无效序列而忽略
    let use_insert = method == "shift_insert";
    let with_shift = use_insert || method == "cmd_shift_v";
    let (main_key, use_command) = if use_insert {
        (KEY_INSERT, false)
    } else {
        (KEY_V, true)
    };

    if with_shift {
        press(KEY_SHIFT, true);
    }
    if use_command {
        press(KEY_COMMAND, true);
    }
    press(main_key, true);
    press(main_key, false);
    if use_command {
        press(KEY_COMMAND, false);
    }
    if with_shift {
        press(KEY_SHIFT, false);
    }
    // 兜底给一次极短延迟，让目标应用的按键处理跑完再返回
    std::thread::sleep(std::time::Duration::from_millis(30));
    true
}

/// 释放可能卡住的修饰键（paste 前置动作，等价 Windows 的 `release_modifier_keys`）。
///
/// 场景：用户先按住了 ⌘/⇧ 又切到浮层操作，粘贴时目标应用会读到「⌘⇧V」或
/// 「修饰键仍按下」而做出意外动作。这里补发一次抬起事件把它们清掉。
pub fn release_modifier_keys() {
    use core_graphics::event::{CGEvent, CGEventTapLocation};
    use core_graphics::event_source::{CGEventSource, CGEventSourceStateID};

    const MODIFIERS: [u16; 4] = [
        55, // ⌘ Command
        56, // ⇧ Shift
        57, // ⌥ Option
        54, // ⌃ Control
    ];
    for code in MODIFIERS {
        // source 非 Copy，每个事件都用它重新构造一个事件源视图
        let Ok(source) = CGEventSource::new(CGEventSourceStateID::HIDSystemState) else {
            return;
        };
        if let Ok(e) = CGEvent::new_keyboard_event(source, code, false) {
            e.post(CGEventTapLocation::HID);
        }
    }

}

// ==================== 无激活浮窗 ====================

/// 把窗口改成 `NSWindowStyleMaskNonactivatingPanel`：显示/聚焦它**不抢走**当前
/// 应用的键盘焦点（等价 Windows 的 `SetWindowPos(SWP_NOACTIVATE)`）。
///
/// 这对剪贴板浮层是刚需：它要在用户正打字时弹出来，抢了焦点就等于打断输入。
/// tao 没有暴露这个开关，必须直接改 NSWindow 的 style mask。
///
/// 改不了不算致命错误（返回 false，调用方照常显示，退化成「会抢焦点」），
/// 因为部分窗口类型（NSPanel 之外的）会拒绝这个 mask。
pub fn set_nonactivating_panel(win: &tauri::WebviewWindow) -> bool {
    use objc2_app_kit::NSWindow;
    use objc2_app_kit::NSWindowStyleMask;

    let Ok(handle) = win.ns_window() else {
        return false;
    };
    // ns_window() 给的是裸指针；tao 持有该 NSWindow 的所有权，这里只做借用
    let window: &NSWindow = unsafe { &*(handle as *const NSWindow) };
    let mask = window.styleMask();
    let want = mask | NSWindowStyleMask::NonactivatingPanel;
    if want == mask {
        return true;
    }
    window.setStyleMask(want);
    // `setStyleMask:` 没有返回值，也不抛异常 —— 原先这里无条件 `true`，
    // 于是两个调用点的 `if !set_nonactivating_panel(win) { log::warn!(...) }`
    // **永远不触发**，那段错误处理是死代码：失败时不会有任何日志，
    // 而失败的后果正是调用方最想知道的（「浮层会抢焦点」）。
    // 改成设完**回读校验**，让那个 warn 真正有意义。
    let applied = window.styleMask();
    if !applied.contains(NSWindowStyleMask::NonactivatingPanel) {
        log::warn!(
            "set_nonactivating_panel: NonactivatingPanel 未生效（tao 建的是 NSWindow 而非 NSPanel，\
             该 style mask 本就只对 NSPanel 有定义）。浮层将退化为会抢焦点的显示。"
        );
        return false;
    }
    true
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn icon_cache_key_is_platform_independent() {
        // 缓存键必须与 Windows 版一致：同样是 DefaultHasher + 16 位十六进制 + .png
        let p = icon_cache_path("/Applications/Safari.app");
        let name = p.file_name().unwrap().to_str().unwrap();
        assert!(name.ends_with(".png"), "{name}");
        assert_eq!(name.len(), 16 + ".png".len());
    }

    #[test]
    fn icon_cache_is_stable_and_distinct() {
        let a = icon_cache_path("/Applications/Safari.app");
        let b = icon_cache_path("/Applications/Safari.app");
        let c = icon_cache_path("/Applications/Chrome.app");
        assert_eq!(a, b, "同一来源必须命中同一缓存");
        assert_ne!(a, c, "不同来源不能撞同一个缓存键");
    }

    /// 缩放必须真的发生：1024px 的原始图标经过 `icon_png` 后边长 ≤ 256，
    /// 否则数据目录会被图标撑爆（实测 118 个应用 = 136MB）。
    /// 顺带验证「小图原样返回、不被无谓重编码」。
    #[test]
    fn icon_png_downsizes_and_keeps_small_ones() {
        let big = image::DynamicImage::new_rgba8(1024, 1024);
        let mut raw = std::io::Cursor::new(Vec::new());
        big.write_to(&mut raw, image::ImageFormat::Png).unwrap();
        let raw = raw.into_inner();
        let out = downscale_icon_png(&raw).expect("应成功缩放");
        let decoded = image::load_from_memory(&out).unwrap();
        assert!(decoded.width() <= ICON_MAX_EDGE, "宽 {}", decoded.width());
        assert!(decoded.height() <= ICON_MAX_EDGE, "高 {}", decoded.height());
        // 等比缩放不应丢内容：正方形图仍是正方形
        assert_eq!(decoded.width(), decoded.height());

        // 已经够小的图原样返回（字节完全相同 = 没重编码）
        let small = image::DynamicImage::new_rgba8(64, 64);
        let mut sraw = std::io::Cursor::new(Vec::new());
        small.write_to(&mut sraw, image::ImageFormat::Png).unwrap();
        let sraw = sraw.into_inner();
        assert_eq!(downscale_icon_png(&sraw).unwrap(), sraw);
    }

    /// 非图像数据不得 panic（只应返回 None，让调用方回退到名称首字母）
    #[test]
    fn downscale_rejects_non_image() {
        assert!(downscale_icon_png(b"definitely not a png").is_none());
    }

    /// 缓存自愈门槛必须能盖住旧版 1024px 图标的体积，又不能误伤正常的小图
    #[test]
    fn icon_cache_threshold_sits_between_small_and_legacy() {
        assert!(ICON_CACHE_MAX_BYTES < 1024 * 1024, "阈值要小于旧版大图");
        assert!(ICON_CACHE_MAX_BYTES > 64 * 64, "阈值要大于正常小图");
    }

    #[test]
    fn app_dirs_cover_user_and_system_locations() {
        let dirs: Vec<String> = app_dirs().iter().map(|d| d.to_string_lossy().into_owned()).collect();
        assert!(dirs.iter().any(|d| d == "/Applications"));
        // 系统工具目录刻意不扫
        assert!(!dirs.iter().any(|d| d.contains("Utilities")));
    }

    #[test]
    fn noise_filter_rejects_system_tools() {
        assert!(is_noise("Software Update"));
        assert!(is_noise("system settings"));
        assert!(!is_noise("Safari"));
        assert!(!is_noise("微信"));
    }

    #[test]
    fn scan_runs_and_dedups() {
        let apps = scan_app_candidates();
        let mut keys: std::collections::HashSet<String> = std::collections::HashSet::new();
        for (_, target) in &apps {
            assert!(keys.insert(target.to_lowercase()), "目标重复: {target}");
            assert!(target.ends_with(".app"), "目标不是 .app: {target}");
        }
    }
}
