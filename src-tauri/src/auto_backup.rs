//! 自动备份（2026-09-29 新增）
//!
//! # 为什么要有
//!
//! 手动导出备份的缺陷是「需要用户想起来」。而数据丢失的代价是**不对称**的：
//! 丢一天的笔记很烦，丢三个月没法重来 —— 所以这件事值得由程序定时做，
//! 而不是指望用户记得点。
//!
//! # 三条设计约束
//!
//! ① **默认关，且必须用户显式选目录**。备份放哪儿是用户的选择（外置盘 /
//!    iCloud / 某块盘）。猜一个位置（比如下载目录）有个致命问题：
//!    **备份和被备份的东西可能落在同一块盘上** —— 磁盘坏了就一起没了，
//!    看起来有备份、实际没有。所以不猜。
//!
//! ② **按「距上次多久」触发，而不是「启动就备份」**。应用一关一开是高频行为，
//!    每次启动都备份会让间隔设置形同虚设，且磁盘反复写。
//!
//! ③ **轮转保留 N 份，删最旧的**。只增不减的话，磁盘会被慢慢塞满 ——
//!    而塞满之后**备份本身也写不进去**，于是「有备份」变成另一种数据丢失。
//!
//! # 时区与「一天」
//!
//! 间隔用**毫秒时长**算，不按「几点」算：后者在夏令时切换那天会出现
//! 23/25 小时，回拨那天甚至可能同一时刻触发两次。本工程的时间戳一律是
//! UTC 毫秒（`now()`），用时长比较就自动免疫了。

use crate::config;
use tauri::Manager;
use std::path::PathBuf;
use std::time::Duration;

/// 单次检查的间隔。刻意不按配置的 `auto_backup_hours` 睡 ——
/// 那意味着改一次设置要重启才生效，且小时级 sleep 无法响应退出。
/// 改成固定心跳 + 到点才动作：唤醒次数多（每天 2880 次）但每次只是几次整数比较。
const TICK: Duration = Duration::from_secs(30);

/// 单次备份的超时上限。备份是「读库 + 写 zip」，正常在秒级；
/// 设上限是为了磁盘异常时不把后台线程永久卡住。
const BACKUP_TIMEOUT: Duration = Duration::from_secs(300);

/// 备份文件名前缀。轮转时按前缀 + 时间戳排序，只认自己写的文件 ——
/// 用户可能把备份放到一个装着别的 zip 的目录里。
const PREFIX: &str = "m-hub-backup-";

/// 启动自动备份守护线程。
///
/// 幂等：`STARTED` 是 `Once`，重复调用只生效一次。
pub fn start(app: tauri::AppHandle) {
    static STARTED: std::sync::Once = std::sync::Once::new();
    STARTED.call_once(move || {
        std::thread::spawn(move || loop {
            std::thread::sleep(TICK);
            // 单跳 panic 隔离：这个线程承担着「数据不会悄悄丢」这层保护，
            // 一次 panic 就让它**永久**消失且无任何报错。下一跳照常。
            let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                if due(&app) {
                    run_backup(&app);
                }
            }));
            if r.is_err() {
                log::error!("[自动备份] 本跳 panic，已跳过（下一跳继续）");
            }
        });
    });
}

/// 是否到了该备份的时刻。
///
/// 纯函数（只读配置），便于单测。这里不做「距上次超过 N 小时」的判断之外
/// 的任何推断 —— 尤其**不**因为「从没备份过」就立刻备份：用户刚选好目录时，
/// 立刻弹一个备份出来会让人以为出了什么问题。
fn due(_app: &tauri::AppHandle) -> bool {
    match due_at(config::load()) {
        Some(_) => true,
        None => false,
    }
}

/// `Some(需要备份)` / `None`（未到点或未启用）。
/// 抽成纯函数：不依赖 tauri::AppHandle，才能单测。
pub fn due_at(cfg: config::AppConfig) -> Option<Duration> {
    if cfg.auto_backup_dir.trim().is_empty() || cfg.auto_backup_hours <= 0 {
        return None;
    }
    // last_ms == 0 表示「从未备份过」。此时 elapsed 等于「距 1970 年的时长」，
    // 是个巨大的数，直接比较会**立刻触发** —— 而用户刚选好目录就弹一个备份出来，
    // 会让人以为出了什么问题。所以这里显式挡掉。
    // （`set_auto_backup_config` 改完设置会**主动**补一次备份，不靠这条路径。）
    if cfg.auto_backup_last_ms == 0 {
        return None;
    }
    let elapsed = now_ms().saturating_sub(cfg.auto_backup_last_ms);
    if elapsed < cfg.auto_backup_hours * 3_600_000 {
        return None;
    }
    Some(Duration::from_millis(elapsed as u64))
}

pub fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// 跑一次备份：复制库 → 轮转 → 更新「上次备份」时刻。
///
/// 顺序有讲究：**先落盘、后更新时刻**。反过来的话，一旦备份失败，
/// 「上次」已经被推进到当下，于是这次失败会被当成「已经备份过」而静默跳过
/// 直到下一个间隔 —— 用户以为有备份，其实没有。
fn run_backup(app: &tauri::AppHandle) {
    let mut cfg = config::load();
    let dir = PathBuf::from(cfg.auto_backup_dir.trim());
    if let Err(e) = std::fs::create_dir_all(&dir) {
        log::warn!("[自动备份] 创建目录失败 {}: {e}", dir.display());
        return;
    }

    let Some(conn) = app.try_state::<crate::commands::DbState>() else {
        log::warn!("[自动备份] 数据库尚未就绪，跳过本次");
        return;
    };

    let name = format!(
        "{PREFIX}{}.zip",
        chrono::Local::now().format("%Y%m%d-%H%M%S")
    );
    let result = {
        let Ok(c) = conn.0.lock() else {
            log::warn!("[自动备份] 取数据库锁失败，跳过本次");
            return;
        };
        crate::commands::write_backup_archive(&c, &dir, Some(&name))
    };

    match result {
        Ok(name) => {
            log::info!("[自动备份] 已备份 -> {}", dir.join(&name).display());
            let keep = cfg.auto_backup_keep.max(1);
            if let Err(e) = rotate(&dir, keep) {
                log::warn!("[自动备份] 轮转失败: {e}");
            }
            // 备份确实落盘了，才推进「上次」时刻
            cfg.auto_backup_last_ms = now_ms();
            if let Err(e) = config::save(&cfg) {
                log::warn!("[自动备份] 写回「上次备份」时刻失败: {e}");
            }
        }
        Err(e) => {
            // 刻意**不**推进 last_ms：下一跳（30 秒后）就会重试，
            // 而不是等到下个间隔才有机会补上。
            log::error!("[自动备份] 备份失败: {e}（下一跳会重试）");
        }
    }
}

/// 只保留最近 `keep` 份，删掉更旧的。返回删掉的数量。
///
/// 排序键取**文件名里的时间戳**（而不是 mtime）：文件被复制/同步工具动过之后
/// mtime 就不再代表备份时间了，按 mtime 排会删错份数。
pub fn rotate(dir: &std::path::Path, keep: i64) -> Result<usize, String> {
    let keep = keep.max(1) as usize;
    let entries = std::fs::read_dir(dir).map_err(|e| e.to_string())?;
    let mut mine: Vec<PathBuf> = Vec::new();
    for e in entries.flatten() {
        let p = e.path();
        let name = p.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        // 严格匹配「前缀 + 14 位时间戳 + .zip」，避免误删用户放在同一目录的其它 zip
        let Some(stamp) = name.strip_prefix(PREFIX).and_then(|s| s.strip_suffix(".zip")) else {
            continue;
        };
        // 时间戳长度必须是 **15**：`%Y%m%d-%H%M%S` = 8 位日期 + 1 位连字符 + 6 位时间。
        // ⚠️ 这里原先写的是 14 —— 于是**真实备份一个都匹配不上**，轮转永远是 0 份、
        // 磁盘被慢慢塞满，而「有自动备份」这个状态看起来一切正常。
        // 判据改成「能解析成合法日期」而不是数长度：这样即使将来改日期格式，
        // 也不会又变成静默失配。
        if stamp.len() == 15 && chrono::NaiveDateTime::parse_from_str(
            &stamp.replace('-', " "),
            "%Y%m%d %H%M%S",
        )
        .is_ok()
        {
            mine.push(p);
        }
    }
    if mine.len() <= keep {
        return Ok(0);
    }
    // 文件名时间戳是定长数字串，字典序即时间序
    mine.sort();
    let to_delete = mine.len() - keep;
    let mut removed = 0usize;
    for p in &mine[..to_delete] {
        match std::fs::remove_file(p) {
            Ok(()) => removed += 1,
            Err(e) => log::warn!("[自动备份] 删除旧备份失败 {}: {e}", p.display()),
        }
    }
    if removed > 0 {
        log::info!("[自动备份] 轮转：删除 {removed} 份旧备份，保留 {keep} 份");
    }
    Ok(removed)
}

#[cfg(test)]
mod tests {
    use super::*;

    /**
     * 一个**保证独占**的临时目录。
     *
     * 原先这里用的是**写死**的名字（两个固定字面量）。写死的名字在同一个测试
     * 进程里靠「彼此不同」勉强不撞，但只要同时跑两份测试二进制（比如一边
     * `cargo test` 一边 `tauri build` 也在跑测试），两边就会 `remove_dir_all`
     * 掉对方的目录 —— flaky，且症状与真因隔了好几层。
     *
     * 改用 `create_dir` 原子地「创建或失败」、失败就加后缀重试：没有
     * 「先查再建」的 TOCTOU 窗口，也不再依赖「名字要够独特」这种约定。
     */
    fn tmp_dir(tag: &str) -> std::path::PathBuf {
        let base = std::env::temp_dir();
        for n in 0..10_000u32 {
            let d = base.join(format!("mhub_{tag}_{}_{}", std::process::id(), n));
            if std::fs::create_dir(&d).is_ok() {
                return d;
            }
        }
        panic!("连续 10000 次都建不出临时目录（{tag}）");
    }

    fn cfg_with(dir: &str, hours: i64, last: i64) -> config::AppConfig {
        let mut c = config::AppConfig::default();
        c.auto_backup_dir = dir.to_string();
        c.auto_backup_hours = hours;
        c.auto_backup_last_ms = last;
        c
    }

    /// 未选目录 / 间隔为 0 → 不备份。这两条是「默认关」的实现，
    /// 测它们是为了保证将来改默认值时不会不小心变成默认开。
    #[test]
    fn disabled_when_no_dir_or_zero_interval() {
        assert!(due_at(cfg_with("", 24, 0)).is_none(), "没选目录就不该备份");
        assert!(due_at(cfg_with("/tmp/x", 0, 0)).is_none(), "间隔为 0 就不该备份");
        assert!(
            due_at(cfg_with("   ", 24, 0)).is_none(),
            "只有空白字符的目录等同于没选"
        );
    }

    /// 刚备份过 → 不再备份。防止「一关一开就备份一次」。
    #[test]
    fn not_due_right_after_a_backup() {
        let now = now_ms();
        assert!(due_at(cfg_with("/tmp/x", 24, now)).is_none(), "刚备份过不该再备");
        assert!(
            due_at(cfg_with("/tmp/x", 24, now - 23 * 3_600_000)).is_none(),
            "距上次 23 小时（间隔 24）不该备份"
        );
    }

    /// 超过间隔 → 该备份。
    #[test]
    fn due_after_interval_elapsed() {
        let now = now_ms();
        assert!(
            due_at(cfg_with("/tmp/x", 24, now - 25 * 3_600_000)).is_some(),
            "距上次 25 小时（间隔 24）该备份"
        );
    }

    /// 「从没备份过」时**不**立刻备份。用户刚选好目录就弹一个备份出来，
    /// 会让人以为出了什么问题。
    #[test]
    fn never_backed_up_does_not_fire_immediately() {
        assert!(
            due_at(cfg_with("/tmp/x", 24, 0)).is_none(),
            "last=0 表示从未备份，不应立刻触发"
        );
    }

    /// 轮转：只删自己写的文件，且只删超出份数的那些。
    #[test]
    fn rotate_keeps_newest_n_and_ignores_foreign_files() {
        let dir = tmp_dir("rotate");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        // 5 份自己的备份 + 3 个不该动的文件
        for i in 1..=5 {
            std::fs::write(dir.join(format!("{PREFIX}2026010{i}-000000.zip")), b"x").unwrap();
        }
        std::fs::write(dir.join("我的重要文件.zip"), b"x").unwrap();
        std::fs::write(dir.join("m-hub-backup-notazip.zip"), b"x").unwrap();
        std::fs::write(dir.join("m-hub-backup-202601010000.zip.bak"), b"x").unwrap();

        let removed = rotate(&dir, 3).unwrap();
        assert_eq!(removed, 2, "5 份留 3 份，应删 2");
        assert!(dir.join("我的重要文件.zip").exists(), "用户自己的文件不能删");
        assert!(dir.join("m-hub-backup-notazip.zip").exists(), "非时间戳命名的不能删");
        assert!(dir.join("m-hub-backup-202601010000.zip.bak").exists(), "后缀不对的不能删");
        // 最新的两份（04/05）必须在
        assert!(dir.join(format!("{PREFIX}20260104-000000.zip")).exists());
        assert!(dir.join(format!("{PREFIX}20260105-000000.zip")).exists());
        // 最旧的一份必须已删
        assert!(!dir.join(format!("{PREFIX}20260101-000000.zip")).exists());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn rotate_is_noop_when_under_limit() {
        let dir = tmp_dir("rotate_noop");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        for i in 1..=3 {
            std::fs::write(dir.join(format!("{PREFIX}2026010{i}-000000.zip")), b"x").unwrap();
        }
        assert_eq!(rotate(&dir, 7).unwrap(), 0);
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 3, "不够份数时一份都不能删");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
