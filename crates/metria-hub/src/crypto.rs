//! 节点 token 可逆加密（Pull 模式）。
//!
//! Hub 需持有节点 token 明文才能向 Agent 认证（pull 调度），与「只存哈希」原则冲突；
//! 折衷：以 `METRIA_SESSION_SECRET` 经 HKDF-SHA256 派生密钥做 AES-256-GCM 加密存储。
//! 轮换 session secret 会使已存 token 无法解密，需在 Web 重新生成安装命令。

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce};
use base64::Engine;
use hkdf::Hkdf;
use sha2::Sha256;

const INFO: &[u8] = b"metria-node-token";
/// AES-GCM 标准 96 位 nonce。
const NONCE_LEN: usize = 12;

fn derive_key() -> [u8; 32] {
    let secret = std::env::var("METRIA_SESSION_SECRET")
        .unwrap_or_else(|_| "metria-dev-session-secret-change-me".into());
    let hk = Hkdf::<Sha256>::new(None, secret.as_bytes());
    let mut key = [0u8; 32];
    hk.expand(INFO, &mut key).expect("hkdf expand 32 bytes");
    key
}

/// 加密节点 token：返回 base64(nonce || ciphertext)。
pub fn encrypt_node_token(plain: &str) -> Result<String, String> {
    let cipher = Aes256Gcm::new_from_slice(&derive_key()).map_err(|e| e.to_string())?;
    let mut nonce_bytes = [0u8; NONCE_LEN];
    use aes_gcm::aead::rand_core::RngCore;
    use aes_gcm::aead::OsRng as AeadOsRng;
    AeadOsRng.fill_bytes(&mut nonce_bytes);
    let ct = cipher
        .encrypt(Nonce::from_slice(&nonce_bytes), plain.as_bytes())
        .map_err(|e| e.to_string())?;
    let mut out = nonce_bytes.to_vec();
    out.extend(ct);
    Ok(base64::engine::general_purpose::STANDARD.encode(out))
}

/// 解密节点 token；密钥不匹配或数据损坏返回 None。
pub fn decrypt_node_token(enc: &str) -> Option<String> {
    let raw = base64::engine::general_purpose::STANDARD.decode(enc).ok()?;
    if raw.len() <= NONCE_LEN {
        return None;
    }
    let (nonce, ct) = raw.split_at(NONCE_LEN);
    let cipher = Aes256Gcm::new_from_slice(&derive_key()).ok()?;
    let plain = cipher.decrypt(Nonce::from_slice(nonce), ct).ok()?;
    String::from_utf8(plain).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_and_tamper() {
        let enc = encrypt_node_token("mct-abc123").unwrap();
        assert_eq!(decrypt_node_token(&enc).as_deref(), Some("mct-abc123"));
        // 篡改后解密失败
        let mut tampered = enc.clone();
        tampered.replace_range(4..5, if &enc[4..5] == "A" { "B" } else { "A" });
        assert!(
            decrypt_node_token(&tampered).is_none()
                || decrypt_node_token(&tampered).as_deref() != Some("mct-abc123")
        );
    }
}
