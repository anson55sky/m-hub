# m-hub (个人效率工作台) — macOS

**生成:** 2026-08-29 | **分支:** master | **版本:** 0.8.0
**平台:** macOS（本仓库是上游 [x-hub](https://github.com/dckxx/x-hub) 的 macOS 移植版）

## 概述

基于 Tauri 2 + Vue 3 + Tailwind CSS 4 的本地桌面效率工作台（Bento 风格 Dashboard）。侧栏导航含 **工作台**（自由网格布局：时钟/天气/便签×2/系统监视/提示词/待办/日历/倒计时/概览卡 + 最近使用通栏，时钟与天气支持多形态展示，可经布局编辑器所见即所得编排）、**待办**（标签筛选 / 月·周日历 / 周期待办 / 置顶）、**速记**（笔记）、**速达**（应用/网页/文件资源），另有**扩展中心/扩展视图**与侧栏左下角独立入口**设置**（两级导航：左栏 7 大类 常规/外观/工作台/功能/扩展/账号/数据与关于，当前大类的子项在其下缩进展开——如「扩展」下含 扩展 / Skills；顶部另有**设置项搜索**；右侧**只挂载当前大类**的分区）。主要子系统：**AI 对话面板**（OpenAI 兼容流式，Ctrl+Shift+K）、**剪贴板历史**（Ctrl+`）、**扩展系统**（module/view/window/drawer 四形态 + service 托管 + 市场）、**应用自动更新**、**倒计时浮窗**、**天气/一言**。Rust 后端管理 SQLite 数据持久化，前端使用 Vite 8 + TypeScript 6。

## 结构

```
m-hub/
├── src/                        # 前端源码 (Vue 3 SPA)
│   ├── main.ts                 # 入口：引入 style.css，createApp(App).mount('#app')
│   ├── App.vue                 # 窗口壳：按 label 路由（main 主窗 / sticky-* 便签浮窗 / countdown-* 倒计时浮窗 / clipboard 剪贴板浮层 / ext-* 扩展浮窗 / prompt-float 提示词浮窗 / todo-float 待办浮窗 / chat AI 对话独立窗 / floating-ball 悬浮球 / notice 通知）
│   ├── index/index.vue         # 首页：侧栏导航(工作台/待办/速记/速达) + 扩展中心/扩展视图 + 左下角设置入口 + 视图协调 + 三轴主题 + 启动欢迎页
│   ├── style.css               # 设计令牌（亮/暗色 CSS 变量）+ Tailwind + 通用组件样式
│   ├── api/tauri.ts            # 所有 Tauri invoke 调用封装（219 个命令）+ 模型/配置类型
│   ├── stores/workbench.ts     # 响应式状态管理（reactive + readonly，无 Pinia；工作台/便签/待办/倒计时/提示词/AI 对话/扩展/更新）
│   ├── composables/            # useResourceIcon（资源图标）/ useFocusTrap（焦点陷阱）/ useTheme + themeTokens（三轴主题，后者广播给扩展 iframe）/ useDashboardLayout（工作台网格布局 + 形态注册表）/ useDashPreviewData（布局编辑器预览的共享派生数据，口径逐张照抄真卡）/ useExtensionFrame（扩展 webview 桥接）/ useShortcutRecorder（快捷键录制）
│   ├── utils/                  # categories（文件分类）/ time / web / errorText（错误码→人话）/ subcategoryPath（小类路径规则）/ notesParse（更新说明结构化）/ error-report / chime（提示音）/ weather（Open-Meteo 天气码映射）/ quotes（本地名言兜底语料）/ todoParse（序号列表拆多条待办）/ lunar（农历转换）/ sudaCustom（工作台自定义速达槽位的内容口径，真卡与编辑器缩印共用）
│   └── components/
│       ├── TitleBar.vue        # 透明自制标题栏（startDragging 拖动 + 窗口控制 + AI 对话/搜索入口）
│       ├── ClockCard.vue       # 时钟卡片（三形态：大时钟 HH:mm 日期天气语录 / 今日阴阳历 农历+干支生肖 / 极简时间；cq 响应式 + preview 模式）
│       ├── WeatherCard.vue     # 天气卡片（独立模块，两形态：简版 图标温度城市 / 详情版 体感湿度风状况；数据来自 store.weather）
│       ├── SysMonitorCard.vue  # 系统资源监视器（CPU/内存，2s 轮询，sysinfo 后端）
│       ├── StickyCard.vue      # 便签卡片 ×2（布局部件 slot 1/2，统一玻璃卡，600ms 防抖自动保存）
│       ├── CountdownCard.vue   # 倒计时卡片（时长/定时/每天/间隔 新建 + 列表 + 暂停/浮窗/删除）
│       ├── CountdownFloat.vue  # 倒计时圆形浮窗（水位水波动画，透明置顶小窗，countdown-{id} label）
│       ├── DetachedStickyWindow.vue # 便签脱离浮窗小窗（sticky-{id} label 专属渲染）
│       ├── NotesOverviewCard.vue / TodoOverviewCard.vue / ResourcesOverviewCard.vue  # 速记/待办/速达概览布局部件
│       ├── PromptBoxCard.vue   # 提示词百宝箱卡片（点击复制 + 置顶标 + 复制计数）
│       ├── PromptManageDialog.vue  # 提示词管理弹窗（新增/编辑/删除/置顶）
│       ├── PromptFloat.vue     # 提示词整列表浮窗（prompt-float label 专属渲染）
│       ├── TodoCard.vue        # 待办清单（分段视图 + 优先级圆点 + 行内编辑 + 删除撤销；工作台卡片，常驻）
│       ├── TodoCalendarCard.vue # 工作台「日历」模块：月历上标出待办分布（含周期待办虚拟实例），只读、点整卡进待办视图
│       ├── TodoView.vue        # 待办独立视图（宽窗左列表 40% + 右日历 60%，窄窗单栏切换；范围×形态双维工具栏 + 标签筛选 + 月/周日历 + 行渲染复用 TodoRow，异步分包）
│       ├── TodoEditDialog.vue  # 待办编辑弹层（标题/描述/标签/置顶/周期规则/截止与提醒，一次保存）
│       ├── TodoDateTimeField.vue # 待办「截止/提醒/周期结束」时刻字段（reka-ui DatePicker + TimeField + 快捷时间，禁原生 datetime-local）
│       ├── TodoRow.vue         # 待办行（卡片/浮窗/视图共用：优先级圆点、标签、周期徽标、子待办、行内编辑、拖拽排序、删除撤销）
│       ├── ConfirmDialog.vue   # 通用确认弹窗（勾父带子 / 删标签 / 删除等二次确认；宿主层单实例）
│       ├── TodoFloat.vue       # 待办整列表浮窗（todo-float label 专属渲染）
│       ├── RecentBar.vue       # 最近使用通栏（按 last_launched_at 排序，前 10）
│       ├── Suda.vue            # 速达资源管理（全部/常用/应用/网页/文件 + 大类小类筛选（ADR 0012，含层级展开与横向滚动箭头）+ 拖拽导入 + 扫描安装应用 + 指定浏览器打开 + 网页应用内打开入口）
│       ├── SudaFormDialog.vue  # 新增/编辑资源弹窗（app/web/file + 小类选择（各大类一套，层级缩进）+ 文件选择，网络地址不取图标）
│       ├── SudaWebPanel.vue    # 速达网页主窗内嵌面板（ADR 0011：自绘工具栏 + 内容区空白位，子 webview 由 Rust 预创建，见 suda_browser.rs）
│       ├── BrowserChrome.vue   # 独立应用内浏览器顶栏（suda-web-{i}-chrome label 专属渲染：轻量 tab 条 + 地址栏 + 系统浏览器出口）
│       ├── SudaScanDialog.vue  # 扫描已安装应用批量导入弹窗
│       ├── SecretNoteDialog.vue # 速达加密备注弹窗（v0.8.0 发布说明 ⑧：正文密文 + 名称明文，打开即解密，清空=永久删除需确认）
│       ├── SudaCustomCard.vue  # 工作台「自定义速达」卡片（suda1..4 槽位：可点击启动网格 + 右上角配置入口）
│       ├── SudaCustomEditDialog.vue # 自定义速达内容配置弹窗（手动挑选/大类/小类，存 AppConfig.suda_custom_modules）
│       ├── AppSelect.vue       # 通用下拉选择器（无头封装，样式自绘）
│       ├── NoteList.vue        # 笔记条目列表（标题/相对时间/摘要 + 标签筛选）
│       ├── NoteEditor.vue      # 笔记编辑器（Milkdown Crepe 所见即所得 + 图片粘贴/拖拽/点击上传落盘 + 标签行 + 600ms 防抖自动保存，异步分包）
│       ├── GlobalSearch.vue    # Ctrl+K 全局搜索弹窗（资源/笔记/待办 + 300ms 防抖）
│       ├── ChatPanel.vue       # AI 对话面板（OpenAI 兼容 SSE 流式 + 多会话 + 四方位停靠/拖拽调尺寸 + Markdown 渲染；mode="window" 供独立窗复用）
│       ├── ChatWindow.vue      # AI 对话独立窗口（chat label 专属渲染：无边框 + 自制标题栏 拖动/置顶/关闭=隐藏，内嵌 ChatPanel mode="window"）
│       ├── AiProviders.vue     # 设置「AI 助手」区：供应商/模型管理（连通性测试 + 拉取模型批量添加 + API Key 钥匙串/界面脱敏）
│       ├── ClipboardOverlay.vue # 剪贴板历史浮层（clipboard label 专属渲染：文本/图片/文件 + 粘贴回填 + 置顶）
│       ├── ExtensionCenter.vue # 扩展中心视图（三标签页：已安装清单 / 市场安装 / 「我的扩展」本机源码目录直挂，登记即加载、可发布；权限管理/固定到侧栏；每行可「打开所在目录」跳开发调试）
│       ├── ExtensionView.vue   # 扩展 view 形态（主区内嵌扩展页面）
│       ├── ExtensionWindow.vue # 扩展 window 形态浮窗（ext-{id} label 专属渲染）
│       ├── ExtensionSettingsDialog.vue # 扩展设置/权限详情弹窗（信息区列出目录路径 + 「打开目录」）
│       ├── MarketDetailDialog.vue  # 扩展市场详情/安装弹窗（含截图展示区：主图 + 缩略图切换 + 点击放大灯箱，无截图时给空态交代）
│       ├── UpdateCheckDialog.vue   # 应用更新全局弹窗（新版本信息/下载进度/跳过此版本/立即重启）
│       ├── DashboardLayoutEditor.vue # 工作台布局编辑器视图（所见即所得：clock/weather 真实卡片 + 其余模块真实结构等比缩印、形态切换、最小尺寸钳制 + 适配徽标，保存 dashboard_layout）
│       ├── DashModulePreview.vue # 布局编辑器模块预览：逐张复刻真实卡片 DOM 与设计令牌，尺寸按 --dp-k 等比缩印，数据取 store 真值（同一组件供画布与形态浮层缩略图共用）
│       ├── AboutSection.vue    # 设置「关于」区（版本/开源声明/版本历史/检查更新）
│       ├── SettingsView.vue    # 设置页**外壳**：两级导航（左栏 7 大类 + 当前大类的子项缩进展开）+ 设置项搜索 + 面板路由（按需加载，见约定 50）
│       ├── SettingsSkeleton.vue # 设置页外壳加载骨架（异步分包期间的同形占位；配置在 index.vue 的 defineAsyncComponent）
│       ├── settingsIndex.generated.ts # 设置项搜索索引（**自动生成，勿手改**：scripts/gen-settings-index.mjs 扫外壳 + settings/*.vue 提取，见约定 49）
│       ├── settings/           # 设置页 7 个大类面板（各含自己的脚本 + 模板，按需 import）
│       │   ├── GeneralPanel.vue / AppearancePanel.vue / WorkbenchPanel.vue / FeaturesPanel.vue / ExtensionsPanel.vue（含「扩展」「Skills」两个分区，Skills 分区抽成同目录 SkillsSection.vue；本地扩展目录的增删在扩展中心「我的扩展」标签页，见约定 55）/ AccountPanel.vue / DataPanel.vue
│       │   ├── PanelLoading.vue # 面板加载占位（切大类时避免右侧空白）
│       │   └── shared.css      # 设置页样式（外移自 scoped，规则统一带 .settings-view 前缀，见约定 50）
│       └── ContextMenu.vue     # 通用右键菜单
├── src-tauri/                  # Tauri 后端 (Rust)
│   ├── src/
│   │   ├── main.rs             # Windows 子系统入口 → app_lib::run()
│   │   ├── lib.rs              # Tauri Builder：数据库/托盘/快捷键/窗口状态/单实例/数据迁移与恢复/219 命令注册/mhub-note 笔记图片协议/退出停 service
│   │   ├── commands.rs         # Tauri 命令处理函数（资源/笔记/待办/便签/提示词/倒计时/对话/剪贴板/配置/窗口/备份等）
│   │   ├── models.rs           # Resource/Note/Todo/Sticky/DetachedSticky/Snippet/ClipboardItem/Tag/Countdown/Chat* 结构体
│   │   ├── db.rs               # rusqlite 数据库初始化与迁移（init_in_memory 仅测试用）
│   │   ├── config.rs           # 数据根下 app.json 读写（AppConfig 全字段 serde default，字段清单见「注意事项·配置位置」）
│   │   ├── paths.rs            # 数据根目录解析（标准版 %APPDATA%\m-hub / 便携版 exe\data / 设置自定义迁移）
│   │   ├── process.rs          # 外部进程启动/URL 打开/本地路径打开（app/web/file）+ UAC 提权
│   │   ├── browsers.rs         # 已安装浏览器枚举（注册表 StartMenuInternet）+ open_url_with_browser 指定浏览器打开
│   │   ├── shortcut.rs         # 全局快捷键注册（主窗 Ctrl+Shift+Space + 剪贴板 Ctrl+`，均可自定义）
│   │   ├── tray.rs             # 系统托盘（显示/隐藏/退出菜单）
│   │   ├── sysmon.rs           # 系统资源监视（CPU/内存，sysinfo crate）
│   │   ├── chat.rs             # OpenAI 兼容 SSE 流式对话客户端 + API Key 系统钥匙串（keyring）存取
│   │   ├── chat_window.rs      # AI 对话独立窗口（label chat：无条件启动预创建隐藏常驻 + show/hide 快操作 + 几何防抖落盘 + 置顶持久化，运行期绝不建窗，见约定 33/41）
│   │   ├── clipboard.rs        # 剪贴板监听与历史（文本/图片落盘/文件）+ 粘贴注入（clipboard_paste_method）
│   │   ├── online.rs           # 联网服务：连通性探活/天气（Open-Meteo）/城市地理编码/IP 定位/名言（hitokoto）
│   │   ├── extension.rs        # 扩展扫描/安装/卸载/权限 + extensions_stamp 热更新检测
│   │   ├── market.rs           # 扩展市场（registry.json Ed25519 验签 + GitHub zip 安装/更新/卸载）
│   │   ├── mhub_api.rs         # 扩展桥 API：CAPABILITIES 静态注册表 + mhub_call 分发（见关键约定 32）
│   │   ├── service.rs          # service 扩展托管（Node 后端进程启动/动态端口/探活/停止）
│   │   ├── runtime.rs          # service 运行时解析（系统 Node 优先 → 内置运行时按需下载，自动降级）
│   │   ├── proxy.rs            # /svc/<extId>/* 本地反向代理（扩展前端 → service 后端）
│   │   ├── updater.rs          # 应用自动更新（update.json 验签 + sha256 下载校验 + 重启两步 rename 自替换/回滚）
│   │   ├── signing.rs          # Ed25519 分离签名验签（市场清单 + 更新清单共用，内嵌公钥）
│   │   ├── skills.rs           # 扩展开发技能包（Skills）：内置 m-hub-extension 一键装到本机 AI 助手 skills 目录
│   │   ├── autostart.rs        # 开机自启动（HKCU Run 键 + --autostart-hidden 静默驻留托盘；清理旧计划任务残留）
│   │   ├── countdown_ticker.rs # 倒计时后台驱动线程（1s 轮询到期项→通知+事件+顺延）
│   │   ├── countdown_window.rs # 倒计时圆形浮窗（创建/销毁/位置持久化，countdown-{id}）
│   │   ├── sticky_window.rs    # 便签脱离浮窗（创建/销毁，sticky-{id}）
│   │   ├── float_window.rs     # 通用整列表浮窗（prompt-float 提示词 / todo-float 待办）
│   │   ├── floating_ball.rs    # 桌面悬浮球（预创建透明置顶小窗 + 环形菜单几何 + 贴边半隐/悬停滑出的边缘监视循环，见 ADR 0004）
│   │   ├── notify.rs           # 右下角自绘通知窗（独立 WebView「notice」，跨 Win10/11 一致；替代 tauri-plugin-notification，前端 NoticeOverlay.vue 渲染卡片）
│   │   ├── suda_browser.rs     # 速达「应用内打开网页」（ADR 0011）：主窗内嵌面板 child webview + 独立浏览器窗口池×1（2026-09-25 内存优化 4→2→1，全部页面收进 tab；chrome 走轻量入口 chrome.html，chrome/content 双子 webview），启动期预创建、运行期零 build/destroy；tauri `unstable` 特性
│   │   ├── notes_port.rs      # 速记导出/导入 zip（v0.8.0 发布说明 ⑤：manifest 为数据源 + 可读 .md 副本 + 图片相对路径，见约定 83）
│   │   ├── secret.rs          # 速达加密备注的 AEAD 加解密 + 主密钥存取（v0.8.0 发布说明 ⑧，密钥只进系统钥匙串，见约定 85）
│   │   └── repo/               # 数据访问层：resource, note, todo, sticky, detached_sticky, snippet, tag, countdown, chat, clipboard, subcategory
│   ├── capabilities/default.json  # Tauri 权限声明（含 start-dragging/global-shortcut/dialog/notification）
│   └── tauri.conf.json         # 窗口配置（无边框、1400x900）
├── （无 docs/ 目录）          # ⚠️ 曾经的 docs/*.md 全部**从未提交过**（2026-10-06 开源时逐个核对发现：
│                              #   design-spec / reka-ui / adr/0001·0005·0006·0008 / file-search-plan /
│                              #   agent-foundation-plan / todo-upgrade-plan / prototypes/…）。
│                              #   这些文档的决策内容**全部已在**本文件与 DESIGN.md / CONTEXT.md 里，
│                              #   所以**不补写** —— 补一份就是第二份真相，正是本项目一贯避免的。
│                              #   下方各条约定里的 docs 引用已全部改为就地引用。
│                              #   `scripts/check-doc-links.mjs`（prebuild 跑）守住「不许再写死链」。
│                              #   扩展系统的 spec/api/evolution 文档在 m-hub-extensions 仓库。
├── DESIGN.md                   # 当前设计系统（唯一实现基线，与 style.css 对齐；§8 为 Reka UI 组件规范）
├── PRODUCT.md                  # 产品定义（用户/目标/品牌个性/设计原则/无障碍基线）
├── CONTEXT.md                  # 领域术语表（易混淆概念精确区分；「引导」节为规划中未实施）
└── package.json
```

## 入口点

| 关注点 | 文件 |
|--------|------|
| 前端启动 | `src/main.ts` → `src/App.vue`（窗口壳，按 label 路由） → `src/index/index.vue`（首页） |
| 状态管理 | `src/stores/workbench.ts` → `useStore()` |
| 后端启动 | `src-tauri/src/main.rs` → `src-tauri/src/lib.rs` → `run()` |
| 路由 | 无 Vue Router，侧栏切换 activeView 渲染对应视图/面板 |

## 设计规范

**实现基线见 `DESIGN.md`。**（曾有一份 `docs/design-spec.md` 作为原始 v1.0 基线，**从未提交过**；其内容早已被 `DESIGN.md` 取代并按实现同步加注，因此**不补写**——见结构树里那条说明。）速览：

- **设计令牌**：全部定义在 `src/style.css`（CSS 变量，亮色 `:root` + 暗色 `[data-theme="dark"]` 覆盖），组件一律引用变量，禁止硬编码色值
- **主色（三轴主题 v0.1.15）**：品牌强调色由 `--accent` 内联 CSS 变量注入（默认亮 `#5B5BF5` / 暗 `#8b8bff`），`--brand-500/600/50/glow` 全部经 `color-mix` 派生自 `--accent`；主题 = 模式（亮/暗/系统）× 预设（10 单色 + 10 渐变）× 强调色（8 预设 + 自定义 hex）三轴独立配置（`useTheme` + 设置「外观」区）
- **玻璃卡片**：常驻表面用 `--frost-surface`（静态烘焙渐变伪毛玻璃）+ `--frost-edge` 顶部高光 + `--shadow-card` + `--radius-lg`(12px)，内部控件 8px；真 `backdrop-filter` 仅用于弹窗/菜单/下拉等瞬态表面（性能策略见 DESIGN.md §7），唯一例外是 opt-in 沉浸模式的静态 `.card`（见 `AGENTS.md 约定 69`）；铬件（侧栏/标题栏）任何壁纸形态下都保持全透明，与背景构成同一连续平面，勿给铬件垫材质
- **强调色**：`--c-yellow/red/blue/green/pink/orange/purple/gray` 8 色 + ink/soft 变体，资源图标按名称 hash 取色（`useResourceIcon`）
- **字体层级**：Section title 16/650、Body 13、Caption 12、Micro 11（见 DESIGN.md §3）
- **布局**：`app-body` 两栏 Grid（220px 侧栏 / 56px 收起态）；工作台为自由编排 Bento 网格（`useDashboardLayout` + 布局编辑器）；速记/速达/扩展中心为独立视图
- **交互动效**：hover 轻微上浮 + shadow、按钮按下 scale(0.96)、弹窗 0.2s 缩放渐入
- **弹窗遮罩**：统一 `--scrim` 令牌（暗色下保证对比度），`useFocusTrap` 焦点陷阱

## 前后端通信

- **唯一通道：** `@tauri-apps/api/core` → `invoke<ReturnType>('command_name', args)`
- **类型安全：** 所有 invoke 调用封装在 `src/api/tauri.ts` 的 `tauriApi` 对象中，含完整 TypeScript 类型
- **环境守卫：** `isTauri()` 检查 `'__TAURI_INTERNALS__' in window`，确保浏览器预览环境不崩溃
- **命令注册：** `src-tauri/src/lib.rs` 的 `invoke_handler!` 宏列出全部 219 个命令（前端封装一一对应 `src/api/tauri.ts`）

## 数据模型（SQLite）

| 表 | 说明 |
|----|------|
| `resources` | 速达资源（app/web/file，category=所属大类的小类名（NULL=未归类）/icon/args/sort_order/last_launched_at） |
| `resource_subcategories` | 速达小类（ADR 0012：kind/**name（全路径，层级见约定 76）**/sort_order/is_default，UNIQUE(kind,name) 各大类一套；文件大类 7 内置值经建表种子并入） |
| `notes` | 速记笔记（title/content） |
| `tags` / `note_tags` | 笔记标签（多对多） |
| `todos` | 待办（done/priority/completed_at/due_at/remind_at/parent_id 子待办/sort_order 手动拖拽排序位/description 轻量 Markdown/pinned 置顶/repeat_* 周期规则 + repeat_done_count 累计次数） |
| `todo_tags` / `todo_tag_links` | 待办标签（**与笔记标签是两套独立定义**，见 ADR 0010） |
| `stickies` / `detached_stickies` | 便签（slot 1/2）与脱离浮窗 |
| `snippets` | 提示词（is_pinned/copy_count/last_copied_at） |
| `countdowns` | 倒计时（repeat_mode once/daily/interval + end_at/total_ms/interval_minutes/paused/finished/floated/float_x/float_y） |
| `chat_sessions` / `chat_messages` | AI 对话会话与消息（session_id 索引，模型/角色/内容/用量） |
| `clipboard_history` | 剪贴板历史（kind text/image/file + image_path/file_paths/source_app/is_pinned） |

> 旧版 `groups`/`files` 表已并入 `resources`（Speed-to-launch 合一）；索引含 `idx_notes_updated`、`idx_todos_created`、`idx_resources_category`、`idx_countdowns_end` 等。

## 关键约定

1. **无 Pinia：** 使用 `reactive()` + `readonly()` 自定义 store 模式
2. **无 Vue Router：** 侧栏 `navigation` 数组 + `activeView` 切换，工作台为组合式面板网格
3. **App.vue 窗口壳：** 按窗口 label 路由（`main` 主窗渲染 `src/index/index.vue`、`sticky-{id}` 便签浮窗、`countdown-{id}` 倒计时浮窗、`clipboard` 剪贴板浮层、`ext-{id}` 扩展浮窗、`prompt-float` 提示词浮窗、`todo-float` 待办浮窗），仅做路由分发，零业务逻辑；所有首页逻辑在 `src/index/index.vue`
4. **无 NaiveUI：** 全部 UI 自绘，样式基于 `style.css` 设计令牌（Bento 玻璃风格 + 暗色 `[data-theme="dark"]`）
5. **图标用 lucide-vue-next：** 组件内 `import { Xxx } from 'lucide-vue-next'`，按需 `:size`/`:stroke-width`（1.8~2.2）微调，颜色继承 currentColor；仅 TitleBar 品牌 Logo 保留手写 SVG
6. **窗口拖动：** TitleBar 用 `getCurrentWindow().startDragging()` + mousedown 监听（非 `data-tauri-drag-region` 属性）
7. **窗口事件拦截：** 关闭按钮隐藏至托盘而非退出（`lib.rs` on_window_event + `api.prevent_close()`）
8. **窗口状态持久化：** 尺寸/位置/置顶在关闭时由 Rust 端保存到 JSON，启动时恢复；最大化图标切换用 `isMaximized()` + `onResized` 监听；**最大化态不写回**（persist 跳过，否则下次以「非最大化 + 超大窗口」恢复反而撑出屏幕）；**小屏适配（v0.7.2）**：启动恢复后若所在显示器（逻辑像素 = 物理像素 ÷ scale_factor）容不下 1400×900 → 直接 `maximize()`，否则窗口下半部分掉到屏幕外无从挽救
9. **笔记/便签自动保存：** 600ms 防抖（NoteEditor.vue / StickyCard.vue）
10. **搜索防抖：** 300ms（GlobalSearch.vue）
11. **全局快捷键：** **四个**全局快捷键都在 `shortcut.rs` 统一注册、统一分发（v0.7.2 起搜索/AI 对话升级为全局）——主窗显隐默认 Ctrl+Shift+Space、剪贴板浮层默认 Ctrl+`（避开 Ctrl+Shift+V 无格式粘贴）、全局搜索默认 Ctrl+K、AI 对话默认 Ctrl+Shift+K；均可在设置 → 快捷键录制（4 个 recorder 实例共用 `useShortcutRecorder`），失焦/回车自动保存，录制中失焦取消并还原。**分发与改绑各只有一份实现**：`setup()` 的 with_handler 按 config 比对 emit `clipboard-toggle`/`search-shortcut`/`chat-shortcut`/`global-shortcut-toggle`（前两个 Rust 侧 app.listen 处理显隐/浮层，后两个主窗前端监听开弹窗/分流对话形态）；`rebind_shortcut` 承载全部 set_*_shortcut 命令的冲突预检/反注册/注册/回滚。**桌面端绝不给搜索/对话再挂应用内 window keydown 监听**——全局热键与 keydown 双触发会让两次 toggle 相互抵消，表现恰是「按了没反应」（keydown 兜底只存在于非 Tauri 预览）；改绑用 `ConfiguredShortcut` 枚举走 `set_configured_shortcut`，别再复制粘贴第四份
12. **轻提示：** index.vue `provide('showToast')`，子组件 `inject` 使用
13. **只读 props：** store.state 为 readonly 深度代理，组件 props 用 `readonly Note[]` 等类型
14. **拖拽导入：** 拖入 exe/lnk/文件夹到窗口 → `onDragDropEvent`（Suda.vue）→ `parse_dropped_path` 命令（.lnk 经 PowerShell COM 解析目标 + System.Drawing 提取图标存 `app_data_dir/icons/`）→ 自动预填资源弹窗；图标经 `convertFileSrc`（assetProtocol 已启用，作用域为**白名单子目录**：icons / wallpapers / clipboard-images，见约定 44）渲染，提取失败回退名称 hash 首字母。注意：该功能依赖 Tauri 原生拖放拦截（`dragDropEnabled` 默认开），它与 WebView2 内 HTML5 拖拽互斥、无运行时开关（tauri 2.11 仅有创建时的 `disable_drag_drop_handler()`）——笔记编辑器块拖拽已改为指针实现绕开（见约定 38），速达原生拖入保持不受影响
15. **PowerShell 调用约定：** 一律用**环境变量传参**（`Command::env`）而非 `$args`——实测 `-Command` 模式下 `$args` 不可靠；输出前设 `[Console]::OutputEncoding=UTF8` 防中文乱码
16. **文件选择：** 已集成 tauri-plugin-dialog（`dialog:allow-open` 权限）；SudaFormDialog 路径/图标输入框右侧有选择按钮，选 exe/lnk 自动解析名称与图标，选图标文件经 `import_icon_file` 存入 icons 目录
17. **AI 用量：** 已拆分为 service 扩展 `com.m-hub.token-stats`（实时读 opencode 数据库聚合，宿主零 token 代码）；详见 `m-hub-extensions/extensions/com.m-hub.token-stats`。宿主侧旧用量代码（`usage.rs`/TokenStatsCard/用量视图）已全部移除，侧栏无「用量」入口
18. **系统监视：** `sysmon.rs` 用 sysinfo crate 返回 CPU/内存，2s 轮询（SysMonitorCard.vue）
19. **GPU 性能约束（v0.1.13）：** 常驻卡片默认禁用 `backdrop-filter`，一律用 `--frost-surface` 静态烘焙渐变模拟毛玻璃；`backdrop-filter` 只允许出现在瞬态层（弹窗/菜单/下拉/tooltip）+ 沉浸模式的静态 `.card`（opt-in 受控例外，见 `AGENTS.md 约定 69`）；周期性更新的进度条用 `transform: scaleX` 而非 `width`，避免触发布局重排
20. **倒计时驱动（v0.1.13）：** 到期判定、通知、顺延全部在 Rust `countdown_ticker.rs` 后台线程（1s 轮询），**不能依赖前端 setInterval**（WebView 隐藏/最小化会节流）；前端只做展示与用户操作。到点发自绘右下角通知（`notify.rs::show_notice`，跨 Win10/11 一致；前端监听未就绪时通知暂存容量 8、`notice_ready` 命令注册完成后重放，防启动首秒提醒静默丢失）+ emit `countdown-fired` / `countdowns-changed` 事件；完全退出/休眠期间错过的提醒（超 5s）静默顺延不补发。`once` 到点置 finished 灰态，`daily` 按 24h 顺延，`interval` 按 `interval_minutes` 顺延。**计时门控：倒计时计时 ⇔ 工作台有倒计时卡片 ∨ 该倒计时已浮窗**——前端按「已提交」的 dashboard_layout（编辑器草稿不算）在主窗口调 `set_countdown_card_visible` 上报（`useDashboardLayout.ts` 的 syncCommitted），卡片不在场时后端冻结全部非浮窗倒计时（`repo/countdown.rs::auto_pause_*`，`auto_paused` 列标记自动冻结、与手动暂停区分），卡片恢复显示或浮窗浮起/收起时按暂停语义恢复/冻结对应倒计时（`resume_if_auto_paused`/`auto_pause_single`）；`CardVisible(AtomicBool)` 默认 false，防前端未就绪时倒计时抢跑到点
21. **倒计时浮窗（v0.1.13）：** 每个倒计时可浮起为独立透明圆窗（label `countdown-{id}`，300×340 固定、无边框、置顶、skip_taskbar），圆形水位随剩余比例下降 + 双层正弦波滚动动画；浮起状态与位置持久化在 `countdowns` 表，重启恢复；`once` 到点自动收窗。App.vue 按 label 前缀路由到 `CountdownFloat.vue`
22. **倒计时提示音：** 默认关闭（`countdown_sound` 配置，设置视图开关）；开启后前端 WebAudio 合成双音（`utils/chime.ts`，无外部音频文件），仅主窗口播放避免多窗重音
23. **reka-ui（^2.10.3）组件：** 仅用于复杂输入（DatePicker 定时日期 / TimeField 时:分 / NumberField 步进），无头组件样式全部自绘；v-model 绑定 `Time`/`DateValue` 一律用 `shallowRef`（含 `#private` 字段，ref 深度解包破坏类型匹配）——详见 `AGENTS.md 约定 23/24/25`
24. **reka-ui Portal 弹层（铁律）：** `DatePickerContent` 等经 Portal 渲染到 `<body>` 后父组件 scoped `data-v` 不传播到容器，容器样式（`z-index`/背景/边框/阴影）全部失效 → 日历被 `modal-mask`(100) 盖住选不到；容器样式必须用 `:global()`，`z-index` 设 110（CountdownCard.vue `.cc-calendar-content` 即此例）
25. **reka-ui segment 组件（铁律）：** `TimeField`/`DatePickerField` 外层禁止 `<label>` 包裹（segment 是 contenteditable div、非 labelable，label 会激活组件内部隐藏 input → `onFocus` 强制聚焦第一个 segment，表现为点「分」跳「时」）；外层用 `<div class="cc-field">`；`NumberField` 的原生 input 不受影响可继续用 label
26. **主题三轴系统（v0.1.15）：** 主题 = 模式（light/dark/system，`data-theme`）× 预设（10 单色 `data-preset` + 10 渐变，渐变仅覆盖 `--app-bg` 背景）× 强调色（8 预设 + 自定义 hex，inline `--accent`）。`style.css` 中 `--brand-500` = `var(--accent)`，`--brand-600/50/glow` 均 `color-mix` 派生；实现/读取都在 `composables/useTheme.ts`，配置字段 `theme_mode`/`theme_preset`/`accent_color`（旧 `theme` 字段经 serde alias 自动迁移）
27. **工作台自由网格布局 + 形态注册表（v0.3.0 引入网格，v0.5.x 形态化）：** 工作台卡片由配置 `dashboard_layout`（JSON 网格坐标，每项含 `variant` 形态字段）驱动，模块目录 + 形态注册表在 `useDashboardLayout.ts`（`DASH_MODULES`，每模块声明多个形态，每形态带 `min` 最小完整尺寸 / `ideal` 推荐尺寸 / 名称）：clock（big/lunar/minimal 三形态）、weather（now/detail）、sysmon / sticky1 / sticky2 / notes / todo_overview / resources / countdown / prompts / todo / calendar / recent 单形态，外加 suda1..suda4「自定义速达」固定槽位（单形态 grid）。**「自定义速达」槽位（suda1..suda4，2026-09-25，同便签 1/2 池子模式）**：内容配置存 `AppConfig.suda_custom_modules`（source = pinned（resource_ids 勾选顺序即展示序，已删资源自动跳过）/ app / web / file / subcategory；前端改 `state.config` 后 saveConfig 整体落盘，无专属命令；**从布局删卡不删配置**，重加同槽位内容还在），内容过滤/排序唯一实现在 `utils/sudaCustom.ts`（真卡 `SudaCustomCard` 与编辑器缩印共用，预览口径铁律），点击条目走 `store.launchResource`（网页打开方式分流 / 最近使用 / last_launched_at 全生效）。**仅真渲染的模块暴露多形态**，其余单形态保持现状。可增删、拖拽、调宽高并持久化；编辑入口为设置 →「布局编辑器」（`DashboardLayoutEditor.vue`，完成后回工作台）。**所见即所得编辑器**：clock/weather 画布内挂载真实组件（`preview` prop 禁交互），其余模块（含扩展 module）与形态浮层缩略图统一走 `components/DashModulePreview.vue`——**逐张复刻真实卡片的 DOM 结构与设计令牌**（卡头 13px/600 + 14px 品牌色 lucide 图标 + 右侧 26px 钮位、概览 `--bg-card-soft` 统计块、待办分组头/圆点/日期徽标、最近使用 42px 图标格……数据全部来自 `useDashPreviewData` 的模块级共享 computed——所有格子只做一次过滤/排序，编辑器在场时按分钟推进时间/农历/倒计时剩余/相对时间，离开即停定时器；无数据即真实卡片空态），尺寸一律写成 `calc(真实px * var(--u))`，`--u = 1px * --dp-k`，`--dp-k` 由编辑器按「画布每列像素 ÷ 真实工作台每列像素」实测注入（ResizeObserver + 窗口宽推算，clamp 0.5~1）；**铁律：预览不许退回纯 cqh 小字号**（旧实现正文只给 2.6cqh ≈ 3px，与真卡 13px 差 3~4 倍，就是「预览不真实、又小又空」的根因），也不许 1:1 原 px（画布格子只有真实的 65%~85%，必然溢出裁字）；形态浮层的缩略框按该形态真实宽高比成形（`aspect-ratio: idealW/idealH`，系数 `thumbK = 62px ÷ 真实卡片宽`），列表只设防超大 DOM 的**软上限**（待办每组 8 / 提示词 12 / 倒计时 8 / 最近使用 20），可见行数由容器 `overflow: hidden` 按格子高度裁切——裁切本身即「这块不够大」的诚实信号；**派生数据的过滤/排序口径必须与对应真卡逐条一致**（待办分组计数取全量而非截断数、倒计时按 `end_at` 升序、速记最近一条按 `updated_at` 降序、最近使用按 `new Date(last_launched_at)` 倒序、今日新增沿用真卡的 `toDateString()` 判定、相对时间与摘要沿用 `NotesOverviewCard` 的 `fmtTime`/`summary`），预览数字或顺序与真卡不符会被用户当成新 bug。画布列间距与卡片圆角与真实工作台取齐（gap 16px / `--radius-lg`），适配状态的彩色描边只在 hover 或开「填充审计」时出现（红=低于最小常显），避免四色边框糊满画布失真；缩放钳制到形态最小尺寸，卡片带适配徽标（绿=正好铺满/黄=紧凑/蓝=弹性空间/红=低于最小自动钳制），⇄ 按钮切换形态（格小于新形态最小自动补足并就近让位）。**容器查询弹性**：ClockCard/WeatherCard 内容用 cq（cqw/cqh）+ clamp 双保险——真实工作台格子（`repeat(N,minmax(0,1fr))` 随窗口缩放）保持现有观感，编辑器缩略按比例缩印（前提：`.dash-cell` 与编辑器 `.le-cell` 均为 `container-type: size`）。老数据无 variant 字段 → 回退 defaultVariant 天然兼容；`dashboard_mid_content` 已废弃不再被 UI 读取（保留在配置结构中向后兼容）。天气模块（WeatherCard）为独立卡片，数据来自 store.state.weather（当前天气，无 7 日预报）；农历用 `utils/lunar.ts` 经典数据表算法（1900–2100，已验证与官方天文历在 1996-10 之后全部一致，更早年份为经典表与天文历的历史差异，今日展示不受影响）
28. **侧栏默认收起（v0.1.15）：** `sidebarCollapsed` 默认 `true`（56px 图标态，hover 出名称气泡）；展开/收起按钮仅在设置开启 `sidebar_toggle` 后出现（默认关闭）；720px 以下强制恢复文字导航
29. **关于 / 更新日志（v0.1.16，v0.3.0 改版，v0.7.2 再改）：** 设置「关于」区（`AboutSection.vue`）展示版本号 + 开源声明 + 检查更新；**内置「版本历史」折叠列表已移除**，改为「版本历史 → GitHub Releases」跳转（`tauriApi.openExternal` 打开 `https://github.com/dckxx/m-hub/releases`）。版本号运行时读 `app.package_info().version`（随 `tauri.conf.json` 烘焙），README badge 是文档侧唯一真相。升级提醒已改走**应用自动更新**链路（见约定 35：updater.rs 静默检查 + UpdateCheckDialog 弹窗），旧 `check_whats_new`/`whats_new_enabled`/`last_seen_version` 机制已移除。**v0.7.2 起版本历史不再打包进二进制**：`about.rs`（`include_str! RELEASE_NOTES` + `version_sections()`/`latest_section()`）已删除，`get_app_info` 只返回 `version`（exe 去重几十 KB）。RELEASE_NOTES 仍为累积式单一数据源：每发版在顶部新增一节 `# vX.Y.Z 发布说明`，客户端不再解析它；v0.7.2 起 `publish-release.ps1` 的 `-Notes` 缺省自动取该最新一节的完整文本写进 update.json.notes（客户端弹窗全量展示、可滚动），不再手写摘要
30. **优先复用现成组件：** 需要下拉选择、弹窗、输入等交互控件时，先查 `src/components/` 已有通用组件（如 `AppSelect.vue` 下拉选择器、`ContextMenu.vue` 右键菜单、`useFocusTrap` 焦点陷阱），优先复用而非新写原生控件（如原生 `<select>`）——保证交互与视觉一致、避免样式重复（反例：设置「粘贴方式」曾用原生 `<select>` 加 `min-width` 撑宽，应改用 `AppSelect`）
31. **新增浮窗窗口必须同步多处 label 配置（否则浮窗闪出「欢迎回来」启动页）：** 启动欢迎页 `#boot-splash` 内联在 `index.html`（所有窗口共用），head 内联脚本用**白名单**判定——只要 `window.__TAURI_INTERNALS__.metadata.currentWindow.label !== 'main'` 就 `data-no-splash` 隐藏 splash（切勿改回黑名单逐个罗列，漏加即复现本 bug）。新增浮窗需同步：① `capabilities/default.json` 的 `windows` 数组加 label；② `App.vue` 按 label 路由到浮窗组件；③ Rust 侧窗口 label 常量（如 `float_window.rs`）；④ `lib.rs` 注册对应命令。
32. **扩展桥 API 能力注册表（v0.2.3）：** 桥 API 不再在 `mhub_api.rs` 手工 `match` 分发，而是集中在一张 `CAPABILITIES` 静态表（每项 `namespace`/`method`/`permission`/`handler`）；**新增能力 = 表里加一行 + 写 handler**，`runtime.info` 返回 `capabilities` 清单供扩展探测。manifest 支持 `requires`（依赖宿主能力，写 `namespace.method`）/`dependsOn`（依赖其它扩展 id，驼峰）/`disabled`（条件禁用 `{platform}`）/`expose`（跨扩展调用白名单）/`actions`（快捷动作，扩展中心渲染按钮）；扫描器求值后在扩展中心标「缺能力/缺依赖/已禁用」并拦截打开。扩展配置走 `config.*` 桥 API：`manifest.config` 为作者默认，`.config.json` 为「用户覆盖」层（覆盖优先，升级扩展不冲掉）；`storage.*` 仍是扩展私有键值、与 `config.*` 各自独立；`sharedStorage.*` 为跨扩展共享键值（需 `shared-storage` 权限）。扩展间协作两条路：事件总线 `events.emit/on`（emit 需 `events` 权限，广播走前端 `broadcastExtensionEvent`）+ 跨扩展调用 `runtime.callExtension`（前端 `routeExtensionCall` 路由 + Rust 校验 expose 白名单 + 目标扩展 `mhub.expose(method, fn)` 注册）。运行时热更新：`extensions_stamp` 命令对 manifest 的路径+mtime 做 FNV 哈希，扩展中心 5s 轮询变化即刷新列表（无需重启）。**`runtime.openPermissions`（v0.7.2，无需权限）**：扩展请求打开**自己的**设置/授权弹窗——Rust handler 直接 emit `open-extension-settings`（payload=extId），主窗 index.vue 切到扩展中心经 `jump-settings` prop 让 ExtensionCenter 开设置弹窗（列表未就绪由 watch 补开）；service 扩展后端未授权（PERMISSION_DENIED）时页面用 `mhub.openPermissions()` 给「去授权」直达（token-stats view/module 已接入，见 `skills/m-hub-extension` bridge-api.md）；预检扫描经 `precheck.rs::TOP_LEVEL_METHODS` 把顶层 `mhub.openPermissions(...)` 换算成该能力名对账。**扩展中心行点击 = 打开详情（v0.7.2，已安装与我的扩展两标签页同口径）**：直接打开扩展走行右侧 ▶ 按钮（emit open，按 `extension_open_modes` 分流），守卫逻辑收在 `withRowGuard` 一份；**`runtime.openExternal` 按扩展「链接打开方式」分流（v0.7.2）**：`extension_link_modes[extId]` = `browser` → 系统默认浏览器，其余/未配置 = `inapp` → `suda_browser::suda_browser_open_url` 应用内浏览器池（默认软件内），配置在扩展详情弹窗分段按钮、经 saveConfig 落盘，skill 文档（SKILL.md/bridge-api.md/mhub.d.ts）已同步改为「默认应用内浏览器」
33. **AI 对话（v0.3.0，v0.5.4 加独立窗口形态）：** `chat.rs` 为 OpenAI 兼容 SSE 流式客户端（reqwest），API Key 存系统钥匙串 keyring（**不明文落盘**），界面脱敏（查看/复制）；⚠️ **平台模型的 Key 一律不在界面展示/复制**（v0.6.1 修，2026-09-17 用户反馈）：平台条目（`platform:<模型名>`）钥匙串里存的是占位符 `chat::PLATFORM_KEY_SENTINEL`，真凭据是**账号登录态**、只在请求时由 `chat::resolve_api_key` 现取——`commands::get_chat_api_key` 对占位符直接返回 Err（纯函数 `api_key_for_ui` 有回归测试），平台条目在设置里**根本不渲染成卡片**（没有 Key 可展示，也没有 Key 需要填）；自备供应商模型照旧支持测试连通性 + 拉取模型列表批量勾选添加，同供应商共享 Key。⚠️ **平台免费额度是「一个开关 + 一个对话入口」，不是供应商卡片**（v0.6.1 改，2026-09-17 用户要求）：设置「AI 助手」里它只渲染成一个开关（`AiProviders.vue` 的 `.ap-platform` 开关行），供应商名称 / Base URL / API Key / 测试连通 / 获取模型 / 模型标签这些字段全是自备供应商的事，平台一条都不显示——它没有需要用户填的东西。配置条目仍是每条 `platform:<模型名>`（`chat::is_platform_model` 靠这个前缀识别）：开关开启 = 拉 `/api/v1/ai/models` 整体写入，关闭 = 从配置里整体移除。**自备供应商一切照旧**（卡片、测试连通、获取模型、同供应商共享 Key 均不动）。AI 对话的模型下拉里这批条目**折叠成「m-hub 平台」一项**（`ChatPanel.vue` 的 `modelOptions`；入口名与条目判定的前端单一来源是 `api/tauri.ts` 的 `PLATFORM_ENTRY_NAME`/`isPlatformModel`，`AiProviders.vue` 与 `ChatPanel.vue` 共用，勿再各抄一份），平台侧配了几个模型用户既看不到也不必选：**只有一个就永远用它，多个就在它们之间轮询（负载切换）**——`commands::pick_chat_model` 判定「会话选中的是平台入口名 ∨ 命中某个 `platform:*` 条目（老会话存的具体平台模型名）」，命中即走进程内 `PLATFORM_RR` 游标轮询，不固化成当时那一个；新会话默认名由 `commands::default_session_model_name` 决定（自备默认优先，默认恰好是平台条目时存入口名）；平台条目一个都没有时给「平台额度未开启」的明确指路，而不是含糊回退到别的模型。⚠️ 两个坑：① 平台中转只实现 `POST /v1/chat/completions`，**没有** OpenAI 的 `GET /v1/models`（实测 `/v1/models`、`/v1/models/`、`/models`、`/v1/` 全部 404 `{"error":"not_found"}`，而 `POST /v1/chat/completions` 不带凭据回 401 = 路由存在，故**对话链路本身是好的，404 只说明这个列表端点不存在**）——平台模型的列表与地址只能走 `loadPlatformModels()`（`account_status` 取地址 + `platform_models` 取列表），套用自备供应商那条通用探测必然失败；② 通用探测在 Key 为空时会拿 `key_id` 去钥匙串取 Key，而平台条目钥匙串里只有占位符 → `commands::probe_key` 对占位符直接报错（纯函数有回归测试）兜底。**平台条目不在 providers 列表里（不渲染卡片），但 `collectAll()` 必须原样带回**：保存自备供应商是整体覆盖写 `chat_models`，漏掉平台条目 = 把用户开着的平台额度悄悄关掉。同理**开关只就地更新 `platformModels`（`saved.filter(isPlatformModel)`）、绝不回读整表**——开关与自备供应商列表毫无关系，回读会重建全部卡片（界面闪一下、展开态要重继承，还会把用户正在填的新供应商卡片冲掉）；`saveAll` 之后那次必要的回读走 `loadProviders({ silent: true })`：不进 loading 占位（否则整块列表被「加载中…」顶掉一下再回来，就是用户反馈的「点开关界面闪一下」），并继承展开态/勾选态。**两种形态互斥，由设置「AI 助手 → 以独立窗口打开 AI 对话」（`chat_window_mode`）决定**：默认内嵌抽屉（`ChatPanel.vue`，四方位停靠 `chat_panel_side` + 宽/高/透明度均持久化，标题栏按钮 / Ctrl+Shift+K 唤起）；开启后为独立小窗（`chat_window.rs` + `ChatWindow.vue`，label `chat`，无边框 + 自制标题栏：拖动/置顶 `chat_window_pinned`/关闭=隐藏，可缩放，几何由后端在 Moved/Resized 防抖落盘，首次落点主窗中央略偏右下）。主窗 `toggleChat()` 按开关分流（独立形态调 `chat_window_toggle`，抽屉状态不参与），设置页切换时后端 emit `chat-window-mode` 让主窗收起已开的抽屉；悬浮球「AI 对话」入口同样分流——`floating_ball::trigger` 的 `view:chat` 在独立形态下直接唤起小窗、不弹主窗。独立窗遵守约定 41：**无条件**启动预创建隐藏常驻（v0.5.5 起；v0.5.4 曾按配置惰性建窗、开关切换时运行期现场 build，实测挂死整 app——该「唯一一次运行期建窗」的例外论已被撤销），关闭只 hide 不 destroy，形态开关仅改镜像 + 显隐。独立窗的 ChatPanel 走 `mode="window"`（隐藏拖拽手柄与收起钮、不读写抽屉尺寸配置）；主题实时跟随靠 `useTheme` 按窗口 label 门控广播（**只有主窗广播**，独立窗自己 applyTheme 不再转发，防自激循环）。侧栏「对话」入口仍暂时隐藏（`index.vue` visibleNavigation 过滤，勿直接删导航项）
34. **剪贴板历史（v0.2.x）：** `clipboard.rs` 后台监听，记录文本/图片/文件三类（相同内容去重，图片缩略图落盘 `数据根\clipboard\images`，删除/清空/过期联动删文件）；浮层是独立窗口（label `clipboard`，ClipboardOverlay.vue），全局快捷键默认 Ctrl+`；条数/TTL/媒体开关/粘贴方式（`clipboard_paste_method`）均入配置。「保存图片」导出按目标扩展名经 `clipboard::transcode_image_bytes` 转码：截图类应用只往剪贴板写 CF_DIB 位图、快照落成 .bmp，存 .png 时重编码为 PNG，并处理 32bpp DIB 的 alpha 字节未初始化问题（整图全 0 按不透明处理，否则转出的 PNG 整张透明；PNG 源不动）；快照落盘格式与写回剪贴板格式均不变
35. **应用自动更新（v0.3.0，v0.7.2 加「稍后再提示」）：** `updater.rs` 自研链路——启动 5s 后 + 每 `update_interval_hours`（默认 4h）静默检查内置清单地址（v0.6.1 起 = `config::update_manifest_url()`，平台服务端接口 `/api/v1/app/update`，服务端再代理 COS 的 `releases/update.json`；此前是配置项 `update_endpoint`，已废弃）（`signing.rs` Ed25519 验签通过才信任 + `minimumUpgradable` 跳级保护，标准版/便携版分别取包）→ UpdateCheckDialog 弹窗（可跳过版本，记 `skipped_update_version`；「取消」已改为**「稍后再提示」**：`snooze_update` 写 `update_snooze_until_ms` 暂停 30 分钟、到点 spawn 补检一次——不补检的话 4h 循环会把「稍后」变成「最多 4h 后」；手动检查不受暂停窗口影响）→ 流式下载 sha256 校验 → 重启时「exe → exe.old / 新 exe → exe」两步 rename 自替换，失败回滚下次重试；更新包暂存 `数据根\updates\` 用完即清；弹窗描述区全量展示 notes、超高滚动（`max-height: min(44vh, 420px)`）。下载侧五个防呆：`download_update` 用 AtomicBool+Drop 守卫互斥（并发触发会截断同一 tmp 文件，直接报错不排队）；HTTP 客户端**不设总超时**，改连接 15s + 空闲读 30s（慢链路 ~30KB/s 下载 8.7MB 需数分钟，总超时必误杀慢而活跃的下载）；**断点续传**——下载中断（reqwest 报 error decoding response body，慢链路长连接被中途掐断的统一表象）后从 tmp 已有字节处 `Range: bytes=N-` 续传（R2 支持 206；服务器回 200 全量时清零重下；残片 ≥ 预期大小视为脏文件丢弃），sha256 在终局从盘上完整文件重算（续传前段未经流式 hasher）；**自动退避重试**——中断后 2s 自动续传重试（上限 8 次），用户无需反复手点；**416 防死循环**——续传偏移越界（清单与服务端包不同步）时丢弃残片重下而非反复 416；前端 `showAvailable` 在 busy（下载在途）时把重开的弹窗直接切到下载进度视图（进度事件仍在推送），避免「可更新视图 + 灰按钮无进度」的困惑态。**清单拉取与非 2xx 也要重试（v0.5.4 补）**：清单+签名拉取 3 次退避重试、下载的**非 2xx 响应码**同纳入 8 次重试（此前只重试网络错误）、全部失败路径落日志——清单从无失败记录恰是重试盲区，v0.5.4 发布首日踩中（COS 瞬时 404 一次即整单失败，且失败不落日志无从排查）
36. **联网开关（v0.3.0）：** `online.rs` 提供连通性探活 / 天气（Open-Meteo）/ 城市地理编码 / IP 定位 / 名言（hitokoto），受设置「联网」`online_enabled` 总开关控制；语录离线自动回退 `utils/quotes.ts` 本地语料
37. **应用壁纸 + 卡片玻璃透明度：** 壁纸仅主窗口渲染（index.vue 首子元素 `z-index:-1` 固定层，浮窗不跟随），导入走 `import_wallpaper`（复制进 `数据根\wallpapers\` 内容哈希命名 + 清目录旧文件，仅收 png/jpg/webp/bmp、≤30MB，gif 等动图拒收），assetProtocol（作用域仅白名单子目录，见约定 44）经 `convertFileSrc` 渲染；模糊作用于壁纸层整体（静态 `filter: blur`，**不是**卡片局部 backdrop——见 `AGENTS.md 约定 37`）；壁纸蒙版 `wallpaper_veil`（0–0.85 默认 0.3，主题中性底色 `--bg-base-a/b` 罩层，亮色提亮/暗色压暗）解决照片壁纸上侧栏/标题栏灰字与图标对比度不足的问题；沉浸模式 `wallpaper_immersive`（默认关，ADR 0003 受控例外）：`.card` 启用 backdrop-filter blur(16px) 局部取景 + 基底 alpha 大幅下调（亮 0.18/0.12 暗 0.10/0.06，仍乘 `--glass-dim`），开启时整屏静态模糊自动让位、设置里隐藏其开关；铬件（侧栏/标题栏）任何壁纸形态下都保持全透明与背景同一平面（勿垫材质/蒙纱，观感割裂——ADR 0003 有记录），其灰字/图标可读性由壁纸蒙版 + 壁纸态可读性增强负责：html `data-wallpaper` 标记驱动，灰阶令牌（--text-2/3/4）在铬件与主区（main）子树内向 ink 端压两档（不碰全局令牌），文字光晕双层分级——铬件为描边级（四向 1px text-shadow + 柔光）+ svg 黑晕仅暗色/白墨态保留（**亮色壁纸态不加白色 svg 光晕**——白晕在图标上呈粉笔白框，用户反馈已去掉；黑晕承担浅色图标在浅色照片上的对比），卡片在真实透底时（`data-wallpaper-clear` = 玻璃透明度 <0.9 或沉浸模式）才加弱一档柔光晕（.card/.sv-content/.extension-center/.le-root，图标只有黑晕没有白晕），不透底保持锐利，侧栏 hover 气泡等瞬态表面排除在外；透底态卡片**去白边**：`.card` 的 border-soft 白描边 + frost-edge 顶部白高光在壁纸上呈粉笔白框，经 `data-wallpaper-clear='1'` 作用域收敛为透明描边只留落影（亮色 `--shadow-card` 仅 5–6% 暖影、照片上不可见，补一档中性落影；暗色沿用原落影），无壁纸/不透底时白边照旧；卡片玻璃透明度 `glass_opacity`（0.4–1.0）经 `--glass-dim` 乘进 `--frost-base` alpha，亮暗两套基底共用同一乘数，弹窗/菜单等瞬态表面不受影响；壁纸文件被外部删除时前端静默回退渐变背景（不抹配置）；**扩展与壁纸必须共存**（扩展侧页面底）：扩展 view 形态的容器链（`.view-extension` → `.extension-view` → `iframe[background:transparent]`）全透明，页面底由扩展自己铺——`themeTokens` 专门导出 `pageBg` → 桥脚本映射 `--mhub-page-bg`（无壁纸 = 宿主整页背景，有壁纸 = `transparent`），扩展写 `background: var(--mhub-page-bg, transparent)` 两种形态都对；**扩展若用 `--mhub-bg-page` 铺底会把壁纸整块盖住**（它是 alpha=1 的不透明渐变），而透底态宿主又把 `--mhub-text-*` 整体翻白 → 白底白字（实测踩过；契约测试 `bridge_script_maps_all_theme_tokens` 守着令牌映射齐全）；扩展抽屉 `.ext-drawer` 壁纸态改为 `--frost-surface` + 真 `backdrop-filter` 取景模糊（它不在 main 子树里、取不到透底态对 `--bg-card-solid` 的覆盖，保持 0.88 白会在壁纸上切出一块死白），透底态去左描边留落影 + 文字光晕（口径同 `.card`）

38. **速记编辑器（Milkdown Crepe）+ 笔记图片：** `NoteEditor.vue` 用 Crepe 所见即所得（`@milkdown/crepe` + `@milkdown/kit`，Markdown 为真相源，`markdownUpdated` 序列化结果走既有 600ms 防抖；AI 特性关闭保持纯本地；index.vue 经 `defineAsyncComponent` 异步分包加载，浏览器预览无后端时图片回退 data URL）；主题靠 `.milkdown` 上的 `--crepe-*` 变量映射设计令牌，暗色经 `[data-theme="dark"]` 覆盖（Crepe 无内置动态主题切换，Milkdown #1839），**`--crepe-color-outline` 必须映射中性灰墨（`--text-3`）**——它同时承担工具栏/块把手/链接气泡的图标色与发丝线，曾误映射 `--border-soft`（亮色 55% 白）导致白图标叠白底工具栏不可见；壁纸透底态（`data-wallpaper-clear='1'`）下编辑器浮层（工具栏/斜杠菜单/链接气泡/图片说明）经同名作用域收为深玻璃实底 + 白系图标（30% 烟玻璃叠亮部照片会糊掉白图标），blockquote 默认 `padding-left: 40px` 过宽已覆写为 12px；Crepe 自带 UI 文案默认英文，已在 featureConfigs 统一汉化（斜杠菜单三组/图片块上传按钮与占位/空文档占位符「开始记录…」/链接提示/选中工具栏 aria 标签），升级 Crepe 后新增文案同样在该配置补。图片三路入口（粘贴/拖拽/点击上传）**全部由 Crepe 上传管线处理**：plugin-upload 的 `handlePaste`/`handleDrop` + `ImageBlock` 的 `onUpload/inlineOnUpload/blockOnUpload` → `utils/noteImage.ts` → `import_note_image` 命令（base64 传参，内容哈希命名落盘 `数据根\notes\images\`，png/jpg/jpeg/webp/bmp/gif、≤10MB，同内容天然去重）；**切勿自建 DOM paste 监听**——plugin-upload 已处理粘贴，叠加监听会插入两张重复图片。Markdown 内嵌 `http://mhub-note.localhost/<hash>.<ext>`——lib.rs 注册的 `mhub-note` 自定义协议按数据根实时解析文件（URL 不含数据根绝对路径，迁数据目录后仍有效；Windows 形态为 `http://<scheme>.localhost/`），文件名严格校验 16 位哈希 + 扩展名白名单防路径穿越，孤儿图片暂无 GC。**图片尺寸接管（宽度语义）：** Crepe 的 `onImageLoad` 会把图片高度锁成像素（`style.height`，宽度 auto）——窗口放大后图片不跟随变宽（用户反馈过）；NoteEditor 在 CSS 层 `height: auto !important` 解锁、让宽度成为唯一尺寸维度，并按 node attrs 的 `ratio` 恢复尺寸：ratio=1（默认）响应式 `min(自然宽, 100%)` 占满内容区，ratio<1（用户拖过宽度把手）按百分比定宽；ratio 沿用 Crepe 的 markdown alt 序列化通道（`![0.75](url "caption")`）持久化。Crepe 内置 row-resize 高度把手已 CSS 隐藏，自定义**宽度把手**（image-wrapper 右下角：CSS `::after` 斜向三点视觉 + JS `IMAGE_BLOCK_HANDLE_PX` 26px 命中区）由 NoteEditor 在编辑器根上 pointer 委托实现——pointerdown 取消（preventDefault）即可抑制 ProseMirror 的 mouse 兼容链，up 时 ratio = 实测宽 ÷ min(自然宽, 容器宽) 写回 node attrs（≥0.98 归一为 1 恢复响应式），拖拽中 `buttons === 0` 视为松键（鼠标移出窗口松键不派发 pointerup）。**点击图片预览**：click 委托命中 `.milkdown-image-block .image-wrapper`（排除 caption 输入/右上角操作按钮/把手区）或行内 `img.image-inline` 打开 lightbox（Teleport 到 body + `--scrim` + backdrop-filter 瞬态表面，Esc/点击遮罩/关闭按钮关闭，z-index 200）。标题自动派生：标题为默认值（空/「无标题笔记」）时从正文首行取纯文本（`utils/markdown.ts` 的 `deriveNoteTitle`，NoteList 摘要与 GlobalSearch 片段同样经 `markdownPlainText` 去语法），用户改过标题即不再接管。**标题输入框 `@input` 也必须走 scheduleSave 防抖保存链路**（否则只有改正文才落库——改完标题不改正文即丢失，切换/重启回退为派生值）；空标题落库时归一为「无标题笔记」（normalizeTitle）。`markdownPlainText` 需清理：hardbreak 的行尾反斜杠形态（`\\+\n`，粘贴富文本时 `<br>` 转成 hardbreak 的序列化残留）、字面 `<br>`/`&nbsp;`（网页纯文本复制残留）与 CommonMark 转义符——顺序上 `\\+\n` 必须在 `\s+` 空白合并之前、转义符先于 `<br>`（否则 `\<br>` 漏清），deriveNoteTitle 逐行判断前还要去掉行尾孤立 `\`。

⚠️ **不可见字符清理（`utils/invisibleChars.ts`，与 Rust `repo::note.rs` 同码位）**：粘贴的网页正文常夹带零宽空格（`U+200B` 防断词）、软连字符（`U+00AD`）这类 `\s` 不认的字符。**一条字符分三类处理，判据是「显示上占不占宽度」**：① 有可见宽度的（NBSP、各类 Unicode 空格、全角空格、换行/段分隔符）→ **换成普通空格**（删掉会让两个词贴在一起，产出「缺了间隔的伪连字」，而那个间隔在原文里存在过）；② 不可见的零宽类与软连字符 → **删掉**（换成空格是**凭空往文本里注入空格**：第一版一律换空格，于是 `discuss<U+200B>ion` → `discuss ion`、`第<U+200B>3` → `第 3`，把单词劈开了）；③ ZWJ `U+200D` **刻意不清**（emoji 序列靠它组合，清掉 emoji 就碎）。**两处必须一致**：前端清的是摘要与搜索结果片段，Rust 清的是**搜索命中**——`LIKE` 完全不认这些码位，`发<U+200B>布计划` 配关键词「发布计划」在 `LIKE` 眼里那四个字根本不连续，搜不到。Rust 侧兜底是「第 1 段原样 `LIKE` 落空才全表扫 + 归一化比对」，**必须同时归一化正文**（第一版只清关键词侧，测试当场就红），且关键词归一化后为空时要返回空而不是继续（否则 `contains("")` 恒真，整个笔记库会被当成结果）。
    两条顺序：`markdownPlainText` 里 **`stripInvisible` 必须在结构清理之前**（网页复制的正文常把零宽字符插进标签内部，`<br<U+200B>>` 对 `/<br\s*\/?>/` 不匹配，先清零宽字符才认得出是换行标签）；Rust `search_like` 的 **`ESCAPE '\'` 两个谓词都要写**（只写一处 → 搜正文里的 `%` 不命中）。
    守卫：`invisibleChars.test.mjs` 8 例 + `markdown.test.mjs` 10 例 + `repo::note::tests` 19 例，全部做过变异验证。**通配符必须转义**（搜 `100%` 否则变成「100 开头、任意结尾」），且这类断言要直测 `search_like` —— 走 `search` 会被第 2 段的全表扫兜回来，实测去掉 `ESCAPE` 后测试照样全绿。三个时序陷阱：① Crepe 容器 rootEl 在 setup 阶段未渲染，watch 须同时观察 rootEl 且 `flush:'post'`，否则首次挂载永远空白；② watch 回调引用的所有状态（saveTimer/noteTags 等）必须声明在 watch 之前——immediate 回调在 setup 阶段同步执行，后置声明触发 TDZ；③ 防抖的「同步回显不保存」判断必须用与当前笔记内容的**一致性比较**，不能用一次性布尔标记（会吞掉整段粘贴等单批次编辑的首次保存）。块拖拽（六点把手）已改为**指针实现**（`utils/blockDrag.ts`，NoteEditor 在 crepe create 后经 `editorViewCtx` 挂载）：pointerdown/move/up 跟踪 + 插入线指示 + 单事务搬移顶层块（一个撤销步骤）。原因：plugin-block 原生走 HTML5 DnD（dragstart 写 dataTransfer → dragover/drop 搬移），与主窗口 `dragDropEnabled` 的原生文件拖放拦截互斥——Tauri 窗口内拖拽启动后收不到 dragover/drop，表现为「拖得动、落不下」；指针实现不再依赖 HTML5 DnD，浏览器与 Tauri 行为一致，速达原生拖入不受影响。util 在捕获阶段拦掉把手容器上的原生 dragstart（window capture，须先于 plugin-block 的监听），并依赖 Crepe 把手 DOM 结构（`.milkdown-block-handle` 内两个 `.operation-item`，第 1 个加号、第 2 个把手）——升级 Crepe 需复核。`posAtCoords` 探针 x 必须取 pm 内容水平中心（左缘附近处于内边距区会粗解析错块）；`coords.inside` 命中文本块时返回的是**块的起始 pos**（resolve 后 depth=0），走 nodeAt 分支爬顶层。外部图片拖入编辑器上传仍受原生拦截限制（粘贴/点击上传可用）。另注意：速达拖入导入只在 Tauri 窗口可用，浏览器预览中拖文件显示禁止图标是正常表现（无后端接收）

39. **扩展 iframe 后台久置空白与两层自愈：** 扩展 iframe 是跨源帧（`asset.localhost` ≠ 主窗 `tauri.localhost`，独立渲染进程），窗口长时间不可见（隐藏到托盘/最小化/完全遮挡）后 WebView2 会挂起或丢弃其渲染状态，恢复前台后扩展区表现为空白而宿主 UI 正常，且点击「同一个」已打开的扩展不会自愈——`openExtensionSurface` 每次都 new 对象但 id/surface 值不变，useExtensionFrame 的 watch 比较值、不触发重载。两层修复：① `useExtensionFrame` 监听 `visibilitychange`，恢复可见且后台超 60s（`RESUME_RELOAD_AFTER_MS`）即重载 iframe（短时切换不重载，避免丢扩展内输入状态；`hiddenAt` 初始化即隐藏也统计，覆盖 `--autostart-hidden` 静默驻留场景）；② 宿主每次「打开某扩展」（`onOpenExtension`/`openExtensionSurface` view+drawer 分支/`openExtensionDrawer`）递增 `extensionReloadTick`，经 `ExtensionView` 的 `reloadKey` prop 纳入 watch 源，点击同一扩展也强制重新导航。此前表现为「后台久了点开扩展空白、来回切换才恢复」，切换之所以有效正是触发了 extId 变化那条重载路径

40. **待办排期/子待办（v0.3.4）：** 待办支持**截止日期 + 提醒 + 子待办**。数据层：`todos` 表加 `due_at`/`remind_at`（毫秒时间戳，倒计时同款 epoch 基准）、`remind_fired`（防重复提醒）、`parent_id`（REFERENCES todos ON DELETE CASCADE，删父级联删子；仅一层无嵌套）；老库经 `db.rs` 幂等 ALTER 迁移。命令：`create_todo` 增可选 `parentId`、`schedule_todo(id, dueAt, remindAt)`（每次排期重置 remind_fired 重新武装提醒）。提醒触发在 Rust `todo_reminder.rs` 后台线程（1s 轮询 `remind_at` 到期且未完成未触发的项；超 5s 视为错过静默跳过不补发——与倒计时 ticker 同款语义），到点发托盘气泡通知 + emit `todo-remind`（主窗 toast）+ `todos-changed`。前端：分组规则 逾期→今天→有日期→无日期（`utils/todoSchedule.ts` 纯函数，徽标 逾期红/今天橙/明天品牌色/更远灰，仅日期的截止按当天 23:59 展示为「今天」无时间）；行渲染抽成递归子组件 `TodoRow.vue`（子待办缩进嵌在父行内、子行无优先级圆点/无「+」；父行显示 n/m 进度条），卡片经 provide 注入 `todoOpenSchedule`/`todoRemoveTodo`（排期弹层单实例 Teleport 到 body；删除在卡片做级联+撤销恢复）。注意：待办浮窗与待办概览卡只统计/展示**顶级待办**（`parent_id == null`），子待办不进总盘子。**拖拽排序（v0.4.3）**：顶级待办支持组内上下拖动（`todos.sort_order` 列，NULL=未手动排序；指针实现而非 HTML5 DnD，同约定 14/38），落点后整组 id 顺序经 `reorder_todo_orders` 写回 sort_order；组内排序键统一 `utils/todoSchedule.ts` 的 `compareByOrder`（sort_order 升序，未排序按创建时间倒序排在前），TodoCard / TodoFloat 共用；改截止日期换组时 `repo::schedule` 清空该条 sort_order 回默认排序，新建条目落入手动排序过的组时由 store.createTodo 以 [新条目, ...组内原序] 整组重写补置顶位；子待办不参与拖动。**待办 v1（标签 / 置顶 / 周期 / 日历视图）：** 数据层再加 `description`（轻量 Markdown，勾选一律用子待办）/`pinned`/`repeat_mode`+`repeat_*`（once/daily/weekdays/weekly/monthly/yearly/custom，含 until/count 结束条件）/`repeat_done_count`；标签是**独立于笔记标签**的一套（`todo_tags` + `todo_tag_links`，见 ADR 0010）。**周期规则的唯一实现在 Rust `todo_recurrence.rs`**（前端只消费 `expand_todo_occurrences` 的展开结果，不重写规则，从根上不存在两端漂移）：勾选 = 「完成本轮」——`due_at` 就地滚到下一个**未来**时刻（逾期不补历史）、`remind_at` 同偏移平移并重新武装、计数 +1、子待办复位、**不置 done**（永不进已完成列表）；规则用尽（until 越界 / count 用尽）转回一次性并保留该行；撤销把 `due_at` 滚回上一轮。⚠️ 迭代预算用尽（`MAX_ITER`）与「规则自然结束」必须区分——前者返回 Err 上报，混为一谈会把还在生效的周期**静默降级成一次性**。规则列范围一律 `RepeatRule::validate` fail-fast（扩展桥是不可信输入源：`monthNth=0` 会让规则永远算不出下一轮、`every` 极大值会让 chrono 溢出 panic 并毒化 DbState 互斥锁）。工作台新增独立 `calendar` 模块（`TodoCalendarCard.vue`，只读、点整卡进待办视图），**原 clock 的「整月日历」形态已删除**（工作台不需要两个日历；老布局里的 `variant:'month'` 由 `dashVariantDef` 归一为 `big`，不做数据迁移，见 `docs/todo-upgrade-plan.md` §7）。待办独立视图 `TodoView.vue`（异步分包）：宽窗左列表 40% + 右日历 60%，窄窗切单栏 + 「范围 × 形态」双维工具栏；行渲染**复用 `TodoRow.vue`**（不再自绘），拖拽排序逻辑抽在 `composables/useTodoDrag.ts`（卡片与视图共用同一份）；日历格子可拖拽改期（真实条目改 `due_at`，周期算出的虚拟实例不可拖）。**日期/时间一律用应用内控件**（`TodoDateTimeField.vue` = reka-ui DatePicker + TimeField），禁止原生 `datetime-local`；下拉一律 `AppSelect`（其 `compact`/`disabled` 变体由组件自身承担尺寸与禁用视觉）。扩展桥新增 12 个 `data.*` 能力（`todos.setDescription/setPinned/setRepeat/setTags/completeRecurring/undoRecurring/expandOccurrences` + `todoTags.list/links/create/update/delete`），仍全部落在 `CAPABILITIES` 静态表里

41. **运行时禁止现场创建/销毁 WebView2 窗口（悬浮球/剪贴板浮层卡死铁律，v0.5.0）：** 应用启动后**运行时** `WebviewWindowBuilder::build()` 或 `win.destroy()` 一个 WebView2 窗口是主线程长任务，与悬浮球菜单收拢/几何切换等窗口操作在主线程事件循环上交错时，**WebView2 controller 创建会挂起 → 整窗未响应（卡死）**。曾两次踩中：剪贴板浮层现场创建与悬浮球操作交错（已改启动预创建隐藏常驻，见 `clipboard.rs::init_overlay_window` 注释）；悬浮球「停用 = destroy、启用 = 运行时 rebuild」（设置开关反复切换即卡死，已改为启动预创建 + 开关只 `hide()/show()`，见 `floating_ball.rs::init/apply_enabled`）。**铁律：凡是会随设置开关/高频切换存在性、或与悬浮球/拖拽/菜单等窗口操作可能交错的浮窗，一律启动期预创建 + 隐藏常驻，唤起/收起/开关只做 ShowWindow 级快操作，绝不现场 build/destroy**（renderer 常驻内存 = 换取零卡死 + 零建窗延迟的既定代价）。新增浮窗生命周期先照此自查：能常驻就常驻；确需随用随建的（便签/倒计时浮窗等低频瞬态窗口）也要避开与上述窗口操作同时发生。相关：新增浮窗的 label 四处同步见约定 31。**⚠️ 事故实录（v0.5.3 已发布版通知弹窗整体失联，2147bd9 审查批次引入）：本铁律的执行方式是把「运行时现场 build 兜底」从取窗函数里删掉——但删的时候必须保住启动期 `init()` 里的那一次 build，它不是兜底、正是本铁律要求的预创建本身。删成「全工程零 build」= 窗口从此不存在，且症状是静默失效（事件全进永不被重放的暂存队列），测试与类型检查全绿、只有实机暴露。教训三条：① 执行「禁止运行时 X」类规则前先 grep 确认删后仍恰有一处启动期 X；② 隐藏常驻窗的「前端 ready 信号依赖窗内脚本」时，未就绪分支不许纯排队死等（隐藏态 WebView2 可能不跑脚本），要先显示透明空窗促加载再暂存（见 `notify.rs::show_notice` 注释）；③ 此类跨端时序回归单测覆盖不到，改动通知/剪贴板/悬浮球任一浮窗后，发版前必须实机触发一轮（下一条清单已加）。**第三次复发（v0.5.4 AI 对话独立窗，约定 41 被「例外论」击穿）**：新功能以「用户在设置页把开关切到开的那一刻没有并发窗口操作，现场建一次从此常驻，符合铁律意图」为由，在 `chat_window::apply_mode` 运行期 build → 用户实机开关即整 app 挂死（对话窗打不开、供应商列表等所有同步命令不再返回、托盘退出无反应）。伪论证点在于「此刻没有并发窗口操作」根本不可保证（悬浮球边缘监视循环 100ms 一次搬窗、通知/剪贴板隐藏窗常驻）。**结论：本铁律零例外——全工程任何运行期 `WebviewWindowBuilder::build()` 都不允许，浮窗无条件启动预创建（哪怕是低频可选功能，闲置 renderer 内存是铁律已定价的代价）；「为省内存按配置惰性建窗」= 把 build 挪进运行期，同罪**
42. **悬浮球贴边几何多处口径必须一致（v0.5.3 修「拖开还弹回」「悬停露不全」「贴边即消失」「甩出屏外死锁」）：** 停靠/滑出全由 Rust 边缘监视循环 `floating_ball.rs::edge_tick`（100ms 轮询 `GetCursorPos` + 窗口矩形、直接 `SetWindowPos`）实现，前端零参与。**七条铁律**：① **滑出量 = 窗口半边长**（`BALL_SIZE/2`，不是球半径 `BALL_R`）——按球半径只平移 24px 时窗口仍有 26px 留在屏外，悬停看不到完整球体与周围粒子/陀螺环（视觉最外圈半径 47 > 露出的一半）；② **`DOCK_TRIGGER`（吸附触发距离，逻辑 px）必须 > 滑出量 + `POS_TOL`（位置一致容差，物理 px）**，现值 120 > 50 + 10——定界依据用户实测数据：真贴边的松手点球心距边 4~103px（103 是「靠边了但不吸附」的抱怨点，必须覆盖），而用户明确称为「不靠边的地方」最小是 146px；120 恰好分开。**教训：吸附带曾按「用户以为的靠边」一路放宽到 200，结果 146~199 的自由放置也被吸走，球从松手点滑到屏边被感知为「没靠边也自己挪一下/抖动」——吸附带必须按「真贴边带」定，不按单次抱怨扩**；吸附动作本身（松手后球平滑滑到屏边）是预期行为，不是 bug；③ **停靠记忆是唯一真相，窗口向记忆收敛**（`at_hidden` / `at_popped` 之一在 `POS_TOL` 内）：窗口不在停靠几何上 = drag_end 落位搬窗丢失，补搬到（光标停在球上→滑出位，否则→半隐位），**绝不反向改记忆**——旧逻辑「以窗口为准改记」把刚吸附的位置改漂（日志实证记忆 y 在 446→435→416→365 间乱跳）；球被拖走必经 drag_end 重写记忆，轮询期间记忆不可能过期；自由位记忆同理：窗口偏离记忆位 → 补齐到记忆位；滑出/半隐两态也按位置实时推导，不再有进程内 `POPPED` 布尔缓存；④ **拖拽松手检测必须在 Rust 监视循环做（`lmb_down()` 左键释放后的第一跳），绝不能用 `startDragging()` 的 promise 当拖动结束信号**——实测该 promise 在拖动开始时就 resolve，松手钩子读到的是拖动中途位置（与最终位置差几百 px，日志实证：`拖拽松手 球心=(937,527)` 而窗口实际在 (368,464)），吸附/记忆全错，冷却后「落位补齐」再把球从屏边搬回中途（「拖到边缘松手，球弹回屏幕中间」的根因）。前端位移超阈值移交原生拖动时调 `floating_ball_drag_begin` 仅记录武装时刻（**附带 30s TTL + `floating_ball_drag_cancel`**：startDragging 启动失败/拖动中窗口隐藏等残留标志超期自动作废、失败路径主动清零——否则用户下一次无关的左键单击松开会被当成拖拽落位，「带外绝不移动」被破坏）；`edge_tick` 发现左键释放后的第一跳执行 `settle_drag`（钳制/吸附/落位/写记忆一次做完，末尾 emit `floating-ball-settled` 让前端重拉停靠边）；**所有窗口搬移都是异步 IPC**：`settle_drag` / 补齐搬窗后武装 `LAST_DRAG_SETTLE_MS` 冷却（800ms）——监视循环在间隙读到「窗口旧位置 + 新记忆」会误判（日志表现：松手后立刻连刷「停靠纠偏」、y 来回跳）；落位搬窗丢失不当场复读补搬（监视循环的时序已无此必要），交给冷却后的「落位补齐」按记忆收敛；滑出/隐回的 `slide_to` 在监视线程内串行执行、目标位都在候选集里，天然无此竞态；⑤ **半隐位 = `dock_hidden_pos`（停靠球心位 − 半边长 + dx·PEEK）**，`edge_tick` 与 `drag_end` 共用这一个函数，禁止各写一份——PEEK（8 逻辑 px）是向屏内多露的余量，球心精确压屏边只露 24px 一条弧、观感像「球直接没了」（用户反馈）；⑥ **工作区矩形一律用 `nearest_work_rect`**：窗口完全离屏时 `current_monitor()` 返回 None（手速快把球甩出屏外），`drag_end` 的钳制/吸附/落位与监视循环会整段失效 → 球点不到也拖不回的死锁；该函数兜底取「中心最近」的显示器；⑦ **`edge_tick` 开头（AUTO_HIDE 检查之前）有无条件救球分支**：球心（窗口中心）出工作区 → 钳回 + 写记忆 + 冷却；半隐停靠球心恰在边缘线上不会误触发。落位吸附算法统一在 `dock_snap`，半边长都取窗口实际 `outer_size` 的一半——任何一处改口径都要同步其余，详见 `AGENTS.md 约定 42` 实施修订节。

43. **`<style scoped>` 里 `:global()` 必须整条选择器包进同一个括号（lightningcss 管线陷阱，v0.5.5 实测）：** `:global(前缀) 后代选择器` 的写法会被 lightningcss 压缩管线**吃掉括号后的后代部分**——产物里只剩 `前缀 { }`（作用到整个 body），后代样式**从未编译进去**且不报任何错。曾中招：剪贴板浮层 3 条深色规则（`.cb-panel/.cb-ctx/.cb-toast`）长期静默失效；TodoRow 拖拽抑制规则写成该形式被编译成 `body.todo-row-dragging { display:none }`，**拖拽瞬间整页消失**。正确：`:global(html.dark .foo)` / `:global(body.todo-row-dragging .todo-del)`。自查：`grep -E ':global\([^)]*\)\s*[.#a-zA-Z\u4e00-\u9fff]' src/**/*.vue` 应为零命中（注释除外），或构建后在 `dist/assets/*.css` 里搜目标选择器确认完整存在

44. **扩展内容走独立协议 `mhub-ext`，资产作用域只放行白名单子目录（安全铁律；本条即原 `docs/adr/0008` 的全文，那份文档从未提交过）：** 扩展入口与相对资源由 `lib.rs` 注册的 `mhub-ext` 自定义协议服务（URL `http://mhub-ext.e-<id 摘要>.localhost/<扩展 id>/<相对路径>`，实现见 `ext_protocol.rs`），入口 HTML 由该协议**动态注入桥脚本**（不再写 `<扩展目录>/.xhpack/<surface>.html`，开发源码目录因此不会被写脏）。这样做的唯一目的是让各扩展独立 origin（`mhub-ext.e-<id 摘要>.localhost`）与承载用户数据的 `asset.localhost` **跨源**——否则扩展里一行 `fetch` 就能取走数据根下的 `mhub.db`/`app.json`/日志，桥 API 的权限系统形同虚设。配套：`tauri.conf.json` 的 `assetProtocol.scope` 为 `[]`，`lib.rs` 启动时只 `allow_directory` 白名单子目录（`icons`/`wallpapers`/`clipboard/images`，扩展图标也走 `mhub-ext` 协议）——**任何"顺手放行整个数据根或 `$APPDATA/**`"的改动都是安全回退，禁止**。协议侧校验：扩展 id 形状白名单 + 相对路径逐段 percent 解码后禁 `..`/`\`/`:`/NUL/控制字符 + canonicalize 后必须仍在扩展目录内（防符号链接逃逸）；响应一律 `Cache-Control: no-store`。前端**不再自拼 asset 地址**，`read_extension_entry` 返回该协议的完整 URL。
45. **本机源码目录直挂 + 热重载（旧称「开发者模式」；本条即原 `docs/adr/0005` 的全文，那份文档从未提交过）：** 扩展中心「我的扩展」标签页加进来的本机扩展源码目录会被直挂加载，**不复制进扩展根、没有开关**。配置只存 `dev_extensions`（`dev_mode_enabled` 已废弃、读到即忽略）；目录映射**不落盘**，启动 setup 里按配置重建（`apply_dev_extensions`）；已安装与开发扩展目录均不得加入共享 asset 作用域。`ExtensionEntry.source` 区分 `installed`/`dev`；同 id 冲突时**已装优先**并跳过开发目录；开发扩展不进已装列表、不参与市场更新与卸载（对 dev id 调 `uninstall_extension` 会明确报错指路，不静默成功）。宿主侧一律用 `ext_protocol::resolve_ext_dir` 解析扩展目录（已装→扩展根，开发→源码目录），`service.rs` 起后端同样适用。热重载：`dev_extensions_stamp` 对开发目录树（跳过 `node_modules` 与 `.` 开头项）算 FNV+mtime，每个扩展 iframe 每 1.5s 轮询、戳变化且自己是 dev 扩展即重设 iframe `src`——**已装扩展那套 `extensions_stamp` 只盯 manifest、改 HTML 不触发，热重载不能复用它**。**宿主侧一切「按扩展 id 找文件」的路径必须经 `ext_protocol::resolve_ext_dir`**（已装→扩展根，dev→源码目录）：用 `extensions_root().join(id)` 会让开发扩展的桥 API 整条失效（`load_manifest`/`storage`/`config`/`permissions` 全 NOT_FOUND），更糟的是 `storage.set` 的 `create_dir_all` 会**在扩展根下凭空建出 `<id>/` 目录** → 下次扫描把它当成没有 manifest 的扩展标 invalid（列表显示「读取 manifest.json 失败 (os error 2)」），并因 id 相同把**真正的开发扩展挤掉**（`scan_extensions` 的同 id 冲突检查必须只认 `!invalid` 的条目）。实测踩过：用户在计算器里算了一步 `9×6`，扩展根下就多出个只含 `.storage.json` 的空壳目录，列表里只剩一条点不开的「不可用」；`open_extension_window`/`get_extension_permissions`/`permissions_path` 三处同因。dev 扩展的数据文件（`.storage.json` 等）落在**源码目录**（点开头、打包与 `**/.*` 忽略规则都会排除）。**⚠️ 路径归一（v0.6.2 修「开发目录挂的 service 扩展后端永远起不来」）：Windows 的 `std::fs::canonicalize` 返回 verbatim 形式（`\\?\A:\…`），它能被 Rust 自己的 fs 调用接受，但**交给外部程序就出事**——Node 的 CJS 加载器会把 `\\?\A:\…\backend\server.js` 拆错（`Error: EISDIR: illegal operation on a directory, lstat 'A:'`）后 `exit 1`，后端在模块初始化阶段就死；`opener`/`explorer` 同样不认。所以**凡是落盘、展示、交给外部进程的路径都必须过 `paths::simplify_path`（剥 `\\?\` / `\\?\UNC\`；含 `.`/`..` 组件与 `Volume{…}` 设备路径不剥）或 `paths::simplify_existing`（简化后仍可访问才采用，兜超长路径）**：`add_dev_extension` 落盘前归一、`apply_dev_extensions` / `dev_mode_status` / `dev_extensions_stamp` 读取时归一（历史配置自愈，用户不必手改 `app.json`）、`remove_dev_extension` 与 `add` 的「是否已登记」比较都按归一后路径、`ext_protocol::resolve_ext_dir` 出口统一归一（一处覆盖 service / 打开目录 / storage / 打包 / precheck 全部调用方）、`service::backend_paths` 再兜一道（`argv[1]` 带前缀必死，`current_dir` 带前缀无害但统一口径）。**回归测试**：`paths::tests::simplify_strips_verbatim_prefix`、`service::tests::backend_paths_strip_verbatim_prefix`、`service::tests::backend_entry_runs_under_real_node`（真起 Node 跑一遍）。**另：service 后端的 stdout/stderr 现在落盘** `<数据根>/logs/service/<扩展 id>.log`（单份上限 1MB，超限删档重来），探活失败时宿主日志会带 `exit=<退出码>` + 日志尾部 20 行——此前 `Stdio::null()` 把「路径不对 / 端口占用 / 依赖缺失」全吞了，前端只剩一个 `serviceReady=false`，表现成「像在下载 Node」。
46. **市场清单只追加字段、不抬 `schemaVersion`；撤销与权限告知：** 客户端对未知字段宽容（serde default），而 `market.rs` 硬拒 `schemaVersion>2` 并回退缓存——**抬版本号会让所有老客户端的市场变空**，因此新字段一律追加、不动版本号。本版追加：`permissions`（发布时由服务端从 manifest 写入，供安装前分级告知）、`revoked`（清单顶层 `id@version` 数组）、`publisher_id`/`verified`。已装版本命中 revoked 时：扩展中心标「已下架」+ 说明，**只警示、绝不静默卸载或禁用**；被撤销的目标版本不提供更新入口（`updateFor` 过滤）。`pack_extension_archive` 打 `.xhpack`（zip，manifest 必须在包根，排除 `node_modules` 与 `.` 开头项）供发布上传。打包时**无扩展名的 `LICENSE` 在包内自动改名加 `.txt`**（服务端关卡按扩展名白名单拒绝无扩展名文件，见 m-hub-server `gate.ts::ALLOWED_EXT`；只改 zip 条目名不动源码目录，保留开源许可声明）。

47. **客户端 ↔ 服务端的请求规格只有一份（`src-tauri/src/api_spec.rs`）：** 所有打到 m-hub-server 的路径与请求体字段名都从那里取，**不要在命令里硬编码 URL**——这层属于「只有联调才会暴露」：两端各自的测试都是绿的，一对接就 404/400，而失败现场往往离改动点很远。字段名是**蛇形**（`poll_id` / `min_app_version` / `invite_code`），因为服务端 Hono 读的是原始键名、不做驼峰转换；`api_spec.rs` 的测试同时锁住「路径与服务端路由一致」「body 用蛇形键」「base 结尾斜杠归一化」。发布上传（multipart）另有一组契约测试（`publisher.rs::tests`），用本地 mock 服务端把客户端真实发出的请求抓下来断言字段名与文件名——**改这两个文件的契约时，先看测试而不是先看调用方。**

48. **浮窗「不进任务栏」必须走 `win_taskbar`（Windows 铁律）：** `WebviewWindowBuilder::skip_taskbar(true)` 在 Windows 上**不足以保证**浮窗不进任务栏——tao 0.35.3 的实现只有一次 `ITaskbarList::DeleteTab`（`tao/src/platform_impl/windows/window.rs::set_skip_taskbar`），而窗口本身对 `Parent::None` 一律置 `WindowFlags::ON_TASKBAR`（同文件 1164 行），ex-style 里始终带 `WS_EX_APPWINDOW`；`WindowFlags::apply_diff` 又在 VISIBLE 变化（每次 hide → show）时用 `SetWindowLongW(GWL_EXSTYLE, …)` 把 ex-style 整份重建回带 APPWINDOW 的形态，Shell 随即重新给它加任务栏按钮。实机取证：悬浮球窗口 ex-style `0x00040118`（APPWINDOW 开、TOOLWINDOW 关、无 owner）→ 任务栏多出一颗「悬浮球」图标；对照组是 tao 自建的托盘 / 热键窗口 `0x080801A0`（带 TOOLWINDOW）→ 从不进任务栏。修复口径在 `src-tauri/src/win_taskbar.rs`：`apply()` 置 `WS_EX_TOOLWINDOW` + 清 `WS_EX_APPWINDOW`（工具窗是 Shell 判定「归不归任务栏管」的依据，顺带不进 Alt+Tab）+ 补一次 `set_skip_taskbar(true)` 摘掉**当前已存在**的按钮；`show()` = `win.show()` + `apply()`。**新增浮窗、或给浮窗加新的显示路径：创建后 `apply()`、显示走 `win_taskbar::show()`，禁止裸 `win.show()`**（主窗 / AI 对话独立窗 / 扩展独立窗是常规窗口，不适用）。顺序可靠性：浮窗显示均发生在 UI 主线程，`send_user_message` 在主线程分支是同步执行的，故「show 之后立即 apply」不会被 tao 的 ex-style 重建反超。

49. **设置页的搜索索引是构建期生成的（`scripts/gen-settings-index.mjs` → `src/components/settingsIndex.generated.ts`）：** 设置页只挂载当前大类（见结构树 SettingsView 说明），**其它大类的设置项不在 DOM 里**，所以顶部搜索框不能靠遍历页面找，必须有一份独立索引；而这份索引**不许手写**——手写必然随实现漂移，症状是「搜不到刚加的设置项」而没人发现。做法：`prebuild` 钩子在每次 `npm run build` 前从 `SettingsView.vue` 模板按 `sv-sec-*` 分区提取标题，**`setting-name` 与 `theme-label` 两套 class 都要提**（外观区的主题模式/主题配色/强调色/应用壁纸/卡片玻璃透明度用的是后者，漏了这几项最常搜的设置就搜不出来）；含 `{{ }}` 插值的条目跳过（账号名、扩展名等不是稳定搜索目标）。索引文件头写明「自动生成勿手改」；改完设置项跑 `npm run gen:settings-index`（幂等，无变化时不改文件）。命中后的跳转 = 切大类 → 按标题文本在 DOM 里找回那一行 → 滚过去并短暂高亮，找不到行则退化为滚到该分区。

50. **设置页按大类拆成面板，样式走 `settings/shared.css` + `.settings-view` 前缀（口径铁律）：** 设置页 = 外壳（`SettingsView.vue`：两级导航 + 设置项搜索 + 面板路由）+ 7 个大类面板（`src/components/settings/*Panel.vue`，按需 `import`；首屏只加载外壳 + 当前大类，切大类才取对应面板，左栏 hover 会预取）。三条必须遵守：① **面板里不放通用设置样式**——scoped 不跨组件，通用样式（`.setting-row` / `.toggle` / `.setting-info`…）抄 7 份就是维护地狱；统一写进 `settings/shared.css` 且**每条规则必须带 `.settings-view` 前缀**——`.toggle` / `.toggle-knob` 在 AboutSection、ExtensionSettingsDialog 里也各自 scoped 定义过，`.mask-*` 在 5 个文件里都有，不加前缀会直接改掉那几处的外观（产物侧已核对：`.settings-view .toggle` 存在、裸 `.toggle` 只剩别处那一份）。**补前缀时必须跳过 `.settings-view` 自身**——写成 `.settings-view .settings-view` 会自嵌套永不匹配，症状是「右侧不能滚动、高度不是整屏」（高度/flex/overflow 全在那条规则上）；机械补前缀用 `^(?!\.settings-view|@|\s|\}|\/\*)` 这类负向断言既幂等又能一次补齐成片漏网的规则（初版搬迁脚本的 `split(/(?<=\})/)` 会被 `@keyframes` 里的嵌套 `}` 打乱，导致其后成片规则被跳过前缀化 —— 现已用批量补前缀修正，并拿原样式区逐一比对过类名，零遗漏）。② **不在 `.settings-view` 子树里的内容不能加前缀**——Teleport 到 body 的弹窗（`.data-move-card` / `.dm-*` / `.mask-*`，样式跟着 `DataPanel.vue`）与需要 `:deep()` 穿透的规则（`.quote-source`，跟着 `WorkbenchPanel.vue`）写进**对应面板的 `<style scoped>`**，靠 data-v 属性命中（Teleport 出去也带）。③ **跨面板跳转要容忍面板还没加载完**：`goToSection` / `jumpToSetting` 都是「找不到 DOM 就每 50ms 重试、最多 1s」，改这两个函数时别把重试去掉。新增设置项：标题用 `setting-name`（外观区那套用 `theme-label`，索引脚本两者都收），改完跑 `npm run gen:settings-index`。

51. **扩展截图（展示物料）链路：multipart 字段名必须带 `[]`、清单只追加字段、类型按文件头判。** 作者在发布弹窗选图（最多 5 张、单张 ≤ 2MB）→ 客户端随包 multipart 上传 → 服务端按**文件头**判类型（不信扩展名，实现见 `m-hub-server/src/modules/submissions/screenshots.ts`）→ 存提审暂存 → 审核通过时才搬进正式桶 `screenshots/<id>/<sha16>.<ext>`（内容哈希命名 = 不可变 = 可长缓存）→ 清单条目加 `screenshots: string[]`（**只追加字段、不抬 schemaVersion**，见约定 46；客户端 `MarketExtension.screenshots` 带 serde default，老清单 = 空数组，详情页显示「作者未提供截图」）。三个坑各记一条：① **multipart 的同名字段必须写成 `key[]`**——Hono 的 `parseBody()` 默认只保留同名键的**最后一个值**（`hono/dist/utils/body.js` 的 `shouldParseAllValues`），不带 `[]` 时传 6 张只到 1 张、而且**服务端不报错**（已由 `publisher.rs::tests::upload_sends_screenshots_with_array_field_name` 锁住这条契约）；② **上架前截图不能有公开地址**（只经 `GET /admin/submissions/:id/screenshots/:index` 给审核预览），公开 URL 在发布写清单时才生成；③ 作者选的图在任意目录、不在资产白名单里，`convertFileSrc` 会被拒，发布弹窗的缩略图走 `read_image_data_url` 命令（限 2MB + 仅 png/jpg/webp，**别扩成任意文件读取通道**）。

52. **平台服务端地址是内置常量、设置里不提供地址入口（`config::DEFAULT_SERVER_URL`，v0.5.6）：** 账号登录 / 平台 AI 额度 / 申请开发者 / 发布扩展 / **市场清单 / 升级清单**都打这个地址，真相源只有 `src-tauri/src/config.rs` 的 `DEFAULT_SERVER_URL`（v0.6.1 起为 `https://m-hub.xfactor.top`，无尾斜杠；`api_spec.rs::base_of` 仍做归一化兜底），唯一出口是 `account::server_url()`（publisher 经 `account::base_url()` 复用；市场/升级地址经 `config::market_registry_url()` / `update_manifest_url()` 拼同一常量）。三条：① **不要再从 `AppConfig.server_url` 读地址**——该字段已废弃、读到即忽略（旧 `app.json` 里可能残留开发期的临时联调地址，而地址入口一旦从设置里移除，用户就再也改不回来：症状是账号/额度/发布全部连不上、界面上无从自救）；`account_set_server` 命令与 `api/tauri.ts` 的封装已一并删除，**别"顺手"加回设置项或让该字段复活**（真需要可配置时，三样一起回：读取逻辑 + 命令 + 设置入口）。② 换地址只改常量一处，且**必须重新发版才生效**（老版本的 exe 里烧的是旧地址）。③ **HTTPS（v0.6.1 起）**：域名已于 2026-09-17 上 Let's Encrypt，常量已切 https。反代侧**http 与 https 双栈并存、不做跳转**（老客户端烘的是 `http://`，双栈 = 行为不变）；⚠️ **将来若要给 80 加 `return 301`，必须只对 GET**——reqwest 默认把 301 的 POST 降级成 GET 并丢 body，老客户端的登录/AI 会直接挂（实测过 `POST http://m-hub.xfactor.top/me` 回 301）。自查：`curl -s -o /dev/null -w '%{http_code}' -X POST http://m-hub.xfactor.top/me` 应为 **404**（方法放行）而非 301。改完地址的联调自查：`curl https://m-hub.xfactor.top/me` 应回 **401 `{"error":"unauthorized"}`**（路由存在、只是没带 token），回 404 才说明域名/前缀错；注意服务端是**两套前缀并存**——`get_me` 走根路径 `/me`，其余全走 `/api/v1/…`，按直觉把它"修正"成 `/api/v1/me` 会立刻 404（已实测）。

53. **账号登录的反馈必须「就地常驻」，不能只靠 toast（v0.5.6 实测）：** GitHub 登录由**服务端代调**（客户端 → `/api/v1/auth/github/device/start`，服务端自己给 GitHub 15s 超时），发起失败实测要 **10s 上下**才回话；而 `showToast` 只显示 2.2s —— 用户点完「开始登录」干等十几秒，期间界面只有按钮文案从「开始登录」变「正在发起…」，体感就是「点了没反应」（实测复现：服务端连不上 GitHub 时稳定 ~10.5s 回 502 `github_unavailable`，末尾那条 toast 一闪即过、用户完全没看见）。所以 `AccountPanel.vue` 的登录结果走 `loginNotice`（busy 灰 / warn 橙 / error 红，就地显示在按钮下方、**常驻到下一次操作**，原始 `CODE:` 文本放 `title` 备查），toast 只作辅助（成功/退出这类即时结果仍可用）。**判断标准：任何可能等 5 秒以上的操作，反馈都不能只挂在 2.2s 的 toast 上。** 附两条：① `GITHUB_UNAVAILABLE`（发起阶段 502）与「授权完成后的 500」都是**服务端出网问题**——`github.com`（换 device code / access token）与 `api.github.com`（取用户信息）是**两个域名**，一个通不代表另一个通，实测国内服务器两个都会间歇性失败（同一分钟三连测全挂、几分钟后三连测全通）。服务端侧的修法是 `m-hub-server` 的 `GITHUB_PROXY`（**注意：只设 `HTTPS_PROXY` 对 Node 22 的原生 fetch 完全无效**——`NODE_USE_ENV_PROXY` 是 Node 24 才有的，代理必须由代码挂 ProxyAgent，见该仓 `src/lib/github-login.ts` 的 `ghFetch`），客户端侧无解，界面文案必须点明「与你的网络无关」，否则用户只会反复自查自己的网络；② 邮箱验证码登录入口**已开启**（2026-09-18：服务端 SMTP 就绪、`POST /api/v1/auth/email/send` 正常发码；此前以 `EMAIL_LOGIN_ENABLED: boolean = false` + 模板 settings-index skip 标记临时隐藏，两处已一并移除，见 `AccountPanel.vue`）。⚠️ **skip 标记的字面文本不要出现在附近注释里** —— `scripts/gen-settings-index.mjs` 是按字面量计数的，注释里写一句「曾用 skip 标记隐藏」就会让它报「标记不成对」直接挡住 `npm run build`（实测踩过）。邮箱链路也走就地常驻反馈（`emailNotice`，与 `loginNotice` 共用 busy/warn/error 样式），并有两条服务端口径必须在界面上说清：**同一邮箱每小时最多 5 封**（撞了回 429 `rate_limited`，只写「发送过于频繁」用户只会立刻再点，越点越久 → `emailErrorText` 把上限写进文案）、**发送成功后 60s 重发冷却**（连点不但撞限流，还会把最新一封的验证码换掉，用户拿着已作废的那封输码）。

54. **扩展开发技能包（Skills）随客户端内置、装到外部 AI 助手目录（v0.6.x）：** 设置 →「扩展 → Skills」（`ExtensionsPanel` 内的 `SkillsSection.vue`，section id `sv-sec-skills`，紧跟在「扩展」分区之后）把内置的 `m-hub-extension` skill 一键安装到本机 AI 编码助手的 skills 根：自动探测 `~/.claude/skills`、`~/.agents/skills`、`~/.codex/skills`、`~/.zcode/skills`、`~/.workbuddy/skills`、豆包的 `%LOCALAPPDATA%\Doubao\User Data\Default\.doubao\agent_mode\workspace\.user_skills`（**只列已存在的、不代用户新建**）+ 自定义目录；**通用目录合并（v0.7.2）**：overview 按 canonicalize 后的物理路径去重——多个根指向同一目录（junction / 自定义目录撞车）只列一行，label 并入（如「DSH / 通用 Agent、ZCode」）；豆包目录分 `.skills`（内置，勿装）与 `.user_skills`（用户层，装这里）；卡片主按钮三态：有未装 → 「一键安装(N)」/ 有本客户端装但落后 → **「一键更新(N)」**（逐个 force 重装，外来同名目录不并入、仍需逐行确认）/ 全齐 → 「已全部安装」置灰；下面每行的「更新 / 卸载」按钮保留。三条铁律：① **单一真相源 = 仓库 `skills/m-hub-extension/`**，由 `src-tauri/build.rs` 扫描后生成 `EMBEDDED_SKILL_FILES`（`$OUT_DIR/skill_files.rs`）经 `include_bytes!` 烘焙进二进制——skill 增删文件只需重新构建，**禁止把 skill 内容另抄一份进 Rust**；② **「已安装 / 可更新」不落宿主状态文件**，靠目标目录里的 `.mhub-skill.json` 标记（记 `hash` = 全包 sha256 前 16 位）与当前二进制的内容哈希（`skills.rs::bundle_hash`）比对判定；用户手工拷进来的副本没有标记 → 识别为 `foreign`，覆盖前**必须二次确认**（`install_skill` 先回 `CONFLICT`，前端 `data-btn.confirm` 两段式确认后带 `force=true` 再来）；③ **写入用「临时目录 + 原子替换」**（先写 `.<dir>.new`，原目录改名 `.old`，成功再删；失败回滚），绝不半截覆盖用户目录。命令 4 个：`get_skill_overview` / `install_skill(path, force)` / `uninstall_skill` / `remove_skill_root`；自定义根登记在 `AppConfig.skill_roots`。包体代价约 79 KB（对 9.1 MB 的 exe ≈ 0.9%）。

55. **「我的扩展」是扩展中心的第三个标签页，且没有「开发者模式」开关（v0.6.x）：** 本机扩展源码目录的**增删**（`add_dev_extension` / `remove_dev_extension`）在 `ExtensionCenter.vue` 的「我的扩展」标签页——它和「装了什么扩展」是同一件事，放在扩展中心才找得到；设置 →「扩展」只留一句指路文案（`ExtensionsPanel` 里的「我的扩展」行 → `emit('open-extensions')` → `index.vue` 切 `activeView='extensions'`）。四条口径：① **列表真源是 `get_dev_mode_status`**（登记了哪些目录），**不是**已加载的扩展清单——目录不存在 / manifest 坏了 / 与已装扩展同 id 的条目也要照常显示，用户得能看到并移除它们；② **「已安装」标签页过滤掉 `source === 'dev'`**（这类不算已安装，与 `CONTEXT.md`「开发扩展不出现在已装清单」对齐），开发目录只在「我的扩展」里露出，右侧给「发布」（目录有效且已加载才出现）与「移除」；③ **没有开关，登记即加载**——`add_dev_extension` 落 `dev_extensions` 后**立即** `allow_directory` + 写开发注册表，`scan_extensions` / `apply_dev_extensions` / `dev_extensions_stamp` **一律不再看 `dev_mode_enabled`**（该配置字段与 `set_dev_mode_enabled` 命令已删除/废弃，`DevModeStatus.enabled` 恒为 `true` 只为旧前端兼容）。产品逻辑是「先在本机调试，觉得行了再走发布（发布需开发者认证）」——**任何文案都不得写成「开启开发者模式后才能添加/调试」**，也不要重新引入开关（评审记录见 ADR 0005「修订」段）；④ 设置项搜索索引随 `sv-sec-skills` 分区一起变化（`npm run gen:settings-index` 重新生成，勿手改）；「我的扩展」已不是设置分区，故索引里不再有它的子项。
56. **扩展相关的四个弹窗内边距必须一致（v0.6.x 修「市场详情比设置弹窗大一圈」）：** 扩展设置（`ExtensionSettingsDialog`，已安装与「我的扩展」两处共用）、扩展市场详情（`MarketDetailDialog`）、发布（`ExtensionPublishDialog`）都挂在全局 `.modal-card` 上，而**该外壳自带 `padding: 24px`**——只写头/体/脚的 padding 而不清零卡片自身，会变成「外壳 24px + 各自内边距」双份叠加（曾出现市场详情比设置弹窗四周各多 24px 的错位）。统一口径：**卡片自身 `padding: 0`，头 `16px 18px 0`、体 `14px 18px`、脚 `12px 18px`**（设置弹窗的体/脚因内部块间距略作 `0 18px` / `4px 18px 16px`，左右仍为 18px）。改任一处弹窗的 padding 前先对照其余三处；视觉核对可临时搭一个空白页把三个弹窗并排渲染后用无头浏览器截图（改完即删，勿留在仓库里）。
57. **市场详情弹窗的截图展示区：主图 + 缩略图切换 + 灯箱，无截图也要有交代（v0.6.x）：** 数据源是清单条目的 `screenshots: string[]`（链路见约定 51；**清单没带该字段 = 空数组**，线上多数扩展目前都没传过图）。展示区口径：有图时渲染 16:10 主图（`object-fit: contain`，不裁切）+ 多图时的缩略图条与左右箭头 + 计数角标，点主图开全屏灯箱（←/→ 翻页、Esc 关、点遮罩关）；单张图不显示翻页控件。**加载失败的 URL 必须剔出展示区**（`shotFailed` 集合 + `@error`），否则远端图失效时留一片破图；`shotIndex` 越界自动回落第 1 张。无图时**保留该分区并给一句空态文案**（而不是整块隐藏）——用户看不到任何图片时会以为功能坏了，而这其实是「作者没上传截图」，需要在发布弹窗里提醒作者补图（发布弹窗的截图入口见约定 51）。**⚠️ 空态判断必须写死 `v-if="!shots.length"`，绝不能挂 `v-else` 到「缩略图条」上**——缩略图条的条件是 `shots.length > 1`（单图不显示），`v-else` 于是会在「正好只有 1 张截图」时与主图**同时渲染**，表现就是「明明有截图却提示作者没上传」（已实际发生：calculator v0.1.1 只有 1 张图，用户报的就是这个）。教训：`v-if/v-else` 只在条件互为反命题时才成对使用；`> N` 与「其余」不互为反命题，必须各自写全条件。
58. **发布弹窗：同一扩展只允许一个待审版本，拦在打包之前且必须写明原因（v0.6.x）：** 产品口径——某扩展有未走完流程的提交时**不允许再提交新版本**，用户必须先撤回或等结果。实现要点三条：① **`blockReason` 常驻在按钮旁**，不是点击后的 toast、也不是纯置灰（置灰必须同时给出「哪个版本在拦路 + 去哪儿解」，并带「定位到这条记录」跳到「我的提交」里那一行并短暂高亮——那一行的撤回按钮就是出路）；② **阻塞状态集合必须与可撤回状态一致**（`uploaded` / `pending_review` / `gate_failed`，见 `withdraw` 的白名单）：能撤回的才拦，拦住也一定能靠自己解开，不会出现「被拦住又无从下手」；`gate_failed` 文案要区分开（提示「改完源码重新提交即可」，而不是「等审核」）；③ **拦截必须在打包之前**——`dev_submit` 是**先 `pack_to_temp` + multipart 上传整包、服务端 `checkDevQuota` 才回 429**（`publisher.rs` → `m-hub-server` `routes.ts`），撞上限制的代价是白等几分钟，所以客户端用列表 + 配额先判（`quota.drafts_remaining === 0` / `daily_submits_remaining === 0`；服务端只回剩余次数不回上限，其余情形交给服务端报错兜底），`submit()` 里还会再拉一次最新列表复核一次。
   **⚠️ 附带修掉的真 bug：该弹窗的初始化 `watch(visible, …)` 少了 `{ immediate: true }`** ——组件在「已经打开」的状态下首次挂载时（刚进扩展中心就点发布等）watch 不触发，提交记录 / 配额 / 预检**永远为空**，置灰逻辑也就永远不生效，用户必须关掉再开一次才正常。教训：**凡是「打开即需要的数据」都必须 `immediate: true`**（或改用 `onMounted` + watch 双入口），别假设父组件一定会经历 null → 有值的切换。
59. **发布弹窗的提交列表：展示分页，但判定必须基于全量（v0.6.x）：** 提交记录可能上百条，列表按「每页 20 条 + 显示更多」渲染（`visibleSubmissions` 切 `submissions`，`hasMore` 用条数比较，**不是**真去翻页拉数据）。但 `loadAllSubmissions()` 会**一次翻页到底**把所有记录拿进内存，原因：阻塞判定（约定 58）与「待处理明细」都要看**所有**未走完流程的提交，只看第 1 页会同时造成两个错误——「明明有待审版本却放行」和「配额数字与明细对不上」。全量拉取失败时降级为已加载的部分并置 `allLoaded=false`，明细面板会显式写明「可能不全，请刷新重试」，不假装完整。列表标题同时显示「共 N 条，显示 M 条」，避免用户以为没别的记录了。定位某条历史记录（明细/阻塞说明里点击）要**先把 `visibleCount` 放大到包含它**再 `nextTick` 滚动，否则目标还没渲染、`scrollIntoView` 找不到元素。
60. **「已下架」是客户端派生状态：服务端下架**不会**改 `submissions.status`（v0.6.x）：** 平台下架某版本只做两件事——写 `revoked` 表 + 重签市场清单（`m-hub-server` `routes.ts::/admin/market/:id/revoke`），**没有任何 `UPDATE submissions SET status`**。所以作者侧那条提交会永远停在 `published`，列表一直显示「已上架」，看着像没生效。客户端只能自己对照：发布弹窗打开时拉市场清单（先 `refresh_market_registry`，失败回退读本地缓存的 `get_market_registry`），取顶层 `revoked`（`id@version` 数组），命中 `${ext_id}@${version}` 的已上架记录渲染成 `delisted`（灰标签「已下架」+ 一行说明）。派生状态**不要**写回 `status`：`statusKey()` 只用于展示，撤回白名单等判断仍看真实 `status`。另外注意这个名额语义的副作用——`published_remaining` 按 `COUNT(DISTINCT ext_id) WHERE status='published'` 算，**被下架的扩展仍占一个「在架可新增」名额**（服务端口径如此），界面文案别把它说成「当前在架数量」。
61. **发布配额归「扩展管理页」，提交记录列表是全账号的（v0.6.x）：** 三件相关的事各有归属，改动时别混：① **配额展示放在扩展中心顶栏**（`ExtensionCenter.vue` 的 `.ec-quota`，右对齐一行小字，「归零」的那项标红加粗）——它是**账号级**的（不限来源、不限扩展），放在发布弹窗里既占地方又容易被误读成「这个扩展的配额」，所以弹窗里已不再显示配额数字；② **待处理明细留在发布弹窗**（提交列表下方的 `.pub-draft-hint` + `.pub-detail`）——它列的是占用「待处理」名额的记录，跟撤回入口在一起才有用；③ **提交记录列表按账号列出、但用标签区分是不是本扩展**（`.pub-item-other`「其他扩展 · <id>」）。为什么不做客户端过滤：服务端 `GET /dev/submissions` **不支持按 ext_id 过滤**（只有 paging + `WHERE user_id = ?`），而配额明细里可能就有别的扩展的记录——若把列表过滤成当前扩展，那些明细行点了会跳不到任何地方（`focusSubmission` 的兜底提示也救不了体验）。现在的口径是「列表完整 + 归属清晰」：本扩展的记录自然集中在最前（按时间倒序，版本迭代就是本扩展的连续记录），非本扩展的带淡色标签。`focusSubmission` 另有一条兜底：目标行确实不在当前渲染条数内时，先放大 `visibleCount` 再滚，仍找不到就落到列表头并写明原因，绝不静默无反应。
   **⚠️ 配额三个数字全是「剩余」，文案必须写成「还可…」，绝不能只写状态词（踩过）：** 服务端 `quotaView` 返回的是 `maxDrafts/maxPublished/maxDailySubmits - 已用`（默认上限 5 / 10 / 3，见 `m-hub-server/src/lib/config.ts` 与 `.env.example`），而服务端**只回剩余次数、不回上限**（PRD 附录 A：配额参数不进开源仓），所以界面上永远写不出「已用 3 / 共 5」这种自解释形式，标签自己就得把话说清。曾写成「发布配额：待处理 5」，用户读成「我有 5 条待处理」→「我明明只有 4 条提交，哪来的 5」，直接把歧义当成 bug 报回来。现在的口径：**扩展中心写「待处理还可 N 条 · 今日还可提交 N 次 · 在架还可新增 N 个」（带量词）+ `title` 注明「数字是还能用的额度」；发布弹窗写「提交记录共 N 条：其中 M 条未走完流程，占用账号的『待处理』额度」**——两处一个说剩余、一个说占用，方向相反，改文案时别互相抄。用户看到「待处理还可 5」且自己只有 4 条记录时，5 恰恰是「一条都没占用」。
62. **`runtime.openExternal` 用窄权限 `open-url`，三处口径必须同步（v0.6.7）：** v0.6.6 把 `openExternal` 从"无需权限"改成需要 `system`（封掉任何带 `network` 的扩展往默认浏览器弹任意页面钓鱼），但**只改了运行时能力表**——本地预检 `precheck.rs` 与服务端关卡 `gate.ts` 仍写「runtime 无需权限」，开发者文档也还写着「无需权限」，于是按文档写的扩展**本地预检过、线上关卡过、装上却点不动链接**，且前端只 `logClientError`、界面无任何提示（用户只能报「点了没反应」）。本版三件事一起做：① 权限**收窄为 `open-url`**（`system` 语义过宽，且要留给将来的 `system.*`；`system` 已从能力表移除，manifest 仍可声明但当前无任何能力由它门控）；② **三处口径同步**——运行时能力表 `mhub_api.rs` 的 `CAPABILITIES`、本地预检 `precheck.rs::permission_for`、服务端关卡 `gate.ts::permissionsUsedInSource`（`runtime.openExternal` 是**唯一按方法而非命名空间**判定的权限，三处都要特判）；③ `useExtensionFrame.ts` 的 `open-external` 分支失败时**必须给可见提示**（此前静默，用户看不到任何原因）。**教训：改「某能力需要什么权限」时必须同时改这三处 + 文档**（`skills/m-hub-extension/` 的 `bridge-api.md`/`manifest.md`/`pitfalls.md`/`debug-deploy.md`/`SKILL.md`/`mhub.d.ts`），只改运行时就是这次的坑。
63. **发布弹窗「发布版本」自动回写 manifest.json（v0.7.0）：** 发布弹窗新增「发布版本」输入框，预填「当前补丁号 +1」；`dev_submit` 新增可选参数 `new_version`，非空时在 `pack_to_temp` **之前**调 `publisher.rs::bump_manifest_in_dir` 把版本写回扩展的 `manifest.json`（必须严格大于当前版本，`market.rs::version_cmp` semver 语义；格式限 `x.y.z` 三段纯数字 + semver 解析，拒绝 prerelease/前导零）再打包——包内 manifest 带上新版本，服务端的版本递增关卡才认。四条实现约束：① **serde_json 必须保持 `preserve_order` feature**（Cargo.toml 有注释）——回写走「解析成 Value → 改顶层 version → to_string_pretty」，没有它 BTreeMap 会把键按字母重排，开发者的 manifest diff 一片狼藉；② 前后端校验同口径（`parseXyz`/`validateNewVersion` vs `validate_new_version`），前端就地报错，Rust 是权威兜底；③ 提交成功后前端用 `result.version` 就地更新 `currentVersion` 并预填下一版（props.extension 要等列表 5s stamp 轮询才刷新，连续提交不能拿旧版本比）——**留空 = 按 manifest 当前版本发布**，这是「关卡挂了重提同一版」的合法路径，别把留空当异常；④ 回写用「`.` 开头临时文件 + rename」原子落盘——`pack_dir_to_archive` 排除 `.` 开头项、`dev_extensions_stamp` 哈希也跳过它们，写一半崩溃既不会把半截文件打进包也不触发热重载。
64. **速达「应用内打开网页」= 面板 child webview + 窗口池，全部启动期预创建（ADR 0011，2026-09-25 实施）：** `suda_browser.rs` 持有全部 webview：主窗内嵌面板（`suda-panel`，main 窗的 child webview）+ 独立窗池 4 扇（`suda-web-0..3`，每扇 `WindowBuilder` + 两个 child webview：content **先建垫底**、chrome **后建盖顶**——chrome 高度变化到 content 边界跟随之间有交叠，顶栏盖内容比内容盖顶栏观感好）。**铁律延续**：运行期只 show/hide/navigate/set_bounds/eval，零 build/destroy；tauri 需 `unstable` 特性（`Window::add_child`/`Webview::set_bounds` 门控其后）。六条口径：① **面板边界由前端上报**——SudaWebPanel 挂载/ResizeObserver 按 `getBoundingClientRect()`（逻辑 px）调 `suda_panel_show/bounds`，Rust 不感知 DOM 布局；独立窗则由 Rust 自算（窗口 inner_size − chrome 高度），chrome 页挂载后实测上报高度（`suda_browser_chrome_height`）。② **权限隔离**：`suda-panel` 与 `suda-web-*-content` 故意**不进** capabilities（外站页面零 IPC），只有 chrome SPA（`suda-web-*` 通配命中）能 invoke。③ **`ADDITIONAL_BROWSER_ARGS` 必须带给每个新 webview**——同一 user data folder 下参数不一致的环境创建会失败（lib.rs 常量注释）。④ **tab 真源在 Rust `SLOTS`**（Mutex，锁内只做内存读写、窗口操作一律锁外——窗口事件回调用同一把锁）；chrome 页挂载时 `suda_browser_state` 拉初态（隐藏窗不跑脚本、显示前的事件不补发，clipboard-shown 同款约定）。⑤ **wry 新窗口请求必须宿主接管**：本工程所有外站 webview 都注册了 `on_new_window`（Deny + 事件交宿主——面板原地导航、独立窗落新 tab），不注册则 target=_blank 全静默失效（extension.rs 踩过的坑）；接管**不在回调里直接 navigate**（重入风险），经事件→命令绕一圈。⑥ content webview 要 `disable_drag_drop_handler()`（给外站页面留原生 DnD）；chrome 页不需要。store.launchResource 按 `suda_web_open_mode`（panel 默认/window，设置 → 功能 → 速达）分流网页条目，panel 模式经 `suda-open-web-panel` CustomEvent 交 index.vue 切视图（store 不持有视图状态）；离开面板视图必须 `suda_panel_hide`（index.vue watch + 组件 onBeforeUnmount 双保险）。⑦ **凡按 label 分发/识别 chrome 页，必须用 webview label（`getCurrentWebview().label`）**：chrome 是池窗口的**子 webview**，`getCurrentWindow().label` 在其中返回的是**窗口** label（`suda-web-0`）而非 webview label（`suda-web-0-chrome`）——App.vue 的路由正则匹配不上就落到兜底，把整个工作台渲染进浏览器窗口（实测踩过：独立窗口打开显示的是工作台面板；BrowserChrome.vue 内部本来就用的 webview label，唯独 App.vue 分发层用错）。单 webview 窗口两类 label 恒等，App.vue 统一改用 webview label 无副作用。⑧ **池窗口两个子 webview 的边界都必须由 Rust 维护**（`apply_content_bounds`：chrome = 顶部条 0,0 ~ 全宽×chrome 高度、content = 顶部条以下，窗口 Resized 与 `suda_browser_chrome_height` 时重算）——此前只算 content 的边界、chrome 从不收缩，它不透明的页面底色盖死下方真正加载网页的 content（实测踩过：独立窗口打开「顶栏 + 一片空白」）；chrome 宽度必须随窗口 inner_size 实时取，否则窗口拉伸后顶栏不跟随。两处配套：**chrome 创建时即给顶部条尺寸**（不是全窗，否则首次显示到高度上报落地之间白闪遮挡 content）；**BrowserChrome 根元素必须 `height: auto`**——100vh 会让高度上报变成循环测量（首次量到全窗高 → clamp 收缩 webview → 100vh 跟着变 → 再上报新值），平衡在 clamp 上限，工具栏下永远多一段应用底色空白（实测踩过）。
65. **速达小类 = 大类下单归属（ADR 0012，2026-09-25 实施）：** `resource_subcategories` 表（`UNIQUE(kind, name)`，各大类一套允许同名不同义；文件大类 7 内置 category 经建表 `INSERT OR IGNORE` 种子并入，老库新库同口径只补缺）。五条口径：① `resources.category` 语义 = **所属大类小类库中的一个小类名**（不引入外键，rename/delete 在 repo 层单事务级联 `UPDATE resources`），NULL =「未归类」且**存量不回填**；② **新建未指定小类 → 自动归默认**在 `repo::resource::create` 统一处理（默认 = is_default 优先、否则排序最前；大类还没有小类则保持 NULL）——扩展桥 `data.resources.create` 同样生效；编辑显式传 NULL = 未归类（不触发自动归默认），两条语义靠命令层区分；③ 删除小类：条目批量改挂默认（删的是默认时排序最前剩余者**晋升为默认**），大类删空回未归类；④ 大类的第一个小类创建时自动成为默认；⑤ 管理入口在 设置 → 功能 → 速达（`sv-sec-suda` + `SudaSubcategoryManager.vue`：行内改名/指针拖拽排序（主窗禁 HTML5 DnD，同约定 14）/星标设默认/两段式删除确认），速达页只有小类筛选通栏（应用/网页/文件通用 + 「未归类」恒显示）与表单小类选择（编辑态含「未归类」项）。测试：`repo::subcategory::tests` 6 例（种子幂等/首类默认/自动归属/改名级联/删除改挂与晋升/排序默认切换）。
66. **主窗句柄必须走 `crate::main_window`，`get_webview_window("main")` 对多 webview 主窗恒为 None（2026-09-25 实测）：** 约定 64 的面板 `add_child` 完成那一刻，主窗在 tauri 里就不再是「webview window」——`Manager::get_webview_window` 内部校验 `Window::is_webview_window()`（**窗口上所有 webview 的 label 必须都等于窗口 label**），多 webview 窗口一律返回 None，而窗口本身活得好好的（`get_window("main")` 按窗口注册表查 label，不受 webview 数量影响）。症状是成片静默失效：全局快捷键/托盘/悬浮球的显隐 toggle 打「未找到主窗口」、`CloseRequested` 拦截注册不上 → **Alt+F4 真关窗**、几何保存失效、四个浮窗初始定位退化为系统级联位、剪贴板 `is_main_window` 恒 false（主窗粘贴回填走错分支）、`hide_to_tray` 无条件打「窗口隐藏至托盘」假成功日志（已改为仅在真隐藏成功时打 INFO，失败由 `tray::hide_window` 落 WARN）。修复 = 全部查找统一走 `lib.rs::main_window(app)`，**今后新增任何拿主窗的代码禁止再写 `get_webview_window("main")`**。注意一处 API 差异：`WebviewWindow::set_background_color` 同时铺窗口与 webview 两侧底色，`Window` 版只铺窗口侧——lib.rs 启动底色处补了一次 `get_webview("main")` 的 webview 侧铺底（按 label 查 webview 表，多 webview 不受影响）。**同一校验还打在命令参数注入上**：`window: tauri::WebviewWindow` 参数在命令里经 `CommandArg::from_command` 解析时同样走 `is_webview_window()`，多 webview 主窗的调用直接 InvokeError（"current webview is not a WebviewWindow"）——实测踩过：主窗的最小化/最大化/置顶全部失效。`minimize_window`/`toggle_maximize`/`set_window_always_on_top` 已改为 `webview: tauri::Webview` + `webview.window()`（`Webview`/`Window` 两种注入永不失败）；**今后新增命令参数一律用 `Webview`/`Window`，禁止 `WebviewWindow`**。同场修复：NoteEditor.vue `detachImageListeners` 声明移到 immediate watch 之前（约定 38-② 同款 TDZ，源码/分屏模式首挂载经 `destroyEditor` 触发）。
67. **Windows 上 `cargo test` 依赖 build.rs 的链接器嵌清单，别改回 `tauri_build::build()`（2026-09-25）：** tauri-build 默认只给 bin 经 RC 资源嵌应用清单（Common-Controls v6），cargo 的 lib 测试 exe 不带清单 → 加载 comctl32 v5 → 缺 muda（tauri 菜单库）导入的 v6 独有函数 `TaskDialogIndirect` → 测试进程启动即 `0xc0000139 STATUS_ENTRYPOINT_NOT_FOUND`，265 个单元测试一个都跑不了（干净树上同样复现，与代码改动无关）。且清单嵌法没有「仅测试」作用域可用：`rustc-link-arg-tests` 不作用于 lib 单元测试 exe（且包无集成测试目标时 cargo 直接拒绝该指令）；总体 `rustc-link-arg` 与 RC 清单并存 → CVT1100 重资源、bin 链接失败。最终口径：build.rs 用 `tauri_build::try_build(Attributes::new().windows_attributes(WindowsAttributes::new_without_app_manifest()))`（不让 tauri-build 嵌），再统一 `rustc-link-arg=/MANIFEST:EMBED` + `/MANIFESTINPUT:<OUT_DIR>/app-manifest.xml`（内容 = tauri 默认清单逐字拷贝，bin 行为不变，测试 exe 同源获得 v6）。**改回默认嵌法 = cargo test 重新全灭，且完全静默**（链接层问题，编译与类型检查全绿）。同场：日志时间戳由插件默认 UTC 改为本地时间（lib.rs 自定义 format，布局不变）——日志里出现 UTC 旧行与新本地行混排属正常。

68. **capability 的 `windows` 字段匹配的是窗口 label，多 webview 窗口的子 webview 必须用 `webviews` 字段（2026-09-26 实测）：** Tauri v2 ACL 的 `resolve_access` 按「webview label 匹配 capability 的 `webviews` 模式 ∨ **窗口** label 匹配 `windows` 模式」放行插件命令（应用自身的 `#[tauri::command]` 不经此门）。速达独立浏览器是唯一的多 webview 窗口（window `suda-web-0` + 子 webview `suda-web-0-chrome`/`-content`），曾把 `suda-web-*-chrome` 写进 `windows` 数组——它永远匹配不上窗口 label `suda-web-0`，而 capability 又没声明 `webviews`，结果 chrome 页的 `plugin:event|listen` 被 ACL 拒绝：`listen()` reject → `onMounted` 在第一个 `await listen` 处静默中断 → 后续监听全部没注册 → chrome 页永远收不到 tab/地址栏同步事件，独立窗口打开网页永远显示「此窗口当前没有打开的页面」空态（挂载时的 `suda_browser_state` 拉取能成功是因为应用命令不走 ACL，反而把症状捂严实了：只有 chrome 页恰好晚于首次打开挂载时才会被拉取掩盖成「正常」）。修复：capability 拆成 `windows`（原样，不含 suda）+ `"webviews": ["suda-web-*-chrome"]`——content 子 webview 两边都匹配不上，维持「外站页面零 IPC」铁律（约定 44 的 capabilities 口径）。**今后给多 webview 窗口的子 webview 配权限一律用 `webviews` 字段**；症状自查：怀疑权限问题时在目标 webview 里 `listen('x', () => {})` 看是否报 `not allowed on window ...`（错误信息里「allowed on」列表会把 capability 的 windows 模式全列出来，对照窗口 label 一眼定位）。改动 capability 后 `tauri dev` 会自动重编译重启（capability 编译期烘进二进制）。

69. **主窗口圆角：窗口透明 + 页面底色在 `.app-shell` 上 + 全出血层一律同款圆角（构建期有守卫）：**
    主窗本体是**透明**的（`tauri.conf.json` 的 `transparent: true` + `decorations: false`），
    圆角靠 `index.vue` 的 `.app-shell` 裁出来。三处缺一不可：
    ① **窗口透明**（否则圆角处漏出窗口的方底色）；② **页面底色在 `.app-shell` 上而不是 `body`**
    （`body` 铺满视口、是矩形，留在那里圆角就白做了）；③ **`.app-shell` 上有 `contain: paint`**
    —— 壁纸层是 `position: fixed; inset: 0`，fixed 元素默认相对**视口**定位，
    **不会被祖先的 `overflow` + `border-radius` 裁剪**；不加 `contain: paint`
    就是「内容圆了、壁纸还是方的」，圆角处露出桌面。
    ⚠️ **第四条：任何全出血层（遮罩 / 灯箱 / 拖拽遮罩）都必须自己声明
    `border-radius: var(--window-radius)`** —— 它们 Teleport 到 `body`、
    绕开了 `.app-shell`，开弹窗那一帧会用方形遮罩把圆角盖回去，表现为「窗口方了一下又圆回来」。
    **这条编译期完全看不出来、类型检查全绿，只能实机看见**，故有构建期守卫
    `scripts/check-rounded-window.mjs`（`npm run build` 的 prebuild 会跑，违规直接失败）。
    小而定位的下拉 / 气泡 / tooltip **不受此限**（它们内缩，永远碰不到窗口边缘）。
    ⚠️ **第五条（2026-09-29 加、同日又作废，见下方「阴影」段）：外扩带存在期间，
    `inset: 0` 一度从「正确」变成「错」** —— 内容圆角落在**窗口边缘往里 32px** 处，
    而 `inset: 0` 的遮罩圆角落在窗口角，四条边各内缩出 32px 的**方形遮罩**盖在
    透明带上（实测：遮罩 1000×700@(0,0) vs 内容 936×636@(32,32)）。
    当时把四个全出血层改成 `inset: var(--window-shadow-margin, 0px)` 解决。
    **外扩带现已整体移除**（理由见下），故这 15 处**全部改回 `inset: 0`**，
    守卫的 `inset` 判定也**只认 `inset: 0`** 一种 —— 故意不认那个 token：
    一旦有人又用上，本守卫应当把它当「铺满但未声明圆角」处理，而不是默默放过
    （「守卫不报错」≠「没问题」）。
    圆角值是独立 token `--window-radius`（**16px**，与 `--radius-lg` 同值，让「窗」与
    「窗里的卡」属于同一套圆角语言）；启动欢迎页 `#boot-splash::before` 也要同款圆角，
    否则启动瞬间有一次形状跳变（它硬编码 16px，由 `check-window-margin.mjs` 锁）。

    **阴影：外扩带已于 2026-09-29 整体移除，窗口现在没有阴影**（结论反转，务必读完再改）：
    - **当初为什么有**：实测 AppKit 对 `opaque = false` 的窗口**一律不画系统阴影** ——
      窗口边缘外 70px→0px 的桌面像素完全平坦（Δ≤2，纯 PNG 噪声）；显式
      `NSWindow::setHasShadow(true)` 也**证伪**（无任何变化，根因与 `hasShadow` 属性无关）。
      而 CSS 阴影只能画在窗口**以内**，于是只能「窗口比可视区大一圈 32px 透明带」，
      用 `.app-shell` 的 `margin` + `box-shadow` 自绘。
    - **为什么移除**：那圈带子是**透明**的 —— 桌面的壁纸与图标会从窗口四周**直接透进来**，
      形成一圈明显的「玻璃框」（用户实测：在彩色壁纸上尤其刺眼，就是这一圈）。
    - **结论一句话**：**「有阴影」与「不漏桌面」在透明窗下二选一**。现取后者：
      内容铺满整窗、圆角 16px、**没有阴影**。想要阴影，唯一干净的路是把窗口改成
      opaque（不透明）并接受直角 —— 那是设计取舍，不该由「画得出来但会露桌面」的
      折中方案来承担。
    - **随之消失的机制**（都别再引用）：`--window-shadow-margin`、
      `WINDOW_SHADOW_MARGIN`、`to_window_size()` / `to_visible_size()`、
      `window_geometry_tests`、`.app-shell` 的 margin/高度扣减/`box-shadow`、
      全出血层的 `inset: var(--window-shadow-margin, 0px)`（15 处，已全部回到 `inset: 0`）、
      resize 手柄为「对准内容边」而加的偏移。
    - **窗口尺寸语义变了**：`WindowState.width/height` 与窗口 inner 尺寸**现在是同一个东西**。
      落盘直接存 `inner_size()`、恢复直接 `set_size()`，不再有任何换算。
      `tauri.conf.json` 的 `width/height/minWidth/minHeight`（1400/900/1000/700）
      必须与 `config.rs::WindowState::default` **相等**（以前是 `+2×外扩带`）。
      老配置存的是「可视区」语义，与现在一致，**无需迁移**。
    - **最大化**：`html[data-window-maximized]` 现在只把 `--window-radius` 归零
      （铺满屏幕就该是直角，否则四角露出桌面色缺口）。
      唯一写方仍是 `TitleBar.vue` 的 `refreshMaximized()`；**Rust 侧刻意不再 eval 一份** ——
      双写方会出现状态不一致的窗口期，且启动期那次 eval 会打在加载中的空白文档上。
    - **边缘点击**：外扩带曾是「看得见的圆角内容边缘与窗口边缘之间的死区」，
      故 `WindowResizeHandles.vue` 用 8 向隐形手柄把窗口边缘变成缩放热区 ——
      这条**与外扩带无关、仍然必需**（无边框窗口在 macOS 上没有系统缩放边）。
      现在手柄直接贴窗口边缘（偏移已删）。它必须由 index.vue 作为 `.app-shell` 的
      **兄弟**渲染 —— 壳上有 `contain: paint`，放壳内会被自己的裁切剪掉。
      最大化态隐藏（窗口就是屏幕四边，与系统 zoom 打架）。

    **「外扩带不许回来」由 `scripts/check-window-margin.mjs` 守住**（prebuild 跑，语义已反转）：
    ① `style.css` 不得再声明 `--window-shadow-margin` ② `lib.rs` 不得再有
    `WINDOW_SHADOW_MARGIN` ③ `tauri.conf.json` 的四项尺寸必须**等于**
    `config.rs::WindowState::default` ④ `index.html` 欢迎页 `inset` 须为 0、
    圆角须与 `--window-radius` 一致。**四条都验过负例。**
    配套 `scripts/check-window-min-size.mjs` 锁 `MIN_INNER_W/H` ⇄ `minWidth/minHeight`
    （tauri 只有 setter 没有 getter，Rust 侧只能写镜像），并拒绝 `minWidth > width`
    这种「一创建就小于自己 minSize」的配错。

70. **macOS 移植的七个静默失效点（2026-09-29 全面排查后一次修完；判据见 P9）：**
    共同形态是**「上游换了数据源，下游的匹配代码没跟着换」**。单测全绿、
    类型检查全绿、构建全绿，只有实机看得见。
    - **窗口完全无法缩放**（最严重，全应用级）。tao 在 macOS 上
      `drag_resize_window` 恒返回 `NotSupported`（`macos/window.rs:963`，
      参数 `_direction` 连读都没读），且无边框窗口用的是
      `Borderless | Resizable`（同文件 `:217`）而 **AppKit 的 borderless 窗口
      没有系统缩放边**。两个原因叠加 = 拖哪儿都不管用。改由
      `window_resize.rs` 自己实现（轮询 `mac::cursor_physical()`，手法同悬浮球）。
      **别再写 `startResizeDragging`** —— 它在 mac 上是最坏的一种状态：
      光标亮起 resize 图标、拖下去什么都不发生，比没有手柄更糟。
    - **剪贴板焦点归还恒失效**。`frontmost_bundle_id` 原先取 `bundleURL` 的
      末段去 `.app`（得到包名 `Safari`），而 `SELF_BUNDLE_ID` 是真 bundle id
      `com.mhub.desktop`，`activate_app` 走的
      `runningApplicationsWithBundleIdentifier:` **只认 id** → 恒返回空。
      已改用 `NSRunningApplication.bundleIdentifier()`。
      ⚠️ **展示用包名、判定/激活用 bundle id，两条路不能互相推导**，
      故 `frontmost_bundle_id` / `frontmost_app_name` 是两个函数。
    - **速达「运行中」绿点一个都不亮**。前端把 `target` 切末段去比进程名，
      而 macOS 的 `target` 是 `.app` **包路径**（`wechat.app`），进程名是 `WeChat`
      （VS Code 更是 `Code Helper`）。改成后端把「可能匹配上的键」
      （进程名 + 包路径 + 包名 + 本地化名）一起回传，前端只做集合查表。
    - **`.app` 提权启动必然失败**：macOS 的「程序」是包（目录），
      `do shell script` 拿目录去 exec 报 126。`launch_elevated` 原先漏了
      `open -a` 分支（`launch_program` 早就有）→ 用户为每个应用白输一次密码。
    - **「无格式粘贴」选项是装饰品**：配置存 `ctrl_shift_v`，而 macOS 分支的
      `match` 只认 `cmd_*`，于是 `ctrl_shift_v` 与 `ctrl_v` **双双落进 `_ => "cmd_v"`**。
    - **「本地程序」文件选择器在 macOS 上一个 `.app` 都选不了**：
      过滤器写死 `extensions: ['exe','lnk']`，而 `.app` 是包、没有可匹配的扩展名。
      同一功能的拖拽路径却是对的（`looksLikeApp` 已含 `.app`）—— 两入口自相矛盾。
      同处 `.icns` 也没放行，而**后端已实现 icns→PNG 转换**，能力做了入口给挡住了。
    - **剪贴板浮层收不到 Esc**：无激活显示的代价。Windows 用
      `RegisterHotKey(VK_ESCAPE)` 兜底，macOS 无对应 API。
      已用 `CGEventTap`（`Session` 而非 `HID`：后者要辅助功能权限，
      为「能按 Esc」额外索权不划算）实现，并**吞掉**该次 Esc 以对齐 Windows。
      ⚠️ **凡是收起浮层的路径都必须 `stop_esc_watch()`** —— 漏一条就留下一个
      还在吞 Esc 的 tap。最容易漏的是「粘贴后收起」（不走 `hide_overlay`）：
      症状是「按一次 ⌃⌘V 粘贴之后 Esc 永久失灵」，且无任何报错。

71. **快捷键的「显示」只有一个出口，且默认值有两份要锁**（构建期双守卫）：
    - `platform.ts::prettyShortcut` 是内部写法 → 显示文本的唯一转换器；
      `shortcutLabel(key, configured)` 是它的平台版快捷封装（用户改过就用改后的）。
    - **文案里绝不能出现裸的 `Ctrl+…` / 直接渲染 `config.xxx_shortcut`** ——
      前者在 macOS 上给出错的键，后者把字面量 `CommandOrControl+K` 怼到用户眼前
      （用户刚用 ⌘K 唤起搜索，弹窗里却写着 `CommandOrControl+K`）。
      守卫 `scripts/check-shortcut-text.mjs`：扫 `.vue` 的 `<template>`，
      查**静态文本节点与静态属性值**里的裸 `Ctrl+`。**属性绑定内的豁免**
      （`prettyShortcut(cfg, isMac ? '⌘K' : 'Ctrl+K')` 是正确的平台兜底分支）。
    - 四个默认值有两份拷贝：`shortcut.rs` 的 `DEFAULT_*`（真相源）与
      `platform.ts` 的 `DEFAULT_SHORTCUT_VARIANTS`（显示用镜像）。
      守卫 `scripts/check-default-shortcuts.mjs` 锁死。⚠️ Rust 那边是
      `#[cfg]` **分叉**的、两平台**允许不同**（剪贴板 mac `⌃⌘V` / win `Ctrl+\``），
      故 TS 镜像**每个键存 mac / other 两个分支**，两侧都要对上 ——
      `other` 分支只在该平台被读到，开发机上验不出来，所以更要锁。
72. **凡是「删掉旧的东西 / 换上新的东西」的操作，必须是「先落新、再原子换、最后才删旧」**（2026-09-29 一次数据丢失排查的总结）：
    - 三处独立的同款缺陷，症状都是**静默的不可逆数据丢失**：
      ① `apply_pending_restore` 原来是「先删正式库 → copy → 删备份」，且每步 `let _ =` 吞错
      —— copy 失败时原库已删、备份也删、什么都不剩，日志还写「已应用待恢复的数据」；
      ② 我自己写的 `resources_old` 恢复里 `DROP TABLE` 放在 `if/else` **外面**，
      并回因列不匹配失败时旧表连同数据一起被删 —— 从「能救」变成「彻底没了」；
      ③ 剪贴板清理的 `doomed` 图片列表在**事务提交前**就 unlink，回滚后留下
      「记录还在、图片没了」的行，且记录还在 → 后续清理不会再删该孤儿文件，永久残留。
    - 正确形状：**复制到暂存名 → 校验内容真的有效 → 同目录 `rename` 原子替换 → 才删旧**。
      Unix 的 `rename(2)` 本身即原子替换，**不需要先删目标**。
      任何一步失败都完整保留「原件 + 暂存 + 标记」，让下次启动自然重试 ——
      恢复要么完整成功，要么完全没发生，**没有中间态**。
    - 校验那步不能省：`copy` 返回 `Ok` **不等于**内容有效（磁盘写满时会只写一半）。
      不校验就替换，用户会拿到一个打不开的库，而原库已被覆盖。
    - 涉及**跨设备/跨目录的产物**（备份 zip）时，存绝对路径的列要写成可移植形式
      （本工程用 `mhub-rel://` 前缀，打包时写、解包时按**当前**根还原）。
      「不改 schema、只在传输格式里改写」比「把库里全改成相对路径」安全一个量级。
      判据：**这个产物换台机器还能用吗**。
73. **测试必须先证明自己会失败，才算数**（2026-09-29 连续踩中三次）：
    - 三次都是「测试绿了但它守的东西根本不会被它抓到」：
      ① 剪贴板「图片必须在 commit 后删」—— 我把 unlink 挪到 commit 前重跑，用例**照样通过**
      （成功路径结束时文件都没了，测不出顺序差别）；
      ② 备份路径改写只测了辅助函数 `rewrite_db_paths`，把生产代码里那句调用删掉**照样全绿**；
      ③ `resources_old` 恢复里把 `DROP` 改成无条件执行，**照样通过**。
    - 结论：**每加一条守卫，都要先把它守的东西破坏掉、确认它变红**，再确认修回去。
      写完就绿不代表它是守卫，只代表它没报错。
    - 补测的顺序是：先想「这段代码出错了会怎样」，再把那个错误**真的制造出来**。
      制造不出来（如「commit 失败」需要故障注入）就在注释里**写明这条验不到什么**，
      别留一条虚假守卫 —— 它比没有更糟，因为会让人以为该处已被覆盖。
      真正能造出来的优先造：③ 就是靠「故意让并回因缺列而失败」才补上的。
    - 造不出用例的场合，退一步把**调用点**做成可测：② 里 `backup_data` 其实不需要
      Tauri handle，于是抽出 `make_backup(conn, target)` 让测试跑生产路径本身。
74. **「只在 macOS 显形」的 bug 有一个共同形状：数据源换了口径，匹配的代码没跟着换**（2026-09-29）：
    - 这批修复大多是这个形状，且都能在 Windows 上量不出来（那边 scale 恒为 1、
      扩展名是 `.exe`、有系统热键与系统缩放边框）：
      · `outer_position()` 返回 `PhysicalPosition`，落盘存原值、恢复用 `LogicalPosition`
        → Retina 上主窗位置**每次重启翻倍**，第 4~5 次飞出屏幕且无法拖回；
      · 三个浮窗把主窗**物理**坐标与浮窗**逻辑**尺寸相加，结果喂给
        `WindowBuilder::position()`（文档明写收逻辑像素）；同一份代码里 `chat_window`
        那份是对的 —— 正确写法一直在工程里，只是没人抄；
      · `window_resize` 全程用**物理** px，而 `minWidth` 是**逻辑** px，不乘 scale
        则 Retina 上算出的下限只有真实值的一半。
    - **修法优先「消灭重复」而不是「各打一个补丁」**：那三份浮窗落点是**三份逐字重复**，
      抽成唯一的 `lib.rs::centered_on_main` 让四处共用，比补三处更能防将来再漂。
    - 反向自查：`grep` 一下所有**坐标/尺寸的取用点**，逐个确认单位；
      `Physical*` 与 `Logical*` 混用编译器不报错、`clamp` 也不报错，
      只在非 1.0 的 scale 下错。
    - ⚠️ 还有一处**同源但需实机确认**：`mac::cursor_physical()` 用「光标所在屏」的
      backing scale，而 tao 的 `outer_position()` 用「窗口所在屏」的 scale ——
      混合 DPI 多显示器下两者不在同一坐标系。修法是「几何比较全用全局点空间，
      只在最终 `set_position` 时换算」（`clipboard.rs::anchor_position` 已是这个范式）。
75. **改 CSS 注释是高风险编辑，必须机械校验配平**（2026-09-29 一天内踩了两次）：
    - 两次都是**漏写收尾符**，后果都是「后面一段规则被整段当成注释吃掉」：
      ① `style.css` 的 `html[data-window-maximized]` 被吞 → **最大化时圆角不再归零**，
         构建是绿的、功能少了一块；② `index/index.vue` 的 `.app-shell` 被吞 →
         构建报 `CssSyntaxError: Missing opening {`。
    - 为什么危险：**症状离原因很远**（少一个收尾符，表现为「圆角没归零」），
      **而且常常是静默的**（解析器不会告诉你「你少写了一个收尾符」，
      它只是把后面吃掉）。本工程 CSS 注释密度极高（几乎每个 token 都带「为什么」），
      所以「编辑注释」是高频操作，不能靠人眼。
    - 守卫 `scripts/check-css-comment-balance.mjs`（prebuild 跑）：扫全部 `.css`、
      `index.html`、以及每个 `.vue` 的 `<style>` 块，做字符级配平检查并**指出行号**。
      两种真实故障都用删除收尾符的方式复现过，均能拦下。
    - ⚠️ **写这个守卫时它自己先犯了同一个错**：文档注释里写了注释符号的字面形式，
      当场把脚本自己的注释提前闭合、脚本根本跑不起来。
      写这类说明时，**不要在注释里出现该符号的字面形式**。
    - 同类推论：改动 CSS 后，除了问「构建是否绿」，还要问
      **「我有没有让某条规则凭空消失」** —— 绿不等于没坏。


---

76. **速达小类的「层级」是 `name` 里的全路径，不是 `parent_id` 列（2026-10-04）：**
    `resource_subcategories.name` 直接存全路径（`开发`、`开发/前端`），段间用 `/`。
    四个理由：① **零迁移**——老数据名里没有 `/`，天然是顶层；新增 `parent_id` 还得回填，
    而回填错了表现为「条目挂到不存在的父级下」，比不改更糟。② `resources.category`
    语义一个字没改（十几处读它的地方不必动，而改错任一处都是「条目从所有视图消失」）。
    ③ `UNIQUE(kind, name)` 继续有效。④ 筛选/统计/扩展桥全不用改。
    代价：**一个层级段里不能含 `/`**（`validate_segment` 拦），否则路径本身歧义化。
    三条必须记住的语义：**筛选某层 = 含全部后代**（`subcategoryContains`，按段比不是
    `startsWith`——`开发` 不是 `开发者` 的祖先）；改名只改自己那一段、整棵子树跟着换父
    （新路径由 Rust 的 `reparent` 算，前端不拼）；删除连子树一起删、子树里的条目一并改挂
    默认小类。判定真源是 `repo::subcategory::tree`，前端那份 `utils/subcategoryPath.ts`
    只做展示，两边各有用例守着对应关系。
    ⚠️ 子级**不给拖拽把手**：拖拽写的是全组顺序，让它跨父级就是「改挂」，那是独立功能。
77. **`ERROR_CODE: 说明` 是给日志的形态，界面不显示代码（2026-10-04）：**
    Rust 侧 `account.rs::api_error` 统一产出 `CODE: 说明`。界面上要让**人读**的那一句由
    `utils/errorText.ts` 出（剥前缀 + 给已知码补「下一步」），原始串留给 `title` 备查。
    已知码表**按服务端实际发出的全集**建（`grep "fail(<状态>, '<码>'"`），不要凭印象补。
    两条容易写错的：① `Error` 实例必须取 `.message`（`String(e)` 会带出 `Error: ` 前缀，
    与要消灭的错误代码是同一种毛病）；② 剥前缀的正则只认**全大写**（`^[A-Z][A-Z0-9_]*:`），
    放宽成 `\w+:` 之后「注意：点这里」会被削成「点这里」，界面上凭空少一句。
78. **「作者明明做了 X，界面上却像没做」要先问：那一环有没有把字节/值留下来（2026-10-04）：**
    发布弹窗的截图区从上线起就是装饰品 —— 客户端 multipart 上传、服务端校验大小与数量、
    **然后把字节丢掉**；市场清单里的 `screenshots` 一直读的是扩展 manifest 里作者手写的
    URL。于是「作者选了 5 张图」与服务端回的 `screenshots: 5` **都是真的**，那 5 个字节却
    从来没有被存下、也没有任何地方读得到它们。同形状的毛病还有两个：只加字段不填值
    （运行期 `buildRegistry` 一开始压根没输出 `author`，而客户端类型与详情页那一栏一直存在）、
    只有展示没有判定。
    **判据：链路上每一环都要能回答「如果这一环不做，下游会显示什么假话」。**
    修的连带项：字节一旦要存要发，就得补**按文件头判类型**（不信扩展名、不信 multipart 的
    `File.type`，否则一张 `text/html` 被原样回给浏览器就是存储型 XSS）、公开地址
    **只对已上架的提交开放**（`submission_assets.asset_kind='shot'` +
    `submissions.status='published'`，否则就是「上架前有公开地址」）、空值不外发
    （服务端把空串当非法输入，每次正常发布都撞 400）。
79. **凡是「按行拆结构渲染远端文本」的都别用 v-html（2026-10-04）：**
    更新说明来自远端更新清单，是**不可信输入**。工程里唯一的 Markdown 渲染是 Milkdown
    （编辑器）那条链，为一段说明拉整套编辑器进更新弹窗不划算；而任何字符串→HTML 的第三方
    渲染器都要另配消毒。故自建按行拆块（`utils/notesParse.ts`：标题/列表/段落 + 段内
    `**加粗**`），用 Vue 模板渲染——**全文没有一个 v-html**。认不出的行按段落原样显示，
    不猜它是标题还是列表：猜错会把一行字突然变成大字，比不排版更难读。

80. **service 后台的授权是**逐版本**的闸门，加「自动信任」开关时别改坏它（2026-10-06）：**
    `extension.rs::service_version_trusted` 要求 `service:version` **等于**当前 manifest
    版本 —— 扩展一升级就重新不信任，用户必须再确认一次。这是有意的：service 扩展要跑
    本地后台程序，而「点一下同意」是唯一的闸门。
    新增 `auto_trust_service`（默认 **false**）时要守三条：① 开关判断写在**最前面**并
    直接 return —— 写成 `&&` 的最后一环就是「界面显示已开、行为没变」，而这是最难发现的
    那种错；② 开了开关**仍要求 `version.is_some()`**（那是「manifest 坏了」，不是
    「用户信任了它」）；③ `#[serde(default = "default_auto_trust_service")]` 必须是
    `false` —— 升级**不得**替用户打开一个安全开关。
    ⚠️ 该函数把 `auto_trust` 作为**参数**而不是内部 `config::load()`：否则用例结论取决
    于跑测试那台机器的用户配置（用户在自己机器上一开开关，CI 就红）——
    与 `merge_disk_authoritative` 同一个理由，能纯函数就纯函数。

82. **内置运行时的下载源：镜像在前、官方在后，顺序本身就是需求（2026-10-06）：**
    `runtime.rs::dist_urls` 返回三个源，**按序尝试、第一个成功即采用**：
    `cdn.npmmirror.com` → `mirrors.aliyun.com` → `nodejs.org`。
    实测（同一时刻同一文件）：镜像 16.1 / 13.4 MB/s，官方 1.28 MB/s —— 官方当天
    还测出过 0.34MB/s。48.8MB 的差距是「3 秒」与「2.5 分钟」。
    **官方必须留在末尾**，这不是偏好：新版本发布时镜像会滞后（404），
    那一刻官方是唯一有该版本的源；顺序反过来 = 新版本永远装不上，
    而症状只显示「镜像 404」，极难归因。
    换源之所以安全：三个源的包**字节完全一致**（实测 sha256 同为 `961024296c2a8e60…`），
    且 `checksum_url` 取的是**同目录**的官方 `SHASUM256.txt` 做校验。
    报错必须带上**每个源各自的失败原因**（`failures` 数组）——
    只说「下载失败」的话，用户与后来人都无法判断该等还是该换版本。
    两条守卫：Rust 侧 `dist_urls_order_is_actually_asserted`、
    `scripts/check-service-startup.mjs` 的规则 ③b。
    ⚠️ 守卫判据写的是「**存在任意镜像**」而非「存在 npmmirror」——
    第一版写死域名，结果「删掉 npmmirror 只留阿里云」这条变异是绿的，
    规则退化成查一个具体域名，域名一换就失效。

83. **速记导出包的 `manifest.json` 是导入的唯一数据源，`.md` 只是给人读的副本**（v0.8.0 ⑤，`notes_port.rs`）：
    包里三样东西：`manifest.json`（文件夹路径 / 标签 / 图标 / 原始时间戳 / 正文）、
    `notes/<文件夹>/<NNN>-<标题>.md`（同一次导出写出的可读副本）、`images/`（内嵌图片）。
    五条口径：① **导入不解析 Markdown** —— 解析意味着标题、标签、时间戳要从文件名与正文里猜，
    猜错就是静默丢数据；代价是「手改 `.md` 再导入不生效」，这一点写在包内 `README.txt` 里。
    ② **导入永远是追加**，按「标题 + 创建时间」判重跳过 —— 导入错一个包的代价只是
    一堆可删的重复行，覆盖的代价是原笔记找不回来。③ **回收站不导出**（否则删掉的笔记会复活）。
    ④ **图片地址在包内改写成 `images/<名>` 相对路径**，导入时改回协议 URL；
    绝不把数据根绝对路径写进包（约定 72 判据：换台机器还能用吗）。
    改写只认 `16 位十六进制 + 白名单扩展名` 这个形状（`import_note_image` 的产出形态），
    否则正文里偶然出现的 `images/照片.jpg`（外部相对链接）会被改写成协议 URL 而成死链。
    协议形态有**两种**（Windows 的 `http://mhub-note.localhost/<名>` 与
    macOS/Linux 原生的 `mhub-note://localhost/<名>`），导出时两种都认，只认一种的话
    另一种形态的图片会在导出后变成死链而作者看不出原因。
    ⑤ 两条命令都是 **async**：同步命令会在主线程上跑完整个打包过程（几百篇笔记 + 几十 MB 图片 = 数秒冻结），
    锁只在 await-free 的一段里持有。⚠️ **导入后前端要调 `store.reloadNotesFull()`**
    （笔记 + 文件夹 + 标签一起刷：导入会**新建**文件夹与标签，只刷笔记的话它们不出现，
    而笔记已经挂在那些文件夹下了）；后端也**刻意不 emit** `notes-changed`（它的监听者
    只刷笔记，正漏掉文件夹与标签这两处新建）。

    ⚠️ **同批修掉的连带项：`list_notes` 曾是「仅元信息」列表，一并改回带正文。**
    判据与症状见 `repo::note::list` 的注释（编辑器切笔记直接取 `props.note.content`，
    列表没正文 = 切走再切回来看到空白笔记，敲两个字就把原文覆盖掉且无报错）。
    **别再把它优化成「只拉元信息」** —— `state.notes` 被它整份替换，编辑器没有第二处来源。

84. **AI 深度整理只返回、不落库，采纳与否由用户在预览面板里决定**（v0.8.0 ⑦，`commands::tidy_note_content`）：
    与「一键美化」的本质差别：那个是纯本地幂等的排版整理、点错了能看出来；
    这里的输出是模型生成的、可能整篇改写，而 600ms 防抖的自动保存会让「替换」立刻落库。
    四条配套：① **模型选择必须复用 `pick_chat_model`**（含平台额度的负载切换）——
    另写一套「取第一个模型」会让「设置里换了模型」在对话里生效、在整理里不生效。
    ② **不新建会话、不写 `chat_messages`**：它是一次性工具，顺手记一条只会给对话列表塞空壳会话。
    ③ **不流式**：用户看的是「改完什么样」而非逐字蹦；流式还多一道「部分 Markdown 渲染成坏的」的排版雷。
    ④ 输入上限 2 万字，超了直接拒 —— 悄悄截断会让模型以为看全了，产出两头都不细的结果。
    模型常把整篇裹进 ```` ``` ```` 代码块返回，`strip_code_fence` 剥掉**外层**围栏而**保留正文内部的代码块**
    （后者往往是用户笔记里的真代码，剥了等于静默删内容）；没闭合的围栏一律不动
    ——宁可多一层可见的围栏，也不要因为「猜它大概是包裹」而吃掉正文。

85. **速达加密备注：名字明文、正文密文、密钥只进钥匙串且读不出时绝不重生成**（v0.8.0 ⑧，`secret.rs`）：
    四条口径（缺一条这套东西就变成「看起来有、其实不能用」）：
    ① **`resources.secret_label` 是明文且列表可见**（界面上是 🔒 + title）——
    名字加密的话用户面对的是一串密文，无法分辨哪条资源写了备注；
    密文只在 `resources.secret_note`，且**只经 `repo::resource::get_secret` 取**，
    绝不进 `COLS`（列表/详情查询）。守卫 `list_payload_never_carries_the_ciphertext`
    断言「序列化给前端的 JSON 里不含密文」+「`COLS` 不含 secret_note」两条（都做过变异验证）。
    ② **主密钥存独立 keyring 服务名 `m-hub-secret`**（不是 `m-hub-chat`）——
    混进 AI Key 那一个的话，某条 chat 模型被删/改名会连带搞丢密钥，全部密文当场不可逆报废。
    ③ **`master_key` 只在 `NoEntry` 时生成**；钥匙串里那串读出来长度不对就**报错**，
    绝不当作「没有密钥」重新生成 —— 重新生成 = 旧密文永久解不开，而症状是一句看不懂的错。
    ④ **AES-256-GCM、每次新 nonce**（守卫 `nonce_is_fresh_per_encryption`）：同明文两次密文必须不同。
    单测**只走纯函数**（`encrypt_with`/`decrypt_with`）——真钥匙串路径在 CI/单测里会弹系统授权框。
    前端：打开弹窗即解密（能打开应用的人就能看备注），但正文**默认掩码显示**
    （`textarea` 没有 `type=password`，隐藏态走 `-webkit-text-security`；
    写成 `:type` 只是个被忽略的属性，界面看起来加了密而实际是明文）；清空正文保存 = 永久删除，需二次确认。

## 平台约定（macOS 移植，**先读这一节再改任何代码**）

上游是一份 Windows 代码，本仓库把它移植到 macOS。**功能一一对应，平台机制全部换过一遍。**
`README.md` 有一张完整的「平台差异对照」表，这里只记**最容易踩、且单测抓不到**的那几条。

### P1 · 新增能力前，先查「macOS 上对应什么」，查不到就不要加

Windows 侧大量能力来自 Win32 / PowerShell / 注册表，macOS 上要么有正规替代、要么根本没有。
往里加东西时按这个顺序问：

| 想做的事 | Windows 做法 | macOS 上的对应物 |
| --- | --- | --- |
| 感知某个系统状态变化 | 收系统消息（`WM_*`） | **多数没有**。改轮询，或干脆去掉这个功能 |
| 读/写剪贴板 | `CF_*` + `AddClipboardFormatListener` | `NSPasteboard` + 轮询 `changeCount` |
| 拿到「当前前台是谁」 | `GetForegroundWindow`（窗口句柄） | `NSRunningApplication.runningApplications`（**只有应用，没有窗口**），粒度更粗 |
| 拿到鼠标全局位置 | `GetCursorPos`（物理像素） | `CGEvent::location()`（**逻辑点**）→ 必须按屏 backing scale 换算成物理像素 |
| 知道左键按没按 | `GetAsyncKeyState` | `CGEventSourceButtonState`（公开 API，**不是** `NSEvent.pressedMouseButtons`，那个被 objc2 标为主线程限定） |
| 装一个无边框透明浮窗 | `decorations:false + transparent(true)` | 同左，**但依赖 `macos-private-api`**（见 P2） |
| 弹一个不抢焦点的窗 | `WS_EX_NOACTIVATE` + `SW_SHOWNA` | `NSWindowStyleMaskNonactivatingPanel`（`mac.rs::set_nonactivating_panel`） |
| 模拟按键 | `keybd_event` | `CGEvent` 合成（**需辅助功能权限**，未授权时系统**静默丢弃**事件） |
| 自启动 | 注册表 Run 键 | `~/Library/LaunchAgents/*.plist` + `launchctl bootstrap` |
| 提权启动 | `Start-Process -Verb RunAs` | `osascript` 的 `do shell script … with administrator privileges`（**两重转义**：AppleScript 字符串 + shell 引用） |
| 取某个程序的图标 | `System.Drawing.Icon::ExtractAssociatedIcon` | `NSWorkspace.iconForFile` → TIFF → `NSBitmapImageRep` → PNG（`mac.rs`） |
| 枚举已安装应用 | 注册表 Uninstall + 开始菜单 | 扫 `/Applications` 等目录下的 `.app` |
| 升级自替换 | 换单个 exe | 换**整个 `.app` 包**（换内层二进制会破坏包签名 → Gatekeeper 拒启） |

**结论先说**：找不到对应物的，**不要**为了「功能完全一样」硬凑。凑出来的东西
（用 `osascript` 模拟点击、用 AppleScript 读窗口层级）在系统更新后静默失效，
且没有任何测试能挡住。上游的「隐藏窗口降内存」在 macOS 上就是**有意 no-op**
（`webview_mem.rs`），不是待办。

### P2 · `macos-private-api` 已开启 → 不上架、自签分发

本工程每个浮窗都是 `transparent(true)` 的无边框圆角小窗。Tauri 在 macOS 上
默认不编译 `transparent()`：tao 把 NSWindow 设成透明，但 WKWebView 仍刷白底，
窗口里是一块白方块 —— 整套圆角设计塌掉。去掉白底靠 WKWebView 的私有 KVC 键
`drawsBackground`，**没有公开替代品**。

后果：`Cargo.toml` 必须开 `tauri` 的 `macos-private-api`，`tauri.conf.json` 必须配
`app.macOSPrivateApi: true`（**两处必须同时改**，只改一处会在 build.rs 阶段直接报错：
「The `tauri` dependency features … does not match the allowlist」）。
因此**过不了 App Store 的静态分析**，m-hub 走自签分发（.app / .dmg）。
将来若必须上架，唯一出路是放弃透明浮窗、改用系统原生窗口外观 —— 设计级返工。

### P3 · 屏幕坐标：全工程只有 `mac.rs` 允许碰 core-graphics

macOS 的屏幕坐标有三层坑，任何一层搞错都是「浮窗跑到半屏之外 / 贴边永远判不出」：

1. **逻辑点 vs 物理像素**：`CGEvent::location()` 给逻辑点；Tauri 的 `outer_position`
   / `work_area()` 给物理像素。Retina 上差 2 倍。本模块（悬浮球）**统一用物理像素**，
   换算收在 `mac.rs::cursor_physical()`。
2. **原点方向**：`CGEvent::location()` 已经是左上原点、y 向下（与 Windows 同）；
   **不要**用 `NSEvent::mouseLocation()`——那是 Cocoa 坐标（左下原点、y 向上），
   多显示器上下排布时换算极易出错。
3. **工作区**：macOS 的 Dock 与菜单栏会盖住屏幕边缘，定位浮窗必须用
   `Monitor::work_area()`（Tauri 内部走 `NSScreen.visibleFrame`）而不是整屏矩形。

**规则**：`core_graphics` 只在 `mac.rs` 里 import。要新查屏幕信息就在那里加函数。

### P4 · 浮窗几何是**共享**的，只有 4 个原语分平台

`floating_ball.rs` 的贴边停靠/滑出几何（`edge_tick` / `dock_snap` / `slide_to` /
`dock_hidden_pos` …）是**两平台共用的同一份代码**，全篇用物理像素。分平台的只有：

- `cursor_pos()` — `GetCursorPos` vs `mac::cursor_physical()`
- `lmb_down()` — `GetAsyncKeyState` vs `CGEventSourceButtonState`
- `window_scale()` — `GetDpiForWindow`（实时 DPI，缓存会过期）vs `scale_factor()`
  （macOS 没有「缩放中途变化」，缓存不会过期，故不需要自愈）
- `apply_geometry_native()` — `SetWindowPos`（尺寸+位置**一次**调用）vs
  `set_size` + `set_position`

⚠️ **改这 4 个之外的几何时不要加 `#[cfg]`**。上游把它们整段 gate 在
`#[cfg(target_os = "windows")]` 下，那是为了让 Windows 独占；移植时已全部解 gate。
重新加回去 = 两平台几何分叉，约定 42 的七条铁律（滑出量 / 吸附带 / 记忆收敛…）
在 macOS 上会立刻失准。

### P5 · 自动更新换的是 `.app` 包，不是二进制

`updater.rs::build_replace_target`：macOS 的分发单元是整个
`/Applications/m-hub.app`。只换 `Contents/MacOS/m-hub` 会留下过期的
`Info.plist` / `Frameworks` / 图标，**并且破坏代码签名**（签名覆盖整个包），
Gatekeeper 随即拒绝启动。备份名是同级的 `m-hub.app.old`，
`ReplaceTarget::commit()` 失败必须回滚到旧版本（mac 上「半个 app」= 彻底起不来）。

拉起新实例走 `open -n <包路径>`（LaunchServices）而不是直接 spawn 包内二进制：
直接 spawn 会绕过应用注册，表现为 Dock 图标延迟出现、窗口不自动前置。

### P6 · 签名密钥是 m-hub 自持的，不要换回上游公钥

`src-tauri/keys/market_public.key` 是**本仓库自己生成**的 Ed25519 公钥，
与上游 x-hub 的不是同一对。私钥只存在于发版机上。
沿用上游公钥意味着「上游作者可以为 m-hub 签一份市场清单 / 升级清单并被客户端接受」——
对一个独立分发的 fork，这是错的信任方向。轮换公钥时**连带更新**
`signing.rs` 的测试向量（用同一私钥重新签发）。

### P7 · 剪贴板是轮询的，别把它改成「更实时」

AppKit **没有**剪贴板变化通知。`NSPasteboard` 的 `changeCount` 只是自增计数器。
本工程 250ms 一跳、只读 `changeCount`（不读内容），变化才触发读取与入库。

两条别动：
- **间隔不能调小**。100ms 级轮询在应用常驻时会持续吃 CPU。感知延迟 250ms 人无感。
- **暂停监听时也要刷新基准**。否则恢复监听会把暂停期间的 N 次变化当成「一次复制」
  一次性入库。

### P9 · 判据：没有「论证注释」的平台 gate 都要当移植遗漏看待

`webview_mem.rs` / `win_taskbar.rs` / `process.rs::activate_existing` 这几处
**有意的 no-op**，每一处都有一段点名具体机制的论证（WebKit 没有对应 API、
activation policy 是 per-app 而非 per-window、LaunchServices 已代劳）。
凡是**没有**这类论证、却因为换了平台而消失的能力，都应当视为「移植遗漏」而非「有意取舍」。

2026-09-29 全面排查就靠这条判据，7 处真遗漏全部命中。它们的共同形态是
**「上游换了数据源，下游的匹配代码没跟着换」**：

| 上游的语义 | macOS 上变成了 | 没跟着换的地方 |
| --- | --- | --- |
| `target` 是 `…\chrome.exe` | `…/WeChat.app`（包路径） | 比进程名 → 绿点全灭 |
| `GetForegroundWindow` 拿 HWND | `NSRunningApplication`（只有应用） | bundle id 误取包名 → 焦点归还失效 |
| 扩展名过滤 `exe/lnk` | `.app` 是包、无扩展名 | 文件选择器全灰 |
| `RegisterHotKey(VK_ESCAPE)` | 无公开等价物 | Esc 兜底消失 |
| 系统缩放边（borderless 窗口没有） | 无 | 整窗不可缩放 |

**加新能力时反过来用这条**：先问「macOS 上这个值/这个名字长什么样」，
再确认**所有消费它的地方**都按新形态匹配了 —— 只改数据源那一端是最容易漏的。

### P8 · AppKit 线程：`NSWorkspace`/`NSPasteboard` 后台可用，窗口层级不行

- **可以**在后台线程用：`NSWorkspace`（图标查询，走 LaunchServices 缓存）、
  `NSPasteboard`（Apple 明确文档化可后台使用）、`CGEvent*`、
  `NSRunningApplication`。
  批量扫描上百个应用图标时因此**不** dispatch 回主线程——那会把
  `scan_installed_apps` 变成几十次 UI 卡顿，而该命令是 `async` 正是为了不冻结界面。
- **不行**：`NSWindow` / `NSView` 的创建与几何。`NSEvent.mouseLocation` /
  `pressedMouseButtons` 被 objc2 标为 `method_family = none`（主线程限定），
  本工程因此改用 core-graphics 的等价 API。

窗口生命周期一律走 Tauri 的主线程事件循环，**本模块禁止运行期 build/destroy 窗口**
（约定 41 铁律，macOS 同样适用）。

---

## 命令速查

```bash
npm run dev           # Vite 开发服务器（浏览器预览 http://localhost:1420）
npm run tauri:dev     # Tauri 开发窗口（需 Rust 工具链）
npm run build         # vue-tsc 类型检查 + vite build
npm run tauri:build   # 构建 .app / .dmg（src-tauri/target/release/bundle/）
cd src-tauri && cargo test --lib    # Rust 单元测试（macOS 直接跑，无需包装脚本）
```

## 发版清单（版本号单一来源 = README）

### ⚠️ 为什么没有 release workflow（2026-10-06 开源时核实）

上游的 `.github/workflows/release.yml` **已删除**：它是纯 Windows 的
（`runs-on: windows-latest` + 找 `src-tauri/target/release/m-hub.exe` +
打 `win-x64.zip` + 便携版 marker 文件），而本仓库是 macOS 版，产物是
`.app` / `.dmg`，那个 exe 根本不存在。三条理由：

1. **CI 上做不出可用的产物**。macOS 分发单元是整个 `.app` 包，且必须签名 ——
   签名要证书，GitHub runner 上没有。所以 CI 只能产出**未签名**的 `.app`，
   用户双击会被 Gatekeeper 拦（还得「右键→打开」或 `xattr -dr com.apple.quarantine`）。
   这比不给更糟：用户会以为下载坏了。
2. **触发条件是 `v*` 标签**，而它编译的是 Windows exe —— 打错一个标签就白烧
   10 分钟 CI 额度并产出一个用不了的东西。
3. **本地构建已经覆盖了**：`npm run tauri:build` 出 `.app` + `.dmg`，本机自签，
   `shasum -a 256` 校验后上传 Release。本轮 v0.7.5 就是这么出的。

保留的两个 workflow：
- `ci.yml` —— 已适配 macOS（`macos-latest` + `npm run tauri:test`，后者在 macOS
  上就是普通 `cargo test --lib`，不需要上游那个嵌 manifest 的 PowerShell 包装，
  见约定 67）。
- `release-extension.yml` —— **已停用**（`workflow_dispatch` only，任何触发只打印
  说明并失败）。它是一份显式的历史记录：扩展发布自 2026-09 起统一走服务端，
  该 workflow 曾依赖的签名私钥与 COS/R2 凭据 secret 已全部删除。
  **不要因为「看起来是遗留」就删它** —— 里面记着「为什么私钥不能留在开源仓」。



每次发版从 README 向下同步版本号（`README.md` 徽章 → `package.json` → `src-tauri/tauri.conf.json` → `src-tauri/Cargo.toml` → `AGENTS.md` 头部），并：

1. 在 `RELEASE_NOTES.md` **顶部**新增一节 `# vX.Y.Z 发布说明`（累积式，旧版依次排后，勿覆盖历史）。若改动要**并入尚未发布的当前版本**（版本号不变，如 v0.5.2 的 tag 已打但 release 未 publish），则不新增节，直接就地修订该节条目。
2. 同步 `README.md` 版本徽章与 `DESIGN.md` 顶部「版本对齐」。
3. git tag 用 `vX.Y.Z` 触发 `.github/workflows/release.yml`（tag 号须与 `tauri.conf.json` version 一致，否则打包产物版本漂移）。**`src-tauri/Cargo.lock` 必须一并提交**：改 `Cargo.toml` 版本号时 cargo 会顺带更新 lock 里 `name = "app"` 的 version，发版提交漏掉它就使远端 tag 的 lock 停在旧版本（v0.5.2 就是这样补交过一次；自查 `git show <tag>:src-tauri/Cargo.lock`）。
4. GitHub Release **正文 = `RELEASE_NOTES.md` 的 `# vX.Y.Z 发布说明` 章节**，由 `release.yml` 自动提取（到下一个一级标题为止；段落缺失才回退 tag annotation）。tag annotation 不再承担 release notes，打 tag 无需写说明；单独改已发布 release 正文用 `gh release edit vX.Y.Z --notes-file <文件>`（v0.5.1 起约定，此前正文误用 commit message）。**坑：`action-gh-release` 只在「创建 release」时用 `body_path`，对已存在的 release（含 draft）只更新产物、不覆盖正文**——移动 tag 重跑（`git tag -f` + `git push -f`）后正文仍是旧文案，必须再手动 `gh release edit --notes-file` 补一次；发版后核对 `gh release view vX.Y.Z --json body`，别默认它已跟着 RELEASE_NOTES 走。
5. **本地测试包（未 commit 的验证包）用 4 段式版本**：只给 `tauri.conf.json` 的 version 加构建元数据 `X.Y.Z+MMDDHHmm`（如 `0.5.3+09081524`，尾段=构建时间），应用内「关于」即可区分是哪次构建；zip 命名 `m-hub-win64-X.Y.Z.MMDDHHmm.zip`。正式提交 / 打 tag 前必须还原为 3 段（其余版本文件不动）——4 段点号版不是合法 semver，Cargo / tauri.conf 都解析不了。
6. **浮窗存活实机自查（v0.5.3/v0.5.4 事故后新增，见约定 41）**：本轮若动过 `notify.rs` / `clipboard.rs` / `floating_ball.rs` / `chat_window.rs` 或任何增删建窗代码，发版前必须从**关闭态干净启动**走一遍完整开关路径：设置里把每个浮窗开关「关→开」一次再唤起（新建独立窗那一刻是最高危点，v0.5.4 就死在首次开启），并设一条 1 分钟后的倒计时/待办提醒确认真右下角弹窗、供应商/配置能正常加载、最后托盘退出正常。这类跨端时序回归单测/类型检查全绿、只有实机暴露。

## 注意事项

- **禁止前台跑 watch 类命令（agent/自动化执行纪律，v0.5.5 实测事故）：** `npm run dev` / `tauri:dev` / `vite` 等常驻进程前台执行会占死执行通道直到超时（曾实测卡住 agent 4.5 小时并吞掉全部追问），且残留进程占着 1420 端口影响后来者。规则：① 验证类型/构建一律用跑完即退的 `npm run build`；② 确需起 dev server 实机验证时后台分离启动（`Start-Process -WindowStyle Hidden` 记 PID）→ 轮询端口就绪 → 测完**必杀**并复查端口已释放；③ 启动前先 `Get-NetTCPConnection -LocalPort 1420` 查残留、有则先清；④ 所有外部命令带超时参数，绝不留 watch 挂等
- **decorations: false**：窗口无边框，标题栏/窗口控制全部自定义（TitleBar.vue）
- **startDragging 权限**：`core:window:allow-start-dragging` 已在 capabilities 声明
- **数据目录：** 数据根默认 `app.path().app_data_dir()/` = `%APPDATA%\m-hub`（identifier 为 `m-hub`；旧标识 `com.workbench.desktop` 的数据在启动时自动迁移一次，且 `lib.rs::fix_icon_paths` 会把数据库中的旧图标路径批量替换为新目录）；支持设置中「更改数据存储路径」（迁移后重启生效）与**便携版**（exe 同目录放空文件 `portable` → 数据固定为 `exe\data`），统一由 `paths.rs` 解析
- **日志：** `tauri-plugin-log` 文件日志 → `%APPDATA%\m-hub\logs\m-hub.log`（Info 级别），同时输出 Stdout + Webview；所有命令入口记录成功/失败，数据查询类用 `log::debug!` 防噪音；启动程序遇 os error 740（需要管理员权限）自动经 PowerShell `Start-Process -Verb RunAs` 触发 UAC 提权
- **配置位置：** 数据根目录下 `app.json`（与数据库同目录，随「更改数据目录」一起迁移；`config.rs` AppConfig 全字段 serde default，新增字段天然兼容老配置）。主要字段：主题三件套（theme_mode/theme_preset/accent_color）+ 外观（wallpaper_path/wallpaper_blur/wallpaper_veil/wallpaper_immersive/glass_opacity）、sidebar_toggle、window、global_shortcut、dashboard_layout（工作台网格）+ dashboard_mid_content（废弃遗留）、countdown_sound、clock_quote、通知驻留时长（notice_duration_ms，毫秒；由 notify.rs 随每条通知下发给通知窗，改设置立即生效）、联网（online_enabled/weather_city/weather_lat/weather_lng/quote_source）、AI 对话（chat_models/chat_panel_width/open/side/height/opacity + 独立窗形态 chat_window_mode/width/height/x/y/pinned，其中 mode 经 chat_window_save_mode、几何由后端记忆，save_config 均以磁盘为准）、剪贴板（clipboard_shortcut/max_items/ttl_days/paused/paste_method/image_enabled/file_enabled）、字号（font_scale/font_sticky/font_notes/font_prompt/font_todo）、扩展（runtime_strategy/sidebar_extensions/extension_open_modes + **已废弃**的 market_endpoint —— 自 v0.6.1 起客户端不再直连对象存储，市场/升级地址按 `config::DEFAULT_SERVER_URL` 拼服务端接口，两个字段只作兼容占位、装载时被 `config.rs::migrate_legacy_endpoints` 归一落盘 + **后端单独写盘**的 dev_extensions、skill_roots，dev_mode_enabled 废弃仅兼容）、自启动（run_at_startup）、自动更新（auto_update_enabled/update_interval_hours/skipped_update_version + **已废弃**的 update_endpoint，同上）
- **「后端单独写盘」的配置字段必须在 `merge_disk_authoritative` 里以磁盘为准，且必须登记进 `BACKEND_MANAGED_FIELDS`（否则被前端旧快照清空）：** 前端 `state.config` 是**启动快照、之后不再刷新**，而 `setDashboardLayout` 等操作会把整份配置发回 `save_config` 落盘；凡是只由后端命令写、前端从不回写的字段（⚠️ **不要在这里写条目数或罗列字段名** —— 那本身就是第 N+1 份拷贝，必然会漂；真相源是 `config.rs::BACKEND_MANAGED_FIELDS`，由 `backend_managed_field_list_is_real_and_effective` 逐条守着「名字存在 + 合并后确实取磁盘值」，`backend_only_fields_survive_a_snapshot_save` 守着「快照覆盖不掉它们」。要查有哪些，看那个常量。**尚未自动化的一点**：新增一个「只由后端写盘」的字段时，如果忘了登记，现有测试**不会**自动报出来 —— 两条测试守的是「清单内」与「已知的漏登记实例」，不是「所有后端独占写字段」这个集合本身（要自动判定就得扫描各模块的写盘点，那会非常脆）。所以**加这类字段时，记得顺手在 `backend_only_fields_survive_a_snapshot_save` 里加一段断言**，把「它会被快照吃掉」这个失败模式钉住。
  **⚠️ 判据：「这个字段在不在 `src/api/tauri.ts` 的 `AppConfig` 里」既不是必要条件、也不是保护手段。** 前端认识它 → 提交的是启动快照里的旧值；前端不认识它 → 整份提交后反序列化按 `AppConfig::default()` 的**同名字段值**补缺（容器级 `#[serde(default)]`，不是字段类型的 `Default`），**两种照样覆盖磁盘**——`skill_roots` 恰恰因为不在类型里才被悄无声息地补成空数组。所以「不放进 `AppConfig`」救不了你，唯一的保护是「合并 + 登记 + 测试」这三件套。
  新增此类字段时**必须同时**改四处：字段注释、`merge_disk_authoritative` 合并、`config.rs` 的 `BACKEND_MANAGED_FIELDS` 清单、`src/api/tauri.ts` 的 `AppConfig` 注释清单；漏登会被 `config::tests::merge_keeps_backend_managed_fields` / `backend_managed_field_list_is_real_and_effective` 直接判红（合并逻辑做成纯函数就是为了能在 config.rs 里无 `AppHandle` 地回归；注意登记表靠人维护，忘登则测试是绿的，所以这四处同步没有捷径）。
- **数据恢复：** `backup_data`/`restore_data` 命令只把备份暂存为 `restore.db`/`restore_icons` 并写 `.restore_pending` 标记，重启时 `apply_pending_restore` 才替换正式数据（lib.rs）
- **SQLite：** 使用 `rusqlite` crate（bundled）
- **Tauri 权限：** 新增前端 API 调用需在 `src-tauri/capabilities/default.json` 声明对应权限
- **测试工具函数：** `db.rs::init_in_memory` 仅 `#[cfg(test)]` 使用；`repo/*.rs` 大多含单元测试（snippet/chat/clipboard 等有端到端验证）
- **dialog 插件：** 前端 `@tauri-apps/plugin-dialog` 的 `open()` 需 `dialog:allow-open` 权限（已声明）；浏览器预览环境需 `isTauri()` 守卫
- **reka-ui 调试：** segment 输入/焦点问题必须用**真实键盘事件**验证（Playwright `browser_press_key` 逐个按键），`browser.type` 类工具是直接改 DOM 文本、不触发 `keydown`，会造成「显示变了但 v-model 不同步」的假象；验证时检查快照中 segment 的 `[active]`（焦点位置）与隐藏 input 的 value（如 `15:45:00`）是否同步
- **指定浏览器打开网页（v0.3.0）：** 速达右键网页资源可「用 XX 打开」；浏览器列表来自注册表 `SOFTWARE\Clients\StartMenuInternet`（HKLM+HKCU+Wow6432Node，`browsers.rs` 枚举，按 exe 路径去重、按显示名排序；显示名取键默认值/LocalizedString，间接字符串或缺失时按 exe 文件名兜底）；`open_url_with_browser` 仅放行 Web 资源 + http/https，成功后与默认打开一致刷新 `last_launched_at`；前端在 Suda.vue 挂载时预热列表缓存，右键零等待
- **右键菜单置位必须在事件派发外（陷阱）：** `ContextMenu.vue` 在 window 上监听 `contextmenu`/`click` 用于点别处关闭菜单；若在卡片 `@contextmenu` 处理器里**同步**置 `menu.visible = true`，同一事件冒泡到 window 的关闭监听会把刚开的菜单立刻关掉（表现为右键无反应），且菜单已开时在另一资源上右键会因 visible 未变化不触发定位 watch（菜单出现在旧位置）。Suda.vue `openMenu` 用 `setTimeout(0)` 把置位推迟到派发结束后——新增右键入口时必须沿用此模式
- **cargo test（Windows 必须走包装脚本）：** 直接 `cargo test` 的测试 exe 启动即死（`STATUS_ENTRYPOINT_NOT_FOUND` 0xC0000139）——tauri/wry 静态导入 `comctl32!TaskDialogIndirect`，该导出只存在于 Common-Controls v6 程序集，而测试 exe 不经 tauri-build 的 manifest 嵌入（后者只作用于 bin 目标），缺激活上下文时 loader 用 comctl32 5.82 解析导入。修复：`npm run tauri:test`（`scripts/cargo-test.ps1`）——`cargo test --no-run` 后用 SDK `mt.exe` 给每个测试 exe 嵌入 `src-tauri/windows/app.manifest`（幂等覆盖 RT_MANIFEST）再运行。诊断手法：PE 导入表逐符号 `GetProcAddress` 探测缺失导出（误报需过滤）；`cargo:rustc-link-arg-tests` 不覆盖 lib 单元测试（仅 tests/ 目录），别再尝试链接器路线
- **WebView2 内存级别联动（v0.7.0，webview_mem.rs，内存优化 P0+P3）：** 所有常驻隐藏窗（主窗/对话/剪贴板/通知/悬浮球/prompt-float/todo-float/速达池窗口/扩展独立窗 `ext-*`）隐藏时把 WebView2 内存目标级别设为 `Low`（`ICoreWebView2_19::SetMemoryUsageTargetLevel`：弃缓存换页、**脚本照常跑、事件照常收**，区别于冻结渲染进程的 TrySuspend），显示前恢复 `Normal`——与约定 41 完全兼容：窗口生命周期一根手指不碰，只翻内存级别。结构 = 显隐函数里的**快路径**（show 前 `on_shown` / hide 后 `on_hidden` / 速达槽位 `on_slot`，tray/clipboard/chat/floating_ball/notify/suda_browser 六处接线）+ **300ms 轮询纠偏**（按窗口实际可见性收敛，覆盖任务栏还原最小化主窗等绕过 Rust 显隐函数的路径；状态表去重，级别没变不打 COM）。新增常驻窗口必须进 `managed_targets`；`sticky-*`/`countdown-*` 瞬态窗不参与；**suda-panel 故意不设**（主窗子 webview，若与主窗共享 renderer，主窗可见时把它设 Low 会拖累整个主窗）。开关 `webview_mem_low_on_hide`（设置 → 功能 → 性能「隐藏窗口时降低内存占用」，默认开）。P3：`ADDITIONAL_BROWSER_ARGS` 追加 `--js-flags=--scavenger_max_new_space_capacity_mb=8 --disk-cache-size=33554432`——与 tauri.conf.json 主窗的 additionalBrowserArgs **必须逐字一致**（webview_mem.rs 守卫测试锁住；不一致 = 同一 user data folder 下环境创建失败，内存不降反增），GC 卡顿则把 8 放宽到 16–32。**实测口径（2026-09-25）：Low 对空白页 renderer −65%（48→13-20MB），但对完整 SPA 页只 −10%（~110→~100MB）——Low 吐得掉缓存、吐不掉 Vue 活堆（应用实例 + 组件树 + store 拷贝），活堆必须靠轻量入口减**。对话窗的活堆随会话增长（消息 DOM + mdCache 渲染缓存），另有专项卸载：`chat_window::hide_window` emit `chat-window-hidden` → ChatPanel.unload() 清消息/流式缓冲/mdCache（流式进行中跳过），show 时 refresh() 重拉当前会话（数据都在 SQLite，几十毫秒）——隐藏态回到 ~20MB 不随聊天长度增长。webview_mem 每次级别变更落一行 INFO 日志（`[webview-mem] <label> → Low/Normal`），诊断 P0 是否生效看这里
- **壳窗口轻量入口（v0.7.0，内存优化 P1/P2）：** 速达 chrome 顶栏 / 悬浮球 / 通知窗不再加载 index.html，各自走多页入口 `chrome.html` / `ball.html` / `notice.html`（vite `rolldownOptions.input` 四页，入口脚本在 `src/light/*.ts`）——只挂自己的组件（BrowserChrome/FloatingBallWindow/NoticeOverlay）+ `style.css`，不含 Index/store/UpdateCheckDialog。三组件本就自包含（无 inject，主题各自管理：Ball/Notice 内部 get_theme_config 自举 + 事件跟随；Chrome 入口脚本补一次 `getThemeConfig` 覆盖 useTheme 的 store 默认值）。**新增轻量壳窗口的步骤**：根目录加 `<name>.html` + `src/light/<name>.ts`（createApp + style.css + 组件）+ vite input 加一行 + Rust 建窗处 `WebviewUrl::App("<name>.html")`。App.vue 里对应 label 分支保留作兜底（这些窗口现在根本不会加载 index.html，分支不可达但零害）。dev 模式下 Vite dev server 直接服务根目录 html，无需额外配置。配套：App.vue 按 label 给各窗口设置 `document.title`（m-hub 主窗/对话/剪贴板/…）——Win11 任务管理器的 WebView2 列表按页面标题命名 renderer，不设就全部同名分不清；只改页面标题，不影响任务栏/Alt+Tab 的原生窗口标题

## 待实现

- 已决策未实施：**首次使用引导**（快速设置弹窗 + 帮助视图；术语预登记于 `CONTEXT.md`「引导」节，实施前代码中无 OnboardingDialog/HelpView/onboarding_done）
- 方案已定稿未实施：**本地文件搜索**（索引工作区模型）
- 方案已定稿未实施：**Agent 底座**（宿主内自研 agent loop，非外部引擎；模型接入走「平台额度 + BYOK」双路，13 个首批工具 + 三档权限模式。实施前代码中无 `src-tauri/src/agent/`、`chat_messages` 无 `turn_id` 列、`role` 仍只有 user/assistant）

> ⚠️ 这三项原各有一份方案文档（`docs/adr/0001`、`docs/file-search-plan`、`docs/adr/0006` +
> `docs/agent-foundation-plan`），**全部从未提交过**（2026-10-06 开源时逐个核对发现）。
> 方案要点已压缩成上面这几行；**不补写完整文档** —— 那会产生第二份真相，
> 而这几行本身才是「有没有实施」的判据。
- 可探索方向：拖拽排序动效打磨、键盘导航、前端单元测试、打包发布全流程验证（tauri:build）
