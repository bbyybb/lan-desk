**中文** | [English](#security-policy-english)

# 安全策略

## 支持的版本

| 版本 | 支持状态 |
|------|---------|
| v1.0 (最新) | ✅ 支持 |

建议始终使用 [最新版本](https://github.com/bbyybb/lan-desk/releases/latest)。

## 报告安全漏洞

如果你发现了安全漏洞，**请不要**通过公开的 Issue 提交。

请通过以下方式私密报告：

1. **GitHub 私密漏洞报告**（推荐）：前往 [Security Advisories](https://github.com/bbyybb/lan-desk/security/advisories/new) 提交
2. **邮件**：发送至 [lan-desk-security@bbyybb.dev](mailto:lan-desk-security@bbyybb.dev)
3. **其他**：通过 GitHub 个人资料页的联系方式，或在 [Discussions](https://github.com/bbyybb/lan-desk/discussions) 中私信联系

请在报告中包含：
- 漏洞的详细描述
- 复现步骤
- 潜在影响
- 如果有的话，建议的修复方案

我会在收到报告后尽快回复并处理。通常情况下：
- **48 小时内**确认收到报告
- **7 个工作日内**完成初步评估并回复处理方案

## 已知安全限制

- **PIN 码强度**：8 位数字 PIN（10000000~99999999，约 9000 万种组合），配合授权弹窗 30 秒超时和人工确认作为缓解措施
- **自签名 TLS**：不防中间人攻击，仅防被动嗅探，建议仅在可信局域网内使用
- **文件传输限制**：接收端限制单文件最大 2GB，防止磁盘填充攻击
- **UDP 发现无认证**：局域网设备发现协议未加密未认证，仅用于便捷发现，不应作为可信来源
- **无人值守模式风险**：开启固定密码 + 自动接受连接后，任何知道密码的人均可无需确认直接连接。强烈建议：(1) 使用高强度固定密码（≥12位，包含大小写字母、数字和特殊字符）；(2) 仅在可信局域网内启用；(3) 不再需要时及时关闭无人值守模式
- **远程终端（PTY Shell）**：控制角色可启动完整系统 Shell，获得服务端进程权限。远程终端默认禁用，需在设置中手动启用。建议在不需要时保持禁用，并使用强 PIN 码保护控制权限。

## 安全设计说明

本项目是一个**局域网远程桌面工具**，设计用于可信网络环境。安全措施包括：

- **TLS 1.3 加密**：所有 TCP 通信经 TLS 加密，防止局域网嗅探
- **PIN 码认证**：8 位密码学安全随机 PIN + Argon2id + 随机 salt 哈希传输（内存 19MB，迭代 2 次，防暴力破解）
- **授权确认**：被控端弹窗确认，30 秒超时自动拒绝
- **心跳检测**：15 秒无响应自动断开
- **PIN 暴力破解防护**：基于 IP 的指数退避锁定（5 次失败后锁定 5 分钟→15 分钟→45 分钟→…→最长 24 小时封顶），并设有全局速率限制（每分钟最多 10 次失败）
- **PIN 常量时间比较**：PIN 哈希验证使用常量时间比较（constant-time comparison），防止时序攻击
- **并发文件传输限制**：同时进行的文件传输数量限制为 5 个，并对传输偏移量范围进行校验，防止资源耗尽和越界访问
- **速率限制器过期清理**：速率限制器定期清理过期条目，防止内存耗尽攻击
- **TLS 证书持久化**：自签名证书保存在用户数据目录（`server_cert.der` / `server_key.der`），重启后复用以保持 TOFU 指纹一致。私钥文件使用 AES-256-GCM 认证加密保护（v3 格式），密钥通过 HKDF-SHA256 基于平台机器标识（Windows MachineGuid / Linux machine-id / macOS IOPlatformUUID）派生，防止明文泄露和跨机器拷贝。保持向后兼容，自动迁移旧格式（v2 SHA256-CTR / v1 XOR）

请注意：
- 自签名 TLS 证书不防中间人攻击，仅防被动嗅探
- 仅适用于局域网，**不建议**暴露到公网

---

<a id="security-policy-english"></a>

# Security Policy (English)

## Supported Versions

| Version | Status |
|---------|--------|
| v1.0 (latest) | ✅ Supported |

Always use the [latest version](https://github.com/bbyybb/lan-desk/releases/latest).

## Reporting a Vulnerability

If you discover a security vulnerability, **please do not** report it via public Issues.

Please report privately through:

1. **GitHub Private Vulnerability Reporting** (recommended): Go to [Security Advisories](https://github.com/bbyybb/lan-desk/security/advisories/new)
2. **Email**: Send to [lan-desk-security@bbyybb.dev](mailto:lan-desk-security@bbyybb.dev)
3. **Other**: Find contact info on the GitHub profile page, or reach out via [Discussions](https://github.com/bbyybb/lan-desk/discussions)

Please include:
- Detailed description of the vulnerability
- Steps to reproduce
- Potential impact
- Suggested fix, if any

Response times:
- **Within 48 hours**: Acknowledge receipt
- **Within 7 business days**: Initial assessment and response

## Known Security Limitations

- **PIN strength**: 8-digit numeric PIN (~90M combinations), mitigated by 30s authorization dialog timeout and manual confirmation
- **Self-signed TLS**: Does not prevent MITM, only passive sniffing — use only on trusted LANs
- **File transfer limit**: 2GB max file size on receiver side to prevent disk-fill attacks
- **UDP discovery unauthenticated**: LAN discovery protocol is unencrypted/unauthenticated, for convenience only
- **Unattended mode risk**: With fixed password + auto-accept enabled, anyone who knows the password can connect without confirmation. Strongly recommended: (1) use a strong fixed password (12+ characters with mixed case, digits, and symbols); (2) enable only on trusted LANs; (3) disable unattended mode when no longer needed
- **Remote Terminal (PTY Shell)**: The controlling user can launch a full system shell with the server process's privileges. The remote terminal is disabled by default and must be manually enabled in Settings. It is recommended to keep it disabled when not needed, and use a strong PIN to protect control access.

## Security Design Notes

This project is a **LAN remote desktop tool** designed for trusted network environments. Security measures include:

- **TLS 1.3 encryption**: All TCP traffic is TLS-encrypted
- **PIN authentication**: 8-digit cryptographic random PIN + Argon2id + random salt hashed transmission (19MB memory, 2 iterations, brute-force resistant)
- **Authorization dialog**: Host-side confirmation popup, 30s auto-deny
- **Heartbeat**: Auto-disconnect after 15s of no response
- **PIN brute-force protection**: IP-based exponential backoff lockout (5 failures → 5min → 15min → 45min → … → 24h max), with global rate limit (10 failures per minute)
- **Constant-time PIN hash comparison**: PIN hash verification uses constant-time comparison to prevent timing attacks
- **Concurrent file transfer limit (5)**: The number of simultaneous file transfers is limited to 5, with offset range validation to prevent resource exhaustion and out-of-bounds access
- **Rate limiter expired entry cleanup**: The rate limiter periodically cleans up expired entries to prevent memory exhaustion attacks
- **TLS certificate persistence**: Self-signed certificates saved in user data directory (`server_cert.der` / `server_key.der`), reused across restarts to maintain TOFU fingerprint consistency. The private key file is protected with AES-256-GCM authenticated encryption (v3 format), with keys derived via HKDF-SHA256 based on platform machine identity (Windows MachineGuid / Linux machine-id / macOS IOPlatformUUID), preventing plaintext leakage and cross-machine key copying. Backward compatible with automatic migration from legacy formats (v2 SHA256-CTR / v1 XOR)

Please note:
- Self-signed TLS certificates do not prevent MITM attacks, only passive sniffing
- Designed for LAN use only, **not recommended** for public internet exposure
