use crate::commands::DbState;
use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindow};

/// 倒计时浮窗 label 前缀；完整 label 为 `countdown-{id}`
pub const COUNTDOWN_WINDOW_PREFIX: &str = "countdown-";

/// 浮窗固定尺寸（v0.1.15 进一步缩小并增强透明感：150×170 → 120×132）
pub const COUNTDOWN_WIDTH: f64 = 120.0;
pub const COUNTDOWN_HEIGHT: f64 = 132.0;

pub fn window_label(id: i64) -> String {
    format!("{}{}", COUNTDOWN_WINDOW_PREFIX, id)
}

fn parse_id(label: &str) -> Option<i64> {
    label
        .trim_start_matches(COUNTDOWN_WINDOW_PREFIX)
        .parse::<i64>()
        .ok()
}
/// 浮窗初始落点：主窗中心略偏右下。**单位与算法统一由
/// `lib.rs::centered_on_main` 提供**（此前此处有三份各自为政的实现，
/// 其中把物理坐标与逻辑尺寸混算，Retina 上落点偏近一倍）。
fn initial_position(app: &AppHandle) -> Option<(f64, f64)> {
    crate::centered_on_main(app, COUNTDOWN_WIDTH as f64, COUNTDOWN_HEIGHT as f64)
}

/// 创建（或聚焦）倒计时浮窗。已存在同 label 窗口时复用。
/// 传入已保存的位置；未保存则出现在主窗口中心附近。
pub fn create_or_focus(
    app: &AppHandle,
    id: i64,
    x: Option<f64>,
    y: Option<f64>,
) -> tauri::Result<WebviewWindow> {
    let label = window_label(id);
    if let Some(win) = app.get_webview_window(&label) {
        // 显示走 win_taskbar：tao 的 hide/show 会重建 ex-style 把 WS_EX_APPWINDOW
        // 加回来，任务栏随即多出一颗按钮（见 win_taskbar 模块注释）
        crate::win_taskbar::show(&win);
        let _ = win.set_focus();
        return Ok(win);
    }

    let (pos_x, pos_y) = match (x, y) {
        (Some(px), Some(py)) => {
            // 同 sticky_window：恢复落点要**校正**（库里可能有单位错误的旧值，
            // 也可能被拖出屏幕），详见 lib.rs::sanitize_float_position
            let (ax, ay) = crate::sanitize_float_position(
                app, px, py, COUNTDOWN_WIDTH, COUNTDOWN_HEIGHT,
            );
            if (ax - px).abs() > 0.5 || (ay - py).abs() > 0.5 {
                if let Some(state) = app.try_state::<DbState>() {
                    if let Ok(conn) = state.0.lock() {
                        let _ = crate::repo::countdown::update_position(&conn, parse_id(&label).unwrap_or(0), ax, ay);
                    }
                }
            }
            (ax, ay)
        }
        _ => initial_position(app).unwrap_or((240.0, 200.0)),
    };

    let builder = tauri::WebviewWindowBuilder::new(
        app,
        &label,
        WebviewUrl::App("index.html".into()),
    )
    .title("倒计时")
    .inner_size(COUNTDOWN_WIDTH, COUNTDOWN_HEIGHT)
    .resizable(false)
    .decorations(false)
    .transparent(true)
    .always_on_top(true)
    .skip_taskbar(true)
    .visible(true)
    .position(pos_x, pos_y)
    .additional_browser_args(crate::ADDITIONAL_BROWSER_ARGS);

    #[cfg(target_os = "windows")]
    {
        builder = builder.shadow(false);
    }

    let win = builder.build()?;
    crate::win_taskbar::apply(&win);

    // 移动时持久化位置
    let app_handle = app.clone();
    let moved_win = win.clone();
    win.on_window_event(move |event| {
        if let tauri::WindowEvent::Moved(_) = event {
            if let Some(id) = parse_id(&moved_win.label()) {
                if let Ok(pos) = moved_win.outer_position() {
                    // ⚠️ 换算成**逻辑**像素再落盘（2026-09-29 修）。
                    // `outer_position()` 是 PhysicalPosition，而恢复侧
                    // `WindowBuilder::position()` 收逻辑像素 —— 差一个 scale_factor，
                    // Retina 上等于把坐标翻倍，拖到中间的浮窗下次启动就跑到屏幕右下角。
                    let scale = moved_win.scale_factor().unwrap_or(1.0).max(0.01);
                    if let Some(state) = app_handle.try_state::<DbState>() {
                        if let Ok(conn) = state.0.lock() {
                            let _ = crate::repo::countdown::update_position(
                                &conn,
                                id,
                                pos.x as f64 / scale,
                                pos.y as f64 / scale,
                            );
                        }
                    }
                }
            }
        }
    });

    Ok(win)
}

/// 关闭浮窗并销毁窗口
pub fn destroy(app: &AppHandle, id: i64) {
    if let Some(win) = app.get_webview_window(&window_label(id)) {
        let _ = win.close();
    }
}

/// 启动时恢复所有已浮起的倒计时
pub fn restore_all(app: &AppHandle, countdowns: &[crate::models::Countdown]) {
    for c in countdowns {
        if let Err(e) = create_or_focus(app, c.id, c.float_x, c.float_y) {
            log::warn!("恢复倒计时浮窗 id={} 失败: {}", c.id, e);
        } else {
            log::info!(
                "已恢复倒计时浮窗: id={} name={} @ ({:?},{:?})",
                c.id,
                c.name,
                c.float_x,
                c.float_y
            );
        }
    }
}
