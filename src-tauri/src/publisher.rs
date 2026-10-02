//! 扩展发布（客户端侧）：把开发中的扩展打包上传到平台，并查看自己的提交。
//!
//! 流程：`pack_dir_to_archive`（与本地打包同一实现，保证产物结构一致）→ multipart 上传 →
//! 服务端跑关卡 → 返回逐项结论（未通过时前端直接展示"哪里不合格"）。
//!
//! 两条纪律：
//! - **打包在临时目录进行**，绝不往开发者的源码目录里写任何东西（见 ADR 0005）。
//! - 客户端**不内置任何审核规则**，只展示服务端返回的结论（PRD 附录 A 红线）。

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::cmp::Ordering;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

/// 关卡单项结论（与 `admin-web` 的展示口径一致：给人看的 label/detail）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GateItem {
    pub id: String,
    pub label: String,
    pub ok: bool,
    #[serde(default)]
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SubmitResult {
    /// 服务端的提交 id
    pub id: i64,
    /// pending_review / gate_failed
    pub status: String,
    pub gate_passed: bool,
    pub gate_items: Vec<GateItem>,
    pub ext_id: String,
    pub version: String,
    /// 剩余配额（服务端只回剩余次数，不回上限）
    pub quota: Option<Value>,
}

fn nanos() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0)
}

/// 扩展 id 里的点号等字符不能进临时文件名
fn sanitize(id: &str) -> String {
    id.chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '-' })
        .collect()
}

/// 打包某个扩展（已装或开发中）到临时文件，返回 (路径, id, version)
fn pack_to_temp(app: &tauri::AppHandle, id: &str) -> Result<(std::path::PathBuf, String, String), String> {
    let dir = crate::ext_protocol::resolve_ext_dir(app, id)?;
    let out = std::env::temp_dir().join(format!("xhpack-{}-{}.xhpack", sanitize(id), nanos()));
    let (pack_id, version, _size, _sha) = crate::market::pack_dir_to_archive(&dir, &out)?;
    Ok((out, pack_id, version))
}

/// 发布版本号校验：`x.y.z` 三段纯数字（与服务端关卡同口径，`1.0.0-beta` / `1.2` 这类不合法），
/// 且须能被 semver 解析（拒绝 `01.2.3` 这类前导零，保证 manifest 里始终是规范 semver，
/// 后续 version_cmp 走 semver 路径而不是数字回退）。
fn validate_new_version(v: &str) -> Result<(), String> {
    let parts: Vec<&str> = v.split('.').collect();
    let shape_ok = parts.len() == 3
        && parts
            .iter()
            .all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()));
    if !shape_ok || semver::Version::parse(v).is_err() {
        return Err(format!("版本号必须是 x.y.z 三段纯数字（如 0.2.1），当前填的是「{v}」"));
    }
    Ok(())
}

/// 把新版本号写回扩展目录的 manifest.json（发布弹窗「发布版本」的落盘动作），返回写回前的旧版本。
///
/// - 只改顶层 `version` 字段：serde_json 开了 `preserve_order`（见 Cargo.toml），键序保持原样，
///   开发者眼里的 diff 只有版本那一行；其余字段（含嵌套对象里碰巧也叫 version 的键）一律不动。
/// - 必须严格大于当前版本（`version_cmp`，semver 语义）——服务端关卡要求版本递增。
/// - 原子落盘：先写 `.` 开头的临时文件再改名覆盖——`pack_dir_to_archive` 排除 `.` 开头项、
///   `dev_extensions_stamp` 的目录树哈希也跳过它们，写一半崩溃既不会把半截文件打进包，也不触发热重载。
pub fn bump_manifest_in_dir(dir: &Path, new_version: &str) -> Result<String, String> {
    validate_new_version(new_version)?;
    let current = crate::extension::read_manifest(dir)?.version;
    if crate::market::version_cmp(new_version, &current) != Ordering::Greater {
        return Err(format!("新版本 {new_version} 必须大于当前 manifest 版本 {current}"));
    }
    let path = dir.join("manifest.json");
    let raw = std::fs::read_to_string(&path).map_err(|e| format!("IO_ERROR: 读取 manifest.json 失败 {e}"))?;
    let mut value: Value = serde_json::from_str(&raw).map_err(|e| format!("manifest 解析失败：{e}"))?;
    match value.get_mut("version") {
        Some(Value::String(s)) => *s = new_version.to_string(),
        _ => return Err("manifest.json 缺少 version 字符串字段".into()),
    }
    let mut out = serde_json::to_string_pretty(&value).map_err(|e| format!("manifest 序列化失败 {e}"))?;
    if raw.ends_with('\n') {
        out.push('\n');
    }
    let tmp = dir.join(".manifest.json.bump-tmp");
    std::fs::write(&tmp, out).map_err(|e| format!("IO_ERROR: 写入 manifest 失败 {e}"))?;
    std::fs::rename(&tmp, &path).map_err(|e| format!("IO_ERROR: 替换 manifest.json 失败 {e}"))?;
    Ok(current)
}

/// 原子写文件：先写 `.` 开头的临时文件再 rename 覆盖（约定 72）。
///
/// ⚠️ 为什么必须「写临时 + rename」而不是直接写：
///    写到一半崩溃/磁盘满会留下**半截文件**，而 manifest.json 半截是解析不了的
///    —— 扩展直接变成「读 manifest 失败」且**没有任何工具能修**。
///    rename(2) 本身即原子替换，不需要先删目标。
fn write_atomic(path: &std::path::Path, bytes: &[u8]) -> std::io::Result<()> {
    let tmp = path.with_extension("json.rollback-tmp");
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, path)
}

/// 上传打包产物：multipart（package 文件 + 展示字段），返回服务端结论
async fn upload(
    base: &str,
    token: &str,
    pkg: &Path,
    // ⚠️ 这两个字段**不是可选的**：服务端 `POST /api/v1/dev/submissions` 拿它们
    // 做版本递增关卡与市场清单条目，缺了直接回 400 `缺少 ext_id 或 version`
    // （2026-10-01 实机：发布弹窗报 BAD_REQUEST，实际是这里根本没发）。
    //
    // 值来自 `pack_to_temp()` 的返回值 —— 它们本来就在手里（打包时读 manifest
    // 得到的），只是没往下传。别再「顺手删掉看着没用���参数」：
    // 端口契约的字段是**服务端要的**，不是本地用不用得到。
    ext_id: &str,
    version: &str,
    changelog: &str,
    min_app_version: &str,
    homepage: &str,
    screenshots: &[String],
) -> Result<Value, String> {
    let bytes = std::fs::read(pkg).map_err(|e| format!("IO_ERROR: 读取安装包失败 {e}"))?;
    let part = reqwest::multipart::Part::bytes(bytes)
        .file_name("ext.xhpack")
        .mime_str("application/zip")
        .map_err(|e| e.to_string())?;
    let mut form = reqwest::multipart::Form::new().part("package", part);
    // 蛇形键名（约定 47：服务端读原始键名、不做驼峰转换）
    form = form.text("ext_id", ext_id.to_string());
    form = form.text("version", version.to_string());
    if !changelog.is_empty() {
        form = form.text("changelog", changelog.to_string());
    }
    if !min_app_version.is_empty() {
        form = form.text("min_app_version", min_app_version.to_string());
    }
    if !homepage.is_empty() {
        form = form.text("homepage", homepage.to_string());
    }
    // 截图：字段名必须是 `screenshots[]` —— 服务端 Hono 的 parseBody **默认只保留同名键的最后一个值**，
    // 只有 `key[]` 形态才会被收集成数组（实测踩过：不带 [] 时传 6 张只到 1 张，且静默通过）。
    for path in screenshots {
        let p = Path::new(path);
        let shot = std::fs::read(p).map_err(|e| format!("IO_ERROR: 读取截图失败 {}：{e}", p.display()))?;
        let mime = match p.extension().and_then(|e| e.to_str()).map(|s| s.to_ascii_lowercase()).as_deref() {
            Some("png") => "image/png",
            Some("webp") => "image/webp",
            _ => "image/jpeg",
        };
        let name = p
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("screenshot")
            .to_string();
        let part = reqwest::multipart::Part::bytes(shot)
            .file_name(name)
            .mime_str(mime)
            .map_err(|e| e.to_string())?;
        form = form.part("screenshots[]", part);
    }

    // 上传可能较慢（几 MB 包），给足超时；代理策略见 crate::net
    let client = crate::net::direct()
        .timeout(std::time::Duration::from_secs(180))
        .build()
        .map_err(|e| e.to_string())?;
    let resp = client
        .post(format!("{}{}", base.trim_end_matches('/'), crate::api_spec::submit_extension_path()))
        .bearer_auth(token)
        .multipart(form)
        .send()
        .await
        .map_err(|e| format!("NETWORK_ERROR: 上传失败 {e}"))?;
    let status = resp.status();
    let text = resp.text().await.unwrap_or_default();
    if !status.is_success() {
        return Err(crate::account::api_error(status, &text));
    }
    serde_json::from_str(&text).map_err(|e| format!("INTERNAL: 响应解析失败 {e}"))
}

/// 发布当前开发中的扩展（可选先回写新版本号 → 打包 → 上传 → 返回关卡结论）
#[tauri::command]
pub async fn dev_submit(
    app: tauri::AppHandle,
    id: String,
    changelog: Option<String>,
    min_app_version: Option<String>,
    homepage: Option<String>,
    screenshots: Option<Vec<String>>,
    // 发布弹窗「发布版本」：非空时先把该版本写回 manifest.json 再打包（须大于当前版本）——
    // 包内 manifest 带上新版本号，服务端的版本递增关卡才认。空 = 按 manifest 当前版本发布。
    new_version: Option<String>,
) -> Result<SubmitResult, String> {
    let token = crate::account::session_token().ok_or("UNAUTHORIZED: 请先在「设置 → 账号」登录")?;
    let base = crate::account::base_url();

    // ⚠️ 版本回写在上传**之前**落盘（打包要读它），但**失败必须回滚**。
    //
    // 实测踩过（2026-10-01）：第一次提交把关卡/配额挡下了，而 manifest.json
    // 已经写成 0.1.4 并留在盘上。用户改完再提交时填 0.1.4 → 服务端回
    // 「新版本 0.1.4 必须大于当前 manifest 版本 0.1.4」——
    // **被自己的残留挡死**，而界面完全看不出 0.1.4 是上一步写进去的。
    //
    // 形状照约定 72：落新 → 尝试 → 失败则恢复原件 → 最后才 commit。
    // 注意回滚要**在错误上追加说明**，否则用户仍以为「版本号非法」。
    let bumped: Option<(std::path::PathBuf, String)> = match new_version
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        Some(v) => {
            let dir = crate::ext_protocol::resolve_ext_dir(&app, &id)?;
            let mpath = dir.join("manifest.json");
            // 先记下原值，供回滚
            let before = std::fs::read_to_string(&mpath).unwrap_or_default();
            let old = bump_manifest_in_dir(&dir, v)?;
            log::info!("发布版本回写: {id} {old} -> {v}");
            Some((mpath, before))
        }
        None => None,
    };

    // 打包 + 上传。**任何一步失败都要先回滚版本**再把错误抛出去。
    //
    // pack_id / version 要在闭包**外**可见：后面的日志与 SubmitResult 还要用。
    // 闭包返回它们与上传结果。
    let mut sent: Option<(String, String)> = None;
    let attempt = async {
        let (pkg, pack_id, version) = pack_to_temp(&app, &id)?;
        log::info!("发布打包完成: {pack_id} v{version} -> {}", pkg.display());
        let shots = screenshots.clone().unwrap_or_default();
        let r: Result<Value, String> = upload(
            &base,
            &token,
            &pkg,
            &pack_id,
            &version,
            changelog.clone().unwrap_or_default().trim(),
            min_app_version.clone().unwrap_or_default().trim(),
            homepage.clone().unwrap_or_default().trim(),
            &shots,
        )
        .await;
        // 无论成败都清掉临时包（源码目录从不被写入）
        let _ = std::fs::remove_file(&pkg);
        // ⚠️ 这里**不能**写成 `Ok((r, ...))` —— 那样 attempt 会变成
        //    `Result<Result<Value,String>, String>`，外层 Ok 里还套一个 Result，
        //    下面 `v.get("gate")` 就编译不过（第一版就是这么写的）。
        //    拍平：直接把 upload 的结果当作闭包的返回值。
        r.map(|val| (val, (pack_id, version)))
    }
    .await;

    let (result, (pack_id, version)) = match attempt {
        Ok(pair) => pair,
        Err(e) => {
            if let Some((mpath, before)) = &bumped {
                if let Err(rb) = write_atomic(mpath, before.as_bytes()) {
                    log::warn!("发布失败且版本回滚失败: {}（原始错误：{e}）", rb);
                    return Err(format!(
                        "{e}\n\n⚠️ 版本号回滚也失败了，请手动检查 {}\n   （manifest.json 可能停留在一个已提交的版本上）",
                        mpath.display()
                    ));
                }
                log::info!("发布失败，已回滚 manifest.json 版本");
                return Err(format!(
                    "{e}\n\n（本次填写的版本号已回滚 —— 它只在你提交成功时才会真正生效。）"
                ));
            }
            return Err(e);
        }
    };
    let v = result;
    log::info!("发布上传完成: {pack_id} v{version}");

    let gate = v.get("gate").cloned().unwrap_or(Value::Null);
    let items: Vec<GateItem> = gate
        .get("items")
        .and_then(|x| serde_json::from_value(x.clone()).ok())
        .unwrap_or_default();
    let gate_passed = gate.get("passed").and_then(|x| x.as_bool()).unwrap_or(false);
    let status = v.get("status").and_then(|x| x.as_str()).unwrap_or("unknown").to_string();
    let failed = items.iter().filter(|i| !i.ok).count();
    log::info!(
        "发布提交完成: {pack_id} v{version} status={status} 关卡={} 未过项={failed}",
        if gate_passed { "通过" } else { "未通过" }
    );

    Ok(SubmitResult {
        id: v.get("id").and_then(|x| x.as_i64()).unwrap_or(0),
        status,
        gate_passed,
        gate_items: items,
        ext_id: pack_id,
        version,
        quota: v.get("quota").cloned(),
    })
}

/// 读取本地图片为 data URL —— **只用于发布弹窗的截图缩略图预览**。
///
/// 作者选中的图片在任意目录，不在资产协议白名单里（见约定 44），`convertFileSrc` 会被拒；
/// 所以在这里读成 base64 回传。**限制大小与类型**，别让它变成任意文件读取通道。
#[tauri::command]
pub fn read_image_data_url(path: String) -> Result<String, String> {
    const MAX_BYTES: u64 = 2 * 1024 * 1024;
    let p = std::path::Path::new(&path);
    let meta = std::fs::metadata(p).map_err(|e| format!("IO_ERROR: 读取失败 {e}"))?;
    if !meta.is_file() {
        return Err("INVALID_ARGUMENT: 不是文件".into());
    }
    if meta.len() > MAX_BYTES {
        return Err("TOO_LARGE: 图片超过 2 MB".into());
    }
    let ext = p
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let mime = match ext.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        _ => return Err("INVALID_ARGUMENT: 仅支持 PNG / JPG / WebP".into()),
    };
    let bytes = std::fs::read(p).map_err(|e| format!("IO_ERROR: 读取失败 {e}"))?;
    use base64::Engine as _;
    let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
    Ok(format!("data:{mime};base64,{b64}"))
}

/// 我的提交列表（含剩余配额：服务端只回剩余次数）
#[tauri::command]
pub async fn dev_list_submissions(page: Option<u32>, page_size: Option<u32>) -> Result<Value, String> {
    let token = crate::account::session_token().ok_or("UNAUTHORIZED: 请先登录")?;
    let query = format!(
        "?page={}&page_size={}",
        page.unwrap_or(1).max(1),
        page_size.unwrap_or(20).clamp(1, 200)
    );
    crate::account::get_json(
        &format!("{}{query}", crate::api_spec::my_submissions_path()),
        &token,
    )
    .await
}

/// 单个提交详情（含关卡逐项结论；**不含** AI 预审报告——那是给审核者看的）
#[tauri::command]
pub async fn dev_get_submission(id: i64) -> Result<Value, String> {
    let token = crate::account::session_token().ok_or("UNAUTHORIZED: 请先登录")?;
    crate::account::get_json(&crate::api_spec::submission_detail_path(id), &token).await
}

/// 撤回自己的提交（审核结束前可用）
#[tauri::command]
pub async fn dev_withdraw_submission(id: i64) -> Result<Value, String> {
    let token = crate::account::session_token().ok_or("UNAUTHORIZED: 请先登录")?;
    let base = crate::account::base_url();
    let (url, body) = crate::api_spec::withdraw_submission(&base, id);
    crate::account::post_json(&url, Some(&token), body).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::mpsc::{channel, Receiver};

    /// 发布失败时 manifest.json 的版本必须**回滚**（2026-10-01 实机事故）。
    ///
    /// 事故形状：版本先落盘（打包要读它），上传被关卡/配额挡下后**没回滚**。
    /// 用户改完再提交时填同一个版本号 → 服务端回「必须大于当前版本 0.1.4」——
    /// **被自己的残留挡死**，而界面完全看不出 0.1.4 是上一步写进去的。
    ///
    /// 判据：模拟「服务端拒绝」之后，磁盘上的 manifest.json 必须回到原样。
    /// 约定 72 的形状 —— 落新 → 尝试 → 失败恢复原件 → 才 commit。
    #[test]
    fn publish_failure_rolls_back_manifest_version() {
        let dir = tempfile::tempdir().unwrap();
        let mpath = dir.path().join("manifest.json");
        // ⚠️ 必须带齐 manifest 必填字段（id/name/version/runtime/kind/surfaces）：
        //    bump_manifest_in_dir 是**真的解析** manifest 的，字段不全会直接拒。
        //    我第一版只写了 id+version，测试红在「manifest 解析失败」——
        //    症状与「回滚逻辑坏了」一样，先确认实现才知道是数据不合规。
        let original = r#"{"id":"com.example","name":"示例","version":"0.1.3",
                          "runtime":"web","kind":"module","surfaces":[]}"#;
        std::fs::write(&mpath, original).unwrap();

        // 先模拟「回写版本」（与 dev_submit 里同一函数）
        let old = bump_manifest_in_dir(dir.path(), "0.1.4").unwrap();
        assert_eq!(old, "0.1.3");
        let after_bump = std::fs::read_to_string(&mpath).unwrap();
        assert!(
            after_bump.contains("0.1.4"),
            "回写没生效，后续断言就没意义"
        );

        // 上传失败 → 回滚
        let rollback = write_atomic(&mpath, original.as_bytes());
        assert!(rollback.is_ok(), "回滚不应失败：{rollback:?}");
        assert_eq!(
            std::fs::read_to_string(&mpath).unwrap(),
            original,
            "发布失败后 manifest.json 必须回到原样 —— 否则用户被自己的残留挡死"
        );
    }

    /// write_atomic 失败时**不能**留下半截文件（约定 72）
    #[test]
    fn write_atomic_leaves_no_partial_file() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("manifest.json");
        std::fs::write(&p, "OLD").unwrap();
        write_atomic(&p, b"NEW-NEW-NEW").unwrap();
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "NEW-NEW-NEW");
        // 临时文件不该残留（它会被 pack_dir_to_archive 排除，但留着仍是垃圾）
        let leftovers: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n.contains("rollback-tmp"))
            .collect();
        assert!(leftovers.is_empty(), "临时文件残留：{leftovers:?}");
    }
    use std::time::Duration;

    fn header_end(buf: &[u8]) -> Option<usize> {
        buf.windows(4).position(|w| w == b"\r\n\r\n")
    }

    fn content_length(head: &str) -> usize {
        head.lines()
            .find_map(|l| {
                let (k, v) = l.split_once(':')?;
                if k.eq_ignore_ascii_case("content-length") {
                    v.trim().parse::<usize>().ok()
                } else {
                    None
                }
            })
            .unwrap_or(0)
    }

    /// 极简 mock 服务端：接一次请求，把「请求行 + 头 + body」原样回传，再返回固定 JSON。
    /// 用它是为了验证**客户端发出去的契约**（路径、鉴权头、multipart 字段名），
    /// 这是只有联调才会暴露、而单测最容易漏掉的一层。
    fn spawn_mock(resp_body: &'static str) -> (String, Receiver<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind mock");
        let addr = listener.local_addr().unwrap();
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                let mut buf: Vec<u8> = Vec::new();
                let mut tmp = [0u8; 16384];
                loop {
                    let n = stream.read(&mut tmp).unwrap_or(0);
                    if n == 0 {
                        break;
                    }
                    buf.extend_from_slice(&tmp[..n]);
                    if let Some(pos) = header_end(&buf) {
                        let head = String::from_utf8_lossy(&buf[..pos]).to_string();
                        let need = pos + 4 + content_length(&head);
                        while buf.len() < need {
                            let n = stream.read(&mut tmp).unwrap_or(0);
                            if n == 0 {
                                break;
                            }
                            buf.extend_from_slice(&tmp[..n]);
                        }
                        break;
                    }
                }
                let _ = tx.send(String::from_utf8_lossy(&buf).to_string());
                let resp = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    resp_body.len(),
                    resp_body
                );
                let _ = stream.write_all(resp.as_bytes());
            }
        });
        (format!("http://{addr}"), rx)
    }

    #[tokio::test]
    async fn upload_sends_the_contract_the_server_expects() {
        let (base, rx) = spawn_mock(
            r#"{"id":7,"status":"pending_review","gate":{"passed":true,"items":[]},"quota":{"drafts_remaining":3}}"#,
        );
        let dir = tempfile::tempdir().unwrap();
        let pkg = dir.path().join("com.example.x-1.0.0.xhpack");
        std::fs::write(&pkg, b"PK\x03\x04 not-a-real-zip-but-fine-for-contract").unwrap();

        let v = upload(
            &base,
            "tok-test",
            &pkg,
            "com.example.x",
            "1.0.0",
            "首版说明",
            "0.5.5",
            "https://example.com/x",
            &[],
        )
        .await
        .expect("上传应当成功");

        assert_eq!(v.get("id").and_then(|x| x.as_i64()), Some(7));
        assert_eq!(
            v.get("status").and_then(|x| x.as_str()),
            Some("pending_review")
        );

        let raw = rx.recv_timeout(Duration::from_secs(5)).expect("mock 未收到请求");
        assert!(
            raw.starts_with("POST /api/v1/dev/submissions "),
            "路径必须与服务端路由一致：{raw}"
        );
        // HTTP 头名大小写不敏感（hyper 实际发出的是小写），这里统一按小写比对
        let lower = raw.to_lowercase();
        assert!(
            lower.contains("authorization: bearer tok-test"),
            "必须带 Bearer 鉴权头：{raw}"
        );
        // 字段名是服务端 parseBody 读取的键，拼错就整单失败
        // ⚠️ 列表里**必须有** `ext_id` 与 `version`（2026-10-01 补）。
        //
        // 这条契约测试在它们缺席的情况下**一直是绿的** —— 它只断言了
        // package/changelog/min_app_version/homepage，而真正的两个必填字段
        // 一个都没查。症状是实机点「打包并发布」报
        // `BAD_REQUEST: 缺少 ext_id 或 version`，服务端一切正常。
        //
        // 教训：mock 服务端**只回固定的 200**，于是「客户端少发字段」这类
        // 错误永远不会被发现 —— 除非那条测试显式断言请求体里有它。
        // 凡是「服务端必填」的字段，都要在这个列表里。
        for field in [
            "package",
            // ↓ 服务端拿它做版本递增关卡与市场清单条目，缺了直接 400
            "ext_id",
            "version",
            "changelog",
            "min_app_version",
            "homepage",
        ] {
            assert!(
                raw.contains(&format!("name=\"{field}\"")),
                "缺少 multipart 字段 {field}：{raw}"
            );
        }
        assert!(raw.contains("filename=\"ext.xhpack\""), "包文件名不符");
        assert!(
            raw.contains("application/zip"),
            "包的 Content-Type 应为 application/zip"
        );
        assert!(raw.contains("首版说明"), "更新说明未随请求发出");
        assert!(raw.contains("0.5.5"), "宿主最低版本未随请求发出");
    }

    #[tokio::test]
    async fn upload_omits_empty_optional_fields() {
        let (base, rx) = spawn_mock(r#"{"id":1,"status":"gate_failed","gate":{"passed":false,"items":[]}}"#);
        let dir = tempfile::tempdir().unwrap();
        let pkg = dir.path().join("p.xhpack");
        std::fs::write(&pkg, b"PK\x03\x04x").unwrap();

        upload(&base, "t", &pkg, "com.example.x", "1.0.0", "", "", "", &[])
            .await
            .expect("上传应当成功");
        let raw = rx.recv_timeout(Duration::from_secs(5)).expect("mock 未收到请求");
        assert!(raw.contains("name=\"package\""));
        for field in ["changelog", "min_app_version", "homepage"] {
            assert!(
                !raw.contains(&format!("name=\"{field}\"")),
                "空的可选字段不应发送：{field}"
            );
        }
    }

    #[tokio::test]
    async fn upload_surfaces_server_error_code() {
        let (base, _rx) = spawn_mock(r#"{"error":"draft_quota_exceeded","message":"待处理的提交过多"}"#);
        let dir = tempfile::tempdir().unwrap();
        let pkg = dir.path().join("p.xhpack");
        std::fs::write(&pkg, b"PK\x03\x04x").unwrap();

        // mock 一律返回 200，这里只验「错误码能被解析出来」的路径；
        // 4xx 分支由服务端 smoke 覆盖（服务端返回的是同样的 JSON 形状）
        let v = upload(&base, "t", &pkg, "com.example.x", "1.0.0", "", "", "", &[])
            .await
            .expect("200 时应当成功解析");
        assert_eq!(
            v.get("error").and_then(|x| x.as_str()),
            Some("draft_quota_exceeded")
        );
    }

    /// 截图的字段名必须是 `screenshots[]`。
    /// 服务端 Hono 的 `parseBody()` 默认只保留**同名键的最后一个值**，只有 `key[]` 形态会被收集成数组 ——
    /// 不带 [] 时多张截图只到 1 张，而且**服务端不会报错**（实测踩过）。
    #[tokio::test]
    async fn upload_sends_screenshots_with_array_field_name() {
        let (base, rx) = spawn_mock(r#"{"id":9,"status":"pending_review","gate":{"passed":true,"items":[]}}"#);
        let dir = tempfile::tempdir().unwrap();
        let pkg = dir.path().join("p.xhpack");
        std::fs::write(&pkg, b"PK\x03\x04x").unwrap();
        let shot1 = dir.path().join("shot1.png");
        let shot2 = dir.path().join("shot2.png");
        // 这里只验字段名与 Content-Type；图片类型由服务端按文件头判定
        std::fs::write(&shot1, b"\x89PNG\r\n\x1a\nnot-a-real-png").unwrap();
        std::fs::write(&shot2, b"\x89PNG\r\n\x1a\nnot-a-real-png").unwrap();

        upload(
            &base,
            "t",
            &pkg,
            "com.example.x",
            "1.0.0",
            "",
            "",
            "",
            &[
                shot1.to_string_lossy().into_owned(),
                shot2.to_string_lossy().into_owned(),
            ],
        )
        .await
        .expect("上传应当成功");

        let raw = rx.recv_timeout(Duration::from_secs(5)).expect("mock 未收到请求");
        assert_eq!(
            raw.matches("name=\"screenshots[]\"").count(),
            2,
            "两张截图应各发一个 screenshots[] part：{raw}"
        );
        assert!(
            !raw.contains("name=\"screenshots\""),
            "不要用不带 [] 的字段名（服务端只会保留最后一个）：{raw}"
        );
        assert!(raw.contains("image/png"), "截图 Content-Type 应为 image/png");
        assert!(
            raw.contains("filename=\"shot1.png\"") && raw.contains("filename=\"shot2.png\""),
            "截图文件名应随请求发出"
        );
    }

    /// 最小 manifest：键序故意非字母序，且嵌套对象里放一个同名 version 键——
    /// 回写只许动顶层 version，其余字段与键序必须原样保留。
    fn write_bump_fixture(dir: &Path, version: &str) {
        let raw = format!(
            r#"{{
  "name": "示例扩展",
  "version": "{version}",
  "id": "com.example.dev",
  "config": {{
    "version": "internal-marker"
  }}
}}
"#
        );
        std::fs::write(dir.join("manifest.json"), raw).unwrap();
    }

    #[test]
    fn bump_writes_new_version_preserving_order_and_nested_fields() {
        let dir = tempfile::tempdir().unwrap();
        write_bump_fixture(dir.path(), "0.1.0");

        let old = bump_manifest_in_dir(dir.path(), "0.2.0").unwrap();
        assert_eq!(old, "0.1.0");

        let raw = std::fs::read_to_string(dir.path().join("manifest.json")).unwrap();
        let v: Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(v["version"], "0.2.0");
        // 嵌套对象里碰巧同名的键不能被误改
        assert_eq!(v["config"]["version"], "internal-marker");
        // 键序保持原样（serde_json preserve_order），开发者的 diff 只有 version 一行
        let keys: Vec<&str> = v.as_object().unwrap().keys().map(String::as_str).collect();
        assert_eq!(keys, vec!["name", "version", "id", "config"]);
        // 末尾换行风格保留；临时文件不残留
        assert!(raw.ends_with('\n'));
        assert!(!dir.path().join(".manifest.json.bump-tmp").exists());
        // 类型化读取（与服务端关卡同一读取口径）拿到新版本
        let m = crate::extension::read_manifest(dir.path()).unwrap();
        assert_eq!(m.version, "0.2.0");
    }

    #[test]
    fn bump_rejects_non_greater_or_malformed_versions() {
        let dir = tempfile::tempdir().unwrap();
        write_bump_fixture(dir.path(), "0.2.0");

        // 相同 / 更小 / 非法格式（非 x.y.z、prerelease、缺段、前导零、带 v 前缀、空串）
        for bad in ["0.2.0", "0.1.9", "1.0.0-beta", "1.2", "v1.0.1", "01.2.4", ""] {
            let err = bump_manifest_in_dir(dir.path(), bad).unwrap_err();
            assert!(!err.is_empty(), "应拒绝「{bad}」");
        }
        // 拒绝后 manifest 原样未动
        let m = crate::extension::read_manifest(dir.path()).unwrap();
        assert_eq!(m.version, "0.2.0");
    }

    #[test]
    fn bump_rejects_missing_manifest() {
        let dir = tempfile::tempdir().unwrap();
        assert!(bump_manifest_in_dir(dir.path(), "1.0.0").is_err());
    }
}
