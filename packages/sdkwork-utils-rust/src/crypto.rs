use crate::encoding::base64url_encode;
use aes_gcm::{aead::Aead, Aes256Gcm, KeyInit, Nonce};
use base64::{engine::general_purpose::STANDARD, Engine as _};
use hkdf::Hkdf;
use hmac::{Hmac, Mac};
use rand::RngCore;
use sha2::{Digest, Sha256};

type HmacSha256 = Hmac<Sha256>;

/// AES-256-GCM 随机 nonce 长度（12 字节，NIST 推荐）。
const AES_GCM_NONCE_LEN: usize = 12;
/// AES-256 密钥长度（32 字节）。
const AES_256_KEY_LEN: usize = 32;

pub fn sha256_hash(value: &[u8]) -> String {
    let digest = Sha256::digest(value);
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// The raw 32-byte SHA-256 digest, for callers that need to encode it
/// themselves (base64url, base64, raw bytes) instead of the hex form
/// [`sha256_hash`] returns.
pub fn sha256_digest(value: &[u8]) -> [u8; 32] {
    Sha256::digest(value).into()
}

pub fn hmac_sha256(value: &[u8], secret: &[u8]) -> String {
    let mut mac = <HmacSha256 as Mac>::new_from_slice(secret).expect("hmac key");
    mac.update(value);
    mac.finalize()
        .into_bytes()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

pub fn hmac_sha256_base64url(value: &[u8], secret: &[u8]) -> String {
    let mut mac = <HmacSha256 as Mac>::new_from_slice(secret).expect("hmac key");
    mac.update(value);
    base64url_encode(&mac.finalize().into_bytes())
}

pub fn verify_hmac_sha256_base64url(value: &[u8], secret: &[u8], signature: &[u8]) -> bool {
    let mut mac = <HmacSha256 as Mac>::new_from_slice(secret).expect("hmac key");
    mac.update(value);
    mac.verify_slice(signature).is_ok()
}

pub fn secure_compare(left: &str, right: &str) -> bool {
    if left.len() != right.len() {
        return false;
    }
    let mut result = 0u8;
    for (left_byte, right_byte) in left.bytes().zip(right.bytes()) {
        result |= left_byte ^ right_byte;
    }
    result == 0
}

/// 使用 HKDF-SHA256 从高熵密钥材料派生 32 字节 AES-256 密钥。
///
/// 相较裸 SHA-256 摘要做密钥，HKDF 提供：
/// - 盐值隔离（不同 salt 派生不同密钥）
/// - 符合 RFC 5869 标准的密钥派生流程
/// - info 上下文绑定（防止跨用途密钥复用）
pub fn derive_aes_256_key(secret: &[u8], salt: &[u8], info: &[u8]) -> [u8; AES_256_KEY_LEN] {
    let hkdf = Hkdf::<Sha256>::new(Some(salt), secret);
    let mut okm = [0_u8; AES_256_KEY_LEN];
    hkdf.expand(info, &mut okm)
        .expect("HKDF-SHA256 expand 32 bytes always succeeds");
    okm
}

/// AES-256-GCM 加密：随机 nonce + 密文，输出 base64(nonce || ciphertext)。
///
/// 安全特性：
/// - 每次加密使用 CSPRNG 生成的随机 12 字节 nonce
/// - nonce 与密文一同存储，解密时无需额外传递 nonce
pub fn aes_gcm_encrypt(key: &[u8], plaintext: &[u8]) -> Result<String, String> {
    let cipher = Aes256Gcm::new_from_slice(key)
        .map_err(|error| format!("invalid AES key length: {error}"))?;
    let mut nonce_bytes = [0_u8; AES_GCM_NONCE_LEN];
    rand::thread_rng().fill_bytes(&mut nonce_bytes);
    let nonce = Nonce::from_slice(&nonce_bytes);
    let ciphertext = cipher
        .encrypt(nonce, plaintext)
        .map_err(|error| format!("AES-256-GCM encrypt: {error}"))?;
    let mut payload = Vec::with_capacity(AES_GCM_NONCE_LEN + ciphertext.len());
    payload.extend_from_slice(&nonce_bytes);
    payload.extend_from_slice(&ciphertext);
    Ok(STANDARD.encode(&payload))
}

/// AES-256-GCM 解密：输入 base64(nonce || ciphertext)，返回明文。
pub fn aes_gcm_decrypt(key: &[u8], encoded: &str) -> Result<Vec<u8>, String> {
    let payload = STANDARD
        .decode(encoded)
        .map_err(|error| format!("base64 decode encrypted payload: {error}"))?;
    if payload.len() <= AES_GCM_NONCE_LEN {
        return Err("encrypted payload too short".to_string());
    }
    let (nonce_bytes, ciphertext) = payload.split_at(AES_GCM_NONCE_LEN);
    let cipher = Aes256Gcm::new_from_slice(key)
        .map_err(|error| format!("invalid AES key length: {error}"))?;
    let nonce = Nonce::from_slice(nonce_bytes);
    cipher
        .decrypt(nonce, ciphertext)
        .map_err(|error| format!("AES-256-GCM decrypt: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::encoding::base64url_decode;

    #[test]
    fn crypto_helpers() {
        assert_eq!(
            sha256_hash(b"hello"),
            "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
        );
        assert_eq!(
            hmac_sha256(b"payload", b"secret"),
            "b82fcb791acec57859b989b430a826488ce2e479fdf92326bd0a2e8375a42ba4"
        );
        let signature = hmac_sha256_base64url(b"payload", b"secret");
        assert!(verify_hmac_sha256_base64url(
            b"payload",
            b"secret",
            &base64url_decode(signature.as_str()).expect("signature")
        ));
    }

    #[test]
    fn aes_gcm_encrypt_decrypt_round_trip() {
        let key = derive_aes_256_key(b"master-secret", b"salt", b"sdkwork-env-variable");
        let plaintext = b"super-secret-api-key-12345";
        let encoded = aes_gcm_encrypt(&key, plaintext).expect("encrypt");
        let decoded = aes_gcm_decrypt(&key, &encoded).expect("decrypt");
        assert_eq!(decoded, plaintext);
    }

    #[test]
    fn aes_gcm_encrypt_produces_different_ciphertexts() {
        // 相同明文每次加密应产生不同密文（随机 nonce）
        let key = derive_aes_256_key(b"master-secret", b"salt", b"sdkwork-env-variable");
        let plaintext = b"same-secret";
        let encoded_a = aes_gcm_encrypt(&key, plaintext).expect("encrypt a");
        let encoded_b = aes_gcm_encrypt(&key, plaintext).expect("encrypt b");
        assert_ne!(encoded_a, encoded_b);
        assert_eq!(
            aes_gcm_decrypt(&key, &encoded_a).expect("decrypt a"),
            plaintext
        );
    }

    #[test]
    fn derive_aes_256_key_is_deterministic() {
        // 相同 secret + salt + info 应派生相同密钥
        let key_a = derive_aes_256_key(b"secret", b"salt", b"info");
        let key_b = derive_aes_256_key(b"secret", b"salt", b"info");
        assert_eq!(key_a, key_b);
        // 不同 salt 派生不同密钥
        let key_c = derive_aes_256_key(b"secret", b"different-salt", b"info");
        assert_ne!(key_a, key_c);
    }
}
