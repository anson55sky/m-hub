//! 出网客户端构造：区分「连自己服务器」与「第三方」两类。
//!
//! ## 前提已经翻转（2026-09-30，务必先读这段再改）
//!
//! 本模块原先的理由是：*「我们自己服务器（**国内** IP）经代理转发时 TLS 握手直接失败」*
//! —— 所以 `net::direct()` 强制直连。
//!
//! 那个前提**今天不成立了**。平台服务端已迁到 Cloudflare Workers
//! （`*.workers.dev`），而 `workers.dev` 在部分网络里**被 DNS 污染**：实测同一台机器上
//! 四个 DNS（本地 / 1.1.1.1 / 8.8.8.8 / 223.5.5.5）对同一域名给出**四个不同的假 IP**
//! （31.13.70.13 / 162.125.7.1 / 157.240.21.9 / 150.107.3.176，全是 Facebook、Dropbox
//! 之类无关段），两次查询的假 IP 还不同（随机化 = 污染特征）。
//! 对照组：`www.cloudflare.com` 正常解析到 104.16.124.96 并返回 200 —— 即
//! **Cloudflare 的 IP 段没被封，污染是针对 `workers.dev` 这个域名的**。
//!
//! 于是原先的策略从「直连是唯一出路」翻转成「**直连和代理都要试**」。
//!
//! ## 为什么不能简单地「去掉 no_proxy()」
//!
//! reqwest 只认**环境变量**代理（`HTTP_PROXY` / `HTTPS_PROXY` / `ALL_PROXY`），
//! **不读 macOS「系统设置 → 网络 → 代理」**。而 Clash / Surge / Shadowrocket 的
//! 「系统代理」模式写的正是系统设置（它们把流量导向本地 HTTP 端口，
//! 例如 ClashX 7890、Surge 6152），**不设环境变量** —— 于是对绝大多数用户来说，
//! 只去掉 `no_proxy()` 等于什么都没改。
//!
//! 所以 [`system_proxy_url`] 去读系统的实际代理设置（`scutil --proxy`），
//! 再由 [`client`] 显式装配。**两条路都通**：有代理走代理、没代理直连。
//!
//! ## 为什么「有代理就走代理」而不是两种都探、哪个通用哪个
//!
//! 每请求探两条路 = 连接超时翻倍（现在 20s → 40s），而本模块的调用点里
//! 有**后台轮询**（更新检查每 4h 一次）与**用户主动等待**（手动检查更新、
//! 发消息），一次多余的 20s 卡顿是用户能感觉到的。
//!
//! 代价是「服务器直连可达时多一跳」。这个代价可接受：系统代理的存在本身
//! 就说明用户有走代理的意图，而约定的原始顾虑（「别被本地代理带沟里」）
//! 建立在「服务器在国内」这个**现已不成立**的前提上。
//!
//! ## 第三方服务维持原样
//!
//! 用户自配的 AI 服务、天气等保持 reqwest 默认（跟随系统代理）——
//! 它们在外网、可能恰恰**需要**代理才能访问。

/// 「连自己服务器」的客户端构造器。
///
/// **不再**无条件 `no_proxy()`：见模块头的前提翻转说明。
/// 函数名保留 `direct` 是为了少改调用点 —— 但它现在的含义已不是「直连」，
/// 而是「直连优先、代理兜底」。新代码请用名字更准的 [`client`]。
pub fn direct() -> reqwest::ClientBuilder {
    client()
}

/// 连平台服务端的客户端构造器。
///
/// 策略：**有系统代理就走代理、没配代理就直连**，两条路都通。
///
/// ## 为什么用 `Proxy::custom` 而不是「两个 proxy 都加上」
///
/// reqwest 0.12.28 **没有** `fallback_proxy`（查过 `src/async_impl/client.rs`
/// 与 `src/proxy.rs`，只有 `proxy()` 与 `no_proxy()`）。而 `Proxy::custom`
/// 收一个 `Fn(&Url) -> Option<Proxy>`：返回 `Some` 走代理、返回 `None` 直连 ——
/// 这正是我们要的「二选一」，且是 reqwest 公开支持的写法。
///
/// ⚠️ 代价要说清：`Proxy::custom` 决定的是**「用不用代理」**，不是
/// 「代理失败后重试直连」。若代理能建连但返回 407/502，请求会如实带着那个
/// 错误返回，不会自动改走直连。这是对的 —— 代理确实在应答，回退与否该由
/// 服务端错误处理判断，不该由传输层替它猜。
pub fn client() -> reqwest::ClientBuilder {
    match system_proxy_url() {
        Some(p) => {
            log::info!("检测到系统代理（{p}），平台服务端请求走代理");
            match reqwest::Proxy::all(&p) {
                Ok(px) => reqwest::Client::builder().proxy(px),
                Err(e) => {
                    log::warn!("系统代理地址不可用（{p}: {e}），本次改走直连");
                    reqwest::Client::builder().no_proxy()
                }
            }
        }
        // 没有代理配置：保持 `no_proxy()`，显式表达「不走环境变量代理」。
        // 与 reqwest 默认行为的区别在极端情况下才有（env 里有陈旧代理配置，
        // 而系统设置里没开）—— 那时直连是我们想要的。
        None => reqwest::Client::builder().no_proxy(),
    }
}

/// 读 macOS 的**系统代理设置**，返回形如 `http://127.0.0.1:7890` 的 URL。
///
/// ## 为什么要自己读
///
/// reqwest 只认环境变量。Clash / Surge / Shadowrocket 的「系统代理」模式
/// 只改系统设置、不设环境变量 —— 这是绝大多数国内用户的实际用法。
/// 不读系统设置，等于对这些人完全无效。
///
/// ## 实现选择：`scutil --proxy` 而不是 CFNetwork 直调
///
/// `CFNetworkCopySystemProxySettings` 要新起一个 `SystemConfiguration` 依赖，
/// 而 `scutil --proxy` 是系统自带、输出稳定、**零新增依赖**。
/// 它是子进程调用（~10-30ms），但只在**建客户端时**调一次，不是轮询路径。
///
/// 另：`SCDynamicStore` 的键在部分系统上会因隐私设置不可读，
/// `scutil` 走的是同一条受支持的路径，稳。
#[cfg(target_os = "macos")]
pub fn system_proxy_url() -> Option<String> {
    use std::process::Command;
    let out = Command::new("/usr/sbin/scutil")
        .arg("--proxy")
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout);

    // scutil --proxy 的输出（ClashX 系统代理模式的真实形状）：
    //   <dictionary> {
    //     HTTPEnable : 0
    //     HTTPSEnable : 1
    //     HTTPSPort : 31181
    //     HTTPSProxy : 127.0.0.1
    //     ProxyAutoConfigEnable : 0
    //     SOCKSEnable : 0
    //   }
    //
    // ⚠️ 键名**逐条写死**，不做「从 Enable 键名推导 Proxy 键名」的切片。
    //   推导写法在 HTTPS 上碰巧成立（`"HTTPSEnable"[..4]` = "HTTPS"），
    //   但对 SOCKS 会切出 `"SOCK"` —— 而键名是 `SOCKSProxy`，得到 None，
    //   于是 SOCKS 代理**静默失效**。踩过。
    //   另外 `ExceptionsList` / `ProxyAutoConfigEnable` / `FTPPassive` 这些键
    //   与目标键共享前缀，朴素 `strip_prefix` 有匹配串味的风险，
    //   故取值时要求「冒号前恰好等于该键」。
    const CANDIDATES: &[(&str, &str, &str, &str, u16)] = &[
        // (Enable 键, Proxy 键, Port 键, scheme, 缺省端口)，顺序即优先级
        ("HTTPSEnable", "HTTPSProxy", "HTTPSPort", "http", 443),
        ("HTTPEnable", "HTTPProxy", "HTTPPort", "http", 80),
        ("SOCKSEnable", "SOCKSProxy", "SOCKSPort", "socks5h", 1080),
    ];
    for (enable_key, proxy_key, port_key, scheme, default_port) in CANDIDATES {
        if scutil_value(&text, enable_key).as_deref() != Some("1") {
            continue;
        }
        let Some(host) = scutil_value(&text, proxy_key).filter(|h| !h.is_empty()) else {
            // 开着开关却没给主机 = 配置坏了，继续试下一个候选而不是直接返回 None
            log::warn!("系统代理 {enable_key}=1 但 {proxy_key} 为空，跳过该协议");
            continue;
        };
        let port = scutil_value(&text, port_key)
            .and_then(|p| p.parse::<u16>().ok())
            .unwrap_or(*default_port);
        return Some(format!("{scheme}://{host}:{port}"));
    }
    None
}

/// 非 macOS 平台：只认环境变量（交给 reqwest 默认行为处理）。
#[cfg(not(target_os = "macos"))]
pub fn system_proxy_url() -> Option<String> {
    for k in ["HTTPS_PROXY", "https_proxy", "ALL_PROXY", "all_proxy", "HTTP_PROXY", "http_proxy"] {
        if let Ok(v) = std::env::var(k) {
            if !v.is_empty() {
                return Some(v);
            }
        }
    }
    None
}

/**
 * 从 `scutil --proxy` 输出里取 `Key : value` 的 value。
 *
 * ⚠️ 要求**冒号前恰好等于该键**，而不是「以该键开头」：
 * 真实输出里 `ProxyAutoConfigEnable` 与 `Proxy…`、`ExceptionsList` 与 `Exception…`
 * 都会让朴素 `strip_prefix` 串味。
 */
#[cfg(target_os = "macos")]
fn scutil_value(text: &str, key: &str) -> Option<String> {
    for line in text.lines() {
        let t = line.trim();
        let Some((k, v)) = t.split_once(':') else { continue };
        if k.trim() == key {
            return Some(v.trim().to_string());
        }
    }
    None
}

/// 供诊断用：代理配置的一句话摘要（**不含**任何凭据）。
pub fn proxy_diagnostic() -> String {
    match system_proxy_url() {
        Some(p) => format!("系统代理 = {p}"),
        None => "未检测到系统代理（直连）".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn system_proxy_url_is_none_or_looks_like_a_url() {
        // 真机上可能读到真代理，也可能没有 —— 两种都必须不 panic，
        // 且读到的值必须是 reqwest 能接受的 URL 形态
        if let Some(p) = system_proxy_url() {
            assert!(
                p.starts_with("http://") || p.starts_with("socks5h://"),
                "代理地址必须带协议头，否则 reqwest 认不出: {p}"
            );
            assert!(!p.contains('@'), "代理 URL 里不该带凭据（会进日志）: {p}");
        }
    }

    #[test]
    fn diagnostic_never_leaks_credentials() {
        let d = proxy_diagnostic();
        assert!(!d.contains('@'), "诊断输出不得含凭据: {d}");
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn scutil_parsing_handles_the_real_output_shape() {
        // 贴一份真实的 scutil --proxy 输出形状（ClashX 系统代理模式）
        let text = "<dictionary> {\n  ExceptionsList : <array> {\n  }\n  FTPPassive : 1\n  HTTPEnable : 1\n  HTTPPort : 6152\n  HTTPProxy : 127.0.0.1\n  HTTPSEnable : 1\n  HTTPSPort : 6152\n  HTTPSProxy : 127.0.0.1\n  SOCKSEnable : 0\n  SOCKSPort : 1080\n  SOCKSProxy : 127.0.0.1\n}";
        assert_eq!(scutil_value(text, "HTTPSEnable").as_deref(), Some("1"));
        assert_eq!(scutil_value(text, "HTTPSProxy").as_deref(), Some("127.0.0.1"));
        assert_eq!(scutil_value(text, "HTTPSPort").as_deref(), Some("6152"));
        // 关闭的 SOCKS 不该被当成可用
        assert_eq!(scutil_value(text, "SOCKSEnable").as_deref(), Some("0"));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn scutil_parsing_ignores_unrelated_keys_with_shared_prefix() {
        // 真实输出里 ExceptionsList / FTPPassive 等键与目标键共享前缀，
        // 朴素 `strip_prefix` 可能匹配错 —— 这条钉住取值不会串味
        let text = "<dictionary> {\n  HTTPSEnable : 1\n  HTTPSProxy : 10.0.0.1\n  HTTPSPort : 8443\n  SOCKSProxy : 10.0.0.2\n}";
        assert_eq!(scutil_value(text, "SOCKSProxy").as_deref(), Some("10.0.0.2"));
        // HTTPSPort 只该匹配自己，不该被 HTTPSProxy 的前缀吃掉
        assert_eq!(scutil_value(text, "HTTPSPort").as_deref(), Some("8443"));
        // 共享前缀的键必须取不到（否则会把 ProxyAutoConfigEnable 当成 Proxy…）
        let pac = "<dictionary> {\n  ProxyAutoConfigEnable : 1\n  ProxyAutoConfigURLString : http://x\n}";
        assert_eq!(scutil_value(pac, "Proxy"), None, "不得匹配到前缀相同的其它键");
        assert_eq!(
            scutil_value(pac, "ProxyAutoConfigURLString").as_deref(),
            Some("http://x")
        );
    }
}
