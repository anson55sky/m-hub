//! 窗口缩放拖拽（macOS 上自己实现）
//!
//! # 为什么不用 `startResizeDragging`
//!
//! 两个独立原因叠加，导致 macOS 上**任何窗口都完全无法缩放**（Windows 拖右下角即可）：
//!
//! 1. tao 在 macOS 上的 `drag_resize_window` 恒返回 `NotSupported`
//!    （`tao-0.35.3/src/platform_impl/macos/window.rs:963`，参数 `_direction`
//!    连读都没读）。Linux / Windows 都有实现。
//! 2. tao 给无边框窗口用的是 `NSWindowStyleMaskBorderless | Resizable`
//!    （同文件 `:217`），而 **AppKit 的 borderless 窗口没有系统缩放边** ——
//!    那个透明的可拖边框是 `Titled` 窗口才有的东西。
//!
//! 也就是说留着「8 向手柄 + resize 光标」会变成最坏的一种状态：
//! **光标明确承诺了这里能拖，点下去什么都不发生**。比没有手柄更糟。
//!
//! # 做法
//!
//! 沿用悬浮球边缘监视的同款手法：mousedown 后起一个后台线程，轮询
//! `mac::cursor_physical()` / `mac::lmb_down()`，边移动边改几何，松手即止。
//! 之所以必须轮询而不能在网页里监听 `mousemove`：指针一旦移出窗口
//! （拖右边缘时必然移出），网页就再也收不到事件 —— 这正是
//! `startResizeDragging` 这类**系统级** API 存在的理由。
//!
//! # 单位
//!
//! 全篇**物理像素**。`outer_position()` / `inner_size()` / `monitor.work_area()`
//! 在 Tauri 两平台都返回物理像素（AGENTS.md 约定 P3），光标走
//! `mac::cursor_physical()`（内部已按 backing scale 换算），三者同单位，
//! 不需要任何换算 —— 换算正是这个项目里最容易出错的地方（P3 列的三个坑）。

use std::time::Duration;

/// 轮询间隔。16ms ≈ 一帧，缩放跟手；只在拖拽期间运行，松手即退出。
const TICK: Duration = Duration::from_millis(16);
/// 等左键按下的最长时间。前端是 mousedown 调的，但命令送达前用户可能已松手
/// （快速点一下边缘），此时若直接进入主循环会读到「已松开」并立刻退出，
/// 表现为「偶尔拖不动」—— 竞态，必须给一个短窗口确认按钮确实按着。
const ARM_TIMEOUT: Duration = Duration::from_millis(400);

/// 缩放方向。八个取值与 Tauri 的 `ResizeDirection` 同构（前端传小驼峰字符串）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Edges {
    pub north: bool,
    pub south: bool,
    pub east: bool,
    pub west: bool,
}

/// 解析方向字符串。未知取值返回 `None`（由调用方转成 Err，不静默当默认方向）。
pub fn parse_edges(direction: &str) -> Option<Edges> {
    Some(match direction.trim().to_ascii_lowercase().as_str() {
        "north" => Edges { north: true, ..Default::default() },
        "south" => Edges { south: true, ..Default::default() },
        "east" => Edges { east: true, ..Default::default() },
        "west" => Edges { west: true, ..Default::default() },
        "northeast" => Edges { north: true, east: true, ..Default::default() },
        "northwest" => Edges { north: true, west: true, ..Default::default() },
        "southeast" => Edges { south: true, east: true, ..Default::default() },
        "southwest" => Edges { south: true, west: true, ..Default::default() },
        _ => return None,
    })
}

/// 记录一次缩放拖拽的起点状态
struct DragStart {
    edges: Edges,
    cursor: (i32, i32),
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    /// 起始所在显示器的工作区（物理像素），用于把窗口夹在屏幕内
    work: (f64, f64, f64, f64),
}

/// 按下边缘后调用。**立即返回**，实际拖拽在后台线程里进行。
///
/// 取不到起点状态（窗口正忙/已销毁）时静默返回：这是一次纯交互增强，
/// 失败的表现最多是「这次没拖动」，不该打断用户。
pub fn begin(window: tauri::Window<tauri::Wry>, edges: Edges) {
    let Some(start) = snapshot(&window, edges) else {
        return;
    };
    std::thread::spawn(move || run(window, start));
}

fn snapshot(window: &tauri::Window<tauri::Wry>, edges: Edges) -> Option<DragStart> {
    // 最大化态没有「边」可拖：此时窗口等于工作区，再拖只会把它拽离屏幕。
    // 顺带把「最大化时不该缩放」这件事也一并挡住。
    if window.is_maximized().unwrap_or(false) {
        return None;
    }
    let cursor = crate::mac::cursor_physical()?;
    let pos = window.outer_position().ok()?;
    let size = window.inner_size().ok()?;
    let work = window
        .current_monitor()
        .ok()
        .flatten()
        .map(|m| {
            let r = m.work_area();
            (
                r.position.x as f64,
                r.position.y as f64,
                r.size.width as f64,
                r.size.height as f64,
            )
        })
        // 拿不到工作区就不夹（而不是夹到 0）：宁可允许拖出屏幕，
        // 也不要因为一个显示器枚举失败就把窗口锁死在 1×1
        .unwrap_or((0.0, 0.0, f64::MAX / 4.0, f64::MAX / 4.0));
    Some(DragStart {
        edges,
        cursor,
        x: pos.x as f64,
        y: pos.y as f64,
        w: size.width as f64,
        h: size.height as f64,
        work,
    })
}

fn run(window: tauri::Window<tauri::Wry>, s: DragStart) {
    // ① 等左键：见 ARM_TIMEOUT 的说明
    let mut armed = false;
    let deadline = std::time::Instant::now() + ARM_TIMEOUT;
    while std::time::Instant::now() < deadline {
        if crate::mac::lmb_down() {
            armed = true;
            break;
        }
        std::thread::sleep(TICK);
    }
    if !armed {
        return;
    }

    // ② 边移动边改几何
    let mut last: Option<(i32, i32)> = None;
    loop {
        if !crate::mac::lmb_down() {
            break;
        }
        let Some(cursor) = crate::mac::cursor_physical() else {
            break;
        };
        if last != Some(cursor) {
            last = Some(cursor);
            apply(&window, &s, cursor);
        }
        std::thread::sleep(TICK);
    }
    // 记**结束**几何，不是起始的 —— 原先打的是 s.w/s.h/s.x/s.y（拖拽开始时的值），
    // 对诊断毫无用处：用户报「窗口大小不对」时，日志里显示的是拖之前的大小。
    // 这条是单次一行（不是每帧），INFO 级不吵；而缩放是本模块唯一没有事后痕迹的
    // 交互，出问题时没有这条就只能靠猜。
    let (w, h, x, y) = geometry_for(&s, last.unwrap_or(s.cursor));
    log::info!(
        "[window-resize] 缩放结束: 可视区 {}x{} @ ({},{})",
        (w - crate::WINDOW_SHADOW_MARGIN * 2.0).max(1.0) as i64,
        (h - crate::WINDOW_SHADOW_MARGIN * 2.0).max(1.0) as i64,
        x as i64,
        y as i64
    );
}

/// 拖拽到 `cursor` 时的新几何：`(宽, 高, x, y)`。
///
/// 抽成纯函数是因为这段是本模块里**唯一会算错**的地方，而它所在的线程闭包
/// 没法单测（要真窗口 + 真光标）。纯函数化之后，下面 4 条边界能直接被测试覆盖。
///
/// 全部物理像素。`edges` 决定哪几条边参与。
fn geometry_for(s: &DragStart, cursor: (i32, i32)) -> (f64, f64, f64, f64) {
    let dx = cursor.0 as f64 - s.cursor.0 as f64;
    let dy = cursor.1 as f64 - s.cursor.1 as f64;

    let mut w = s.w;
    let mut h = s.h;
    if s.edges.east {
        w += dx;
    }
    if s.edges.west {
        w -= dx;
    }
    if s.edges.south {
        h += dy;
    }
    if s.edges.north {
        h -= dy;
    }
    // 夹到工作区。**下限交给 AppKit**：tao 建窗时调过 `NSWindow.setMinSize`
    // （macos/window.rs:1753），比 min size 小的 set_size 会被系统自己夹住，
    // 这里重复夹一遍反而会在最小尺寸被抬高后算出错误的窗口原点。
    w = w.clamp(1.0, s.work.2);
    h = h.clamp(1.0, s.work.3);

    // 北/西边要连带移动窗口原点，否则是「往内长」而不是「往外长」
    let x = if s.edges.west {
        s.x - (w - s.w)
    } else {
        s.x
    };
    let y = if s.edges.north {
        s.y - (h - s.h)
    } else {
        s.y
    };
    (w, h, x, y)
}

/// 按当前光标位置算新几何并落盘
fn apply(window: &tauri::Window<tauri::Wry>, s: &DragStart, cursor: (i32, i32)) {
    let (w, h, x, y) = geometry_for(s, cursor);
    let _ = window.set_size(tauri::PhysicalSize::new(w.round() as u32, h.round() as u32));
    if s.edges.west || s.edges.north {
        let _ = window.set_position(tauri::PhysicalPosition::new(x.round() as i32, y.round() as i32));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_all_eight_directions() {
        assert_eq!(parse_edges("North").unwrap(), Edges { north: true, ..Default::default() });
        assert_eq!(parse_edges("west").unwrap(), Edges { west: true, ..Default::default() });
        let c = parse_edges("SouthWest").unwrap();
        assert!(c.south && c.west && !c.north && !c.east, "角落方向 {c:?}");
        // 带空白也应容忍：前端可能拼出 " North"
        assert!(parse_edges("  east  ").unwrap().east);
    }

    /// 未知方向必须报错而不是静默落到某个默认边 —— 静默猜错方向的后果是
    /// 「拖右边却把左边也拉长」，用户完全无法理解。
    #[test]
    fn rejects_unknown_direction() {
        assert!(parse_edges("").is_none());
        assert!(parse_edges("up").is_none());
        assert!(parse_edges("NorthWestish").is_none());
    }

    // ---- 几何（拖到某点后窗口该变成什么样）----

    fn start(edges: Edges) -> DragStart {
        DragStart {
            edges,
            cursor: (1000, 500),
            x: 100.0,
            y: 200.0,
            w: 800.0,
            h: 600.0,
            work: (0.0, 0.0, 3000.0, 2000.0),
        }
    }

    #[test]
    fn dragging_east_grows_width_and_keeps_origin() {
        let s = start(Edges { east: true, ..Default::default() });
        let (w, h, x, y) = geometry_for(&s, (1100, 500));
        assert_eq!((w, h, x, y), (900.0, 600.0, 100.0, 200.0));
    }

    /// 西边是这里最容易错的一条：宽度按 -dx 变，同时**原点必须反向移动**，
    /// 否则「向左拖」会变成「窗口右边不动、左边框往右长」。
    #[test]
    fn dragging_west_grows_width_leftwards_and_anchors_right_edge() {
        let s = start(Edges { west: true, ..Default::default() });
        let (w, h, x, y) = geometry_for(&s, (900, 500));
        assert_eq!((w, h), (900.0, 600.0));
        assert_eq!((x, y), (0.0, 200.0), "右边缘应保持在 100+800=900 不变");
    }

    #[test]
    fn dragging_north_anchors_bottom_edge() {
        let s = start(Edges { north: true, ..Default::default() });
        let (w, h, x, y) = geometry_for(&s, (1000, 400));
        assert_eq!((w, h), (800.0, 700.0));
        assert_eq!((x, y), (100.0, 100.0), "下边缘应保持在 200+600=800 不变");
    }

    #[test]
    fn corner_drag_grows_both_axes() {
        let s = start(Edges { north: true, west: true, ..Default::default() });
        let (w, h, x, y) = geometry_for(&s, (900, 400));
        assert_eq!((w, h, x, y), (900.0, 700.0, 0.0, 100.0));
    }

    /// 拖过头（把窗口往里拖成负宽）必须被夹住，且**原点不能跟着跑飞** ——
    /// 夹住宽度后原点要按夹后的尺寸算，否则窗口会瞬移到别处。
    #[test]
    fn overshrink_is_clamped_and_origin_stays_consistent() {
        let s = start(Edges { west: true, ..Default::default() });
        let (w, _h, x, _y) = geometry_for(&s, (3000, 500));
        assert_eq!(w, 1.0, "宽度应被夹到下限");
        assert_eq!(x, 100.0 - (1.0 - 800.0), "原点须按夹后的宽度算，右边缘才是锚点");
    }

    /// 不能超过所在显示器的工作区（否则窗口大半掉到屏幕外）
    #[test]
    fn never_exceeds_work_area() {
        let mut s = start(Edges { east: true, south: true, ..Default::default() });
        s.work = (0.0, 0.0, 1500.0, 900.0);
        let (w, h, _x, _y) = geometry_for(&s, (9000, 9000));
        assert_eq!((w, h), (1500.0, 900.0));
    }

    /// 取不到工作区时 clamp 的上界极大，不会把窗口锁死（见 snapshot 的兜底注释）
    #[test]
    fn missing_work_area_does_not_lock_window() {
        let mut s = start(Edges { east: true, ..Default::default() });
        s.work = (0.0, 0.0, f64::MAX / 4.0, f64::MAX / 4.0);
        // 起点光标 x=1000、窗口宽 800，拖到 x=5000 → dx=4000 → 宽 4800
        let (w, _h, _x, _y) = geometry_for(&s, (5000, 500));
        assert_eq!(w, 4800.0, "工作区缺失时应允许自由放大，而不是夹成 1px");
    }

    #[test]
    fn every_direction_parses_to_at_least_one_edge() {
        for d in [
            "north", "south", "east", "west", "northeast", "northwest", "southeast", "southwest",
        ] {
            let e = parse_edges(d).unwrap_or_default();
            assert!(
                e.north || e.south || e.east || e.west,
                "方向 {d} 解析成了「哪边都不拖」"
            );
        }
    }
}
