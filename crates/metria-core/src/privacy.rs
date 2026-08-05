//! 隐私脱敏：路径哈希、敏感信息擦除。
//!
//! 默认不上传：完整绝对路径、用户名、Hostname、Git Remote、环境变量、
//! API Key、Authorization、Cookie、SSH 私钥、数据库连接串。

use std::sync::OnceLock;

use regex::Regex;

use crate::model::ContentHash;

/// 将完整路径哈希化（blake3），禁止默认上传完整路径。
pub fn hash_path(path: &str) -> ContentHash {
    ContentHash::hash_str(path)
}

/// 脱敏文本：擦除常见敏感模式。
pub fn redact_text(input: &str) -> String {
    let mut out = input.to_string();
    for re in sensitive_patterns() {
        out = re.replace_all(&out, "[REDACTED]").into_owned();
    }
    out
}

/// 脱敏 URL：去掉查询参数中的 token/key/secret 等。
pub fn redact_url(input: &str) -> String {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| {
        Regex::new(
            r#"(?i)(\b(token|key|secret|password|access_token|api_key|sig|signature)=)[^&;\s"']+"#,
        )
        .expect("static regex")
    });
    re.replace_all(input, "${1}[REDACTED]").into_owned()
}

/// 提取 git remote 的哈希（先脱敏 URL 再哈希）。
pub fn git_remote_hash(remote: &str) -> ContentHash {
    let redacted = redact_url(remote);
    hash_path(&redacted)
}

fn sensitive_patterns() -> &'static Vec<Regex> {
    static PATTERNS: OnceLock<Vec<Regex>> = OnceLock::new();
    PATTERNS.get_or_init(|| {
        vec![
            // Authorization / Cookie 头
            Regex::new(r"(?i)\b(Authorization|Proxy-Authorization|Cookie)\s*:\s*[^\r\n]+")
                .expect("re"),
            // 常见 API Key 前缀
            Regex::new(r"(?i)\bsk-[A-Za-z0-9_\-]{16,}").expect("re"),
            Regex::new(r"(?i)\b[A-Z][A-Z0-9]{15}").expect("re"),
            // 连接串 / URI 中的凭据
            Regex::new(r"(?i)([a-z]+://)[^/\s@:]+:[^/\s@]+@").expect("re"),
            // JSON 字段中的 key/secret 值
            Regex::new(r#"(?i)"(api[_-]?key|access[_-]?token|secret|password|authorization)"\s*:\s*"[^"]+""#)
                .expect("re"),
            // SSH 私钥块
            Regex::new(
                r"(?s)-----BEGIN (RSA |OPENSSH |EC |DSA |ENCRYPTED )?PRIVATE KEY-----.*?-----END.*?-----",
            )
            .expect("re"),
        ]
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_hashed_not_raw() {
        let h = hash_path("/home/user/projects/secret-repo");
        assert!(!h.as_str().contains("secret-repo"));
        assert_eq!(h.as_str().len(), 64);
    }

    #[test]
    fn redact_authorization() {
        let s = "Authorization: Bearer sk-abcdef1234567890abcdef1234567890";
        let r = redact_text(s);
        assert!(r.contains("[REDACTED]"));
        assert!(!r.contains("sk-abcdef"));
    }

    #[test]
    fn redact_sk_key_anywhere() {
        let s = "using key sk-proj-0123456789abcdef0123456789abcdef";
        let r = redact_text(s);
        assert!(!r.contains("sk-proj"));
    }

    #[test]
    fn redact_ssh_key_block() {
        let s = "-----BEGIN OPENSSH PRIVATE KEY-----\nabc\n-----END OPENSSH PRIVATE KEY-----";
        let r = redact_text(s);
        assert!(r.contains("[REDACTED]"));
        assert!(!r.contains("BEGIN OPENSSH"));
    }

    #[test]
    fn redact_url_token() {
        let s = "https://example.com/x?token=SECRETVALUE&id=1";
        let r = redact_url(s);
        assert!(r.contains("[REDACTED]"));
        assert!(!r.contains("SECRETVALUE"));
    }

    #[test]
    fn git_remote_hashed() {
        let h = git_remote_hash("https://user:pass@example.com/org/repo.git");
        assert_eq!(h.as_str().len(), 64);
    }
}
