use crate::commands::DbState;
use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindow};

/// 浮窗便签窗口的 label 前缀；完整 label 为 `sticky-{slot}`
pub const STICKY_WINDOW_PREFIX: &str = "sticky-";

/// 浮窗固定尺寸（设计定稿：约 260×280，不可缩放）
pub const STICKY_WIDTH: f64 = 260.0;
pub const STICKY_HEIGHT: f64 = 280.0;

pub fn window_label(slot: i64) -> String {
    format!("{}{}", STICKY_WINDOW_PREFIX, slot)
}
/// 浮窗初始落点：主窗中心略偏右下。**单位与算法统一由
/// `lib.rs::centered_on_main` 提供**（此前此处有三份各自为政的实现，
/// 其中把物理坐标与逻辑尺寸混算，Retina 上落点偏近一倍）。
fn initial_position(app: &AppHandle) -> Option<(f64, f64)> {
    crate::centered_on_main(app, STICKY_WIDTH as f64, STICKY_HEIGHT as f64)
}

/// 创建（或重建）便签浮窗。已存在同 label 窗口时先复用。
/// 传入已保存的位置；未保存则默认出现在主窗口中心附近。
pub fn create_or_focus(
    app: &AppHandle,
    slot: i64,
    x: Option<f64>,
    y: Option<f64>,
    always_on_top: bool,
) -> tauri::Result<WebviewWindow> {
    let label = window_label(slot);
    if let Some(win) = app.get_webview_window(&label) {
        // 显示走 win_taskbar：tao 的 hide/show 会重建 ex-style 把 WS_EX_APPWINDOW
        // 加回来，任务栏随即多出一颗按钮（见 win_taskbar 模块注释）
        crate::win_taskbar::show(&win);
        let _ = win.set_focus();
        return Ok(win);
    }

    let (pos_x, pos_y) = match (x, y) {
        (Some(px), Some(py)) => {
            // 恢复落点必须**校正**而不是照搬：库里可能存着单位错误的旧值
            // （历史 bug：存物理、按逻辑读，Retina 上坐标翻倍），
            // 也可能被用户拖到屏幕外。详见 lib.rs::sanitize_float_position。
            let (ax, ay) = crate::sanitize_float_position(app, px, py, STICKY_WIDTH, STICKY_HEIGHT);
            // 顺手把纠正后的值写回库：否则每次启动都要重新纠正一次，
            // 而且一旦用户把它拖到某块不存在的屏上，坏值会一直留着。
            if (ax - px).abs() > 0.5 || (ay - py).abs() > 0.5 {
                if let Some(state) = app.try_state::<DbState>() {
                    if let Ok(conn) = state.0.lock() {
                        let _ = crate::repo::detached_sticky::update_position(
                            &conn, slot, ax, ay,
                        );
                    }
                }
            }
            (ax, ay)
        }
        _ => initial_position(app).unwrap_or((200.0, 200.0)),
    };

    let builder = tauri::WebviewWindowBuilder::new(
        app,
        &label,
        WebviewUrl::App("index.html".into()),
    )
    .title("便签")
    .inner_size(STICKY_WIDTH, STICKY_HEIGHT)
    .resizable(false)
    .decorations(false)
    .transparent(true)
    .shadow(true)
    .always_on_top(always_on_top)
    .skip_taskbar(true)
    .visible(true)
    .position(pos_x, pos_y)
    .additional_browser_args(crate::ADDITIONAL_BROWSER_ARGS);

    // 透明窗口在 Windows 上不能同时启用 resizable 与阴影拉伸，
    // 保持固定尺寸，窗口内内容即卡片本体
    #[cfg(target_os = "windows")]
    {
        builder = builder.shadow(false);
    }

    let win = builder.build()?;
    crate::win_taskbar::apply(&win);

    // 移动时持久化位置。⚠️ 必须换算成**逻辑**像素（2026-09-29 修）。
    // `outer_position()` 返回的是 PhysicalPosition（tauri 文档明写），
    // 而恢复侧走 `WindowBuilder::position()`，后者收**逻辑**像素。
    // 两者差一个 scale_factor，于是 Retina 上拖到中间的便签下次启动就落到
    // 屏幕右下角（坐标翻倍），得再手动拖回来。
    // 注释原先写着「存逻辑坐标」—— 那是错的，代码一直存的是物理。
    let app_handle = app.clone();
    let moved_win = win.clone();
    win.on_window_event(move |event| {
        if let tauri::WindowEvent::Moved(_) = event {
            let slot = moved_win
                .label()
                .trim_start_matches(STICKY_WINDOW_PREFIX)
                .parse::<i64>()
                .unwrap_or(0);
            if let Ok(pos) = moved_win.outer_position() {
                let scale = moved_win.scale_factor().unwrap_or(1.0).max(0.01);
                if let Some(state) = app_handle.try_state::<DbState>() {
                    if let Ok(conn) = state.0.lock() {
                        let _ = crate::repo::detached_sticky::update_position(
                            &conn,
                            slot,
                            pos.x as f64 / scale,
                            pos.y as f64 / scale,
                        );
                    }
                }
            }
        }
    });

    Ok(win)
}

/// 聚焦已存在的浮窗（脱离 icon 再次点击时）
pub fn focus(app: &AppHandle, slot: i64) -> bool {
    let Some(win) = app.get_webview_window(&window_label(slot)) else {
        return false;
    };
    crate::win_taskbar::show(&win);
    let _ = win.set_focus();
    true
}

/// 关闭浮窗并销毁窗口
pub fn destroy(app: &AppHandle, slot: i64) {
    if let Some(win) = app.get_webview_window(&window_label(slot)) {
        let _ = win.close();
    }
}

/// 启动时恢复所有已脱离的浮窗
pub fn restore_all(app: &AppHandle, stickies: &[crate::models::DetachedSticky]) {
    for s in stickies {
        if let Err(e) = create_or_focus(app, s.slot, s.x, s.y, s.always_on_top) {
            log::warn!("恢复浮窗便签 slot={} 失败: {}", s.slot, e);
        } else {
            log::info!("已恢复浮窗便签: slot={} @ ({:?},{:?})", s.slot, s.x, s.y);
        }
    }
}
