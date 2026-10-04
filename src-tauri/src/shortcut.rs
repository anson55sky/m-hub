use std::str::FromStr;
use tauri::{AppHandle, Emitter};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};

/// 主窗口显隐（全局呼出）默认快捷键。
///
/// **macOS 用 `⌘⇧Space`**：注意 macOS 的 Spotlight 是 `⌘Space`（不带 Shift），
/// 两者不冲突；而上游原本的 `⌘⇧Space` 在 Finder 里是「以选中内容搜索」，
/// 但那是 Finder 内的应用级快捷键、不进系统热键池，全局注册能拿到。
#[cfg(target_os = "macos")]
pub const DEFAULT_TOGGLE_SHORTCUT: &str = "CommandOrControl+Shift+Space";
#[cfg(not(target_os = "macos"))]
pub const DEFAULT_TOGGLE_SHORTCUT: &str = "Ctrl+Shift+Space";

/// 剪贴板历史浮层默认呼出快捷键。
///
/// - **Windows**：`Ctrl+\``（Backquote）。避开 `Ctrl+Shift+V`「无格式粘贴」等高频组合。
/// - **macOS**：`⌃⌘V`（Control+Command+V）。这里**不能用**上游选的 `⌘⌥V`
///   ——那是 macOS 自带的「粘贴并匹配样式」，系统级注册，m-hub 抢不到，
///   注册只会静默失败（表现为快捷键录进去了、按了没反应）。
///   `⌃⌘V` 是 macOS 剪贴板管理器的行业惯例（Raycast / Pastebot 一系），不撞系统键。
#[cfg(target_os = "macos")]
pub const DEFAULT_CLIPBOARD_SHORTCUT: &str = "CommandOrControl+Control+V";
#[cfg(not(target_os = "macos"))]
pub const DEFAULT_CLIPBOARD_SHORTCUT: &str = "Ctrl+`";

/// 全局搜索默认呼出快捷键。
#[cfg(target_os = "macos")]
pub const DEFAULT_SEARCH_SHORTCUT: &str = "CommandOrControl+K";
#[cfg(not(target_os = "macos"))]
pub const DEFAULT_SEARCH_SHORTCUT: &str = "Ctrl+K";

/// AI 对话默认呼出快捷键。
#[cfg(target_os = "macos")]
pub const DEFAULT_CHAT_SHORTCUT: &str = "CommandOrControl+Shift+K";
#[cfg(not(target_os = "macos"))]
pub const DEFAULT_CHAT_SHORTCUT: &str = "Ctrl+Shift+K";

/// 统一捕获默认快捷键（2026-09-29 新增）。
///
/// 从任何地方一行记下东西，自动路由到速记 / 待办 / 提示词 / 倒计时 / 速达。
/// 键位选 `⇧⌘U` 而不是 `⇧⌘C`：后者与「拷贝为样式」一类系统/应用级注册相邻，
/// 抢不到的表现是**静默失效**（注册失败只落在日志里）。这与剪贴板键避开
/// `⌘⌥V` 属于同一类考虑（见 `DEFAULT_CLIPBOARD_SHORTCUT` 的注释）。
#[cfg(target_os = "macos")]
pub const DEFAULT_CAPTURE_SHORTCUT: &str = "CommandOrControl+Shift+U";
#[cfg(not(target_os = "macos"))]
pub const DEFAULT_CAPTURE_SHORTCUT: &str = "Ctrl+Shift+U";

/// 判断两个快捷键字符串是否代表同一个物理按键组合
/// （如 Windows 上 CommandOrControl 与 Ctrl 是同一个键，仅写法不同）
pub fn same_hotkey(a: &str, b: &str) -> bool {
    match (Shortcut::from_str(a), Shortcut::from_str(b)) {
        (Ok(x), Ok(y)) => x.id() == y.id(),
        _ => false,
    }
}

pub fn register_toggle_shortcut(app: &AppHandle, shortcut: &str) -> Result<(), String> {
    app.global_shortcut()
        .register(shortcut)
        .map_err(|e| format!("{}", e))
}

pub fn unregister_toggle_shortcut(app: &AppHandle, shortcut: &str) -> Result<(), String> {
    app.global_shortcut()
        .unregister(shortcut)
        .map_err(|e| format!("{}", e))
}

pub fn is_shortcut_registered(app: &AppHandle, shortcut: &str) -> bool {
    app.global_shortcut()
        .is_registered(shortcut)
}

pub fn setup(app: &tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    let handle = app.handle().clone();
    app.handle().plugin(
        tauri_plugin_global_shortcut::Builder::new()
            .with_handler(move |app, shortcut: &Shortcut, event| {
                if event.state == ShortcutState::Pressed {
                    // 诊断：确认全局热键事件是否到达本进程（用于定位「录入成功但呼出/隐藏不生效」）
                    log::info!("[快捷键] 事件触发: {} state={:?}", shortcut, event.state);
                    // 按当前配置分发四个全局快捷键（搜索 / AI 对话事件由主窗前端监听处理，
                    // 见 index.vue；剪贴板与主窗显隐由 lib.rs 的 app.listen 处理）
                    let cfg = crate::config::load();
                    let pressed = shortcut.to_string();
                    if same_hotkey(&cfg.clipboard_shortcut, &pressed) {
                        let _ = app.emit("clipboard-toggle", ());
                    } else if same_hotkey(&cfg.search_shortcut, &pressed) {
                        let _ = app.emit("search-shortcut", ());
                    } else if same_hotkey(&cfg.chat_shortcut, &pressed) {
                        let _ = app.emit("chat-shortcut", ());
                    } else if same_hotkey(&cfg.capture_shortcut, &pressed) {
                        let _ = app.emit("capture-shortcut", ());
                    } else {
                        let _ = app.emit("global-shortcut-toggle", ());
                    }
                }
            })
            .build(),
    )?;

    let config = crate::config::load();
    // ⚠️ 逐个按**启用开关**决定是否注册（2026-10-03）。
    //   关掉 = 不注册，但配置里的组合原样保留 —— 用户随时能开回来，
    //   不必重新录一遍。组合本身仍照常注册与冲突预检（那是「这个键归谁」的问题，
    //   与「要不要启用」无关；分开看才能解释「关掉了为什么还提示冲突」）。
    for (key, enabled) in enabled_pairs(&config) {
        if !enabled {
            log::info!("[快捷键] {} 已被用户停用，不注册（组合保留在配置里）", key);
            continue;
        }
        if let Err(e) = register_toggle_shortcut(&handle, key) {
            log::warn!("[快捷键] 注册{}失败: {}", key, e);
        } else {
            log::info!("[快捷键] 已注册{}: {}", key, key);
        }
    }
    Ok(())
}

/// 五个全局快捷键的「开关 + 组合」对照表。
///
/// ⚠️ 这里**刻意枚举全部五个**，而不是「四个 + 捕获那个例外」——
///   统一捕获 ⇧⌘U 同样是全局注册的快捷键，少给它一个开关会让它在
///   设置列表里成为唯一的例外（而例外就会被当成 bug）。
///
/// 顺序固定（主窗/剪贴板/搜索/对话/捕获）：日志与测试都按这个顺序断言。
pub fn enabled_pairs(cfg: &crate::config::AppConfig) -> Vec<(&'static str, bool)> {
    shortcut_items(cfg).into_iter().map(|(l, _, e)| (l, e)).collect()
}

/// 按开关启停某一个全局快捷键。
///
/// 失败要**回滚**（约定 72 的形状：先落新 → 尝试 → 失败恢复）：
/// 先反注册旧的，成功注册新的才算换；注册失败就把旧的注册回去。
/// 否则「打开开关」失败会留下一个**既没开也没关**的中间态 ——
/// 配置说开着、实际没注册，用户按半天没反应也看不出是哪一环坏了。
/// 五个全局快捷键的「标签 / 热键 / 启用开关」三合一。
///
/// ⚠️ `enabled_pairs` 只给标签与开关，**给不出热键** —— 而冲突描述恰恰需要
///   热键才能比对。第一版 `describe_conflict` 拿 `enabled_pairs` 的第二项当热键
///   去比，类型直接对不上（bool vs &str）。两张表看着重复，故合并成这一张：
///   `enabled_pairs` 改成它的薄封装，全工程只有这一处列全五个。
pub fn shortcut_items(cfg: &crate::config::AppConfig) -> Vec<(&'static str, &str, bool)> {
    vec![
        ("全局快捷键", &cfg.global_shortcut, cfg.shortcut_toggle_enabled),
        ("剪贴板快捷键", &cfg.clipboard_shortcut, cfg.shortcut_clipboard_enabled),
        ("搜索快捷键", &cfg.search_shortcut, cfg.shortcut_search_enabled),
        ("AI 对话快捷键", &cfg.chat_shortcut, cfg.shortcut_chat_enabled),
        ("统一捕获快捷键", &cfg.capture_shortcut, cfg.shortcut_capture_enabled),
    ]
}

pub fn set_enabled(app: &AppHandle, previous: &str, next: &str, enable: bool) -> Result<(), String> {
    if enable {
        if is_shortcut_registered(app, next) {
            // 已经注册着（可能本来就是开的）—— 直接成功，别走反注册再注册
            // 那条路：它会先把旧键摘掉，若紧接着注册失败就两头都没了
            return Ok(());
        }
        match register_toggle_shortcut(app, next) {
            Ok(()) => Ok(()),
            Err(e) if is_conflict_error(&e) => {
                // ⚠️ macOS 上「关掉 → 立刻重开」会**误报冲突**（发布说明里那条修复）。
                //   成因：反注册是异步的，OS 侧的系统级注册还没释放，
                //   紧接着 register 就被回一个「HotKey already registered」。
                //   而这个组合**完全合法**（用户只是把开关拧回去），
                //   报「快捷键冲突」会让人以为这个键真的被别人占着，
                //   于是去找根本不存在的「占用程序」。
                //
                //   修法：显式反注册一次（迫使 OS 释放）再注册。
                //   反注册失败**不作为错误** —— 它本来就可能没注册着。
                log::info!("[快捷键] {} 首次注册报冲突，先反注册再试一次", next);
                let _ = unregister_toggle_shortcut(app, next);
                register_toggle_shortcut(app, next).map_err(|e2| {
                    describe_conflict(app, next, &format_shortcut_error(&e2))
                })
            }
            Err(e) => Err(format_shortcut_error(&e)),
        }
    } else {
        unregister_toggle_shortcut(app, previous).or_else(|e| {
            // 「本来就没注册」不是错误：关掉一个已关闭的开关应当是幂等的
            if is_conflict_error(&e) {
                Ok(())
            } else {
                Err(e)
            }
        })
    }
}

/// 把「旧快捷键 → 新快捷键」的改绑一次做完：冲突预检、反注册旧的、注册新的，
/// 注册失败时回滚旧键。四个可自定义快捷键（主窗/剪贴板/搜索/AI 对话）的
/// set_*_shortcut 命令共用这一份逻辑，只是各自读写配置里自己的字段。
pub fn rebind_shortcut(app: &AppHandle, previous: &str, next: &str) -> Result<(), String> {
    if crate::shortcut::is_shortcut_registered(app, next) {
        // 说清楚是「和我们自己的另一个撞了」还是「被别的软件占了」——
        // 两者的下一步完全不同（换个键 / 去关那个软件）。
        return Err(describe_conflict(app, next, "快捷键冲突"));
    }
    if let Err(e) = unregister_toggle_shortcut(app, previous) {
        if !is_conflict_error(&e) {
            return Err(e);
        }
        return Err("快捷键冲突".into());
    }
    if let Err(e) = register_toggle_shortcut(app, next) {
        let mapped = format_shortcut_error(&e);
        if !mapped.eq(&e) {
            let _ = register_toggle_shortcut(app, previous);
        }
        return Err(mapped);
    }
    Ok(())
}

/// 冲突的**具体**说明（发布说明：真冲突时要说明是和哪个快捷键撞了，
/// 还是被其它程序占用）。
///
/// ⚠️ 原来只有一句「快捷键冲突」，用户无从下手 —— 他既不知道撞的是自己
///   的另一个快捷键（换个键就行），也不知道是别的软件占了（得去关那个软件），
///   两种情况的**下一步完全不同**，而界面把它们说成了同一句话。
///
/// 判据：我们自己的五个快捷键都写在配置里，故可以逐一比对；
/// **都不匹配**才归为「被其它程序占用」。
pub fn describe_conflict(app: &AppHandle, hotkey: &str, fallback: &str) -> String {
    let cfg = crate::config::load();
    // ⚠️ 只拿**启用中**的来比：已关掉的快捷键不占键，拿它当冲突理由是误导
    //   （用户会看到「和『搜索快捷键』撞了」，而去打开那个开关就又撞一次）。
    for (label, own, enabled) in shortcut_items(&cfg) {
        if enabled && same_hotkey(own, hotkey) {
            return format!("{hotkey} 已经被「{label}」占用了");
        }
    }
    let _ = app;
    if is_conflict_error(fallback) {
        format!("{hotkey} 被其它程序占用了（可在「系统设置 → 键盘」里看谁抢着它）")
    } else {
        fallback.to_string()
    }
}

pub fn format_shortcut_error(err: &str) -> String {
    if err.contains("HotKey") || err.contains("already") || err.contains("occupied") {
        "快捷键冲突".to_string()
    } else if err.contains("UnsupportedKey") || err.contains("InvalidFormat") || err.contains("EmptyToken") {
        "快捷键格式无效，请重新录入（修饰键在前、单个主键，如 Ctrl+Shift+K）".to_string()
    } else {
        err.to_string()
    }
}

pub fn is_conflict_error(err: &str) -> bool {
    err.contains("HotKey") || err.contains("already") || err.contains("occupied")
}
