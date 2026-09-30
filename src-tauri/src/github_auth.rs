//! 客户端直连 GitHub 的设备码登录（不经过 m-hub 服务端）。
//!
//! ## 为什么可以绕开服务端
//!
//! GitHub 的 **Device Flow 根本不需要 `client_secret`** —— 这正是它被设计出来的
//! 用途：给那些没法安全保存密钥的设备（`gh`、git、Docker 登录都走这条）。
//! 旧实现把这段代换放在服务端（`/api/v1/auth/github/device/*`），于是平台服务端
//! 一挂，整条登录就全废；而它在 GitHub 这一段并没有提供任何不可替代的能力。
//!
//! 客户端 ID 本身**不是密钥**（device flow 的 public client），公开嵌进二进制即可。
//!
//! ## 这个登录能得到什么、不能得到什么
//!
//! 能：真的 GitHub 身份（用户名/头像/邮箱）、一个可用的 GitHub 凭据
//!     （以后「从我的仓库装扩展」这类本地功能就靠它）。
//! 不能：平台 AI 额度、申请扩展开发者、发布扩展、市场清单 —— 这些都要服务端
//!     签发的会话。所以设置页必须把那些入口标成「平台服务未连接」，
//!     **不能让用户以为登录了就全能**（拿一个假成功换真投诉）。
//!
//! 凭据与平台的 `session-token` 是**两回事**，分两个钥匙串条目存：
//! 混在一起会出现「退出了 GitHub 却把平台会话也清了」这种反直觉行为。

use serde::Serialize;

/// 钥匙串服务名与条目名。**刻意与平台的 `m-hub-account` / `session-token` 分开**
/// （见模块文档：两个凭据生命周期不同，混存会互相清掉）。
const KEYRING_SERVICE: &str = "m-hub-github";
const TOKEN_KEY: &str = "github-token";
/// 旧版曾把 GitHub token 落到明文文件，升级时顺手清掉
fn legacy_token_file() -> std::path::PathBuf {
    crate::config::config_dir().join("github_token.json")
}

/// GitHub OAuth App 的 Client ID。
///
/// **需要你填**：GitHub → Settings → Developer settings → OAuth Apps → New OAuth App
/// 建好后复制 Client ID。设备码流程不需要 client_secret，所以只有这一项。
pub const GITHUB_CLIENT_ID: &str = "";

// ---------------- 对外的类型 ----------------

/// 第一步的结果：让用户去浏览器输码
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GithubDeviceStart {
    /// 后续轮询用的凭据（**不要**展示给用户）
    pub device_code: String,
    /// 展示给用户的码，如 `ABCD-1234`
    pub user_code: String,
    /// 让用户打开的地址
    pub verification_uri: String,
    /// 建议的轮询间隔（秒）
    pub interval: u64,
    /// 有效期（秒）
    pub expires_in: u64,
}

/// 轮询一步的结果
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum GithubPoll {
    /// 还在等用户去授权
    Pending,
    /// 授权完成
    Done,
    /// 出错了（`message` 已译成中文）
    Failed { message: String },
}

/// GitHub 身份
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GithubIdentity {
    pub login: String,
    pub name: String,
    pub avatar_url: String,
    pub email: String,
    /// 账号主页
    pub html_url: String,
}

/// 本地登录态（**只表示「这台机器上有某个 GitHub 身份的凭据」**，
/// 不表示通过了任何平台校验）
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GithubStatus {
    pub logged_in: bool,
    /// 未配置 Client ID 时为 true —— 界面据此显示「先去 GitHub 建一个 OAuth App」
    pub needs_client_id: bool,
    pub identity: Option<GithubIdentity>,
}

// ---------------- 凭据存取 ----------------

fn save_token(token: &str) -> Result<(), String> {
    // 旧明文文件：迁移完就删，不留一份在磁盘上
    let legacy = legacy_token_file();
    if legacy.exists() {
        let _ = std::fs::remove_file(&legacy);
    }
    keyring::Entry::new(KEYRING_SERVICE, TOKEN_KEY)
        .map_err(|_| "系统钥匙串不可用，未保存 GitHub 凭据".to_string())?
        .set_password(token)
        .map_err(|e| format!("钥匙串写入失败: {e}"))
}

fn load_token() -> Option<String> {
    keyring::Entry::new(KEYRING_SERVICE, TOKEN_KEY)
        .ok()
        .and_then(|e| e.get_password().ok())
        .filter(|s| !s.is_empty())
}

fn clear_token() {
    if let Ok(entry) = keyring::Entry::new(KEYRING_SERVICE, TOKEN_KEY) {
        let _ = entry.delete_credential();
    }
    let _ = std::fs::remove_file(legacy_token_file());
}

// ---------------- 状态 ----------------

pub fn client_id_configured() -> bool {
    !GITHUB_CLIENT_ID.trim().is_empty()
}

/// 当前登录态。token 存在即视为已登录；拿不到身份信息时 `identity` 为 None
/// （凭据可能已被 GitHub 撤销，界面据此提示重新登录）。
pub async fn status() -> GithubStatus {
    if !client_id_configured() {
        return GithubStatus {
            logged_in: false,
            needs_client_id: true,
            identity: None,
        };
    }
    let Some(token) = load_token() else {
        return GithubStatus {
            logged_in: false,
            needs_client_id: false,
            identity: None,
        };
    };
    let identity = match fetch_identity(&token).await {
        Ok(id) => Some(id),
        Err(e) => {
            log::warn!("GitHub 身份读取失败（凭据可能已失效）: {e}");
            None
        }
    };
    GithubStatus {
        logged_in: identity.is_some(),
        needs_client_id: false,
        identity,
    }
}

/// 退出：清掉 GitHub 凭据（**不动**平台的 session-token）
pub fn logout() -> GithubStatus {
    clear_token();
    GithubStatus {
        logged_in: false,
        needs_client_id: !client_id_configured(),
        identity: None,
    }
}

// ---------------- 与 GitHub 通信 ----------------

fn client() -> reqwest::Client {
    // 流式登录接口不该被整体超时砍掉；给一个宽松的兜底即可
    reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .user_agent("m-hub")
        .build()
        .unwrap_or_default()
}

fn missing_client_id() -> String {
    "还没配置 GitHub Client ID：到 GitHub → Settings → Developer settings → OAuth Apps → \
     New OAuth App 建一个，把 Client ID 填进 src-tauri/src/github_auth.rs 的 GITHUB_CLIENT_ID"
        .into()
}

/// GitHub 建议的轮询间隔 → 实际用的间隔（秒）。
///
/// 下限 5s：GitHub 文档要求不低于它给的 `interval`，低于会被限流；而它偶尔会
/// 给 0 或 1（缺字段时我们用默认值），照抄就等于自己按 0 间隔狂打接口。
pub fn poll_interval(github_says: Option<u64>) -> u64 {
    github_says.unwrap_or(5).max(5)
}

/// 第一步：换一对设备码。
pub async fn device_start() -> Result<GithubDeviceStart, String> {
    let cid = GITHUB_CLIENT_ID.trim();
    if cid.is_empty() {
        return Err(missing_client_id());
    }
    let resp = client()
        .post("https://github.com/login/device/code")
        // 不带这个头时 GitHub 返回 form-urlencoded，这里要 JSON
        .header("Accept", "application/json")
        .form(&[("client_id", cid), ("scope", "read:user user:email")])
        .send()
        .await
        .map_err(|e| format!("连不上 GitHub：{e}"))?;
    let body: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("GitHub 返回内容无法解析：{e}"))?;

    let field = |k: &str| body.get(k).and_then(|v| v.as_str()).unwrap_or("").to_string();
    let device_code = field("device_code");
    if device_code.is_empty() {
        let msg = field("error_description");
        return Err(if msg.is_empty() {
            "GitHub 没有下发设备码".to_string()
        } else {
            msg
        });
    }
    let interval = poll_interval(body.get("interval").and_then(|v| v.as_u64()));
    let expires_in = body.get("expires_in").and_then(|v| v.as_u64()).unwrap_or(900);
    Ok(GithubDeviceStart {
        device_code,
        user_code: field("user_code"),
        // GitHub 现在给的是 verification_uri_complete（带码的直连地址），
        // 但对用户展示裸地址更清楚，让他自己输码
        verification_uri: if field("verification_uri").is_empty() {
            "https://github.com/login/device".into()
        } else {
            field("verification_uri")
        },
        interval,
        expires_in,
    })
}

/// 轮询一步。`device_code` 从上一步拿。
pub async fn device_poll(device_code: &str) -> Result<GithubPoll, String> {
    let cid = GITHUB_CLIENT_ID.trim();
    if cid.is_empty() {
        return Err(missing_client_id());
    }
    if device_code.trim().is_empty() {
        return Err("缺少设备码".into());
    }
    let resp = client()
        .post("https://github.com/login/oauth/access_token")
        .header("Accept", "application/json")
        .form(&[
            ("client_id", cid),
            ("device_code", device_code),
            ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
        ])
        .send()
        .await
        .map_err(|e| format!("连不上 GitHub：{e}"))?;
    let body: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("GitHub 返回内容无法解析：{e}"))?;

    let err = body.get("error").and_then(|v| v.as_str()).unwrap_or("");
    if let Some(v) = verdict_of(err) {
        return Ok(v);
    }

    let token = body
        .get("access_token")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    if token.is_empty() {
        return Ok(GithubPoll::Pending);
    }
    let identity = fetch_identity(token).await?;
    save_token(token)?;
    Ok(GithubPoll::Done)
}

/// 用 token 读一次 `/user`，顺便验证凭据还有效。
async fn fetch_identity(token: &str) -> Result<GithubIdentity, String> {
    let resp = client()
        .get("https://api.github.com/user")
        .header("Authorization", format!("Bearer {token}"))
        .header("Accept", "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2022-11-28")
        .send()
        .await
        .map_err(|e| format!("读 GitHub 身份失败：{e}"))?;

    if resp.status() == reqwest::StatusCode::UNAUTHORIZED {
        return Err("GitHub 凭据已失效，请重新登录".into());
    }
    let body: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("GitHub 返回内容无法解析：{e}"))?;
    let s = |k: &str| body.get(k).and_then(|v| v.as_str()).unwrap_or("").to_string();
    let login = s("login");
    if login.is_empty() {
        return Err("GitHub 没返回用户信息".into());
    }
    // 私有邮箱时 /user 的 email 为空，补一次 /user/emails 拿主邮箱
    let mut email = s("email");
    if email.is_empty() {
        if let Ok(r) = client()
            .get("https://api.github.com/user/emails")
            .header("Authorization", format!("Bearer {token}"))
            .header("Accept", "application/vnd.github+json")
            .header("X-GitHub-Api-Version", "2022-11-28")
            .send()
            .await
        {
            if let Ok(v) = r.json::<serde_json::Value>().await {
                if let Some(arr) = v.as_array() {
                    if let Some(primary) = arr
                        .iter()
                        .find(|e| e.get("primary").and_then(|p| p.as_bool()).unwrap_or(false))
                        .or_else(|| arr.first())
                    {
                        email = primary
                            .get("email")
                            .and_then(|e| e.as_str())
                            .unwrap_or("")
                            .to_string();
                    }
                }
            }
        }
    }
    let name = {
        let n = s("name");
        if n.is_empty() {
            s("login")
        } else {
            n
        }
    };
    Ok(GithubIdentity {
        login,
        name,
        avatar_url: s("avatar_url"),
        email,
        html_url: s("html_url"),
    })
}

/// GitHub 轮询响应里的 `error` 字段 → 我们该怎么反应。**纯函数**。
///
/// 返回 `None` 表示「没有 error 字段」，由调用方继续看 `access_token`。
///
/// 这段是本模块唯一会判错的地方，而判错的后果都很隐蔽：
/// 把 `authorization_pending` 当失败 → 用户刚点确认就看到「失败」；
/// 把 `expired_token` 当成「继续等」→ 界面永远转圈，用户以为程序卡了。
/// 而它每次返回的错误码组合是 GitHub 决定的，我们造不出来，所以单测只能覆盖
/// **分类逻辑**本身（用构造的 JSON），覆盖不到 GitHub 真实返回的组合。
fn verdict_of(err: &str) -> Option<GithubPoll> {
    match err {
        // 用户还没在浏览器里确认：正常，继续等
        "authorization_pending" | "slow_down" => Some(GithubPoll::Pending),
        // 码过期 / 被用过：换不回 token 了，得重来
        "expired_token" => Some(GithubPoll::Failed {
            message: "这个码已过期，请重新发起登录".into(),
        }),
        "access_denied" => Some(GithubPoll::Failed {
            message: "你取消了授权".into(),
        }),
        // 没有 error 字段 → 继续看有没有 access_token
        "" => None,
        other => {
            // 未知错误码：把 GitHub 给的描述原样带出去，别吞掉
            Some(GithubPoll::Failed {
                message: format!("GitHub 拒绝了这次授权：{other}"),
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn is_pending(v: &GithubPoll) -> bool {
        matches!(v, GithubPoll::Pending)
    }

    #[test]
    fn pending_codes_are_not_failures() {
        // 这两个码是「正常等待」，判成失败会让用户刚点确认就看到报错
        assert!(is_pending(&verdict_of("authorization_pending").unwrap()));
        assert!(is_pending(&verdict_of("slow_down").unwrap()));
    }

    #[test]
    fn expired_and_denied_are_terminal() {
        // 判成「继续等」的话界面会永远转圈，用户以为程序卡死
        assert!(matches!(
            verdict_of("expired_token"),
            Some(GithubPoll::Failed { .. })
        ));
        assert!(matches!(
            verdict_of("access_denied"),
            Some(GithubPoll::Failed { .. })
        ));
    }

    #[test]
    fn no_error_field_defers_to_access_token_check() {
        assert!(verdict_of("").is_none());
    }

    #[test]
    fn unknown_code_is_surfaced_not_swallowed() {
        // 未知码不能当成「继续等」，也不能静默 —— 用户会卡在转圈
        match verdict_of("some_new_github_error") {
            Some(GithubPoll::Failed { message }) => {
                assert!(message.contains("some_new_github_error"), "实际: {message}")
            }
            other => panic!("未知错误码必须判失败，实际: {other:?}"),
        }
    }

    #[test]
    fn poll_interval_never_goes_below_five_seconds() {
        // GitHub 偶尔给 0/1，或缺字段（→ None）。照抄就等于狂打接口被限流
        assert_eq!(poll_interval(None), 5);
        assert_eq!(poll_interval(Some(0)), 5);
        assert_eq!(poll_interval(Some(1)), 5);
        // 给的间隔更大时必须尊重它，不能压到 5s
        assert_eq!(poll_interval(Some(30)), 30);
    }

    /// Client ID 填错是**最可能**的填错方式：把 client_secret 粘进来、
    /// 或粘了一整行 `client_id=xxx`。那不会编译失败，只会在 GitHub 那边
    /// 报一个看不懂的错误。所以这里直接按形状拦一道。
    #[test]
    fn client_id_is_empty_or_a_40_hex_string() {
        let id = GITHUB_CLIENT_ID.trim();
        if id.is_empty() {
            return; // 还没填：合法状态
        }
        assert_eq!(
            id.len(),
            40,
            "GitHub OAuth App 的 Client ID 是 40 位十六进制；这一串长度 {}，\
             多半是粘错了（client_secret / 带前缀 / 带了空格）",
            id.len()
        );
        assert!(
            id.chars().all(|c| c.is_ascii_hexdigit()),
            "Client ID 只应含十六进制字符，实际: {id:?}"
        );
    }
}
