<div align="center">

# ⚡ m-hub — 本地个人效率工作台（macOS）

基于 **Tauri 2 + Vue 3 + TypeScript + Rust** 的桌面效率工具，Bento 风格界面。
**所有数据默认本地存储，不上传云端。**

> **平台**：本仓库是 **macOS 版**。功能与上游 Windows 版 [x-hub](https://github.com/dckxx/x-hub)
> 一一对应，差异只在平台实现——所有 Windows API（CF_HTML 剪贴板、Win32 浮窗几何、
> 注册表自启、PowerShell 图标提取…）都换成了对应的 macOS 机制
> （NSPasteboard / NSWindow + CGEvent / LaunchAgent / NSWorkspace）。
> 详见 [平台差异对照](#-平台差异对照)。

![Tauri](https://img.shields.io/badge/Tauri-2.x-24C8DB?logo=tauri&logoColor=white)
![Vue](https://img.shields.io/badge/Vue-3.x-42b883?logo=vuedotjs&logoColor=white)
![TypeScript](https://img.shields.io/badge/TypeScript-6.x-3178c6?logo=typescript&logoColor=white)
![Rust](https://img.shields.io/badge/Rust-1.77+-dea584?logo=rust&logoColor=white)
![SQLite](https://img.shields.io/badge/SQLite-local-003b57?logo=sqlite&logoColor=white)
![Version](https://img.shields.io/badge/version-0.7.3-blue)
![License](https://img.shields.io/badge/license-MIT-blue)

</div>


## ✨ 功能特性

### 🕐 工作台
时钟（含**实时天气** Open-Meteo 温度/体感/湿度/风速 + 城市/IP 定位）、系统资源监视器（CPU/内存，2s 轮询）、便签（2 槽，600ms 防抖自动保存）、提示词百宝箱、待办清单、最近使用通栏。工作台为**自由编排 Bento 网格**：9 种部件（时钟/便签×2/速记概览/待办概览/速达数量/倒计时/提示词/待办）可在布局编辑器中增删、拖拽、调宽高并记忆。时钟语录接入**在线名言（hitokoto）**，离线自动回退本地语料，点击换一句。

### ⏳ 倒计时
时长/定时/每天/间隔**四种模式**（最多 6 个）；暂停/继续/浮窗/删除；**后台驱动**（Rust 1s 轮询，到点发系统通知，`once` 灰态 / `daily` / `interval` 自动顺延，休眠错过静默顺延）；可浮起为**透明圆形水罐浮窗**（水位水波动画，独立置顶小窗，位置持久化）；可选到点提示音（WebAudio 合成双音）。

### 🚀 速达
应用 / 网页 / 文件**三类资源合一**管理；分组筛选（全部/常用/应用/网页/文件 + 文件二级分类）；拖拽 exe/lnk 导入并**自动提取程序图标**；**扫描已安装应用批量导入**；点击一键启动——**已在运行的应用 / 浏览器直接把它已有窗口调度到前台**（含最小化与收进托盘的），不再重复开一个实例；右键菜单操作；删除可撤销。

### 📝 速记
纯文本 + **Markdown 编辑/预览**；600ms 防抖自动保存；**标签管理**与按标签筛选；相对时间/摘要列表。

### 🔍 全局搜索
`Ctrl+K` 唤起，300ms 防抖，同时检索**资源、笔记、待办**，点击直达。

### ✅ 待办清单
添加/完成/删除；优先级循环切换（普通/重要/紧急，10px 纯色圆点）；行内编辑；待办与已完成视图分离；支持全局搜索直达与高亮。

### 📋 剪贴板历史
快捷键 `Ctrl+\`` 全局唤起浮层；记录**文本 / 图片 / 文件**三类内容——复制图片/截图自动落盘缩略图、复制文件记路径；支持粘贴回剪贴板、图片预览与「保存图片」、相同内容自动去重；图片/文件记录开关可配。

### 🤖 AI 对话
**OpenAI 兼容流式对话**（DeepSeek/OpenAI/Ollama/one-api 等，SSE 打字机效果）；**多会话管理**（新建/切换/删除）；**面板四方位停靠**（左/右/上/下，可拖拽调整尺寸并记忆位置）；**Markdown 渲染**回复（代码块/表格/列表等）；供应商模型管理——**测试连通性** + **拉取模型批量勾选添加** + 同供应商模型共享 API Key + **保存前校验**（卡片未添加模型会被拦下并指名，避免半填卡片被静默丢弃）；API Key 存**系统钥匙串**，界面脱敏（👁 查看 / 📋 复制）；面板透明度可调（50%–100%）；`Ctrl+Shift+K` 唤起。

### 💬 提示词百宝箱
常用提示词片段管理；置顶 + 复制计数；卡片一键复制。

### 🧩 扩展系统
**扩展中心**本地清单展示已安装扩展，支持从 **GitHub 仓库地址下载 zip** 解包安装、卸载、检查更新；**manifest 注册表**解析权限声明与版本；扩展支持 **module 卡片**（嵌入主界面）/ **view 页面** / **window / drawer 浮窗** 四种形态；宿主注入 **桥 API（window.mhub）** 与主程序交互（权限按 manifest 逐项授权，**安装前单独明示高风险能力**；打开外链需声明 `open-url` 权限，未声明时给出可见提示而非静默失败）；**service 托管**内置运行时按需下载、自动降级；扩展可**固定到左侧栏**，点击即打开，并选择「视图 / 窗口 / 抽屉」打开方式。

### ⚙️ 系统设置
**三轴主题**（模式 亮/暗/系统 × 10 色 + 10 渐变预设 × 强调色 8 预设/自定义）、**全局快捷键录入**（失焦/回车自动保存）、**开机自启动**（Run 键方式，登录后静默驻留托盘）、**工作台布局编辑器**（9 种部件自由编排）、**联网**（总开关 / 城市设置 / 名言来源）、**通知驻留时长**（1–60 秒，改完立即生效）、**倒计时提示音开关**、**AI 助手**（供应商/模型配置）、**AI 对话面板透明度**、**数据备份与恢复**、**数据存储路径**。

### 🖥️ 窗口能力
无边框 + 透明自制标题栏（拖动/最大化/还原/置顶按钮/关闭至托盘）；系统托盘常驻；`Ctrl+Shift+Space` 全局唤起；记忆窗口位置尺寸；便签/倒计时独立浮窗。

### 🔄 应用自动更新
**自研升级链路**：从 `releases/update.json` 升级清单拉取**新版本信息**（Ed25519 分离签名验签，内嵌公钥验签通过才信任）→ semver 版本比较 + **跳级保护**（`minimumUpgradable` 下限）→ **自动静默检查**（启动 5s 后 + 默认每 4 小时，可在 About 关闭）；发现新版本弹出**全局更新弹窗**（版本号 / 说明 / 体积 / 便携版标记），支持**跳过此版本**（记录到配置）/**立即更新**（流式下载 + 实时进度条 + sha256 完整性校验）/ 就绪后**立即重启**；重启时解包并两步 rename **自替换**，失败自动回滚、下次启动重试；About「检查更新」可手动触发。更新包分发在腾讯云 COS，支持标准版 / 便携版分别取包。

### 💾 数据存储与便携
所有数据默认本地存储，支持三种形态灵活切换：**标准版**数据默认在 `%APPDATA%\m-hub`，可在设置中「更改数据存储路径」迁移到任意目录（迁移后重启生效）；**便携版**只需在 exe 同目录放一个空文件 `portable`，数据即固定跟随 `exe\data` 子目录，整个文件夹拷到 U 盘即可随身携带；**数据备份/恢复**打包为 `m-hub-backup-时间戳.zip` 单个压缩包，便于归档与迁移。

## ⌨️ 快捷键

macOS 上默认用 **⌘（Command）** 作为主修饰键。

| 快捷键 | 功能 |
| --- | --- |
| `⌘ + K` | 唤起全局搜索 |
| `⌘ + ⇧ + K` | 唤起 / 收起 AI 对话面板 |
| `⌘ + ⇧ + Space` | 显示 / 隐藏主窗口（可在设置中自定义） |
| `⌃ + ⌘ + V` | 唤起剪贴板历史浮层（可在设置中自定义） |
| `Esc` | 关闭弹窗 |

> 剪贴板默认键**没有**沿用上游 Windows 版的 `⌘⌥V`——那是 macOS 自带的
> 「粘贴并匹配样式」，系统级注册，第三方抢不到（注册会静默失败）。
> `⌃⌘V` 是 macOS 剪贴板管理器的通行惯例，不撞任何系统键。
>
> ⚠️ **模拟粘贴需要辅助功能权限**：在系统设置 → 隐私与安全性 → 辅助功能里
> 给 m-hub 打开开关，否则「粘贴回原处」会静默无效（不报错、按键没反应）。
> 这一点在应用首次启动时会写进日志（`日志/辅助功能权限`）。

## 🛠️ 技术栈

| 层 | 技术 |
| --- | --- |
| 前端 | Vue 3（`<script setup>`）+ TypeScript + Tailwind CSS 4 + Vite 8 |
| 图标 | lucide-vue-next（按需引入，颜色继承 currentColor） |
| 表单 | reka-ui 无头组件（DatePicker / TimeField / NumberField，样式自绘） |
| 后端 | Rust（Tauri 2）+ rusqlite（SQLite, WAL 模式）+ sysinfo（系统资源）+ 自绘右下角通知窗（见 notify.rs）+ reqwest（OpenAI 兼容 SSE 流式）+ keyring（API Key 存 **Keychain**） |
| 平台 | NSPasteboard（剪贴板历史，轮询 changeCount）+ AppKit/CoreGraphics（浮窗几何、贴边停靠、模拟按键）+ LaunchAgent（开机自启）+ NSWorkspace（应用图标与已安装应用扫描） |
| 状态 | `reactive()` + `readonly()` 自定义 store（无 Pinia） |
| 样式 | 设计令牌 CSS 变量（Bento 风格，见 `DESIGN.md`），三轴主题（模式 × 预设 × 强调色） |

## 🚀 快速开始

### 环境要求

- macOS 10.15+（Ventura 及以上系统同样适用）
- Xcode Command Line Tools（`xcode-select --install`）
- Node.js 18+
- Rust 1.77.2+（[rustup](https://rustup.rs/)）
- WebView 由系统自带（WKWebView），无需额外安装

### 安装与运行

```bash
npm install

npm run dev           # Vite 浏览器预览 (http://localhost:1420)
npm run tauri:dev     # Tauri 桌面开发窗口
npm run build         # vue-tsc 类型检查 + vite build
npm run tauri:build   # 构建 .app / .dmg（产物在 src-tauri/target/release/bundle/）
```

构建产物：

```
src-tauri/target/release/bundle/macos/m-hub.app   # 应用包
src-tauri/target/release/bundle/dmg/m-hub_0.7.2_aarch64.dmg
```

首次打开被 Gatekeeper 拦下时（本机自签，未做公证 notarization）：

```bash
xattr -dr com.apple.quarantine /Applications/m-hub.app
```

### macOS 权限

| 能力 | 何时需要 | 不给会怎样 |
| --- | --- | --- |
| 辅助功能 | 剪贴板「粘贴回原处」（模拟 ⌘V） | 静默无效，**不报错** |
| 通知 | 倒计时 / 待办提醒 | 退回到应用内右下角通知窗（自绘，不依赖系统通知，本就默认走这条） |
| 完全磁盘访问 | 读取 `/Applications` 以外的自定义目录 | 只影响「更改数据存储路径」指向受保护目录时的读写 |

三项都是**用到才问**：辅助功能在首次模拟粘贴时由系统弹窗询问；
通知在首次推送时询问。应用不主动索取。


## 📂 目录结构

```
src/
├── main.ts / App.vue        # 入口与窗口壳（App.vue 按窗口 label 路由：主界面 / 便签浮窗 / 倒计时浮窗 / 剪贴板浮层 / 扩展浮窗 / 提示词浮窗 / 待办浮窗）
├── index/index.vue          # 首页：侧栏导航（工作台/速记/速达/扩展）+ 三轴主题 + 工作台布局协调 + 搜索/设置协调
├── style.css                # 设计令牌（亮/暗色）+ 通用样式
├── api/tauri.ts             # Tauri invoke 类型安全封装（120+ 个命令）
├── stores/workbench.ts      # 响应式状态管理（工作台/系统信息/提示词/倒计时/AI 对话/扩展/自动更新）
├── composables/             # 组合式函数（useResourceIcon / useFocusTrap / useTheme）
├── utils/                   # 文件分类 / 时间 / 错误上报 / chime 提示音
└── components/              # 功能组件（工作台卡片/速达/速记/搜索/待办/设置/倒计时/AI 对话/扩展中心/更新弹窗…）

src-tauri/
└── src/
    ├── lib.rs               # 应用构建：数据库/托盘/快捷键/窗口状态/数据迁移/命令注册 + 定时检查更新
    ├── commands.rs          # Tauri 命令
    ├── models.rs / db.rs    # 模型与 SQLite 迁移
    ├── config.rs            # 配置持久化（主题/窗口/全局快捷键/提示音/通知驻留时长/AI 模型/更新源与自动更新开关）
    ├── process.rs           # 程序启动（已在运行则调度窗口到前台）/ URL 打开 / 提权（UAC）
    ├── shortcut.rs / tray.rs
    ├── sysmon.rs            # 系统资源监视（CPU/内存）
    ├── notify.rs            # 右下角自绘通知窗（跨 Win10/11，前端 NoticeOverlay.vue 渲染卡片）
    ├── chat.rs              # OpenAI 兼容 SSE 流式对话客户端 + API Key 钥匙串存取
    ├── countdown_ticker.rs  # 倒计时后台驱动线程（1s 轮询 → 通知 + 事件 + 顺延）
    ├── countdown_window.rs  # 倒计时圆形浮窗（创建/销毁/位置持久化）
    ├── market.rs            # 扩展市场远端清单 + Ed25519 验签 + 下载/更新/卸载
    ├── updater.rs           # 应用自动更新（update.json 验签 + 下载校验 + 重启自替换）
    └── repo/                # 数据访问层（resource/note/todo/sticky/snippet/tag/countdown/chat）
```

## 🔒 数据与隐私

数据统一存放在「数据根」目录下（macOS 标准版默认为
`~/Library/Application Support/m-hub`，可经设置改到任意目录；
便携版为「可执行文件同级 `data/`」）：

- **数据库**：`数据根/app.db`（SQLite，resources/notes/todos/stickies/snippets/tags/countdowns/chat_sessions/chat_messages）
- **图标**：`数据根/icons/`（拖拽导入/扫描安装应用时自动提取的程序图标）
- **日志**：`数据根/logs/m-hub.log`（文件日志，便于排查）
- **剪贴板快照**：`数据根/clipboard/images/`（复制图片时落盘的缩略图；删除 / 清空 / 过期清理时联动删除）
- **备份**：设置内一键备份/恢复，打包为 `m-hub-backup-时间戳.zip` 压缩包（数据库 + 图标）
- **应用更新**：下载的更新包暂存在 `数据根/updates/`，自替换成功 / 失败回滚后自动清理
- **AI 对话**：会话与消息存本地 SQLite；API Key 存入**系统钥匙串**（keyring），界面脱敏展示，**不明文落盘、不上传**

## 配图

> 下列截图取自上游 x-hub。界面与 Windows 版一致（同一套前端 + 同一套设计令牌），
> 仅窗口装饰与部分系统交互按 macOS 惯例调整；本仓库尚未提供 macOS 实机截图。


---

## 🗺️ 平台差异对照

功能与上游 x-hub 一一对应；差异全部集中在「同一件事在 macOS 上该怎么做」。
这张表是改代码前的索引——**先在这里找到自己要去的那一行**，别去翻 Windows 实现。

| 能力 | Windows（上游） | macOS（本仓库） | 代码位置 |
| --- | --- | --- | --- |
| 剪贴板变化感知 | `AddClipboardFormatListener` → `WM_CLIPBOARDUPDATE` 消息推送 | 轮询 `NSPasteboard.changeCount`（250ms），AppKit **无**推送 API | `clipboard.rs::start_monitor` |
| 剪贴板文本 | `CF_UNICODETEXT` | `public.utf8-plain-text` | `clipboard.rs` / `mac.rs` |
| 剪贴板富文本 | `CF_HTML` + StartFragment/EndFragment 偏移解析 | `public.html`（整份文档，交前端 sanitize） | `clipboard.rs` |
| 剪贴板图片 | `PNG` 自定义格式 / `CF_DIBV5`→`CF_DIB` | `public.png` / `public.tiff` | `clipboard.rs` |
| 剪贴板文件 | `CF_HDROP` + `DragQueryFileW` | `public.file-url`（+ 老的 `public.file-name` 兜底） | `mac.rs::read_clipboard_files` |
| 点浮层外部收起 | `SetWindowsHookExW(WH_MOUSE_LL)` 低级鼠标钩子 | 借用悬浮球已有的 100ms 光标轮询做矩形判定 | `floating_ball.rs::tick_clipboard_overlay` |
| 浮窗「不抢焦点」显示 | `WS_EX_NOACTIVATE` + `SW_SHOWNA` | NSWindow `NSWindowStyleMaskNonactivatingPanel` | `mac.rs::set_nonactivating_panel` |
| 模拟粘贴按键 | `keybd_event` 扫 `Ctrl+V` | `CGEvent` 合成 `⌘V`（**需辅助功能权限**） | `mac.rs::send_paste_keystroke` |
| 归还焦点 | `SetForegroundWindow(hwnd)` | `NSRunningApplication.activateWithOptions` | `mac.rs::activate_app` |
| 开机自启 | 注册表 `HKCU\...\Run` | `~/Library/LaunchAgents/com.mhub.desktop.plist` + `launchctl` | `autostart.rs` |
| 提权启动 | PowerShell `Start-Process -Verb RunAs`（UAC） | `osascript` 的 `do shell script … with administrator privileges` | `process.rs::launch_elevated` |
| 「已在运行则前置」 | 枚举同名进程 + 挑主窗口 + `SetForegroundWindow` | 恒交给 `open -a`（LaunchServices 天然复用实例并激活） | `process.rs` |
| 拖入导入 | `.exe` 直接读 / `.lnk` 经 WScript COM 解析目标 | `.app` 包直接用 / 无扩展名但带可执行位 | `commands.rs::parse_dropped_path` |
| 应用图标提取 | `System.Drawing.Icon::ExtractAssociatedIcon`（GDI，32×32） | `NSWorkspace.iconForFile` → TIFF → `NSBitmapImageRep` → PNG → **缩到 256px**（原始 1024px，118 个应用会占 136MB） | `mac.rs::extract_app_icon` |
| 扫描已安装应用 | 注册表 Uninstall 项 + 开始菜单 `.lnk` | 扫 `/Applications`、`~/Applications`、`/System/Applications` 下的 `.app` | `mac.rs::scan_app_candidates` |
| 枚举浏览器 | 注册表 `Clients\StartMenuInternet` | 扫 `.app` + 读 `Info.plist` 判是否声明 `http`/`https` scheme | `browsers.rs` |
| 屏幕坐标 | 物理像素、左上原点 | **逻辑点**、左上原点（`CGEvent.location()`）；与物理像素差一个 backing scale | `mac.rs::cursor_physical` |
| 托盘 / Dock | 任务栏 + 托盘图标 | 保留 **Dock 图标**（托盘类应用无 Dock 会让用户找不到窗口），⌘Tab 由系统管 | `win_taskbar.rs` |
| 透明无边框浮窗 | `transparent(true)` 原生支持 | 需开 `tauri` 的 **`macos-private-api`**（见下） | `Cargo.toml` |
| WebView 内存调档 | `ICoreWebView2_19::SetMemoryUsageTargetLevel` | **无对应能力**，有意 no-op；省内存改靠轻量壳窗口 | `webview_mem.rs` |
| 自替换升级 | 换单个 `.exe`（两步 rename） | 换**整个 `.app` 包**（只换内层二进制会破坏包签名） | `updater.rs::build_replace_target` |
| 便携版 | exe 同目录放 `portable` 标志 | 同左，但 .app 包必须自包含；详见 `paths.rs` 模块注释 | `paths.rs` |

### 三个需要提前知道的取舍

**1. 开启了 `macos-private-api`，因此无法上架 Mac App Store。**

本工程**每一个**浮窗都是 `decorations:false + transparent(true)` 的无边框圆角小窗。
Tauri 在 macOS 上默认**不编译** `transparent()`：tao 会把 NSWindow 置成透明，
但 WKWebView 仍照旧刷自己的白底——窗口里是一块白方块，只有圆角外透明，
整套 Bento 圆角设计直接塌掉。去掉那层白底靠的是 WKWebView 上的私有 KVC 键
`drawsBackground`，**没有公开替代品**。

所以 m-hub 走**自签分发**（.app / .dmg），不上架。若将来必须上架，
唯一出路是放弃透明浮窗、改用系统原生窗口外观——那是一次设计级返工。

**2. 剪贴板监听是轮询，不是推送。**

AppKit 没有「剪贴板变了」的回调，`NSPasteboard` 的 `changeCount` 只是个自增计数器、
不带任何通知机制。所以只能轮询它：250ms 一跳，只读 `changeCount`（一次 IPC，
不读内容），变化才触发真正的读取与入库。复制到历史入库的感知延迟 ≤ 250ms，
人无感；100ms 级会把常驻应用的 CPU 吃满，得不偿失。

**3. 「隐藏窗口降低内存占用」在 macOS 上没做，因为做不到。**

WebKit 没有 MemoryUsageTargetLevel 的等价物；macOS 对内存压力是**系统级**处理的
（memory pressure 通知 + JetSAM 采样，WebKit 内部自行降级），应用侧再手工配一个
档位既做不到、也不该做。取而代之，macOS 侧把省内存的力气花在
**轻量壳窗口**（chrome / ball / notice 三个入口不加载 index.html）
与对话窗隐藏时的 `unload()` 上——都是真的在减活堆，而不是调一个不存在的旋钮。

### 已知未实现 / 降级的部分

- **「用 XX 浏览器打开」依赖 LaunchServices 枚举。** 已改为向系统问
  「哪些应用能打开这个 https 链接」（`-[NSWorkspace URLsForApplicationsToOpenURL:]`），
  这是「系统默认浏览器」设置面板的同一份数据，所以 Setapp 子目录、
  Homebrew 改过路径、手工拷贝的应用都能列出来（早先只扫三个目录的一级，
  会漏掉整类）。代价是结果里会混进个别「能当浏览器用但不是浏览器」的 App
  （如某些 AI 客户端注册了 http），这是系统数据本身的性质。
- **剪贴板浮层的 Esc 关闭依赖全局事件 tap**（`CGEventTap`，非 HID 层，
  不需要辅助功能权限）。它会**吞掉浮层显示期间的那一次 Esc** —— 与 Windows 版
  `RegisterHotKey` 的行为一致。tap 建不起来时只记日志，浮层照常可用，
  此时只能点浮层外部关闭。
- **主窗没有系统阴影，是自绘的。** macOS 的 AppKit 不给透明窗口画系统阴影
  （实测显式 `setHasShadow(true)` 也无效），所以窗口比内容大了一圈
  `--window-shadow-margin`(32px) 让 CSS `box-shadow` 有地方画。
  连带三处行为：窗口持久化存的是**可视区**尺寸而非窗口尺寸（老配置语义不变、无需迁移）；
  最大化时外扩带/圆角/阴影一起归零（否则屏幕四边会露出透出桌面的缝）；
  外扩带本身是 8 向缩放手柄的容身处。口径见 AGENTS.md 约定 69。
- **模拟粘贴**依赖辅助功能权限，未授权时静默无效（有日志，非静默失败）。
  未实现的原生能力（WebView 内存调档）已在上面「三个取舍」里逐条说明，不在此重复。
- **平台服务端仍是上游作者的**（`https://m-hub.xfactor.top`）。账号登录 /
  AI 额度 / 扩展市场 / 应用升级都打这一个内置常量，换地址只改
  `config.rs::DEFAULT_SERVER_URL` 一处并重新发版（设置页没有地址入口）。
  要真正独立分发，这是下一个要动的地方。
- **自动更新的分发链路**（COS 上的 `releases/update.json` + Ed25519 签名）
  沿用上游的发布侧。签名密钥**已经是 m-hub 自己的一对**（见 AGENTS.md 约定 P6），
  但发布机还得另配 —— 换句话说「客户端能验签，但没人签得出它认的包」，
  在自建发布流程之前自动更新是不通的。
- **代码签名是 ad-hoc 自签**（`signingIdentity: "-"`，本机可直接跑），
  **未做 Apple 公证 notarization**。别人机器上首次打开会被 Gatekeeper 拦，
  需 `xattr -dr com.apple.quarantine`，或自行购买开发者证书走正式签名+公证。
- **Intel Mac**（`darwin-x64`）已支持分发目标，但本仓库只在 Apple Silicon 上验证过。
- **Dock 徽标 / 应用角标**：上游也没有，未新增。

---

## 📄 License

MIT

本仓库是 [x-hub](https://github.com/dckxx/x-hub) 的 macOS 移植版，遵循其 MIT 许可证。
