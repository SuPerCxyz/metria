//! 模型名与 Provider 名的归一化，以及通配模式匹配。
//!
//! 归一化只做确定性、可重复的轻量转换；不得丢原始名称（raw 始终保留）。

use crate::error::ModelError;

/// 归一化模型名。
///
/// 规则（纯本地、可重复）：
/// 1. 去空白、转小写；
/// 2. 处理常见别名（如 `claude-opus-4-6` → `claude-opus-4.6`）；
/// 3. 未知模型保持小写化后的原始串，不猜测。
pub fn normalize_model(raw: &str) -> String {
    let mut s = raw.trim().to_ascii_lowercase();
    // Claude API 名称中的数字连接线 `-` 归一化为 `.`
    if s.starts_with("claude-") {
        s = collapse_claude_digits(&s);
    }
    // 连续空白折叠
    s = s.split_whitespace().collect::<Vec<_>>().join(" ");
    s
}

fn collapse_claude_digits(s: &str) -> String {
    // claude-opus-4-6 -> claude-opus-4.6（仅版本段）
    let mut parts: Vec<&str> = s.split('-').collect();
    if parts.len() >= 4 {
        // 尝试从末尾开始找两个纯数字段并合并
        let n = parts.len();
        if parts[n - 2].chars().all(|c| c.is_ascii_digit())
            && parts[n - 1].chars().all(|c| c.is_ascii_digit())
        {
            let merged = format!("{}.{}", parts[n - 2], parts[n - 1]);
            parts.truncate(n - 2);
            parts.push(&merged);
            return parts.join("-");
        }
    }
    s.to_string()
}

/// 归一化 Provider 名。
///
/// 已知别名映射；未知值小写返回（保留原始串由调用方保存）。
pub fn normalize_provider(raw: &str) -> String {
    let s = raw.trim().to_ascii_lowercase();
    match s.as_str() {
        "claude" => "anthropic".to_string(),
        "openai" | "openai-compatible" | "azure-openai" => s.clone(),
        "deepseek" => s.clone(),
        other => other.to_string(),
    }
}

/// 通配模式匹配：支持 `*`（任意长度）与 `?`（单字符）。
pub fn pattern_match(pattern: &str, value: &str) -> bool {
    wildcard_match(pattern, value)
}

fn wildcard_match(pattern: &str, value: &str) -> bool {
    let p: Vec<char> = pattern.chars().collect();
    let v: Vec<char> = value.chars().collect();
    let (mut pi, mut vi) = (0usize, 0usize);
    let mut star: Option<usize> = None;
    let mut star_match = 0usize;
    while vi < v.len() {
        if pi < p.len() && (p[pi] == '?' || p[pi] == v[vi]) {
            pi += 1;
            vi += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star = Some(pi);
            star_match = vi;
            pi += 1;
        } else if let Some(sp) = star {
            star_match += 1;
            vi = star_match;
            pi = sp + 1;
        } else {
            return false;
        }
    }
    while pi < p.len() && p[pi] == '*' {
        pi += 1;
    }
    pi == p.len()
}

/// 校验模型字符串非空且不超长。
pub fn validate_model(s: &str) -> Result<(), ModelError> {
    if s.trim().is_empty() || s.chars().count() > 200 {
        return Err(ModelError::Normalize("模型名非法".to_string()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_normalization() {
        assert_eq!(normalize_model("claude-opus-4-6"), "claude-opus-4.6");
        assert_eq!(normalize_model("  GPT-4o  "), "gpt-4o");
        assert_eq!(normalize_model("claude-sonnet-4-5"), "claude-sonnet-4.5");
        assert_eq!(normalize_model("o3-mini"), "o3-mini");
    }

    #[test]
    fn provider_normalization() {
        assert_eq!(normalize_provider("Claude"), "anthropic");
        assert_eq!(normalize_provider("ANTHROPIC"), "anthropic");
        assert_eq!(normalize_provider("deepseek"), "deepseek");
    }

    #[test]
    fn wildcard_matching() {
        assert!(pattern_match("claude-*", "claude-opus-4.6"));
        assert!(pattern_match("*", "anything"));
        assert!(pattern_match("claude-?onnet-*", "claude-sonnet-4.5"));
        assert!(!pattern_match("claude-*", "gpt-4o"));
        assert!(pattern_match("gpt-4o", "gpt-4o"));
        assert!(!pattern_match("gpt-4o", "gpt-4o-mini"));
    }
}
