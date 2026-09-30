//! 剪贴板历史：监听 / 读取 / 写入 / 粘贴注入 / 历史浮层。
//!
//! # 两平台的机制差异（读代码前必读）
//!
//! | 能力 | Windows | macOS |
//! |---|---|---|
//! | 感知变化 | `AddClipboardFormatListener` → `WM_CLIPBOARDUPDATE` **消息推送** | `NSPasteboard.changeCount` **轮询**（AppKit 无推送 API） |
//! | 文本 | `CF_UNICODETEXT` | `public.utf8-plain-text` |
//! | 富文本 | `CF_HTML` + `StartFragment/EndFragment` 偏移解析 | `public.html`（整份文档，交给前端 sanitize） |
//! | 图片 | `PNG` 自定义格式 / `CF_DIBV5` → `CF_DIB` | `public.png` / `public.tiff` |
//! | 文件 | `CF_HDROP` + `DragQueryFileW` | `public.file-url`（+ 老的 `public.file-name` 兜底） |
//! | 点外部收起 | `SetWindowsHookExW(WH_MOUSE_LL)` 低级鼠标钩子 | 复用 [`crate::floating_ball`] 的光标轮询（AppKit 无全局鼠标钩子） |
//! | 粘贴注入 | `keybd_event` 扫 `Ctrl+V` | `CGEvent` 合成 `⌘V`（**需辅助功能授权**） |
//! | 归还焦点 | `SetForegroundWindow(hwnd)` | `NSRunningApplication.activateWithOptions` |
//!
//! ## macOS 的三处硬约束（改代码前先读）
//!
//! 1. **坐标原点翻转**：Cocoa 的屏幕坐标原点在**左下角**、y 向上；Windows 是左上角、
//!    y 向下。`cursor_anchor_position` 是全工程唯一把鼠标位置换算成 Tauri 窗口坐标的
//!    地方，Y 轴翻错会让浮层跑到屏幕顶部外面去——本文件里任何 `y` 参与算式的地方
//!    都要过这一关。
//! 2. **粘贴注入要辅助功能权限**：`CGEvent` 合成按键属于「模拟输入」，macOS 10.15 起
//!    必须先在「系统设置 → 隐私与安全性 → 辅助功能」里给 m-hub 打勾，否则系统**静默
//!    丢弃**事件（不报错、按键就是没反应）。`send_paste_keystroke` 启动时打一条
//!    `AXIsProcessTrusted` 检查并落日志，把这个坑显式告诉用户，而不是让人对着
//!    「点了没反应」猜。
//! 3. **没有全局鼠标钩子**：`WH_MOUSE_LL` 在 mac 上没有等价物。浮层「点外部收起」
//!    改用 `floating_ball` 里已有的 100ms 光标轮询（同一套边缘监视线程，顺带做
//!    矩形判定），不额外开线程。

use crate::commands::DbState;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::Mutex;
use tauri::{AppHandle, Emitter, Manager, WebviewUrl};
#[cfg(target_os = "windows")]
use windows_sys::Win32::Foundation::{GlobalFree, POINT, RECT};
#[cfg(target_os = "windows")]
use windows_sys::Win32::Graphics::Gdi::{GetMonitorInfoW, MonitorFromPoint, MONITORINFO, MONITOR_DEFAULTTONEAREST};
#[cfg(target_os = "windows")]
use windows_sys::Win32::System::DataExchange::{
    AddClipboardFormatListener, CloseClipboard, EmptyClipboard,
    GetClipboardData, IsClipboardFormatAvailable, OpenClipboard, RegisterClipboardFormatW,
    RemoveClipboardFormatListener, SetClipboardData,
};
#[cfg(target_os = "windows")]
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
#[cfg(target_os = "windows")]
use windows_sys::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock, GMEM_MOVEABLE, GMEM_ZEROINIT};
#[cfg(target_os = "windows")]
use windows_sys::Win32::System::Ole::{CF_DIB, CF_DIBV5, CF_HDROP, CF_UNICODETEXT};
#[cfg(target_os = "windows")]
use windows_sys::Win32::UI::Shell::{DragQueryFileW, DROPFILES, HDROP};
#[cfg(target_os = "windows")]
use windows_sys::Win32::System::Threading::AttachThreadInput;
#[cfg(target_os = "windows")]
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    keybd_event, RegisterHotKey, SendInput, UnregisterHotKey, INPUT, INPUT_KEYBOARD, KEYBDINPUT,
    KEYEVENTF_EXTENDEDKEY, KEYEVENTF_KEYUP, KEYEVENTF_SCANCODE, MAPVK_VK_TO_VSC, MapVirtualKeyW,
    VK_CONTROL, VK_ESCAPE, VK_INSERT, VK_LMENU, VK_LCONTROL, VK_LSHIFT, VK_LWIN, VK_MENU,
    VK_RCONTROL, VK_RMENU, VK_RSHIFT, VK_RWIN, VK_SHIFT,
};
#[cfg(target_os = "windows")]
use windows_sys::Win32::UI::WindowsAndMessaging::{
    BringWindowToTop, CallNextHookEx, CreateWindowExW, DefWindowProcW, DestroyWindow,
    DispatchMessageW, GetCursorPos, GetForegroundWindow, GetMessageW, GetWindowLongPtrW,
    GetWindowRect, GetWindowThreadProcessId, IsIconic, IsWindowVisible, PostMessageW,
    RegisterClassW, SetForegroundWindow, SetWindowLongPtrW, SetWindowsHookExW, SetWindowPos,
    ShowWindow, TranslateMessage, UnhookWindowsHookEx, UnregisterClassW, GWL_EXSTYLE,
    HWND_MESSAGE, HWND_TOPMOST, MSG, MSLLHOOKSTRUCT, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE,
    SWP_SHOWWINDOW, SW_HIDE, SW_SHOWNA, SW_RESTORE, WH_MOUSE_LL, WNDCLASSW, WS_EX_NOACTIVATE,
    WM_APP, WM_CLIPBOARDUPDATE, WM_HOTKEY, WM_LBUTTONDOWN, WM_MBUTTONDOWN, WM_RBUTTONDOWN,
};

/// 剪贴板浮窗窗口 label（App.vue 按此路由渲染 ClipboardOverlay）
pub const CLIPBOARD_WINDOW_LABEL: &str = "clipboard";
pub const CLIPBOARD_WIDTH: f64 = 520.0;
pub const CLIPBOARD_HEIGHT: f64 = 440.0;

/// 相同内容去重：内容一致的记录直接挪到最前，不新增重复条目
/// 自复制回声抑制窗口（粘贴/复制历史项后，事件监听会再次触发，用 hash 识别并跳过）
const ECHO_SUPPRESS_SECONDS: u64 = 10;
/// 富文本读取递增重试（Office 等分阶段写入剪贴板：先文本后 HTML）
const HTML_RETRY_DELAYS_MS: [u64; 7] = [0, 40, 80, 140, 220, 360, 560];
/// 剪贴板事件去抖：一次复制可能触发多次 WM_CLIPBOARDUPDATE（多格式逐步写入）
const SETTLE_MS: u64 = 80;

/// 唤起浮层前的焦点归属。两平台粒度不同，用类型别名抹平，
/// 上层（`paste_to_previous_window` / `hide_overlay`）不必分叉写法。
///
/// - **Windows**：前台**窗口句柄**（HWND）——能精确到「还焦点给哪个窗口」；
/// - **macOS**：前台**应用 bundle id**——AppKit 没有「前台窗口句柄」这种公开概念
///   （`NSWorkspace.frontmostApplication` 给的是应用），粒度更粗但足够。
#[cfg(target_os = "windows")]
pub type PrevFocus = isize;
#[cfg(target_os = "macos")]
pub type PrevFocus = String;

/// 跨命令共享的剪贴板浮层状态
pub struct ClipboardState {
    /// 唤起浮层前聚焦的窗口（粘贴时先恢复焦点再注入粘贴快捷键）
    pub prev_focus: Mutex<Option<PrevFocus>>,
}

impl Default for ClipboardState {
    fn default() -> Self {
        Self {
            prev_focus: Mutex::new(None),
        }
    }
}

/// 最近一次「我们自己写入」的剪贴板内容指纹（文本+HTML 哈希 + 时间戳），
/// 用于抑制监听回声：粘贴/复制历史项会再次触发剪贴板事件，不应重复入库。
static LAST_SELF_SET: Mutex<Option<(u64, std::time::Instant)>> = Mutex::new(None);

/// 全局鼠标钩子句柄（浮层可见时启用，点击浮层外部即收起）
#[cfg(target_os = "windows")]
static MOUSE_HOOK: Mutex<Option<isize>> = Mutex::new(None);

/// 浮层窗口句柄缓存：钩子回调运行在监听线程，禁止跨线程 Tauri 调用，
/// 显示浮层时写入，钩子回调只读它做矩形判断。
#[cfg(target_os = "windows")]
static OVERLAY_HWND: Mutex<Option<isize>> = Mutex::new(None);

/// Esc 事件监视的持有句柄（macOS 侧的 Esc 兜底关闭，等价于 Windows 的
/// `register_esc_hotkey`）。浮层隐藏时 drop —— tap 随之拆除，不再吞 Esc。
#[cfg(target_os = "macos")]
static ESC_WATCH: Mutex<Option<crate::mac::EscWatch>> = Mutex::new(None);

/// 停掉 Esc 监视（drop 即拆除事件 tap，之后不再吞 Esc）。
///
/// **凡是收起浮层的路径都必须调它**，漏一条就会留下一个还在吞 Esc 的 tap：
/// 最容易漏的是「粘贴后收起」——那条路径只 `hide_overlay_window`，不走
/// `hide_overlay`，于是用户按一次 ⌃⌘V 粘贴之后，Esc 就永久失灵了
/// （而且没有任何报错，只表现为「Esc 什么都不发生」）。
#[cfg(target_os = "macos")]
fn stop_esc_watch() {
    // ⚠️ **必须先出锁再 drop**（2026-09-29 修，原实现是自死锁）。
    // `*slot = None` 会在**锁仍持有时** drop 掉旧的 `EscWatch`，而它的
    // `Drop` 里要 `h.join()` 等监视线程收工。于是形成死锁环：
    //   边缘监视线程：持 ESC_WATCH ──join──▶ 等 tap 线程结束
    //   tap 线程    ：等 ESC_WATCH（它正走 on_esc → hide_overlay → stop_esc_watch）
    // 两者互等，**永久**卡死，且没有任何报错。
    //
    // 症状：悬浮球悬停不再滑出、拖拽不再吸附、剪贴板浮层点外部收不起来，
    // 而且 **Esc 被永久吞掉**（tap 线程也卡住、拆不掉）—— 任何 App 的 Esc 都失效，
    // 只能重启 m-hub。
    //
    // 原先的 `Drop` 里只挡了「自己 join 自己」（tap 线程自己 drop 自己的场景），
    // 没挡「drop 者 ≠ 被 join 者、但被 join 者要同一把锁」—— 这是标准的
    // lock-ordering 违规，加 join 之前必须想清楚被 join 线程会不会回头拿锁。
    let taken = {
        let mut slot = ESC_WATCH.lock().unwrap_or_else(|e| e.into_inner());
        slot.take()
    };
    // 锁已释放，此刻 drop 才安全
    drop(taken);
}

/// 一次延迟窗口操作任务：粘贴后恢复焦点+注入按键，或收起后归还焦点。
/// 统一交给单一 worker 线程串行执行，避免频繁 spawn 短命线程造成线程数波动。
#[cfg(target_os = "windows")]
enum DelayedWinOp {
    /// 粘贴到唤起前窗口：先释放修饰键、必要时恢复目标窗口焦点，再按目标应用注入粘贴快捷键
    Paste {
        hwnd: isize,
        paste_method: &'static str,
    },
    /// 收起浮层后归还焦点给唤起前窗口（若前台仍在本进程）
    RestoreFocus { hwnd: isize },
}

/// 延迟窗口操作 worker：所有粘贴/归还焦点在同一个线程上串行执行
#[cfg(target_os = "windows")]
static WIN_OP_TX: Mutex<Option<std::sync::mpsc::Sender<DelayedWinOp>>> = Mutex::new(None);

/// 提交一次延迟窗口操作（粘贴或归还焦点）
#[cfg(target_os = "windows")]
fn submit_win_op(op: DelayedWinOp) {
    let tx = WIN_OP_TX.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(tx) = tx.as_ref() {
        let _ = tx.send(op);
    }
}

/// 初始化延迟窗口操作 worker（应用启动时调用一次）
#[cfg(target_os = "windows")]
pub fn init_win_op_worker() {
    let (tx, rx) = std::sync::mpsc::channel::<DelayedWinOp>();
    if let Ok(mut guard) = WIN_OP_TX.lock() {
        *guard = Some(tx);
    }
    std::thread::spawn(move || {
        while let Ok(op) = rx.recv() {
            match op {
                DelayedWinOp::Paste { hwnd, paste_method } => {
                    // 先给系统一点时间完成窗口隐藏与焦点转移
                    std::thread::sleep(std::time::Duration::from_millis(50));
                    release_modifier_keys();
                    // 前台若仍是我们的窗口（浮层刚隐藏/被激活过），先恢复目标窗口焦点；
                    // 浮层从未抢焦点时（无激活显示）前台始终是目标应用，无需恢复。
                    let fg = unsafe { GetForegroundWindow() } as isize;
                    let own_pid = std::process::id();
                    let mut fg_pid: u32 = 0;
                    unsafe {
                        GetWindowThreadProcessId(fg as *mut core::ffi::c_void, &mut fg_pid);
                    }
                    log::info!(
                        "剪贴板粘贴：隐藏后前台 hwnd={:#x} pid={}，本进程 pid={}，目标 hwnd={:#x}",
                        fg,
                        fg_pid,
                        own_pid,
                        hwnd
                    );
                    if fg_pid == own_pid {
                        force_focus_window(hwnd as *mut core::ffi::c_void);
                        log::info!("剪贴板粘贴：前台仍在本进程，已恢复目标窗口焦点");
                    }
                    // 沉降：等焦点真正移交到目标窗口
                    std::thread::sleep(std::time::Duration::from_millis(150));
                    let fg_after = unsafe { GetForegroundWindow() } as isize;
                    let visible = unsafe { IsWindowVisible(hwnd as *mut core::ffi::c_void) } != 0;
                    log::info!(
                        "剪贴板粘贴：沉降后前台 hwnd={:#x}，目标可见={}",
                        fg_after,
                        visible
                    );
                    // 验证目标窗口仍可见（可能已关闭/最小化），避免输入注入到错误窗口
                    if !visible {
                        log::warn!("剪贴板粘贴：目标窗口不可见，跳过输入注入");
                        continue;
                    }
                    log::info!("剪贴板粘贴：采用粘贴方式 {:?}，注入按键", paste_method);
                    send_paste_keystroke(paste_method);
                }
                DelayedWinOp::RestoreFocus { hwnd } => {
                    std::thread::sleep(std::time::Duration::from_millis(100));
                    let fg = unsafe { GetForegroundWindow() } as isize;
                    let mut pid: u32 = 0;
                    unsafe {
                        GetWindowThreadProcessId(fg as *mut core::ffi::c_void, &mut pid);
                    }
                    if pid == std::process::id() {
                        force_focus_window(hwnd as *mut core::ffi::c_void);
                    }
                }
            }
        }
    });
}

fn content_hash(text: &str, html: Option<&str>) -> u64 {
    let mut h = DefaultHasher::new();
    text.hash(&mut h);
    html.hash(&mut h);
    h.finish()
}

fn is_self_set(text: &str, html: Option<&str>) -> bool {
    let Ok(guard) = LAST_SELF_SET.lock() else {
        return false;
    };
    let Some((hash, at)) = guard.as_ref() else {
        return false;
    };
    at.elapsed().as_secs() < ECHO_SUPPRESS_SECONDS && *hash == content_hash(text, html)
}

/// 恢复记录时清除自复制指纹：暂停期间从浮窗复制/粘贴会写入指纹，
/// 恢复后短时间内复制相同内容会被误判为回声丢弃，这里重置为全新状态。
pub fn clear_self_set_fingerprint() {
    if let Ok(mut guard) = LAST_SELF_SET.lock() {
        *guard = None;
    }
}

/// 剪贴板读取结果（优先级：图片 > 文件 > 文本）
pub enum ClipboardPayload {
    Text { content: String, html: Option<String> },
    Image { bytes: Vec<u8>, format: ImageFormat },
    Files { paths: Vec<String> },
}

/// 图片原始格式（落盘快照时的编码依据）
#[derive(Clone, Copy, PartialEq)]
pub enum ImageFormat {
    /// PNG 剪贴板格式（现代应用主流，两平台都有），原样存 .png
    Png,
    /// CF_DIB / CF_DIBV5 位图，包装 14 字节文件头存 .bmp（仅 Windows）
    Dib,
    /// `public.tiff`：macOS 截图与预览的原生格式（仅 macOS）
    Tiff,
}

impl ImageFormat {
    /// 落盘时的文件扩展名（不带点）。
    /// 三个变体在两平台都穷举：某平台读不到某种格式不代表该变体不存在，
    /// 写半边 match 会立刻变成 non-exhaustive 编译错误。
    pub fn ext(&self) -> &'static str {
        match self {
            ImageFormat::Png => "png",
            ImageFormat::Dib => "bmp",
            ImageFormat::Tiff => "tiff",
        }
    }
}

/// 图片/文件字节指纹：用于回声抑制与「相同内容只留一条」去重
fn hash_bytes(bytes: &[u8]) -> u64 {
    let mut h = DefaultHasher::new();
    bytes.hash(&mut h);
    h.finish()
}

/// 文件列表指纹：路径 + 各文件大小（同名不同内容也能区分），用于回声抑制
fn hash_files(paths: &[String]) -> u64 {
    let mut h = DefaultHasher::new();
    for p in paths {
        p.hash(&mut h);
        if let Ok(meta) = std::fs::metadata(p) {
            meta.len().hash(&mut h);
        }
    }
    h.finish()
}

/// 判断给定哈希是否为「我们自己写入」的回声（图片/文件路径）
fn is_self_set_hash(hash: u64) -> bool {
    let Ok(guard) = LAST_SELF_SET.lock() else {
        return false;
    };
    let Some((h, at)) = guard.as_ref() else {
        return false;
    };
    at.elapsed().as_secs() < ECHO_SUPPRESS_SECONDS && *h == hash
}

/// 记录回声指纹（图片/文件写入后调用）
fn record_self_set_hash(hash: u64) {
    if let Ok(mut guard) = LAST_SELF_SET.lock() {
        *guard = Some((hash, std::time::Instant::now()));
    }
}

// ---- 剪贴板读取（Windows） ----

fn read_utf16_null_terminated(ptr: *const u16, max_chars: usize) -> String {
    if ptr.is_null() {
        return String::new();
    }
    let mut len = 0usize;
    unsafe {
        while len < max_chars && *ptr.add(len) != 0 {
            len += 1;
        }
    }
    if len == 0 {
        return String::new();
    }
    let slice = unsafe { std::slice::from_raw_parts(ptr, len) };
    String::from_utf16_lossy(slice)
}

/// 从 CF_HTML 数据中提取实际片段（StartFragment/EndFragment 偏移优先，标签兜底）
#[cfg(target_os = "windows")]
fn extract_cf_html_fragment(bytes: &[u8]) -> Option<String> {
    let s = String::from_utf8_lossy(bytes);
    let parse = |key: &str| -> Option<usize> {
        let i = s.find(key)?;
        let digits: String = s[i + key.len()..]
            .chars()
            .take(10)
            .take_while(|c| c.is_ascii_digit())
            .collect();
        digits.parse::<usize>().ok()
    };
    if let (Some(a), Some(b)) = (parse("StartFragment:"), parse("EndFragment:")) {
        if b > a && b <= s.len() {
            return Some(s[a..b].to_string());
        }
    }
    let a = s.find("<!--StartFragment-->")?;
    let b = s.find("<!--EndFragment-->")?;
    let start = a + "<!--StartFragment-->".len();
    if b > start {
        Some(s[start..b].to_string())
    } else {
        None
    }
}

/// 读取剪贴板纯文本与可选 HTML 片段（失败返回 None）
#[cfg(target_os = "windows")]
pub fn read_clipboard() -> Option<(String, Option<String>)> {
    unsafe {
        if OpenClipboard(std::ptr::null_mut()) == 0 {
            return None;
        }

        let mut text: Option<String> = None;
        let handle = GetClipboardData(CF_UNICODETEXT as u32);
        if !handle.is_null() {
            let ptr = GlobalLock(handle);
            if !ptr.is_null() {
                let t = read_utf16_null_terminated(ptr as *const u16, 1_000_000);
                if !t.trim().is_empty() {
                    text = Some(t);
                }
                GlobalUnlock(handle);
            }
        }

        let html = read_html_fragment_unlocked();

        CloseClipboard();
        if text.is_none() && html.is_none() {
            None
        } else {
            Some((text.unwrap_or_default(), html))
        }
    }
}

/// 读取剪贴板 HTML 片段（调用方需已 OpenClipboard）
#[cfg(target_os = "windows")]
unsafe fn read_html_fragment_unlocked() -> Option<String> {
    let fmt = RegisterClipboardFormatW(windows_sys::core::w!("HTML Format"));
    if fmt == 0 {
        return None;
    }
    let h = GetClipboardData(fmt);
    if h.is_null() {
        return None;
    }
    let ptr = GlobalLock(h);
    if ptr.is_null() {
        return None;
    }
    let mut len = 0usize;
    while len < 2_000_000 && *(ptr as *const u8).add(len) != 0 {
        len += 1;
    }
    let html = if len > 0 {
        let bytes = std::slice::from_raw_parts(ptr as *const u8, len);
        extract_cf_html_fragment(bytes)
    } else {
        None
    };
    GlobalUnlock(h);
    html
}

/// 读取剪贴板：优先拿文本；仅当剪贴板已注册 HTML 格式（内容可能仍在分阶段写入）时，
/// 才按递增延迟重试，避免只存到纯文本丢失富文本格式。
/// 之前用 clipboard_format_count() > 1 判断「是否富文本」，但纯文本复制（记事本等）也常带
/// CF_TEXT/CF_OEMTEXT/CF_LOCALE 等多个格式，导致每次复制都空跑 ~1.4s 的重试、入库明显延迟。
#[cfg(target_os = "windows")]
pub fn read_clipboard_with_retry() -> Option<(String, Option<String>)> {
    let (text, html) = read_clipboard()?;
    if html.is_some() || text.trim().is_empty() {
        return Some((text, html));
    }
    let has_html_format = unsafe {
        let fmt = RegisterClipboardFormatW(windows_sys::core::w!("HTML Format"));
        fmt != 0 && IsClipboardFormatAvailable(fmt) != 0
    };
    if !has_html_format {
        return Some((text, html));
    }
    for &d in &HTML_RETRY_DELAYS_MS[1..] {
        if d > 0 {
            std::thread::sleep(std::time::Duration::from_millis(d));
        }
        unsafe {
            if OpenClipboard(std::ptr::null_mut()) != 0 {
                let html = read_html_fragment_unlocked();
                CloseClipboard();
                if let Some(h) = html {
                    if !h.trim().is_empty() {
                        return Some((text, Some(h)));
                    }
                }
            }
        }
    }
    Some((text, html))
}

/// 读取剪贴板图片（调用方需已 OpenClipboard）：
/// 优先 PNG 格式（浏览器/微信/QQ/截图工具均放置），否则 CF_DIBV5 → CF_DIB。
/// 返回原始字节与格式（DIB 无文件头，落盘时补 BMP 头）。
#[cfg(target_os = "windows")]
unsafe fn read_image_unlocked() -> Option<(Vec<u8>, ImageFormat)> {
    let png_fmt = RegisterClipboardFormatW(windows_sys::core::w!("PNG"));
    if png_fmt != 0 {
        let h = GetClipboardData(png_fmt);
        if !h.is_null() {
            if let Some(bytes) = read_global_bytes(h) {
                return Some((bytes, ImageFormat::Png));
            }
        }
    }
    for fmt in [CF_DIBV5 as u32, CF_DIB as u32] {
        let h = GetClipboardData(fmt);
        if !h.is_null() {
            if let Some(bytes) = read_global_bytes(h) {
                return Some((bytes, ImageFormat::Dib));
            }
        }
    }
    None
}

/// 读取剪贴板文件列表（CF_HDROP，调用方需已 OpenClipboard）
#[cfg(target_os = "windows")]
unsafe fn read_files_unlocked() -> Option<Vec<String>> {
    let h = GetClipboardData(CF_HDROP as u32);
    if h.is_null() {
        return None;
    }
    let hdrop = h as HDROP;
    let count = DragQueryFileW(hdrop, 0xFFFF_FFFF, std::ptr::null_mut(), 0);
    if count == 0 {
        return None;
    }
    let mut paths = Vec::with_capacity(count as usize);
    for i in 0..count {
        // 先取长度，再取内容（宽字符路径）
        let len = DragQueryFileW(hdrop, i, std::ptr::null_mut(), 0) as usize;
        if len == 0 {
            continue;
        }
        let mut buf = vec![0u16; len + 1];
        let got = DragQueryFileW(hdrop, i, buf.as_mut_ptr(), (len + 1) as u32) as usize;
        if got > 0 {
            paths.push(String::from_utf16_lossy(&buf[..got]));
        }
    }
    if paths.is_empty() {
        None
    } else {
        Some(paths)
    }
}

/// 读取 Global 内存块为字节（GlobalSize 取准确长度，调用方需已 OpenClipboard）
#[cfg(target_os = "windows")]
unsafe fn read_global_bytes(h: windows_sys::Win32::Foundation::HGLOBAL) -> Option<Vec<u8>> {
    let ptr = GlobalLock(h);
    if ptr.is_null() {
        return None;
    }
    let size = GlobalSize(h);
    let bytes = if size > 0 {
        std::slice::from_raw_parts(ptr as *const u8, size).to_vec()
    } else {
        Vec::new()
    };
    GlobalUnlock(h);
    if bytes.is_empty() {
        None
    } else {
        Some(bytes)
    }
}

/// 尝试读取图片（自带 Open/Close 剪贴板）
#[cfg(target_os = "windows")]
fn try_read_image() -> Option<(Vec<u8>, ImageFormat)> {
    unsafe {
        if OpenClipboard(std::ptr::null_mut()) == 0 {
            return None;
        }
        let result = read_image_unlocked();
        CloseClipboard();
        result
    }
}

/// 尝试读取文件列表（自带 Open/Close 剪贴板）
#[cfg(target_os = "windows")]
fn try_read_files() -> Option<Vec<String>> {
    unsafe {
        if OpenClipboard(std::ptr::null_mut()) == 0 {
            return None;
        }
        let result = read_files_unlocked();
        CloseClipboard();
        result
    }
}

/// 读取剪贴板并按类型分派：**图片 > 文件 > 文本**（与 Windows 同一优先级）。
///
/// - Windows：图片 `PNG`/`CF_DIBV5`→`CF_DIB`，文件 `CF_HDROP`，文本带
///   `read_clipboard_with_retry`（Office 等分阶段写 HTML 的重试）；
/// - macOS：图片 `public.png`/`public.tiff`，文件 `public.file-url`，文本 + `public.html`。
#[cfg(target_os = "macos")]
pub fn read_clipboard_payload() -> Option<ClipboardPayload> {
    if let Some((bytes, kind)) = crate::mac::read_clipboard_image() {
        let format = if kind == "png" {
            ImageFormat::Png
        } else {
            ImageFormat::Tiff
        };
        return Some(ClipboardPayload::Image { bytes, format });
    }
    if let Some(paths) = crate::mac::read_clipboard_files() {
        if !paths.is_empty() {
            return Some(ClipboardPayload::Files { paths });
        }
    }
    let content = crate::mac::read_clipboard_text()?;
    let html = crate::mac::read_clipboard_html();
    Some(ClipboardPayload::Text { content, html })
}

/// 读取剪贴板并按类型分派：图片 > 文件 > 文本。
#[cfg(target_os = "windows")]
pub fn read_clipboard_payload() -> Option<ClipboardPayload> {
    if let Some((bytes, format)) = try_read_image() {
        return Some(ClipboardPayload::Image { bytes, format });
    }
    if let Some(paths) = try_read_files() {
        return Some(ClipboardPayload::Files { paths });
    }
    let (text, html) = read_clipboard_with_retry()?;
    Some(ClipboardPayload::Text { content: text, html })
}

// ---- 剪贴板写入 ----

/// 构造 CF_HTML 数据（含 StartHTML/EndHTML/StartFragment/EndFragment 偏移）
#[cfg(target_os = "windows")]
fn build_cf_html(fragment: &str) -> Vec<u8> {
    let body = format!(
        "<html><body><!--StartFragment-->{}</body>",
        fragment
    );
    // 头部四段偏移各占 10 位零填充数字：先用占位 0 求出真实头长，再据此算各偏移。
    // 之前硬编码 108 与实际头长（105 字节）不符，导致 StartFragment/EndFragment 整体错位，
    // 粘贴富文本时片段首尾错乱，且读回片段与写入片段不一致使回声抑制失效。
    let prefix_len = format!(
        "Version:0.9\r\nStartHTML:{:010}\r\nEndHTML:{:010}\r\nStartFragment:{:010}\r\nEndFragment:{:010}\r\n",
        0usize, 0usize, 0usize, 0usize
    )
    .len();
    let start_frag = prefix_len + "<html><body><!--StartFragment-->".len();
    let end_frag = start_frag + fragment.len();
    let end_html = prefix_len + body.len();
    let header = format!(
        "Version:0.9\r\nStartHTML:{:010}\r\nEndHTML:{:010}\r\nStartFragment:{:010}\r\nEndFragment:{:010}\r\n",
        prefix_len, end_html, start_frag, end_frag
    );
    let mut out = header.into_bytes();
    out.extend_from_slice(body.as_bytes());
    out
}

/// 写入剪贴板：纯文本 + 可选 HTML（粘贴回原处时优先还原富文本格式）。
/// 写入成功会记录内容指纹，抑制监听线程的回声重复入库。
#[cfg(target_os = "windows")]
pub fn set_clipboard(text: &str, html: Option<&str>) -> Result<(), String> {
    unsafe {
        if OpenClipboard(std::ptr::null_mut()) == 0 {
            return Err("无法打开系统剪贴板".into());
        }
        if EmptyClipboard() == 0 {
            CloseClipboard();
            return Err("无法清空系统剪贴板".into());
        }

        // CF_UNICODETEXT
        let mut buf: Vec<u16> = text.encode_utf16().collect();
        buf.push(0);
        let mem = GlobalAlloc(GMEM_MOVEABLE, buf.len() * 2);
        if mem.is_null() {
            CloseClipboard();
            return Err("剪贴板内存分配失败".into());
        }
        let dst = GlobalLock(mem);
        if dst.is_null() {
            GlobalFree(mem);
            CloseClipboard();
            return Err("剪贴板内存锁定失败".into());
        }
        std::ptr::copy_nonoverlapping(buf.as_ptr(), dst as *mut u16, buf.len());
        GlobalUnlock(mem);
        if SetClipboardData(CF_UNICODETEXT as u32, mem).is_null() {
            GlobalFree(mem);
            CloseClipboard();
            return Err("写入剪贴板失败".into());
        }

        // CF_HTML（可选，失败不阻塞纯文本写入）
        if let Some(h) = html {
            let fmt = RegisterClipboardFormatW(windows_sys::core::w!("HTML Format"));
            if fmt != 0 {
                let bytes = build_cf_html(h);
                let mem = GlobalAlloc(GMEM_MOVEABLE | GMEM_ZEROINIT, bytes.len());
                if !mem.is_null() {
                    let dst = GlobalLock(mem);
                    if !dst.is_null() {
                        std::ptr::copy_nonoverlapping(bytes.as_ptr(), dst as *mut u8, bytes.len());
                        GlobalUnlock(mem);
                        if SetClipboardData(fmt, mem).is_null() {
                            GlobalFree(mem);
                        }
                    } else {
                        GlobalFree(mem);
                    }
                }
            }
        }

        CloseClipboard();

        // 记录回声指纹（文本+HTML），供监听线程 10s 内跳过自身写入
        if let Ok(mut guard) = LAST_SELF_SET.lock() {
            *guard = Some((content_hash(text, html), std::time::Instant::now()));
        }
        Ok(())
    }
}

/// 把 CF_DIB 数据（BITMAPINFOHEADER + 调色板 + 像素，无文件头）包装成 BMP 文件字节。
/// 只补 14 字节 BITMAPFILEHEADER，不做像素重编码。
#[cfg(target_os = "windows")]
fn dib_to_bmp(dib: &[u8]) -> Vec<u8> {
    if dib.len() < 40 {
        return dib.to_vec();
    }
    let bi_size = u32::from_le_bytes([dib[0], dib[1], dib[2], dib[3]]) as usize;
    let bit_count = u16::from_le_bytes([dib[14], dib[15]]) as usize;
    let clr_used = if dib.len() >= 36 {
        u32::from_le_bytes([dib[32], dib[33], dib[34], dib[35]]) as usize
    } else {
        0
    };
    let pal_entries = if clr_used != 0 {
        clr_used
    } else if bit_count <= 8 {
        1usize << bit_count
    } else {
        0
    };
    let off_bits = 14 + bi_size + pal_entries * 4;
    let file_size = 14 + dib.len();
    let mut out = Vec::with_capacity(file_size);
    out.extend_from_slice(b"BM");
    out.extend_from_slice(&(file_size as u32).to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes()); // reserved
    out.extend_from_slice(&(off_bits as u32).to_le_bytes());
    out.extend_from_slice(dib);
    out
}

/// 把 BMP 文件字节剥掉 14 字节文件头，得到 CF_DIB 数据（写回剪贴板用）
#[cfg(target_os = "windows")]
fn bmp_to_dib(bmp: &[u8]) -> &[u8] {
    if bmp.len() > 14 && &bmp[0..2] == b"BM" {
        &bmp[14..]
    } else {
        bmp
    }
}

/// 剪贴板图片快照目录：`数据根/clipboard/images/`
fn clipboard_images_dir() -> Option<std::path::PathBuf> {
    let dir = crate::paths::data_root().join("clipboard").join("images");
    std::fs::create_dir_all(&dir).ok()?;
    Some(dir)
}

/// 图片落盘快照：PNG 原样存 .png，DIB 补文件头存 .bmp。
/// 返回快照绝对路径（失败返回 None，由调用方决定是否降级为文本）。
fn save_image_snapshot(
    bytes: &[u8],
    format: ImageFormat,
    hash: u64,
) -> Option<String> {
    let dir = clipboard_images_dir()?;
    let ext = format.ext();
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let name = format!("{:016x}_{}.{}", hash, nanos, ext);
    let path = dir.join(name);
    let data = match format {
        ImageFormat::Png | ImageFormat::Tiff => bytes.to_vec(),
        #[cfg(target_os = "windows")]
        ImageFormat::Dib => dib_to_bmp(bytes),
        #[cfg(not(target_os = "windows"))]
        ImageFormat::Dib => bytes.to_vec(),
    };
    std::fs::write(&path, &data).ok()?;
    Some(path.to_string_lossy().into_owned())
}

/// 快照字节按目标扩展名转码（png/bmp 两种，格式一致则原样返回）：
/// 截图类应用只往剪贴板写 CF_DIB 位图，快照因此落成 .bmp——导出为 .png 时在
/// 这里重编码。32bpp DIB 的 alpha 字节是 GDI 的未初始化值（通常整图为 0），
/// 直接转码会得到整张全透明的 PNG，故 BMP 源 alpha 全 0 时按不透明处理
/// （PNG 源不动，那是真透明）。
pub fn transcode_image_bytes(bytes: &[u8], dest_ext: &str) -> Result<Vec<u8>, String> {
    // 源格式按**文件头**判（TIFF 两平台是 II*/MM* 两种字节序）
    let src_is_bmp = bytes.len() > 2 && &bytes[0..2] == b"BM";
    let src_is_png = bytes.starts_with(&[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a]);
    let src_is_tiff = bytes.len() > 4
        && (&bytes[0..4] == [0x49, 0x49, 0x2a, 0x00] || &bytes[0..4] == [0x4d, 0x4d, 0x00, 0x2a]);
    let src = if src_is_bmp {
        "bmp"
    } else if src_is_png {
        "png"
    } else if src_is_tiff {
        // macOS 的系统截图 / 预览默认往剪贴板写 public.tiff
        "tiff"
    } else {
        return Err("图片快照不是 BMP/PNG/TIFF 格式".into());
    };
    let dest = dest_ext.trim_start_matches('.').to_ascii_lowercase();
    if dest.is_empty() || src == dest {
        return Ok(bytes.to_vec());
    }
    let img = image::load_from_memory(bytes).map_err(|e| format!("解码图片失败: {}", e))?;
    // 只有 BMP 需要 alpha 兜底：32bpp DIB 的 alpha 字节是 GDI 的未初始化值
    // （通常整图为 0），直接转码会得到整张全透明的 PNG。PNG 源不动（那是真透明），
    // TIFF 源是自描述格式，alpha 本身就是有效值，同样不动。
    let img = if src_is_bmp { normalize_bmp_alpha(img) } else { img };
    let fmt = match dest.as_str() {
        "png" => image::ImageFormat::Png,
        "bmp" => image::ImageFormat::Bmp,
        "tiff" => image::ImageFormat::Tiff,
        other => return Err(format!("不支持的导出格式: {}", other)),
    };
    let mut out = std::io::Cursor::new(Vec::new());
    img.write_to(&mut out, fmt)
        .map_err(|e| format!("编码图片失败: {}", e))?;
    Ok(out.into_inner())
}

/// BMP 解码结果的 alpha 兜底：整图 alpha 全 0 视为不透明，
/// 有任一非 0 则是真 alpha 通道，原样保留
fn normalize_bmp_alpha(img: image::DynamicImage) -> image::DynamicImage {
    let mut rgba = img.to_rgba8();
    if !rgba.pixels().any(|p| p[3] != 0) {
        for p in rgba.pixels_mut() {
            p[3] = 255;
        }
    }
    image::DynamicImage::ImageRgba8(rgba)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 浮层定位夹取：夹的是**工作区**，不是整块屏。
    ///
    /// 这组用例对着用户报的那个症状写：「浮层跳到下方挡住的地方」。
    /// 取一块典型的 1512×982 屏、Dock 吃掉底部 ~80pt 的工作区
    /// （visibleFrame ≈ 0,0,1512,902）作为夹取基准。
    #[cfg(target_os = "macos")]
    mod anchor {
        use super::super::anchor_in_work_area;

        const W: f64 = 520.0;
        const H: f64 = 440.0;
        // 工作区（逻辑点）：Dock 让底部 80pt 不可用
        const AX: f64 = 0.0;
        const AY: f64 = 0.0;
        const AW: f64 = 1512.0;
        const AH: f64 = 902.0;

        #[test]
        fn cursor_middle_centers_and_sits_above_cursor() {
            let (x, y) = anchor_in_work_area(756.0, 450.0, W, H, AX, AY, AW, AH).unwrap();
            assert_eq!(x, 496, "水平应居中于光标");
            assert_eq!(y, 420, "应落在光标上方 30pt");
        }

        /// 用户报的那个症状：光标在屏幕下部时，浮层**不能**被摆到工作区之外。
        /// 改用整屏矩形夹取时这里会算出 542（982-440），下半截正好压在 Dock 底下。
        #[test]
        fn cursor_near_bottom_clamps_inside_work_area() {
            let (_, y) = anchor_in_work_area(756.0, 900.0, W, H, AX, AY, AW, AH).unwrap();
            assert_eq!(y + H as i32, AH as i32, "浮层底边应正好贴工作区底边");
            assert!(y + H as i32 <= AH as i32, "浮层不得超出工作区");
        }

        #[test]
        fn cursor_near_top_clamps_to_work_area_top() {
            let (x, y) = anchor_in_work_area(756.0, 0.0, W, H, AX, AY, AW, AH).unwrap();
            assert_eq!(y, 0, "顶部应夹回工作区上边");
            assert_eq!(x, 496);
        }

        #[test]
        fn cursor_near_right_edge_stays_inside() {
            let (x, _) = anchor_in_work_area(1510.0, 450.0, W, H, AX, AY, AW, AH).unwrap();
            assert_eq!(x + W as i32, AW as i32, "右缘应贴工作区右边");
        }

        /// 工作区比浮层还窄（极小屏 / 显示器枚举异常）时不能 panic：
        /// clamp 的下界大于上界会直接 panic，浮层就会停在上次旧位置或屏幕外。
        #[test]
        fn work_area_smaller_than_panel_does_not_panic() {
            let r = anchor_in_work_area(100.0, 100.0, W, H, 0.0, 0.0, 300.0, 200.0);
            assert!(r.is_some(), "应仍给出结果而不是 None");
        }

        /// 非主屏：工作区原点不是 0 时必须整体平移，不能当成主屏。
        #[test]
        fn second_monitor_nonzero_origin_is_respected() {
            // 第二块屏在主屏右侧：逻辑 (1600, 0, 1512, 902)
            let (x, _) = anchor_in_work_area(2400.0, 450.0, W, H, 1600.0, 0.0, 1512.0, 902.0)
                .unwrap();
            assert!(x >= 1600, "不得跑到第二块屏左边之外");
            assert!(x + W as i32 <= 1600 + 1512, "不得超出第二块屏右缘");
        }
    }

    /// 构造 32bpp BI_RGB 的 BMP 文件字节（自底向上像素序，同 GDI 写剪贴板的形态）
    #[cfg(target_os = "windows")]
    fn bmp32_file(width: u32, height: u32, px: &[[u8; 4]]) -> Vec<u8> {
        let mut dib = Vec::new();
        dib.extend_from_slice(&40u32.to_le_bytes()); // biSize = BITMAPINFOHEADER
        dib.extend_from_slice(&(width as i32).to_le_bytes());
        dib.extend_from_slice(&(height as i32).to_le_bytes());
        dib.extend_from_slice(&1u16.to_le_bytes()); // planes
        dib.extend_from_slice(&32u16.to_le_bytes()); // bitCount
        dib.extend_from_slice(&0u32.to_le_bytes()); // BI_RGB
        dib.extend_from_slice(&(width * height * 4).to_le_bytes()); // sizeImage
        dib.extend_from_slice(&0u32.to_le_bytes()); // xPelsPerMeter
        dib.extend_from_slice(&0u32.to_le_bytes()); // yPelsPerMeter
        dib.extend_from_slice(&0u32.to_le_bytes()); // clrUsed
        dib.extend_from_slice(&0u32.to_le_bytes()); // clrImportant
        for row in (0..height as usize).rev() {
            for col in 0..width as usize {
                let [r, g, b, a] = px[row * width as usize + col];
                // BMP 像素字节序是小端 DWORD 的 B,G,R,A
                dib.extend_from_slice(&[b, g, r, a]);
            }
        }
        dib_to_bmp(&dib)
    }

    /// BMP 是 Windows 剪贴板的产物（DIB 补 14 字节文件头），macOS 读不到
    #[cfg(target_os = "windows")]
    #[test]
    fn transcode_bmp_to_png_preserves_pixels() {
        let px = [
            [255, 0, 0, 255],
            [0, 255, 0, 255],
            [0, 0, 255, 255],
            [255, 255, 255, 255],
        ];
        let bmp = bmp32_file(2, 2, &px);
        let png = transcode_image_bytes(&bmp, ".png").expect("转码应成功");
        assert!(png.starts_with(&[0x89, b'P', b'N', b'G']));
        let decoded = image::load_from_memory(&png).unwrap().to_rgba8();
        // 自底向上的存储经解码还原为自顶向下，逐像素一致
        for (i, e) in px.iter().enumerate() {
            assert_eq!(decoded.get_pixel((i % 2) as u32, (i / 2) as u32).0, *e);
        }
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn transcode_bmp_zero_alpha_becomes_opaque() {
        let px = [[10, 20, 30, 0], [40, 50, 60, 0], [70, 80, 90, 0], [1, 2, 3, 0]];
        let bmp = bmp32_file(2, 2, &px);
        let png = transcode_image_bytes(&bmp, "png").unwrap();
        let decoded = image::load_from_memory(&png).unwrap().to_rgba8();
        for p in decoded.pixels() {
            assert_eq!(p[3], 255, "alpha 全 0 的 DIB 应按不透明导出");
        }
        assert_eq!(decoded.get_pixel(0, 0).0, [10, 20, 30, 255]);
    }

    #[test]
    fn transcode_same_format_passthrough() {
        let mut png = std::io::Cursor::new(Vec::new());
        image::DynamicImage::new_rgb8(1, 1)
            .write_to(&mut png, image::ImageFormat::Png)
            .unwrap();
        let bytes = png.into_inner();
        assert_eq!(transcode_image_bytes(&bytes, ".png").unwrap(), bytes);
    }

    /// TIFF 源（macOS 系统截图的原生剪贴板格式）转 PNG 必须保住像素。
    /// 这条用例挡住的是「加了 tiff 支持但没开 image crate 的 tiff feature」
    /// 与「TIFF 字节序判错」两类翻车——两者都只在 mac 上显形。
    #[test]
    fn transcode_tiff_to_png_preserves_pixels() {
        let src = image::DynamicImage::new_rgba8(2, 1);
        let mut tiff = std::io::Cursor::new(Vec::new());
        src.write_to(&mut tiff, image::ImageFormat::Tiff).unwrap();
        let tiff = tiff.into_inner();
        assert!(
            &tiff[0..4] == [0x49, 0x49, 0x2a, 0x00] || &tiff[0..4] == [0x4d, 0x4d, 0x00, 0x2a],
            "TIFF 文件头应为 II* 或 MM*，实际 {:02x?}",
            &tiff[0..4]
        );
        let png = transcode_image_bytes(&tiff, "png").expect("TIFF→PNG 转码应成功");
        assert!(png.starts_with(&[0x89, b'P', b'N', b'G']));
        let decoded = image::load_from_memory(&png).unwrap();
        assert_eq!(decoded.width(), 2);
        assert_eq!(decoded.height(), 1);
    }

    /// 未知格式必须**明确报错**，不能静默原样返回——
    /// 原样返回会让用户以为「保存成功了」，打开却是一堆乱码。
    #[test]
    fn transcode_rejects_unknown_source_format() {
        assert!(transcode_image_bytes(b"not an image at all", "png").is_err());
    }
}

/// 分配 Global 内存并写入字节、SetClipboardData（调用方需已 OpenClipboard 且 EmptyClipboard）
#[cfg(target_os = "windows")]
unsafe fn set_global_data(format: u32, data: &[u8]) -> Result<(), String> {
    let mem = GlobalAlloc(GMEM_MOVEABLE | GMEM_ZEROINIT, data.len());
    if mem.is_null() {
        return Err("剪贴板内存分配失败".into());
    }
    let dst = GlobalLock(mem);
    if dst.is_null() {
        GlobalFree(mem);
        return Err("剪贴板内存锁定失败".into());
    }
    std::ptr::copy_nonoverlapping(data.as_ptr(), dst as *mut u8, data.len());
    GlobalUnlock(mem);
    if SetClipboardData(format, mem).is_null() {
        GlobalFree(mem);
        return Err("写入剪贴板失败".into());
    }
    Ok(())
}

/// 写入图片到剪贴板：BMP 快照写 CF_DIB，PNG 快照写 PNG 剪贴板格式。
/// 写回后记录回声指纹，抑制监听线程重复入库。
#[cfg(target_os = "windows")]
pub fn set_clipboard_image(path: &str) -> Result<(), String> {
    let bytes = std::fs::read(path).map_err(|e| format!("读取图片快照失败: {}", e))?;
    let is_bmp = bytes.len() > 2 && &bytes[0..2] == b"BM";
    let data: Vec<u8> = if is_bmp {
        bmp_to_dib(&bytes).to_vec()
    } else {
        bytes.clone()
    };
    // 回声指纹基于「写回剪贴板的实际字节」（PNG 原始字节 / 剥掉文件头的 DIB），
    // 与监听端读到的一致，确保粘贴后能被正确识别为自身写入回声。
    let hash = hash_bytes(&data);
    unsafe {
        if OpenClipboard(std::ptr::null_mut()) == 0 {
            return Err("无法打开系统剪贴板".into());
        }
        if EmptyClipboard() == 0 {
            CloseClipboard();
            return Err("无法清空系统剪贴板".into());
        }
        let result = if is_bmp {
            set_global_data(CF_DIB as u32, &data)
        } else {
            let fmt = RegisterClipboardFormatW(windows_sys::core::w!("PNG"));
            if fmt == 0 {
                Err("无法注册 PNG 格式".into())
            } else {
                set_global_data(fmt, &data)
            }
        };
        CloseClipboard();
        result?;
    }
    record_self_set_hash(hash);
    Ok(())
}

/// 构造 CF_HDROP 数据：DROPFILES 结构 + 宽字符路径表（每条以 null 结尾，末尾额外 null）
#[cfg(target_os = "windows")]
fn build_hdrop(paths: &[&str]) -> Vec<u8> {
    let header_size = std::mem::size_of::<DROPFILES>();
    let mut payload: Vec<u16> = Vec::new();
    for p in paths {
        payload.extend(p.encode_utf16());
        payload.push(0);
    }
    payload.push(0); // 列表结束标记

    let total = header_size + payload.len() * 2;
    let mut out = vec![0u8; total];
    let df = DROPFILES {
        pFiles: header_size as u32,
        pt: POINT { x: 0, y: 0 },
        fNC: 0,
        fWide: 1,
    };
    let df_bytes: &[u8] = unsafe {
        std::slice::from_raw_parts(&df as *const DROPFILES as *const u8, header_size)
    };
    out[..header_size].copy_from_slice(df_bytes);
    for (i, u) in payload.iter().enumerate() {
        let b = u.to_le_bytes();
        out[header_size + i * 2..header_size + i * 2 + 2].copy_from_slice(&b);
    }
    out
}

/// 写入文件列表到剪贴板（CF_HDROP）：只引用原路径，不拷贝文件内容。
/// 已不存在的路径会被剔除；全部失效则报错提示用户。
#[cfg(target_os = "windows")]
pub fn set_clipboard_files(paths: &[String]) -> Result<(), String> {
    let existing: Vec<&str> = paths
        .iter()
        .map(|s| s.as_str())
        .filter(|p| std::path::Path::new(p).exists())
        .collect();
    if existing.is_empty() {
        return Err("文件已不存在，无法粘贴".into());
    }
    let owned: Vec<String> = existing.iter().map(|s| s.to_string()).collect();
    let hash = hash_files(&owned);
    let data = build_hdrop(&existing);
    unsafe {
        if OpenClipboard(std::ptr::null_mut()) == 0 {
            return Err("无法打开系统剪贴板".into());
        }
        if EmptyClipboard() == 0 {
            CloseClipboard();
            return Err("无法清空系统剪贴板".into());
        }
        let result = set_global_data(CF_HDROP as u32, &data);
        CloseClipboard();
        result?;
    }
    record_self_set_hash(hash);
    Ok(())
}

// ---- 粘贴注入（恢复焦点 + 发送粘贴快捷键） ----

/// 发送一次按键（基于扫描码，比虚拟键码更兼容各类应用/终端）
#[cfg(target_os = "windows")]
unsafe fn send_key(scan: u16, flags: u32) {
    let mut input: INPUT = std::mem::zeroed();
    input.r#type = INPUT_KEYBOARD;
    input.Anonymous.ki = KEYBDINPUT {
        wVk: 0,
        wScan: scan,
        dwFlags: flags,
        time: 0,
        dwExtraInfo: 0,
    };
    SendInput(1, &input, std::mem::size_of::<INPUT>() as i32);
}

/// 发送粘贴快捷键，method 支持：
/// - ctrl_v：Ctrl+V（常规应用）
/// - ctrl_shift_v：Ctrl+Shift+V（部分终端/命令行只支持无格式粘贴）
/// - shift_insert：Shift+Insert（终端通用粘贴）
#[cfg(target_os = "windows")]
fn send_paste_keystroke(method: &str) {
    unsafe {
        let ctrl = MapVirtualKeyW(VK_CONTROL as u32, MAPVK_VK_TO_VSC) as u16;
        let shift = MapVirtualKeyW(VK_SHIFT as u32, MAPVK_VK_TO_VSC) as u16;
        let v = MapVirtualKeyW('V' as u32, MAPVK_VK_TO_VSC) as u16;
        let insert = MapVirtualKeyW(VK_INSERT as u32, MAPVK_VK_TO_VSC) as u16;
        match method {
            "ctrl_shift_v" => {
                send_key(ctrl, KEYEVENTF_SCANCODE);
                send_key(shift, KEYEVENTF_SCANCODE);
                send_key(v, KEYEVENTF_SCANCODE);
                send_key(v, KEYEVENTF_SCANCODE | KEYEVENTF_KEYUP);
                send_key(shift, KEYEVENTF_SCANCODE | KEYEVENTF_KEYUP);
                send_key(ctrl, KEYEVENTF_SCANCODE | KEYEVENTF_KEYUP);
            }
            "shift_insert" => {
                send_key(shift, KEYEVENTF_SCANCODE);
                send_key(insert, KEYEVENTF_SCANCODE | KEYEVENTF_EXTENDEDKEY);
                send_key(insert, KEYEVENTF_SCANCODE | KEYEVENTF_EXTENDEDKEY | KEYEVENTF_KEYUP);
                send_key(shift, KEYEVENTF_SCANCODE | KEYEVENTF_KEYUP);
            }
            _ => {
                // ctrl_v
                send_key(ctrl, KEYEVENTF_SCANCODE);
                send_key(v, KEYEVENTF_SCANCODE);
                send_key(v, KEYEVENTF_SCANCODE | KEYEVENTF_KEYUP);
                send_key(ctrl, KEYEVENTF_SCANCODE | KEYEVENTF_KEYUP);
            }
        }
    }
}

/// 释放所有修饰键（Ctrl/Shift/Alt/Win 的左右键），
/// 避免用户还按着 Shift 时 Ctrl+V 变成 Ctrl+Shift+V 无格式粘贴。
#[cfg(target_os = "windows")]
fn release_modifier_keys() {
    unsafe {
        let keys = [
            VK_SHIFT, VK_LSHIFT, VK_RSHIFT, VK_CONTROL, VK_LCONTROL, VK_RCONTROL, VK_MENU,
            VK_LMENU, VK_RMENU, VK_LWIN, VK_RWIN,
        ];
        for k in keys {
            keybd_event(k as u8, 0, KEYEVENTF_KEYUP, 0);
        }
    }
}

/// 强行把目标窗口带到前台并聚焦（AttachThreadInput 绕开前台锁定 + 还原最小化窗口）。
#[cfg(target_os = "windows")]
fn force_focus_window(hwnd: *mut core::ffi::c_void) {
    unsafe {
        if hwnd.is_null() || IsWindowVisible(hwnd) == 0 {
            return;
        }
        let should_restore = IsIconic(hwnd) != 0;
        let fg = GetForegroundWindow();
        if fg as isize != hwnd as isize {
            let mut fg_pid: u32 = 0;
            let mut target_pid: u32 = 0;
            let fg_tid = GetWindowThreadProcessId(fg, &mut fg_pid);
            let target_tid = GetWindowThreadProcessId(hwnd, &mut target_pid);
            if fg_tid != 0 && target_tid != 0 && fg_tid != target_tid {
                AttachThreadInput(fg_tid, target_tid, 1);
                SetForegroundWindow(hwnd);
                if should_restore {
                    ShowWindow(hwnd, SW_RESTORE);
                }
                BringWindowToTop(hwnd);
                AttachThreadInput(fg_tid, target_tid, 0);
            } else {
                SetForegroundWindow(hwnd);
                if should_restore {
                    ShowWindow(hwnd, SW_RESTORE);
                }
                BringWindowToTop(hwnd);
            }
        }
    }
}

/// 前台进程名（给定窗口句柄），用于识别终端等特殊粘贴目标
#[cfg(target_os = "windows")]
fn process_name_of_hwnd(hwnd: isize) -> Option<String> {
    let mut pid: u32 = 0;
    unsafe { GetWindowThreadProcessId(hwnd as *mut core::ffi::c_void, &mut pid) };
    if pid == 0 {
        return None;
    }
    process_name_of_pid(pid)
}

/// 已知终端/命令行窗口进程名（这类窗口通常不支持 Ctrl+V，需要 Ctrl+Shift+V 或 Shift+Insert）
#[cfg(target_os = "windows")]
const TERMINAL_NAMES: [&str; 24] = [
    "cmd.exe",
    "powershell.exe",
    "pwsh.exe",
    "conhost.exe",
    "openconsole.exe",
    "windowsterminal.exe",
    "wt.exe",
    "wezterm.exe",
    "alacritty.exe",
    "kitty.exe",
    "mintty.exe",
    "hyper.exe",
    "conemu64.exe",
    "cmder.exe",
    "tabby.exe",
    "warp.exe",
    "ghostty.exe",
    "mobaxterm.exe",
    "tabs.exe",
    "terminus.exe",
    "fluentterminal.exe",
    "ssh.exe",
    "sshd.exe",
    "windows terminal",
];

#[cfg(target_os = "windows")]
fn is_terminal_name(name: &str) -> bool {
    TERMINAL_NAMES
        .iter()
        .any(|t| name.eq_ignore_ascii_case(t))
}

/// 解析最终粘贴方式：auto 时按目标窗口是否终端自动选择 Ctrl+Shift+V / Ctrl+V
#[cfg(target_os = "windows")]
fn resolve_paste_method(cfg: &str, target_hwnd: isize) -> &'static str {
    match cfg {
        "ctrl_shift_v" => "ctrl_shift_v",
        "shift_insert" => "shift_insert",
        "ctrl_v" => "ctrl_v",
        _ => {
            let name = process_name_of_hwnd(target_hwnd).unwrap_or_default();
            if is_terminal_name(&name) {
                "ctrl_shift_v"
            } else {
                "ctrl_v"
            }
        }
    }
}

/// 判断唤起浮层前聚焦的窗口是否为本应用主窗口：
/// 主窗口输入框在 WebView2 内，浮层抢焦点会让其失去 DOM 焦点，
/// 恢复焦点 + 注入 Ctrl+V 依赖窗口激活/焦点时序，实际不可靠，
/// 改为直接向主窗口 JS 派发内容、由其插回原输入框。
#[cfg(target_os = "windows")]
fn is_main_window(app: &AppHandle, prev_focus: &PrevFocus) -> bool {
    crate::main_window(app)
        .and_then(|w| w.hwnd().ok())
        .map(|h| h.0 as isize == *prev_focus)
        .unwrap_or(false)
}

/// macOS 侧等价判定：唤起前的前台**应用**就是本应用（bundle id 相同）。
/// 粒度比 Windows 的 HWND 粗——用户若同时开着本应用的两个窗口，判定会认为
/// 「就是主窗口」，于是内容直接派发给主窗 JS 而不注入按键。对剪贴板这个场景
/// 可以接受：派发路径本身会再由主窗按焦点元素插入，比模拟按键更可靠。
#[cfg(target_os = "macos")]
fn is_main_window(_app: &AppHandle, prev_focus: &PrevFocus) -> bool {
    prev_focus == SELF_BUNDLE_ID
}

/// 粘贴到唤起浮层前聚焦的窗口：
/// - 本应用主窗口：隐藏浮层后直接向主窗口 JS 派发内容，由其插回原输入框（WebView2 焦点时序不可靠）
/// - 外部窗口：隐藏浮层 →（若前台被我们抢占则恢复目标窗口焦点）→ 按目标应用发送粘贴快捷键
pub fn paste_to_previous_window(app: &AppHandle, content: &str, html: Option<&str>) {
    #[cfg(target_os = "windows")]
    {
        unregister_esc_hotkey();
        uninstall_mouse_hook();
    }
    #[cfg(target_os = "macos")]
    {
        crate::floating_ball::untrack_clipboard_overlay();
        // 这条路径不走 hide_overlay，必须自己停 —— 漏了就是「粘贴一次后 Esc 永久失灵」
        stop_esc_watch();
    }
    if let Some(win) = app.get_webview_window(CLIPBOARD_WINDOW_LABEL) {
        hide_overlay_window(&win);
    }
    // `Option<PrevFocus>` 在 macOS 上是 `Option<String>`（非 Copy），
    // 必须 clone 出锁内值，不能 `*g` 直接 move
    let prev = app
        .state::<ClipboardState>()
        .prev_focus
        .lock()
        .ok()
        .and_then(|g| g.clone());
    let Some(prev_focus) = prev else {
        log::warn!("剪贴板粘贴：未记录到唤起前窗口，仅写入剪贴板");
        return;
    };
    log::info!("剪贴板粘贴：唤起前焦点 {:?}", prev_focus);

    // 本应用主窗口 + 文本内容：直接派发内容给主窗口 JS 插入（浮层已隐藏，无需恢复焦点）。
    // 图片/文件（content 为空）不在此分支——走下方 Ctrl+V 注入，目标应用读剪贴板对应格式。
    if !content.is_empty() && is_main_window(app, &prev_focus) {
        let payload = serde_json::json!({
            "content": content,
            "html": html,
        });
        if let Err(e) = app.emit_to("main", "clipboard-paste-request", payload) {
            log::warn!("剪贴板粘贴：向主窗口派发插入请求失败: {}", e);
        }
        return;
    }

    #[cfg(target_os = "windows")]
    {
        // 提交到常驻 worker 串行执行（粘贴时序依赖窗口隐藏/焦点转移，延迟需按序进行）
        let paste_method =
            resolve_paste_method(&crate::config::load().clipboard_paste_method, prev_focus);
        submit_win_op(DelayedWinOp::Paste {
            hwnd: prev_focus,
            paste_method,
        });
    }
    #[cfg(target_os = "macos")]
    {
        paste_macos(app, &prev_focus);
    }
}

// ---- 浮层窗口 ----

/// 剪贴板监听消息窗口句柄（Esc 兜底热键注册到该窗口，由监听线程消息循环分发 WM_HOTKEY）
#[cfg(target_os = "windows")]
static LISTENER_HWND: Mutex<Option<isize>> = Mutex::new(None);
/// Esc 兜底关闭热键 ID
#[cfg(target_os = "windows")]
const ESC_HOTKEY_ID: i32 = 0x5C43;

/// 低层鼠标钩子回调：浮层可见时，点击落在浮层窗口矩形之外 → 收起浮层。
/// 浮层以无激活方式显示、不持有焦点，靠此钩子感知「点击外部」（含点击本应用主窗口）。
/// 钩子回调运行在安装钩子的线程上，这里只做矩形判断，随后向监听消息窗口
/// PostMessage 一个自定义消息，由监听线程统一收口处理（避免在钩子回调里直接做窗口操作）。
///
/// macOS 没有等价机制：那边借 `floating_ball` 的边缘监视循环做同样的矩形判定
/// （见 `crate::floating_ball::tick_clipboard_overlay`）。
#[cfg(target_os = "windows")]
const WM_XHUB_CLIPBOARD_DISMISS: u32 = WM_APP + 0x5C01;
/// 监听线程专用消息：安装/卸载低层鼠标钩子（钩子必须由拥有消息循环的线程安装）
#[cfg(target_os = "windows")]
const WM_XHUB_INSTALL_MOUSE_HOOK: u32 = WM_APP + 0x5C02;
#[cfg(target_os = "windows")]
const WM_XHUB_UNINSTALL_MOUSE_HOOK: u32 = WM_APP + 0x5C03;

/// 低层鼠标钩子回调：浮层可见时，点击落在浮层窗口矩形之外 → 收起浮层。
/// 回调运行在「安装钩子的线程」上，这里只做纯 Win32 矩形判断（禁止任何 Tauri 调用，
/// 否则会经 send_user_message 阻塞并拖垮该线程消息循环），随后向监听消息窗口
/// PostMessage 一个自定义消息，由监听线程统一收口处理。
#[cfg(target_os = "windows")]
unsafe extern "system" fn mouse_hook_proc(code: i32, wparam: usize, lparam: isize) -> isize {
    if code >= 0 {
        let msg = wparam as u32;
        if msg == WM_LBUTTONDOWN || msg == WM_RBUTTONDOWN || msg == WM_MBUTTONDOWN {
            let clicked_outside = {
                let Some(hwnd) = *OVERLAY_HWND.lock().unwrap_or_else(|e| e.into_inner()) else {
                    return CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam);
                };
                if IsWindowVisible(hwnd as *mut core::ffi::c_void) == 0 {
                    return CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam);
                }
                let st = lparam as *const MSLLHOOKSTRUCT;
                let pt = (*st).pt;
                let mut rc: RECT = std::mem::zeroed();
                GetWindowRect(hwnd as *mut core::ffi::c_void, &mut rc);
                // 5px 边距兜底，贴近边缘的点击不算「外部」
                pt.x < rc.left - 5 || pt.x > rc.right + 5 || pt.y < rc.top - 5 || pt.y > rc.bottom + 5
            };
            if clicked_outside {
                if let Some(hwnd) = *LISTENER_HWND.lock().unwrap_or_else(|e| e.into_inner()) {
                    let _ = PostMessageW(
                        hwnd as *mut core::ffi::c_void,
                        WM_XHUB_CLIPBOARD_DISMISS,
                        0,
                        0,
                    );
                }
            }
        }
    }
    CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam)
}

/// 请求监听线程安装低层鼠标钩子（浮层可见时启用，隐藏时关闭）。
/// 钩子必须由监听线程（拥有 GetMessageW 消息循环）安装，才能由该线程驱动回调，
/// 避免挂在全局快捷键线程上导致回调阻塞拖死 Esc 链路。
#[cfg(target_os = "windows")]
fn install_mouse_hook() {
    let hwnd = match *LISTENER_HWND.lock().unwrap_or_else(|e| e.into_inner()) {
        Some(h) => h,
        None => return,
    };
    unsafe {
        PostMessageW(hwnd as *mut core::ffi::c_void, WM_XHUB_INSTALL_MOUSE_HOOK, 0, 0);
    }
}

/// 请求监听线程卸载低层鼠标钩子（浮层隐藏时调用）
#[cfg(target_os = "windows")]
fn uninstall_mouse_hook() {
    let hwnd = match *LISTENER_HWND.lock().unwrap_or_else(|e| e.into_inner()) {
        Some(h) => h,
        None => return,
    };
    unsafe {
        PostMessageW(hwnd as *mut core::ffi::c_void, WM_XHUB_UNINSTALL_MOUSE_HOOK, 0, 0);
    }
}

/// 监听线程实际安装低层鼠标钩子（由消息循环调用，保证钩子挂在本线程）
#[cfg(target_os = "windows")]
fn install_mouse_hook_impl() {
    let Ok(mut guard) = MOUSE_HOOK.lock() else {
        return;
    };
    if guard.is_some() {
        return;
    }
    let hmod = unsafe { GetModuleHandleW(std::ptr::null()) };
    let hook = unsafe { SetWindowsHookExW(WH_MOUSE_LL, Some(mouse_hook_proc), hmod, 0) };
    if hook.is_null() {
        log::warn!("剪贴板浮层：注册低层鼠标钩子失败");
        return;
    }
    *guard = Some(hook as isize);
    log::info!("剪贴板浮层：低层鼠标钩子已启用（监听线程）");
}

/// 监听线程实际卸载低层鼠标钩子（由消息循环调用）
#[cfg(target_os = "windows")]
fn uninstall_mouse_hook_impl() {
    let Ok(mut guard) = MOUSE_HOOK.lock() else {
        return;
    };
    if let Some(hook) = guard.take() {
        unsafe {
            UnhookWindowsHookEx(hook as *mut core::ffi::c_void);
        }
    }
}

/// 注册「浮层可见期间的 Esc 兜底关闭」全局热键：
/// 浮层以无激活方式显示时自身收不到键盘事件，靠 RegisterHotKey 拦截 Esc，
/// 由剪贴板监听线程把 WM_HOTKEY 转成关闭动作（隐藏后自动注销，避免吞掉全局 Esc）。
#[cfg(target_os = "windows")]
fn register_esc_hotkey() {
    let hwnd = match *LISTENER_HWND.lock().unwrap_or_else(|e| e.into_inner()) {
        Some(h) => h,
        None => return,
    };
    let ok = unsafe {
        RegisterHotKey(hwnd as *mut core::ffi::c_void, ESC_HOTKEY_ID, 0, VK_ESCAPE as u32) != 0
    };
    if !ok {
        log::warn!("剪贴板浮层：注册 Esc 兜底热键失败（可能被其他程序占用）");
    }
}

/// 注销 Esc 兜底热键（浮层隐藏时调用）
#[cfg(target_os = "windows")]
fn unregister_esc_hotkey() {
    let hwnd = match *LISTENER_HWND.lock().unwrap_or_else(|e| e.into_inner()) {
        Some(h) => h,
        None => return,
    };
    unsafe {
        UnregisterHotKey(hwnd as *mut core::ffi::c_void, ESC_HOTKEY_ID);
    }
}

/// 浮层窗口原生可见性：`WebviewWindow::is_visible()` 在 Windows 上对 WebView2 子窗口
/// 返回不准（隐藏后仍可能报 true，见 tray.rs 顶部同款注释），会让 toggle_overlay 误入
/// 「收起」分支——表现为从悬浮球唤起剪贴板毫无反应。直查 Win32 IsWindowVisible
/// （对自身的 ShowWindow(SW_SHOW/HIDE) 判定准确）。
#[cfg(target_os = "windows")]
fn overlay_native_visible(win: &tauri::WebviewWindow) -> bool {
    match win.hwnd() {
        Ok(hwnd) => unsafe { IsWindowVisible(hwnd.0) != 0 },
        Err(_) => win.is_visible().unwrap_or(false),
    }
}

#[cfg(not(target_os = "windows"))]
fn overlay_native_visible(win: &tauri::WebviewWindow) -> bool {
    win.is_visible().unwrap_or(false)
}

/// 唤起/收起剪贴板浮层（全局快捷键 / 悬浮球菜单触发）：
/// 唤起前记录当前前台窗口（粘贴还原目标），浮层为独立置顶小窗。
/// 浮层以「无激活」方式显示（不抢走当前输入框焦点），用户点击搜索框时才激活。
///
/// 窗口生命周期：启动时由 init_overlay_window 预创建并隐藏常驻——运行时现场
/// 创建 WebView2 窗口是主线程长任务，曾与悬浮球菜单收拢等并发窗口操作交错导致
/// 整窗未响应。唤起/收起只做 ShowWindow 级快操作，绝不现场 build。
pub fn toggle_overlay(app: &AppHandle) {
    // 已存在且可见：收起走统一 hide_overlay（注销热键/钩子 + 归还唤起前窗口焦点）。
    // 不要在此覆盖 prev_focus——浮层被激活后前台是浮层自身，覆盖会把还原目标错写成浮层。
    if let Some(win) = app.get_webview_window(CLIPBOARD_WINDOW_LABEL) {
        if overlay_native_visible(&win) {
            log::info!("剪贴板浮层：toggle 时已可见，执行收起");
            hide_overlay(app);
            return;
        }
    }
    log::info!("剪贴板浮层：toggle 唤起");

    // 显示：记录唤起前的前台焦点，供关闭/粘贴时还原
    let prev: Option<PrevFocus> = current_focus();
    if let Ok(mut guard) = app.state::<ClipboardState>().prev_focus.lock() {
        *guard = prev;
    }

    let Some(win) = app.get_webview_window(CLIPBOARD_WINDOW_LABEL) else {
        // 兜底：窗口不在（预创建失败或被销毁）时现场补建并显示。正常路径由
        // init_overlay_window 启动预创建，这里极少走到
        if let Ok(win) = build_overlay_window(app) {
            log::info!("剪贴板浮层窗口已补建");
            show_ready_overlay(&win, app);
        }
        return;
    };
    show_ready_overlay(&win, app);
}

/// 构建剪贴板浮层窗口（visible=false，显示由调用方决定）
fn build_overlay_window(app: &AppHandle) -> tauri::Result<tauri::WebviewWindow> {
    let builder = tauri::WebviewWindowBuilder::new(
        app,
        CLIPBOARD_WINDOW_LABEL,
        WebviewUrl::App("index.html".into()),
    )
    .title("剪贴板历史")
    .inner_size(CLIPBOARD_WIDTH, CLIPBOARD_HEIGHT)
    .resizable(false)
    .decorations(false)
    .transparent(true)
    .always_on_top(true)
    .skip_taskbar(true)
    .visible(false)
    .background_color(tauri::window::Color(0, 0, 0, 0))
    .additional_browser_args(crate::ADDITIONAL_BROWSER_ARGS);

    // 透明窗口在 Windows 上启用系统阴影会把边缘渲染成黑色描边（黑边），
    // 面板自带 CSS 阴影，OS 层阴影关闭即可（与便签浮窗一致）
    #[cfg(target_os = "windows")]
    {
        builder = builder.shadow(false);
    }
    builder.build()
}

/// 启动时预创建剪贴板浮层窗口（隐藏常驻）：
/// 运行时现场创建 WebView2 窗口曾在点击悬浮球「剪贴板」时与菜单收拢的窗口
/// 操作在主线程事件循环上交错，导致整窗未响应（WebView2 controller 创建挂起、
/// 嵌套消息循环重入）。改为启动阶段（无并发窗口操作）一次性建好，此后唤起
/// /收起均为 ShowWindow 级快操作。代价：renderer 常驻内存（约 60–120MB）。
pub fn init_overlay_window(app: &AppHandle) {
    if app.get_webview_window(CLIPBOARD_WINDOW_LABEL).is_some() {
        return;
    }
    match build_overlay_window(app) {
        Ok(_) => log::info!("剪贴板浮层窗口已预创建（隐藏常驻）"),
        Err(e) => log::warn!("剪贴板浮层窗口预创建失败: {}", e),
    }
}

/// 显示就绪的浮层：定位到鼠标附近 + 无激活显示 + 热键/钩子 + 通知页面刷新
fn show_ready_overlay(win: &tauri::WebviewWindow, app: &AppHandle) {
    // 先恢复内存级别再显示（webview_mem：Low 态缓存已吐，首帧前回 Normal）
    crate::webview_mem::on_shown(app, CLIPBOARD_WINDOW_LABEL);
    // 每次唤起都重新定位到鼠标附近（窗口可能被拖走过、或显示器布局变化）
    if let Some((px, py)) = cursor_anchor_position(win) {
        let _ = win.set_position(anchor_position(px, py));
    }
    show_overlay_no_activate(win);
    #[cfg(target_os = "windows")]
    {
        register_esc_hotkey();
        install_mouse_hook();
    }
    #[cfg(target_os = "macos")]
    {
        // macOS 没有全局鼠标钩子：登记到 floating_ball 的 100ms 光标轮询里做
        // 矩形判定（点浮层之外即收起）。见模块文档「没有全局鼠标钩子」。
        crate::floating_ball::track_clipboard_overlay();
        // 无激活显示的代价是浮层**收不到键盘事件**，Esc 收不到。
        // 用 CGEventTap 兜住（等价 Windows 的 register_esc_hotkey）。
        let app_for_esc = app.clone();
        match crate::mac::start_esc_watch(move || {
            hide_overlay(&app_for_esc);
        }) {
            Ok(watch) => {
                // 同样必须出锁再 drop 旧的（见 stop_esc_watch 的死锁说明）
                let replaced = {
                    let mut slot = ESC_WATCH.lock().unwrap_or_else(|e| e.into_inner());
                    slot.replace(watch)
                };
                drop(replaced);
            }
            // 失败不阻断：浮层照常显示，只是没有 Esc 兜底
            Err(()) => log::warn!("剪贴板浮层：Esc 兜底不可用，只能点浮层外部关闭"),
        }
    }
    let _ = app.emit_to(CLIPBOARD_WINDOW_LABEL, "clipboard-shown", ());
}

/// 无激活显示浮层：附加 WS_EX_NOACTIVATE 后以 SW_SHOWNA 显示并置顶，
/// 不抢占前台焦点（原输入框保持焦点，外部应用可直接粘贴）。
#[cfg(target_os = "windows")]
fn show_overlay_no_activate(win: &tauri::WebviewWindow) {
    {
        if let Ok(hwnd) = win.hwnd() {
            if let Ok(mut guard) = OVERLAY_HWND.lock() {
                *guard = Some(hwnd.0 as isize);
            }
            unsafe {
                let ex = GetWindowLongPtrW(hwnd.0, GWL_EXSTYLE);
                SetWindowLongPtrW(hwnd.0, GWL_EXSTYLE, ex | WS_EX_NOACTIVATE as isize);
                ShowWindow(hwnd.0, SW_SHOWNA);
                SetWindowPos(
                    hwnd.0,
                    HWND_TOPMOST,
                    0,
                    0,
                    0,
                    0,
                    SWP_NOMOVE | SWP_NOSIZE | SWP_SHOWWINDOW | SWP_NOACTIVATE,
                );
            }
            // 原生 SW_SHOWNA 显示不会重建 ex-style，但仍可能带着 WS_EX_APPWINDOW
            // （预创建时的形态）→ 统一摘掉任务栏按钮（见 win_taskbar 模块注释）
            crate::win_taskbar::apply(win);
            return;
        }
        crate::win_taskbar::show(win);
    }
    #[cfg(not(target_os = "windows"))]
    crate::win_taskbar::show(win);
}

/// 激活浮层：清除 WS_EX_NOACTIVATE 并强制前台（用户点击搜索框开始键盘操作时调用）
pub fn activate_overlay(app: &AppHandle) {
    if let Some(win) = app.get_webview_window(CLIPBOARD_WINDOW_LABEL) {
        #[cfg(target_os = "windows")]
        if let Ok(hwnd) = win.hwnd() {
            // 先摘掉 WS_EX_NOACTIVATE：带这个位的窗口**收不到**键盘焦点
            unsafe {
                let ex = GetWindowLongPtrW(hwnd.0, GWL_EXSTYLE);
                SetWindowLongPtrW(hwnd.0, GWL_EXSTYLE, ex & !(WS_EX_NOACTIVATE as isize));
            }
            let _ = win.set_focus();
            force_focus_window(hwnd.0);
            return;
        }
        #[cfg(not(target_os = "windows"))]
        let _ = win.set_focus();
    }
}

/// 收起浮层：仅隐藏（窗口由 init_overlay_window 预创建后常驻，不做销毁回收）。
/// 销毁会迫使下次唤起现场重建 WebView2 窗口——该路径曾与悬浮球操作交错导致
/// 整窗未响应；renderer 常驻内存是换取唤起零等待 + 无运行时建窗的代价。
fn hide_overlay_window(win: &tauri::WebviewWindow) {
    // 先清掉浮层窗口句柄缓存，避免低层鼠标钩子残留句柄对已隐藏窗口做矩形判断
    #[cfg(target_os = "windows")]
    if let Ok(mut guard) = OVERLAY_HWND.lock() {
        *guard = None;
    }
    // 隐藏后把常驻 renderer 的内存目标级别降到 Low（webview_mem，轮询兜底）
    crate::webview_mem::on_hidden(win.app_handle(), win.label());
    #[cfg(target_os = "windows")]
    {
        if let Ok(hwnd) = win.hwnd() {
            unsafe {
                ShowWindow(hwnd.0, SW_HIDE);
            }
            return;
        }
        let _ = win.hide();
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = win.hide();
    }
}

/// 收起浮层：隐藏后若前台仍在本应用（浮层/主窗口），把焦点还给唤起前的窗口
pub fn hide_overlay(app: &AppHandle) {
    #[cfg(target_os = "windows")]
    {
        unregister_esc_hotkey();
        uninstall_mouse_hook();
    }
    #[cfg(target_os = "macos")]
    {
        crate::floating_ball::untrack_clipboard_overlay();
        // Esc 回调本身也走这里，而它正跑在 tap 线程上 —— EscWatch 的 Drop
        // 对「join 自己」做了处理，故这条路径安全。
        stop_esc_watch();
    }
    let Some(win) = app.get_webview_window(CLIPBOARD_WINDOW_LABEL) else {
        return;
    };
    let was_visible = overlay_native_visible(&win);
    hide_overlay_window(&win);
    if !was_visible {
        return;
    }
    let prev = app
        .state::<ClipboardState>()
        .prev_focus
        .lock()
        .ok()
        .and_then(|g| g.clone());
    let Some(prev_focus) = prev else {
        return;
    };
    #[cfg(target_os = "windows")]
    {
        // 提交到常驻 worker 串行执行：延迟 100ms 后归还焦点
        submit_win_op(DelayedWinOp::RestoreFocus { hwnd: prev_focus });
    }
    #[cfg(target_os = "macos")]
    {
        // macOS 隐藏无激活面板不会把焦点交还给上一个应用，必须显式激活它，
        // 否则用户会「切走了才发现自己在 m-hub 里」（键盘焦点仍在浮层上）
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(100));
            if !crate::mac::activate_app(&prev_focus) {
                log::debug!("剪贴板浮层收起：未能激活原应用 {}", prev_focus);
            }
        });
    }
}

/// 计算浮层初始位置：光标附近（工作区范围内），避免覆盖点击来源
/// 返回物理像素坐标（GetCursorPos/MonitorFromPoint 均为物理像素），
/// 放置窗口时使用 Position::Physical，避免 HiDPI 缩放下坐标错位。
#[cfg(target_os = "windows")]
fn cursor_anchor_position() -> Option<(i32, i32)> {
    unsafe {
        let mut pt: POINT = std::mem::zeroed();
        if GetCursorPos(&mut pt) == 0 {
            return None;
        }
        let monitor = MonitorFromPoint(pt, MONITOR_DEFAULTTONEAREST);
        let mut info: MONITORINFO = std::mem::zeroed();
        info.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
        if GetMonitorInfoW(monitor, &mut info) == 0 {
            return None;
        }
        let rc: RECT = info.rcWork;
        let w = CLIPBOARD_WIDTH as i32;
        let h = CLIPBOARD_HEIGHT as i32;
        let x = (pt.x - w / 2).clamp(rc.left, rc.right - w);
        let y = (pt.y - 30).clamp(rc.top, rc.bottom - h);
        Some((x, y))
    }
}

// ---- 事件驱动监听（AddClipboardFormatListener + WM_CLIPBOARDUPDATE） ----

/// 监听消息窗口类名（进程内唯一）
#[cfg(target_os = "windows")]
const LISTENER_CLASS: *const u16 = windows_sys::core::w!("MHubClipboardListenerWnd");

/// 消息窗口过程：事件由 GetMessageW 循环按消息类型分发，这里走默认处理
#[cfg(target_os = "windows")]
unsafe extern "system" fn listener_wndproc(
    hwnd: *mut core::ffi::c_void,
    msg: u32,
    wparam: usize,
    lparam: isize,
) -> isize {
    DefWindowProcW(hwnd, msg, wparam, lparam)
}

/// 取前台窗口所属进程名（作为历史来源应用，随剪贴板变化时读取）
#[cfg(target_os = "windows")]
fn foreground_app_name() -> Option<String> {
    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.is_null() {
            return None;
        }
        let mut pid: u32 = 0;
        GetWindowThreadProcessId(hwnd, &mut pid);
        if pid == 0 {
            return None;
        }
        process_name_of_pid(pid)
    }
}

/// 进程名缓存容量上限：常驻托盘应用长期运行会累积大量 PID 条目（每个前台进程复制都新增一条），
/// 超过上限时清空重建，避免缓存无限增长（缓慢泄漏）与 PID 复用后返回陈旧进程名。
#[cfg(target_os = "windows")]
const MAX_PROCESS_CACHE: usize = 256;

/// 按 PID 取进程名（带缓存）。`remove_dead_processes=false` 避免每次刷新都全量枚举所有进程，
/// 否则一次复制会卡数百毫秒甚至数秒，拖慢入库并阻塞监听线程消息循环。
/// 同一前台应用连续复制时 PID 不变，缓存命中后跳过 sysinfo 查询，进一步降低入库延迟。
#[cfg(target_os = "windows")]
fn process_name_of_pid(pid: u32) -> Option<String> {
    if pid == 0 {
        return None;
    }
    static CACHE: std::sync::OnceLock<Mutex<std::collections::HashMap<u32, String>>> =
        std::sync::OnceLock::new();
    let cache = CACHE.get_or_init(|| Mutex::new(std::collections::HashMap::new()));
    if let Ok(cached) = cache.lock() {
        if let Some(name) = cached.get(&pid) {
            return Some(name.clone());
        }
    }
    let mut sys = sysinfo::System::new();
    sys.refresh_processes(
        sysinfo::ProcessesToUpdate::Some(&[sysinfo::Pid::from_u32(pid)]),
        false,
    );
    let name = sys
        .process(sysinfo::Pid::from_u32(pid))
        .map(|p| p.name().to_string_lossy().to_string());
    if let Some(name) = &name {
        if let Ok(mut cached) = cache.lock() {
            if cached.len() >= MAX_PROCESS_CACHE {
                cached.clear();
            }
            cached.insert(pid, name.clone());
        }
    }
    name
}

/// 入库辅助：拿数据库连接并执行给定 repo 操作（监听线程跨线程访问 DbState）
fn insert_into_db<T>(
    app: &AppHandle,
    f: impl FnOnce(&rusqlite::Connection) -> Result<T, String>,
) -> Result<T, String> {
    let state = app.try_state::<DbState>().ok_or("剪贴板状态未就绪")?;
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    f(&conn)
}

/// 处理一次剪贴板变化事件：读取（含 HTML 重试）→ 回声抑制 → 入库。
/// 由独立 worker 线程调用（去抖已在 worker 完成），这里不做额外 sleep。
fn handle_clipboard_update(app: &AppHandle) {
    // 暂停记录时跳过（暂停前已入库的数据保留）
    let cfg = crate::config::load();
    if cfg.clipboard_paused {
        return;
    }

    let payload = read_clipboard_payload().or_else(|| {
        // 读取失败（复制方仍占用剪贴板）：稍等重试同一条，避免内容被锁定时静默丢失
        std::thread::sleep(std::time::Duration::from_millis(120));
        read_clipboard_payload()
    });
    let Some(payload) = payload else {
        return;
    };

    let source = foreground_app_name();
    let result: Result<(), String> = match payload {
        ClipboardPayload::Text { content, html } => {
            if content.trim().is_empty() {
                return;
            }
            // 自复制回声：粘贴/复制历史项后系统会再次触发事件，跳过自身写入
            if is_self_set(&content, html.as_deref()) {
                log::debug!("剪贴板监听：忽略自身文本写入回声");
                return;
            }
            insert_into_db(app, |conn| {
                crate::repo::clipboard::insert(conn, &content, html.as_deref(), source.as_deref())
                    .map_err(|e| e.to_string())
            })
        }
        ClipboardPayload::Image { bytes, format } => {
            if !cfg.clipboard_image_enabled {
                return;
            }
            let hash = hash_bytes(&bytes);
            if is_self_set_hash(hash) {
                log::debug!("剪贴板监听：忽略自身图片写入回声");
                return;
            }
            let Some(path) = save_image_snapshot(&bytes, format, hash) else {
                log::warn!("剪贴板图片快照落盘失败，已跳过");
                return;
            };
            let dedup_key = format!("{:016x}", hash);
            match insert_into_db(app, |conn| {
                crate::repo::clipboard::insert_image(conn, &dedup_key, &path, source.as_deref())
                    .map_err(|e| e.to_string())
            }) {
                Ok(true) => Ok(()),
                Ok(false) => {
                    // 相同图片已存在（去重挪到最前），删除本次落盘的冗余快照
                    let _ = std::fs::remove_file(&path);
                    Ok(())
                }
                Err(e) => Err(e),
            }
        }
        ClipboardPayload::Files { paths } => {
            if !cfg.clipboard_file_enabled {
                return;
            }
            let hash = hash_files(&paths);
            if is_self_set_hash(hash) {
                log::debug!("剪贴板监听：忽略自身文件写入回声");
                return;
            }
            insert_into_db(app, |conn| {
                crate::repo::clipboard::insert_files(conn, &paths, source.as_deref())
                    .map_err(|e| e.to_string())
            })
        }
    };

    if let Err(e) = result {
        log::warn!("剪贴板历史入库失败: {}", e);
    }
}

/// 启动剪贴板事件驱动监听线程（Q8：启动零加载，不碰历史数据；仅在剪贴板变化时落库）。
/// 取代原来的 500ms 轮询：AddClipboardFormatListener 注册消息窗口，
/// 剪贴板一变化即收到 WM_CLIPBOARDUPDATE，实时且无轮询开销。
/// macOS 侧没有消息推送，改用 `changeCount` 轮询（见文件末尾的同名函数）。
#[cfg(target_os = "windows")]
pub fn start_monitor(app: AppHandle) {
    // 剪贴板内容读取 + 入库放到独立 worker 线程：去抖沉降后只处理最新状态。
    // 监听线程的消息循环必须保持空闲，才能及时处理 Esc 热键、外部点击收起
    // 与鼠标钩子装卸消息；若在循环里同步读剪贴板，一旦复制方短暂占用剪贴板
    // （重试 + sysinfo 查询），整条消息队列都会被拖住，导致浮层关不掉。
    let (tx, rx) = std::sync::mpsc::channel::<()>();
    let worker_app = app.clone();
    std::thread::spawn(move || {
        while rx.recv().is_ok() {
            // 一次复制可能触发多次 WM_CLIPBOARDUPDATE（多格式逐步写入），
            // 沉降期内持续吞掉新事件，只保留最后一次再处理。
            std::thread::sleep(std::time::Duration::from_millis(SETTLE_MS));
            while rx.try_recv().is_ok() {}
            handle_clipboard_update(&worker_app);
        }
    });

    std::thread::spawn(move || {
        log::info!("剪贴板监听线程启动（事件驱动 WM_CLIPBOARDUPDATE）");
        unsafe {
            let hinstance = GetModuleHandleW(std::ptr::null());
            let class = WNDCLASSW {
                style: 0,
                lpfnWndProc: Some(listener_wndproc),
                cbClsExtra: 0,
                cbWndExtra: 0,
                hInstance: hinstance,
                hIcon: std::ptr::null_mut(),
                hCursor: std::ptr::null_mut(),
                hbrBackground: std::ptr::null_mut(),
                lpszMenuName: std::ptr::null(),
                lpszClassName: LISTENER_CLASS,
            };
            if RegisterClassW(&class) == 0 {
                let err = std::io::Error::last_os_error();
                log::warn!("注册剪贴板监听窗口类失败: {}", err);
            }
            let hwnd = CreateWindowExW(
                0,
                LISTENER_CLASS,
                std::ptr::null(),
                0,
                0,
                0,
                0,
                0,
                HWND_MESSAGE,
                std::ptr::null_mut(),
                hinstance,
                std::ptr::null(),
            );
            if hwnd.is_null() {
                log::warn!("创建剪贴板监听消息窗口失败");
                return;
            }
            if AddClipboardFormatListener(hwnd) == 0 {
                log::warn!("AddClipboardFormatListener 注册失败");
                DestroyWindow(hwnd);
                return;
            }
            log::info!("剪贴板格式监听已注册");
            if let Ok(mut guard) = LISTENER_HWND.lock() {
                *guard = Some(hwnd as isize);
            }

            let mut msg: MSG = std::mem::zeroed();
            while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) != 0 {
                if msg.message == WM_CLIPBOARDUPDATE {
                    let _ = tx.send(());
                } else if msg.message == WM_HOTKEY && msg.wParam as i32 == ESC_HOTKEY_ID {
                    // Esc 兜底关闭：浮层以无激活方式显示时自身收不到键盘事件，
                    // 热键把 Esc 转发到这里，若浮层仍可见则收起
                    if app
                        .get_webview_window(CLIPBOARD_WINDOW_LABEL)
                        .map(|w| w.is_visible().unwrap_or(false))
                        .unwrap_or(false)
                    {
                        hide_overlay(&app);
                    }
                } else if msg.message == WM_XHUB_INSTALL_MOUSE_HOOK {
                    install_mouse_hook_impl();
                } else if msg.message == WM_XHUB_UNINSTALL_MOUSE_HOOK {
                    uninstall_mouse_hook_impl();
                } else if msg.message == WM_XHUB_CLIPBOARD_DISMISS {
                    // 低层鼠标钩子：点击落在浮层窗口矩形之外 → 收起
                    hide_overlay(&app);
                } else {
                    TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
            }

            if let Ok(mut guard) = LISTENER_HWND.lock() {
                *guard = None;
            }
            RemoveClipboardFormatListener(hwnd);
            DestroyWindow(hwnd);
            UnregisterClassW(LISTENER_CLASS, hinstance);
        }
    });
}

// ============================================================
//                    macOS 实现
// ============================================================
//
// AppKit 没有剪贴板变化通知、没有全局鼠标钩子、坐标原点与 Windows 相反，
// 三件事分别由下面的「轮询」「借用悬浮球的监视循环」「逻辑点坐标」消化。
// 其余（去重、回声抑制、落盘、入库、浮层生命周期）与 Windows 共用同一套代码。

/// 本应用的 bundle id（与 `tauri.conf.json` 的 `identifier` 一致）。
/// 用于判断「唤起浮层前的前台应用是不是我们自己」。
#[cfg(target_os = "macos")]
pub const SELF_BUNDLE_ID: &str = "com.mhub.desktop";

/// 取当前前台焦点，平台各自的粒度（见 [`PrevFocus`]）
#[cfg(target_os = "windows")]
fn current_focus() -> Option<PrevFocus> {
    Some(unsafe { GetForegroundWindow() } as isize)
}

/// 取当前前台应用 bundle id
#[cfg(target_os = "macos")]
fn current_focus() -> Option<PrevFocus> {
    crate::mac::frontmost_bundle_id()
}

/// 浮层初始定位用的窗口位置类型。
///
/// **两平台不能统一**，这是最容易翻车的一处：
/// - Windows 的 `GetCursorPos` 返回**物理像素**，配 `Position::Physical`；
/// - macOS 的 `CGEvent::location()` 返回**逻辑点**，配 `Position::Logical`。
///   配错的症状是 HiDPI（Retina）屏上浮层跑到了半屏之外。
#[cfg(target_os = "windows")]
fn anchor_position(x: i32, y: i32) -> tauri::Position {
    tauri::Position::Physical(tauri::PhysicalPosition::new(x, y))
}
#[cfg(target_os = "macos")]
fn anchor_position(x: i32, y: i32) -> tauri::Position {
    tauri::Position::Logical(tauri::LogicalPosition::new(x as f64, y as f64))
}

/// 计算浮层初始位置：光标附近，并夹在光标所在显示器的**工作区**内。
///
/// 返回**逻辑点**（左上原点），与 [`anchor_position`] 配套。
///
/// ## 为什么是工作区而不是整块屏
///
/// macOS 的 Dock 与菜单栏会吃掉屏幕边缘，Tauri 的 `Monitor::work_area()` 内部走
/// `NSScreen.visibleFrame`。夹到**整屏**矩形的后果是浮层可以被摆到 Dock 底下、
/// 下半截被遮住 —— 用户报的正是「浮层跳到下方挡住的地方」。
///
/// Windows 那条路径取的是 `GetMonitorInfoW` 的 `rcWork`（本来就是工作区），
/// 移植到 macOS 时这一处被换成了整屏 `CGDisplayBounds`，并用「macOS 本就允许
/// 浮窗压在菜单栏上」把差异合理化了。但那条只对**很矮**的浮层成立：
/// 520×440 的面板压到 Dock 上是实打实的遮挡，而且是稳定复现的那种。
#[cfg(target_os = "macos")]
fn cursor_anchor_position(win: &tauri::WebviewWindow) -> Option<(i32, i32)> {
    let (cx, cy) = crate::mac::cursor_point()?;
    let (ax, ay, aw, ah) = work_area_logical_at(win, cx, cy)?;
    anchor_in_work_area(cx, cy, CLIPBOARD_WIDTH, CLIPBOARD_HEIGHT, ax, ay, aw, ah)
}

/// 把浮层摆在光标附近并夹进工作区。**纯函数**（无窗口、无光标），所以下面每条
/// 边界都能被单测直接覆盖 —— 这段是本模块唯一会算错的地方，而它所在的
/// 「监听线程 + 真实显示器」环境没法在测试里搭出来。
///
/// `(cx, cy)` 光标逻辑点；`(ax, ay, aw, ah)` 工作区逻辑点；`w`/`h` 浮层逻辑尺寸。
#[cfg(target_os = "macos")]
fn anchor_in_work_area(
    cx: f64,
    cy: f64,
    w: f64,
    h: f64,
    ax: f64,
    ay: f64,
    aw: f64,
    ah: f64,
) -> Option<(i32, i32)> {
    // 上界用 `.max()`：工作区真比浮层还窄时（极小屏 / 显示器枚举异常）宁可让浮层
    // 略微超屏，也不能让 clamp 因 min > max 而 panic —— 定位一旦 panic，浮层会
    // 停在上一次的旧位置甚至屏幕外，且没有任何报错。
    let x = (cx - w / 2.0).clamp(ax, (ax + aw - w).max(ax));
    // 浮层在光标上方展开（与 Windows 一致）：y = 光标 - 30，再夹进工作区
    let y = (cy - 30.0).clamp(ay, (ay + ah - h).max(ay));
    Some((x.round() as i32, y.round() as i32))
}

/// 光标所在显示器的**工作区**，换算成逻辑点 `(x, y, w, h)`（左上原点）。
///
/// `Monitor::work_area()` / `position()` / `size()` 给的都是**物理** px，而本模块的
/// 浮层尺寸与 [`anchor_position`] 都是逻辑点，故整块除以该显示器的 scale。
///
/// 选显示器：优先取**包含光标**的那块（浮层要跟着光标走，而不是跟着浮层上次
/// 待的位置走）；点不到就依次回退主显示器 / 浮层所在显示器；再取不到返回
/// `None` —— 调用方保持原位置不动，**不拿整屏矩形顶替**（那正是本函数要修的错）。
#[cfg(target_os = "macos")]
fn work_area_logical_at(
    win: &tauri::WebviewWindow,
    cx: f64,
    cy: f64,
) -> Option<(f64, f64, f64, f64)> {
    let monitors = win.available_monitors().ok()?;
    let fallback = win
        .primary_monitor()
        .ok()
        .flatten()
        .or_else(|| win.current_monitor().ok().flatten());
    let pick = monitors
        .iter()
        .find(|m| {
            let s = m.scale_factor().max(0.01);
            let (px, py) = (m.position().x as f64 / s, m.position().y as f64 / s);
            let (pw, ph) = (m.size().width as f64 / s, m.size().height as f64 / s);
            cx >= px && cx < px + pw && cy >= py && cy < py + ph
        })
        .cloned()
        .or(fallback)?;
    let s = pick.scale_factor().max(0.01);
    let r = pick.work_area();
    Some((
        r.position.x as f64 / s,
        r.position.y as f64 / s,
        r.size.width as f64 / s,
        r.size.height as f64 / s,
    ))
}

/// 无激活显示浮层：把 NSWindow 改成 `NonactivatingPanel` 后再 `show()`。
/// 这样浮层出现在光标旁时，**用户正在输入的文本框仍然保持焦点**——
/// 这正是剪贴板浮层的意义所在。
#[cfg(target_os = "macos")]
fn show_overlay_no_activate(win: &tauri::WebviewWindow) {
    if !crate::mac::set_nonactivating_panel(win) {
        log::warn!("剪贴板浮层：设置 NonactivatingPanel 失败，将退化为会抢焦点的显示");
    }
    let _ = win.show();
    crate::win_taskbar::apply(win);
    // 置顶：浮层要在所有普通窗口之上
    let _ = win.set_always_on_top(true);
}

/// 写入纯文本（可选同时写富文本）到剪贴板，写后记录回声指纹。
#[cfg(target_os = "macos")]
pub fn set_clipboard(text: &str, html: Option<&str>) -> Result<(), String> {
    // 回声指纹基于「即将写入的内容」，与监听端读到的口径一致
    let hash = content_hash(text, html);
    crate::mac::write_clipboard_text(text, html)?;
    if let Ok(mut guard) = LAST_SELF_SET.lock() {
        *guard = Some((hash, std::time::Instant::now()));
    }
    Ok(())
}

/// 写入图片到剪贴板（按文件头判断 PNG / TIFF），写后记录回声指纹。
#[cfg(target_os = "macos")]
pub fn set_clipboard_image(path: &str) -> Result<(), String> {
    let bytes = std::fs::read(path).map_err(|e| format!("读取图片快照失败: {}", e))?;
    // 回声指纹基于写回剪贴板的实际字节，与监听端读到的一致
    let hash = hash_bytes(&bytes);
    crate::mac::write_clipboard_image(path)?;
    record_self_set_hash(hash);
    Ok(())
}

/// 写入文件列表到剪贴板（只引用原路径，不拷贝内容），写后记录回声指纹。
#[cfg(target_os = "macos")]
pub fn set_clipboard_files(paths: &[String]) -> Result<(), String> {
    let existing: Vec<String> = paths
        .iter()
        .filter(|p| std::path::Path::new(p).exists())
        .cloned()
        .collect();
    if existing.is_empty() {
        return Err("文件已不存在，无法粘贴".into());
    }
    let hash = hash_files(&existing);
    crate::mac::write_clipboard_files(&existing)?;
    record_self_set_hash(hash);
    Ok(())
}

/// 粘贴到唤起前的应用：延迟一小段时间让浮层彻底隐藏 → 归还焦点 → 合成粘贴键。
///
/// 为什么必须延迟：macOS 隐藏窗口与交还焦点的动画需要时间，立刻注入按键
/// 会被仍在前台的**浮层自己**吃掉，表现为「粘了个寂寞」。
fn paste_macos(app: &AppHandle, prev_focus: &PrevFocus) {
    let method = crate::config::load().clipboard_paste_method;
    let bundle_id = prev_focus.clone();
    let app = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(200));
        crate::mac::release_modifier_keys();

        // 焦点若仍在本应用（浮层无激活失败、或用户点过浮层），先交还
        if let Some(front) = crate::mac::frontmost_bundle_id() {
            if front == SELF_BUNDLE_ID {
                if !crate::mac::activate_app(&bundle_id) {
                    log::warn!("剪贴板粘贴：无法激活原应用 {}", bundle_id);
                    return;
                }
                std::thread::sleep(std::time::Duration::from_millis(150));
            }
        }
        let _ = &app;

        // 模拟输入需要辅助功能权限；未授权时 CGEventPost 会被系统静默丢弃，
        // 与其让用户对着「点了没反应」猜，不如把这条路径说明白
        if !crate::mac::ax_is_trusted() {
            log::warn!(
                "剪贴板粘贴：未获得「辅助功能」权限，无法模拟 ⌘V。\
                 请到「系统设置 → 隐私与安全性 → 辅助功能」中允许 m-hub"
            );
            return;
        }
        // 配置里存的是**跨平台的逻辑口径**（ctrl_v / ctrl_shift_v / shift_insert /
        // auto），macOS 上对应的物理键是 ⌘V / ⌘⇧V / ⇧Insert。
        //
        // ⚠️ 这里原先只认 `cmd_*`，而配置里根本不会有 `cmd_*` —— 于是
        // `ctrl_v` 和 `ctrl_shift_v` **双双落进 `_ => "cmd_v"`**，
        // 「无格式粘贴」选了等于没选，恒发 ⌘V，在只认 ⇧⌘V 的地方直接粘不出来。
        // 选项成了纯装饰（2026-09-29 修）。
        //
        // `auto` 在 macOS 上恒等于 ⌘V，且这是**正确**行为：macOS 的终端
        // 全部支持 ⌘V（不像 Windows 终端只认 Ctrl+Shift+V），上游那套
        // 「按终端进程名白名单自动改键」的逻辑在 mac 上没有存在意义。
        let mac_method = match method.as_str() {
            "ctrl_shift_v" => "cmd_shift_v",
            "shift_insert" => "shift_insert",
            _ => "cmd_v",
        };
        crate::mac::send_paste_keystroke(mac_method);
    });
}

/// 前台应用名（记为历史条目的来源应用）。
///
/// 走 `mac::frontmost_app_name()`（内部取 `NSRunningApplication.localizedName`），
/// 与判定/激活用的 [`crate::mac::frontmost_bundle_id`] 分开：展示要的是
/// 用户认得出的名字，激活 API 要的是 bundle id，两者不能互相推导。
#[cfg(target_os = "macos")]
fn foreground_app_name() -> Option<String> {
    crate::mac::frontmost_app_name().filter(|s| !s.is_empty())
}

/// 延迟窗口操作 worker：Windows 侧靠它串行执行「隐藏 → 归还焦点 → 注入按键」。
/// macOS 的焦点归还由 `hide_overlay` / `paste_macos` 各自就地派生线程完成，
/// 没有共享队列，故这里只是空实现（保留函数名与调用点，两平台同构）。
#[cfg(target_os = "macos")]
pub fn init_win_op_worker() {
    if !crate::mac::ax_is_trusted() {
        log::warn!(
            "[剪贴板] 未获得「辅助功能」权限：模拟粘贴将不可用。\
             请到「系统设置 → 隐私与安全性 → 辅助功能」中允许 m-hub。"
        );
    } else {
        log::info!("[剪贴板] 辅助功能权限已授予，模拟粘贴可用");
    }
}

/// 启动剪贴板监听。
///
/// **轮询而非推送**——这是 macOS 移植里最本质的一处差异：AppKit 没有
/// 「剪贴板变了」的回调（`NSPasteboard` 的 `changeCount` 只是自增计数器，
/// 没有任何通知机制）。所以只能轮询它。
///
/// 轮询间隔取 250ms：复制一次到历史入库的感知延迟在 0.25s 内，人无感；
/// 而 100ms 级的轮询在应用常驻时会持续占用 CPU，得不偿失。变化检测只看
/// `changeCount`（系统维护的整数，读一次是一次 IPC），比每轮真去读内容
/// 便宜一到两个数量级。
#[cfg(target_os = "macos")]
pub fn start_monitor(app: AppHandle) {
    use objc2_app_kit::NSPasteboard;

    // 内容读取 + 入库放到独立 worker：轮询线程只管「发现变化」，
    // 绝不能被读取/入库的耗时拖慢（否则密集复制时会漏事件）
    let (tx, rx) = std::sync::mpsc::channel::<()>();
    let worker_app = app.clone();
    std::thread::spawn(move || {
        while rx.recv().is_ok() {
            // 一次复制可能连续改多次 changeCount（多格式分阶段写入），
            // 沉降期内吞掉新事件，只处理最后一次
            std::thread::sleep(std::time::Duration::from_millis(SETTLE_MS));
            while rx.try_recv().is_ok() {}
            handle_clipboard_update(&worker_app);
        }
    });

    std::thread::spawn(move || {
        let pb = NSPasteboard::generalPasteboard();
        let mut last = pb.changeCount();
        log::info!("剪贴板监听线程启动（轮询 NSPasteboard.changeCount，间隔 250ms）");
        loop {
            std::thread::sleep(std::time::Duration::from_millis(250));
            // 暂停时也要刷新基准，否则恢复监听会把这期间的 N 次变化一次性
            // 当成「一次复制」入库
            if crate::config::load().clipboard_paused {
                last = pb.changeCount();
                continue;
            }
            let now = pb.changeCount();
            if now != last {
                last = now;
                let _ = tx.send(());
            }
        }
    });
}
