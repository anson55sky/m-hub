//! 速达资源的加密备注（v0.8.0，发布说明 ⑧）。
//!
//! 四条口径，缺一条这套东西就会变成「看起来有、其实不能用」：
//!
//! 1. **密钥只进系统钥匙串，永不落盘**（与 AI Key 同一套 `keyring` 通道，
//!    但**独立服务名** —— 混进 `m-hub-chat` 的话，一条 chat 模型被删/改名时
//!    会连带把加密备注的密钥搞丢，全部密文当场报废且不可逆）。
//! 2. **备注名不加密**（`resources.secret_label` 存明文）：列表里要能一眼看出
//!    「哪条资源有备注」，而全加密的话用户面对的是一串密文。
//!    正文（`secret_note`）才是密文。
//! 3. **密钥读不出来时绝不重新生成**：重新生成会让**已有的全部密文**永久解不开，
//!    而症状是「点开备注报一句看不懂的错」。宁可报错让用户自己处理。
//! 4. **AES-256-GCM，每次新 nonce**：同一条明文两次加密的密文必须不同，
//!    否则「密文相同 ⇒ 明文相同」就成了可比对的指纹。
//!
//! 单元测试**只走纯函数**（`encrypt_with` / `decrypt_with`）——真钥匙串路径
//! 在 CI/单测里会弹系统授权框、也会让测试结果取决于跑测试那台机器的状态。

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Key, Nonce};
use base64::Engine as _;

/// 钥匙串服务名 / 账号名。**刻意与 AI Key 的 `m-hub-chat` 分开**（见头部第 1 条）。
const KEYRING_SERVICE: &str = "m-hub-secret";
const KEY_ACCOUNT: &str = "secret-note-key";

const KEY_LEN: usize = 32;
/// AES-GCM 标准 nonce 长度（96 bit）。
const NONCE_LEN: usize = 12;
/// GCM 认证标签长度。
const TAG_LEN: usize = 16;

fn b64() -> base64::engine::general_purpose::GeneralPurpose {
    base64::engine::general_purpose::STANDARD
}

/// 加密一段备注，返回 base64(`nonce || 密文 || tag`)。
pub fn encrypt(plaintext: &str) -> Result<String, String> {
    encrypt_with(&master_key()?, plaintext)
}

/// 解密 `encrypt` 的产物。
pub fn decrypt(blob: &str) -> Result<String, String> {
    decrypt_with(&master_key()?, blob)
}

/// 纯函数加密（测试与 `encrypt` 共用同一份实现）。
fn encrypt_with(key: &[u8; KEY_LEN], plaintext: &str) -> Result<String, String> {
    let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(key));
    let mut nonce_bytes = [0u8; NONCE_LEN];
    getrandom::fill(&mut nonce_bytes).map_err(|e| format!("生成随机数失败: {e}"))?;
    let ciphertext = cipher
        .encrypt(Nonce::from_slice(&nonce_bytes), plaintext.as_bytes())
        .map_err(|_| "加密失败".to_string())?;
    let mut out = Vec::with_capacity(NONCE_LEN + ciphertext.len());
    out.extend_from_slice(&nonce_bytes);
    out.extend_from_slice(&ciphertext);
    Ok(b64().encode(out))
}

/// 纯函数解密。
fn decrypt_with(key: &[u8; KEY_LEN], blob: &str) -> Result<String, String> {
    let raw = b64()
        .decode(blob.trim())
        .map_err(|_| "备注数据已损坏（不是有效的密文）".to_string())?;
    // 太短就装不下 nonce + 认证标签：早判早给错，不去喂 AES-GCM 报一堆
    // 「invalid length」——那句话用户看不懂，也指不到「数据被改过」。
    if raw.len() < NONCE_LEN + TAG_LEN {
        return Err("备注数据不完整".into());
    }
    let (nonce, body) = raw.split_at(NONCE_LEN);
    let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(key));
    let plain = cipher.decrypt(Nonce::from_slice(nonce), body).map_err(|_| {
        "解密失败：备注数据与当前密钥不匹配（可能换过系统账号，或数据被改过）".to_string()
    })?;
    String::from_utf8(plain).map_err(|_| "备注内容不是合法文本".into())
}

/// 取主密钥；没有就生成一个并存进钥匙串。
///
/// ⚠️ **读不到（或读出来不对）时绝不重新生成**（见头部第 3 条）。
fn master_key() -> Result<[u8; KEY_LEN], String> {
    let entry = keyring::Entry::new(KEYRING_SERVICE, KEY_ACCOUNT)
        .map_err(|e| format!("访问系统钥匙串失败: {e}"))?;
    match entry.get_password() {
        Ok(stored) => decode_key(&stored),
        Err(keyring::Error::NoEntry) => {
            let mut key = [0u8; KEY_LEN];
            getrandom::fill(&mut key).map_err(|e| format!("生成密钥失败: {e}"))?;
            entry
                .set_password(&b64().encode(key))
                .map_err(|e| format!("把密钥写入系统钥匙串失败: {e}"))?;
            log::info!("已为加密备注生成新的主密钥并存入系统钥匙串");
            Ok(key)
        }
        Err(e) => Err(format!("读取系统钥匙串中的密钥失败: {e}")),
    }
}

/// 解析钥匙串里存的那串 base64 密钥。**长度不对就报错，绝不重算**（头部第 3 条）。
fn decode_key(stored: &str) -> Result<[u8; KEY_LEN], String> {
    let bytes = b64()
        .decode(stored.trim())
        .map_err(|_| "钥匙串里的加密备注密钥已损坏（无法解析）".to_string())?;
    if bytes.len() != KEY_LEN {
        return Err(format!(
            "钥匙串里的加密备注密钥长度不对（{} 字节，应为 {KEY_LEN} 字节）",
            bytes.len()
        ));
    }
    let mut key = [0u8; KEY_LEN];
    key.copy_from_slice(&bytes);
    Ok(key)
}

#[cfg(test)]
mod tests {
    use super::*;

    const K1: [u8; KEY_LEN] = [7u8; KEY_LEN];
    const K2: [u8; KEY_LEN] = [9u8; KEY_LEN];

    /// 加密 → 解密必须原样回来，且密文不是明文（没做「假装加密」这种装饰）。
    #[test]
    fn round_trip_is_exact_and_ciphertext_hides_plaintext() {
        for plain in [
            "",
            "一行",
            "多行\n第二行\n\n- 列表项\n- 第二项",
            "密码：hunter2；API Key：sk-abcdef",
            "中文与 emoji 🧪 混排",
            &"长".repeat(20_000),
        ] {
            let blob = encrypt_with(&K1, plain).unwrap();
            assert_ne!(blob, plain, "密文与明文相同 = 没加密");
            assert!(!blob.contains("mhub-note") && !blob.contains("hunter2"));
            assert_eq!(decrypt_with(&K1, &blob).unwrap(), plain);
        }
    }

    /// 同一明文两次加密必须得到**不同**密文（GCM nonce 随机），且都能解回原文。
    /// 这是「密文相同即明文相同」那种可比对指纹的唯一防线。
    #[test]
    fn nonce_is_fresh_per_encryption() {
        let a = encrypt_with(&K1, "同一条备注").unwrap();
        let b = encrypt_with(&K1, "同一条备注").unwrap();
        assert_ne!(a, b, "两次加密结果相同 ⇒ nonce 被复用了");
        assert_eq!(decrypt_with(&K1, &a).unwrap(), "同一条备注");
        assert_eq!(decrypt_with(&K1, &b).unwrap(), "同一条备注");
    }

    /// 密文被改动一个字节就必须解不开（GCM 认证）。
    #[test]
    fn tampering_is_detected() {
        let blob = encrypt_with(&K1, "别改我").unwrap();
        let mut raw = b64().decode(&blob).unwrap();
        let last = raw.len() - 1;
        raw[last] ^= 0x01;
        let tampered = b64().encode(&raw);
        let err = decrypt_with(&K1, &tampered).unwrap_err();
        assert!(err.contains("解密失败"), "{err}");

        // 改 nonce 也一样（GCM 的 nonce 也参与认证）
        let mut raw2 = b64().decode(&blob).unwrap();
        raw2[0] ^= 0x80;
        assert!(decrypt_with(&K1, &b64().encode(&raw2)).is_err());
    }

    /// 换一把密钥解不开旧密文（这就是「密钥丢了 = 数据永久不可逆」的由来）。
    #[test]
    fn wrong_key_fails_loudly() {
        let blob = encrypt_with(&K1, "密文").unwrap();
        let err = decrypt_with(&K2, &blob).unwrap_err();
        assert!(err.contains("解密失败"), "{err}");
    }

    /// 残缺 / 非 base64 的数据要给**人话**错误，不能把 AES 内部的报错抛给用户。
    #[test]
    fn malformed_input_gives_readable_errors() {
        assert!(decrypt_with(&K1, "这不是 base64!!").unwrap_err().contains("损坏"));
        assert!(decrypt_with(&K1, "").unwrap_err().contains("不完整"));
        assert!(decrypt_with(&K1, &b64().encode([0u8; 20])).unwrap_err().contains("不完整"));
    }

    /// 钥匙串里那串密钥长度不对时**报错**，不得当成「没有密钥」而重新生成 ——
    /// 重新生成会让已有的全部密文永久解不开。
    #[test]
    fn bad_key_length_is_rejected_not_regenerated() {
        let err = decode_key(&b64().encode([1u8; 8])).unwrap_err();
        assert!(err.contains("长度不对"), "{err}");
        assert!(decode_key("不是 base64").unwrap_err().contains("损坏"));
    }
}