////! Phase 4: 包签名（HMAC-SHA256，设计方案 13.2）
////!
//! 签名流程：
//! 1. 从 .auz 读取 META-INF/checksum.sha256 文件
//! 2. 对校验和文件内容计算 HMAC-SHA256
//! 3. 将签名写入 META-INF/signature.sig
//! 4. 验证时重新计算 HMAC 并比较

use hmac::{Hmac, Mac};
use sha2::Sha256;

/// 签名文件路径
pub const SIGNATURE_PATH: &str = "META-INF/signature.sig";
/// 校验和文件路径
pub const CHECKSUM_PATH: &str = "META-INF/checksum.sha256";

type HmacSha256 = Hmac<Sha256>;

/// 签名错误
#[derive(Debug, Clone)]
pub enum SigningError {
    Io(String),
    Key(String),
    Signature(String),
}

impl std::fmt::Display for SigningError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SigningError::Io(m) => write!(f, "io error: {}", m),
            SigningError::Key(m) => write!(f, "key error: {}", m),
            SigningError::Signature(m) => write!(f, "signature error: {}", m),
        }
    }
}

/// 密钥配置
#[derive(Debug, Clone)]
pub struct KeyConfig {
    /// HMAC 密钥（hex 字符串）
    pub hmac_key: String,
    /// 签名者标识
    pub signer: String,
    /// 签名时间（Unix 时间戳）
    pub timestamp: u64,
}

impl KeyConfig {
    /// 从环境变量加载密钥配置
    pub fn from_env() -> Result<Self, SigningError> {
        let hmac_key = std::env::var("AURA_SIGNING_KEY")
            .map_err(|_| SigningError::Key("AURA_SIGNING_KEY 未设置".to_string()))?;
        let signer = std::env::var("AURA_SIGNER")
            .unwrap_or_else(|_| "unknown".to_string());
        let timestamp = std::env::var("AURA_TIMESTAMP")
            .ok()
            .and_then(|t| t.parse().ok())
            .unwrap_or_else(|| {
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_secs()
            });
        Ok(KeyConfig { hmac_key, signer, timestamp })
    }

    /// 从密钥文件加载（第一行为 hex 密钥，第二行为签名者）
    pub fn from_file(path: &str) -> Result<Self, SigningError> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| SigningError::Io(format!("无法读取密钥文件: {}", e)))?;
        let mut lines = content.lines();
        let hmac_key = lines
            .next()
            .ok_or_else(|| SigningError::Key("密钥文件为空".to_string()))?
            .trim()
            .to_string();
        let signer = lines
            .next()
            .map(|s| s.trim().to_string())
            .unwrap_or_else(|| "unknown".to_string());
        Ok(KeyConfig { hmac_key, signer, timestamp: 0 })
    }
}

/// 签名器 — 生成 HMAC 签名
pub struct Signer {
    config: KeyConfig,
}

impl Signer {
    pub fn new(config: KeyConfig) -> Self {
        Self { config }
    }

    /// 对校验和文件内容签名
    pub fn sign(&self, checksum_content: &str) -> Result<String, SigningError> {
        let key = hex::decode(&self.config.hmac_key)
            .map_err(|e| SigningError::Key(format!("密钥 hex 解码失败: {}", e)))?;

        let mut mac = HmacSha256::new_from_slice(&key)
            .map_err(|e| SigningError::Key(e.to_string()))?;
        mac.update(checksum_content.as_bytes());
        let signature = mac.finalize().into_bytes();

        Ok(hex::encode(signature))
    }

    /// 生成签名文件内容
    pub fn generate_signature_file(
        &self,
        checksum_content: &str,
    ) -> Result<String, SigningError> {
        let signature = self.sign(checksum_content)?;
        Ok(format!(
            "{}\n{}\n{}\n",
            self.config.signer, self.config.timestamp, signature
        ))
    }
}

/// 签名验证器
pub struct Verifier;

impl Verifier {
    /// 验证签名文件
    pub fn verify(
        checksum_content: &str,
        signature_file: &str,
    ) -> Result<bool, SigningError> {
        let lines: Vec<&str> = signature_file.lines().collect();
        if lines.len() < 3 {
            return Err(SigningError::Signature(
                "签名文件格式错误".to_string(),
            ));
        }

        let _signer = lines[0].trim();
        let _timestamp: u64 = lines[1].trim().parse().map_err(|_| {
            SigningError::Signature("时间戳格式错误".to_string())
        })?;
        let signature_hex = lines[2].trim();

        // 获取密钥
        let key_config = KeyConfig::from_env()?;
        let key = hex::decode(&key_config.hmac_key)
            .map_err(|e| SigningError::Key(format!("密钥 hex 解码失败: {}", e)))?;

        // 重新计算 HMAC
        let mut mac = HmacSha256::new_from_slice(&key)
            .map_err(|e| SigningError::Key(e.to_string()))?;
        mac.update(checksum_content.as_bytes());
        let expected = hex::encode(mac.finalize().into_bytes());

        Ok(expected == signature_hex)
    }
}

/// 签名工具函数
pub mod signing_utils {
    use super::*;

    /// 对字节内容计算 HMAC-SHA256
    pub fn hmac_sha256(key: &[u8], data: &[u8]) -> Result<Vec<u8>, SigningError> {
        let mut mac = HmacSha256::new_from_slice(key)
            .map_err(|e| SigningError::Key(e.to_string()))?;
        mac.update(data);
        Ok(mac.finalize().into_bytes().to_vec())
    }

    /// 验证 HMAC
    pub fn verify_hmac(key: &[u8], data: &[u8], signature: &[u8]) -> Result<bool, SigningError> {
        let expected = hmac_sha256(key, data)?;
        Ok(expected == signature.to_vec())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sign_and_verify() {
        let key_config = KeyConfig {
            hmac_key: "deadbeefdeadbeefdeadbeefdeadbeef".to_string(),
            signer: "test".to_string(),
            timestamp: 1234567890,
        };
        let signer = Signer::new(key_config);

        let checksum = "abc123  META-INF/aura.toml\ndef456  lib/test.auc\n";
        let sig_file = signer.generate_signature_file(checksum).unwrap();

        assert!(sig_file.contains("test"));
        assert!(sig_file.contains("1234567890"));
        // 签名是 64 字符的 hex
        let sig_line = sig_file.lines().nth(2).unwrap();
        assert_eq!(sig_line.len(), 64);
    }

    fn test_hmac_roundtrip() {
        let key = b"my-secret-key";
        let data = b"some data to sign";

        let sig = signing_utils::hmac_sha256(key, data).unwrap();
        assert_eq!(sig.len(), 32);

        assert!(signing_utils::verify_hmac(key, data, &sig).unwrap());
        assert!(!signing_utils::verify_hmac(b"wrong-key", data, &sig).unwrap());
    }

    #[test]
    fn test_sign_deterministic() {
        let key_config = KeyConfig {
            hmac_key: "aabbccdd".to_string(),
            signer: "test".to_string(),
            timestamp: 0,
        };
        let signer = Signer::new(key_config);

        let sig1 = signer.sign("hello").unwrap();
        let sig2 = signer.sign("hello").unwrap();
        assert_eq!(sig1, sig2);
    }

    #[test]
    fn test_bad_key() {
        let key_config = KeyConfig {
            hmac_key: "not-hex".to_string(),
            signer: "test".to_string(),
            timestamp: 0,
        };
        let signer = Signer::new(key_config);
        assert!(signer.sign("hello").is_err());
    }

    fn test_bad_signature_format() {
        assert!(Verifier::verify("checksum", "bad-format").is_err());
        assert!(Verifier::verify("checksum", "line1\nline2").is_err());
    }
}
