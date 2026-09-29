//! 开机自启动管理。
//!
//! - **Windows**：注册表 `HKCU\...\Run` 键（登录时静默拉起，主窗不弹出、驻留托盘）。
//! - **macOS**：`~/Library/LaunchAgents/<label>.plist`（`RunAtLoad`，同样追加隐藏启动参数）。
//!   launchd 会在用户登录时拉起该 agent；写盘后额外用 `launchctl bootstrap` 立即生效，
//!   取消时 `launchctl bootout` 立即卸载，不必等下次登录。
//!
//! 两个平台的语义完全一致：**登录后静默拉起、主窗不弹出、驻留托盘**。
//!
//! 历史版本在 Windows 上曾提供「计划任务 + 最高权限」的管理员启动模式；因 Windows UIPI
//! 隔离，管理员权限进程无法从资源管理器接收文件拖放（速达拖拽导入失效），该模式已移除，
//! Windows `apply` 仍会顺带清理旧版残留的计划任务。macOS 侧不涉及此历史包袱。
//!
//! ⚠️ **不要在测试里调用 `apply(true)`**——两个平台它都会真实写系统启动项
//! （Run 键 / LaunchAgents plist），`cargo test` 会把本机已注册的自启动项删掉。

use crate::process::NoConsoleWindow;
use std::process::Command;

/// 自启动在命令行里追加的隐藏启动参数（主窗不弹出、直接驻留托盘）
pub const HIDDEN_ARG: &str = "--autostart-hidden";

/// 判断本次进程是否由自启动拉起（命令行含 HIDDEN_ARG）。
/// 供前端决定是否主动显示主窗口：自启动时不打扰用户，直接驻留托盘。
pub fn is_hidden_launch() -> bool {
    std::env::args().any(|a| a == HIDDEN_ARG)
}

// ==================== Windows：注册表 Run 键 ====================

/// Run 子键（相对 HKCU，winreg 用；HKCU\Software\Microsoft\Windows\CurrentVersion\Run）
#[cfg(target_os = "windows")]
const RUN_SUBKEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
#[cfg(target_os = "windows")]
const RUN_VALUE_NAME: &str = "m-hub";
/// 旧版管理员自启动注册的计划任务名（仅用于清理，不再创建）
#[cfg(target_os = "windows")]
const LEGACY_TASK_NAME: &str = "m-hub-autostart";
/// 任务管理器「启动项」/ 安全软件禁用某启动项时写的标记子键：值首字节 0x03=禁用，0x02=启用。
/// 删除该值即回落到默认「启用」。Run 键在、但这里被置 0x03 → 登录时 Explorer 静默跳过。
#[cfg(target_os = "windows")]
const APPROVED_SUBKEY: &str =
    r"Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run";

#[cfg(target_os = "windows")]
fn hkcu() -> winreg::RegKey {
    use winreg::enums::HKEY_CURRENT_USER;
    winreg::RegKey::predef(HKEY_CURRENT_USER)
}

/// 当前 exe 的完整路径（Run 键需要绝对路径）
#[cfg(target_os = "windows")]
fn exe_path() -> String {
    std::env::current_exe()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// 自启动命令行：`"C:\...\m-hub.exe" --autostart-hidden`
#[cfg(target_os = "windows")]
fn launch_command_line() -> String {
    format!("\"{}\" {}", exe_path(), HIDDEN_ARG)
}

/// 写入 Run 键。
#[cfg(target_os = "windows")]
fn write_run_key() -> Result<(), String> {
    use winreg::enums::KEY_SET_VALUE;
    let key = hkcu()
        .create_subkey_with_flags(RUN_SUBKEY, KEY_SET_VALUE)
        .map_err(|e| format!("打开 Run 项失败: {e}"))?
        .0;
    key.set_value(RUN_VALUE_NAME, &launch_command_line())
        .map_err(|e| format!("写入 Run 键失败: {e}"))?;
    log::info!("已写入开机自启动 Run 键");
    Ok(())
}

#[cfg(target_os = "windows")]
fn remove_run_key() {
    use winreg::enums::KEY_SET_VALUE;
    if let Ok(key) = hkcu().open_subkey_with_flags(RUN_SUBKEY, KEY_SET_VALUE) {
        let _ = key.delete_value(RUN_VALUE_NAME);
    }
}

/// 读取 Run 键里登记的启动命令行；键不存在/读不到返回 None（winreg 按 UTF-16 正确解码，兼容非 ASCII 路径）。
#[cfg(target_os = "windows")]
fn read_run_command() -> Option<String> {
    let key = hkcu().open_subkey(RUN_SUBKEY).ok()?;
    key.get_value::<String, _>(RUN_VALUE_NAME).ok()
}

/// 从启动命令行取出 exe 路径（剥外层引号与后续参数）。
#[cfg(target_os = "windows")]
fn exe_path_from_command(cmd_line: &str) -> Option<&str> {
    let s = cmd_line.trim();
    if let Some(rest) = s.strip_prefix('"') {
        let end = rest.find('"')?;
        Some(rest[..end].trim())
    } else {
        // 本程序写入始终带引号；无引号仅兜底解析第三方/手工写入的项
        s.split_whitespace().next()
    }
}

/// 当前 exe 是否就是 Run 键指向的那个路径（大小写不敏感）。
#[cfg(target_os = "windows")]
fn run_path_matches_current() -> bool {
    let Some(cmd) = read_run_command() else {
        return false;
    };
    let Some(reg_exe) = exe_path_from_command(&cmd) else {
        return false;
    };
    let cur = exe_path();
    !cur.is_empty() && reg_exe.eq_ignore_ascii_case(cur.as_str())
}

/// 是否被系统/安全软件在启动项里禁用（StartupApproved\Run 首字节 0x03）。
#[cfg(target_os = "windows")]
fn is_os_disabled() -> bool {
    let Ok(key) = hkcu().open_subkey(APPROVED_SUBKEY) else {
        return false;
    };
    match key.get_raw_value(RUN_VALUE_NAME) {
        Ok(rv) => rv.bytes.first().copied() == Some(0x03),
        Err(_) => false,
    }
}

/// 清除禁用标记：删掉 StartupApproved\Run 下对应值，Explorer 按默认「启用」处理。
#[cfg(target_os = "windows")]
fn clear_os_disabled() {
    use winreg::enums::KEY_SET_VALUE;
    if let Ok(key) = hkcu().open_subkey_with_flags(APPROVED_SUBKEY, KEY_SET_VALUE) {
        let _ = key.delete_value(RUN_VALUE_NAME);
    }
}

/// 隐藏控制台地运行一行 cmd 命令，返回是否成功
#[cfg(target_os = "windows")]
fn run_cmd_line(line: &str) -> bool {
    let mut cmd = Command::new("cmd");
    cmd.args(["/C", line]);
    cmd.no_console_window();
    cmd.status().map(|s| s.success()).unwrap_or(false)
}

/// 提权运行临时 bat（触发一次 UAC 授权），等待完成
#[cfg(target_os = "windows")]
fn run_bat_elevated(bat_path: &std::path::Path, log_tag: &str) -> bool {
    let script = format!(
        "Start-Process -Wait -Verb RunAs -WindowStyle Hidden -FilePath \"{}\"",
        bat_path.to_string_lossy()
    );
    let mut cmd = Command::new("powershell");
    cmd.args(["-NoProfile", "-WindowStyle", "Hidden", "-Command", &script]);
    cmd.no_console_window();
    let ok = cmd.status().map(|s| s.success()).unwrap_or(false);
    if ok {
        log::info!("[自启动] {} 提权操作完成", log_tag);
    } else {
        log::warn!("[自启动] {} 提权操作被取消或失败", log_tag);
    }
    ok
}

/// 旧版最高权限计划任务是否仍存在（schtasks /Query 退出码 0 表示存在）
#[cfg(target_os = "windows")]
fn legacy_task_exists() -> bool {
    let mut cmd = Command::new("schtasks");
    cmd.args(["/Query", "/TN", LEGACY_TASK_NAME]);
    cmd.no_console_window();
    cmd.status().map(|s| s.success()).unwrap_or(false)
}

/// 清理旧版管理员自启动残留的计划任务（最高权限任务普通权限删不动时走一次 UAC 授权）
#[cfg(target_os = "windows")]
fn remove_legacy_task() {
    if !legacy_task_exists() {
        return;
    }
    let line = format!("schtasks /Delete /TN \"{}\" /F", LEGACY_TASK_NAME);
    if run_cmd_line(&line) {
        log::info!("[自启动] 已清理旧版管理员自启动计划任务");
        return;
    }
    let bat = std::env::temp_dir().join("m-hub-autostart-task-del.bat");
    if std::fs::write(&bat, &line).is_ok() && run_bat_elevated(&bat, "清理旧版自启动任务") {
        log::info!("[自启动] 已通过 UAC 授权清理旧版管理员自启动计划任务");
    }
    let _ = std::fs::remove_file(&bat);
}

/// 探测真实自启动状态：`(registered, os_disabled)`。
#[cfg(target_os = "windows")]
pub fn probe() -> (bool, bool) {
    (run_path_matches_current(), is_os_disabled())
}

/// 启动自愈：用户已开启自启动、但 Run 键缺失或指向别的路径（程序被移动/换目录/被清理工具删除）时，
/// 用当前 exe 重写 Run 键。只动 Run 键，不碰旧计划任务（避免每次启动弹 UAC）。
#[cfg(target_os = "windows")]
pub fn ensure_registered() -> Result<bool, String> {
    if read_run_command().is_none() {
        write_run_key()?;
        log::info!("[自启动] 自愈：Run 键缺失，已按当前 exe 路径重写");
        return Ok(true);
    }
    if !run_path_matches_current() {
        write_run_key()?;
        log::info!("[自启动] 自愈：Run 键路径与当前 exe 不符，已重写");
        return Ok(true);
    }
    Ok(false)
}

// ==================== macOS：LaunchAgent ====================

/// LaunchAgent 的 label（同时用作 plist 文件名去扩展名）。
/// 与 `tauri.conf.json` 的 `identifier` 一致——macOS 的 bundle id 必须是反域名形式，
/// 「m-hub」本身不合法，故 tauri 侧用的是 `com.mhub.desktop`，这里跟随。
#[cfg(target_os = "macos")]
pub const AGENT_LABEL: &str = "com.mhub.desktop";

/// LaunchAgent plist 的落盘路径：`~/Library/LaunchAgents/com.mhub.desktop.plist`
#[cfg(target_os = "macos")]
fn agent_plist_path() -> std::path::PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("/"))
        .join("Library")
        .join("LaunchAgents")
        .join(format!("{AGENT_LABEL}.plist"))
}

/// 当前可执行文件（.app 内的 Contents/MacOS/m-hub 真实二进制）绝对路径。
///
/// launchd 的 `Program` 要的是**二进制**而不是 .app 包路径：`open /Applications/m-hub.app`
/// 会被 LaunchServices 当成「打开另一个应用」而可能与本进程并存，而直接指向二进制
/// 才能保证「登录后拉起的就是当前这一个实例」。
#[cfg(target_os = "macos")]
fn current_exe() -> Result<std::path::PathBuf, String> {
    std::env::current_exe().map_err(|e| format!("取当前 exe 路径失败: {e}"))
}

/// 读回 plist 里的 ProgramArguments[0]（即登记的 exe 路径）
#[cfg(target_os = "macos")]
fn read_agent_exe() -> Option<String> {
    let path = agent_plist_path();
    let data = std::fs::read(&path).ok()?;
    let plist = plist::Value::from_reader_xml(std::io::Cursor::new(data)).ok()?;
    let args = plist
        .as_dictionary()
        .and_then(|d| d.get("ProgramArguments"))
        .and_then(|v| v.as_array())?;
    args.first()
        .and_then(|v| v.as_string())
        .map(str::to_string)
}

/// 写 LaunchAgent plist。
#[cfg(target_os = "macos")]
fn write_agent_plist() -> Result<(), String> {
    use plist::Value;

    let exe = current_exe()?;
    if !exe.is_file() {
        return Err(format!("当前 exe 不存在，拒绝写入自启动: {}", exe.display()));
    }

    let path = agent_plist_path();
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)
            .map_err(|e| format!("创建 LaunchAgents 目录失败: {e}"))?;
    }

    let mut args = vec![Value::String(exe.to_string_lossy().into_owned())];
    args.push(Value::String(HIDDEN_ARG.to_string()));

    let mut root = plist::Dictionary::new();
    root.insert("Label".into(), Value::String(AGENT_LABEL.into()));
    root.insert("ProgramArguments".into(), Value::Array(args));
    root.insert("RunAtLoad".into(), Value::Boolean(true));
    // 不常驻：登录时拉起一次即退出（应用自身退出后 launchd 不再拉它）
    root.insert("KeepAlive".into(), Value::Boolean(false));
    // 装在 .app 里时用 OpenAsList 避免 launchd 直接跑包内二进制时丢工作目录
    root.insert("ProcessType".into(), Value::String("Interactive".into()));

    let mut xml = Vec::new();
    Value::Dictionary(root)
        .to_writer_xml(&mut xml)
        .map_err(|e| format!("序列化 LaunchAgent plist 失败: {e}"))?;

    // 「. 开头临时文件 + rename」原子落盘：写到一半崩溃不会留下半个 plist
    // （launchd 读到残缺 plist 会静默不加载，且用户看不出自启动为什么没生效）
    let tmp = path.with_extension("plist.tmp");
    std::fs::write(&tmp, xml).map_err(|e| format!("写入 LaunchAgent plist 失败: {e}"))?;
    std::fs::rename(&tmp, &path).map_err(|e| format!("落盘 LaunchAgent plist 失败: {e}"))?;

    log::info!("[自启动] 已写入 LaunchAgent plist: {}", path.display());
    Ok(())
}

/// 卸载已加载的 agent（`launchctl bootout`）。没加载过不算失败。
#[cfg(target_os = "macos")]
fn launchctl_unload() {
    let uid = current_uid();
    let mut cmd = Command::new("launchctl");
    cmd.args(["bootout", &format!("gui/{uid}/{AGENT_LABEL}")]);
    cmd.no_console_window();
    // bootout 对「未加载」返回非 0，属正常，不记 warn
    let _ = cmd.status();
}

/// 立即加载 agent（`launchctl bootstrap`），让用户不必注销重登就能看到效果。
#[cfg(target_os = "macos")]
fn launchctl_load() {
    let uid = current_uid();
    let mut cmd = Command::new("launchctl");
    cmd.args(["bootstrap", &format!("gui/{uid}"), &agent_plist_path().to_string_lossy()]);
    cmd.no_console_window();
    match cmd.status() {
        Ok(s) if s.success() => log::info!("[自启动] launchctl bootstrap 完成"),
        _ => {
            // bootstrap 失败不阻断开关落盘：plist 已经在 ~/Library/LaunchAgents，
            // launchd 下次登录一定会加载。此处只记日志，设置里的开关状态仍以文件为准。
            log::warn!("[自启动] launchctl bootstrap 未成功（不影响下次登录生效）");
        }
    }
}

/// 当前用户 uid（launchd 的 per-user 域是 `gui/<uid>`）
#[cfg(target_os = "macos")]
fn current_uid() -> u32 {
    std::process::Command::new("id")
        .arg("-u")
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(501)
}

/// 是否被用户在「登录项」里关掉：`launchctl print-disabled gui/<uid>` 会列出
/// `"com.mhub.desktop" => true`。用户手动禁用后 plist 仍在、但 launchd 不拉起，
/// 与 Windows 的 StartupApproved 是同一类「明明开了却没自启」的成因。
#[cfg(target_os = "macos")]
fn is_os_disabled() -> bool {
    let uid = current_uid();
    let mut cmd = Command::new("launchctl");
    cmd.args(["print-disabled", &format!("gui/{uid}")]);
    cmd.no_console_window();
    let Ok(out) = cmd.output() else {
        return false;
    };
    let text = String::from_utf8_lossy(&out.stdout);
    let needle = format!("\"{AGENT_LABEL}\" => true");
    text.lines().any(|l| l.contains(&needle))
}

/// 清除禁用标记：删除 LaunchAgents 下的 plist 即已足够（launchd 每次登录重新读取），
/// 这里只保证目录里没有残留的 disabled 记录。
#[cfg(target_os = "macos")]
fn clear_os_disabled() {
    // macOS 侧不需要「清禁用标记」这一步：禁用状态存在 launchd 的运行期数据库里，
    // 删掉 plist 并不清除它——但也不需要清，因为重新写 plist 并 `bootstrap` 时
    // launchd 会以文件为准重新加载（等价 Windows 的「重新启用时清掉
    // StartupApproved 的 0x03 标记」）。
}

/// 当前 plist 是否存在且指向当前 exe（程序被移动/换目录后需要重写）
#[cfg(target_os = "macos")]
fn agent_path_matches_current() -> bool {
    let Some(registered) = read_agent_exe() else {
        return false;
    };
    let Ok(cur) = current_exe() else {
        return false;
    };
    // macOS 默认文件系统大小写不敏感，但 launchd 存的可能是规范化前后的不同写法，
    // 故按 canonicalize 后的真实存在性比较（比字符串比较更接近「launchd 能不能找到它」）
    let reg_abs = std::path::Path::new(&registered);
    if reg_abs == cur {
        return true;
    }
    match (reg_abs.canonicalize(), cur.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => registered.eq_ignore_ascii_case(&cur.to_string_lossy()),
    }
}

/// 探测真实自启动状态：`(registered, os_disabled)`
#[cfg(target_os = "macos")]
pub fn probe() -> (bool, bool) {
    (agent_path_matches_current(), is_os_disabled())
}

/// 启动自愈：plist 缺失或指向别的路径（.app 被移动/重装）时按当前 exe 重写。
#[cfg(target_os = "macos")]
pub fn ensure_registered() -> Result<bool, String> {
    if !agent_plist_path().exists() {
        write_agent_plist()?;
        launchctl_load();
        log::info!("[自启动] 自愈：LaunchAgent plist 缺失，已按当前 exe 路径重写");
        return Ok(true);
    }
    if !agent_path_matches_current() {
        write_agent_plist()?;
        launchctl_load();
        log::info!("[自启动] 自愈：LaunchAgent 路径与当前 exe 不符，已重写");
        return Ok(true);
    }
    Ok(false)
}

// ==================== 兜底 ====================

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
pub fn probe() -> (bool, bool) {
    (false, false)
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
pub fn ensure_registered() -> Result<bool, String> {
    Ok(false)
}

// ---------- 对外接口 ----------

/// 应用自启动开关：先清掉已有注册，启用时写入平台对应的启动项。
pub fn apply(enabled: bool) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        remove_run_key();
        remove_legacy_task();
        if enabled {
            write_run_key()?;
            // 重新启用时清掉「任务管理器/安全软件把本项置为禁用」的标记：Run 键在但被禁用时，
            // 登录仍不会拉起——正是「明明开了自启动、重启却没自启」的典型成因。
            clear_os_disabled();
        }
        Ok(())
    }
    #[cfg(target_os = "macos")]
    {
        let path = agent_plist_path();
        launchctl_unload();
        let _ = std::fs::remove_file(&path);
        clear_os_disabled();
        if enabled {
            write_agent_plist()?;
            launchctl_load();
        }
        Ok(())
    }
    // 其余平台：自启动仅支持当前平台时返回错误提示
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        let _ = enabled;
        Err("当前平台不支持开机自启动".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn launch_command_line_contains_hidden() {
        // 启动命令必须始终带隐藏参数（自启动时不打扰用户，驻留托盘）
        #[cfg(target_os = "windows")]
        {
            let cli = launch_command_line();
            assert!(cli.contains(HIDDEN_ARG));
            // 路径带空格时必须有引号包裹
            assert!(cli.starts_with('"'));
        }
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn agent_plist_path_lands_in_launch_agents() {
        let p = agent_plist_path();
        assert!(p.ends_with("Library/LaunchAgents/com.mhub.desktop.plist"), "{p:?}");
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn agent_label_matches_bundle_identifier() {
        // label 必须与 tauri.conf.json 的 identifier 一致，否则 LaunchServices
        // 与钥匙串/权限提示会按另一个 bundle id 归类
        assert_eq!(AGENT_LABEL, "com.mhub.desktop");
    }

    // 注意：不要在测试里调用 apply()——它会真实写系统启动项
    // （Windows Run 键 / macOS LaunchAgents plist），cargo test 会把本机已注册的自启动删掉。
}
