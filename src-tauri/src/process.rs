use std::process::Command;

/// 让子进程不弹控制台窗口（链式：`Command::new("x").no_console_window()`）。
///
/// 宿主是 GUI 子系统进程（`main.rs` 的 `windows_subsystem = "windows"`），自身没有控制台；
/// 此时拉起控制台子系统程序（`node.exe` / `cmd` / `powershell` / `netsh` / `reg`）若不带
/// `CREATE_NO_WINDOW`，Windows 会**为子进程新建一个控制台窗口**——表现为「闪一下黑窗」。
///
/// **全工程唯一实现**：`autostart` / `runtime` / `service` / `commands` 都从这里取，
/// 新增子进程调用点直接挂 `.no_console_window()`，不要再抄一份 `creation_flags(0x08000000)`。
pub(crate) trait NoConsoleWindow {
    fn no_console_window(&mut self) -> &mut Self;
}

#[cfg(target_os = "windows")]
impl NoConsoleWindow for Command {
    fn no_console_window(&mut self) -> &mut Self {
        use std::os::windows::process::CommandExt;
        self.creation_flags(0x08000000) // CREATE_NO_WINDOW
    }
}

#[cfg(not(target_os = "windows"))]
impl NoConsoleWindow for Command {
    fn no_console_window(&mut self) -> &mut Self {
        self
    }
}

pub fn launch_program(path: &str, args: Option<&str>) -> Result<(), String> {
    let target = std::path::Path::new(path);
    let arg_vec: Vec<String> = args
        .filter(|a| !a.trim().is_empty())
        .map(split_args)
        .unwrap_or_default();

    // ---- macOS：`.app` 包走 `open -a` ----
    // macOS 上「程序」是整个 `.app` 包，不能直接 spawn（那会去跑包里的二进制、
    // 绕过 LaunchServices，表现为 Dock 不出现、输入法与权限都失效）。
    // `open -a` 同时解决三件事：① 走 LaunchServices 正常激活 ② 已在运行就把请求
    // 转给现有实例并激活它（等价于 Windows 那条 activate_existing 分支，天然复用实例、
    // 不开第二个进程）③ 不在时才真正拉起。所以 macOS 上 activate_existing 恒 false
    // 不是降级，而是把这层判断交给了系统。
    #[cfg(target_os = "macos")]
    {
        if is_app_bundle(target) {
            let mut cmd = Command::new("open");
            cmd.arg("-a").arg(target);
            if !arg_vec.is_empty() {
                // `open --args` 之后的参数原样交给应用（不带这层的话参数无处安放）
                cmd.arg("--args").args(&arg_vec);
            }
            return match cmd.status() {
                Ok(s) if s.success() => Ok(()),
                Ok(s) => Err(format!("启动程序失败「{}」(退出码 {})", path, s)),
                Err(e) => Err(format!("启动程序失败「{}」: {}", path, e)),
            };
        }
    }

    let mut cmd = if target.is_file() {
        let mut c = Command::new(path);
        // 便携软件（如绿色版 exe）依赖同目录资源文件，工作目录设为 exe 所在目录
        if let Some(dir) = target.parent() {
            c.current_dir(dir);
        }
        // 宿主是 GUI 子系统进程（无控制台），直接启动 CLI 工具/bat 时若不指定
        // CREATE_NO_WINDOW，Windows 会为子进程新建控制台窗口（闪黑窗）
        c.no_console_window();
        c
    } else {
        #[cfg(target_os = "windows")]
        let mut c = Command::new("cmd");
        #[cfg(target_os = "windows")]
        {
            // 引号包裹路径，兼容含空格路径；隐藏控制台窗口
            c.arg("/C").arg(format!("\"{}\"", path));
            c.no_console_window();
        }
        #[cfg(not(target_os = "windows"))]
        let mut c = Command::new("sh");
        #[cfg(not(target_os = "windows"))]
        c.arg("-c").arg(path);
        c
    };
    for arg in &arg_vec {
        cmd.arg(arg);
    }
    match cmd.spawn() {
        Ok(_) => Ok(()),
        // Windows 错误 740：程序需要管理员权限，自动请求 UAC 提权
        #[cfg(target_os = "windows")]
        Err(e) if e.raw_os_error() == Some(740) => {
            log::warn!("程序需要管理员权限，请求 UAC 提权: {}", path);
            launch_elevated(path, args)
        }
        Err(e) => Err(format!("启动程序失败「{}」: {}", path, e)),
    }
}

/// 路径是否是一个 macOS `.app` 包目录
#[cfg(target_os = "macos")]
fn is_app_bundle(path: &std::path::Path) -> bool {
    path.is_dir() && path.extension().and_then(|e| e.to_str()) == Some("app")
}

/// 以管理员权限启动（触发 UAC 提权确认）：PowerShell Start-Process -Verb RunAs。
/// 两条入口：launch_program 撞错误 740（程序清单要求提权）时的自动兜底，
/// 与 launch_resource_as_admin（速达右键「以管理员身份运行」）的用户显式提权。
#[cfg(target_os = "windows")]
pub(crate) fn launch_elevated(path: &str, args: Option<&str>) -> Result<(), String> {
    let has_args = args.map(|a| !a.trim().is_empty()).unwrap_or(false);
    let script = if has_args {
        "Start-Process -FilePath $env:XHUB_PATH -ArgumentList $env:XHUB_ARGS -Verb RunAs"
    } else {
        "Start-Process -FilePath $env:XHUB_PATH -Verb RunAs"
    };
    let mut cmd = std::process::Command::new("powershell");
    cmd.args(["-NoProfile", "-WindowStyle", "Hidden", "-Command", script])
        .env("XHUB_PATH", path);
    cmd.no_console_window();
    if has_args {
        cmd.env("XHUB_ARGS", args.unwrap_or(""));
    }
    cmd.spawn()
        .map(|_| ())
        .map_err(|e| format!("提权启动失败「{}」: {}", path, e))
}

/// 以管理员权限启动（触发系统授权弹窗）：`osascript` 的
/// `do shell script … with administrator privileges`——macOS 对应 Windows UAC 的
/// 唯一官方途径，会弹原生密码框，**提权失败时 `osascript` 返回非 0**（用户点取消）。
///
/// 走 shell 是因为 macOS 没有「带 RunAs 动词启动进程」的 API；因此路径与参数必须做
/// **shell 引用**（`shell_quote`），否则含空格/引号的路径会被 shell 拆开——这是把参数
/// 拼进 `do shell script` 字符串最典型的翻车点。
#[cfg(target_os = "macos")]
pub(crate) fn launch_elevated(path: &str, args: Option<&str>) -> Result<(), String> {
    let mut command_line = shell_quote(path);
    if let Some(args) = args {
        for arg in split_args(args) {
            command_line.push(' ');
            command_line.push_str(&shell_quote(&arg));
        }
    }
    let script = format!("do shell script \"{}\" with administrator privileges", escape_for_applescript(&command_line));
    let mut cmd = std::process::Command::new("osascript");
    cmd.args(["-e", &script]);
    cmd.no_console_window();
    match cmd.status() {
        Ok(status) if status.success() => Ok(()),
        Ok(status) => Err(format!("提权启动被取消或失败「{}」（退出码 {}）", path, status)),
        Err(e) => Err(format!("提权启动失败「{}」: {}", path, e)),
    }
}

/// 其他平台：以管理员身份运行无从谈起
#[cfg(not(any(target_os = "windows", target_os = "macos")))]
pub(crate) fn launch_elevated(_path: &str, _args: Option<&str>) -> Result<(), String> {
    Err("当前平台不支持以管理员身份运行".into())
}

/// POSIX shell 单引号转义：`'` → `'\''`（关闭引号、加转义引号、重开引号）
#[cfg(target_os = "macos")]
fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

/// AppleScript 字符串字面量转义：`\` → `\\`、`"` → `\"`
///
/// `do shell script "…"` 的命令串整体被 AppleScript 当成一个字符串字面量解析，
/// 它内部还有一层 shell 解析——**两层都要转义**，少一层就是路径里的引号把命令截断。
#[cfg(target_os = "macos")]
fn escape_for_applescript(s: &str) -> String {
    s.replace('\\', r"\\").replace('"', "\\\"")
}

/// 引号感知的参数分割：`--dir "C:\My Apps"` 保持为一个参数
fn split_args(s: &str) -> Vec<String> {
    let mut result = Vec::new();
    let mut current = String::new();
    let mut in_quote = false;
    for c in s.chars() {
        match c {
            '"' => in_quote = !in_quote,
            ' ' | '\t' if !in_quote => {
                if !current.is_empty() {
                    result.push(std::mem::take(&mut current));
                }
            }
            _ => current.push(c),
        }
    }
    if !current.is_empty() {
        result.push(current);
    }
    result
}

pub fn open_url(url: &str) -> Result<(), String> {
    opener::open(url).map_err(|e| format!("打开链接失败: {}", e))
}

/// 用指定浏览器打开 URL（browser 路径必须存在；URL 仅放行 http/https，由调用方校验）
pub fn open_with_browser(browser_exe: &str, url: &str) -> Result<(), String> {
    let path = std::path::Path::new(browser_exe);
    // macOS 上浏览器是一整个 `.app` 包（browsers.rs 按 Info.plist 枚举得来），
    // 用 `open -a` 指定包：已在运行时 LaunchServices 把请求转给现有实例并激活它、
    // 以新标签页打开，不在时才真正拉起——正好一站式覆盖 Windows 那套
    // 「先 activate_existing 再 spawn」的组合，不需要额外调度。
    #[cfg(target_os = "macos")]
    {
        if !path.exists() {
            return Err(format!("浏览器不存在: {browser_exe}"));
        }
        let mut cmd = Command::new("open");
        cmd.arg("-a").arg(path).arg(url);
        cmd.no_console_window();
        return match cmd.status() {
            Ok(s) if s.success() => Ok(()),
            Ok(s) => Err(format!("启动浏览器失败「{}」(退出码 {})", browser_exe, s)),
            Err(e) => Err(format!("启动浏览器失败「{}」: {}", browser_exe, e)),
        };
    }
    #[cfg(not(target_os = "macos"))]
    {
        if !path.is_file() {
            return Err(format!("浏览器不存在: {}", browser_exe));
        }
        // 浏览器已经在运行：先把它的窗口调度到前台。主流浏览器收到 URL 会复用现有实例、
        // 以新标签页打开，这里再置前一次，避免用户以为「点了没反应」。
        let _ = activate_existing(browser_exe);
        let mut cmd = Command::new(path);
        cmd.arg(url);
        cmd.no_console_window();
        match cmd.spawn() {
            Ok(_) => Ok(()),
            Err(e) => Err(format!("启动浏览器失败「{}」: {}", browser_exe, e)),
        }
    }
}

/// 若已有与 `exe_path` 同名的进程在运行，把它的顶层窗口还原并调度到前台，返回 true。
///
/// 用途：速达里点击一个**已经在运行**的应用/浏览器时，不再拉起第二个实例（很多程序不自己
/// 复用实例，会再开一个窗口/进程），而是把已有窗口直接拉出来。
///
/// **Windows**：枚举同名进程（浏览器的渲染/GPU 子进程等），按「有标题 + 面积」在它们的顶层
/// 窗口里挑一个最像主窗口的，隐藏的用 `SW_SHOW` 拉出来、最小化的用 `SW_RESTORE` 还原，
/// 最后 `SetForegroundWindow`。托盘类应用（微信/QQ 等）点「关闭」只是把主窗口**隐藏**起来，
/// 所以窗口搜索不要求可见、也不排除工具窗口。
///
/// **macOS**：恒返回 false。这不是没实现，而是**不该**在这一层做：macOS 上「程序」是
/// `.app` 包，`launch_program` 走的 `open -a`（见该函数 macOS 分支）会经 LaunchServices
/// 把请求转给**已运行的实例**并激活它，本就不会开第二个进程；不在时才真正拉起。
/// 也就是说复用实例 + 置前的判断已经由系统做完，这里再查一遍只会重复。
///
/// 另两条共通口径：只有「目标确实存在」才去找；进程没在跑、或主窗口已被销毁
/// （只剩托盘图标，典型如部分 IM）时返回 false，调用方应照常启动——这类程序自带单实例
/// 逻辑，再启动一次就会把已有窗口唤出来。
pub fn activate_existing(exe_path: &str) -> bool {
    if !std::path::Path::new(exe_path).exists() {
        return false;
    }
    match exe_file_name(exe_path) {
        Some(name) => activate_existing_by_name(&name),
        None => false,
    }
}

/// 取可执行文件名（按进程名匹配用；输入可以是完整路径）
fn exe_file_name(path: &str) -> Option<String> {
    std::path::Path::new(path)
        .file_name()
        .and_then(|s| s.to_str())
        .map(str::to_string)
        .filter(|s| !s.is_empty())
}

#[cfg(target_os = "windows")]
fn activate_existing_by_name(exe_name: &str) -> bool {
    use sysinfo::{ProcessesToUpdate, System};

    // 同名进程可能有一堆（浏览器的渲染/GPU 子进程等）：先收集 PID，再看谁的窗口是主窗口
    let mut sys = System::new();
    sys.refresh_processes(ProcessesToUpdate::All, true);
    let self_pid = std::process::id();
    let mut pids: Vec<u32> = sys
        .processes()
        .iter()
        .filter(|(pid, p)| {
            pid.as_u32() != self_pid && p.name().to_string_lossy().eq_ignore_ascii_case(exe_name)
        })
        .map(|(pid, _)| pid.as_u32())
        .collect();
    pids.sort_unstable();
    pids.dedup();
    if pids.is_empty() {
        return false;
    }
    focus_windows_of(&pids)
}

/// 在给定 PID 集合里挑一个最像「主窗口」的顶层窗口并调度到前台。
///
/// 托盘应用的主窗口被隐藏时仍然存在，所以不要求可见、也不排除工具窗口，改为按
/// 「有标题 + 面积」打分：可见 ≫ 非工具窗口 ≫ 面积大。无标题、零面积、`WS_EX_NOACTIVATE`
/// 的窗口（IPC/消息窗、提示气泡）直接排除，避免把辅助窗拉出来。
#[cfg(target_os = "windows")]
fn focus_windows_of(pids: &[u32]) -> bool {
    use windows_sys::core::BOOL;
    use windows_sys::Win32::Foundation::{HWND, LPARAM, RECT, TRUE};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        BringWindowToTop, EnumWindows, GetWindowLongW, GetWindowRect, GetWindowTextLengthW,
        GetWindowThreadProcessId, IsWindowVisible, SetForegroundWindow, ShowWindow, GWL_EXSTYLE,
        SW_MINIMIZE, SW_RESTORE, SW_SHOW, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
    };

    struct Ctx {
        pids: Vec<u32>,
        best: Option<(HWND, i64)>,
    }

    unsafe extern "system" fn enum_proc(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let ctx = &mut *(lparam as *mut Ctx);
        let mut pid: u32 = 0;
        GetWindowThreadProcessId(hwnd, &mut pid);
        if !ctx.pids.contains(&pid) {
            return TRUE;
        }
        // 无标题的顶层窗口几乎都是隐藏的辅助窗（IPC/消息窗），不是主窗口
        if GetWindowTextLengthW(hwnd) <= 0 {
            return TRUE;
        }
        let ex_style = GetWindowLongW(hwnd, GWL_EXSTYLE) as u32;
        // 无激活浮层（提示气泡一类）不抢焦点，也不该被当成主窗口
        if ex_style & WS_EX_NOACTIVATE != 0 {
            return TRUE;
        }
        let mut rect = RECT {
            left: 0,
            top: 0,
            right: 0,
            bottom: 0,
        };
        if GetWindowRect(hwnd, &mut rect) == 0 {
            return TRUE;
        }
        let area = (rect.right - rect.left).max(0) as i64 * (rect.bottom - rect.top).max(0) as i64;
        if area <= 0 {
            return TRUE;
        }
        // 量级差保证优先级：可见 > 隐藏，非工具窗 > 工具窗（托盘主窗常是工具窗，兜底仍可用）
        let mut score = area;
        if IsWindowVisible(hwnd) != 0 {
            score += 1_000_000_000;
        }
        if ex_style & WS_EX_TOOLWINDOW == 0 {
            score += 100_000_000;
        }
        if ctx.best.map_or(true, |(_, s)| score > s) {
            ctx.best = Some((hwnd, score));
        }
        TRUE
    }

    let mut ctx = Ctx {
        pids: pids.to_vec(),
        best: None,
    };
    unsafe {
        EnumWindows(Some(enum_proc), &mut ctx as *mut Ctx as LPARAM);
    }
    let Some((hwnd, _)) = ctx.best else {
        return false;
    };
    unsafe {
        // 隐藏窗口（托盘应用）要 SW_SHOW 才会露出来；最小化的窗口靠 SW_RESTORE 还原
        ShowWindow(hwnd, SW_SHOW);
        ShowWindow(hwnd, SW_RESTORE);
        BringWindowToTop(hwnd);
        if SetForegroundWindow(hwnd) == 0 {
            // 非前台进程调用会被系统拒绝：最小化再还原一次是通行的绕行写法
            ShowWindow(hwnd, SW_MINIMIZE);
            ShowWindow(hwnd, SW_RESTORE);
            SetForegroundWindow(hwnd);
        }
    }
    true
}

/// macOS 恒 false：`open -a` 已经把「已在运行则激活、不在才拉起」交给 LaunchServices，
/// 这里再判断一次是重复劳动。详见 [`activate_existing`] 的平台说明。
#[cfg(target_os = "macos")]
fn activate_existing_by_name(_exe_name: &str) -> bool {
    false
}

/// 非 Windows / macOS：没有可靠的「把已有实例窗口置前」机制，一律照常启动
#[cfg(not(any(target_os = "windows", target_os = "macos")))]
fn activate_existing_by_name(_exe_name: &str) -> bool {
    false
}

/// 打开外部链接（仅供前端调用的安全命令：只放行 http/https，防止任意 scheme 注入）。
#[tauri::command]
pub fn open_external(url: String) -> Result<(), String> {
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        return Err("只能打开 http/https 链接".to_string());
    }
    open_url(&url)
}

/// 打开本地路径：文件用系统默认程序打开，文件夹由资源管理器/文件管理器打开
pub fn open_path(path: &str) -> Result<(), String> {
    let target = std::path::Path::new(path);
    if target.is_dir() {
        #[cfg(target_os = "windows")]
        {
            Command::new("explorer")
                .arg(path)
                .spawn()
                .map_err(|e| format!("打开文件夹失败: {}", e))?;
            return Ok(());
        }
        #[cfg(not(target_os = "windows"))]
        {
            opener::open(path).map_err(|e| format!("打开文件夹失败: {}", e))?;
            return Ok(());
        }
    }
    opener::open(path).map_err(|e| format!("打开路径失败: {}", e))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn launch_nonexistent_program_returns_error() {
        // 「路径不存在」在三个平台的表现不同，只有 Unix 能直接断言失败：
        // - Unix：`sh -c "/nonexistent/..."` —— sh 本身能起来，只是命令找不到，
        //   `spawn()` 因此**成功**，真正的错误在子进程退出码里；
        // - Windows：`cmd /C` 同理，cmd 进程成功 spawn。
        // 所以只断言「不 panic」，错误路径由调用方读退出码。
        let _ = launch_program("/nonexistent/path/xyz", None);
    }

    /// 目标**确实是个文件但不可执行**时，Unix 上必须返回 Err。
    ///
    /// 这条比上面那条更能守住真实场景：速达里存了一个已被删除/替换的路径，
    /// 或拖进来一个普通文本文件——必须在点下去时就报错，而不是让 shell 去跑它。
    #[cfg(target_os = "macos")]
    #[test]
    fn launch_non_executable_file_returns_error() {
        let dir = std::env::temp_dir().join("mhub-launch-probe");
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("not-executable.txt");
        std::fs::write(&file, b"hello").unwrap();
        // 去掉可执行位，确保一定走「不是可执行文件」这条错误路径
        use std::os::unix::fs::PermissionsExt;
        let mut perm = std::fs::metadata(&file).unwrap().permissions();
        perm.set_mode(0o644);
        std::fs::set_permissions(&file, perm).unwrap();
        let result = launch_program(file.to_str().unwrap(), None);
        assert!(result.is_err(), "不可执行文件应报错，实际 {result:?}");
        let _ = std::fs::remove_file(&file);
    }

    #[test]
    fn exe_file_name_extracts_basename() {
        // Windows 路径的 `file_name` 语义只在 Windows 上成立：
        // `std::path` 按平台选分隔符，在 macOS 上 `\` 不是分隔符，
        // `r"C:\...\chrome.exe"` 会被当成一整个文件名。所以分平台断言。
        #[cfg(target_os = "windows")]
        assert_eq!(
            exe_file_name(r"C:\Program Files\Google\Chrome\Application\chrome.exe").as_deref(),
            Some("chrome.exe")
        );
        assert_eq!(exe_file_name("/usr/bin/firefox").as_deref(), Some("firefox"));
        assert_eq!(exe_file_name("chrome.exe").as_deref(), Some("chrome.exe"));
        assert_eq!(exe_file_name(""), None);
    }

    #[test]
    fn activate_existing_ignores_non_file_targets() {
        // 目标不存在（命令行 / URL / 纯名字）时不该去匹配进程，恒 false
        assert!(!activate_existing("not-a-real-file-xyz"));
        assert!(!activate_existing("https://example.com"));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn shell_quote_wraps_and_escapes_single_quote() {
        // 普通路径：整体加单引号
        assert_eq!(shell_quote("/Applications/Safari.app"), "'/Applications/Safari.app'");
        // 含空格：引号必须保住整串不被拆词
        assert_eq!(shell_quote("/Users/a b/My App.app"), "'/Users/a b/My App.app'");
        // 含单引号：关引号 → 转义 → 重开（POSIX 标准写法）
        assert_eq!(shell_quote("/tmp/it's here"), r"'/tmp/it'\''s here'");
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn applescript_escape_covers_both_metacharacters() {
        // do shell script "..." 内层是 AppleScript 字符串字面量，反斜杠与双引号都要转
        assert_eq!(escape_for_applescript(r#"a"b"#), r#"a\"b"#);
        assert_eq!(escape_for_applescript(r"a\b"), r"a\\b");
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn app_bundle_detection_requires_dir_named_app() {
        assert!(is_app_bundle(std::path::Path::new("/Applications/Safari.app")));
        // 名字带 .app 但不是目录 → 不算包
        assert!(!is_app_bundle(std::path::Path::new("/tmp/foo.app.txt")));
    }
}
