// 网页 favicon 抓取：新建/导入网页速达时自动配图标。
//
// ## 为什么要有它
//
// 原来只是**推导**出一个 `${origin}/favicon.ico` 填进表单（`utils/web.ts` 的
// `deriveFaviconUrl`）。那不是「自动取图标」——
// · 它假定站点 favicon 就在根路径（大量站点不是：GitHub 的在 `/favicon.ico`
//   但 VitePress 之类在 assets 下，抖音/小红书干脆 403）；
// · 更要命的是**抓回来的字节没落盘**。`icon` 字段被前端当 URL 用，
//   而速达的图标最终要经 `convertFileSrc` 从数据根读 —— 一个外站 URL
//   会被资产协议的白名单拒掉，于是图标**永远显示不出来**。
//
// 所以这里必须把字节抓下来、存进数据根的 `icons/`，返回**本地路径**。
//
// ## 抓不到时返回 None，**不报错**
//
// 取不到 favicon 是绝大多数站点的常态（403、超时、非图片格式）。
// 让它抛错会把「加一个网页速达」变成一件需要重试的事 —— 而界面上已经有
// `.suda-letter` 首字母兜底（`useResourceIcon`），够用。
// 首字母不是妥协：它对「自己常去的十几个站点」比一堆错图标更整齐。

/// 单个图标最大 512 KB —— favicon 通常几 KB，超过的多半是 HTML 错误页
const MAX_ICON_BYTES: usize = 512 * 1024;
const FETCH_TIMEOUT_SECS: u64 = 8;

#[tauri::command]
pub async fn fetch_web_favicon(url: String) -> Result<Option<String>, String> {
    let trimmed = url.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    // 只认 http/https：javascript: / file: / data: 都可能被拿来读本地文件
    if !(trimmed.starts_with("http://") || trimmed.starts_with("https://")) {
        return Ok(None);
    }

    let base = match parse_origin(trimmed) {
        Some(b) => b,
        None => return Ok(None),
    };

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(FETCH_TIMEOUT_SECS))
        .redirect(reqwest::redirect::Policy::limited(3))
        .build()
        .map_err(|e| e.to_string())?;

    // ⚠️ 按「常见程度」依次尝试。`/favicon.ico` 是惯例但不是保证，
    //   HTML 里的 `<link rel="icon">` 才是权威 —— 故先抓首页 HTML 找声明，
    //   找不到再退回 `/favicon.ico`。反过来（先抓 /favicon.ico）会让
    //   大量用 VitePress/Hugo 的站点拿到一张 404 图标。
    let mut candidates: Vec<String> = Vec::new();
    if let Some(html) = fetch_text(&client, &base).await {
        candidates.extend(icon_links_from_html(&html, &base));
    }
    candidates.push(format!("{base}/favicon.ico"));

    for c in candidates {
        if let Some(bytes) = fetch_bytes(&client, &c).await {
            if !looks_like_image(&bytes) {
                continue
            }
            return Ok(Some(store_icon(&bytes, c.split('?').next().unwrap_or("fav"))?));
        }
    }
    Ok(None)
}

// ---------------------------------------------------------------- 小工具

/// 只取 `scheme://host[:port]`，丢掉路径与查询
fn parse_origin(url: &str) -> Option<String> {
    let rest = url.split_once("://")?.1;
    let host = rest.split(['/', '?', '#']).next().unwrap_or("");
    if host.is_empty() {
        return None;
    }
    Some(format!("{}://{}", url.split("://").next().unwrap_or("http"), host))
}

/// 从 HTML 里挑出 `<link rel="icon">` 的 href（去重、保持出现顺序）
///
/// ⚠️ `rel` 的大小写与顺序都不可靠（`rel="SHORTCUT ICON"`、`rel="apple-touch-icon"`），
///   故用「包含 icon」判断而不是相等。
/// ⚠️ 只取看起来像图片的 href：`data:` 跳过（体积可能上千 KB 且不是文件），
///   `javascript:` / `vbscript:` 一律跳过 —— 那是 URL 注入面。
fn icon_links_from_html(html: &str, base: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let lower = html.to_ascii_lowercase();
    let mut idx = 0usize;
    while let Some(pos) = lower[idx..].find("<link") {
        let start = idx + pos;
        let end = lower[start..].find('>').map(|x| start + x).unwrap_or(lower.len());
        let tag = &html[start..end];
        idx = end + 1;

        if !lower[start..end].contains("icon") {
            continue
        }
        let Some(href) = attr(tag, "href") else { continue };
        let href = html_trim_entities(&href);
        if !looks_like_icon_href(&href) {
            continue
        }
        if let Some(abs) = absolutize(base, &href) {
            if !out.contains(&abs) {
                out.push(abs);
            }
        }
    }
    out
}

/// 这个 href 能不能拿去**抓**。
///
/// ⚠️ 四种拒答各有理由，缺一个就出事：
/// · `data:` —— 抓不到（不是网络地址），且可能几百 KB；
/// · `javascript:` / `vbscript:` —— URL 注入面；
/// · `file:` —— 能读本地文件。
///
/// ⚠️ 这里**完整地**判掉这四种，而不是与调用点的判断各管一半：
///   原来 `data:` 在调用点、其余三个在这里，于是「过滤到底生效没有」
///   没法单独测（实测把调用点的 `data:` 判断删掉，测试照样绿 ——
///   因为 `absolutize` 会顺带拒掉它）。
///   过滤器要能独立回答「行不行」，调用点才敢只写一句 `if !looks_like_icon_href(...)`。
fn looks_like_icon_href(href: &str) -> bool {
    // ⚠️ **这一条在今天与 `absolutize` 重复**（实测：把调用点的过滤删掉，
    //   `icon_links_skips_data_and_script_hrefs` 仍然绿）。原因是
    //   `absolutize` 只接受 `http(s)://` / `//` / `/` 开头，而这四种 scheme
    //   一个都不满足，于是被它顺带拒掉了。
    //
    //   留着它是**有意的冗余**，不是不知道它重复：
    //   · 它把「为什么拒这四个」写成代码而不是靠 `absolutize` 的形状隐含；
    //   · `absolutize` 将来若放宽（比如支持相对路径解析），这道过滤仍是唯一的
    //     防线 —— 而放宽 `absolutize` 恰恰是最可能发生的那种改动。
    //
    //   **代价要说清**：因为它是冗余的，所以**无法被测试单独观察**
    //   （删掉它，测试照绿）。真正的承重过滤是 `absolutize`，
    //   而那条由 `absolutize_handles_absolute_protocol_relative_and_root_paths`
    //   守着。别在这里再加测试去「证明」这条 —— 那只会写出一条永远绿的守卫。
    let h = href.trim().to_ascii_lowercase();
    !(h.starts_with("data:")
        || h.starts_with("javascript:")
        || h.starts_with("vbscript:")
        || h.starts_with("file:"))
}

fn attr(tag: &str, name: &str) -> Option<String> {
    let lower = tag.to_ascii_lowercase();
    let needle = format!("{name}=");
    let p = lower.find(&needle)? + needle.len();
    let rest = tag[p..].trim_start();
    let mut chars = rest.chars();
    let quote = chars.next()?;
    if quote != '"' && quote != '\'' {
        return None;
    }
    let body = &rest[quote.len_utf8()..];
    let close = body.find(quote)?;
    Some(body[..close].to_string())
}

/// 极简实体还原：只处理 `&amp;` —— href 里唯一必须还原的（否则带 query 的
/// favicon 地址会拼错）。其余实体原样保留。
fn html_trim_entities(s: &str) -> String {
    s.replace("&amp;", "&")
}

/// 把 href 变成绝对地址；相对路径按 base 的 host 根目录拼
fn absolutize(base: &str, href: &str) -> Option<String> {
    if href.starts_with("http://") || href.starts_with("https://") {
        return Some(href.to_string());
    }
    if href.starts_with("//") {
        let scheme = base.split("://").next().unwrap_or("https");
        return Some(format!("{scheme}:{href}"));
    }
    if !href.starts_with('/') {
        return None;
    }
    Some(format!("{base}{href}"))
}

/// 按**文件头**判是不是图片（不信扩展名、不信 Content-Type）
///
/// ⚠️ 理由与关卡里那张「按文件头判类型」的表同款：很多站点 `/favicon.ico`
///   返回的是 HTML 错误页（200 + `text/html`）。存成 `.png` 的 HTML
///   在界面上表现为「图标位置一块白/一块乱码」，而下载过程全程成功。
fn looks_like_image(b: &[u8]) -> bool {
    // ⚠️ 不要写 `b.len() > 4 && …` 这种「外层统一长度守卫」：
    //   JPEG 的魔数只有 3 字节（FF D8 FF），一条真 JPEG 恰好 4 字节时会被误杀。
    //   而且所有魔数本身都够特异（3~4 字节不等），额外的长度门槛防不住任何东西，
    //   只会造成假阴性。需要更长魔数的格式（webp / bmp）各自带长度判断即可。
    b.starts_with(&[0x89, 0x50, 0x4e, 0x47]) // png
        || b.starts_with(&[0xff, 0xd8, 0xff]) // jpeg
        || b.starts_with(b"GIF8") // gif
        || b.starts_with(&[0x00, 0x00, 0x01, 0x00]) // ico
        || (b.len() > 12 && b[0] == b'B' && b[1] == b'M') // bmp
        || (b.len() > 12 && &b[0..4] == b"RIFF" && &b[8..12] == b"WEBP") // webp
}

/// 落盘到数据根 `icons/`，内容哈希命名（同名同内容天然去重）。
/// 返回**绝对路径** —— 前端要经 `convertFileSrc` 读它。
fn store_icon(bytes: &[u8], hint: &str) -> Result<String, String> {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(bytes);
    let hash = hex(&digest[..8]);
    let ext = sniff_ext(bytes);
    let dir = crate::paths::data_root().join("icons");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let name = format!(
        "web-{}-{}.{}",
        hint.chars().filter(|c| c.is_ascii_alphanumeric()).take(24).collect::<String>(),
        hash,
        ext
    );
    let path = dir.join(&name);
    if !path.exists() {
        std::fs::write(&path, bytes).map_err(|e| e.to_string())?;
    }
    Ok(crate::paths::simplify_existing(&path).to_string_lossy().into_owned())
}

fn sniff_ext(b: &[u8]) -> &'static str {
    if b.starts_with(&[0x89, 0x50, 0x4e, 0x47]) {
        "png"
    } else if b.starts_with(&[0xff, 0xd8, 0xff]) {
        "jpg"
    } else if b.starts_with(b"GIF8") {
        "gif"
    } else if b.starts_with(&[0x00, 0x00, 0x01, 0x00]) {
        "ico"
    } else if b.len() > 12 && &b[0..4] == b"RIFF" && &b[8..12] == b"WEBP" {
        "webp"
    } else if b.len() > 12 && b[0] == b'B' && b[1] == b'M' {
        "bmp"
    } else {
        "png"
    }
}

fn hex(b: &[u8]) -> String {
    let mut s = String::with_capacity(b.len() * 2);
    for x in b {
        use std::fmt::Write;
        let _ = write!(s, "{x:02x}");
    }
    s
}

async fn fetch_text(client: &reqwest::Client, url: &str) -> Option<String> {
    let r = client.get(url).header("User-Agent", "m-hub-favicon/1").send().await.ok()?;
    if !r.status().is_success() {
        return None;
    }
    r.text().await.ok()
}

async fn fetch_bytes(client: &reqwest::Client, url: &str) -> Option<Vec<u8>> {
    let r = client.get(url).header("User-Agent", "m-hub-favicon/1").send().await.ok()?;
    if !r.status().is_success() {
        return None;
    }
    // ⚠️ 先读 Content-Length 再决定要不要读体：有些站点会给一个几 MB 的
    //   「favicon」（其实是整张 sprite 图），全读进来会卡住界面。
    if r.content_length().map(|n| n as usize > MAX_ICON_BYTES).unwrap_or(false) {
        return None;
    }
    // ⚠️ 用 `bytes()` 而不是 `Read::read_to_end`：reqwest 的 Response 实现了
    //   futures AsyncRead，不是 std::io::Read，两者的方法同名但不能互换 ——
    //   写错时编译器只说「找不到方法」，不会提示你「你导入的是另一个 Read」。
    let buf = r.bytes().await.ok()?;
    if buf.len() > MAX_ICON_BYTES {
        return None;
    }
    Some(buf.to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_origin_keeps_only_scheme_and_host() {
        assert_eq!(parse_origin("https://a.com/x/y?z=1").as_deref(), Some("https://a.com"));
        assert_eq!(parse_origin("http://a.com:8080/p").as_deref(), Some("http://a.com:8080"));
        assert_eq!(parse_origin("not a url"), None);
    }

    #[test]
    fn icon_links_picks_link_rel_icon_in_any_case_and_order() {
        let html = r#"
            <link rel="icon" href="/a.png">
            <link rel="SHORTCUT ICON" href="/b.ico">
            <link rel='apple-touch-icon' sizes="180x180" href="/c.png">
            <link rel="stylesheet" href="/style.css">
        "#;
        let got = icon_links_from_html(html, "https://x.com");
        assert!(got.contains(&"https://x.com/a.png".to_string()), "{got:?}");
        assert!(got.contains(&"https://x.com/b.ico".to_string()), "{got:?}");
        assert!(got.contains(&"https://x.com/c.png".to_string()), "{got:?}");
        assert!(!got.contains(&"https://x.com/style.css".to_string()), "样式表不是图标");
    }

    #[test]
    fn icon_links_skips_data_and_script_hrefs() {
        let html = r#"<link rel="icon" href="data:image/png;base64,AAA">
                     <link rel="icon" href="javascript:alert(1)">
                     <link rel="icon" href="/ok.png">"#;
        let got = icon_links_from_html(html, "https://x.com");
        assert_eq!(got, vec!["https://x.com/ok.png".to_string()]);
    }

    /**
     * ⚠️ 上面那条**单独**守着这个过滤是**不够**的（实测过：把过滤删掉它照样绿）——
     *   因为 `absolutize` 也会拒掉不以 `/` 或 `http` 开头的 href，
     *   于是 `data:` / `javascript:` 被**顺带**挡掉，测不出「过滤本身有没有生效」。
     *
     * 两条断言缺一不可：一条直测过滤函数（证明它自己是对的），
     * 一条走整条链路（证明它接在正确的地方）。少了任一条就有一类改动能悄悄破坏它。
     */
    /**
     * ⚠️ 这条测的是**策略本身**（哪些 scheme 不可抓），不是「调用点接对了没有」。
     *   后者今天**测不出来**，因为 `absolutize` 顺带也拒掉了 —— 详见
     *   `looks_like_icon_href` 里那段「与 absolutize 重复」的说明。
     *   别把它当成「删除调用点过滤会红」的证明。
     */
    #[test]
    fn looks_like_icon_href_rejects_embedded_and_script_schemes() {
        assert!(!looks_like_icon_href("data:image/png;base64,AAA"));
        assert!(!looks_like_icon_href("javascript:alert(1)"));
        assert!(!looks_like_icon_href("vbscript:msgbox"));
        assert!(!looks_like_icon_href("file:///etc/passwd"));
        assert!(looks_like_icon_href("/ok.png"));
        assert!(looks_like_icon_href("https://y.com/i.ico"));
    }

    /// 大量站点 `/favicon.ico` 回的是 HTML 错误页（200 + text/html）。
    /// 存成 `.png` 的 HTML 在界面上是「一块白」，而下载全程成功 ——
    /// 所以必须按**文件头**判，不能看 Content-Type 或扩展名。
    #[test]
    fn looks_like_image_rejects_html_error_page() {
        assert!(!looks_like_image(b"<!DOCTYPE html><html><head><title>404</title>"));
        assert!(!looks_like_image(b""));
        assert!(looks_like_image(&[0x89, 0x50, 0x4e, 0x47, 0x0d]));
        assert!(looks_like_image(&[0xff, 0xd8, 0xff, 0xe0]));
        assert!(looks_like_image(b"GIF89a"));
        assert!(looks_like_image(&[0x00, 0x00, 0x01, 0x00, 0x01]));
    }

    #[test]
    fn sniff_ext_matches_the_magic() {
        assert_eq!(sniff_ext(&[0x89, 0x50, 0x4e, 0x47]), "png");
        assert_eq!(sniff_ext(&[0xff, 0xd8, 0xff]), "jpg");
        assert_eq!(sniff_ext(b"GIF89a"), "gif");
        assert_eq!(sniff_ext(&[0x00, 0x00, 0x01, 0x00]), "ico");
    }

    #[test]
    fn absolutize_handles_absolute_protocol_relative_and_root_paths() {
        assert_eq!(absolutize("https://x.com", "https://y.com/i.png").as_deref(), Some("https://y.com/i.png"));
        assert_eq!(absolutize("https://x.com", "//y.com/i.png").as_deref(), Some("https://y.com/i.png"));
        assert_eq!(absolutize("https://x.com", "/i.png").as_deref(), Some("https://x.com/i.png"));
        // 相对路径（不带 /）本实现不支持 —— 宁可少一个候选，也不要猜错
        assert_eq!(absolutize("https://x.com", "i.png"), None);
    }

    #[test]
    fn html_entities_in_href_are_restored() {
        assert_eq!(html_trim_entities("/i.png?a=1&amp;b=2"), "/i.png?a=1&b=2");
    }
}