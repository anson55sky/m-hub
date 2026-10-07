//! 层级全路径的纯函数。两套业务共用：速达小类（`resource_subcategories`）
//! 与速记文件夹（`note_folders`）。
//!
//! ## 为什么要抽出来
//!
//! 两边的数据结构是**同构**的：`name` 存全路径（`工作`、`工作/会议`），段间用 `/`，
//! 判祖先按段比而不是字符串前缀比，改名/删除要整棵子树换父。
//! 第一次实现（速达小类）时这些函数直接写在 `subcategory.rs` 里；速记文件夹
//! 需要同一套时，**拷贝一份**意味着从此有两个 `is_within`，而它恰恰是最容易写错
//! 的那个（`starts_with("开发")` 会把 `开发者` 也算成后代 —— 症状是
//! 「删掉『开发者』顺手带走了『开发』下所有条目」，而界面上这两行看着毫无关系）。
//!
//! **判据：两处逻辑相同就抽，不要等第三个调用方。** 抽出来的收益是「改一处」，
//! 成本是多一层间接 —— 只有当两份真的**逐字相同**时才值得。

/// 层级分隔符。
pub const SEP: char = '/';

/// 段是否合法：非空、不超长、**不含分隔符**、不含首尾空白。
///
/// `label` 是业务名词（「小类」/「文件夹」），只进错误文案 —— 让用户看到
/// 「文件夹名称不能包含「/」」而不是「小类名称…」，因为此刻他在建文件夹。
pub fn validate_segment(seg: &str, label: &str) -> Result<(), String> {
    let t = seg.trim();
    if t.is_empty() {
        return Err(format!("{label}名称不能为空"));
    }
    if t.chars().count() > 20 {
        return Err(format!("{label}名称需为 1–20 个字符"));
    }
    if t.contains(SEP) {
        // ⚠️ 必须拦：分隔符进了段名，「A/B」就有两种解释，路径本身歧义化，
        //   而歧义会在改名/删除时静默算错子树（删「开发」把「开发/前端」也删了，
        //   用户以为是两件独立的事）。
        return Err(format!("{label}名称不能包含「{SEP}」（那是层级分隔符）"));
    }
    if t != seg {
        return Err(format!("{label}名称首尾不能有空格"));
    }
    Ok(())
}

/// 拼子级全路径。
pub fn join_path(parent: &str, seg: &str) -> String {
    if parent.is_empty() {
        seg.to_string()
    } else {
        format!("{parent}{SEP}{seg}")
    }
}

/// 全路径的最后一段（显示用）。
pub fn leaf_of(path: &str) -> &str {
    path.rsplit(SEP).next().unwrap_or(path)
}

/// 全路径去掉最后一段（= 父路径）；顶层返回空串。
pub fn parent_path(path: &str) -> &str {
    match path.rfind(SEP) {
        Some(i) => &path[..i],
        None => "",
    }
}

/// 层级深度：顶层 0。
pub fn depth_of(path: &str) -> usize {
    if path.is_empty() {
        0
    } else {
        path.split(SEP).count() - 1
    }
}

/// `path` 是否在 `ancestor` 之下（**含自身**）。
///
/// ⚠️ 必须是**按段**比，不是字符串前缀比：`开发` 不是 `开发者` 的祖先，
///   而 `starts_with("开发")` 会说是。空 `ancestor` 表示「根」，一切都在根之下。
pub fn is_within(path: &str, ancestor: &str) -> bool {
    if ancestor.is_empty() {
        return true;
    }
    path == ancestor
        || (path.len() > ancestor.len()
            && path.starts_with(ancestor)
            && path.as_bytes()[ancestor.len()] == SEP as u8)
}

/// 把一个全路径改挂到新父路径下（改名 / 删除级联用）。不在 `from` 之下则 `None`。
pub fn reparent(from: &str, to: &str, path: &str) -> Option<String> {
    if path == from {
        return Some(to.to_string());
    }
    let rest = path.strip_prefix(from)?.strip_prefix(SEP)?;
    Some(join_path(to, rest))
}

/// 层级树节点的**通用骨架**。业务侧各自映射成自己的节点类型
/// （速达小类还要 `is_default`，速记文件夹不要）。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct TreeNode {
    pub id: i64,
    pub full_path: String,
    pub depth: usize,
    pub children: Vec<TreeNode>,
}

/// 由一组 `(id, 全路径)` 构造嵌套树。
///
/// `rows` 必须已按业务期望的顺序排好（本工程两处都是 `sort_order ASC, id ASC`）——
/// **同级次序直接由输入序决定**。
///
/// ⚠️ **必须多趟**，不能单趟边走边挂：排序是全组的，子级完全可能排在父级之前
///   （老数据全是 `sort_order` 0..n，用户能把子级拖到父级前面；改名换父更会
///   直接打破「建子级时 sort_order ≥ 当前最大值」那个约定）。
///   单趟实现在那种数据上会误报「孤儿节点」—— 而孤儿错误一旦出现，正确的
///   处置是查数据，而数据其实没问题。
///
/// ⚠️ 同级挂载**必须按输入序插入**，不能一律 `push`。子级分到不同趟时
///   （甲先到但挂不上、乙当趟就挂上），一律 push 会把次序反过来 ——
///   界面上文件夹树变成「乙在甲前面」，而 `sort_order` 明明是甲小。
///   我第一版就是这么写的，被 `build_tree_keeps_sibling_order_across_passes` 抓到。
///
/// 剩下的才是真孤儿：跑完所有仍有进展的趟之后还挂不上的，父级是真的不存在。
/// **报错而不是跳过** —— 跳过的后果是那棵子树在界面上凭空消失，
/// 用户以为笔记/条目丢了。
pub fn build_tree(rows: &[(i64, String)], label: &str) -> Result<Vec<TreeNode>, String> {
    // 内部带一个输入序，供跨趟插入定位；转出时剥掉，不进 JSON
    struct Pending {
        id: i64,
        full_path: String,
        ord: usize,
    }
    #[derive(Debug)]
    struct Node {
        node: TreeNode,
        ord: usize,
        children: Vec<Node>,
    }

    fn find_mut<'a>(nodes: &'a mut [Node], path: &str) -> Option<&'a mut Node> {
        for n in nodes.iter_mut() {
            if n.node.full_path == path {
                return Some(n);
            }
            if let Some(found) = find_mut(&mut n.children, path) {
                return Some(found);
            }
        }
        None
    }

    fn push_ordered(list: &mut Vec<Node>, item: Node) {
        let pos = list.iter().position(|n| n.ord > item.ord).unwrap_or(list.len());
        list.insert(pos, item);
    }

    fn strip(nodes: Vec<Node>) -> Vec<TreeNode> {
        nodes
            .into_iter()
            .map(|n| TreeNode {
                id: n.node.id,
                full_path: n.node.full_path,
                depth: n.node.depth,
                children: strip(n.children),
            })
            .collect()
    }

    let mut roots: Vec<Node> = Vec::new();
    let mut pending: Vec<Pending> = rows
        .iter()
        .enumerate()
        .map(|(ord, (id, full_path))| Pending {
            id: *id,
            full_path: full_path.clone(),
            ord,
        })
        .collect();

    while !pending.is_empty() {
        let mut next: Vec<Pending> = Vec::new();
        let mut progressed = false;
        for p in pending {
            let parent = parent_path(&p.full_path).to_string();
            let node = Node {
                node: TreeNode {
                    id: p.id,
                    full_path: p.full_path.clone(),
                    depth: depth_of(&p.full_path),
                    children: Vec::new(),
                },
                ord: p.ord,
                children: Vec::new(),
            };
            if parent.is_empty() {
                push_ordered(&mut roots, node);
                progressed = true;
            } else if let Some(target) = find_mut(&mut roots, &parent) {
                push_ordered(&mut target.children, node);
                progressed = true;
            } else {
                next.push(p);
            }
        }
        if !progressed {
            // 剩下的挂不上：父级真的不存在。报第一个即可 —— 一次修完通常要重建
            // 整棵路径，报一串反而淹掉根因。
            let p = &next[0];
            return Err(format!(
                "{label}「{}」的父级「{}」不存在（孤儿节点）",
                p.full_path,
                parent_path(&p.full_path)
            ));
        }
        pending = next;
    }

    Ok(strip(roots))
}

#[cfg(test)]
mod tests {
    use super::*;

    const L: &str = "文件夹";

    #[test]
    fn path_shapes() {
        assert_eq!(join_path("", "工作"), "工作");
        assert_eq!(join_path("工作", "会议"), "工作/会议");
        assert_eq!(leaf_of("工作/会议"), "会议");
        assert_eq!(leaf_of("工作"), "工作");
        assert_eq!(parent_path("工作/会议/周会"), "工作/会议");
        assert_eq!(parent_path("工作"), "");
        assert_eq!(depth_of("工作"), 0);
        assert_eq!(depth_of("工作/会议"), 1);
        assert_eq!(depth_of(""), 0);
    }

    /// ⚠️ 这条是整套路径逻辑里最容易被写错的一处：`开发` **不是** `开发者` 的祖先。
    #[test]
    fn is_within_compares_by_segment_not_prefix() {
        assert!(is_within("开发/前端", "开发"));
        assert!(is_within("开发", "开发"));
        assert!(!is_within("开发者", "开发"));
        assert!(!is_within("开发", "开发/前端"));
        assert!(is_within("开发/前端", ""));
        // 同前缀但不同段：必须为假，否则「删开发者带走开发全部条目」
        assert!(!is_within("开发者/移动端", "开发"));
    }

    #[test]
    fn reparent_moves_whole_subtree() {
        assert_eq!(reparent("开发", "研发", "开发"), Some("研发".into()));
        assert_eq!(reparent("开发", "研发", "开发/前端"), Some("研发/前端".into()));
        assert_eq!(reparent("开发", "研发", "开发/前端/React"), Some("研发/前端/React".into()));
        assert_eq!(reparent("开发", "研发", "开发者"), None);
        assert_eq!(reparent("开发", "", "开发/前端"), Some("前端".into()));
    }

    #[test]
    fn validate_segment_rejects_separator() {
        assert!(validate_segment("工作", L).is_ok());
        assert!(validate_segment("", L).is_err());
        assert!(validate_segment("  ", L).is_err());
        assert!(validate_segment(" 工作", L).is_err(), "首尾空格");
        assert!(validate_segment("工作/会议", L).is_err(), "含分隔符");
        assert!(validate_segment(&"x".repeat(21), L).is_err(), "超长");
        // 错误文案用的是业务名词，不是硬编码的「小类」
        assert!(validate_segment("a/b", L).unwrap_err().contains("文件夹"));
    }

    #[test]
    fn build_tree_nests_by_path() {
        let rows = vec![
            (1i64, "工作".to_string()),
            (2, "工作/会议".to_string()),
            (3, "工作/会议/周会".to_string()),
            (4, "生活".to_string()),
        ];
        let t = build_tree(&rows, L).unwrap();
        assert_eq!(t.len(), 2);
        assert_eq!(t[0].full_path, "工作");
        assert_eq!(t[0].children[0].full_path, "工作/会议");
        assert_eq!(t[0].children[0].children[0].full_path, "工作/会议/周会");
        assert_eq!(t[0].children[0].children[0].depth, 2);
        assert!(t[1].children.is_empty());
    }

    /// 排序是**全组**的，子级可以排在父级前面 —— 所以必须多趟，不能单趟边走边挂。
    /// 单趟实现在这种数据上会误报「孤儿节点」，而数据其实没问题。
    #[test]
    fn build_tree_handles_child_sorted_before_parent() {
        let rows = vec![
            (2i64, "工作/会议".to_string()),
            (1, "工作".to_string()),
        ];
        let t = build_tree(&rows, L).unwrap();
        assert_eq!(t.len(), 1);
        assert_eq!(t[0].children.len(), 1);
        assert_eq!(t[0].children[0].full_path, "工作/会议");
    }

    /// 三层全部倒序（孙 → 子 → 父）：要两趟才挂得完。
    #[test]
    fn build_tree_handles_fully_reversed_order() {
        let rows = vec![
            (3i64, "工作/会议/周会".to_string()),
            (2, "工作/会议".to_string()),
            (1, "工作".to_string()),
        ];
        let t = build_tree(&rows, L).unwrap();
        assert_eq!(t.len(), 1);
        assert_eq!(t[0].children[0].children[0].full_path, "工作/会议/周会");
    }

    /// 同级次序必须仍是输入序（跨趟也不能乱）。
    #[test]
    fn build_tree_keeps_sibling_order_across_passes() {
        let rows = vec![
            (9i64, "工作/甲".to_string()), // sort_order 小，先到但挂不上
            (1, "工作".to_string()),
            (8, "工作/乙".to_string()), // sort_order 也小，第二趟挂
        ];
        let t = build_tree(&rows, L).unwrap();
        let kids: Vec<&str> = t[0].children.iter().map(|c| c.full_path.as_str()).collect();
        assert_eq!(kids, vec!["工作/甲", "工作/乙"]);
    }

    /// 孤儿必须**报错**而不是跳过：跳过的后果是那棵子树在界面上凭空消失。
    #[test]
    fn build_tree_rejects_orphan() {
        let rows = vec![(1i64, "工作/不存在的父".to_string())];
        let err = build_tree(&rows, L).unwrap_err();
        assert!(err.contains("孤儿节点"), "实际：{err}");
    }

    /// 真孤儿混在正常数据里时，报的仍然是**那个**孤儿，不是别的。
    #[test]
    fn build_tree_reports_the_real_orphan_among_valid_rows() {
        let rows = vec![
            (1i64, "工作".to_string()),
            (2, "工作/会议".to_string()),
            (3, "工作/会议/周会".to_string()),
            (4, "幽灵/子".to_string()),
        ];
        let err = build_tree(&rows, L).unwrap_err();
        assert!(err.contains("幽灵/子"), "实际：{err}");
    }
}
