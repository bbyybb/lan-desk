use std::time::Duration;

use tauri::State;

use lan_desk_protocol::discovery::DiscoveryService;

use crate::state::{AppState, SecurePin};

use super::PeerInfo;
use super::PinPair;

// ──────────────── PIN 管理 ────────────────

/// 获取当前双 PIN
#[tauri::command]
pub async fn get_pins(state: State<'_, AppState>) -> Result<PinPair, String> {
    let auth = state.auth.read().await;
    Ok(PinPair {
        control_pin: auth.control_pin.as_str().to_string(),
        view_pin: auth.view_pin.as_str().to_string(),
    })
}

/// 刷新双 PIN（生成新的一对）
#[tauri::command]
pub async fn refresh_pins(state: State<'_, AppState>) -> Result<PinPair, String> {
    let (new_control, new_view) = lan_desk_protocol::generate_pin_pair();
    let result = PinPair {
        control_pin: new_control.clone(),
        view_pin: new_view.clone(),
    };
    let mut auth = state.auth.write().await;
    auth.control_pin = SecurePin::new(new_control);
    auth.view_pin = SecurePin::new(new_view);
    Ok(result)
}

/// 设置无人值守模式
#[tauri::command]
pub async fn set_unattended(
    auto_accept: bool,
    fixed_password: bool,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let mut auth = state.auth.write().await;
    auth.auto_accept = auto_accept;
    auth.fixed_password = fixed_password;
    Ok(())
}

/// 设置固定密码（替代随机 PIN）
#[tauri::command]
pub async fn set_fixed_pins(
    control_pin: String,
    view_pin: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    if control_pin.len() < 6 || control_pin.len() > 20 {
        return Err(
            "[ERR_CONTROL_PIN_LENGTH] Control password must be 6-20 characters".to_string(),
        );
    }
    if view_pin.len() < 6 || view_pin.len() > 20 {
        return Err("[ERR_VIEW_PIN_LENGTH] View password must be 6-20 characters".to_string());
    }
    if control_pin == view_pin {
        return Err("[ERR_PINS_SAME] Control and view passwords must be different".to_string());
    }
    let mut auth = state.auth.write().await;
    auth.control_pin = SecurePin::new(control_pin);
    auth.view_pin = SecurePin::new(view_pin);
    Ok(())
}

// ──────────────── 设备发现 ────────────────

#[tauri::command]
pub async fn discover_peers(state: State<'_, AppState>) -> Result<Vec<PeerInfo>, String> {
    #[cfg(feature = "desktop")]
    let port = state.server.read().await.port;
    #[cfg(not(feature = "desktop"))]
    let port = lan_desk_protocol::DEFAULT_TCP_PORT;

    let hostname = hostname::get()
        .map(|h| h.to_string_lossy().to_string())
        .unwrap_or_else(|_| "unknown".to_string());

    let device_id = crate::commands::server::get_device_id();
    let service = DiscoveryService::bind_with_screen(hostname, port, 0, 0, device_id)
        .await
        .map_err(|e| {
            format!(
                "[ERR_DISCOVERY_BIND] Failed to bind discovery service: {}",
                e
            )
        })?;

    let peers = service
        .discover(Duration::from_secs(2))
        .await
        .map_err(|e| format!("[ERR_DISCOVERY_FAILED] Device discovery failed: {}", e))?;

    Ok(peers
        .into_iter()
        .map(|p| PeerInfo {
            addr: format!("{}:{}", p.addr.ip(), p.tcp_port),
            hostname: p.hostname,
            os: p.os,
            device_id: p.device_id,
        })
        .collect())
}

// ──────────────── 网络信息 ────────────────

#[derive(Debug, Clone, serde::Serialize)]
pub struct NetworkAddr {
    pub ip: String,
    pub name: String,
    /// "lan" | "tailscale" | "zerotier" | "other"
    pub net_type: String,
}

/// 获取本机所有 IPv4 地址，标注 Tailscale/ZeroTier 虚拟网卡
#[tauri::command]
pub fn get_network_info() -> Vec<NetworkAddr> {
    let mut addrs = Vec::new();
    if let Ok(ifaces) = if_addrs::get_if_addrs() {
        for iface in ifaces {
            if iface.is_loopback() {
                continue;
            }
            let ip = iface.ip();
            if !ip.is_ipv4() {
                continue;
            }
            let ip_str = ip.to_string();
            let name = iface.name.clone();
            let name_lower = name.to_lowercase();

            // Tailscale: 接口名含 "tailscale" 或 IP 在 100.64.0.0/10 范围
            // ZeroTier: 接口名以 "zt" 开头
            let net_type = if name_lower.contains("tailscale") || is_tailscale_ip(&ip_str) {
                "tailscale"
            } else if name_lower.starts_with("zt") {
                "zerotier"
            } else if ip_str.starts_with("192.168.")
                || ip_str.starts_with("10.")
                || is_rfc1918_172(&ip_str)
            {
                "lan"
            } else {
                "other"
            };

            addrs.push(NetworkAddr {
                ip: ip_str,
                name,
                net_type: net_type.to_string(),
            });
        }
    }
    // VPN 地址排在前面
    addrs.sort_by_key(|a| match a.net_type.as_str() {
        "tailscale" => 0,
        "zerotier" => 1,
        "lan" => 2,
        _ => 3,
    });
    addrs
}

/// RFC 1918: 172.16.0.0/12 (172.16.x.x - 172.31.x.x)
fn is_rfc1918_172(ip: &str) -> bool {
    if let Some(rest) = ip.strip_prefix("172.") {
        if let Some(second) = rest.split('.').next() {
            if let Ok(n) = second.parse::<u8>() {
                return (16..=31).contains(&n);
            }
        }
    }
    false
}

fn is_tailscale_ip(ip: &str) -> bool {
    // Tailscale uses 100.64.0.0/10 (100.64.x.x - 100.127.x.x)
    if let Some(rest) = ip.strip_prefix("100.") {
        if let Some(second) = rest.split('.').next() {
            if let Ok(n) = second.parse::<u8>() {
                return (64..=127).contains(&n);
            }
        }
    }
    false
}

// ──────────────── Wake-on-LAN ────────────────

/// 发送 WOL 魔术包唤醒远程设备
#[tauri::command]
pub async fn wake_on_lan(mac_address: String) -> Result<(), String> {
    let mac_bytes = parse_mac(&mac_address)
        .ok_or_else(|| format!("[ERR_INVALID_MAC] Invalid MAC address: {}", mac_address))?;

    // 构建魔术包：6 个 0xFF + MAC 地址重复 16 次
    let mut packet = vec![0xFFu8; 6];
    for _ in 0..16 {
        packet.extend_from_slice(&mac_bytes);
    }

    // UDP 广播到 255.255.255.255:9
    let socket = tokio::net::UdpSocket::bind("0.0.0.0:0")
        .await
        .map_err(|e| format!("[ERR_UDP_BIND] Failed to bind UDP: {}", e))?;
    socket
        .set_broadcast(true)
        .map_err(|e| format!("[ERR_BROADCAST] Failed to set broadcast: {}", e))?;
    socket
        .send_to(&packet, "255.255.255.255:9")
        .await
        .map_err(|e| format!("[ERR_WOL_SEND] Failed to send WOL packet: {}", e))?;

    tracing::info!("已发送 WOL 魔术包到 {}", mac_address);
    Ok(())
}

fn parse_mac(mac: &str) -> Option<[u8; 6]> {
    let parts: Vec<&str> = mac.split([':', '-']).collect();
    if parts.len() != 6 {
        return None;
    }
    let mut bytes = [0u8; 6];
    for (i, part) in parts.iter().enumerate() {
        bytes[i] = u8::from_str_radix(part, 16).ok()?;
    }
    Some(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    // ──────────────── is_tailscale_ip ────────────────

    #[test]
    fn tailscale_ip_range_lower_bound() {
        assert!(is_tailscale_ip("100.64.0.1"));
    }

    #[test]
    fn tailscale_ip_range_upper_bound() {
        assert!(is_tailscale_ip("100.127.255.255"));
    }

    #[test]
    fn tailscale_ip_below_range() {
        assert!(!is_tailscale_ip("100.63.0.1"));
    }

    #[test]
    fn tailscale_ip_above_range() {
        assert!(!is_tailscale_ip("100.128.0.1"));
    }

    #[test]
    fn tailscale_ip_private_address() {
        assert!(!is_tailscale_ip("192.168.1.1"));
    }

    // ──────────────── is_rfc1918_172 ────────────────

    #[test]
    fn rfc1918_172_lower_bound() {
        assert!(is_rfc1918_172("172.16.0.1"));
    }

    #[test]
    fn rfc1918_172_upper_bound() {
        assert!(is_rfc1918_172("172.31.255.255"));
    }

    #[test]
    fn rfc1918_172_below_range() {
        assert!(!is_rfc1918_172("172.15.0.1"));
    }

    #[test]
    fn rfc1918_172_above_range() {
        assert!(!is_rfc1918_172("172.32.0.1"));
    }
}
