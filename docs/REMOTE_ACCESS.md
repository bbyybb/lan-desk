**中文** | [English](#remote-access-english)

# 跨网络远程访问指南

LAN-Desk 默认在局域网内使用。借助**内网穿透工具**，可以实现跨网络（公网）远程控制。

## 方案对比

| 方案 | 难度 | 速度 | 费用 | 推荐场景 |
|------|------|------|------|----------|
| **Tailscale** | ⭐ 最简单 | 快（P2P 打洞） | 免费（个人） | 个人跨网络使用 |
| **ZeroTier** | ⭐ 最简单 | 快（P2P 打洞） | 免费（25 设备） | 小团队 |
| **Cloudflare Tunnel** | ⭐⭐ 简单 | 快（全球 CDN） | 免费 | 稳定长期穿透 |
| **frp** | ⭐⭐⭐ 需自建 | 取决于服务器 | 需要一台公网服务器 | 企业/高级用户 |
| **ngrok** | ⭐⭐ 简单 | 中等 | 免费版有限制 | 临时使用 |
| **WireGuard VPN** | ⭐⭐⭐ 需配置 | 最快 | 需要公网服务器 | 企业 VPN |

## 方案一：Tailscale（推荐，最简单）

Tailscale 会在两台设备间创建虚拟局域网，无需公网服务器。

### 步骤

1. **两台电脑都安装 Tailscale**
   - 下载：https://tailscale.com/download
   - 安装后登录同一账号

2. **获取 Tailscale IP**
   - 被控端打开 Tailscale 客户端
   - 记下分配的 IP（如 `100.64.x.x`）

3. **LAN-Desk 连接**
   - 控制端 LAN-Desk 输入被控端的 Tailscale IP（如 `100.64.1.2`）
   - 输入被控端显示的 PIN
   - 点击连接

```
控制端 (家里) ←→ Tailscale 虚拟网络 ←→ 被控端 (公司)
      LAN-Desk                              LAN-Desk
   输入 100.64.1.2                      显示 PIN: 790700
```

> **优点**：零配置、P2P 直连（不经过中转服务器）、端到端加密
> **缺点**：需要两端都安装 Tailscale

## 方案二：ZeroTier

与 Tailscale 类似的虚拟局域网方案。

### 步骤

1. **注册 ZeroTier 网络**：https://my.zerotier.com/
2. **两台电脑安装 ZeroTier**，加入同一网络 ID
3. **获取 ZeroTier IP**（如 `10.147.x.x`）
4. **LAN-Desk 连接**：输入 ZeroTier IP + PIN

## 方案三：Cloudflare Tunnel（免费，稳定）

Cloudflare Tunnel 利用 Cloudflare 全球网络转发 TCP 流量，无需自建服务器，免费无限制。

### 步骤

1. **注册 Cloudflare 账号**：https://dash.cloudflare.com/sign-up

2. **被控端安装 cloudflared**
   - 下载：https://developers.cloudflare.com/cloudflare-one/connections/connect-networks/downloads/
   - 或 Windows: `winget install Cloudflare.cloudflared`
   - 或 macOS: `brew install cloudflared`

3. **登录并创建隧道**
   ```bash
   cloudflared tunnel login
   cloudflared tunnel create lan-desk
   ```

4. **配置隧道转发 LAN-Desk 端口**

   创建 `~/.cloudflared/config.yml`：
   ```yaml
   tunnel: lan-desk
   credentials-file: ~/.cloudflared/<TUNNEL_ID>.json

   ingress:
     - hostname: lan-desk.your-domain.com
       service: tcp://localhost:25605
     - service: http_status:404
   ```

5. **启动隧道（被控端）**
   ```bash
   cloudflared tunnel run lan-desk
   ```

6. **控制端连接**

   控制端也需要安装 cloudflared，通过隧道访问：
   ```bash
   cloudflared access tcp --hostname lan-desk.your-domain.com --url localhost:25605
   ```
   然后在 LAN-Desk 中输入 `127.0.0.1` + PIN 连接。

```
控制端 ←→ cloudflared ←→ Cloudflare 网络 ←→ cloudflared ←→ 被控端
                         (全球 CDN 加速)
```

> **优点**：免费无限制、Cloudflare 全球网络低延迟、无需公网服务器、支持自定义域名
> **缺点**：需要 Cloudflare 账号和域名、两端都需安装 cloudflared

### 简化方案（Quick Tunnel，无需域名）

如果没有域名，可以用临时隧道：

```bash
# 被控端（一行命令，自动分配临时域名）
cloudflared tunnel --url tcp://localhost:25605
```

输出类似 `https://xxx-yyy-zzz.trycloudflare.com`，控制端用这个地址连接。

> **注意**：Quick Tunnel 每次重启域名会变化，适合临时使用。

## 方案四：frp 内网穿透（需自建服务器）

适合有公网服务器的用户，可以穿透任何 NAT。

### 服务器端 (frps)

```ini
# frps.toml
bindPort = 7000
```

```bash
frps -c frps.toml
```

### 被控端 (frpc)

```ini
# frpc.toml
serverAddr = "你的公网服务器IP"
serverPort = 7000

[[proxies]]
name = "lan-desk-tcp"
type = "tcp"
localIP = "127.0.0.1"
localPort = 25605
remotePort = 25605

[[proxies]]
name = "lan-desk-udp"
type = "udp"
localIP = "127.0.0.1"
localPort = 25606
remotePort = 25606
```

```bash
frpc -c frpc.toml
```

### 控制端

LAN-Desk 中输入 `公网服务器IP`（不需要指定端口，默认 25605）。

```
控制端 ──→ 公网服务器:25605 ──frp──→ 被控端:25605
```

## 方案五：ngrok（临时使用）

```bash
# 被控端运行
ngrok tcp 25605
```

ngrok 会分配一个公网地址（如 `0.tcp.ngrok.io:12345`），控制端输入此地址连接。

> **注意**：ngrok 免费版地址每次重启会变化

## 方案六：WireGuard VPN（企业级，最快）

WireGuard 是一种现代、高性能的 VPN 协议，代码精简、加密强度高、连接速度极快。通过 WireGuard 在两台设备之间建立加密隧道后，即可像在局域网一样使用 LAN-Desk。

> **前提条件**：需要一台拥有公网 IP 的服务器作为 VPN 中转节点。

### 服务器端配置

1. **安装 WireGuard**
   ```bash
   # Ubuntu/Debian
   sudo apt install wireguard

   # CentOS/RHEL
   sudo yum install wireguard-tools
   ```

2. **生成密钥对**
   ```bash
   wg genkey | tee server_private.key | wg pubkey > server_public.key
   ```

3. **创建服务器配置** `/etc/wireguard/wg0.conf`：
   ```ini
   [Interface]
   PrivateKey = <服务器私钥>
   Address = 10.10.0.1/24
   ListenPort = 51820
   # 开启 IP 转发（使客户端之间可以互通）
   PostUp = iptables -A FORWARD -i wg0 -j ACCEPT; iptables -t nat -A POSTROUTING -o eth0 -j MASQUERADE
   PostDown = iptables -D FORWARD -i wg0 -j ACCEPT; iptables -t nat -D POSTROUTING -o eth0 -j MASQUERADE

   # 被控端
   [Peer]
   PublicKey = <被控端公钥>
   AllowedIPs = 10.10.0.2/32

   # 控制端
   [Peer]
   PublicKey = <控制端公钥>
   AllowedIPs = 10.10.0.3/32
   ```

4. **启动 WireGuard**
   ```bash
   sudo wg-quick up wg0
   # 设置开机自启
   sudo systemctl enable wg-quick@wg0
   ```

### 被控端配置

1. **安装 WireGuard**
   - Windows: https://www.wireguard.com/install/
   - macOS: `brew install wireguard-tools` 或从 App Store 安装
   - Linux: `sudo apt install wireguard`

2. **生成密钥对**
   ```bash
   wg genkey | tee client_host_private.key | wg pubkey > client_host_public.key
   ```

3. **创建配置** `wg0.conf`：
   ```ini
   [Interface]
   PrivateKey = <被控端私钥>
   Address = 10.10.0.2/24
   DNS = 8.8.8.8

   [Peer]
   PublicKey = <服务器公钥>
   Endpoint = 你的公网服务器IP:51820
   AllowedIPs = 10.10.0.0/24
   PersistentKeepalive = 25
   ```

### 控制端配置

1. **安装 WireGuard**（同上）

2. **生成密钥对**
   ```bash
   wg genkey | tee client_ctrl_private.key | wg pubkey > client_ctrl_public.key
   ```

3. **创建配置** `wg0.conf`：
   ```ini
   [Interface]
   PrivateKey = <控制端私钥>
   Address = 10.10.0.3/24
   DNS = 8.8.8.8

   [Peer]
   PublicKey = <服务器公钥>
   Endpoint = 你的公网服务器IP:51820
   AllowedIPs = 10.10.0.0/24
   PersistentKeepalive = 25
   ```

### 通过 WireGuard 隧道连接 LAN-Desk

1. 服务器、被控端、控制端分别启动 WireGuard
2. 被控端启动 LAN-Desk，记下 PIN
3. 控制端在 LAN-Desk 中输入被控端的 WireGuard IP `10.10.0.2` + PIN
4. 点击连接

```
控制端 (10.10.0.3) ←→ WireGuard 服务器 (10.10.0.1) ←→ 被控端 (10.10.0.2)
      LAN-Desk            公网服务器:51820              LAN-Desk
   输入 10.10.0.2                                   显示 PIN: 790700
```

> **优点**：速度极快（内核级实现）、加密强度高（ChaCha20）、配置简洁、资源占用极低
> **缺点**：需要公网服务器、需要手动管理密钥和配置

## 安全提醒

跨公网使用时请注意：

| 安全措施 | LAN-Desk 内建 | 额外建议 |
|----------|---------------|----------|
| 传输加密 | ✅ TLS 1.3 | 穿透工具自身也加密更好 |
| 身份认证 | ✅ PIN 码 + Argon2id | 定期更换 PIN |
| 授权确认 | ✅ 弹窗确认 | 不用时关闭 LAN-Desk |
| 中间人防护 | ⚠️ 自签名证书 | Tailscale/WireGuard 提供额外加密层 |

**最佳实践**：
- 优先使用 Tailscale/ZeroTier（有额外加密层）
- 避免将 LAN-Desk 端口直接暴露到公网（无防火墙保护）
- 不使用时关闭 LAN-Desk 或断开穿透

---

<a id="remote-access-english"></a>

# Remote Access Guide (English)

LAN-Desk works on LAN by default. With **NAT traversal tools**, it can work across any network.

## Comparison

| Solution | Difficulty | Speed | Cost | Best For |
|----------|-----------|-------|------|----------|
| **Tailscale** | ⭐ Easiest | Fast (P2P) | Free (personal) | Personal use |
| **ZeroTier** | ⭐ Easiest | Fast (P2P) | Free (25 devices) | Small teams |
| **Cloudflare Tunnel** | ⭐⭐ Easy | Fast (global CDN) | Free | Stable long-term |
| **frp** | ⭐⭐⭐ Self-hosted | Depends on server | Needs a VPS | Enterprise |
| **ngrok** | ⭐⭐ Easy | Medium | Free tier limited | Temporary use |
| **WireGuard** | ⭐⭐⭐ Config needed | Fastest | Needs a VPS | Enterprise VPN |

## Option 1: Tailscale (Recommended)

1. Install Tailscale on both computers: https://tailscale.com/download
2. Log in with the same account
3. Note the host's Tailscale IP (e.g., `100.64.1.2`)
4. In LAN-Desk, enter the Tailscale IP + PIN → Connect

## Option 2: ZeroTier

1. Create a network at https://my.zerotier.com/
2. Install ZeroTier on both PCs, join the same network
3. Connect using ZeroTier IP + PIN

## Option 3: Cloudflare Tunnel (Free, Stable)

Uses Cloudflare's global network to relay TCP traffic. Free, no VPS needed.

1. Install cloudflared: https://developers.cloudflare.com/cloudflare-one/connections/connect-networks/downloads/
2. Create tunnel:
   ```bash
   cloudflared tunnel login
   cloudflared tunnel create lan-desk
   ```
3. Configure `~/.cloudflared/config.yml`:
   ```yaml
   tunnel: lan-desk
   credentials-file: ~/.cloudflared/<TUNNEL_ID>.json
   ingress:
     - hostname: lan-desk.your-domain.com
       service: tcp://localhost:25605
     - service: http_status:404
   ```
4. Host runs: `cloudflared tunnel run lan-desk`
5. Controller runs: `cloudflared access tcp --hostname lan-desk.your-domain.com --url localhost:25605`
6. Connect in LAN-Desk using `127.0.0.1` + PIN

**Quick Tunnel (no domain needed):**
```bash
cloudflared tunnel --url tcp://localhost:25605
```

## Option 4: frp (Self-hosted)

Server (`frps.toml`):
```ini
bindPort = 7000
```

Host client (`frpc.toml`):
```ini
serverAddr = "your-server-ip"
serverPort = 7000

[[proxies]]
name = "lan-desk-tcp"
type = "tcp"
localIP = "127.0.0.1"
localPort = 25605
remotePort = 25605

[[proxies]]
name = "lan-desk-udp"
type = "udp"
localIP = "127.0.0.1"
localPort = 25606
remotePort = 25606
```

Controller: Enter `your-server-ip` in LAN-Desk.

## Option 5: ngrok (Temporary)

```bash
# On host machine
ngrok tcp 25605
```

Enter the assigned address (e.g., `0.tcp.ngrok.io:12345`) in LAN-Desk.

## Option 6: WireGuard VPN (Enterprise, Fastest)

WireGuard is a modern, high-performance VPN protocol with minimal code, strong encryption, and extremely fast connections. Once a WireGuard tunnel is established between two devices, LAN-Desk works as if they were on the same LAN.

> **Prerequisite**: Requires a server with a public IP to act as the VPN relay node.

### Server Setup

1. **Install WireGuard**
   ```bash
   # Ubuntu/Debian
   sudo apt install wireguard

   # CentOS/RHEL
   sudo yum install wireguard-tools
   ```

2. **Generate key pair**
   ```bash
   wg genkey | tee server_private.key | wg pubkey > server_public.key
   ```

3. **Create server config** `/etc/wireguard/wg0.conf`:
   ```ini
   [Interface]
   PrivateKey = <server-private-key>
   Address = 10.10.0.1/24
   ListenPort = 51820
   # Enable IP forwarding (allows clients to communicate with each other)
   PostUp = iptables -A FORWARD -i wg0 -j ACCEPT; iptables -t nat -A POSTROUTING -o eth0 -j MASQUERADE
   PostDown = iptables -D FORWARD -i wg0 -j ACCEPT; iptables -t nat -D POSTROUTING -o eth0 -j MASQUERADE

   # Host (controlled machine)
   [Peer]
   PublicKey = <host-public-key>
   AllowedIPs = 10.10.0.2/32

   # Controller
   [Peer]
   PublicKey = <controller-public-key>
   AllowedIPs = 10.10.0.3/32
   ```

4. **Start WireGuard**
   ```bash
   sudo wg-quick up wg0
   # Enable on boot
   sudo systemctl enable wg-quick@wg0
   ```

### Host (Controlled Machine) Setup

1. **Install WireGuard**
   - Windows: https://www.wireguard.com/install/
   - macOS: `brew install wireguard-tools` or install from App Store
   - Linux: `sudo apt install wireguard`

2. **Generate key pair**
   ```bash
   wg genkey | tee client_host_private.key | wg pubkey > client_host_public.key
   ```

3. **Create config** `wg0.conf`:
   ```ini
   [Interface]
   PrivateKey = <host-private-key>
   Address = 10.10.0.2/24
   DNS = 8.8.8.8

   [Peer]
   PublicKey = <server-public-key>
   Endpoint = your-server-ip:51820
   AllowedIPs = 10.10.0.0/24
   PersistentKeepalive = 25
   ```

### Controller Setup

1. **Install WireGuard** (same as above)

2. **Generate key pair**
   ```bash
   wg genkey | tee client_ctrl_private.key | wg pubkey > client_ctrl_public.key
   ```

3. **Create config** `wg0.conf`:
   ```ini
   [Interface]
   PrivateKey = <controller-private-key>
   Address = 10.10.0.3/24
   DNS = 8.8.8.8

   [Peer]
   PublicKey = <server-public-key>
   Endpoint = your-server-ip:51820
   AllowedIPs = 10.10.0.0/24
   PersistentKeepalive = 25
   ```

### Connect LAN-Desk Through WireGuard

1. Start WireGuard on the server, host, and controller
2. Start LAN-Desk on the host and note the PIN
3. In LAN-Desk on the controller, enter the host's WireGuard IP `10.10.0.2` + PIN
4. Click Connect

```
Controller (10.10.0.3) <-> WireGuard Server (10.10.0.1) <-> Host (10.10.0.2)
      LAN-Desk               VPS:51820                       LAN-Desk
   Enter 10.10.0.2                                       Shows PIN: 790700
```

> **Pros**: Fastest speed (kernel-level), strong encryption (ChaCha20), minimal config, very low overhead
> **Cons**: Requires a VPS with public IP, manual key management

## Security Notes

- **Prefer Tailscale/ZeroTier** — they add an extra encryption layer
- **Avoid exposing port 25605 directly** to the public internet
- **Rotate PIN regularly** when using over public networks
- **Close LAN-Desk when not in use**
