mod account;
mod api_spec;
mod autostart;
mod browsers;
mod chat;
mod chat_window;
mod clipboard;
mod commands;
mod config;
mod credentials;
mod countdown_ticker;
mod countdown_window;
mod db;
mod extension;
mod ext_protocol;
mod floating_ball;
mod float_window;
pub mod market;
#[cfg(target_os = "macos")]
mod mac;
mod models;
mod net;
mod notify;
mod online;
mod paths;
mod precheck;
mod process;
mod proxy;
mod publisher;
mod repo;
mod runtime;
mod service;
mod shortcut;
pub mod signing;
mod skills;
mod suda_browser;
mod sticky_window;
mod sysmon;
mod todo_reminder;
mod todo_recurrence;
mod tray;
pub mod updater;
mod webview_mem;
mod win_taskbar;
mod window_resize;
mod mhub_api;

/// WebView2 附加浏览器参数（所有窗口必须完全一致——同一 user data folder 下
/// 不同参数的环境创建会失败；tauri.conf.json 主窗的 additionalBrowserArgs 与此
/// 逐字一致，webview_mem.rs 有守卫测试）。
/// 保留 wry 默认的 --disable-features 前缀；曾带 --disable-background-timer-throttling
/// （禁用后台定时器节流），已摘除：隐藏窗口里的 JS 定时器交还浏览器自动节流
/// （钳到 ≥1s、长期隐藏降到每分钟 1 次）兜底，重量级轮询由 useAdaptivePolling
/// 按可见性/聚焦自行门控。到点类「正事」（倒计时/待办提醒）在 Rust 原生线程，
/// 不受此参数影响。
/// 内存优化 P3 追加两项（微软 WebView2 资源优化指南推荐值）：
/// - --js-flags=--scavenger_max_new_space_capacity_mb=8：压 V8 新生代堆上限，
///   降低 JS 引擎常驻内存；若主窗/编辑器出现 GC 卡顿可放宽到 16–32
/// - --disk-cache-size=33554432：磁盘缓存上限 32MB
pub const ADDITIONAL_BROWSER_ARGS: &str =
    "--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection --js-flags=--scavenger_max_new_space_capacity_mb=8 --disk-cache-size=33554432";

use commands::DbState;
use rusqlite::Connection;
use tauri::{Listener, Manager};

/// 初始化数据库并返回连接
fn init_database() -> Result<Connection, Box<dyn std::error::Error>> {
    let app_data_dir = crate::paths::data_root().to_path_buf();
    std::fs::create_dir_all(&app_data_dir)?;

    // 启动时应用待恢复的数据（恢复命令只暂存，重启后替换）
    apply_pending_restore(&app_data_dir);

    let db_path = app_data_dir.join("app.db");
    let conn = db::init(&db_path)?;
    log::info!("数据库初始化完成: {}", db_path.display());
    Ok(conn)
}

/// 应用待恢复的数据：将 restore.db / restore_icons 替换为正式数据
fn apply_pending_restore(app_data: &std::path::Path) {
    let flag = app_data.join(".restore_pending");
    if !flag.exists() {
        return;
    }
    let db = app_data.join("app.db");
    let restore = app_data.join("restore.db");

    if restore.exists() {
        // ⚠️ 原来是「先删正式库 → copy → 删备份」，且每一步的 `let _ =` 都丢弃错误。
        // copy 失败（磁盘满 / 权限 / I/O）时的后果是：**原库已删、备份也删、**
        // 什么都不剩，而日志还写「已应用待恢复的数据」—— 静默的全量数据丢失。
        //
        // 改为「复制到暂存名 → 校验它确实是合法 SQLite → 同目录 rename 原子替换」。
        // Unix 的 rename(2) 本身就会原子替换目标，**不需要先删原库**。
        // 任何一步失败都**原封不动**保留 原库 + restore.db + .restore_pending，
        // 下次启动自然重试 —— 恢复要么完整成功，要么完全没发生，没有中间态。
        let staged = app_data.join("app.db.restoring");
        let _ = std::fs::remove_file(&staged);

        if let Err(e) = std::fs::copy(&restore, &staged) {
            log::error!(
                "恢复数据库：复制暂存失败（{e}）。**已保留原数据与备份，未做任何改动**，下次启动会重试"
            );
            return;
        }
        if let Err(e) = validate_sqlite(&staged) {
            let _ = std::fs::remove_file(&staged);
            log::error!(
                "恢复数据库：暂存文件不是合法的 SQLite 库（{e}）。已丢弃暂存，**原数据未动**，下次启动会重试"
            );
            return;
        }
        if let Err(e) = std::fs::rename(&staged, &db) {
            log::error!(
                "恢复数据库：替换失败（{e}）。**已保留原数据与备份**，下次启动会重试"
            );
            return;
        }
        // 旧库的 -wal/-shm 属于已被替换掉的那个库，必须清掉，否则下次打开时
        // SQLite 会拿旧的 WAL 去校验新库。放在 rename **之后**：万一 rename 失败，
        // 原库连同它的 WAL 都还在，数据不残缺。SQLite 本身会校验 WAL 的
        // salt/checksum 并忽略不匹配的残留，故这个顺序是安全的。
        let _ = std::fs::remove_file(app_data.join("app.db-wal"));
        let _ = std::fs::remove_file(app_data.join("app.db-shm"));
        // 确认换上去的新库能打开，才敢删备份
        if let Err(e) = validate_sqlite(&db) {
            log::error!("恢复数据库：替换后的库无法打开（{e}）。**已保留备份 restore.db**，请勿删除");
            return;
        }
        let _ = std::fs::remove_file(&restore);
        log::info!("已应用待恢复的数据（数据库）");
    }

    let restore_icons = app_data.join("restore_icons");
    if restore_icons.exists() {
        // 同款问题：原来 remove_dir_all(icons) 之后 rename，rename 失败则图标全灭且不重试。
        // 改为「先搬到旁路 → 换上去 → 再删旧的」，失败时旧图标目录仍完整。
        let icons = app_data.join("icons");
        let old_icons = app_data.join("icons.old");
        let _ = std::fs::remove_dir_all(&old_icons);
        let swap = if icons.exists() {
            std::fs::rename(&icons, &old_icons).is_ok()
                && std::fs::rename(&restore_icons, &icons).is_ok()
        } else {
            std::fs::rename(&restore_icons, &icons).is_ok()
        };
        if swap {
            let _ = std::fs::remove_dir_all(&old_icons);
            log::info!("已应用待恢复的数据（图标）");
        } else {
            // 换不上去就把旧的搬回去，别让用户处于「两个都没有」的状态
            let _ = std::fs::remove_dir_all(&icons);
            if old_icons.exists() {
                let _ = std::fs::rename(&old_icons, &icons);
            }
            log::error!("恢复图标：替换失败。已还原原图标目录，备份仍在，下次启动会重试");
            return;
        }
    }
    let _ = std::fs::remove_file(&flag);
}

/// 打开 SQLite 文件并跑一条查询，确认它不是损坏/截断的文件。
///
/// 存在的意义：恢复流程里 copy 成功**不等于**内容有效（磁盘写满时 copy 可能
/// 返回 Ok 却只写了一半）。不校验就替换的话，用户会拿到一个打不开的库，
/// 而原库已经被覆盖掉了 —— 那比恢复失败严重得多。
fn validate_sqlite(path: &std::path::Path) -> Result<(), String> {
    use rusqlite::OpenFlags;
    let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(|e| e.to_string())?;
    // pragma user_version 会真正读文件头与 b-tree 的一页，截断的库在这里就会露馅
    conn.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
        .map(|_| ())
        .map_err(|e| e.to_string())
}

/// 一次性迁移旧目录（com.workbench.desktop）数据到新目录（m-hub）
fn migrate_legacy_data() {
    let new_dir = crate::paths::data_root().to_path_buf();

    // ⚠️ 守卫原先是 `if new_dir.exists() { return; }`，**恒为真、这段从来没执行过**
    // （2026-09-29 修）。原因是调用顺序：`data_root()` 在本函数之前就已被调用多次，
    // 其中 `paths::write_bootstrap_at` 与资产作用域循环的 `create_dir_all(data_root().join(rel))`
    // 都会**先把新数据根建出来**。于是从 com.workbench.desktop 升级上来的用户，
    // 速记/待办/速达/便签**全部为空**（app.db 是新建的空库），设置全默认，
    // 旧数据完好躺在旧目录里，日志里也不会出现「已从旧目录迁移数据」。
    //
    // 改用「目标根里有没有 app.db」作判据 —— 它才是「这套数据根已经在用」的
    // 真实标志（目录本身会被引导文件、assets 目录等先建出来，不能作数）。
    if new_dir.join("app.db").exists() {
        return;
    }
    // 上次迁移中途失败会留下半截目录；没有标记就重试，覆盖残缺的部分。
    // （有标记则跳过：即使之后用户自己往新目录放了 app.db，也不再动它。）

    let Some(legacy_dir) = dirs::data_dir().map(|d| d.join("com.workbench.desktop")) else {
        return;
    };
    if !legacy_dir.exists() {
        return;
    }
    if std::fs::create_dir_all(&new_dir).is_err() {
        return;
    }
    let mut failures = 0usize;
    if let Ok(entries) = std::fs::read_dir(&legacy_dir) {
        for entry in entries.flatten() {
            let src = entry.path();
            let dst = new_dir.join(entry.file_name());
            // 逐条失败必须落日志：原来全部 `let _ =` 吞掉，迁移失败时用户
            // 只看到「数据空了」，而日志里干净得什么都没有，无从排查。
            let res = if src.is_dir() {
                copy_dir(&src, &dst)
            } else {
                std::fs::copy(&src, &dst).map(|_| ())
            };
            if let Err(e) = res {
                failures += 1;
                log::warn!(
                    "旧数据迁移失败 {} → {}: {e}",
                    src.display(),
                    dst.display()
                );
            }
        }
    }
    if failures > 0 {
        // 不写标记 → 下次启动继续补齐剩下的条目（已拷成功的会被覆盖，可重入）
        log::warn!(
            "旧数据迁移有 {failures} 项失败，将在下次启动重试；已成功的条目会被覆盖，可安全重跑"
        );
    } else {
        let _ = std::fs::write(new_dir.join(".legacy_migrated"), b"1");
    }
    log::info!(
        "已从旧目录迁移数据: {}（失败 {failures} 项）",
        legacy_dir.display()
    );
}

fn copy_dir(src: &std::path::Path, dst: &std::path::Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let s = entry.path();
        let d = dst.join(entry.file_name());
        if s.is_dir() {
            copy_dir(&s, &d)?;
        } else {
            std::fs::copy(&s, &d)?;
        }
    }
    Ok(())
}

/// 将数据库中旧目录（com.workbench.desktop）的图标路径替换为当前目录（m-hub）
/// 幂等：重复执行无副作用，每次启动调用
fn fix_icon_paths(conn: &Connection) {
    let new_dir = crate::paths::data_root();
    let Some(old_dir) = dirs::data_dir().map(|d| d.join("com.workbench.desktop")) else {
        return;
    };
    let old = old_dir.to_string_lossy().into_owned();
    let new = new_dir.to_string_lossy().into_owned();
    // ⚠️ 这里用 `substr(...) = ?` 而不是 `LIKE ? || '%'`（2026-09-29 修）。
    // `LIKE` 模式下 `_` 是「匹配任意单个字符」的通配符，而 macOS 用户名里
    // 带下划线非常常见（`/Users/john_doe/…`）。于是 `LIKE '/Users/john_doe/%'`
    // 还会顺带匹配到 `/Users/johnXdoe/…` 这类**不相干**的路径，
    // 把它们一并 replace 成 m-hub 目录 —— 改坏别的应用的路径记录。
    // `substr(icon, 1, length(?1)) = ?1` 是纯字符串比较，语义精确。
    match conn.execute(
        "UPDATE resources SET icon = replace(icon, ?1, ?2)
           WHERE substr(icon, 1, length(?1)) = ?1",
        rusqlite::params![old, new],
    ) {
        Ok(n) if n > 0 => log::info!("已修复 {} 条图标路径为 m-hub 目录", n),
        _ => {}
    }
}

/// 验证窗口位置是否在任意可用的显示器内
pub(crate) fn is_position_on_screen(x: f64, y: f64) -> bool {
    // Tauri 2 没有直接枚举显示器的 API，使用一个合理的边界检查
    // 允许负坐标（多显示器配置），但限制在合理范围内
    x >= -10000.0 && x <= 10000.0 && y >= -10000.0 && y <= 10000.0
}

/// 主窗句柄统一入口（ADR 0011 起必须走这里）：
/// 主窗自「速达应用内打开网页」起内嵌了子 webview（suda-panel），tauri 的
/// `get_webview_window` 内部校验 is_webview_window（窗口上所有 webview 的 label
/// 必须都等于窗口 label），多 webview 窗口一律返回 None——按它找主窗会让托盘/
/// 快捷键/悬浮球/关闭拦截/几何保存/浮窗定位全部静默失效（实测踩过）。
/// `get_window` 按窗口注册表查 label，不受窗口上有几个 webview 影响。
pub fn main_window(app: &tauri::AppHandle) -> Option<tauri::Window<tauri::Wry>> {
    app.get_window("main")
}


/// 浮窗「首次建窗」的落点：以主窗中心为基准，向右下偏移半个浮窗尺寸 + 少量留白。
///
/// 返回**逻辑**像素 —— 因为 `WindowBuilder::position(x, y)` 的文档明写
/// 「The initial position of the window in **logical pixels**」。
///
/// # 为什么必须是这一个函数（2026-09-29 修）
///
/// 此前有**四份**几乎相同的实现，其中三份是错的：
/// `sticky_window` / `countdown_window` / `float_window` 把主窗的**物理**坐标
/// （`outer_position()`）与浮窗的**逻辑**尺寸直接相加，再把结果喂给收逻辑像素的
/// `position()`。Retina（scale 2.0）上算出来的落点会右偏/下偏接近一倍：
///
/// ```text
/// 主窗 pos=(0,0) size=2928×1928（物理）、便签 260×360（逻辑）
///   期望  x = 0 + 1464 - 260 + 40 = 1244（物理）→ 622（逻辑）
///   实际  x = 0 + 1464 - 130 + 40 = 1374  → 被当逻辑解释 = 物理 2748  ← 偏了一倍多
/// ```
///
/// 症状是便签/倒计时/提示词·待办浮窗**首次弹出就严重偏移**，小屏上直接飞出屏幕
/// 边缘（够不着、拖不回来）。只有 `chat_window` 一份是对的（它显式乘了 `scale`），
/// 正好说明正确写法就在工程里、只是没人抄。
///
/// # 算法
///
/// 在**物理空间**里算中心对齐（避免中途反复换算），最后整体除回 `scale` 变逻辑；
/// 两个留白常量（40/24）本身是「小间距」语义，按逻辑像素计。
pub fn centered_on_main(app: &tauri::AppHandle, width: f64, height: f64) -> Option<(f64, f64)> {
    let main = main_window(app)?;
    if !main.is_visible().unwrap_or(false) {
        return None;
    }
    let scale = main.scale_factor().unwrap_or(1.0).max(0.01);
    let pos = main.outer_position().ok()?;
    let size = main.outer_size().ok()?;
    Some(centered_offset(
        pos.x as f64,
        pos.y as f64,
        size.width as f64,
        size.height as f64,
        width,
        height,
        scale,
    ))
}

/// 浮窗恢复落点的校正：保证结果**永远在某个屏幕的可见范围内**，否则退回主窗中心。
///
/// 入参与返回值都是**逻辑**像素。
///
/// # 为什么需要它（2026-09-29）
///
/// 便签/倒计时浮窗的落点是从库里读回的**历史值**，而历史值有两个来源的问题：
///
/// ① **单位曾经是错的**。落盘时存的是 `outer_position()` —— **物理**像素；
///    恢复时却喂给只收**逻辑**像素的 `WindowBuilder::position()`。
///    这与主窗 `WindowState` 是同一个 bug（约定 74），在 Retina（scale 2.0）上
///    坐标直接翻倍：用户在中间拖好的便签，下次启动就落到屏幕右下角，
///    得再手动拖回来。已把落盘侧改成存逻辑像素，但**库里已有的旧值仍然是错的**。
///
/// ② 用户可能把浮窗拖到了屏幕外、或者摘掉了那块显示器。
///
/// 所以光「统一单位」不够 —— 必须再加一道**边界校验**。
/// 这一道对任何来源的坏值都有效：只要落点不在任何屏幕的工作区里，就地纠正。
///
/// 判定用「窗口中心是否落在某块屏的工作区内」：中心在屏上，用户就能看到并抓住它拖回来；
/// 中心不在屏上则必然够不着，这是**不可恢复**的状态（与主窗飞出屏幕同类）。
/// 找到最近的屏后把落点夹进去（不是置中，避免把用户靠边的便签拽到屏幕正中）。
pub fn sanitize_float_position(
    app: &tauri::AppHandle,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
) -> (f64, f64) {
    let Ok(monitors) = app.available_monitors() else {
        // 显示器枚举失败：宁可保留原落点，也不要在信息不足时乱动用户的窗口
        return (x, y);
    };
    // Tauri 的 Monitor 几何是**物理**像素，逐屏除以各自的 scale 换到逻辑
    let rects: Vec<(f64, f64, f64, f64)> = monitors
        .iter()
        .map(|m| {
            let wa = m.work_area();
            let s = m.scale_factor().max(0.01);
            let l = wa.position.x as f64 / s;
            let t = wa.position.y as f64 / s;
            (l, t, l + wa.size.width as f64 / s, t + wa.size.height as f64 / s)
        })
        .collect();
    if rects.is_empty() {
        return (x, y);
    }
    let cx = x + w / 2.0;
    let cy = y + h / 2.0;
    if rects.iter().any(|(l, t, r, b)| cx >= *l && cx <= *r && cy >= *t && cy <= *b) {
        return (x, y); // 中心在屏上，保持用户拖到的位置
    }
    // 中心离哪块屏最近就夹进哪块屏
    let (l, t, r, b) = rects
        .iter()
        .min_by(|a, b2| {
            let da = (cx - (a.0 + a.2) / 2.0).powi(2) + (cy - (a.1 + a.3) / 2.0).powi(2);
            let db = (cx - (b2.0 + b2.2) / 2.0).powi(2) + (cy - (b2.1 + b2.3) / 2.0).powi(2);
            da.partial_cmp(&db).unwrap_or(std::cmp::Ordering::Equal)
        })
        .copied()
        .unwrap_or((0.0, 0.0, 1920.0, 1080.0));
    // 屏幕比窗口还窄时用 .max() 兜住：宁可超屏也不要算出负的可拖动范围
    let nx = x.clamp(l, (r - w).max(l));
    let ny = y.clamp(t, (b - h).max(t));
    log::info!("浮窗落点 ({x},{y}) 不在任何屏幕内，已纠正到 ({nx},{ny})");
    (nx, ny)
}

/// [`centered_on_main`] 的纯计算部分（抽出以便单测，见 `window_geometry_tests`）。
///
/// 入参 `px/py/sw/sh` 是主窗的**物理**像素，`width/height` 是浮窗的**逻辑**尺寸，
/// 返回**逻辑**像素的落点。
pub fn centered_offset(
    px: f64,
    py: f64,
    sw: f64,
    sh: f64,
    width: f64,
    height: f64,
    scale: f64,
) -> (f64, f64) {
    let scale = scale.max(0.01);
    let x = (px + (sw - width * scale) / 2.0) / scale + 40.0;
    let y = (py + (sh - height * scale) / 2.0) / scale + 24.0;
    (x, y)
}


/// 应用启动时恢复上次保存的窗口位置、尺寸与置顶状态
fn restore_window_state(app: &tauri::App) {
    let config = config::load();
    if let Some(window) = main_window(app.handle()) {
        let ws = &config.window;
        // ws.width/height 是**可视区**尺寸，落进窗口时要补上外扩带（约定 69）
        // 窗口 inner 尺寸**就是**可视区尺寸：外扩带 2026-09-29 移除后没有任何换算
        let _ = window.set_size(tauri::LogicalSize::new(ws.width, ws.height));
        // ws.x/y 是**逻辑**像素（落盘时已从 physical 换算），故这里用 LogicalPosition
        if let (Some(x), Some(y)) = (ws.x, ws.y) {
            if is_position_on_screen(x, y) {
                let _ = window.set_position(tauri::LogicalPosition::new(x as i32, y as i32));
            }
        }
        if ws.always_on_top {
            let _ = window.set_always_on_top(true);
        }
        // 小屏适配：屏幕（按窗口当前所在显示器，取不到回退主显示器）容不下
        // 默认尺寸（`WindowState::default`）时直接最大化启动——否则窗口下半部分掉到屏幕外，没有任何
        // 入口能把窗口拖回来。物理像素先按 DPI 缩放折算成逻辑像素再比较；
        // 比较基准取「默认尺寸与记忆尺寸的较大者」，记忆尺寸更小时也按默认判。
        // ⚠️ 基准要按**窗口**尺寸比（含外扩带），不是可视区：真正决定「放不放得下」
        // 的是窗口框。可视区放得下但窗口框超出屏幕，一样会被菜单栏/Dock 吃掉下半截。
        // ⚠️ 默认值取自 `WindowState::default()` 而非写死字面量：这里曾经硬编码
        // 1400/900，那是 `config.rs` 里同一组数字的**第五份拷贝** —— 只改其中一处，
        // 小屏判断就会静默失准（表现为「换到小屏笔记本上窗口下半截掉出屏幕」）。
        // 写成引用就不存在第五处可漂移。
        let default_size = config::WindowState::default();
        let want_w = ws.width.max(default_size.width);
        let want_h = ws.height.max(default_size.height);
        if let Ok(Some(monitor)) = window.current_monitor().or_else(|_| window.primary_monitor()) {
            let scale = monitor.scale_factor();
            let logical_w = monitor.size().width as f64 / scale;
            let logical_h = monitor.size().height as f64 / scale;
            if logical_w < want_w || logical_h < want_h {
                log::info!(
                    "屏幕逻辑分辨率 {:.0}x{:.0} 小于窗口 {:.0}x{:.0}，启动即最大化",
                    logical_w,
                    logical_h,
                    want_w,
                    want_h
                );
                let _ = window.maximize();
            }
        }
        log::info!(
            "恢复窗口状态: {}x{} @ ({:?},{:?}) 置顶={}",
            ws.width,
            ws.height,
            ws.x,
            ws.y,
            ws.always_on_top
        );
    }
}

/// 保存窗口位置与尺寸到配置
fn persist_window_state(app: &tauri::AppHandle) {
    if let Some(window) = main_window(app) {
        if window.is_minimized().unwrap_or(false) {
            return;
        }
        // 最大化态不覆盖记忆的常规几何：保存的是放大后的整屏尺寸，下次启动
        // 会以「非最大化 + 超大窗口」恢复，反而把窗口撑出屏幕
        if window.is_maximized().unwrap_or(false) {
            return;
        }
        if let Ok(pos) = window.outer_position() {
            if let Ok(size) = window.inner_size() {
                // inner_size() 含外扩带，落盘前扣掉，只存可视区（约定 69）。
                //
                // ⚠️ 位置必须**换算成逻辑像素**再存（2026-09-29 修）。
                // `outer_position()` 返回的是 `PhysicalPosition`（tauri 文档明写），
                // 而恢复侧用的是 `LogicalPosition` —— 两者差一个 `scale_factor`。
                // Windows 上 scale 基本恒为 1，量不出来；macOS Retina 是 2.0，
                // 于是**每退出再启动一次，位置就往右下翻一倍**：
                // 300 → 600 → 1200 → 2400 → 4800（已出屏）。
                // 而 `is_position_on_screen` 只查 ±10000，拦不住，**窗口一旦出屏
                // 就没有任何入口能把它拖回来** —— 症状与约定 8 记的
                // 「窗口下半部分掉到屏幕外」一模一样，但那里归因成了尺寸问题。
                //
                // 为什么存逻辑而不是存物理：同一份 `WindowState` 里的
                // width/height 本来就是逻辑（可视区）像素，x/y 跟着用逻辑
                // 才自洽；而且逻辑坐标在「换一块不同 DPI 的显示器」后仍然有意义，
                // 物理坐标不成立。
                let scale = window.scale_factor().unwrap_or(1.0).max(f64::MIN_POSITIVE);
                // inner 尺寸即可视区尺寸，直接落盘
                let visible = tauri::LogicalSize::new(size.width as f64, size.height as f64);
                let _guard = config::lock();
                let mut cfg = config::load();
                cfg.window.x = Some(pos.x as f64 / scale);
                cfg.window.y = Some(pos.y as f64 / scale);
                cfg.window.width = visible.width;
                cfg.window.height = visible.height;
                match config::save(&cfg) {
                    Ok(()) => log::debug!(
                        "窗口状态已保存: {}x{} @ ({},{}) [逻辑像素]",
                        visible.width,
                        visible.height,
                        cfg.window.x.unwrap_or_default(),
                        cfg.window.y.unwrap_or_default()
                    ),
                    Err(e) => log::warn!("窗口状态保存失败: {}", e),
                }
            }
        }
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        // 单实例：重复双击 exe 时不另起新进程（避免再冷启动一个 WebView2、窗口出现在后台），
        // 而是直接把已运行实例的主窗口唤起并置前
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            crate::tray::show_window(app);
        }))
        .plugin(
            tauri_plugin_log::Builder::default()
                .level(log::LevelFilter::Info)
                // 插件默认时间戳取 now_utc（比本地慢 8 小时，曾让人把上午的日志读成凌晨）；
                // 自带 timezone_strategy(UseLocal) 会把布局改成 [级别][target]，
                // 这里复刻默认布局 [日期][时间][target][级别]，只换成本地时间
                .format(|out, message, record| {
                    let now = chrono::Local::now();
                    out.finish(format_args!(
                        "[{}][{}][{}][{}] {}",
                        now.format("%Y-%m-%d"),
                        now.format("%H:%M:%S"),
                        record.target(),
                        record.level(),
                        message
                    ))
                })
                .targets([
                    tauri_plugin_log::Target::new(tauri_plugin_log::TargetKind::Stdout),
                    tauri_plugin_log::Target::new(tauri_plugin_log::TargetKind::Webview),
                    // 文件日志写到数据根目录（默认 app_log_dir 是 %LOCALAPPDATA%\logs，不符合统一目录约定）
                    tauri_plugin_log::Target::new(tauri_plugin_log::TargetKind::Folder {
                        path: crate::paths::data_root().join("logs"),
                        file_name: Some("m-hub".into()),
                    }),
                ])
                .build(),
        )
        .plugin(tauri_plugin_dialog::init())
        // 笔记图片协议：笔记 Markdown 内嵌 http://mhub-note.localhost/<hash>.<ext>，
        // 按「数据根/notes/images/<文件名>」读取（URL 不含数据根绝对路径，迁数据目录后仍有效）；
        // 文件名严格校验为 16 位十六进制哈希 + 白名单扩展名，杜绝路径穿越
        .register_uri_scheme_protocol("mhub-note", |_ctx, request| {
            let name = request.uri().path().trim_start_matches('/');
            let valid = {
                let mut parts = name.split('.');
                match (parts.next(), parts.next(), parts.next()) {
                    (Some(hash), Some(ext), None) => {
                        hash.len() == 16
                            && hash.chars().all(|c| c.is_ascii_hexdigit())
                            && matches!(
                                ext.to_lowercase().as_str(),
                                "png" | "jpg" | "jpeg" | "webp" | "bmp" | "gif"
                            )
                    }
                    _ => false,
                }
            };
            if !valid {
                return tauri::http::Response::builder()
                    .status(400)
                    .body(Vec::new())
                    .unwrap();
            }
            let mime = match name.rsplit('.').next().unwrap_or("").to_lowercase().as_str() {
                "png" => "image/png",
                "jpg" | "jpeg" => "image/jpeg",
                "webp" => "image/webp",
                "bmp" => "image/bmp",
                "gif" => "image/gif",
                _ => "application/octet-stream",
            };
            match std::fs::read(crate::paths::data_root().join("notes").join("images").join(name))
            {
                Ok(bytes) => tauri::http::Response::builder()
                    .header("Content-Type", mime)
                    .header("Access-Control-Allow-Origin", "*")
                    // 内容哈希命名 ⇒ 同名即同内容，可长缓存
                    .header("Cache-Control", "public, max-age=31536000, immutable")
                    .body(bytes)
                    .unwrap(),
                Err(_) => tauri::http::Response::builder()
                    .status(404)
                    .body(Vec::new())
                    .unwrap(),
            }
        })
        // 扩展内容协议：扩展入口与其相对资源的唯一来源。
        // origin = `mhub-ext.localhost`，与承载用户数据的 `asset.localhost` 跨源，
        // 扩展因此无法直接读取数据根下的数据库 / 配置（见 docs/adr/0008）
        .register_uri_scheme_protocol("mhub-ext", |ctx, request| {
            crate::ext_protocol::handle(ctx.app_handle(), request)
        })
        .setup(|app| {
            log::info!("========== m-hub 启动 ==========");

            // 资产协议作用域：**只**放行必要的子目录（图标 / 壁纸 / 剪贴板图片 / 扩展），
            // 绝不放行整个数据根，也绝不写回 tauri.conf 的 `$APPDATA/**`。
            //
            // 理由（见 docs/adr/0008-extension-content-origin-isolation.md）：资产协议作用域
            // 是**全局单例**，而扩展 iframe 与资产资源同源 ⇒ 放行数据根等于任何扩展都能直接
            // fetch 到用户数据库（mhub.db）、app.json 与日志，从而绕开桥 API 的权限系统。
            // 数据目录可被改到 %APPDATA% 之外（自定义目录 / U 盘便携），故必须动态放行。
            const ASSET_SCOPE_SUBDIRS: [&str; 3] =
                ["icons", "wallpapers", "clipboard/images"];
            for rel in ASSET_SCOPE_SUBDIRS {
                let dir = crate::paths::data_root().join(rel);
                // 目录必须先存在：allow_directory 会额外注册 canonicalize 后的模式变体，
                // 目录不存在时拿不到该变体，请求侧 canonicalize 后就匹配不上（403）。
                if let Err(e) = std::fs::create_dir_all(&dir) {
                    log::warn!("资产作用域目录创建失败 {}: {e}", dir.display());
                }
                if let Err(e) = app.asset_protocol_scope().allow_directory(&dir, true) {
                    log::warn!("资产作用域放行失败 {}: {e}", dir.display());
                }
            }

            // 旧版本（com.workbench.desktop 标识）数据迁移到 m-hub 目录
            migrate_legacy_data();

            // 升级自替换非常早期执行：必须在数据库/其他句柄持有 exe 相关资源前，
            // 且仅在真正待应用时才做替换（幂等）。失败只记日志不阻断启动。
            updater::apply_pending_update(app.handle(), &app.package_info().version.to_string());

            let conn = init_database()?;
            fix_icon_paths(&conn);
            app.manage(DbState(std::sync::Mutex::new(conn)));
            app.manage(clipboard::ClipboardState::default());
            app.manage(service::ServiceState::default());
            // 开发扩展目录映射（「我的扩展」登记；扩展协议与扫描按它解析源码目录）
            app.manage(ext_protocol::DevExtensionDirs::default());
            // 本机源码目录重放：资产作用域放行不落盘，重启必须按配置重新放行
            extension::apply_dev_extensions(app.handle());

            // 账号会话启动校验（异步，不阻塞窗口创建）：token 失效则静默清理
            let account_handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                account::verify_session_on_startup(&account_handle).await;
            });

            // 启动扩展反向代理（/svc/<extId>/* → 127.0.0.1:<service 端口>，统一加 CORS 头）
            let proxy_port = tauri::async_runtime::block_on(proxy::start(app.handle().clone()))
                .unwrap_or_else(|e| {
                    log::warn!("扩展反向代理启动失败: {e}");
                    0
                });
            app.manage(proxy::ProxyState::new(proxy_port));

            tray::setup(app)?;
            shortcut::setup(app)?;

            restore_window_state(app);

            // 自启动静默模式（--autostart-hidden）：主窗不显示、直接驻留托盘。
            // 必须经 tray::hide_window 更新自维护的显隐状态位，否则 MAIN_WINDOW_VISIBLE
            // 保持默认 true，全局快捷键 toggle_window 会误判「窗口可见」，只 set_focus
            // 隐藏窗口上无效调用，导致快捷键无法呼出主窗口。
            if crate::autostart::is_hidden_launch() {
                crate::tray::hide_window(app.handle());
            }

            // 桌面悬浮球（ADR 0004）：须在 autostart-hidden 的 hide_window 之后初始化，
            // 显隐联动（主窗隐藏 → 球显示）才能覆盖静默启动场景
            floating_ball::init(app.handle());

            // 主窗口启动时隐藏（tauri.conf.json visible:false），等前端内容可绘制后再 show，
            // 避免 WebView2 冷启动期间出现空白/白屏等待窗口；这里先铺上主题底色，
            // 若 show 早于首帧绘制，也只会闪主题色而非纯白
            // 主题三态：dark，或 system 且系统偏好深色 → 暗色底色；light / 系统浅色 → 亮色
            let config = config::load();
            // 开机自启动自愈：用户开了自启动、但 Run 键因换目录/被清理工具删除/路径变更而失效时，
            // 按当前 exe 重写。放在主窗显示前，静默修复，不阻断启动。
            if config.run_at_startup {
                match autostart::ensure_registered() {
                    // 自愈详情已由 ensure_registered 内部记录（键缺失/路径不符两种），
                    // 这里不再重复写一条
                    Ok(true) => {}
                    Ok(false) => {}
                    Err(e) => log::warn!("开机自启动自愈失败: {e}"),
                }
            }
            // 主窗本体是**透明**的（tauri.conf 的 transparent:true，窗口圆角靠
            // .app-shell 的 border-radius 裁出，约定 69），因此窗口/webview 侧的底色
            // 必须是全透明 —— 这里是过去**唯一**能把圆角填成不透明矩形的地方：
            // 哪怕 CSS 侧全对，只要这里 alpha=255，圆角处就会是一块纯色方角。
            //
            // 亮/暗的底色由 CSS 侧的 `--app-bg` / `.app-shell` 承担（那里本来就在
            // 画渐变），窗口层只需要「什么都不画」。
            let bg = tauri::window::Color(0, 0, 0, 0);
            if let Some(window) = main_window(app.handle()) {
                let _ = window.set_background_color(Some(bg));
                // WebviewWindow::set_background_color 原本同时铺窗口与 webview 两侧底色，
                // Window 版只铺窗口侧，这里补 webview 侧保持原行为（get_webview 按
                // label 查 webview 表，多 webview 窗口不受影响）
                if let Some(webview) = app.get_webview("main") {
                    let _ = webview.set_background_color(Some(bg));
                }
                // 启动即前台：避免窗口偶尔出现在其他窗口后面；稍等再补一次焦点，
                // 绕开 Windows 前台锁定的瞬时限制
                let _ = window.set_focus();
                let handle = window.clone();
                std::thread::spawn(move || {
                    std::thread::sleep(std::time::Duration::from_millis(250));
                    let _ = handle.set_focus();
                });
            }

            // 恢复上次已脱离的浮窗便签（位置/置顶/内容均持久化）
            {
                let state = app.state::<DbState>();
                let conn = state.0.lock().map_err(|e| e.to_string())?;
                let detached = crate::repo::detached_sticky::list(&conn).unwrap_or_default();
                drop(conn);
                sticky_window::restore_all(app.handle(), &detached);
            }

            // 恢复上次已浮起的倒计时浮窗（浮起状态 + 位置持久化）
            {
                let state = app.state::<DbState>();
                let conn = state.0.lock().map_err(|e| e.to_string())?;
                let floated = crate::repo::countdown::list_floated(&conn).unwrap_or_default();
                drop(conn);
                countdown_window::restore_all(app.handle(), &floated);
            }

            // 工作台倒计时卡片可见性门控：默认不可见，待前端按已提交布局上报后放开，
            // 避免启动早期（前端未就绪/卡片实际不在布局中）倒计时抢跑到点
            app.manage(countdown_ticker::CardVisible(
                std::sync::atomic::AtomicBool::new(false),
            ));

            // 启动倒计时后台驱动线程（每秒扫描到期项，托盘/隐藏时不受 WebView 节流影响）
            countdown_ticker::start(app.handle().clone());

            // 启动待办提醒后台线程（remind_at 到点发系统通知 + 前端 toast）
            todo_reminder::start(app.handle().clone());

            // 启动剪贴板监听线程（启动零加载历史，仅剪贴板变化时落库）
            clipboard::start_monitor(app.handle().clone());

            // 启动剪贴板延迟窗口操作 worker（粘贴/归还焦点统一串行执行，避免频繁 spawn 短命线程）
            clipboard::init_win_op_worker();

            // 预创建剪贴板浮层窗口（隐藏常驻）：运行时现场创建 WebView2 窗口曾与
            // 悬浮球操作交错导致整窗未响应（见 clipboard.rs::init_overlay_window 注释）
            clipboard::init_overlay_window(app.handle());

            // 预创建通知窗（隐藏常驻）：右下角自绘通知，跨 Win10/11 一致（详见 notify.rs）
            notify::init(app.handle());

            // AI 对话独立窗口（无条件预创建隐藏常驻，运行期绝不建窗，详见 chat_window.rs）
            chat_window::init(app.handle());

            // 速达「应用内打开网页」：主窗内嵌面板 webview + 独立浏览器窗口池×1
            // （全部页面收进 tab；无条件预创建隐藏常驻，运行期只 show/hide/navigate/set_bounds，见 suda_browser.rs）
            suda_browser::init(app.handle());

            // WebView2 内存级别联动（P0，webview_mem.rs）：窗口隐藏时把常驻 renderer
            // 的内存目标级别设为 Low（弃缓存换页、脚本照常跑），显示前恢复 Normal。
            // 快路径接在各显隐函数里，这里起的是 300ms 轮询纠偏兜底线程
            webview_mem::init(app.handle());

            // 关闭事件：拦截默认关闭，改为隐藏至托盘
            if let Some(window) = main_window(app.handle()) {
                let app_handle = app.handle().clone();
                let win_for_events = window.clone();
                window.on_window_event(move |event| {
                    match event {
                        tauri::WindowEvent::CloseRequested { api, .. } => {
                            log::info!("收到关闭请求：保存状态并隐藏至托盘");
                            persist_window_state(&app_handle);
                            api.prevent_close();
                            crate::tray::hide_window(&app_handle);
                        }
                        // 最小化/还原联动悬浮球：最小化也是主窗「视觉不可见」，
                        // 球应出现。Windows 上最小化状态变化伴随 Resized 事件，借此
                        // 检测（MAIN_WINDOW_VISIBLE 状态位只覆盖托盘/快捷键显隐链）
                        tauri::WindowEvent::Resized(_) => {
                            let minimized = win_for_events.is_minimized().unwrap_or(false);
                            crate::floating_ball::set_main_minimized(&app_handle, minimized);
                        }
                        _ => {}
                    }
                });
            }

            // 全局快捷键事件：切换主窗口显示/隐藏
            let app_handle = app.handle().clone();
            app.listen("global-shortcut-toggle", move |_| {
                crate::tray::toggle_window(&app_handle);
            });

            // 剪贴板快捷键事件：唤起/收起剪贴板历史浮层
            let app_handle = app.handle().clone();
            app.listen("clipboard-toggle", move |_| {
                crate::clipboard::toggle_overlay(&app_handle);
            });

            // 搜索快捷键事件：唤起主窗（对话框由主窗前端收到同名事件后打开）
            let app_handle = app.handle().clone();
            app.listen("search-shortcut", move |_| {
                crate::tray::show_window(&app_handle);
            });

            // AI 对话快捷键事件：抽屉形态先唤起主窗（面板在主窗里）；
            // 独立窗口形态不弹主窗，由主窗前端收到事件后直接唤起对话小窗
            let app_handle = app.handle().clone();
            app.listen("chat-shortcut", move |_| {
                if !crate::chat_window::mode_enabled() {
                    crate::tray::show_window(&app_handle);
                }
            });

            // 静默检查更新：启动 5s 后一次，此后按配置间隔（默认 4h）循环。
            // 受 auto_update_enabled 开关控制；检查失败静默（updater 内部记日志）。
            {
                let handle = app.handle().clone();
                tauri::async_runtime::spawn(async move {
                    tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                    loop {
                        {
                            let cfg = crate::config::load();
                            if cfg.auto_update_enabled {
                                let _ = updater::check_for_update(handle.clone(), None).await;
                            }
                        }
                        let hours = crate::config::load().update_interval_hours.max(1);
                        tokio::time::sleep(std::time::Duration::from_secs(hours * 3600)).await;
                    }
                });
            }

            log::info!("m-hub 启动完成");
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_initial_data,
            commands::create_resource,
            commands::update_resource,
            commands::delete_resource,
            commands::reorder_resources,
            commands::launch_resource,
            commands::launch_resource_as_admin,
            commands::list_installed_browsers,
            commands::open_url_with_browser,
            commands::create_note,
            commands::update_note,
            commands::delete_note,
            commands::list_notes,
            commands::list_todos,
            commands::create_todo,
            commands::toggle_todo,
            commands::update_todo,
            commands::delete_todo,
            commands::schedule_todo,
            commands::reorder_todo_orders,
            commands::move_todo_child,
            commands::set_todo_description,
            commands::set_todo_pinned,
            commands::set_todo_repeat,
            commands::complete_todo_recurring,
            commands::undo_todo_recurring,
            commands::expand_todo_occurrences,
            commands::list_todo_tags,
            commands::create_todo_tag,
            commands::update_todo_tag,
            commands::delete_todo_tag,
            commands::set_todo_tags,
            commands::list_todo_tag_links,
            commands::list_stickies,
            commands::save_sticky,
            commands::get_detached_stickies,
            commands::detach_sticky,
            commands::focus_detached_sticky,
            commands::save_detached_sticky,
            commands::toggle_detached_sticky_pin,
            commands::restore_detached_sticky,
            commands::delete_detached_sticky,
            commands::list_countdowns,
            commands::create_countdown,
            commands::update_countdown,
            commands::delete_countdown,
            commands::pause_countdown,
            commands::resume_countdown,
            commands::float_countdown,
            commands::unfloat_countdown,
            commands::set_countdown_card_visible,
            commands::list_snippets,
            commands::create_snippet,
            commands::update_snippet,
            commands::delete_snippet,
            commands::toggle_snippet_pin,
            commands::record_snippet_copy,
            commands::toggle_prompt_float,
            commands::toggle_todo_float,
            commands::toggle_float_pin,
            commands::search_all,
            commands::save_config,
            commands::set_window_always_on_top,
            commands::set_always_on_top_config,
            commands::get_global_shortcut,
            commands::set_global_shortcut,
            commands::set_search_shortcut,
            commands::set_chat_shortcut,
            commands::get_capture_shortcut,
            commands::set_capture_shortcut,
            commands::get_run_at_startup,
            commands::set_run_at_startup,
            commands::get_startup_hidden,
            notify::notice_ready,
            notify::notice_layout,
            notify::notice_dismiss_window,
            commands::log_client_error,
            commands::minimize_window,
            commands::toggle_maximize,
            commands::window_resize_begin,
            commands::hide_to_tray,
            commands::parse_dropped_path,
            commands::import_icon_file,
            commands::import_wallpaper,
            commands::cleanup_wallpapers,
            commands::import_note_image,
            commands::inspect_path,
            commands::scan_installed_apps,
            commands::get_running_processes,
            commands::list_tags,
            commands::create_tag,
            commands::delete_tag,
            commands::get_note_tags,
            commands::set_note_tags,
            commands::list_note_tags,
            commands::backup_data,
            commands::restore_data,
            commands::get_data_path,
            commands::change_data_dir,
            commands::restart_app,
            commands::list_chat_sessions,
            commands::create_chat_session,
            commands::delete_chat_session,
            commands::rename_chat_session,
            commands::set_chat_session_model,
            commands::list_chat_messages,
            commands::send_chat_message,
            commands::get_chat_models,
            commands::save_chat_models,
            commands::fetch_chat_provider_models,
            commands::get_chat_api_key,
            commands::set_chat_panel,
            commands::get_chat_panel,
            commands::set_chat_panel_side,
            commands::get_ui_config,
            chat_window::chat_window_get_state,
            chat_window::chat_window_toggle,
            chat_window::chat_window_close,
            chat_window::chat_window_set_pinned,
            chat_window::chat_window_save_mode,
            chat_window::chat_window_open_settings,
            suda_browser::suda_browser_open,
            suda_browser::suda_browser_open_url,
            suda_browser::suda_browser_open_tab,
            suda_browser::suda_browser_activate_tab,
            suda_browser::suda_browser_close_tab,
            suda_browser::suda_browser_close,
            suda_browser::suda_browser_navigate,
            suda_browser::suda_browser_back,
            suda_browser::suda_browser_forward,
            suda_browser::suda_browser_reload,
            suda_browser::suda_browser_open_system,
            suda_browser::suda_browser_chrome_height,
            suda_browser::suda_browser_state,
            suda_browser::suda_browser_slots,
            suda_browser::suda_panel_show,
            suda_browser::suda_panel_bounds,
            suda_browser::suda_panel_hide,
            suda_browser::suda_panel_navigate,
            suda_browser::suda_panel_back,
            suda_browser::suda_panel_forward,
            suda_browser::suda_panel_reload,
            commands::list_subcategories,
            commands::create_subcategory,
            commands::rename_subcategory,
            commands::delete_subcategory,
            commands::reorder_subcategories,
            commands::set_default_subcategory,
            commands::set_suda_web_open_mode,
            commands::get_app_info,
            commands::clipboard_list,
            commands::clipboard_copy,
            commands::clipboard_paste,
            commands::clipboard_toggle_pin,
            commands::clipboard_delete,
            commands::clipboard_clear,
            commands::clipboard_set_paused,
            commands::clipboard_activate,
            commands::clipboard_hide,
            commands::set_clipboard_paste_method,
            commands::set_clipboard_media_enabled,
            commands::clipboard_export_image,
            commands::clipboard_get_info,
            commands::set_clipboard_shortcut,
            commands::set_clipboard_retention,
            sysmon::get_system_info,
            extension::list_extensions,
            extension::extensions_stamp,
            extension::read_extension_entry,
            extension::open_extension_window,
            extension::open_extension_dir,
            extension::uninstall_extension,
            extension::get_extension_permissions,
            extension::set_extension_permission,
            extension::get_dev_mode_status,
            extension::add_dev_extension,
            extension::remove_dev_extension,
            extension::dev_extensions_stamp,
            // 扩展开发技能包（Skills）：内置 m-hub-extension 一键装到本机 AI 助手的 skills 目录
            skills::get_skill_overview,
            skills::install_skill,
            skills::uninstall_skill,
            skills::remove_skill_root,
            // 平台账号（登录 / 额度 / 开发者申请）
            // 注意：服务端地址内置为常量（见 config::DEFAULT_SERVER_URL），无 account_set_server 命令
            account::account_status,
            account::account_login_github_start,
            account::account_login_github_poll,
            account::account_login_email_send,
            account::account_login_email_verify,
            account::account_logout,
            account::account_redeem,
            account::dev_apply,
            account::dev_apply_status,
            account::account_list_devices,
            account::account_revoke_device,
            // 扩展发布（打包上传 / 我的提交 / 撤回）
            publisher::dev_submit,
            // 发布弹窗的截图缩略图预览（读本地图为 data URL）
            publisher::read_image_data_url,
            publisher::dev_list_submissions,
            publisher::dev_get_submission,
            publisher::dev_withdraw_submission,
            // 发布前本地预检（作者侧 lint：只做已开源口径的检查，不作为放行依据）
            precheck::precheck_extension,
            // 平台 AI 额度（登录后可直接使用的模型列表）
            chat::platform_models,
            market::get_market_registry,
            market::refresh_market_registry,
            market::install_from_market,
            market::install_local_archive,
            market::pack_extension_archive,
            market::update_extension,
            updater::check_for_update,
            updater::download_update,
            updater::get_update_status,
            updater::skip_update_version,
            updater::snooze_update,
            process::open_external,
            mhub_api::mhub_call,
            commands::check_connectivity,
            commands::get_weather,
            commands::get_quote,
            commands::set_weather_city,
            commands::locate_weather_by_ip,
            floating_ball::floating_ball_get_state,
            floating_ball::floating_ball_save_settings,
            floating_ball::floating_ball_drag_begin,
            floating_ball::floating_ball_drag_cancel,
            floating_ball::floating_ball_expand,
            floating_ball::floating_ball_trigger,
            floating_ball::floating_ball_context_menu,
            floating_ball::floating_ball_reapply,
            commands::get_theme_config,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app, event| match event {
            // 程序坞图标点开（macOS）：`applicationShouldHandleReopen:` 由 Tauri 转成本事件。
            //
            // ⚠️ 这一段原先**完全缺失**，症状正是「点红绿灯关闭后，再点程序坞图标
            // 打不开主页面，必须去点菜单栏/托盘图标才行」。
            // 原因：点红绿灯走 CloseRequested → prevent_close + 隐藏窗口，
            // 进程仍在跑但**一个可见窗口都没有**；此时点程序坞图标，系统发的是
            // Reopen，而不是「启动新进程」。事件没人处理 → 什么都不会发生。
            //
            // 光 show_window() 还不够：最后一个窗口隐藏后 NSApplication 处于
            // 「无可见窗口」状态，系统回焦规则不会自动把本应用拉到前台，
            // 于是即便窗口 show 了也像是没反应 —— 必须显式 activate 一次。
            //
            // `has_visible_windows == true` 时不把主窗拽出来：用户可能只是在点
            // 程序坞图标想唤起菜单栏、或让已开着的窗口前置，不该顺手改窗口显隐。
            #[cfg(target_os = "macos")]
            tauri::RunEvent::Reopen {
                has_visible_windows, ..
            } => {
                // ⚠️ 判据**必须问主窗自己**，不能用事件的 `has_visible_windows`
                // （2026-09-29 修：第一版就栽在这里，实测日志连续 8 次
                //   has_visible_windows=true，一次都没进 show_window 分支）。
                // 原因：那个字段的含义是「**本 App 有没有任意可见窗口**」，
                // 而**悬浮球**一直是可见的 —— 它不随主窗隐藏而隐藏。
                // 所以只要开了悬浮球，这个字段就恒为 true，
                // 「主窗已隐藏至托盘」这个真正需要处理的情况反而被漏掉，
                // 表现就是：点红绿灯关闭后再点程序坞图标，**什么都不发生**。
                //
                // 正确判据分三种，与 tray::show_window 覆盖的范围一致：
                //   ① 不可见（隐藏至托盘）→ show + unminimize + focus
                //   ② 可见但最小化          → 同上（show_window 里带 unminimize）
                //   ③ 可见且未最小化        → 只激活，把它拉到前台即可，
                //                              **不要**顺手改窗口显隐
                let main = crate::main_window(app);
                let visible = main
                    .as_ref()
                    .and_then(|w| w.is_visible().ok())
                    .unwrap_or(false);
                let minimized = main
                    .as_ref()
                    .and_then(|w| w.is_minimized().ok())
                    .unwrap_or(false);
                log::info!(
                    "收到程序坞重新打开：has_visible_windows={has_visible_windows}                      主窗可见={visible} 已最小化={minimized}"
                );
                // 两个信号取「或」：窗口自身的 is_visible（权威但依赖 AppKit）
                // 与本工程自维护的显隐状态（tray.rs 用自己的 show/hide 调用维护，
                // 不依赖系统接口 —— 那边注释写了「在多屏/远程桌面/DPI 缩放下
                // 系统接口会返回不准确的值」，这正是不能只信单一来源的理由）。
                let tracked_visible = tray::is_main_window_visible();
                if !visible || minimized || !tracked_visible {
                    log::info!("程序坞重开 → 显示主窗（visible={visible} minimized={minimized} tracked={tracked_visible}）");
                    tray::show_window(app);
                } else {
                    // 已开着就只是「拉到前台」，不重复 show（避免窗口闪一下）
                    log::info!("程序坞重开 → 主窗已开着，仅拉到前台");
                    if let Some(w) = main {
                        let _ = w.set_focus();
                    }
                }
                // 无论哪种都要激活：最后一个窗口隐藏后 NSApplication 处于
                // 「无可见窗口」状态，系统回焦规则不会自动把本应用拉到前台，
                // 不显式 activate 的话点了图标像是完全无响应。
                mac::activate_self();
            }
            // 宿主退出：停止所有 service 后端进程，避免 Node 子进程残留
            tauri::RunEvent::Exit => {
                service::stop_all(app);
                // 先显式移除托盘图标：set_visible/destroy 那类 API 会 run_on_main_thread 回投到
                // 事件循环（本回调正在主线程里跑，投回去没人处理，就是当年「退出流程被拖死」的
                // 真凶）；remove_tray_by_id 是同步移除 + 析构发 Shell_NotifyIcon(NIM_DELETE)，
                // 不依赖事件循环，既不解架也消掉 Explorer 里悬停才清的幽灵图标。
                app.remove_tray_by_id("main-tray");
                // Windows 上 WebView2 子窗口销毁偶发把退出流程拖死（托盘点「退出」
                // 后进程不消失），清理完成后直接结束进程，保证退出 100% 生效
                std::process::exit(0);
            }
            // 其余事件（窗口事件转发、主题变化等）无需在此处理
            _ => {}
        });
}

