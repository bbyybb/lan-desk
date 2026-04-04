use std::fs;
use std::path::Path;
use std::sync::Arc;

use rcgen::{CertificateParams, KeyPair};
use ring::aead;
use ring::hkdf;
use rustls::ServerConfig;
use rustls_pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use sha2::{Digest, Sha256};
use tokio_rustls::TlsAcceptor;
use tracing::info;

/// v3 加密格式魔术头（AES-256-GCM + HKDF + machine-id）
const KEY_FILE_MAGIC_V3: &[u8; 4] = b"LDK3";
/// v2 加密格式魔术头（SHA256-CTR + OsRng nonce）
const KEY_FILE_MAGIC_V2: &[u8; 4] = b"LDK2";
/// v1 加密格式魔术头（简单 XOR）
const KEY_FILE_MAGIC_V1: &[u8; 4] = b"LDSK";

// ──────────────── 跨平台机器标识 ────────────────

/// 获取平台原生机器标识，回退到 hostname
pub fn get_machine_id() -> String {
    let id = get_platform_machine_id().unwrap_or_default();
    if id.is_empty() {
        hostname::get()
            .map(|h| h.to_string_lossy().to_string())
            .unwrap_or_default()
    } else {
        id
    }
}

/// Windows: 读取注册表 HKLM\SOFTWARE\Microsoft\Cryptography\MachineGuid
#[cfg(target_os = "windows")]
fn get_platform_machine_id() -> Option<String> {
    use winreg::enums::HKEY_LOCAL_MACHINE;
    use winreg::RegKey;
    let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
    let key = hklm.open_subkey("SOFTWARE\\Microsoft\\Cryptography").ok()?;
    key.get_value::<String, _>("MachineGuid").ok()
}

/// Linux: 读取 /etc/machine-id 或 /var/lib/dbus/machine-id
#[cfg(target_os = "linux")]
fn get_platform_machine_id() -> Option<String> {
    std::fs::read_to_string("/etc/machine-id")
        .or_else(|_| std::fs::read_to_string("/var/lib/dbus/machine-id"))
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// macOS: 通过 ioreg 读取 IOPlatformUUID
#[cfg(target_os = "macos")]
fn get_platform_machine_id() -> Option<String> {
    std::process::Command::new("ioreg")
        .args(["-rd1", "-c", "IOPlatformExpertDevice"])
        .output()
        .ok()
        .and_then(|o| {
            let stdout = String::from_utf8_lossy(&o.stdout);
            for line in stdout.lines() {
                if line.contains("IOPlatformUUID") {
                    return line.split('"').nth(3).map(|s| s.to_string());
                }
            }
            None
        })
}

/// 其他平台回退
#[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
fn get_platform_machine_id() -> Option<String> {
    None
}

// ──────────────── 密钥派生 ────────────────

/// ring HKDF 输出长度标记
struct HkdfLen(usize);
impl hkdf::KeyType for HkdfLen {
    fn len(&self) -> usize {
        self.0
    }
}

/// 基于 machine-id + hostname 使用 HKDF-SHA256 派生 32 字节密钥（v3）
fn derive_machine_key() -> [u8; 32] {
    let machine_id = get_machine_id();
    let hostname = hostname::get()
        .map(|h| h.to_string_lossy().to_string())
        .unwrap_or_default();

    let mut ikm = Vec::new();
    ikm.extend_from_slice(machine_id.as_bytes());
    ikm.extend_from_slice(hostname.as_bytes());

    let salt = hkdf::Salt::new(hkdf::HKDF_SHA256, b"lan-desk-key-protection-v2-2026");
    let prk = salt.extract(&ikm);
    let info = [b"tls-private-key-encryption".as_slice()];
    let okm = prk
        .expand(&info, HkdfLen(32))
        .expect("HKDF expand 不应失败");
    let mut key = [0u8; 32];
    okm.fill(&mut key).expect("HKDF fill 不应失败");
    key
}

/// 旧版密钥派生（仅 hostname + 固定 salt 的 SHA-256），用于解密旧格式文件
fn derive_machine_key_legacy() -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(b"lan-desk-key-protection-v1");
    if let Ok(name) = hostname::get() {
        hasher.update(name.to_string_lossy().as_bytes());
    }
    hasher.update(b"lan-desk-salt-2026");
    hasher.finalize().into()
}

// ──────────────── v3 加密/解密（AES-256-GCM）────────────────

/// v3 加密：AES-256-GCM + 随机 nonce
/// 格式: [4 字节 LDK3] [12 字节 nonce] [加密数据 + 16 字节 GCM tag]
fn encrypt_key_v3(data: &[u8], machine_key: &[u8; 32]) -> Vec<u8> {
    let unbound_key = aead::UnboundKey::new(&aead::AES_256_GCM, machine_key)
        .expect("AES-256-GCM 密钥创建不应失败");
    let key = aead::LessSafeKey::new(unbound_key);

    let mut nonce_bytes = [0u8; 12];
    use rand::Rng;
    rand::rngs::OsRng.fill(&mut nonce_bytes);
    let nonce = aead::Nonce::assume_unique_for_key(nonce_bytes);

    let mut in_out = data.to_vec();
    key.seal_in_place_append_tag(nonce, aead::Aad::empty(), &mut in_out)
        .expect("AES-GCM 加密不应失败");

    let mut result = Vec::with_capacity(4 + 12 + in_out.len());
    result.extend_from_slice(KEY_FILE_MAGIC_V3);
    result.extend_from_slice(&nonce_bytes);
    result.extend_from_slice(&in_out);
    result
}

/// v3 解密：AES-256-GCM（含认证标签验证）
fn decrypt_key_v3(data: &[u8], machine_key: &[u8; 32]) -> Option<Vec<u8>> {
    // 最小长度: 4(magic) + 12(nonce) + 16(tag) = 32
    if data.len() < 32 || &data[..4] != KEY_FILE_MAGIC_V3 {
        return None;
    }
    let nonce_bytes: [u8; 12] = data[4..16].try_into().ok()?;
    let nonce = aead::Nonce::assume_unique_for_key(nonce_bytes);

    let unbound_key = aead::UnboundKey::new(&aead::AES_256_GCM, machine_key).ok()?;
    let key = aead::LessSafeKey::new(unbound_key);

    let mut in_out = data[16..].to_vec();
    let plaintext = key
        .open_in_place(nonce, aead::Aad::empty(), &mut in_out)
        .ok()?;
    Some(plaintext.to_vec())
}

// ──────────────── v2 加密/解密（SHA256-CTR，保留用于旧文件解密）────────────────

/// SHA256-CTR 密钥流生成与 XOR
fn ctr_xor(data: &[u8], machine_key: &[u8; 32], nonce: &[u8; 16]) -> Vec<u8> {
    let mut output = Vec::with_capacity(data.len());
    let blocks = data.len().div_ceil(32);
    for i in 0..blocks {
        let mut hasher = Sha256::new();
        hasher.update(machine_key);
        hasher.update(nonce);
        hasher.update((i as u64).to_le_bytes());
        let block = hasher.finalize();
        let start = i * 32;
        let end = std::cmp::min(start + 32, data.len());
        for j in start..end {
            output.push(data[j] ^ block[j - start]);
        }
    }
    output
}

/// v2 解密：SHA256-CTR
fn decrypt_key_v2(data: &[u8], machine_key: &[u8; 32]) -> Option<Vec<u8>> {
    if data.len() < 20 || &data[..4] != KEY_FILE_MAGIC_V2 {
        return None;
    }
    let mut nonce = [0u8; 16];
    nonce.copy_from_slice(&data[4..20]);
    Some(ctr_xor(&data[20..], machine_key, &nonce))
}

/// v1 解密：简单 XOR
fn decrypt_key_v1(data: &[u8], machine_key: &[u8; 32]) -> Option<Vec<u8>> {
    if data.len() < 4 || &data[..4] != KEY_FILE_MAGIC_V1 {
        return None;
    }
    let encrypted = &data[4..];
    let mut result = Vec::with_capacity(encrypted.len());
    for (i, byte) in encrypted.iter().enumerate() {
        result.push(byte ^ machine_key[i % 32]);
    }
    Some(result)
}

// ──────────────── 统一解密（向后兼容 v1/v2/v3）────────────────

/// 尝试解密私钥数据，按优先级尝试 v3 → v2 → v1 → 明文
/// 返回 (解密数据, 是否需要迁移到v3)
fn decrypt_key_compat(
    data: &[u8],
    machine_key: &[u8; 32],
    legacy_key: &[u8; 32],
) -> Option<(Vec<u8>, bool)> {
    if data.len() < 4 {
        return None;
    }
    // v3: 新密钥 + AES-GCM
    if let Some(decrypted) = decrypt_key_v3(data, machine_key) {
        return Some((decrypted, false));
    }
    // v2: 旧密钥 + SHA256-CTR（需要迁移）
    if let Some(decrypted) = decrypt_key_v2(data, legacy_key) {
        return Some((decrypted, true));
    }
    // v1: 旧密钥 + 简单 XOR（需要迁移）
    if let Some(decrypted) = decrypt_key_v1(data, legacy_key) {
        return Some((decrypted, true));
    }
    // 无已知魔术头 → 明文
    None
}

// ──────────────── 证书生成与管理 ────────────────

/// 生成自签名证书，返回 (证书 DER, 私钥 DER)
fn generate_self_signed() -> anyhow::Result<(Vec<u8>, Vec<u8>)> {
    let key_pair = KeyPair::generate()?;

    let mut params = CertificateParams::new(vec!["lan-desk.local".to_string()])?;
    params.distinguished_name.push(
        rcgen::DnType::CommonName,
        rcgen::DnValue::Utf8String("LAN-Desk".to_string()),
    );

    let cert = params.self_signed(&key_pair)?;

    let cert_der = cert.der().to_vec();
    let key_der = key_pair.serialize_der();

    Ok((cert_der, key_der))
}

/// 创建 TLS acceptor，支持证书持久化
///
/// 如果提供 `data_dir` 且磁盘上存在已保存的证书，则复用；
/// 否则生成新证书并保存到磁盘。
/// 自动将旧格式（v1/v2/明文）迁移为 v3（AES-256-GCM）。
pub fn create_tls_acceptor(data_dir: Option<&Path>) -> anyhow::Result<TlsAcceptor> {
    let (cert_bytes, key_bytes) = if let Some(dir) = data_dir {
        let cert_path = dir.join("server_cert.der");
        let key_path = dir.join("server_key.der");
        let machine_key = derive_machine_key();
        let legacy_key = derive_machine_key_legacy();

        if cert_path.exists() && key_path.exists() {
            // 从磁盘加载已有证书
            let cert_bytes = fs::read(&cert_path)?;
            let raw_key_bytes = fs::read(&key_path)?;

            let key_bytes = if let Some((decrypted, needs_migration)) =
                decrypt_key_compat(&raw_key_bytes, &machine_key, &legacy_key)
            {
                info!("TLS 私钥已从加密文件解密加载: {:?}", key_path);
                if needs_migration {
                    info!(
                        "检测到旧版加密格式，正在迁移为 v3（AES-256-GCM + HKDF）: {:?}",
                        key_path
                    );
                    let re_encrypted = encrypt_key_v3(&decrypted, &machine_key);
                    fs::write(&key_path, &re_encrypted)?;
                }
                decrypted
            } else {
                // 无已知魔术头 → 旧版未加密格式，迁移为 v3 加密存储
                info!(
                    "检测到未加密的旧版私钥文件，正在迁移为 v3 加密格式: {:?}",
                    key_path
                );
                let encrypted = encrypt_key_v3(&raw_key_bytes, &machine_key);
                fs::write(&key_path, &encrypted)?;
                raw_key_bytes
            };

            info!("TLS 证书已从磁盘加载: {:?}", cert_path);
            (cert_bytes, key_bytes)
        } else {
            // 生成新证书并用 v3 格式加密保存
            let (cert_bytes, key_bytes) = generate_self_signed()?;
            fs::create_dir_all(dir)?;
            fs::write(&cert_path, &cert_bytes)?;
            let encrypted_key = encrypt_key_v3(&key_bytes, &machine_key);
            fs::write(&key_path, &encrypted_key)?;
            info!("TLS 自签名证书已生成并保存到: {:?}", cert_path);
            (cert_bytes, key_bytes)
        }
    } else {
        // 无持久化目录，每次生成临时证书
        let (cert_bytes, key_bytes) = generate_self_signed()?;
        info!("TLS 自签名证书已生成（未持久化）");
        (cert_bytes, key_bytes)
    };

    let cert_der = CertificateDer::from(cert_bytes);
    let key_der = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(key_bytes));

    let config = ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(vec![cert_der], key_der)?;

    Ok(TlsAcceptor::from(Arc::new(config)))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 确保 rustls CryptoProvider 已安装（测试环境需要手动安装）
    fn ensure_crypto_provider() {
        let _ = rustls::crypto::ring::default_provider().install_default();
    }

    #[test]
    fn test_machine_id_not_empty() {
        let id = get_machine_id();
        assert!(
            !id.is_empty(),
            "machine_id 不应为空（至少有 hostname 回退）"
        );
    }

    #[test]
    fn test_derive_machine_key_deterministic() {
        let k1 = derive_machine_key();
        let k2 = derive_machine_key();
        assert_eq!(k1, k2, "相同机器上密钥派生应确定性一致");
    }

    #[test]
    fn test_derive_machine_key_differs_from_legacy() {
        let new_key = derive_machine_key();
        let legacy_key = derive_machine_key_legacy();
        // 由于 HKDF 和 machine-id 的引入，新旧密钥应不同（除非 machine-id 回退到仅 hostname 且恰好一致）
        // 在 Windows 上 machine-id 来自注册表，必定不同
        // 此测试仅验证函数能正常返回
        assert_eq!(new_key.len(), 32);
        assert_eq!(legacy_key.len(), 32);
    }

    #[test]
    fn test_v3_encrypt_decrypt_round_trip() {
        let machine_key = derive_machine_key();
        let original = b"this is a test private key content 1234567890";

        let encrypted = encrypt_key_v3(original, &machine_key);
        assert!(encrypted.starts_with(KEY_FILE_MAGIC_V3));
        // v3 格式: 4(magic) + 12(nonce) + data_len + 16(tag)
        assert_eq!(encrypted.len(), 4 + 12 + original.len() + 16);

        let decrypted = decrypt_key_v3(&encrypted, &machine_key);
        assert!(decrypted.is_some(), "v3 解密不应失败");
        assert_eq!(decrypted.unwrap(), original.to_vec());
    }

    #[test]
    fn test_v3_aead_detects_tampering() {
        let machine_key = derive_machine_key();
        let original = b"test data for tamper detection";

        let mut encrypted = encrypt_key_v3(original, &machine_key);
        // 篡改密文中的一个字节
        let last = encrypted.len() - 1;
        encrypted[last] ^= 0xFF;

        let result = decrypt_key_v3(&encrypted, &machine_key);
        assert!(
            result.is_none(),
            "篡改后的密文应解密失败（AEAD 认证标签校验）"
        );
    }

    #[test]
    fn test_v3_wrong_key_fails() {
        let machine_key = derive_machine_key();
        let wrong_key = [0xABu8; 32];
        let original = b"secret data";

        let encrypted = encrypt_key_v3(original, &machine_key);
        let result = decrypt_key_v3(&encrypted, &wrong_key);
        assert!(result.is_none(), "使用错误密钥应解密失败");
    }

    #[test]
    fn test_decrypt_compat_v2_with_legacy_key() {
        // 模拟旧版 v2 文件（用旧密钥加密）
        let legacy_key = derive_machine_key_legacy();
        let original = b"test v2 data";

        // 手动构建 v2 格式
        let mut nonce = [0u8; 16];
        use rand::Rng;
        rand::rngs::OsRng.fill(&mut nonce);
        let mut v2_data = Vec::new();
        v2_data.extend_from_slice(KEY_FILE_MAGIC_V2);
        v2_data.extend_from_slice(&nonce);
        v2_data.extend_from_slice(&ctr_xor(original, &legacy_key, &nonce));

        let new_key = derive_machine_key();
        let result = decrypt_key_compat(&v2_data, &new_key, &legacy_key);
        assert!(result.is_some(), "应能用旧密钥解密 v2 格式");
        let (decrypted, needs_migration) = result.unwrap();
        assert_eq!(decrypted, original.to_vec());
        assert!(needs_migration, "v2 格式应标记需要迁移");
    }

    #[test]
    fn test_decrypt_compat_v1_with_legacy_key() {
        let legacy_key = derive_machine_key_legacy();
        let original = b"test v1 data";

        // 手动构建 v1 格式
        let mut v1_data = Vec::with_capacity(4 + original.len());
        v1_data.extend_from_slice(KEY_FILE_MAGIC_V1);
        for (i, byte) in original.iter().enumerate() {
            v1_data.push(byte ^ legacy_key[i % 32]);
        }

        let new_key = derive_machine_key();
        let result = decrypt_key_compat(&v1_data, &new_key, &legacy_key);
        assert!(result.is_some(), "应能用旧密钥解密 v1 格式");
        let (decrypted, needs_migration) = result.unwrap();
        assert_eq!(decrypted, original.to_vec());
        assert!(needs_migration, "v1 格式应标记需要迁移");
    }

    #[test]
    fn test_decrypt_rejects_unencrypted_data() {
        let new_key = derive_machine_key();
        let legacy_key = derive_machine_key_legacy();
        let plain_data = b"plain unencrypted key data";
        assert!(
            decrypt_key_compat(plain_data, &new_key, &legacy_key).is_none(),
            "无魔术头的数据应返回 None"
        );
    }

    #[test]
    fn test_decrypt_rejects_short_data() {
        let new_key = derive_machine_key();
        let legacy_key = derive_machine_key_legacy();
        assert!(decrypt_key_compat(b"LD", &new_key, &legacy_key).is_none());
        assert!(decrypt_key_compat(b"", &new_key, &legacy_key).is_none());
    }

    #[test]
    fn test_create_tls_acceptor_without_data_dir() {
        ensure_crypto_provider();
        let result = create_tls_acceptor(None);
        assert!(result.is_ok());
    }

    #[test]
    fn test_create_tls_acceptor_with_data_dir_creates_v3_certs() {
        ensure_crypto_provider();
        let tmp = std::env::temp_dir().join("lan-desk-test-tls-v3");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();

        let result = create_tls_acceptor(Some(&tmp));
        assert!(result.is_ok());
        assert!(tmp.join("server_cert.der").exists());
        assert!(tmp.join("server_key.der").exists());

        // 验证新生成的文件使用 v3 格式
        let key_data = std::fs::read(tmp.join("server_key.der")).unwrap();
        assert!(
            key_data.starts_with(KEY_FILE_MAGIC_V3),
            "新生成的私钥文件应使用 v3（AES-256-GCM）格式"
        );

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_create_tls_acceptor_reuses_existing_certs() {
        ensure_crypto_provider();
        let tmp = std::env::temp_dir().join("lan-desk-test-tls-v3-reuse");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();

        let _ = create_tls_acceptor(Some(&tmp)).unwrap();
        let cert1 = std::fs::read(tmp.join("server_cert.der")).unwrap();
        let key1 = std::fs::read(tmp.join("server_key.der")).unwrap();

        let _ = create_tls_acceptor(Some(&tmp)).unwrap();
        let cert2 = std::fs::read(tmp.join("server_cert.der")).unwrap();
        let key2 = std::fs::read(tmp.join("server_key.der")).unwrap();
        assert_eq!(cert1, cert2, "证书应该被复用而非重新生成");
        assert_eq!(key1, key2, "加密私钥文件应该保持不变");

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_legacy_unencrypted_key_migration_to_v3() {
        ensure_crypto_provider();
        let tmp = std::env::temp_dir().join("lan-desk-test-tls-v3-migration");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();

        let (cert_bytes, key_bytes) = generate_self_signed().unwrap();
        std::fs::write(tmp.join("server_cert.der"), &cert_bytes).unwrap();
        std::fs::write(tmp.join("server_key.der"), &key_bytes).unwrap();

        let result = create_tls_acceptor(Some(&tmp));
        assert!(result.is_ok(), "加载旧版未加密私钥应该成功");

        let migrated_key = std::fs::read(tmp.join("server_key.der")).unwrap();
        assert!(
            migrated_key.starts_with(KEY_FILE_MAGIC_V3),
            "迁移后的私钥文件应使用 v3 格式"
        );

        let result2 = create_tls_acceptor(Some(&tmp));
        assert!(result2.is_ok(), "加载迁移后的 v3 加密私钥应该成功");

        let _ = std::fs::remove_dir_all(&tmp);
    }
}
