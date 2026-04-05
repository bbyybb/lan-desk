pub mod codec;
pub mod discovery;
pub mod integrity;
pub mod message;

use subtle::ConstantTimeEq;

pub const DEFAULT_TCP_PORT: u16 = 25605;
pub const DEFAULT_UDP_PORT: u16 = 25606;
pub const PROTOCOL_VERSION: u32 = 3;

/// 常量时间字节切片比较，防止时序侧信道攻击
pub fn ct_eq_bytes(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.ct_eq(b).into()
}

/// 常量时间字符串比较，防止时序侧信道攻击
pub fn ct_eq_str(a: &str, b: &str) -> bool {
    ct_eq_bytes(a.as_bytes(), b.as_bytes())
}

/// 生成 8 位随机 PIN（10000000~99999999），使用密码学安全随机数
pub fn generate_pin() -> String {
    use rand::Rng;
    let pin: u32 = rand::rngs::OsRng.gen_range(10_000_000..=99_999_999);
    format!("{}", pin)
}

/// 生成 16 字节随机 salt（hex 编码，用于 PIN 哈希）
pub fn generate_salt() -> String {
    use rand::Rng;
    let mut bytes = [0u8; 16];
    rand::rngs::OsRng.fill(&mut bytes);
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}

/// 生成一对不重复的 PIN（控制 PIN + 查看 PIN）
pub fn generate_pin_pair() -> (String, String) {
    let control = generate_pin();
    let mut view = generate_pin();
    while view == control {
        view = generate_pin();
    }
    (control, view)
}

/// 对 PIN 码做 Argon2id 哈希（用于网络传输，避免明文）
/// 参数：内存 19456 KiB (19 MiB)，迭代 2 次，并行度 1，输出 32 字节
/// salt 应每次会话随机生成，防止预计算攻击
pub fn hash_pin(pin: &str, salt: &str) -> String {
    use argon2::{Algorithm, Argon2, Params, Version};

    let params = Params::new(19_456, 2, 1, Some(32)).expect("Argon2 参数无效");
    let argon2 = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);

    // Argon2 要求 salt 至少 8 字节，如果不足则右填充零字节
    let salt_bytes = salt.as_bytes();
    let padded_salt: Vec<u8> = if salt_bytes.len() < 8 {
        let mut s = salt_bytes.to_vec();
        s.resize(8, 0);
        s
    } else {
        salt_bytes.to_vec()
    };

    let mut output = [0u8; 32];
    argon2
        .hash_password_into(pin.as_bytes(), &padded_salt, &mut output)
        .expect("Argon2 哈希失败");

    output.iter().map(|b| format!("{:02x}", b)).collect()
}

/// 安全处理文件名：防止路径遍历攻击和 Windows 保留设备名
///
/// - 仅保留最终文件名（去除路径分隔符）
/// - 检测并替换 Windows 保留设备名（CON/NUL/AUX/PRN/COM1-9/LPT1-9）
/// - 空字符串或纯 `.` 的文件名回退到 `fallback`
pub fn sanitize_filename(filename: &str, fallback: &str) -> String {
    // 先统一将反斜杠替换为正斜杠，确保跨平台一致解析路径分隔符
    // （Unix 上 `\` 是合法文件名字符，Path::file_name 不会拆分）
    let normalized = filename.replace('\\', "/");
    let safe = std::path::Path::new(&normalized)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| fallback.to_string());

    // 过滤 Windows 保留设备名（大小写不敏感）
    let stem = safe.split('.').next().unwrap_or("");
    let upper = stem.to_uppercase();
    let is_reserved = matches!(
        upper.as_str(),
        "CON"
            | "PRN"
            | "AUX"
            | "NUL"
            | "COM1"
            | "COM2"
            | "COM3"
            | "COM4"
            | "COM5"
            | "COM6"
            | "COM7"
            | "COM8"
            | "COM9"
            | "LPT1"
            | "LPT2"
            | "LPT3"
            | "LPT4"
            | "LPT5"
            | "LPT6"
            | "LPT7"
            | "LPT8"
            | "LPT9"
    );
    if is_reserved {
        return fallback.to_string();
    }
    if safe.is_empty() || safe.chars().all(|c| c == '.') {
        return fallback.to_string();
    }
    safe
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pin_format() {
        for _ in 0..100 {
            let pin = generate_pin();
            assert_eq!(pin.len(), 8, "PIN 长度应为 8: {}", pin);
            assert!(
                pin.chars().all(|c| c.is_ascii_digit()),
                "PIN 应全为数字: {}",
                pin
            );
            let num: u32 = pin.parse().unwrap();
            assert!(
                num >= 10_000_000 && num <= 99_999_999,
                "PIN 应在 10000000-99999999: {}",
                num
            );
        }
    }

    #[test]
    fn test_constants() {
        assert_eq!(DEFAULT_TCP_PORT, 25605);
        assert_eq!(DEFAULT_UDP_PORT, 25606);
        assert_eq!(PROTOCOL_VERSION, 3);
    }

    #[test]
    fn test_generate_pin_pair() {
        for _ in 0..50 {
            let (control, view) = generate_pin_pair();
            assert_ne!(control, view, "控制 PIN 和查看 PIN 不应相同");
            assert_eq!(control.len(), 8);
            assert_eq!(view.len(), 8);
        }
    }

    #[test]
    fn test_generate_salt() {
        let salt = generate_salt();
        assert_eq!(salt.len(), 32, "salt 应为 32 字符 hex: {}", salt);
        assert!(
            salt.chars().all(|c| c.is_ascii_hexdigit()),
            "salt 应为 hex: {}",
            salt
        );
        // 两次生成不应相同
        let salt2 = generate_salt();
        assert_ne!(salt, salt2, "两次 salt 不应相同");
    }

    #[test]
    fn test_hash_pin_with_salt() {
        let pin = "123456";
        let salt1 = "salt_a";
        let salt2 = "salt_b";
        let h1 = hash_pin(pin, salt1);
        let h2 = hash_pin(pin, salt2);
        // 不同 salt 应产生不同哈希
        assert_ne!(h1, h2, "不同 salt 应产生不同哈希");
        // 相同 salt 应产生相同哈希
        let h3 = hash_pin(pin, salt1);
        assert_eq!(h1, h3, "相同 salt 应产生相同哈希");
    }

    #[test]
    fn test_sanitize_filename_normal() {
        assert_eq!(sanitize_filename("test.txt", "fallback"), "test.txt");
        assert_eq!(sanitize_filename("my file.pdf", "fallback"), "my file.pdf");
    }

    #[test]
    fn test_sanitize_filename_strips_path_traversal() {
        assert_eq!(sanitize_filename("../etc/passwd", "fallback"), "passwd");
        assert_eq!(sanitize_filename("foo/bar.txt", "fallback"), "bar.txt");
        assert_eq!(sanitize_filename("foo\\bar.txt", "fallback"), "bar.txt");
    }

    #[test]
    fn test_sanitize_filename_fallback() {
        assert_eq!(sanitize_filename("", "file_0"), "file_0");
        assert_eq!(sanitize_filename("..", "file_1"), "file_1");
    }

    #[test]
    fn test_sanitize_filename_blocks_reserved_names() {
        assert_eq!(sanitize_filename("CON", "safe"), "safe");
        assert_eq!(sanitize_filename("nul.txt", "safe"), "safe");
        assert_eq!(sanitize_filename("com3.log", "safe"), "safe");
        assert_eq!(sanitize_filename("LPT1", "safe"), "safe");
        // 类似但不是保留名的应被允许
        assert_eq!(sanitize_filename("CONSOLE.txt", "safe"), "CONSOLE.txt");
        assert_eq!(sanitize_filename("COM10.dat", "safe"), "COM10.dat");
    }

    #[test]
    fn test_ct_eq_str_equal() {
        assert!(ct_eq_str("hello", "hello"));
        assert!(ct_eq_str("", ""));
        assert!(ct_eq_str("密码测试", "密码测试"));
    }

    #[test]
    fn test_ct_eq_str_not_equal() {
        assert!(!ct_eq_str("hello", "world"));
        assert!(!ct_eq_str("abc", "abd"));
    }

    #[test]
    fn test_ct_eq_str_different_length() {
        assert!(!ct_eq_str("short", "longer_string"));
        assert!(!ct_eq_str("abc", "ab"));
        assert!(!ct_eq_str("", "a"));
    }

    #[test]
    fn test_ct_eq_bytes_equal() {
        assert!(ct_eq_bytes(b"hello", b"hello"));
        assert!(ct_eq_bytes(&[0x00, 0xff, 0x42], &[0x00, 0xff, 0x42]));
    }

    #[test]
    fn test_ct_eq_bytes_not_equal() {
        assert!(!ct_eq_bytes(b"hello", b"world"));
        assert!(!ct_eq_bytes(&[0x00], &[0x01]));
    }

    #[test]
    fn test_ct_eq_bytes_empty() {
        assert!(ct_eq_bytes(b"", b""));
        assert!(ct_eq_bytes(&[], &[]));
    }
}
