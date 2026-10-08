use serde::{Deserialize, Serialize};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

use crate::models::ChatModelConfig;

/// 全局配置写锁：串行化所有「读-改-写」配置命令。
/// 防止并发下旧快照互相覆盖——典型事故：`save_chat_models` 刚把新模型写入 app.json，
/// 另一个命令用启动时读到的旧 `chat_models`（空/过期）整体覆写，导致配置的供应商「消失」。
static CONFIG_LOCK: Mutex<()> = Mutex::new(());

/// 获取配置写锁（所有读-改-写配置的调用点都必须持有它）
pub fn lock() -> MutexGuard<'static, ()> {
    CONFIG_LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WindowState {
    pub width: f64,
    pub height: f64,
    pub x: Option<f64>,
    pub y: Option<f64>,
    pub always_on_top: bool,
}

impl Default for WindowState {
    fn default() -> Self {
        Self {
            width: 1400.0,
            height: 900.0,
            x: None,
            y: None,
            always_on_top: false,
        }
    }
}

/// 工作台「自定义速达」槽位的内容配置（槽位 id 固定为 suda1..suda4，同便签 1/2 的池子模式）。
/// 纯前端读写（用户可编辑项，经 save_config 整体落盘），后端不解释字段含义。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SudaCustomModuleConfig {
    /// 槽位 id：suda1 / suda2 / suda3 / suda4
    #[serde(default)]
    pub id: String,
    /// 内容来源：pinned(手动挑选,顺序=resource_ids) / app / web / file(整个大类) / subcategory(指定小类)
    #[serde(default)]
    pub source: String,
    /// source = subcategory 时的小类名（resources.category 口径）
    #[serde(default)]
    pub subcategory: String,
    /// source = pinned 时的资源 id 序（勾选顺序即展示顺序；已删除的资源自动跳过）
    #[serde(default)]
    pub resource_ids: Vec<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AppConfig {
    /// 主题模式：light / dark / system（旧配置中的 `theme` 字段自动映射到此字段）
    #[serde(alias = "theme")]
    pub theme_mode: String,
    /// 主题预设：indigo / green / morandi / midnight
    pub theme_preset: String,
    /// 强调色（hex，如 #5B5BF5）；null 表示跟随预设推荐强调色
    pub accent_color: Option<String>,
    /// 应用壁纸：主窗口背景图片的绝对路径（空 = 未设置，回退主题渐变背景）
    #[serde(default)]
    pub wallpaper_path: String,

    // ---- 免打扰（2026-09-29 新增）----
    /// 免打扰：开启期间**不弹**通知窗。默认关。
    ///
    /// 为什么要有：悬浮球、待办提醒、倒计时是三个**各自独立**触发的通道，
    /// 用户没法用一处开关把���们一起静音。缺这个开关时它就会天天烦人，
    /// 而用户唯一的办法是退出应用 —— 那等于把工具关掉了。
    #[serde(default)]
    pub dnd_enabled: bool,
    /// 免打扰是否自动在「每天这个时段」开启（如 22:00–8:00 睡觉时段）。
    ///
    /// 默认 **false**：时段一旦默认打开，通知就会在 22:00–08:00 之间**静默地**
    /// 不再弹出 —— 而用户从没要求过这件事，且要靠自己发现「提醒怎么不响了」。
    /// 新增的功能默认都应当是「什么都没变」，要行为改变必须由用户显式打开。
    #[serde(default)]
    pub dnd_scheduled: bool,
    /// 免打扰时段：起始小时（0–23，本地时区）。
    #[serde(default = "default_dnd_start_hour")]
    pub dnd_start_hour: i64,
    /// 免打扰时段：结束小时（0–23，本地时区）。start > end 表示跨零点。
    #[serde(default = "default_dnd_end_hour")]
    pub dnd_end_hour: i64,

    // ---- 聚焦模式（2026-09-29 新增）----
    /// 聚焦模式：只显示 `focus_pins` 里选中的模块，竖排全宽。默认关。
    ///
    /// 为什么默认关：它改变的是**默认看到什么**，属于用户一旦没主动打开就
    /// 不该发生的状态变化（同免打扰时段那条）。
    #[serde(default)]
    pub focus_enabled: bool,
    /// 聚焦模式下显示的模块 id，**顺序即显示顺序**。
    ///
    /// 默认 `todo`（今天要做什么）+ `sticky1`（随手记）+ `countdown`（给这件事
    /// 一个边界）。刻意不含 clock/weather/sysmon —— 那是「环境信息」，全天可见，
    /// 收进聚焦模式等于把它藏起来，而它们并不占多少地方。
    #[serde(default = "default_focus_pins")]
    pub focus_pins: Vec<String>,

    // ---- 自动备份（2026-09-29 新增）----
    /// 自动备份的目标目录（绝对路径）。空 = 不做自动备份。
    ///
    /// 为什么不默认开：备份目录该放哪儿是**用户的选择**（外置盘 / iCloud / 某块盘），
    /// 猜一个位置（下载目录之类）反而可能把备份和被备份的东西放在一起 ——
    /// 磁盘坏了就一起没了。默认关，由用户显式选一次。
    #[serde(default)]
    pub auto_backup_dir: String,
    /// 自动备份间隔（小时）。0 = 不自动备份。
    ///
    /// 默认 24h：备份是「防意外」而不是「防丢失」，而这份数据本来就有手动导出。
    /// 间隔太短会让磁盘反复写，收益却只在「崩了以后少丢一点」。
    #[serde(default = "default_auto_backup_hours")]
    pub auto_backup_hours: i64,
    /// 自动备份保留份数（超出后删最旧的）。至少 1。
    #[serde(default = "default_auto_backup_keep")]
    pub auto_backup_keep: i64,
    /// 上次自动备份的时刻（毫秒时间戳）。0 = 从未备份过。
    ///
    /// 存盘**而不是**每次启动就备份一次：应用一关一开是高频行为，
    /// 而「距上次超过 N 小时才备份」才能让间隔设置真正有意义。
    #[serde(default)]
    pub auto_backup_last_ms: i64,
    /// 壁纸整屏静态模糊（ADR 0002：模糊作用于壁纸层整体，非卡片局部 backdrop）
    #[serde(default = "default_true")]
    pub wallpaper_blur: bool,
    /// 壁纸蒙版：主题底色罩层不透明度（0–0.85，默认 0.3），在壁纸鲜亮度与文字对比度间取平衡
    #[serde(default = "default_wallpaper_veil")]
    pub wallpaper_veil: f64,
    /// 沉浸模式：卡片改用真毛玻璃 backdrop-filter 局部取景模糊（ADR 0003 受控例外，默认关）
    #[serde(default)]
    pub wallpaper_immersive: bool,
    /// 卡片玻璃透明度（0.4–1.0，1.0 = 默认不透明观感）
    #[serde(default = "one")]
    pub glass_opacity: f64,
    /// 侧边栏展开/收缩功能开关（默认关闭）
    pub sidebar_toggle: bool,
    pub window: WindowState,
    pub global_shortcut: String,
    /// 主页面「中上区块」显示内容：
    /// countdown(默认倒计时) / token(Token 统计) / notes(速记统计) / todo(待办概览) / resources(速达数量)
    pub dashboard_mid_content: String,
    /// 工作台自定义布局（placements JSON 数组字符串；空串 = 未自定义，回退推荐布局）
    #[serde(default)]
    pub dashboard_layout: String,
    /// 倒计时到点提示音（默认关闭）
    pub countdown_sound: bool,
    /// 时钟卡片语录（工作台时间卡片下方显示的一句话，空串时回退默认）
    pub clock_quote: String,
    /// 右下角通知弹窗的驻留时长（毫秒，1000–60000；到点自动淡出）。
    /// 后端每次推送通知时读当前值随事件下发，改设置立即生效（无需重启通知窗）
    #[serde(default = "default_notice_duration_ms")]
    pub notice_duration_ms: i64,
    /// 联网功能总开关（默认开启）：有网显示在线内容、无网自动隐藏；关闭后完全不发起网络请求
    #[serde(default = "default_true")]
    pub online_enabled: bool,
    /// 隐藏窗口降低内存占用（默认开启）：窗口隐藏时把 WebView2 内存目标级别设为
    /// Low（弃缓存换页、脚本照常运行），显示前恢复 Normal。见 webview_mem.rs
    #[serde(default = "default_true")]
    pub webview_mem_low_on_hide: bool,
    /// 天气城市展示名（空串 = 未配置，天气卡不显示）
    #[serde(default)]
    pub weather_city: String,
    /// 天气经纬度缓存（geocoding / IP 定位后写入；0 表示未配置）
    #[serde(default)]
    pub weather_lat: f64,
    #[serde(default)]
    pub weather_lng: f64,
    /// 名言来源：online（在线 hitokoto，离线回退本地语料）/ local（仅本地语料）
    #[serde(default = "default_quote_source")]
    pub quote_source: String,
    /// AI 对话自定义模型配置（不绑定厂商，统一 OpenAI 兼容协议；api_key 不落盘）
    pub chat_models: Vec<ChatModelConfig>,
    /// AI 对话右侧面板宽度（320–640px，持久化用户拖拽结果）
    pub chat_panel_width: f64,
    /// AI 对话右侧面板是否展开
    pub chat_panel_open: bool,
    /// AI 对话面板方位：left / right / top / bottom（默认右侧）
    #[serde(default = "default_chat_panel_side")]
    pub chat_panel_side: String,
    /// AI 对话面板在「顶部/底部」方位时的高度（320–640px 之外的拖拽会钳制）
    #[serde(default = "default_chat_panel_height")]
    pub chat_panel_height: f64,
    /// AI 对话右侧面板透明度（0.5–1.0，可在设置中调整）
    #[serde(default = "default_chat_panel_opacity")]
    pub chat_panel_opacity: f64,
    /// AI 对话是否以独立窗口打开（true = 独立小窗，false = 主窗内嵌抽屉，默认）
    #[serde(default)]
    pub chat_window_mode: bool,
    /// AI 对话独立窗口宽度（逻辑 px，由窗口缩放拖拽记忆）
    #[serde(default = "default_chat_window_width")]
    pub chat_window_width: f64,
    /// AI 对话独立窗口高度（逻辑 px）
    #[serde(default = "default_chat_window_height")]
    pub chat_window_height: f64,
    /// AI 对话独立窗口位置（物理 px，拖动松手后由后端记忆，与悬浮球/倒计时浮窗同约定）
    #[serde(default)]
    pub chat_window_x: Option<f64>,
    #[serde(default)]
    pub chat_window_y: Option<f64>,
    /// AI 对话独立窗口是否置顶（自制标题栏的图钉按钮切换）
    #[serde(default)]
    pub chat_window_pinned: bool,

    /// 扩展中心「点一行」的行为（2026-10-03 补）。
    ///
    /// · `detail`（默认）：点行打开扩展详情（信息 / 权限 / 打开方式）
    /// · `open`：点行直接打开扩展，详情走右侧的「详情」按钮
    ///
    /// ⚠️ 默认是 `detail` 而不是 `open`：点一行是最容易误触的动作，
    ///   而「看一眼这个扩展要什么权限」恰好是最该顺手做到的事。
    ///   想改的人去设置里换即可，两种都随时能切回来。
    ///
    /// ⚠️ 用 String 而不是 Rust 枚举：老配置里**没有这个字段**，
    ///   serde default 拿不到枚举值（枚举没有 Default），而 String 的
    ///   `default` 只是一个字符串，写错了还能在前端回落。
    #[serde(default = "default_extension_row_click")]
    pub extension_row_click: String,
    /// 剪贴板历史全局呼出快捷键（默认 Ctrl+Alt+V，可配置）
    pub clipboard_shortcut: String,
    /// 全局搜索呼出快捷键（默认 Ctrl+K，可配置，全局注册）
    #[serde(default = "default_search_shortcut")]
    pub search_shortcut: String,
    /// AI 对话呼出快捷键（默认 Ctrl+Shift+K，可配置，全局注册）
    #[serde(default = "default_chat_shortcut")]
    pub chat_shortcut: String,
    /// 统一捕获快捷键（2026-09-29 新增）。默认 ⇧⌘U / Ctrl+Shift+U。
    /// 用来从任何地方一行记下东西，自动路由到速记/待办/提示词/倒计时/速达。
    pub capture_shortcut: String,
    /// 速记快捷键（2026-10-08 新增，v0.8.0 发布说明 ⑩）。默认 ⌘⇧N / Ctrl+Shift+N。
    /// 按下即切到速记视图并新建一条笔记 —— 与统一捕获互补（捕获要先想清记到哪）。
    #[serde(default = "default_notes_shortcut")]
    pub notes_shortcut: String,

    // ---- 全局快捷键的启用开关（2026-10-03 补）----
    //
    // ⚠️ 「关掉」= **暂时停用**，按键组合**原样保留**。这是刻意的：
    //   与「改绑」分开是两种需求 —— 想换键的人用改绑，想让某个键别再抢
    //   （比如 Ctrl+K 已被别的软件占了）的人用开关。关掉之后原来的组合
    //   还在配置里，随时能打开，**不需要重新录一遍**。
    //
    //   若把「关掉」实现成「清空快捷键字符串」，用户再打开就得重录 ——
    //   而且空字符串会让 `register_toggle_shortcut` 拿着空键去注册，
    //   那是注册失败还可能被吞成 warn，看起来像「开关坏了」。
    //
    // 六个（不是发布说明最初写的「四个」）：统一捕获 ⇧⌘U 与速记 ⌘⇧N 同样是
    // 全局注册的快捷键，只给其余四个开关会让它们在列表里成为唯一的例外。
    #[serde(default = "default_true")]
    pub shortcut_toggle_enabled: bool,
    #[serde(default = "default_true")]
    pub shortcut_clipboard_enabled: bool,
    #[serde(default = "default_true")]
    pub shortcut_search_enabled: bool,
    #[serde(default = "default_true")]
    pub shortcut_chat_enabled: bool,
    #[serde(default = "default_true")]
    pub shortcut_capture_enabled: bool,
    #[serde(default = "default_true")]
    pub shortcut_notes_enabled: bool,
    /// 剪贴板历史最大条数（含置顶；置顶豁免自动清理但计入上限）
    pub clipboard_max_items: i64,
    /// 非置顶记录的保留天数
    pub clipboard_ttl_days: i64,
    /// 速记回收站的保留天数（v0.8.0）。**0 = 永久保留，不自动清理**。
    ///
    /// ⚠️ 0 必须是「不清理」而不是「立即清空」——`purge_expired_trash` 里
    ///   有对应的早退分支。写成 `keep_days.max(1)` 这种归一化会在每次启动时
    ///   把整个回收站清空，而用户从没点过「清空」。
    #[serde(default = "default_notes_trash_days")]
    pub notes_trash_days: i64,
    /// 是否暂停记录（暂停期间复制内容不写入历史）
    pub clipboard_paused: bool,
    /// 粘贴快捷键方式：auto(自动检测终端) / ctrl_v / ctrl_shift_v / shift_insert
    #[serde(default = "default_paste_method")]
    pub clipboard_paste_method: String,
    /// 速达网页条目默认打开方式：panel(主窗内嵌面板) / window(独立应用内浏览器窗口)
    #[serde(default = "default_suda_web_open_mode")]
    pub suda_web_open_mode: String,
    /// 工作台「自定义速达」槽位内容配置（suda1..suda4，见 struct 注释）
    #[serde(default)]
    pub suda_custom_modules: Vec<SudaCustomModuleConfig>,
    /// 速达内嵌面板是否显示工具栏（地址栏 + 前进/后退/刷新；默认不显示，
    /// 隐藏时整个面板区域只渲染网页，返回速达走左侧导航）
    #[serde(default)]
    pub suda_panel_toolbar: bool,
    /// 记录剪贴板图片（复制图片时落盘快照进历史，默认开启）
    #[serde(default = "default_true")]
    pub clipboard_image_enabled: bool,
    /// 记录剪贴板文件（复制文件时记录路径进历史，默认开启）
    #[serde(default = "default_true")]
    pub clipboard_file_enabled: bool,
    /// 全局字体缩放系数（0.85–1.30，默认 1.0）
    #[serde(default = "one")]
    pub font_scale: f64,
    /// 便签模块字体缩放系数（相对全局的额外缩放，默认 1.0）
    #[serde(default = "one")]
    pub font_sticky: f64,
    /// 速记模块字体缩放系数（默认 1.0）
    #[serde(default = "one")]
    pub font_notes: f64,
    /// 提示词模块字体缩放系数（默认 1.0）
    #[serde(default = "one")]
    pub font_prompt: f64,
    /// 待办模块字体缩放系数（默认 1.0）
    #[serde(default = "one")]
    pub font_todo: f64,
    /// 速记编辑器模式：wysiwyg（实时预览，默认）/ split（分屏预览）/ source（源码）。
    /// 按用户记住，不按单篇笔记。非法值由前端读入时归一为 wysiwyg。
    #[serde(default = "default_note_editor_mode")]
    pub note_editor_mode: String,
    /// service 扩展运行时策略：auto（自动检测，默认）/ builtin（始终内置）/ system（始终系统）
    #[serde(default = "default_runtime_strategy")]
    pub runtime_strategy: String,
    /// 自动信任带 service 后台的扩展（2026-10-06）。
    ///
    /// 默认 **false**：service 扩展要跑本地后台程序，逐版本显式授权是唯一的
    /// 安全闸门（`extension.rs::service_version_trusted` 要求
    /// `service:version` 等于当前 manifest 版本 —— 版本一升就重新不信任）。
    ///
    /// ⚠️ 打开它意味着「扩展作者改一版就自动拿到执行权」，与扩展中心那句
    ///   「需要你确认信任当前版本才会启动」正好相反。所以这里只在**用户自己
    ///   明确点开**时才生效，且默认值保持关闭 —— 升级**不会**替用户打开它
    ///   （`#[serde(default = "false")]` 就是这条保证）。
    #[serde(default = "default_auto_trust_service")]
    pub auto_trust_service: bool,
    /// 固定到左侧栏的扩展 id 列表（点击侧栏菜单即在主区打开对应扩展）
    #[serde(default)]
    pub sidebar_extensions: Vec<String>,
    /// 扩展「默认打开方式」映射：extId → view / window / drawer（未设置时默认 view）
    #[serde(default)]
    pub extension_open_modes: std::collections::HashMap<String, String>,
    /// 扩展「链接打开方式」映射：extId → inapp（应用内浏览器，默认）/ browser（系统默认浏览器）。
    /// 门控 runtime.openExternal：扩展页里的外链按此分流
    #[serde(default)]
    pub extension_link_modes: std::collections::HashMap<String, String>,
    /// ⚠️ **已废弃、不再被读取**（v0.6.1）：市场清单地址的唯一真相源是内置常量
    /// [`market_registry_url`]——从 v0.6.1 起客户端**不再直连对象存储**，清单/包/截图一律走
    /// 平台服务端接口（`m-hub-server` 的 `src/modules/market`，服务端再代理 COS）。
    /// 字段保留只为兼容旧 `app.json`：读到即被 [`migrate_legacy_endpoints`] 归一为服务端地址并落盘。
    /// 不要再从它读地址（要恢复「自建分发可配置」时，请连同设置入口一起加回来）。
    #[serde(default)]
    pub market_endpoint: String,
    /// ⚠️ **已废弃、不再被读取**（v0.6.x）：曾经是「开发者模式」总开关，现在**登记即加载**——
    /// 加进「我的扩展」的本机源码目录一律直挂（见 AGENTS.md 约定 45 的 v0.6.x 修订）。
    /// 字段保留只为兼容旧 `app.json` 里残留的 false，读到即忽略；不要再恢复读取。
    #[serde(default)]
    pub dev_mode_enabled: bool,
    /// ⚠️ **已废弃、不再被读取**：m-hub 平台服务端地址的唯一真相源是内置常量
    /// `DEFAULT_SERVER_URL`（v0.5.6 起平台域名正式启用，设置页不再提供地址入口，见约定 52）。
    /// 字段保留只为兼容旧 `app.json`（其中可能残留开发期的临时地址），读到即忽略——
    /// 若哪天又需要可配置，请连同设置入口一起加回来，不要只恢复读取。
    #[serde(default)]
    pub server_url: String,
    /// 「我的扩展」：本机扩展源码目录列表（绝对路径，目录须含 manifest.json）。
    /// 登记即加载（无需开关）；⚠️ 这些目录会被动态加入资产协议作用域，只暴露给扩展内容协议；
    /// 不要添加敏感目录。见 AGENTS.md 约定 45
    #[serde(default)]
    pub dev_extensions: Vec<String>,
    /// 扩展开发技能包（Skills）的自定义安装根目录（自动探测的助手目录之外的 skills 根）。
    /// 安装到自定义目录时登记，供设置 →「扩展 → Skills」列出；只解除登记时见 `remove_skill_root`。
    #[serde(default)]
    pub skill_roots: Vec<String>,
    /// 开机自启动（登录 Windows 时自动驻留托盘）
    #[serde(default)]
    pub run_at_startup: bool,
    /// ⚠️ **已废弃、不再被读取**（v0.6.1）：升级清单地址的唯一真相源是内置常量
    /// [`update_manifest_url`]（同样走平台服务端接口，见 `market_endpoint` 字段注释）。
    /// 字段保留只为兼容旧 `app.json`，读到即被 [`migrate_legacy_endpoints`] 归一并落盘。
    #[serde(default)]
    pub update_endpoint: String,
    /// 自动升级总开关（默认开启）：关闭后不再发起版本检查
    #[serde(default = "default_true")]
    pub auto_update_enabled: bool,
    /// 静默检查更新频率（小时，默认 4）
    #[serde(default = "default_update_interval_hours")]
    pub update_interval_hours: u64,
    /// 用户「跳过此版本」记录的版本号（空 = 未跳过）；check 命中时若与清单版本一致则不再提示
    #[serde(default)]
    pub skipped_update_version: String,
    /// 「稍后再提示」暂停到点（epoch 毫秒，0 = 未暂停）：到期前自动检查不弹更新弹窗
    #[serde(default)]
    pub update_snooze_until_ms: i64,
    /// 桌面悬浮球总开关（ADR 0004，默认开启）：主窗口隐藏时在桌面显示悬浮球
    #[serde(default = "default_true")]
    pub floating_ball_enabled: bool,
    /// 悬浮球贴边自动隐藏：拖到屏幕边缘附近松手 → 球心落在屏边，只露出半个球体；
    /// 悬停时球体完整滑出。取代旧「贴边吸附」（用户反馈吸附从未生效，改为本交互）。
    /// alias：v0.5.2 及更早字段名为 floating_ball_snap，用户显式关闭过的偏好经别名
    /// 自动迁移（同 theme→theme_mode 先例），否则升级后被丢弃回落 default_true
    #[serde(default = "default_true", alias = "floating_ball_snap")]
    pub floating_ball_auto_hide: bool,
    /// 与主窗口同时显示：默认 false = 球仅在主窗隐藏/最小化时出现；
    /// 开启后球常驻桌面，主窗显示也不隐藏（sync_with_main 读此字段联动）
    #[serde(default)]
    pub floating_ball_with_main: bool,
    /// 环形快捷菜单按钮 id 列表（view:xxx / act:xxx，去重后最多 8 个，见 floating_ball.rs）
    #[serde(default = "default_floating_ball_buttons")]
    pub floating_ball_buttons: Vec<String>,
    /// 悬浮球窗口位置（物理 px，拖拽松手后由后端记忆；与倒计时浮窗同约定）
    #[serde(default)]
    pub floating_ball_x: Option<f64>,
    #[serde(default)]
    pub floating_ball_y: Option<f64>,
    /// 悬浮球静止态保持转动（炫酷模式，默认关）：true = 陀螺环常转 + canvas 满帧
    /// （v0.6.2 及以前的行为，更耗电发热）；false = 静止时环暂停 + canvas 降 24fps
    /// （笔记本发热治理，见 FloatingBallWindow 的 rings-idle / IDLE_FPS）
    #[serde(default)]
    pub floating_ball_idle_spin: bool,
}

fn one() -> f64 {
    1.0
}

fn default_paste_method() -> String {
    "auto".to_string()
}

/// 回收站默认保留 30 天：够长（误删通常几天后才发现）、又不至于让回收站无限膨胀。
fn default_notes_trash_days() -> i64 {
    30
}

/// 速达网页默认打开方式：内嵌面板（ADR 0011 2026-09-25 拍板：默认落点 = 面板，可设置）
fn default_suda_web_open_mode() -> String {
    "panel".to_string()
}

/// 全局搜索呼出快捷键默认值（与 shortcut.rs 的 DEFAULT_SEARCH_SHORTCUT 同源）
fn default_search_shortcut() -> String {
    crate::shortcut::DEFAULT_SEARCH_SHORTCUT.to_string()
}

/// AI 对话呼出快捷键默认值（与 shortcut.rs 的 DEFAULT_CHAT_SHORTCUT 同源）
fn default_chat_shortcut() -> String {
    crate::shortcut::DEFAULT_CHAT_SHORTCUT.to_string()
}

/// 速记快捷键默认值（与 shortcut.rs 的 DEFAULT_NOTES_SHORTCUT 同源）
///
/// ⚠️ 必须从常量取，不能在配置里另写一份字符串 —— 那就是第二份真相，
///   两平台 `#[cfg]` 分叉时（剪贴板 mac/非 mac 就不同）必然漂。
fn default_notes_shortcut() -> String {
    crate::shortcut::DEFAULT_NOTES_SHORTCUT.to_string()
}

/// 通知驻留时长默认 5 秒
fn default_notice_duration_ms() -> i64 {
    5000
}

fn default_chat_panel_opacity() -> f64 {
    1.0
}

fn default_chat_panel_side() -> String {
    "right".to_string()
}

fn default_extension_row_click() -> String {
    "detail".to_string()
}

fn default_true() -> bool {
    true
}

fn default_focus_pins() -> Vec<String> {
    vec!["todo".into(), "sticky1".into(), "countdown".into()]
}

fn default_dnd_start_hour() -> i64 {
    22
}

fn default_dnd_end_hour() -> i64 {
    8
}

fn default_auto_backup_hours() -> i64 {
    24
}

fn default_auto_backup_keep() -> i64 {
    7
}

fn default_chat_panel_height() -> f64 {
    380.0
}

fn default_chat_window_width() -> f64 {
    460.0
}

fn default_chat_window_height() -> f64 {
    640.0
}

fn default_wallpaper_veil() -> f64 {
    0.3
}

fn default_quote_source() -> String {
    "online".to_string()
}

fn default_auto_trust_service() -> bool {
    false
}

fn default_runtime_strategy() -> String {
    "auto".to_string()
}

fn default_note_editor_mode() -> String {
    "wysiwyg".to_string()
}

/// m-hub 平台服务端地址（账号登录 / 平台额度 / 申请开发者 / 发布扩展 / 市场清单 / 升级清单都基于它）。
///
/// **唯一真相源，且刻意不可配置**：正式域名启用后，设置页的「服务器地址」入口已移除
/// （见约定 52）。此前可配置是为了开发期临时指向本机联调地址，代价是老用户机器上
/// 残留的临时地址会在正式域名上线后继续生效、而界面上又没有入口可以改回来。
///
/// v0.6.1 起为 `https`：域名已于 2026-09-17 上 Let's Encrypt 证书（http 会 301 到 https），
/// 账号 token、市场清单、升级包与 AI 请求不再明文过网。
///
/// ⚠️ **fork 注意**：这是**上游作者的服务端**，m-hub 目前仍在用（换掉会让
/// 账号登录 / AI 额度 / 扩展市场 / 应用升级这几项直接不可用）。要真正独立分发，
/// 换地址时**只改这一个常量**并重新发版即可（设置页没有地址入口，约定 52）——
/// 但别只改一半：清单、升级、登录、AI 全部打这一个地址，改完要一起验证。
///
/// ## v0.7.3：静态资源迁到 Cloudflare Pages
///
/// 改为 `https://m-hub-server.pages.dev`。三条背景，都是实测得来的：
///
/// ① **`*.workers.dev` 在国内被 DNS 污染**（多个解析器给出不同的假 IP，
///    而 `www.cloudflare.com` 解析正常 → 污染针对域名而非 Cloudflare 的 IP 段）。
///    而 `*.pages.dev` **不被污染**（本机/公共 DNS 三个解析器结果一致），
///    这是选它而不是 Workers 的唯一理由 —— 代码侧 Workers 入口一直现成。
/// ② **免费且 scale-to-zero**，而平台托管服务空闲后会休眠：实测休眠 5.5 分钟后
///    首个请求 502、第二个 204。休眠与「清单 + 签名是两次独立请求」天然冲突
///    —— 一次成功一次失败拼在一起必然验签失败。静态资源无进程、无此问题。
/// ③ 市场与更新是**公开 GET**，不需要服务端进程；账号/发布类接口仍走平台服务端。
///
/// ⚠️ Pages 只托管 `public/` 下的静态文件。**改了地址必须重跑 `seed-manifests`
/// 并重新部署**：清单里的 `downloadUrl` 是**签名前写死的字节**，忘了重签会出现
/// 「清单验签通过（它确实被正确签过）但包指向另一个部署」的跨部署混用。
/// 守卫 `server/scripts/check-manifest-urls.mjs` 的主机名断言会拦下这种不一致。
///
/// ## ⚠️⚠️ 这里曾经被我改成 Pages，直接导致平台登录全挂（2026-09-30 修）
///
/// 症状：账号页「发起登录失败：服务端返回 **405**」。
/// 根因：**本常量同时管两类用途**，而我把需要后端进程的那一半也指到了静态托管上：
/// · 需要服务端进程：平台登录 / 平台 AI 额度 / 申请开发者 / 发布扩展（要 POST + 会话 token）
/// · 只需静态文件：市场清单 / 升级清单（公开 GET）
/// `POST /api/v1/auth/github/device/start` 打到 Pages → **405**（它把请求当成
/// 「对某个静态资源的非 GET 方法」）。
///
/// 教训正是本函数上方那句「**别只改一半**」—— 而那半正是我改的。
/// 地址**必须拆成两个常量**（`DEFAULT_SERVER_URL` 走 API，
/// `DEFAULT_ASSET_BASE_URL` 走静态），详见下面 `DEFAULT_ASSET_BASE_URL`。
///
/// ## v0.7.3 最终形态：两者是**同一个** Pages 域名（服务端已迁到 Pages Functions）
///
/// 第一版拆分时 API 还留在平台服务端，于是**又踩了一次「改一半」**：
/// 服务端迁走了、客户端没重新编译，实机一直报 `HTTP_ERROR: 服务端返回 403`
/// （那是平台服务端的访问保护；而新的 Pages 端点从这台机器测是 200）。
///
/// **教训比第一次更值得记**：服务端迁到别处时，「客户端指向哪里」和
/// 「密钥配在哪」是**两件独立的事**，改一件不会带动另一件。
/// 判据：**改完服务端地址后，必须重新编译并重装客户端**，
/// 否则你测的是新服务端、用的是烧死在旧二进制里的旧地址 —— 而症状
/// （403/405）看起来像服务端问题，会把人引向完全错误的方向。
///
/// 判据（下次换地址时先问这个）：**这个请求需要服务端进程吗？**
/// 需要 → API 端点；不需要 → 静态端点。清单地址是 GET 且带签名，可以静态托管；
/// 登录要 POST、还要服务端签发会话 token，纯静态托管做不到（但 Pages Functions 可以）。
pub const DEFAULT_SERVER_URL: &str = "https://m-hub-server.pages.dev";

/// **静态资源托管处**（市场清单 / 升级清单 / 扩展包 / 安装包），v0.7.3 起为 Cloudflare Pages。
///
/// 与 `DEFAULT_SERVER_URL` **必须分开写**，尽管值相同。理由见上面那两个 ⚠️⚠️ 段：
/// 二者的**部署形态不同**（静态文件 vs Functions + D1），一旦有人只改一个、
/// 或者把静态资源挪去别处，症状是「清单验签通过但包取不到」这类难查的问题。
/// 值相同是当前部署的结果，不是「可以合并成一个常量」的理由。
///
/// 两者路径**恰好同形**（`/api/v1/...`），所以静态托管方必须原样保留目录层级 ——
/// Pages 把目录内容映射到站点根，`server/public/api/v1/market/registry`
/// 正好对上客户端请求的路径。这也是为什么 `public/` 里不能有 `index.html`。
///
/// ⚠️ Pages 的 **Functions 优先于静态资产**（Workers 相反），故这四个签名清单
/// 会被 `functions/api/v1/[[path]].ts` 先撞上；服务端靠
/// `handle.ts::is_static_asset` 显式交还 `env.ASSETS` 才能拿到原始字节。
pub const DEFAULT_ASSET_BASE_URL: &str = "https://m-hub-server.pages.dev";

/// 市场清单接口路径（静态托管，`server/public/` 下同名目录；`.sig` 为同级 `{url}.sig`）。
pub const MARKET_REGISTRY_PATH: &str = "/api/v1/market/registry";

/// 应用升级清单接口路径（同上）。
pub const UPDATE_MANIFEST_PATH: &str = "/api/v1/app/update";

/// 市场清单地址（静态托管，`.sig` 为同级 `{url}.sig`）。
pub fn market_registry_url() -> String {
    format!("{DEFAULT_ASSET_BASE_URL}{MARKET_REGISTRY_PATH}")
}

/// 应用升级清单地址（静态托管，`.sig` 为同级 `{url}.sig`）。
pub fn update_manifest_url() -> String {
    format!("{DEFAULT_ASSET_BASE_URL}{UPDATE_MANIFEST_PATH}")
}

fn default_update_interval_hours() -> u64 {
    4
}

/// 环形菜单默认 6 个按钮（ADR 0004）：工作台 / 速记 / 速达 / 全局搜索 / 剪贴板 / 设置
pub fn default_floating_ball_buttons() -> Vec<String> {
    [
        "view:dashboard",
        "view:notes",
        "view:suda",
        "act:search",
        "act:clipboard",
        "view:settings",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            theme_mode: "light".to_string(),
            theme_preset: "indigo".to_string(),
            accent_color: None,
            wallpaper_path: String::new(),
            focus_enabled: false,
            focus_pins: default_focus_pins(),
            dnd_enabled: false,
            dnd_scheduled: false,
            dnd_start_hour: default_dnd_start_hour(),
            dnd_end_hour: default_dnd_end_hour(),
            auto_backup_dir: String::new(),
            auto_backup_hours: default_auto_backup_hours(),
            auto_backup_keep: default_auto_backup_keep(),
            auto_backup_last_ms: 0,
            wallpaper_blur: true,
            wallpaper_veil: default_wallpaper_veil(),
            wallpaper_immersive: false,
            glass_opacity: 1.0,
            sidebar_toggle: false,
            window: WindowState::default(),
            global_shortcut: crate::shortcut::DEFAULT_TOGGLE_SHORTCUT.to_string(),
            dashboard_mid_content: "countdown".to_string(),
            dashboard_layout: String::new(),
            countdown_sound: false,
            clock_quote: String::new(),
            notice_duration_ms: default_notice_duration_ms(),
            online_enabled: true,
            webview_mem_low_on_hide: true,
            weather_city: String::new(),
            weather_lat: 0.0,
            weather_lng: 0.0,
            quote_source: "online".to_string(),
            chat_models: default_chat_models(),
            chat_panel_width: 420.0,
            chat_panel_open: false,
            chat_panel_side: "right".to_string(),
            chat_panel_height: 380.0,
            chat_panel_opacity: 1.0,
            chat_window_mode: false,
            chat_window_width: default_chat_window_width(),
            chat_window_height: default_chat_window_height(),
            chat_window_x: None,
            chat_window_y: None,
            chat_window_pinned: false,
            extension_row_click: default_extension_row_click(),
            clipboard_shortcut: crate::shortcut::DEFAULT_CLIPBOARD_SHORTCUT.to_string(),
            search_shortcut: crate::shortcut::DEFAULT_SEARCH_SHORTCUT.to_string(),
            chat_shortcut: crate::shortcut::DEFAULT_CHAT_SHORTCUT.to_string(),
            capture_shortcut: crate::shortcut::DEFAULT_CAPTURE_SHORTCUT.to_string(),
            notes_shortcut: crate::shortcut::DEFAULT_NOTES_SHORTCUT.to_string(),
            // 全部默认开 —— 升级已有配置的用户不该发现自己的快捷键「突然没了」
            shortcut_toggle_enabled: true,
            shortcut_clipboard_enabled: true,
            shortcut_search_enabled: true,
            shortcut_chat_enabled: true,
            shortcut_capture_enabled: true,
            shortcut_notes_enabled: true,
            clipboard_max_items: 500,
            clipboard_ttl_days: 7,
            notes_trash_days: default_notes_trash_days(),
            clipboard_paused: false,
            clipboard_paste_method: "auto".to_string(),
            suda_web_open_mode: "panel".to_string(),
            suda_custom_modules: Vec::new(),
            suda_panel_toolbar: false,
            clipboard_image_enabled: true,
            clipboard_file_enabled: true,
            font_scale: 1.0,
            font_sticky: 1.0,
            font_notes: 1.0,
            font_prompt: 1.0,
            font_todo: 1.0,
            note_editor_mode: default_note_editor_mode(),
            runtime_strategy: "auto".to_string(),
            auto_trust_service: false,
            sidebar_extensions: Vec::new(),
            extension_open_modes: std::collections::HashMap::new(),
            extension_link_modes: std::collections::HashMap::new(),
            // 废弃字段（不再被读取）：市场清单地址真相源是 config::market_registry_url()
            market_endpoint: String::new(),
            dev_mode_enabled: false, // 已废弃字段：仅为兼容旧 app.json 保留，不再读取
            dev_extensions: Vec::new(),
            skill_roots: Vec::new(),
            // 废弃字段（不再被读取）：地址真相源是 DEFAULT_SERVER_URL，见字段注释
            server_url: String::new(),
            run_at_startup: false,
            // 废弃字段（不再被读取）：升级清单地址真相源是 config::update_manifest_url()
            update_endpoint: String::new(),
            auto_update_enabled: true,
            update_interval_hours: default_update_interval_hours(),
            skipped_update_version: String::new(),
            update_snooze_until_ms: 0,
            floating_ball_enabled: true,
            floating_ball_auto_hide: true,
            floating_ball_with_main: false,
            floating_ball_buttons: default_floating_ball_buttons(),
            floating_ball_x: None,
            floating_ball_y: None,
            floating_ball_idle_spin: false,
        }
    }
}

/// 预置一条 DeepSeek 官方配置作为开箱即用示例（用户可删可改可加）
pub fn default_chat_models() -> Vec<ChatModelConfig> {
    vec![ChatModelConfig {
        id: "deepseek-default".to_string(),
        name: "DeepSeek".to_string(),
        provider_name: "DeepSeek".to_string(),
        base_url: "https://api.deepseek.com/v1".to_string(),
        model: "deepseek-v4-flash".to_string(),
        api_key: String::new(),
        is_default: true,
        has_api_key: false,
    }]
}

pub fn config_dir() -> PathBuf {
    // 配置与数据库同挂数据根：更改数据目录后 app.json 也随数据走，
    // U 盘便携时配置一并继承（数据根解析见 paths.rs）
    crate::paths::data_root().to_path_buf()
}

pub fn config_file() -> PathBuf {
    config_dir().join("app.json")
}

pub fn load() -> AppConfig {
    load_from(&config_file())
}

pub fn load_from(path: &Path) -> AppConfig {
    match fs::read_to_string(path) {
        Ok(content) => match serde_json::from_str::<AppConfig>(&content) {
            Ok(config) => {
                let mut config = config;
                // 迁移结果必须落盘：只改内存的话每次启动都会重新迁移，
                // 磁盘上那份已停用的地址永远留着（用户手工看一眼还是会被误导）。
                if normalize(&mut config) {
                    if let Err(e) = save_to(&config, path) {
                        log::warn!("配置迁移结果写入失败（本次仍按迁移后的值运行）: {e}");
                    }
                }
                config
            }
            Err(_) => {
                // 配置文件损坏：备份并回退默认
                let _ = fs::copy(path, path.with_extension("json.bak"));
                let default = AppConfig::default();
                let _ = save_to(&default, path);
                default
            }
        },
        Err(_) => AppConfig::default(),
    }
}

/// 旧默认语录「日拱一卒」迁移：v0.1.19 起语录改为随机名言金句，
/// 旧默认值视为「未自定义」，置空以启用随机金句。
///
/// 返回值 = 是否有改动（有则调用方负责落盘）。
fn normalize(config: &mut AppConfig) -> bool {
    let mut changed = false;
    if config.clock_quote == "日拱一卒，功不唐捐。" {
        config.clock_quote = String::new();
        changed = true;
    }
    if migrate_legacy_endpoints(config) {
        changed = true;
    }
    changed
}

/// `market_endpoint` / `update_endpoint` 两个字段自 v0.6.1 起**已废弃不再被读取**（清单地址
/// 由 [`market_registry_url`] / [`update_manifest_url`] 按服务端地址拼），但老 `app.json`
/// 里会残留 R2/COS 时代的地址：0.6.0 及更早的安装升级上来后，文件里的旧地址与程序实际
/// 行为不一致，人工排查时会被误导（表现为「市场源异常：…404」但界面无从自救）。
/// 这里把任何非服务端的残留值一句话归一并落盘；空值同样归一，保证文件里始终是当前真相。
fn migrate_legacy_endpoints(config: &mut AppConfig) -> bool {
    let mut changed = false;
    let registry = market_registry_url();
    if config.market_endpoint.trim() != registry {
        if !config.market_endpoint.trim().is_empty() {
            log::info!(
                "市场源字段已废弃（v0.6.1 起走服务端接口），{} → {}",
                config.market_endpoint,
                registry
            );
        }
        config.market_endpoint = registry;
        changed = true;
    }
    let manifest = update_manifest_url();
    if config.update_endpoint.trim() != manifest {
        if !config.update_endpoint.trim().is_empty() {
            log::info!(
                "更新源字段已废弃（v0.6.1 起走服务端接口），{} → {}",
                config.update_endpoint,
                manifest
            );
        }
        config.update_endpoint = manifest;
        changed = true;
    }
    changed
}

/// 后端管理的字段清单（`merge_disk_authoritative` 保留哪些字段）。
///
/// 判定标准：**只由后端命令写盘、前端不回写**（前端 `state.config` 是启动快照）。
/// 新增这类字段时必须同步两处：① `merge_disk_authoritative` 里合并它；
/// ② 这里登记名字 —— 回归测试 `merge_keeps_backend_managed_fields` 逐个按此清单断言，
/// 漏了任何一步都会红（这个坑踩过两次，两次都是用户升级后才发现的）。
#[cfg(test)]
const BACKEND_MANAGED_FIELDS: &[&str] = &[
    // AI 对话模型配置：只经 save_chat_models 变更
    "chat_models",
    // AI 对话独立窗：开关经 chat_window_save_mode、几何由拖拽/缩放记忆
    "chat_window_mode",
    "chat_window_width",
    "chat_window_height",
    "chat_window_x",
    "chat_window_y",
    "chat_window_pinned",
    // 悬浮球：开关经 save_settings、位置由 drag_end 记忆
    "floating_ball_enabled",
    "floating_ball_auto_hide",
    "floating_ball_with_main",
    "floating_ball_buttons",
    "floating_ball_x",
    "floating_ball_y",
    "floating_ball_idle_spin",
    // 剪贴板「粘贴方式」：前端只读（FeaturesPanel 用本地 ref，改动只经
    // set_clipboard_paste_method 命令写盘）。不登记的话，同一面板里改一下
    // 「记录剪贴板图片」就会 saveConfig 整份快照，把它冲回启动时的旧值。
    // 这条在 macOS 上尤其扎眼：约定 70 刚修好「选 ⌘⇧V 却发 ⌘V」，
    // 用户为此去改这个下拉框，然后被无声吃回去。
    "clipboard_paste_method",
    // AI 对话抽屉面板几何/开合：前端只读（index.vue 用本地 ref，改动只经
    // set_chat_panel 命令写盘）。不登记则拖完面板改任意设置就回弹。
    "chat_panel_width",
    "chat_panel_height",
    "chat_panel_open",
    // 主窗位置与尺寸：只经 lib.rs::persist_window_state 写盘（点 × 隐藏到托盘时）。
    // 整块登记是安全的 —— 其中的 always_on_top 由 set_always_on_top_config
    // 走「锁内读-改-写磁盘」，磁盘同样是权威源。
    "window",
    // 「我的扩展」本机源码目录：只经 add/remove_dev_extension 变更
    "dev_extensions",
    "dev_mode_enabled",
    // 已废弃的旧服务端地址（约定 52）：与 market_endpoint / update_endpoint
    // 同属「已废弃但仍要保住磁盘值」的字段，只写盘、从不读取。
    // 不登记的话，每次 saveConfig 都会用 AppConfig::default() 的空串覆盖磁盘 ——
    // 后果是老 app.json 里残留的开发期地址被悄悄抹掉（功能上无影响，
    // 但排查历史问题时那份现场证据没了）。登记后与另两个端点字段口径一致。
    "server_url",
    // Skills 自定义安装根：只经 install_skill / remove_skill_root 变更
    "skill_roots",
    // 「跳过此版本」：只经 skip_update_version 变更
    "skipped_update_version",
    // 「稍后再提示」暂停到期时间：只经 snooze_update 变更
    "update_snooze_until_ms",
    // 已废弃的两个端点字段（v0.6.1）：真相源是内置常量，只由 migrate_legacy_endpoints 归一
    "market_endpoint",
    "update_endpoint",
];

/// `save_config` 的「以磁盘为准」合并（**纯函数，便于回归测试**）：把前端整份提交的配置
/// 与磁盘上的当前配置合并，后端管理的字段一律取磁盘值，其余（用户可编辑项）以前端提交为准。
///
/// 为什么必须这样：前端提交的是**启动快照**（`state.config`），既不认识也不会回写这些后端字段，
/// 让它们跟着快照落盘就等于「保存任意设置 = 把这些字段回滚到启动时刻」。最迷惑的一次是
/// 「加完源码目录顺手把卡片拖进工作台」→ `setDashboardLayout` 整份保存 → 刚加的扩展从
/// 「我的扩展」里凭空消失（扩展其实还在运行、卡片也还在）。
pub fn merge_disk_authoritative(merged: &mut AppConfig, disk: &AppConfig) {
    // 独立窗几何/开关（chat_window::preserve_disk_fields）
    crate::chat_window::preserve_disk_fields(merged, disk);
    merged.chat_models = disk.chat_models.clone();
    merged.floating_ball_enabled = disk.floating_ball_enabled;
    merged.floating_ball_auto_hide = disk.floating_ball_auto_hide;
    merged.floating_ball_with_main = disk.floating_ball_with_main;
    merged.floating_ball_buttons = disk.floating_ball_buttons.clone();
    merged.floating_ball_x = disk.floating_ball_x;
    merged.floating_ball_y = disk.floating_ball_y;
    merged.floating_ball_idle_spin = disk.floating_ball_idle_spin;
    // 剪贴板粘贴方式：前端快照里是启动时的值，改动只经命令写盘（见登记处说明）
    merged.clipboard_paste_method = disk.clipboard_paste_method.clone();
    // AI 对话抽屉面板几何/开合：同上
    merged.chat_panel_width = disk.chat_panel_width;
    merged.chat_panel_height = disk.chat_panel_height;
    merged.chat_panel_open = disk.chat_panel_open;
    // 主窗位置与尺寸：整块以磁盘为准。always_on_top 也在其中，但由
    // set_always_on_top_config 锁内读-改-写磁盘，磁盘值本就是最新的。
    merged.window = disk.window.clone();
    merged.dev_extensions = disk.dev_extensions.clone();
    // 已废弃字段（登记即加载后不再读取），仍以磁盘为准以免被快照写回
    merged.dev_mode_enabled = disk.dev_mode_enabled;
    // 已废弃的两个端点字段（v0.6.1）：前端启动快照里已经没有它们，不合并就会把磁盘上
    // 归一后的服务端地址覆盖成空串（行为无影响，但文件里会来回翻烧饼）
    merged.market_endpoint = disk.market_endpoint.clone();
    merged.update_endpoint = disk.update_endpoint.clone();
    merged.server_url = disk.server_url.clone();
    merged.skill_roots = disk.skill_roots.clone();
    merged.skipped_update_version = disk.skipped_update_version.clone();
    // 「稍后再提示」到期时间由 snooze_update 命令独占写盘，前端快照里只有启动时的旧值；
    // 不合并的话，暂停窗口内保存任意设置（save_config 整份快照落盘）都会把它冲回旧值，
    // 「稍后再提示」被悄悄取消、更新弹窗下一轮自动检查又弹出来
    merged.update_snooze_until_ms = disk.update_snooze_until_ms;
}

pub fn save(config: &AppConfig) -> Result<(), String> {
    save_to(config, &config_file())
}

pub fn save_to(config: &AppConfig, path: &Path) -> Result<(), String> {
    let dir = path
        .parent()
        .ok_or_else(|| "配置目录无效".to_string())?;
    fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let tmp_path = path.with_extension("json.tmp");
    let json = serde_json::to_string_pretty(config).map_err(|e| e.to_string())?;
    // 原子写入：临时文件 + rename
    let mut tmp = fs::File::create(&tmp_path).map_err(|e| e.to_string())?;
    tmp.write_all(json.as_bytes()).map_err(|e| e.to_string())?;
    tmp.sync_all().map_err(|e| e.to_string())?;
    fs::rename(&tmp_path, path).map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config() {
        let c = AppConfig::default();
        assert_eq!(c.theme_mode, "light");
        assert_eq!(c.theme_preset, "indigo");
        assert!(c.accent_color.is_none());
        assert!(!c.sidebar_toggle);
        assert_eq!(c.window.width, 1400.0);
        assert!(!c.window.always_on_top);
        assert_eq!(c.global_shortcut, crate::shortcut::DEFAULT_TOGGLE_SHORTCUT);
        assert_eq!(c.dashboard_mid_content, "countdown");
        assert_eq!(c.note_editor_mode, "wysiwyg");
    }

    #[test]
    fn note_editor_mode_missing_falls_back_to_wysiwyg() {
        let mut value = serde_json::to_value(AppConfig::default()).unwrap();
        value.as_object_mut().unwrap().remove("note_editor_mode");
        let loaded: AppConfig = serde_json::from_value(value).unwrap();
        assert_eq!(loaded.note_editor_mode, "wysiwyg");
    }

    #[test]
    fn save_to_and_load_from_roundtrip() {
        let mut config = AppConfig::default();
        config.theme_mode = "dark".to_string();
        config.theme_preset = "midnight".to_string();
        config.accent_color = Some("#8b8bff".to_string());
        config.sidebar_toggle = true;
        config.window.width = 1280.0;
        config.window.x = Some(100.0);
        config.window.always_on_top = true;

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("app.json");
        save_to(&config, &path).unwrap();

        let loaded = load_from(&path);
        assert_eq!(loaded.theme_mode, "dark");
        assert_eq!(loaded.theme_preset, "midnight");
        assert_eq!(loaded.accent_color.as_deref(), Some("#8b8bff"));
        assert!(loaded.sidebar_toggle);
        assert_eq!(loaded.window.width, 1280.0);
        assert_eq!(loaded.window.x, Some(100.0));
        assert!(loaded.window.always_on_top);
    }

    #[test]
    fn corrupted_config_falls_back_to_default() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("app.json");
        fs::write(&path, "not valid json {{{").unwrap();
        let loaded = load_from(&path);
        assert_eq!(loaded.theme_mode, "light");
        assert!(path.with_extension("json.bak").exists());
    }

    #[test]
    fn missing_config_returns_default() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nonexistent.json");
        let loaded = load_from(&path);
        assert_eq!(loaded.theme_mode, "light");
    }

    #[test]
    fn old_theme_field_migrates_to_theme_mode() {
        // 旧版配置格式：只有 `theme` 字段（light/dark），
        // 依赖 serde `alias = "theme"` 自动映射到 theme_mode
        let old_json = serde_json::json!({
            "theme": "dark",
            "window": {
                "width": 1400.0,
                "height": 900.0,
                "x": null,
                "y": null,
                "always_on_top": false
            },
            "global_shortcut": "Ctrl+Shift+Space",
            "dashboard_mid_content": "countdown",
            "countdown_sound": false
        })
        .to_string();

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("app.json");
        fs::write(&path, old_json).unwrap();

        let loaded = load_from(&path);
        assert_eq!(loaded.theme_mode, "dark");
        assert_eq!(loaded.theme_preset, "indigo");
        assert!(loaded.accent_color.is_none());
    }

    // ---------------- 已废弃端点字段的归一迁移 ----------------
    // 守的是「老 app.json 里的 R2/COS 地址没有任何迁移、升级后刷新市场永远 404」那个坑：
    // 0.6.0 已发布后仍在多台机器上复现，界面上又没有任何入口能改回来。
    // v0.6.1 起两个字段本身废弃（地址改为按服务端地址拼），归一迁移保证文件里不再留着
    // 会误导排查的旧地址。

    #[test]
    fn legacy_endpoint_values_are_normalized_and_persisted() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("app.json");
        let mut cfg = AppConfig::default();
        // R2 时代残留 + COS 时代残留（0.6.0 及更早安装写进 app.json 的默认值）
        cfg.market_endpoint = "https://r2.dckxx.com/extensions/registry.json".to_string();
        cfg.update_endpoint =
            "https://m-hub-dist-1251402600.cos.ap-guangzhou.myqcloud.com/releases/update.json"
                .to_string();
        fs::write(&path, serde_json::to_string_pretty(&cfg).unwrap()).unwrap();

        let loaded = load_from(&path);
        assert_eq!(loaded.market_endpoint, market_registry_url());
        assert_eq!(loaded.update_endpoint, update_manifest_url());

        // 归一必须落盘（否则用户手看 app.json 仍是旧地址，且每次启动都要重写一遍）
        let on_disk = fs::read_to_string(&path).unwrap();
        assert!(on_disk.contains(MARKET_REGISTRY_PATH));
        assert!(on_disk.contains(UPDATE_MANIFEST_PATH));
        assert!(!on_disk.contains("r2.dckxx.com"));
        assert!(!on_disk.contains("cos.ap-guangzhou"));
    }

    #[test]
    fn api_and_static_bases_must_stay_distinct() {
        // 2026-09-30 实机事故：把 DEFAULT_SERVER_URL 一起改成 Cloudflare Pages，
        // 账号页立刻变成「发起登录失败：服务端返回 405」——
        // 因为 `POST /api/v1/auth/github/device/start` 打到静态托管上了。
        //
        // 下面是**当时那条测试抓不到的原因**，值得记下来：
        // `default_config_gets_canonical_endpoints` 断言的是
        // `market_registry_url().starts_with(DEFAULT_SERVER_URL)`，
        // 而我当时把**两个常量改成了同一个值** → 断言必然通过。
        // 也就是说，那条测试守的其实是「两个地址相等」——而那恰好就是 bug 本身。
        //
        // ⚠️ 注意：**「不相等」不再是断言**，见下一条 `api_and_asset_bases_are_deployed_together`。
        // 拆分常量的目的从来不是「让它们不同」，而是「让改动时必须同时想到两个」。
        // 服务端整体迁到 Pages 后两者**确实相同**了 —— 那时该加的是「同步」的守卫，
        // 而不是继续断言「不同」。

        // 无论同不同，谁在用哪个必须写死：清单走静态、登录走 API。
        //
        // 这里曾有第四条 `assert!(!market_registry_url().starts_with(DEFAULT_SERVER_URL))`
        // —— 它只在「两个常量不同」时成立，服务端整体迁到 Pages 后就**恒假**。
        // 留着的教训：**常量同址后，依赖它们差异的断言会变成永久红灯**，
        // 而红灯会让人以为新改动有问题，进而把正确的断言改错（我这次就差点那么做）。
        assert!(market_registry_url().starts_with(DEFAULT_ASSET_BASE_URL));
        assert!(update_manifest_url().starts_with(DEFAULT_ASSET_BASE_URL));
        assert!(crate::account::server_url().starts_with(DEFAULT_SERVER_URL));
    }

    #[test]
    fn api_and_asset_bases_are_deployed_together() {
        // v0.7.3：服务端整体迁到 Cloudflare Pages（Functions + D1），
        // 两个常量的值随之**都**变成 pages.dev。
        //
        // 这条守的是 2026-10-01 那次事故：服务端迁走了、密钥也配到 Pages 了，
        // 但**客户端没有重新编译**，于是用的还是烧死在旧二进制里的
        // `m-hub-server.pocketbay.app` → 实机一直 403。
        // 而那 403 是平台服务端的访问保护，**看起来完全像服务端问题**，
        // 我因此往「服务端配置」方向查了好几轮。
        //
        // 为什么 Rust 测试能抓到：值是编译期常量，只要改这里就会变。
        // 抓不到的是「改了但没重新编译」—— 那要靠流程，不是靠断言。
        // 故本测试的价值是：**下次换地址时它会提醒你两个常量是一对**。
        assert_eq!(
            DEFAULT_ASSET_BASE_URL, DEFAULT_SERVER_URL,
            "服务端已整体迁到 Pages，两个常量必须同址。\
             只改一个的症状是「清单正常但登录失败」或反过来 —— \
             排查方向会被带偏（2026-10-01 踩过：客户端没重编，一直打旧地址得 403）。"
        );
        assert!(
            DEFAULT_SERVER_URL.starts_with("https://"),
            "必须是 https：http 会 301，而 reqwest 默认把 301 的 POST 降级成 GET 并丢 body"
        );
    }

    #[test]
    fn default_config_gets_canonical_endpoints() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("app.json");
        fs::write(&path, serde_json::to_string_pretty(&AppConfig::default()).unwrap()).unwrap();

        let loaded = load_from(&path);
        assert_eq!(loaded.market_endpoint, market_registry_url());
        assert_eq!(loaded.update_endpoint, update_manifest_url());
        // 两个地址都必须落在静态托管域名下，且带 https
        // （http 会 301，而 reqwest 默认把 301 的 POST 降级成 GET 并丢 body）
        //
        // ⚠️ 这里断言的是 `DEFAULT_ASSET_BASE_URL` 而**不是** `DEFAULT_SERVER_URL`。
        // 原先断言后者，而当时两个常量已被我改成同一个值 —— 于是这条测试在
        // 「平台登录全挂」的状态下**照样通过**。它守的不是「地址对」，
        // 只是「两个地址相等」，而那恰好就是 bug 本身。
        for url in [market_registry_url(), update_manifest_url()] {
            assert!(url.starts_with("https://"));
            assert!(url.starts_with(DEFAULT_ASSET_BASE_URL));
        }
    }

    // ---------------- merge_disk_authoritative 回归测试 ----------------
    // 这一组守的是「后端管理的字段被前端启动快照整份覆盖」那个坑（踩过两次）。
    // 反向也守：用户可编辑项必须仍然以前端提交为准，不能被合并顺手冻住。

    /// 造一份「磁盘上的当前配置」：后端管理的字段全部设成**非默认值**，
    /// 这样「被快照覆盖」一定表现为断言失败，而不是恰好等于默认值蒙混过关。
    fn disk_with_backend_values() -> AppConfig {
        AppConfig {
            chat_models: vec![ChatModelConfig {
                id: "disk-model".to_string(),
                name: "磁盘上的模型".to_string(),
                base_url: "https://example.invalid/v1".to_string(),
                model: "disk-model-v1".to_string(),
                api_key: String::new(),
                is_default: true,
                has_api_key: false,
                provider_name: "Disk".to_string(),
            }],
            chat_window_mode: true,
            chat_window_width: 777.0,
            chat_window_height: 666.0,
            chat_window_x: Some(111.0),
            chat_window_y: Some(222.0),
            chat_window_pinned: true,
            floating_ball_enabled: false,
            floating_ball_auto_hide: false,
            floating_ball_with_main: true,
            floating_ball_buttons: vec!["view:disk".to_string()],
            floating_ball_x: Some(333.0),
            floating_ball_y: Some(444.0),
            dev_extensions: vec!["E:\\src\\my-ext".to_string()],
            dev_mode_enabled: true,
            skill_roots: vec!["E:\\skills-custom".to_string()],
            skipped_update_version: "9.9.9".to_string(),
            update_snooze_until_ms: 1_893_456_000_000,
            ..AppConfig::default()
        }
    }

    /// 模拟前端整份提交里「后端字段被冲掉」的那一半：把磁盘配置序列化后**删掉登记的键**
    /// 再反序列化回来 —— 复现「前端不认识这些字段」的路径（如 `skill_roots`）。
    /// 注意另一条路径（前端认识、但带的是启动快照旧值，如 `chat_models`/`floating_ball_*`）
    /// 的形态等价：值不是当前的磁盘值。两条都由下面的断言覆盖。
    /// 补缺用的是 `AppConfig::default()` 的**同名字段值**（容器级 `#[serde(default)]`），
    /// 不是字段类型的 `Default` —— 所以默认模型/默认悬浮球按钮会填进来，而不是空值。
    fn snapshot_without_backend_fields(disk: &AppConfig) -> AppConfig {
        let mut value = serde_json::to_value(disk).unwrap();
        let obj = value.as_object_mut().unwrap();
        for field in BACKEND_MANAGED_FIELDS {
            obj.remove(*field);
        }
        serde_json::from_value(value).unwrap()
    }

    #[test]
    /// 前端整份回写配置时，聚焦模式这两个字段必须**原样回来**。
    ///
    /// 为什么要单独测：前端的 `setFocusMode` 走的是 `saveConfig(state.config)` ——
    /// 把**启动快照**整份送回后端存盘。而快照是在 `focus_enabled` / `focus_pins`
    /// 这两个字段出现**之前**序列化出来的（老用户的磁盘配置里根本没有它们）。
    /// 一次整份回写就会把它们抹掉，于是「聚焦模式开着，重启后自己关掉了」。
    ///
    /// 这类「回写丢字段」的坑本工程踩过两次，所以用 serde 往返 + 逐字段断言钉死：
    /// 少了 `#[serde(default)]` 或字段名写错，这里立刻红。
    #[test]
    fn focus_fields_survive_a_snapshot_round_trip() {
        // 老配置：只有极少数字段（模拟用户升级前的 app.json）
        let old: serde_json::Value = serde_json::json!({ "theme_preset": "green" });
        let parsed: AppConfig = serde_json::from_value(old).expect("老配置应能解析");

        // 前端快照形态：两个字段都带上
        let mut snapshot = parsed.clone();
        snapshot.focus_enabled = true;
        snapshot.focus_pins = vec!["clock".into(), "prompts".into()];

        // 走一遍 serde 往返 = 一次落盘再读回
        let text = serde_json::to_string(&snapshot).unwrap();
        let back: AppConfig = serde_json::from_str(&text).unwrap();
        assert!(back.focus_enabled, "聚焦开关必须活过一次落盘");
        assert_eq!(
            back.focus_pins,
            vec!["clock".to_string(), "prompts".to_string()],
            "聚焦模块列表与顺序必须活过一次落盘"
        );

        // ⚠️ 上面那段往返**抓不住字段被改名**：往返两侧用的是同一个结构体，
        // 改名后它自己跟自己一致，照样通过（实测给字段加 `#[serde(rename = "focus_pin")]`
        // 后，这条测试依旧是绿的 —— 一个假测试比没有测试更糟）。
        // 真正要防的是「改名把磁盘上的老配置读不出来」，所以这里必须用
        // **手写字面量 JSON** 来钉住盘上的键名。
        let on_disk = serde_json::from_str::<AppConfig>(
            r#"{ "focus_enabled": true, "focus_pins": ["clock", "prompts"] }"#,
        )
        .expect("盘上的键名必须正是 focus_enabled / focus_pins");
        assert!(
            on_disk.focus_enabled,
            "\"focus_enabled\" 这个盘上键名被改名了，老配置会读不出聚焦开关"
        );
        assert_eq!(
            on_disk.focus_pins,
            vec!["clock".to_string(), "prompts".to_string()],
            "\"focus_pins\" 这个盘上键名被改名了（且顺序必须保留）"
        );

        // 缺字段时必须落回**默认值**，而不是空/报错。
        // 这里靠的是 AppConfig 上的**容器级** `#[serde(default)]` —— 它让整个结构体
        // 在字段缺失时用 Default::default()。所以单字段的 `#[serde(default = "...")]`
        // 在这里是冗余的：实测把它删掉，这条断言照样绿。
        // 保留它是为了把「这个字段的默认值是什么」写在字段旁边（容器级 default
        // 只会在 AppConfig::default() 里体现，字段旁反而看不见）。
        let fresh: AppConfig = serde_json::from_str("{}").unwrap();
        assert!(!fresh.focus_enabled, "聚焦默认必须关");
        assert_eq!(
            fresh.focus_pins,
            vec!["todo".to_string(), "sticky1".to_string(), "countdown".to_string()],
            "聚焦模块默认值必须是待办 / 便签 / 倒计时"
        );
    }

    fn merge_keeps_backend_managed_fields() {
        let disk = disk_with_backend_values();
        let snapshot = snapshot_without_backend_fields(&disk);

        // 前提校验：快照确实与磁盘不同（否则这个测试什么也没守）
        assert_ne!(
            snapshot.dev_extensions, disk.dev_extensions,
            "快照里的 dev_extensions 应当已被清空，测试前提不成立"
        );
        assert_ne!(
            snapshot.skill_roots, disk.skill_roots,
            "快照里的 skill_roots 应当已被清空，测试前提不成立"
        );
        // 注意：容器级 `#[serde(default)]` 会用 `AppConfig::default()` 的**同名字段值**补缺，
        // 所以缺字段的快照拿到的是默认模型/默认悬浮球按钮，而不是空值 —— 正是旧快照的形状。
        assert_ne!(
            serde_json::to_value(&snapshot.chat_models).unwrap(),
            serde_json::to_value(&disk.chat_models).unwrap()
        );

        let mut merged = snapshot.clone();
        merge_disk_authoritative(&mut merged, &disk);

        // 比对整体 JSON：字段级断言写漏了也逃不掉（JSON 里每个键都要等于磁盘值）
        assert_eq!(
            serde_json::to_value(&merged.chat_models).unwrap(),
            serde_json::to_value(&disk.chat_models).unwrap()
        );
        assert_eq!(merged.chat_window_mode, disk.chat_window_mode);
        assert_eq!(merged.chat_window_width, disk.chat_window_width);
        assert_eq!(merged.chat_window_height, disk.chat_window_height);
        assert_eq!(merged.chat_window_x, disk.chat_window_x);
        assert_eq!(merged.chat_window_y, disk.chat_window_y);
        assert_eq!(merged.chat_window_pinned, disk.chat_window_pinned);
        assert_eq!(merged.floating_ball_enabled, disk.floating_ball_enabled);
        assert_eq!(merged.floating_ball_auto_hide, disk.floating_ball_auto_hide);
        assert_eq!(
            merged.floating_ball_with_main,
            disk.floating_ball_with_main
        );
        assert_eq!(merged.floating_ball_buttons, disk.floating_ball_buttons);
        assert_eq!(merged.floating_ball_x, disk.floating_ball_x);
        assert_eq!(merged.floating_ball_y, disk.floating_ball_y);
        assert_eq!(merged.dev_extensions, disk.dev_extensions);
        assert_eq!(merged.dev_mode_enabled, disk.dev_mode_enabled);
        assert_eq!(merged.skill_roots, disk.skill_roots);
        assert_eq!(merged.skipped_update_version, disk.skipped_update_version);
        assert_eq!(merged.update_snooze_until_ms, disk.update_snooze_until_ms);
    }

    /// 清单漏登就是这条红：任何登记在案的名字都必须是 `AppConfig` 真实存在的字段，
    /// 且合并后确实取到了磁盘值（名字打错/字段改名都能被抓到）。
    #[test]
    fn backend_managed_field_list_is_real_and_effective() {
        let disk = disk_with_backend_values();
        let mut merged = snapshot_without_backend_fields(&disk);
        merge_disk_authoritative(&mut merged, &disk);

        let merged_json = serde_json::to_value(&merged).unwrap();
        let disk_json = serde_json::to_value(&disk).unwrap();
        for field in BACKEND_MANAGED_FIELDS {
            assert!(
                merged_json.get(field).is_some(),
                "BACKEND_MANAGED_FIELDS 里的 `{field}` 不是 AppConfig 的字段（名字写错或字段已改名）"
            );
            assert_eq!(
                merged_json.get(field),
                disk_json.get(field),
                "`{field}` 声明为后端管理，但合并后没有取磁盘值"
            );
        }
    }

    /// 反向：用户可编辑项必须仍以前端提交为准（合并别把手伸过头，把所有设置都冻成磁盘旧值）。
    #[test]
    fn merge_keeps_user_editable_fields_from_snapshot() {
        let disk = AppConfig::default();
        let mut snapshot = snapshot_without_backend_fields(&disk);
        snapshot.theme_mode = "dark".to_string();
        snapshot.theme_preset = "midnight".to_string();
        snapshot.accent_color = Some("#8b8bff".to_string());
        snapshot.sidebar_toggle = true;
        snapshot.dashboard_layout = r#"[{"id":"clock"}]"#.to_string();

        let mut merged = snapshot.clone();
        merge_disk_authoritative(&mut merged, &disk);

        assert_eq!(merged.theme_mode, "dark");
        assert_eq!(merged.theme_preset, "midnight");
        assert_eq!(merged.accent_color.as_deref(), Some("#8b8bff"));
        assert!(merged.sidebar_toggle);
        assert_eq!(merged.dashboard_layout, snapshot.dashboard_layout);

        // ⚠️ `window` 曾经被这条断言锁成「取前端快照值」（当年写的是
        // `snapshot.window.width = 1280.0; assert_eq!(merged.window.width, 1280.0)`），
        // 那正是 bug 本身：位置尺寸只经 `persist_window_state` 写盘，前端从不写，
        // 快照里永远是启动时的旧值，于是「拖完窗口 → 改任意设置 → 下次启动弹回原处」。
        // 现在 `window` 登记为后端管理字段，`snapshot_without_backend_fields`
        // 会把它整块剥掉，合并后应当等于 **disk**。
        assert_eq!(
            merged.window.width, disk.window.width,
            "窗口尺寸必须以磁盘为准，而不是前端启动快照"
        );
        assert_eq!(merged.window.height, disk.window.height);
    }

    /// 后端独占写盘的字段**不能被前端快照覆盖**。
    ///
    /// 这条是 A4/A5/A6 三个漏登记 bug 的正面防线：它们能长期存活，
    /// 正是因为 `config::tests` 只验证「清单里的字段合并后等于磁盘值」，
    /// 验证不了「清单外、但后端独占写的字段被快照吃掉了」。
    #[test]
    fn backend_only_fields_survive_a_snapshot_save() {
        let mut disk = AppConfig::default();
        // 模拟：用户改了这些只有后端会写盘的项
        disk.clipboard_paste_method = "ctrl_shift_v".into();
        disk.chat_panel_width = 777.0;
        disk.chat_panel_height = 555.0;
        disk.chat_panel_open = true;
        disk.window.width = 1234.0;
        disk.window.x = Some(42.0);

        // 模拟前端那份「启动快照」：这些字段都停在更早的值
        let mut snapshot = AppConfig::default();
        snapshot.clipboard_paste_method = "auto".into();
        snapshot.chat_panel_width = 420.0;
        snapshot.chat_panel_height = 380.0;
        snapshot.chat_panel_open = false;
        snapshot.window.width = 1400.0;
        snapshot.window.x = None;

        let mut merged = snapshot.clone();
        merge_disk_authoritative(&mut merged, &disk);

        assert_eq!(merged.clipboard_paste_method, "ctrl_shift_v", "粘贴方式被快照回滚了");
        assert_eq!(merged.chat_panel_width, 777.0, "对话面板宽度被快照回滚了");
        assert_eq!(merged.chat_panel_height, 555.0, "对话面板高度被快照回滚了");
        assert!(merged.chat_panel_open, "对话面板开合态被快照回滚了");
        assert_eq!(merged.window.width, 1234.0, "窗口宽度被快照回滚了");
        assert_eq!(merged.window.x, Some(42.0), "窗口位置被快照回滚了");
    }
}
