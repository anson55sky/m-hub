//! 免打扰（2026-09-29 新增）
//!
//! # 为什么要有
//!
//! 悬浮球、待办提醒、倒计时是**三个各自独立**的触发通道。用户想静音时，
//! 唯一的办法是逐个关掉设置，或者干脆退出应用 —— 而退出等于把工具关掉了。
//! 缺一个统一的「现在不要烦我」开关，它就会天天烦人。
//!
//! # 为什么放在 `notify::show_notice` 这一层
//!
//! 因为那是**所有**通知的必经之路：倒计时到点、待办提醒、未来的任何通知
//! 都要过它。判断写在别处（各个调用方）就等于每加一个通知源就要记得加一次判断 ——
//! 漏一次就是「免打扰开着但它还是弹」，而这种漏是静默的。
//!
//! # 静默 vs 丢弃
//!
//! 免打扰期间通知**不显示但仍记进历史**：用户早上解除免打扰后，
//! 应当能看到夜里错过的东西。全丢等于「提醒消失了」，那是更糟的失败。

use chrono::Timelike;
use crate::config;

/// 当前是否处于免打扰状态。
///
/// 判定 = 手动开关 **或** 落在配置的时间段内。
///
/// 这里是「或」且**只能**是「或」：`dnd_enabled` 是布尔开关，不是三态。
/// 「手动关掉 → 时段也不再生效」那种语义需要第三态（auto / on / off），
/// 而它服务的场景（设了夜间时段、又在夜里临时想看提醒）正确解法是
/// **关掉时段**，而不是临时覆盖 —— 所以不为它加配置项。
pub fn is_dnd_active(cfg: &config::AppConfig, now_hour: i64) -> bool {
    if cfg.dnd_enabled {
        return true;
    }
    if !cfg.dnd_scheduled {
        return false;
    }
    in_window(now_hour, cfg.dnd_start_hour, cfg.dnd_end_hour)
}

/// 某个小时是否落在 `[start, end)` 内。`start > end` 表示跨零点（如 22→8）。
///
/// 左闭右开：`start` 当小时算打扰，`end` 当小时**不算** —— 「22 点到 8 点」
/// 的字面意思就是 8 点已经结束了。
pub fn in_window(hour: i64, start: i64, end: i64) -> bool {
    let h = hour.rem_euclid(24);
    let s = start.rem_euclid(24);
    let e = end.rem_euclid(24);
    if s == e {
        // 起止相同：视为「不启用时段」，否则会变成全天免打扰。
        // 这几乎肯定是配置错了，而全天免打扰的代价（用户以为提醒坏了）远大于
        // 「时段没生效」的代价。
        return false;
    }
    if s < e {
        h >= s && h < e
    } else {
        // 跨零点
        h >= s || h < e
    }
}

/// 当前的免打扰状态（读配置 + 当前本地小时）。
pub fn active() -> bool {
    let cfg = config::load();
    let hour = chrono::Local::now().hour() as i64;
    is_dnd_active(&cfg, hour)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> config::AppConfig {
        config::AppConfig::default()
    }

    /// 时段判定的四种形状。写全这四种是因为跨零点那个分支最容易写错，
    /// 而写错的症状是「半夜不静音」或「白天全静音」——都很难被用户归因。
    #[test]
    fn window_shapes() {
        // 普通区间 9→18
        assert!(in_window(9, 9, 18), "起点当小时算在内");
        assert!(in_window(17, 9, 18));
        assert!(!in_window(18, 9, 18), "终点当小时**不算**在内");
        assert!(!in_window(8, 9, 18));
        // 跨零点 22→8
        assert!(in_window(23, 22, 8), "跨零点的后半夜");
        assert!(in_window(0, 22, 8), "跨零点的凌晨");
        assert!(in_window(7, 22, 8));
        assert!(!in_window(8, 22, 8), "8 点已结束");
        assert!(!in_window(12, 22, 8), "正午不在跨零点区间内");
        assert!(!in_window(21, 22, 8));
        // 越界小时不炸（rem_euclid）
        assert!(!in_window(25, 9, 18), "小时越界应按 rem_euclid 归一而不是崩");
    }

    /// 起止相同 = 配置错了。此时**不能**当成「全天免打扰」——
    /// 那个代价是用户以为提醒坏了，比「时段没生效」大得多。
    #[test]
    fn same_start_end_means_disabled_not_all_day() {
        assert!(!in_window(3, 9, 9), "起止相同应是「未启用」，不是全天");
        assert!(!in_window(15, 9, 9));
    }

    /// 手动开关与时段是「或」：任一为真即静音。
    ///
    /// 这条测试原本断言的是「手动关掉后时段也不再生效」—— 那是我在注释里写下
    /// 却**根本没实现**的语义。写测试时才发现：布尔开关表达不了三态
    /// （auto / on / off）。要么把配置改成三态，要么承认它是「或」。
    /// 这里选后者，并让测试名与断言都对上真实语义 —— 与其留一个必然红的测试
    /// 提醒我，不如把决策记在这里。
    #[test]
    fn manual_and_schedule_are_or_ed() {
        let mut c = cfg();
        c.dnd_scheduled = true;
        c.dnd_start_hour = 0;
        c.dnd_end_hour = 23;
        // 时段覆盖、手动关 → 仍静音（因为时段生效）
        c.dnd_enabled = false;
        assert!(is_dnd_active(&c, 12), "时段覆盖时，即便手动关着也要静音");
        // 手动开 → 一律静音，即便不在时段内
        c.dnd_scheduled = false;
        c.dnd_start_hour = 22;
        c.dnd_end_hour = 8;
        c.dnd_enabled = true;
        assert!(is_dnd_active(&c, 12), "手动打开则一律静音");
    }

    /// 关掉时段后只剩手动开关。
    #[test]
    fn schedule_off_means_only_manual() {
        let mut c = cfg();
        c.dnd_scheduled = false;
        c.dnd_start_hour = 0;
        c.dnd_end_hour = 23;
        c.dnd_enabled = false;
        assert!(!is_dnd_active(&c, 12));
        c.dnd_enabled = true;
        assert!(is_dnd_active(&c, 12));
    }

    /// 默认配置下**完全不静音** —— 无论凌晨还是中午。
    ///
    /// 这条是「新增功能不得改变既有行为」的守卫：时段若默认打开，通知就会在
    /// 22:00–08:00 静默消失，而用户从没要求过、且要靠自己才能发现。
    /// 将来有人改默认值时，这里会立刻红。
    #[test]
    fn defaults_are_fully_off() {
        let c = cfg();
        assert!(!c.dnd_enabled, "免打扰默认必须关");
        assert!(!c.dnd_scheduled, "免打扰时段默认必须关");
        assert!(!is_dnd_active(&c, 1), "默认不应在凌晨静音");
        assert!(!is_dnd_active(&c, 12), "默认不应在中午静音");
        assert!(!is_dnd_active(&c, 23), "默认不应在夜里静音");
    }
}
