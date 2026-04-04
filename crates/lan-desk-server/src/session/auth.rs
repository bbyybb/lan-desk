use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use futures::SinkExt;
use tokio::sync::Mutex;
use tokio_util::codec::Framed;
use tracing::{info, warn};

use lan_desk_protocol::codec::LanDeskCodec;
use lan_desk_protocol::message::{Message, SessionRole};
use lan_desk_protocol::PROTOCOL_VERSION;

use crate::{AuthCallback, AuthRequest, RateLimiter};

use super::Session;

impl<S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static> Session<S> {
    /// 验证 PIN 并请求用户授权（共享逻辑）
    #[allow(clippy::too_many_arguments)]
    pub(super) async fn verify_and_auth(
        framed: &mut Framed<S, LanDeskCodec>,
        addr: SocketAddr,
        hostname: &str,
        pin: &str,
        pin_salt: &str,
        requested_role: SessionRole,
        control_pin: &str,
        view_pin: &str,
        auth_callback: Option<&AuthCallback>,
        rate_limiter: Option<&Arc<Mutex<RateLimiter>>>,
    ) -> anyhow::Result<SessionRole> {
        // 检查 IP 是否已被锁定（指数退避）
        if let Some(rl) = rate_limiter {
            let limiter = rl.lock().await;
            if limiter.is_locked(&addr.ip()) {
                let remaining = limiter.remaining_lockout_secs(&addr.ip());
                let remaining_min = remaining.div_ceil(60);
                warn!(
                    "IP {} 因频繁 PIN 验证失败被锁定，剩余 {} 分钟",
                    addr.ip(),
                    remaining_min
                );
                framed
                    .send(Message::HelloAck {
                        version: PROTOCOL_VERSION,
                        accepted: false,
                        reject_reason: format!(
                            "Too many failed attempts, try again in {} min",
                            remaining_min
                        ),
                        granted_role: SessionRole::Viewer,
                    })
                    .await?;
                anyhow::bail!("IP 被锁定（剩余 {} 分钟）", remaining_min);
            }
        }

        let control_hash = lan_desk_protocol::hash_pin(control_pin, pin_salt);
        let view_hash = lan_desk_protocol::hash_pin(view_pin, pin_salt);

        let granted_role = if lan_desk_protocol::ct_eq_str(pin, &control_hash) {
            requested_role
        } else if lan_desk_protocol::ct_eq_str(pin, &view_hash) {
            SessionRole::Viewer
        } else {
            // PIN 验证失败：记录失败并检查是否触发锁定
            if let Some(rl) = rate_limiter {
                let mut limiter = rl.lock().await;
                let locked = limiter.check_and_record_failure(addr.ip());
                if locked {
                    warn!("IP {} PIN 验证失败次数过多，已锁定", addr.ip());
                }
            }
            tokio::time::sleep(Duration::from_secs(2)).await;
            warn!("PIN 验证失败: 来自 {} ({})", hostname, addr);
            framed
                .send(Message::HelloAck {
                    version: PROTOCOL_VERSION,
                    accepted: false,
                    reject_reason: "密码错误".to_string(),
                    granted_role: SessionRole::Viewer,
                })
                .await?;
            anyhow::bail!("PIN 验证失败");
        };

        // PIN 验证成功：清除失败记录
        if let Some(rl) = rate_limiter {
            let mut limiter = rl.lock().await;
            limiter.record_success(&addr.ip());
        }

        if let Some(cb) = auth_callback {
            let request = AuthRequest {
                hostname: hostname.to_string(),
                addr: addr.to_string(),
                granted_role: format!("{:?}", granted_role),
            };
            if !cb(request).await {
                info!("用户拒绝了来自 {} ({}) 的连接", hostname, addr);
                framed
                    .send(Message::HelloAck {
                        version: PROTOCOL_VERSION,
                        accepted: false,
                        reject_reason: "被控端用户拒绝了连接".to_string(),
                        granted_role: SessionRole::Viewer,
                    })
                    .await?;
                anyhow::bail!("用户拒绝连接");
            }
        }

        framed
            .send(Message::HelloAck {
                version: PROTOCOL_VERSION,
                accepted: true,
                reject_reason: String::new(),
                granted_role,
            })
            .await?;

        Ok(granted_role)
    }
}
