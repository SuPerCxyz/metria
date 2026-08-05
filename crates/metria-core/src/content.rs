//! 内容分类（轻量启发式）与 UTF-8 字节统计。
//!
//! 分类用于 bytes-per-token 差异明显的场景（中文/英文/代码/JSON/base64 等）。
//! 禁止使用大型 AI 模型分类；算法本地、可重复、低资源、允许未知。

use crate::model::ContentProfile;

/// 统计 UTF-8 字节数。
pub fn utf8_bytes(s: &str) -> usize {
    s.len()
}

/// 内容分类主入口。
pub fn classify(text: &str) -> ContentProfile {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return ContentProfile::Unknown;
    }
    let len = trimmed.len();
    if len < 8 {
        // 极短文本不分类
        return ContentProfile::Unknown;
    }
    if looks_like_base64(trimmed, len) {
        return ContentProfile::Base64;
    }
    if looks_like_json(trimmed) {
        return ContentProfile::Json;
    }
    if looks_like_xml(trimmed) {
        return ContentProfile::Xml;
    }
    if looks_like_log(trimmed) {
        return ContentProfile::Log;
    }
    if looks_like_terminal(trimmed) {
        return ContentProfile::TerminalOutput;
    }
    if looks_like_markdown(trimmed) {
        return ContentProfile::Markdown;
    }
    if looks_like_source_code(trimmed) {
        return ContentProfile::SourceCode;
    }
    // 语言比例
    let (zh, en, ascii_other, total) = char_stats(text);
    if total == 0 {
        return ContentProfile::Unknown;
    }
    let zh_ratio = zh as f32 / total as f32;
    let en_ratio = en as f32 / total as f32;
    if zh_ratio >= 0.3 {
        return ContentProfile::NaturalLanguageZh;
    }
    if en_ratio >= 0.5 {
        return ContentProfile::NaturalLanguageEn;
    }
    if ascii_other > 0 && en_ratio < 0.3 && zh_ratio < 0.3 {
        return ContentProfile::Mixed;
    }
    ContentProfile::Unknown
}

fn looks_like_json(s: &str) -> bool {
    let t = s.trim_start();
    if t.starts_with('{') || t.starts_with('[') {
        // 快速结构校验：成对引号
        return s.contains('"') && (s.contains(':') || s.contains(']'));
    }
    false
}

fn looks_like_xml(s: &str) -> bool {
    s.starts_with('<')
        && s.contains('>')
        && (s.contains("</") || s.contains("/>") || s.contains("<?xml"))
}

fn looks_like_base64(s: &str, len: usize) -> bool {
    if len < 32 {
        return false;
    }
    let b64_chars = s
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '+' || *c == '/' || *c == '=' || *c == '\n')
        .count();
    b64_chars as f32 / len as f32 > 0.9 && s.contains('=') && s.len() % 4 == 0
}

fn looks_like_markdown(s: &str) -> bool {
    s.contains("```")
        || s.lines().any(|l| l.trim_start().starts_with('#'))
        || s.contains("**")
        || s.contains("## ")
}

fn looks_like_log(s: &str) -> bool {
    s.lines().any(|l| {
        let l = l.trim_start();
        l.starts_with("202")
            && l.contains('-')
            && (l.contains('T') || l.contains(' '))
            && l.contains(':')
    })
}

fn looks_like_terminal(s: &str) -> bool {
    s.contains('\u{1b}') || s.contains('$') && s.lines().any(|l| l.trim_start().starts_with('$'))
}

const CODE_KEYWORDS: &[&str] = &[
    "fn ",
    "func ",
    "function ",
    "def ",
    "let ",
    "const ",
    "var ",
    "return ",
    "if ",
    "else ",
    "for ",
    "while ",
    "class ",
    "struct ",
    "impl ",
    "import ",
    "use ",
    "package ",
    "public ",
    "private ",
    "SELECT ",
    "INSERT ",
    "{",
    "}",
    ";",
    "=>",
    "::",
    "->",
    "=>",
];

fn looks_like_source_code(s: &str) -> bool {
    let upper = s.to_ascii_uppercase();
    let keyword_hits = CODE_KEYWORDS
        .iter()
        .filter(|k| upper.contains(&k.to_ascii_uppercase()))
        .count();
    let lines = s.lines().count();
    (keyword_hits >= 2 && lines >= 2) || (s.contains('\t') && lines >= 3)
}

fn char_stats(s: &str) -> (u32, u32, u32, u32) {
    let mut zh = 0u32;
    let mut en = 0u32;
    let mut other = 0u32;
    let mut total = 0u32;
    for c in s.chars() {
        if c.is_ascii_alphabetic() {
            en += 1;
        } else if c.is_ascii() {
            other += 1;
        } else {
            zh += 1; // 非 ASCII 粗略计为中日韩
        }
        total += 1;
    }
    (zh, en, other, total)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chinese_detection() {
        let c = classify("你好，请问今天上海的天气怎么样？我打算出门散步。");
        assert_eq!(c, ContentProfile::NaturalLanguageZh);
    }

    #[test]
    fn english_detection() {
        let c = classify("Hello, how are you today? I hope you are doing well.");
        assert_eq!(c, ContentProfile::NaturalLanguageEn);
    }

    #[test]
    fn source_code_detection() {
        let c = classify("fn main() {\n    let x = 42;\n    println!(\"{}\", x);\n}");
        assert_eq!(c, ContentProfile::SourceCode);
    }

    #[test]
    fn json_detection() {
        let c = classify(r#"{"name":"metria","tokens":100}"#);
        assert_eq!(c, ContentProfile::Json);
    }

    #[test]
    fn base64_detection() {
        let c = classify("aGVsbG8gd29ybGQgdGhpcyBpcyBhIGxvbmcgYmFzZTY0IHN0cmluZw==");
        assert_eq!(c, ContentProfile::Base64);
    }

    #[test]
    fn unknown_on_short() {
        assert_eq!(classify("hi"), ContentProfile::Unknown);
        assert_eq!(classify(""), ContentProfile::Unknown);
    }

    #[test]
    fn utf8_byte_count() {
        // "你好" = 6 bytes
        assert_eq!(utf8_bytes("你好"), 6);
        assert_eq!(utf8_bytes("abc"), 3);
    }
}
