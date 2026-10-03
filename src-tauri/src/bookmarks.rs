// 从 Chromium 系浏览器的书签文件里读出文件夹树。
//
// ## 为什么**不**用 SQLite
//
// 直觉上「浏览器的书签存在 SQLite 里」—— 那是 Firefox 的做法。
// Chromium 系（Chrome / Edge / Brave / Chromium / Vivaldi / Arc）存的是
// **一个 JSON 文件**：`<profile>/Bookmarks`。实测 `roots` 下是
// `bookmark_bar` / `other` / `synced`（Edge 还多一个 `workspaces_v2`），
// 每个节点 `type` 为 `folder` / `url`。
//
// 于是这一层**不需要任何数据库依赖**，也不需要拷���锁文件 —— 而拷 `Bookmarks`
// 本身就有坑：浏览器运行时它可能正在写。
//
// ## 「目录也可以变成速达里的分类」
//
// 每个文件夹节点都带一个 `categoryName`，正是发布说明里那句话：
// 浏览器里分好的目录 → 速达里的分类。分类名 = 文件夹全路径（`/` 连接），
// 而不是只有末级 —— 否则两个浏览器里都叫「工作」的文件夹会撞成同一个分类，
// 用户的书签被静默混在一起。
use std::path::{Path, PathBuf};

use serde::Serialize;

/// 单个书签或文件夹（递归）
#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct BookmarkNode {
    /// 稳定 id（浏览器里的 uuid；有极少数老条目没有，用路径兜底）
    pub id: String,
    /// 显示名
    pub name: String,
    /// `url` 条目才有
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// 文件夹的子节点（url 条目为空数组）
    pub children: Vec<BookmarkNode>,
    /// 文件夹 → 速达分类名（全路径）。url 条目为空。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    /// 目录里（递归）的书签条数，界面用来显示「整组有 N 条」
    pub url_count: usize,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserBookmarks {
    /// 浏览器显示名（Google Chrome / Microsoft Edge …）
    pub browser: String,
    /// profile 名（Default / Profile 1 …）
    pub profile: String,
    pub roots: Vec<BookmarkNode>,
}

/// 一个浏览器位置的候选路径（macOS）
fn profile_dirs(browser_dir: &str) -> Vec<(PathBuf, String)> {
    let Some(home) = std::env::var_os("HOME").map(PathBuf::from) else {
        return Vec::new();
    };
    profile_dirs_in(&home.join("Library").join("Application Support"), browser_dir)
}

fn profile_dirs_in(support: &Path, browser_dir: &str) -> Vec<(PathBuf, String)> {
    let base = support.join(browser_dir);
    let mut out = Vec::new();
    // ① `Default` —— 绝大多数人只有这一个，必须排在最前
    out.push((base.join("Default"), "Default".to_string()));
    // ② `Profile N` —— 多profile 用户。按数字**升序**，否则 Profile 10 排在 Profile 2 前
    if let Ok(rd) = std::fs::read_dir(&base) {
        let mut numbered: Vec<(u32, PathBuf, String)> = Vec::new();
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if let Some(rest) = name.strip_prefix("Profile ") {
                if let Ok(n) = rest.parse::<u32>() {
                    numbered.push((n, e.path(), name.clone()));
                }
            }
        }
        numbered.sort_by_key(|(n, _, _)| *n);
        out.extend(numbered.into_iter().map(|(_, p, n)| (p, n)));
    }
    out
}

/// 扫描本机所有 Chromium 系浏览器的书签
#[tauri::command]
pub async fn read_browser_bookmarks() -> Result<Vec<BrowserBookmarks>, String> {
    let found = tokio::task::spawn_blocking(scan_all)
        .await
        .map_err(|e| e.to_string())??;
    if found.is_empty() {
        return Err(
            "没找到任何 Chromium 系浏览器的书签。已查：Google Chrome / Microsoft Edge / Brave / Chromium / Vivaldi / Arc"
                .into(),
        );
    }
    Ok(found)
}

fn scan_all() -> Result<Vec<BrowserBookmarks>, String> {
    let home = match std::env::var_os("HOME") {
        Some(h) => PathBuf::from(h),
        None => return Ok(Vec::new()),
    };
    scan_all_in(&home.join("Library").join("Application Support"))
}

/// 从给定的「Application Support」根目录扫全部浏览器。
///
/// ⚠️ 抽出来是为了**能测**：第一版里根目录写死在函数内部，于是
///   「排序对不对」「一个浏览器坏了会不会连累别人」这两条都只能用
///   开发机上真实安装的浏览器来验 —— 而本机的书签栏是空的，
///   第一版的测试因此对排序**完全无感**（字典序恰好也对）。
fn scan_all_in(base: &Path) -> Result<Vec<BrowserBookmarks>, String> {
    // ⚠️ 顺序即界面里的展示顺序：按用户「主用浏览器」的可能性排，
    //   而不是字典序（否则 Arc 会排在 Chrome 前面）。
    const BROWSERS: &[(&str, &str)] = &[
        ("Google/Chrome", "Google Chrome"),
        ("Microsoft Edge", "Microsoft Edge"),
        ("BraveSoftware/Brave-Browser", "Brave"),
        ("Chromium", "Chromium"),
        ("Vivaldi", "Vivaldi"),
        ("company.thebrowser.Browser", "Arc"),
    ];
    let mut out = Vec::new();
    for (dir, label) in BROWSERS {
        for (path, profile) in profile_dirs_in(base, dir) {
            let f = path.join("Bookmarks");
            if !f.is_file() {
                continue;
            }
            if let Some(b) = parse_bookmark_file(&f, label, &profile) {
                out.push(b);
            }
        }
    }
    Ok(out)
}

/// 解析一个 `Bookmarks` JSON 文件
fn parse_bookmark_file(file: &Path, browser: &str, profile: &str) -> Option<BrowserBookmarks> {
    let raw = std::fs::read(file).ok()?;
    // ⚠️ 浏览器**运行时**这个文件可能正被改写，读到半截 JSON 是常见情况。
    //   失败就返回 None（这个 profile 跳过），绝不把错误抛给界面 ——
    //   「这个浏览器读不了」不该让其它浏览器的书签也一起消失。
    let v: serde_json::Value = serde_json::from_slice(&raw).ok()?;
    let roots_obj = v.get("roots")?.as_object()?;
    let mut roots = Vec::new();
    // ⚠️ `roots` 的键在 JSON 对象里**无序**，而界面上「书签栏」应该排在最前。
    //   故显式给一个优先序，其余按名字排。
    let priority = ["bookmark_bar", "other", "mobile", "synced"];
    let mut keys: Vec<&String> = roots_obj.keys().collect();
    keys.sort_by_key(|k| priority.iter().position(|p| p == *k).unwrap_or(usize::MAX));
    for k in keys {
        let Some(node) = roots_obj.get(k) else { continue };
        // 根节点的 name 有时是空的（如 "mobile"），用它自己的键兜底
        let n = node_from(node, k, None);
        if n.url_count > 0 {
            roots.push(n);
        }
    }
    if roots.is_empty() {
        return None;
    }
    Some(BrowserBookmarks { browser: browser.to_string(), profile: profile.to_string(), roots })
}

/// 单个节点 → `BookmarkNode`
fn node_from(v: &serde_json::Value, fallback_name: &str, category: Option<&str>) -> BookmarkNode {
    let ty = v.get("type").and_then(|x| x.as_str()).unwrap_or("");
    let name = v
        .get("name")
        .and_then(|x| x.as_str())
        .filter(|s| !s.trim().is_empty())
        .unwrap_or(fallback_name)
        .to_string();

    if ty != "folder" {
        let url = v.get("url").and_then(|x| x.as_str()).map(|s| s.to_string());
        return BookmarkNode {
            id: v
                .get("id")
                .and_then(|x| x.as_str())
                .map(|s| s.to_string())
                // ⚠️ 没有 id 的条目（老书签）用「分类|名字|URL」当兜底。三者都要：
                //   · **URL 必须在里面**：同一文件夹里两个同名书签很常见（两个 GitHub 仓库
                //     都叫「首页」之类），少了 URL 就会撞 —— 撞了之后界面 `v-for :key`
                //     重复渲染、勾选框点一个亮两个（实测：这条就是测试逼出来的）；
                //   · 分类要保证跨文件夹不同名；
                //   · 三者都稳定 → 重新扫描后勾选状态不丢。
                .unwrap_or_else(|| format!("u|{}|{}|{}", category.unwrap_or(""), name, url.as_deref().unwrap_or(""))),
            name,
            url,
            children: Vec::new(),
            category: None,
            url_count: 1,
        };
    }

    // 分类名 = 父分类 + 文件夹名（根节点的父分类为空，不带前导斜杠）
    let cat = match category {
        None => name.clone(),
        Some(p) if p.is_empty() => name.clone(),
        Some(p) => format!("{p}/{name}"),
    };
    let children: Vec<BookmarkNode> = v
        .get("children")
        .and_then(|x| x.as_array())
        .map(|arr| {
            arr.iter()
                .map(|c| node_from(c, "", Some(&cat)))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let url_count = children.iter().map(|c| c.url_count).sum();
    BookmarkNode {
        id: v
            .get("id")
            .and_then(|x| x.as_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| format!("f|{cat}")),
        name,
        url: None,
        children,
        category: Some(cat),
        url_count,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> serde_json::Value {
        serde_json::json!({
          "roots": {
            "bookmark_bar": {
              "type": "folder", "name": "书签栏", "id": "1",
              "children": [
                { "type": "folder", "name": "工作", "id": "2", "children": [
                    { "type": "url", "name": "文档", "url": "https://a.com", "id": "3" },
                    { "type": "url", "name": "看板", "url": "https://b.com", "id": "4" }
                ]},
                { "type": "url", "name": "直接", "url": "https://c.com", "id": "5" }
              ]
            },
            "other": {
              "type": "folder", "name": "其他书签", "id": "6",
              "children": [
                { "type": "folder", "name": "工作", "id": "7", "children": [
                    { "type": "url", "name": "别的", "url": "https://d.com", "id": "8" }
                ]}
              ]
            },
            "empty": { "type": "folder", "name": "空", "id": "9", "children": [] }
          }
        })
    }

    /// 「书签栏」必须排第一 —— `roots` 在 JSON 里无序，而界面上它是主入口。
    #[test]
    fn bookmark_bar_comes_first() {
        let f = tempfile::tempdir().unwrap();
        let p = f.path().join("Bookmarks");
        std::fs::write(&p, serde_json::to_vec(&sample()).unwrap()).unwrap();
        let b = parse_bookmark_file(&p, "Chrome", "Default").unwrap();
        assert_eq!(b.roots[0].name, "书签栏");
    }

    /// 空目录**不出现**在列表里 —— 一堆空文件夹只会让用户以为自己导入失败了。
    #[test]
    fn empty_roots_are_dropped() {
        let f = tempfile::tempdir().unwrap();
        let p = f.path().join("Bookmarks");
        std::fs::write(&p, serde_json::to_vec(&sample()).unwrap()).unwrap();
        let b = parse_bookmark_file(&p, "Chrome", "Default").unwrap();
        assert!(!b.roots.iter().any(|r| r.name == "空"));
        assert_eq!(b.roots.len(), 2);
    }

    /**
     * 分类名必须是**全路径**而不是末级文件夹名。
     *
     * ⚠️ 两个浏览器/根下都有「工作」文件夹是很常见的（书签栏里一个、其他书签里一个）。
     *   只取末级名会让它们撞成同一个速达分类，用户两处书签被静默混在一起。
     */
    #[test]
    fn category_is_the_full_path_not_just_the_leaf() {
        let f = tempfile::tempdir().unwrap();
        let p = f.path().join("Bookmarks");
        std::fs::write(&p, serde_json::to_vec(&sample()).unwrap()).unwrap();
        let b = parse_bookmark_file(&p, "Chrome", "Default").unwrap();
        let cats: Vec<&str> = b
            .roots
            .iter()
            .flat_map(|r| r.children.iter())
            .filter_map(|c| c.category.as_deref())
            .collect();
        assert!(cats.contains(&"书签栏/工作"), "{cats:?}");
        assert!(cats.contains(&"其他书签/工作"), "{cats:?}");
        assert_ne!(
            cats[0], cats[1],
            "同名的两个「工作」文件夹必须映射到不同分类，否则书签被混在一起"
        );
    }

    #[test]
    fn url_count_is_recursive() {
        let f = tempfile::tempdir().unwrap();
        let p = f.path().join("Bookmarks");
        std::fs::write(&p, serde_json::to_vec(&sample()).unwrap()).unwrap();
        let b = parse_bookmark_file(&p, "Chrome", "Default").unwrap();
        assert_eq!(b.roots[0].url_count, 3, "2 条在子文件夹 + 1 条直属");
        assert_eq!(b.roots[1].url_count, 1);
    }

    /// 缺 id 的老书签必须有**稳定且唯一**的兜底 id：
    /// 界面用 id 当 v-for key（重复会渲染错），勾选状态也按 id 存（不稳会丢）。
    #[test]
    fn nodes_without_id_get_a_stable_unique_fallback() {
        let v = serde_json::json!({
          "type": "folder", "name": "工作", "children": [
            { "type": "url", "name": "同名", "url": "https://a.com" },
            { "type": "url", "name": "同名", "url": "https://b.com" }
          ]
        });
        let a = node_from(&v, "x", None);
        let b = node_from(&v, "x", None);
        assert_eq!(a.children[0].id, b.children[0].id, "同样两次解析必须给同一个 id");
        assert_ne!(a.children[0].id, a.children[1].id, "同名不同 URL 必须不同 id");
        assert!(!a.children[0].id.is_empty());
    }

    /// 浏览器运行中被改写 → 半截 JSON。这必须**静默跳过**，
    /// 而不是把错误抛给整个列表（那会让其它浏览器的书签一起消失）。
    #[test]
    fn truncated_json_skips_this_profile_only() {
        let f = tempfile::tempdir().unwrap();
        let p = f.path().join("Bookmarks");
        std::fs::write(&p, b"{\"roots\": {\"bookmark_bar\": {\"type\": \"folde").unwrap();
        assert!(parse_bookmark_file(&p, "Chrome", "Default").is_none());
    }

    /**
     * 排序**必须**由「书签栏优先」决定，而不是字典序。
     *
     * ⚠️ 第一版的样本里 `bookmark_bar` 恰好也是字典序第一（serde_json 默认
     *   用 BTreeMap 存对象，解析出来已经排好序了），于是把整段排序删掉
     *   测试**照样绿** —— 一条从不生效的守卫。
     *   所以这里特意塞一个字典序排在 `bookmark_bar` 前面的根（`aaa`），
     *   让「按优先级」与「按字典序」产生可观测的差别。
     */
    #[test]
    fn bookmark_bar_wins_over_alphabetical_order() {
        let v = serde_json::json!({
          "roots": {
            "aaa": { "type": "folder", "name": "AAA", "id": "1", "children": [
                { "type": "url", "name": "x", "url": "https://a", "id": "2" }]},
            "bookmark_bar": { "type": "folder", "name": "书签栏", "id": "3", "children": [
                { "type": "url", "name": "y", "url": "https://b", "id": "4" }]},
            "other": { "type": "folder", "name": "其他", "id": "5", "children": [
                { "type": "url", "name": "z", "url": "https://c", "id": "6" }]}
          }
        });
        let f = tempfile::tempdir().unwrap();
        let p = f.path().join("Bookmarks");
        std::fs::write(&p, serde_json::to_vec(&v).unwrap()).unwrap();
        let b = parse_bookmark_file(&p, "Chrome", "Default").unwrap();
        assert_eq!(b.roots[0].name, "书签栏", "字典序会把 AAA 排到最前");
        assert_eq!(b.roots.len(), 3);
    }

    /**
     * 一个浏览器的书签文件坏了，**不得**连累其它浏览器。
     *
     * ⚠️ 这就是「每个 profile 各自 `ok()?`」那条设计的意义。改坏一个文件
     *   就该只有那一个消失，而不是整体报错让所有浏览器都读不到。
     */
    #[test]
    fn one_broken_browser_does_not_hide_the_others() {
        let support = tempfile::tempdir().unwrap();
        let good = support.path().join("Google/Chrome/Default");
        std::fs::create_dir_all(&good).unwrap();
        std::fs::write(
            good.join("Bookmarks"),
            serde_json::to_vec(&sample()).unwrap(),
        )
        .unwrap();
        let bad = support.path().join("Microsoft Edge/Default");
        std::fs::create_dir_all(&bad).unwrap();
        std::fs::write(bad.join("Bookmarks"), b"{\"roots\": {\"bookmark_bar\": {\"typ").unwrap();

        let got = scan_all_in(support.path()).unwrap();
        assert_eq!(got.len(), 1, "只该剩下 Chrome 那一个：{got:?}");
        assert_eq!(got[0].browser, "Google Chrome");
        assert_eq!(got[0].roots[0].url_count, 3);
    }

    #[test]
    fn profile_dirs_lists_default_first_then_numeric_profiles() {
        let support = tempfile::tempdir().unwrap();
        let base = support.path().join("Google/Chrome");
        for p in ["Default", "Profile 2", "Profile 10"] {
            std::fs::create_dir_all(base.join(p)).unwrap();
        }
        let got = profile_dirs_in(support.path(), "Google/Chrome");
        let names: Vec<&str> = got.iter().map(|(_, n)| n.as_str()).collect();
        assert_eq!(
            names,
            vec!["Default", "Profile 2", "Profile 10"],
            "Default 最前，且 Profile 按**数字**升序（字典序会让 Profile 10 排在 Profile 2 前）"
        );
        let _ = std::env::var_os("HOME");
        let old = std::env::var_os("HOME").map(PathBuf::from).unwrap();
        assert!(!old.join("Library/Application Support/Google/Chrome").is_dir() || true);
        assert_eq!(got[0].1, "Default", "Default 必须排最前");
        // Profile N 按数字升序而不是字典序
        let numbered: Vec<&str> = got.iter().map(|(_, n)| n.as_str()).filter(|n| n.starts_with("Profile ")).collect();
        let sorted = {
            let mut v = numbered.clone();
            v.sort_by_key(|n| n.trim_start_matches("Profile ").parse::<u32>().unwrap_or(0));
            v
        };
        assert_eq!(numbered, sorted, "Profile 必须按数字排，否则 Profile 10 排在 Profile 2 前");
    }
}