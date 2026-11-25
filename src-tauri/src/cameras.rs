//! 相机型号索引 + 中文/简称别名搜索
//!
//! 目标：用户输入「索尼a7m4」「a7m4」「A7 IV」「7M4」都能命中 `Sony ILCE-7M4`。

use serde::Serialize;
use std::path::PathBuf;

/// 主流摄影机身厂商（过滤 Apple/Samsung/Google/Xiaomi/OPPO/HUAWEI/LG 等手机摄像头
/// ——iPhone 32 镜头污染列表；也过滤 Phase One / Leaf / Mamiya / Kodak 等老中画幅）
const MAINSTREAM_MAKES: &[&str] = &[
    "Sony", "Canon", "Nikon", "Fujifilm", "Panasonic",
    "Leica", "LEICA", "OM Digital", "OM", "Olympus",
    "Hasselblad", "Pentax", "PENTAX", "Ricoh", "RICOH", "Sigma",
    "DJI", "Minolta", "Konica",
];

#[derive(Debug, Clone, Serialize)]
pub struct CameraEntry {
    /// Adobe 的 UniqueCameraModel 字符串（DCP 匹配键，必须逐字节一致）
    pub model: String,
    pub make: String,
    /// 型号部分（去掉 make 前缀）
    pub short: String,
    /// 简称别名（英文/拼音），用于模糊搜索
    pub aliases: Vec<String>,
    /// 中文别名（品牌中文名 + 通用简称），供中文搜索
    pub chinese_aliases: Vec<String>,
    /// 对应的 Adobe Standard DCP 路径
    pub std_dcp: PathBuf,
}

fn make_of(model: &str) -> String {
    // 归一化厂商名："LEICA M11" → "Leica"，"OM-1" → "OM System"，"PENTAX K-3" → "Pentax"
    const CANONICAL: &[(&str, &str)] = &[
        ("LEICA", "Leica"),
        ("OM", "OM System"),
        ("OM Digital", "OM System"),
        ("PENTAX", "Pentax"),
        ("RICOH", "Ricoh"),
        ("OLYMPUS", "Olympus"),
        ("FUJIFILM", "Fujifilm"),
    ];
    let raw = model.split_whitespace().next().unwrap_or("");
    for (raw_prefix, canonical) in CANONICAL {
        if raw.eq_ignore_ascii_case(raw_prefix) {
            return canonical.to_string();
        }
    }
    raw.to_string()
}

fn short_of(model: &str, make: &str) -> String {
    model
        .strip_prefix(make)
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| model.to_string())
}

/// 中文品牌名 → 英文 make 前缀（用于把「索尼a7m4」拆成 make + query）
fn chinese_make(q: &str) -> Option<&'static str> {
    const TABLE: &[(&str, &str)] = &[
        ("索尼", "Sony"),
        ("佳能", "Canon"),
        ("尼康", "Nikon"),
        ("富士", "Fujifilm"),
        ("松下", "Panasonic"),
        ("徕卡", "Leica"),
        ("奥林巴斯", "Olympus"),
        ("奥之心", "OM Digital"),
        ("哈苏", "Hasselblad"),
        ("宾得", "Pentax"),
        ("理光", "Ricoh"),
        ("适马", "Sigma"),
        ("大疆", "DJI"),
        ("飞思", "Phase One"),
    ];
    TABLE.iter().find(|(cn, _)| q.contains(cn)).map(|(_, en)| *en)
}

/// 为某机型生成通俗别名（a7m4 / a7r5 / r6m2 这类叫法）
fn aliases_for(make: &str, short: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let compact = short.replace([' ', '-'], "").to_lowercase();
    out.push(compact.clone());
    out.push(short.to_lowercase());

    if make == "Sony" {
        // ILCE-7M4 → a7m4 / 7m4
        // ILCE-7RM5 → a7r5（去掉 M）
        // ILCE-7CM2 → a7c2（去掉 M）
        // ILCE-6700 → 6700（无字母）
        // ILCE-9M3 → a9m3
        if let Some(rest) = short.strip_prefix("ILCE-") {
            let r = rest.replace([' ', '-'], "").to_uppercase();
            out.push(r.to_lowercase()); // 7m4 / 7rm5 / 7cm2 / 6700

            let chars: Vec<char> = r.chars().collect();
            let lead_digits: String = chars.iter().take_while(|c| c.is_numeric()).collect();
            let mid_alpha: String = chars
                .iter().skip(lead_digits.len())
                .take_while(|c| c.is_alphabetic())
                .collect();
            let tail: String = chars.iter().rev().take_while(|c| c.is_numeric())
                .collect::<Vec<_>>().into_iter().rev().collect();

            if !lead_digits.is_empty() {
                if mid_alpha.is_empty() {
                    // ILCE-6700 → a6700（全数字，lead_digits 已经吃完整型号，不能再拼 tail）
                    let cand = format!("a{}", lead_digits);
                    if !out.contains(&cand) { out.push(cand); }
                } else {
                    // RM5 → R5、CM2 → C2 要去掉 M；但 7M4 的 M 是官方叫法（a7m4），要保留
                    let alpha_no_m: String =
                        mid_alpha.chars().filter(|c| *c != 'M').collect();
                    let alpha = if alpha_no_m.is_empty() { mid_alpha.clone() } else { alpha_no_m };
                    let cand = format!("a{}{}{}", lead_digits, alpha, tail).to_lowercase();
                    if !out.contains(&cand) { out.push(cand); }
                }
            }
        }
    } else if make == "Canon" {
        // "EOS R6 Mark II" → r6m2 / r6ii / r62 / eosr6m2
        if let Some(r) = short.strip_prefix("EOS ") {
            let base = r.replace("Mark ", "");
            out.push(base.to_lowercase());
            let lc = base.to_lowercase();
            // R6 II → r6m2 / r62
            let roman_map = [("iii", "3"), ("ii", "2"), ("iv", "4"), ("v", "5"), ("i", "1")];
            let mut n = lc.clone();
            for (rom, num) in roman_map {
                n = n.replace(rom, num);
            }
            let n = n.replace(' ', "");
            if n != lc { out.push(n.clone()); }
            // r6m2 形式
            let m_form = insert_mark(&n);
            if !out.contains(&m_form) { out.push(m_form); }
        }
    } else if make == "Nikon" {
        // "Z 8" → z8；"Z 6III" → z6iii / z63
        let r = short.replace([' ', '-'], "").to_lowercase();
        out.push(r.clone());
    }
    out.sort();
    out.dedup();
    out
}

/// "r62" → "r6m2"（Canon 的常见写法）
fn insert_mark(s: &str) -> String {
    // 找到末尾数字前的位置插入 m
    let chars: Vec<char> = s.chars().collect();
    let tail_len = chars.iter().rev().take_while(|c| c.is_numeric()).count();
    let split_at = chars.len().saturating_sub(tail_len);
    if tail_len == 0 || split_at == 0 {
        return s.to_string();
    }
    let head: String = chars[..split_at].iter().collect();
    let tail: String = chars[split_at..].iter().collect();
    format!("{head}m{tail}")
}

/// 列出主流厂商的全部机型（带别名）
pub fn list_cameras() -> Vec<CameraEntry> {
    let mut out: Vec<CameraEntry> = Vec::new();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();

    for root in crate::matrix::adobe_standard_search_roots() {
        let std_dir = root.join("Adobe Standard");
        let Ok(entries) = std::fs::read_dir(&std_dir) else { continue };
        for e in entries.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            let Some(model) = name
                .strip_suffix(" Adobe Standard.dcp")
                .or_else(|| name.strip_suffix(" Adobe Standard v2.dcp"))
            else { continue };
            if !seen.insert(model.to_string()) { continue; }

            let make = make_of(model);
            if !MAINSTREAM_MAKES.iter().any(|m| m.eq_ignore_ascii_case(&make)) {
                continue;
            }
            let short = short_of(model, &make);
            out.push(CameraEntry {
                model: model.to_string(),
                make: make.clone(),
                short: short.clone(),
                aliases: aliases_for(&make, &short),
                chinese_aliases: chinese_aliases_for(&make, &short),
                std_dcp: e.path(),
            });
        }
    }
    out.sort_by(|a, b| a.make.cmp(&b.make).then(a.short.cmp(&b.short)));
    out
}

/// 把用户查询归一化：'索尼a7m4' → ['sony', 'a7m4'] 形式的多 token
pub fn normalize_query(q: &str) -> Vec<String> {
    let lower = q.trim().to_lowercase();
    let mut tokens: Vec<String> = Vec::new();

    if let Some(make) = chinese_make(&lower) {
        tokens.push(make.to_lowercase());
        // 去掉中文品牌后剩下的部分才是型号
        let mut rest = lower.clone();
        for cn in ["索尼", "佳能", "尼康", "富士", "松下", "徕卡", "奥林巴斯", "奥之心",
                   "哈苏", "宾得", "理光", "适马", "大疆", "飞思"] {
            rest = rest.replace(cn, "");
        }
        let rest = rest.trim().to_string();
        if !rest.is_empty() {
            tokens.push(rest);
        }
    } else {
        // 英文查询："sony a7m4" / "Sony ILCE-7M4" / "a7m4"
        let parts: Vec<&str> = lower.split_whitespace().collect();
        if parts.len() >= 2 {
            let first = parts[0];
            let is_make = MAINSTREAM_MAKES
                .iter()
                .any(|m| m.to_lowercase() == first || m.to_lowercase().starts_with(first));
            if is_make {
                tokens.push(first.to_string());
                let rest = parts[1..].join("").replace('-', "");
                if !rest.is_empty() {
                    tokens.push(rest);
                }
            } else {
                tokens.push(lower.replace(' ', ""));
            }
        } else {
            tokens.push(lower);
        }
    }
    tokens
}

/// 品牌中文名 → 每台相机都挂上对应中文别名
fn chinese_aliases_for(make: &str, short: &str) -> Vec<String> {
    let brand = match make {
        "Sony" => vec!["索尼", "sony"],
        "Canon" => vec!["佳能", "canon"],
        "Nikon" => vec!["尼康", "nikon"],
        "Fujifilm" => vec!["富士", "fuji", "fujifilm"],
        "Panasonic" => vec!["松下", "panasonic"],
        "Leica" => vec!["徕卡", "leica"],
        "OM System" => vec!["奥之心", "奥巴", "奥林巴斯", "om"],
        "Olympus" => vec!["奥林巴斯", "奥巴", "olympus"],
        "Hasselblad" => vec!["哈苏", "hasselblad"],
        "Pentax" => vec!["宾得", "pentax"],
        "Ricoh" => vec!["理光", "ricoh"],
        "Sigma" => vec!["适马", "sigma"],
        "DJI" => vec!["大疆", "dji"],
        "Minolta" => vec!["美能达", "minolta"],
        _ => vec![],
    };
    let mut out: Vec<String> = brand.iter().map(|s| s.to_string()).collect();
    // short 的常见中文简称（去 ILCE-/EOS-/Z 空格）—— 比如让「索尼7m4」能命中
    let compact = short.replace([' ', '-'], "").to_lowercase();
    out.push(compact);
    out.sort();
    out.dedup();
    out
}

/// 判断一个 CameraEntry 是否匹配用户查询
pub fn matches(entry: &CameraEntry, tokens: &[String]) -> bool {
    tokens.iter().all(|t| {
        let t = t.as_str();
        entry.make.to_lowercase().contains(t)
            || entry.short.to_lowercase().contains(t)
            || entry.model.to_lowercase().contains(t)
            || entry.aliases.iter().any(|a| a.contains(t))
            || entry.chinese_aliases.iter().any(|a| a.contains(t))
    })
}

/// 默认 look 源：工作目录里的 VisionLOG DCP（优先 A7M5）
pub fn default_look_source() -> Option<PathBuf> {
    let candidates = [
        std::env::current_dir().ok(),
        std::env::current_dir().ok().and_then(|p| p.parent().map(|x| x.to_path_buf())),
    ];
    let mut found: Vec<PathBuf> = Vec::new();
    for dir in candidates.into_iter().flatten() {
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for e in entries.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            if name.to_lowercase().contains("visionlog") && name.to_lowercase().ends_with(".dcp") {
                found.push(e.path());
            }
        }
        if !found.is_empty() { break; }
    }
    // A7M5 优先（用户手上这份），否则取第一个
    found.sort_by_key(|p| {
        let n = p.file_name().unwrap_or_default().to_string_lossy().to_string();
        if n.contains("7M5") { 0 } else { 1 }
    });
    found.into_iter().next()
}

#[cfg(test)]
mod tests {

    use super::*;

    #[test]
    fn sony_aliases_cover_common_names() {
        let a = aliases_for("Sony", "ILCE-7M4");
        for want in ["a7m4", "7m4"] {
            assert!(a.contains(&want.to_string()), "missing {want} in {a:?}");
        }
        let r = aliases_for("Sony", "ILCE-7RM5");
        assert!(r.contains(&"a7r5".to_string()), "a7r5 missing in {r:?}");
        let c = aliases_for("Sony", "ILCE-7CM2");
        assert!(c.contains(&"a7c2".to_string()), "a7c2 missing in {c:?}");
    }

    #[test]
    fn sony_alias_matrix() {
        let cases = [
            ("ILCE-7M4", "a7m4"),
            ("ILCE-7RM5", "a7r5"),
            ("ILCE-7CM2", "a7c2"),
            ("ILCE-9M3", "a9m3"),
            ("ILCE-6700", "a6700"),
            ("ILCE-1", "a1"),
        ];
        for (short, want) in cases {
            let a = aliases_for("Sony", short);
            assert!(a.contains(&want.to_string()), "{short} should have {want}, got {a:?}");
        }
    }

    #[test]
    fn canon_aliases() {
        let a = aliases_for("Canon", "EOS R6 Mark II");
        assert!(a.iter().any(|x| x == "r6ii" || x == "r62" || x == "r6m2"), "got {a:?}");
    }

    #[test]
    fn chinese_query_normalizes() {
        let t = normalize_query("索尼a7m4");
        assert!(t.contains(&"sony".to_string()));
        assert!(t.contains(&"a7m4".to_string()));
    }

    #[test]
    fn search_hits_sony_a7m4() {
        let e = CameraEntry {
            model: "Sony ILCE-7M4".into(),
            make: "Sony".into(),
            short: "ILCE-7M4".into(),
            aliases: aliases_for("Sony", "ILCE-7M4"),
            chinese_aliases: vec!["索尼".into(), "sony".into(), "ilce7m4".into()],
            std_dcp: PathBuf::new(),
        };
        for q in ["索尼a7m4", "a7m4", "A7M4", "7M4", "ILCE-7M4", "sony a7m4"] {
            let t = normalize_query(q);
            assert!(matches(&e, &t), "query {q:?} normalized {t:?} should match");
        }
    }

    #[test]
    fn mainstream_filter_excludes_scanners() {
        // 通过 make 判定，不依赖本机有没有扫描仪 profile
        let make = make_of("Epson Perfection V850");
        assert!(!MAINSTREAM_MAKES.iter().any(|m| m.eq_ignore_ascii_case(&make)));
    }

    #[test]
    fn integration_search_sony_a7m4() {
        // 端到端：从本机 Adobe Standard 目录读真实机型，各种写法都能搜到 ILCE-7M4
        let all = list_cameras();
        assert!(all.len() > 100, "should load real camera list, got {}", all.len());
        // 不应包含 Apple iPhone
        assert!(
            !all.iter().any(|c| c.make == "Apple"),
            "iPhone must be filtered out"
        );

        let target = all.iter().find(|c| c.model == "Sony ILCE-7M4")
            .expect("Sony ILCE-7M4 must be in list");
        assert!(target.std_dcp.as_os_str() != "", "must carry Adobe Standard DCP path");

        for q in ["索尼a7m4", "a7m4", "A7M4", "7M4", "ILCE-7M4", "sony a7m4"] {
            let tokens = normalize_query(q);
            assert!(matches(target, &tokens), "query {q:?} → tokens {tokens:?} should match");
        }
    }

    #[test]
    fn integration_search_canon_r6() {
        let all = list_cameras();
        let target = all.iter().find(|c| c.model.contains("Canon EOS R6"))
            .expect("Canon EOS R6 must be in list");
        for q in ["r6", "R6", "佳能r6", "canon r6"] {
            let tokens = normalize_query(q);
            assert!(matches(target, &tokens), "query {q:?} → tokens {tokens:?} should match");
        }
    }

    #[test]
    fn integration_search_a7m4_exact_results() {
        // "a7m4" 精确搜索：ILCE-7M4 必须命中；ILCE-7M5 不应误报
        let all = list_cameras();
        let tokens = normalize_query("a7m4");
        let hits: Vec<&CameraEntry> = all.iter().filter(|e| matches(e, &tokens)).collect();
        assert!(
            hits.iter().any(|e| e.model == "Sony ILCE-7M4"),
            "ILCE-7M4 must match, hits: {:?}",
            hits.iter().map(|e| e.model.clone()).collect::<Vec<_>>()
        );
        assert!(
            !hits.iter().any(|e| e.model == "Sony ILCE-7M5"),
            "ILCE-7M5 is a false positive for query a7m4, hits: {:?}",
            hits.iter().map(|e| e.model.clone()).collect::<Vec<_>>()
        );
    }

    #[test]
    fn integration_search_a7m_prefix_matches_both() {
        // "a7m" 前缀：7M4 和 7M5 都应命中（前缀模糊）
        let all = list_cameras();
        let tokens = normalize_query("a7m");
        let hits: Vec<&CameraEntry> = all.iter().filter(|e| matches(e, &tokens)).collect();
        assert!(hits.iter().any(|e| e.model == "Sony ILCE-7M4"));
        assert!(hits.iter().any(|e| e.model == "Sony ILCE-7M5"));
    }

    #[test]
    fn integration_search_brand_only_sony() {
        // 用户只输入「索尼」应返回全部 Sony 机型
        let all = list_cameras();
        let tokens = normalize_query("索尼");
        let hits: Vec<&CameraEntry> = all.iter().filter(|e| matches(e, &tokens)).collect();
        let sony_count = all.iter().filter(|e| e.make == "Sony").count();
        assert_eq!(hits.len(), sony_count, "『索尼』应命中全部 Sony 机型，hits={} sony={}", hits.len(), sony_count);
        assert!(hits.iter().any(|e| e.model == "Sony ILCE-7M4"));
        assert!(hits.iter().any(|e| e.model == "Sony ILCE-7M5"));
    }

    #[test]
    fn integration_search_brand_only_canon() {
        let all = list_cameras();
        let tokens = normalize_query("佳能");
        let hits: Vec<&CameraEntry> = all.iter().filter(|e| matches(e, &tokens)).collect();
        let canon_count = all.iter().filter(|e| e.make == "Canon").count();
        assert_eq!(hits.len(), canon_count);
    }

    #[test]
    fn integration_no_chinese_input_falls_back() {
        // 输入没有中文时也要能搜（纯英文空格）
        let all = list_cameras();
        let target = all.iter().find(|c| c.model == "Sony ILCE-7M4").unwrap();
        let tokens = normalize_query("sony a7m4");
        assert!(matches(target, &tokens));
    }
}
