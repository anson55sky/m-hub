//! 浮窗「不占任务栏/Dock 位置」的平台差异处理。
//!
//! 这不是 Windows 专属问题，而是**两平台根因不同**，所以本模块两端都实现。
//!
//! ## Windows：必须改 ex-style，builder 的 `skip_taskbar(true)` 不够
//!
//! tao 0.35.3 的 skip_taskbar 只有一次 `ITaskbarList::DeleteTab`
//! （`tao/src/platform_impl/windows/window.rs::set_skip_taskbar`），窗口本身仍是普通
//! 顶层窗口——tao 对 `Parent::None` 的窗口一律置 `WindowFlags::ON_TASKBAR`
//! （同文件 1164 行），于是 ex-style 里一直带 `WS_EX_APPWINDOW`。DeleteTab 只是
//! 「现在把按钮删掉」，不是「以后都不给按钮」：窗口的 ex-style 一旦被重建
//! （`WindowFlags::apply_diff` 在 VISIBLE 变化时执行 `SetWindowLongW(GWL_EXSTYLE, …)`，
//! 即每次 hide → show 都会发生一次），Shell 就重新给它加任务栏按钮。
//!
//! 实机取证（v0.5.5，运行中的 m-hub，EnumWindows 读 GWL_EXSTYLE）：
//! 悬浮球窗口 = `0x00040118`（`WS_EX_APPWINDOW` 开、`WS_EX_TOOLWINDOW` 关、无 owner）
//! → 任务栏多出一个「悬浮球」图标；对照组是 tao 自建的托盘/热键窗口
//! `0x080801A0`（带 `WS_EX_TOOLWINDOW`）→ 从不进任务栏。答案就在这面镜子里。
//!
//! 修复口径：把窗口改成**工具窗**：置 `WS_EX_TOOLWINDOW` 并清 `WS_EX_APPWINDOW`。
//! 这两个位才是 Shell 判定「该窗口归不归任务栏管」的依据，工具窗永远没有任务栏按钮
//! （顺带也不进 Alt+Tab，对浮窗而言正是想要的）；再补一次 `set_skip_taskbar(true)`
//! 让**当前已经存在**的那颗按钮立刻消失。
//!
//! ⚠️ **每次 show 之后都必须重新 apply**——tao 每个 flags 变化都会把 ex-style 整份
//! 重建回带 `WS_EX_APPWINDOW` 的形态，所以浮窗的显示统一走本模块的 [`show`]，
//! 不要再直接调 `win.show()`（见 AGENTS.md 约定 48）。
//!
//! ## macOS：tao 层没有这个开关，两边都不需要额外处理
//!
//! macOS 没有「任务栏」这个概念，对应物是 **Dock 图标**与 **⌘Tab 应用切换器**，而两者
//! 都由**整个应用的 activation policy** 决定（`NSApplicationActivationPolicy`），
//! **不是逐窗口**的。tao 的 `WindowAttributes` 在 macOS 上根本没有 `skip_taskbar` 字段
//! （见 `tao-0.35.3/src/platform_impl/macos/window.rs`），所以 builder 传下来的
//! `skip_taskbar(true)` 在 mac 上是**空操作**。
//!
//! 这一点在这里必须写死，因为反过来看很危险：若 macOS 侧**真的**去调
//! `set_skip_taskbar(true)`，它会走 tao 的「把整个 app 降级成 Accessory」路径，
//! 结果是**整个 m-hub 的 Dock 图标消失**（连带主窗口的 ⌘Tab 入口也没了）——为了
//! 浮窗不出现在 ⌘Tab 而牺牲主应用的可发现性，是本末倒置。
//!
//! m-hub 在 mac 上的选择是**保留 Dock 图标**（activation policy = Regular）：
//! ① 主窗口收进托盘后 Dock 仍有图标 → 点一下就回来，符合 mac 用户对「常驻应用」的预期
//!    （与只放托盘、没有 Dock 图标的纯后台应用区分开）；② 悬浮球/通知这类浮窗本来就
//!    是**无边框、小尺寸、可穿透**的，且各有自己的用途，出现在 ⌘Tab 里不构成困扰
//!    （⌘Tab 里显示的是 app 而非窗口，浮窗并不会因此多出一个条目）。
//!
//! 因此 mac 分支是**有意的 no-op**，不是「留个坑以后填」。

/// 让窗口彻底不进任务栏（Windows 置工具窗样式 + 摘掉当前可能已存在的任务栏按钮）。
///
/// 幂等、廉价（一次 GetWindowLong + 必要时一次 SetWindowLong + 一次 DeleteTab），
/// 可在每次显示前无条件调用。
pub fn apply(win: &tauri::WebviewWindow) {
    #[cfg(target_os = "windows")]
    {
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            GetWindowLongPtrW, SetWindowLongPtrW, GWL_EXSTYLE, WS_EX_APPWINDOW, WS_EX_TOOLWINDOW,
        };
        if let Ok(hwnd) = win.hwnd() {
            unsafe {
                let ex = GetWindowLongPtrW(hwnd.0, GWL_EXSTYLE);
                let want = (ex | WS_EX_TOOLWINDOW as isize) & !(WS_EX_APPWINDOW as isize);
                if want != ex {
                    SetWindowLongPtrW(hwnd.0, GWL_EXSTYLE, want);
                }
            }
        }
        // tao 的 DeleteTab：样式改动不会让已显示窗口的按钮自动消失，必须显式摘一次
        let _ = win.set_skip_taskbar(true);
    }
    #[cfg(not(target_os = "windows"))]
    {
        // macOS：Dock / ⌘Tab 由整个 app 的 activation policy 决定，逐窗口无开关；
        // m-hub 刻意保留 Dock 图标（见模块文档「macOS」节），故此处不做任何事。
        let _ = win;
    }
}

/// 显示浮窗并保证其不占任务栏位置（浮窗显示的唯一入口，替代裸 `win.show()`）。
pub fn show(win: &tauri::WebviewWindow) {
    let _ = win.show();
    apply(win);
}
