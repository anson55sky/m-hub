//! 客户端 → m-hub-server 的**请求规格**（路径与请求体字段名的唯一来源）。
//!
//! 为什么单独抽出来：这些路径与字段名必须与服务端路由 / `parseBody` 读的键**完全一致**，
//! 而它们属于「只有联调才会暴露」的一层 —— 两端各自的测试都是绿的，一对接就 404 或 400。
//! 集中在一处 + 单测断言，改动时不容易只改一边。
//!
//! 注意字段名是**蛇形**（`poll_id` / `min_app_version`）：服务端 Hono 读的是原始键名，
//! 不做驼峰转换。

use serde_json::{json, Value};

/// 请求规格：`(完整 URL, JSON 请求体)`
pub type Spec = (String, Value);

fn base_of(base: &str) -> &str {
    base.trim_end_matches('/')
}

// ---------------- 登录（GitHub 设备码流程） ----------------

pub fn github_device_start(base: &str) -> Spec {
    (format!("{}/api/v1/auth/github/device/start", base_of(base)), json!({}))
}

pub fn github_device_poll(base: &str, poll_id: &str) -> Spec {
    (
        format!("{}/api/v1/auth/github/device/poll", base_of(base)),
        json!({ "poll_id": poll_id }),
    )
}

// ---------------- 登录（邮箱验证码） ----------------

pub fn email_send(base: &str, email: &str) -> Spec {
    (
        format!("{}/api/v1/auth/email/send", base_of(base)),
        json!({ "email": email }),
    )
}

pub fn email_verify(base: &str, email: &str, code: &str) -> Spec {
    (
        format!("{}/api/v1/auth/email/verify", base_of(base)),
        json!({ "email": email, "code": code }),
    )
}

// ---------------- 账号与权益 ----------------

/// 兑换邀请码（账号与权益解耦：兑换后才发额度、才可申请开发者）
pub fn redeem_invite(base: &str, code: &str) -> Spec {
    (format!("{}/api/v1/me/redeem", base_of(base)), json!({ "code": code }))
}

/// 开发者申请（理由会展示给审核者）
pub fn dev_apply(base: &str, reason: &str) -> Spec {
    (
        format!("{}/api/v1/dev/apply", base_of(base)),
        json!({ "reason": reason }),
    )
}

/// 开发者申请状态（GET）
pub fn dev_apply_status_path() -> &'static str {
    "/api/v1/dev/apply"
}

/// 我的在线设备（GET）
pub fn device_tokens_path() -> &'static str {
    "/api/v1/me/device-tokens"
}

/// 撤销某台设备
/// ⚠️ `id` 是 **token 前 8 位**（服务端 `r.token.slice(0, 8)`），不是数据库行号 ——
///    必须是 `&str`。原先声明成 `i64`，而前端也确实按 number 传，于是
///    「撤销设备」这条链路两端都传着数字，服务端那边永远匹配不到任何会话。
///    撤销路由按 `token LIKE '<prefix>%'` 匹配，且**必须**限定 `user_id`。
pub fn device_revoke(base: &str, id: &str) -> Spec {
    (
        format!("{}/api/v1/me/device-tokens/{id}/revoke", base_of(base)),
        json!({}),
    )
}

// ---------------- 平台 AI 额度 ----------------

/// 平台可用模型（GET；登录后「使用平台免费额度」的入口）
pub fn platform_models_path() -> &'static str {
    "/api/v1/ai/models"
}

// ---------------- 扩展发布 ----------------

pub fn submit_extension_path() -> &'static str {
    "/api/v1/dev/submissions"
}

pub fn my_submissions_path() -> &'static str {
    "/api/v1/dev/submissions"
}

pub fn submission_detail_path(id: i64) -> String {
    format!("/api/v1/dev/submissions/{id}")
}

pub fn withdraw_submission(base: &str, id: i64) -> Spec {
    (
        format!("{}/api/v1/dev/submissions/{id}/withdraw", base_of(base)),
        json!({}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_match_the_server_routes() {
        let b = "https://x.example";
        assert_eq!(github_device_start(b).0, "https://x.example/api/v1/auth/github/device/start");
        assert_eq!(github_device_poll(b, "p").0, "https://x.example/api/v1/auth/github/device/poll");
        assert_eq!(email_send(b, "a@b.c").0, "https://x.example/api/v1/auth/email/send");
        assert_eq!(email_verify(b, "a@b.c", "123456").0, "https://x.example/api/v1/auth/email/verify");
        assert_eq!(redeem_invite(b, "CODE").0, "https://x.example/api/v1/me/redeem");
        assert_eq!(dev_apply(b, "reason").0, "https://x.example/api/v1/dev/apply");
        assert_eq!(dev_apply_status_path(), "/api/v1/dev/apply");
        assert_eq!(device_tokens_path(), "/api/v1/me/device-tokens");
        // 用真实的 token 前缀形态（8 位十六进制），别用 7 —— 数字样例会让
        // 「id 到底是行号还是字符串前缀」这件事在测试里看不出来
        assert_eq!(
            device_revoke(b, "a1b2c3d4").0,
            "https://x.example/api/v1/me/device-tokens/a1b2c3d4/revoke"
        );
        assert_eq!(platform_models_path(), "/api/v1/ai/models");
        assert_eq!(submit_extension_path(), "/api/v1/dev/submissions");
        assert_eq!(submission_detail_path(9), "/api/v1/dev/submissions/9");
        assert_eq!(withdraw_submission(b, 9).0, "https://x.example/api/v1/dev/submissions/9/withdraw");
    }

    #[test]
    fn bodies_use_the_snake_case_keys_the_server_reads() {
        // 这些键名一旦拼错，服务端读不到参数 → 400/401，且只有联调才会暴露
        assert_eq!(github_device_poll("b", "dev-code").1, json!({ "poll_id": "dev-code" }));
        assert_eq!(email_send("b", "a@b.c").1, json!({ "email": "a@b.c" }));
        assert_eq!(email_verify("b", "a@b.c", "000000").1, json!({ "email": "a@b.c", "code": "000000" }));
        assert_eq!(redeem_invite("b", "X").1, json!({ "code": "X" }));
        assert_eq!(dev_apply("b", "想做一个扩展").1, json!({ "reason": "想做一个扩展" }));
        // 无参数端点必须是空对象（服务端会 await c.req.json().catch(() => null)，空对象最稳）
        assert_eq!(github_device_start("b").1, json!({}));
        assert_eq!(device_revoke("b", "a1b2c3d4").1, json!({}));
        assert_eq!(withdraw_submission("b", 1).1, json!({}));
    }

    #[test]
    fn base_trailing_slash_is_normalized() {
        // 用户在设置里填了带斜杠的地址也要能用
        assert_eq!(
            github_device_start("https://x.example/").0,
            "https://x.example/api/v1/auth/github/device/start"
        );
    }
}
