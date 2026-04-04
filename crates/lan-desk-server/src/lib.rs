pub mod session;
pub mod tls;

use std::collections::HashMap;
use std::future::Future;
use std::net::IpAddr;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::net::TcpListener;
use tokio::sync::{broadcast, Mutex};
use tokio_rustls::TlsAcceptor;
use tracing::{info, warn};
use zeroize::Zeroizing;

use crate::session::{EncodedFrame, Session};

/// PIN 暴力破解防护：基于 IP 的指数退避速率限制器
///
/// 采用指数退避策略：随着失败次数增加，锁定时间指数增长，
/// 使暴力破解在实际中不可行。同时设有全局速率限制防止多 IP 攻击。
pub struct RateLimiter {
    /// 每 IP 状态：(失败次数, 最后一次失败时间)
    failures: HashMap<IpAddr, (u32, Instant)>,
    /// 全局失败计数：(窗口内总失败次数, 窗口起始时间)
    global_failures: (u32, Instant),
}

/// 基础锁定时长（秒）：首次触发锁定为 5 分钟
const BASE_LOCKOUT_SECS: u64 = 300;
/// 触发锁定的失败次数阈值
const LOCKOUT_THRESHOLD: u32 = 5;
/// 最大锁定时长：24 小时
const MAX_LOCKOUT_SECS: u64 = 86400;
/// 全局速率限制：每分钟最多允许的失败总次数（所有 IP 合计）
const GLOBAL_RATE_LIMIT: u32 = 10;
/// 全局速率限制窗口（秒）
const GLOBAL_WINDOW_SECS: u64 = 60;

impl Default for RateLimiter {
    fn default() -> Self {
        Self::new()
    }
}

impl RateLimiter {
    pub fn new() -> Self {
        Self {
            failures: HashMap::new(),
            global_failures: (0, Instant::now()),
        }
    }

    /// 计算指数退避锁定时长（秒）
    /// 第 1 次触发：5 分钟，第 2 次：15 分钟，第 3 次：1 小时，第 4 次：4 小时，之后：24 小时
    fn lockout_duration_secs(failure_count: u32) -> u64 {
        if failure_count <= LOCKOUT_THRESHOLD {
            return 0;
        }
        let excess = failure_count - LOCKOUT_THRESHOLD;
        let duration =
            BASE_LOCKOUT_SECS.saturating_mul(3u64.saturating_pow(excess.saturating_sub(1)));
        duration.min(MAX_LOCKOUT_SECS)
    }

    /// 记录一次失败并返回是否已锁定
    pub fn check_and_record_failure(&mut self, ip: IpAddr) -> bool {
        let now = Instant::now();

        // 更新全局计数
        if now.duration_since(self.global_failures.1).as_secs() > GLOBAL_WINDOW_SECS {
            self.global_failures = (1, now);
        } else {
            self.global_failures.0 += 1;
        }

        let entry = self.failures.entry(ip).or_insert((0, now));
        entry.0 += 1;
        entry.1 = now;

        entry.0 > LOCKOUT_THRESHOLD
    }

    /// 检查 IP 是否被锁定（指数退避 + 全局限制）
    pub fn is_locked(&self, ip: &IpAddr) -> bool {
        // 全局速率限制检查
        if self.global_failures.1.elapsed().as_secs() <= GLOBAL_WINDOW_SECS
            && self.global_failures.0 > GLOBAL_RATE_LIMIT
        {
            return true;
        }

        // 单 IP 指数退避检查
        if let Some((count, last_failure)) = self.failures.get(ip) {
            let lockout_secs = Self::lockout_duration_secs(*count);
            if lockout_secs > 0 && last_failure.elapsed().as_secs() < lockout_secs {
                return true;
            }
        }
        false
    }

    /// 获取剩余锁定时间（秒），未锁定返回 0
    pub fn remaining_lockout_secs(&self, ip: &IpAddr) -> u64 {
        if let Some((count, last_failure)) = self.failures.get(ip) {
            let lockout_secs = Self::lockout_duration_secs(*count);
            if lockout_secs > 0 {
                let elapsed = last_failure.elapsed().as_secs();
                if elapsed < lockout_secs {
                    return lockout_secs - elapsed;
                }
            }
        }
        0
    }

    /// 成功后清除记录
    pub fn record_success(&mut self, ip: &IpAddr) {
        self.failures.remove(ip);
    }

    /// 清理已过期的失败记录（锁定期已过且超过最大锁定时长），防止 HashMap 无限增长
    pub fn cleanup_expired(&mut self) {
        self.failures.retain(|_, (count, last_failure)| {
            let lockout_secs = Self::lockout_duration_secs(*count);
            // Use lockout duration if locked out, otherwise retain for 5 minutes
            let max_retain = if lockout_secs > 0 { lockout_secs } else { 300 };
            last_failure.elapsed().as_secs() <= max_retain
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::IpAddr;

    #[test]
    fn test_rate_limiter_allows_initial_attempts() {
        let mut limiter = RateLimiter::new();
        let ip: IpAddr = "192.168.1.1".parse().unwrap();
        assert!(!limiter.is_locked(&ip));
        // 前 5 次失败不应该锁定（check_and_record_failure 在第 6 次才返回 true）
        for _ in 0..5 {
            assert!(!limiter.check_and_record_failure(ip));
        }
    }

    #[test]
    fn test_rate_limiter_locks_after_threshold() {
        let mut limiter = RateLimiter::new();
        let ip: IpAddr = "192.168.1.2".parse().unwrap();
        // 触发 6 次失败（超过 LOCKOUT_THRESHOLD=5）
        for _ in 0..6 {
            limiter.check_and_record_failure(ip);
        }
        assert!(limiter.is_locked(&ip));
    }

    #[test]
    fn test_rate_limiter_different_ips_independent() {
        let mut limiter = RateLimiter::new();
        let ip1: IpAddr = "192.168.1.1".parse().unwrap();
        let ip2: IpAddr = "192.168.1.2".parse().unwrap();
        // ip1 触发锁定
        for _ in 0..6 {
            limiter.check_and_record_failure(ip1);
        }
        assert!(limiter.is_locked(&ip1));
        // ip2 不受影响（全局限制未触发，因为只有 6 次）
        assert!(!limiter.is_locked(&ip2));
    }

    #[test]
    fn test_rate_limiter_exponential_backoff_durations() {
        // 验证指数退避锁定时长计算
        assert_eq!(RateLimiter::lockout_duration_secs(5), 0); // 阈值以内不锁定
        assert_eq!(RateLimiter::lockout_duration_secs(6), 300); // 第 1 次触发：5 分钟
        assert_eq!(RateLimiter::lockout_duration_secs(7), 900); // 第 2 次：15 分钟
        assert_eq!(RateLimiter::lockout_duration_secs(8), 2700); // 第 3 次：45 分钟
        assert_eq!(RateLimiter::lockout_duration_secs(9), 8100); // 第 4 次：2.25 小时
                                                                 // 超过最大值后封顶为 24 小时
        assert_eq!(RateLimiter::lockout_duration_secs(15), MAX_LOCKOUT_SECS);
    }

    #[test]
    fn test_rate_limiter_global_limit() {
        let mut limiter = RateLimiter::new();
        // 从不同 IP 快速累计超过全局限制
        for i in 0..12 {
            let ip: IpAddr = format!("192.168.1.{}", i).parse().unwrap();
            limiter.check_and_record_failure(ip);
        }
        // 全局限制触发后，即使新 IP 也被锁定
        let new_ip: IpAddr = "10.0.0.1".parse().unwrap();
        assert!(limiter.is_locked(&new_ip));
    }

    #[test]
    fn test_rate_limiter_cleanup_expired() {
        let mut limiter = RateLimiter::new();
        let ip1: IpAddr = "192.168.1.1".parse().unwrap();
        let ip2: IpAddr = "192.168.1.2".parse().unwrap();
        // 记录两个 IP 的失败
        limiter.check_and_record_failure(ip1);
        limiter.check_and_record_failure(ip2);
        assert_eq!(limiter.failures.len(), 2);
        // cleanup 不应删除未过期的条目
        limiter.cleanup_expired();
        assert_eq!(limiter.failures.len(), 2);
    }

    #[test]
    fn test_rate_limiter_success_clears_record() {
        let mut limiter = RateLimiter::new();
        let ip: IpAddr = "192.168.1.3".parse().unwrap();
        // 累计 4 次失败
        for _ in 0..4 {
            limiter.check_and_record_failure(ip);
        }
        assert!(!limiter.is_locked(&ip));
        // 成功后清除记录
        limiter.record_success(&ip);
        assert!(!limiter.is_locked(&ip));
        // 重新计数，应该能再次承受 5 次失败
        for _ in 0..5 {
            assert!(!limiter.check_and_record_failure(ip));
        }
    }

    #[test]
    fn test_rate_limiter_remaining_lockout() {
        let mut limiter = RateLimiter::new();
        let ip: IpAddr = "192.168.1.4".parse().unwrap();
        // 触发锁定
        for _ in 0..6 {
            limiter.check_and_record_failure(ip);
        }
        // 刚触发时剩余时间应接近 BASE_LOCKOUT_SECS
        let remaining = limiter.remaining_lockout_secs(&ip);
        assert!(remaining > 0 && remaining <= BASE_LOCKOUT_SECS);
    }
}

/// 授权请求信息
#[derive(Debug, Clone)]
pub struct AuthRequest {
    pub hostname: String,
    pub addr: String,
    /// 实际授予的角色（Controller / Viewer）
    pub granted_role: String,
}

/// 授权回调类型
pub type AuthCallback =
    Arc<dyn Fn(AuthRequest) -> Pin<Box<dyn Future<Output = bool> + Send>> + Send + Sync>;

struct SessionCounter {
    controller_count: u32,
    observer_count: u32,
}

/// 被控端 TCP 服务器（共享帧广播 + 多用户 + 双 PIN 权限）
pub struct Server {
    listener: TcpListener,
    tls_acceptor: TlsAcceptor,
    sessions: Arc<Mutex<SessionCounter>>,
    control_pin: Zeroizing<String>,
    view_pin: Zeroizing<String>,
    auto_accept: bool,
    auth_callback: Option<AuthCallback>,
    max_observers: u32,
    /// 共享帧广播（第一个连接时初始化捕获线程）
    frame_broadcast: Arc<Mutex<Option<broadcast::Sender<EncodedFrame>>>>,
    /// 捕获线程停止信号
    capture_stop_signal: Arc<AtomicBool>,
    /// PIN 暴力破解防护
    rate_limiter: Arc<Mutex<RateLimiter>>,
    /// 聊天消息转发通道（转发到本地前端）
    pub chat_tx: Option<tokio::sync::mpsc::Sender<(String, String, u64)>>,
    /// 会话事件通知通道（连接/断开 → 本地前端）
    /// 格式: (event_type, hostname, addr, role)  event_type: "connected" | "disconnected"
    pub session_event_tx: Option<tokio::sync::mpsc::Sender<(String, String, String, String)>>,
    /// 被控端→远程控制端的聊天广播通道
    host_chat_broadcast: broadcast::Sender<(String, String, u64)>,
    /// 是否允许远程 Shell 访问
    pub shell_enabled: bool,
    /// 共享原子标志（由外部传入，动态读取最新值）
    pub shell_enabled_flag: Option<Arc<AtomicBool>>,
    /// 会话空闲超时（秒），0 表示不启用
    pub idle_timeout_secs: u64,
    /// 断开时是否自动锁屏
    pub lock_on_disconnect: bool,
    /// 共享捕获线程命令通道（切换显示器、更新设置）
    shared_capture_cmd_tx:
        Mutex<Option<tokio::sync::mpsc::Sender<session::capture::CaptureCommand>>>,
}

impl Server {
    #[allow(clippy::too_many_arguments)]
    pub async fn bind(
        port: u16,
        control_pin: Zeroizing<String>,
        view_pin: Zeroizing<String>,
        auto_accept: bool,
        data_dir: Option<&std::path::Path>,
        shell_enabled: bool,
        idle_timeout_secs: u64,
        lock_on_disconnect: bool,
    ) -> anyhow::Result<Self> {
        let listener = TcpListener::bind(("0.0.0.0", port)).await?;
        let tls_acceptor = tls::create_tls_acceptor(data_dir)?;
        info!(
            "被控端服务器已监听端口 {} (TLS), 控制PIN: [{}位], 查看PIN: [{}位]",
            port,
            control_pin.len(),
            view_pin.len()
        );

        Ok(Self {
            listener,
            tls_acceptor,
            sessions: Arc::new(Mutex::new(SessionCounter {
                controller_count: 0,
                observer_count: 0,
            })),
            control_pin,
            view_pin,
            auto_accept,
            auth_callback: None,
            max_observers: 32,
            frame_broadcast: Arc::new(Mutex::new(None)),
            capture_stop_signal: Arc::new(AtomicBool::new(false)),
            rate_limiter: Arc::new(Mutex::new(RateLimiter::new())),
            chat_tx: None,
            session_event_tx: None,
            host_chat_broadcast: broadcast::channel(16).0,
            shell_enabled,
            shell_enabled_flag: None,
            idle_timeout_secs,
            lock_on_disconnect,
            shared_capture_cmd_tx: Mutex::new(None),
        })
    }

    pub fn set_auth_callback(&mut self, cb: AuthCallback) {
        self.auth_callback = Some(cb);
    }

    /// 获取被控端聊天广播发送端（用于被控端向远程控制端发送消息）
    pub fn host_chat_tx(&self) -> broadcast::Sender<(String, String, u64)> {
        self.host_chat_broadcast.clone()
    }

    /// 确保共享捕获线程已启动，返回帧接收端
    async fn ensure_capture_started(&self) -> broadcast::Receiver<EncodedFrame> {
        let mut broadcast_lock = self.frame_broadcast.lock().await;
        if let Some(ref tx) = *broadcast_lock {
            return tx.subscribe();
        }

        // 首次连接：启动共享捕获线程（带命令通道）
        let (tx, rx) = broadcast::channel::<EncodedFrame>(32);
        let tx_clone = tx.clone();

        // 创建共享命令通道
        let (cmd_tx, cmd_rx) =
            tokio::sync::mpsc::channel::<crate::session::capture::CaptureCommand>(8);
        *self.shared_capture_cmd_tx.lock().await = Some(cmd_tx);

        // 重置停止信号
        self.capture_stop_signal.store(false, Ordering::Release);
        let stop_signal = self.capture_stop_signal.clone();

        std::thread::Builder::new()
            .name("shared-screen-capture".to_string())
            .spawn(move || {
                let rt = tokio::runtime::Builder::new_current_thread()
                    .enable_time()
                    .build()
                    .unwrap();
                rt.block_on(async {
                    Session::<tokio_rustls::server::TlsStream<tokio::net::TcpStream>>::shared_capture_loop_with_cmd(tx_clone, stop_signal, Some(cmd_rx)).await;
                });
            })
            .ok();

        *broadcast_lock = Some(tx);
        rx
    }

    pub async fn run(&self) -> anyhow::Result<()> {
        let mut cleanup_interval = tokio::time::interval(std::time::Duration::from_secs(60));
        loop {
            tokio::select! {
                result = self.listener.accept() => {
                let (stream, addr) = match result {
                    Ok(v) => v,
                    Err(e) => {
                        warn!("TCP accept 失败（瞬态错误，继续监听）: {}", e);
                        continue;
                    }
                };
                info!("收到来自 {} 的 TCP 连接", addr);

            // PIN 暴力破解防护：检查 IP 是否被锁定
            {
                let limiter = self.rate_limiter.lock().await;
                if limiter.is_locked(&addr.ip()) {
                    warn!("IP {} 因频繁失败被锁定，拒绝连接", addr.ip());
                    drop(stream);
                    continue;
                }
            }

            let total = {
                let s = self.sessions.lock().await;
                s.controller_count + s.observer_count
            };
            if total >= self.max_observers.saturating_add(1) {
                warn!("连接数已满（{}），拒绝 {}", total, addr);
                drop(stream);
                continue;
            }

            let tls_stream = match self.tls_acceptor.accept(stream).await {
                Ok(s) => {
                    info!("TLS 握手成功: {}", addr);
                    s
                }
                Err(e) => {
                    warn!("TLS 握手失败 {}: {}", addr, e);
                    continue;
                }
            };

            // 获取共享帧接收端
            let frame_rx = self.ensure_capture_started().await;

            let sessions = self.sessions.clone();
            let control_pin = self.control_pin.clone();
            let view_pin = self.view_pin.clone();
            let auto_accept = self.auto_accept;
            let auth_cb = if auto_accept { None } else { self.auth_callback.clone() };
            let rate_limiter = self.rate_limiter.clone();
            // 每次新连接时读取最新的 shell_enabled（支持运行时动态切换）
            let shell_enabled = self.shell_enabled_flag.as_ref()
                .map(|f| f.load(Ordering::Relaxed))
                .unwrap_or(self.shell_enabled);
            let idle_timeout_secs = self.idle_timeout_secs;
            let lock_on_disconnect = self.lock_on_disconnect;
            let frame_broadcast = self.frame_broadcast.clone();
            let capture_stop_signal = self.capture_stop_signal.clone();
            let chat_tx = self.chat_tx.clone();
            let session_event_tx = self.session_event_tx.clone();
            let shared_cmd_tx = self.shared_capture_cmd_tx.lock().await.clone();
            let host_chat_rx = self.host_chat_broadcast.subscribe();

            tokio::spawn(async move {
                match Session::new_with_broadcast(
                    tls_stream, addr, &control_pin, &view_pin,
                    auth_cb.as_ref(), frame_rx, rate_limiter.clone(),
                    shell_enabled,
                    Duration::from_secs(idle_timeout_secs),
                    lock_on_disconnect,
                ).await {
                    Ok(mut session) => {
                        // 设置聊天消息转发通道
                        if let Some(ref tx) = chat_tx {
                            session.set_chat_tx(tx.clone());
                        }
                        // 共享捕获模式下，把共享命令通道传给 session
                        if session.capture_cmd_tx.is_none() {
                            if let Some(ref tx) = shared_cmd_tx {
                                session.capture_cmd_tx = Some(tx.clone());
                            }
                        }
                        // 设置被控端主机聊天接收通道
                        session.set_host_chat_rx(host_chat_rx);
                        let hostname = session.remote_hostname.clone();
                        let role = session.role_str().to_string();

                        // 根据认证结果更新计数
                        {
                            let mut s = sessions.lock().await;
                            if session.is_viewer() {
                                s.observer_count += 1;
                                info!("{} ({}) 以观察者身份连接", hostname, addr);
                            } else {
                                s.controller_count += 1;
                                info!("{} ({}) 以控制者身份连接", hostname, addr);
                            }
                        }

                        // 通知前端：会话已连接
                        if let Some(ref tx) = session_event_tx {
                            let _ = tx.try_send(("connected".into(), hostname.clone(), addr.to_string(), role.clone()));
                        }

                        let was_viewer = session.is_viewer();
                        if let Err(e) = session.run().await {
                            warn!("会话 {} 异常退出: {}", addr, e);
                        }
                        info!("会话 {} ({}) 已结束", hostname, addr);

                        // 通知前端：会话已断开
                        if let Some(ref tx) = session_event_tx {
                            let _ = tx.try_send(("disconnected".into(), hostname, addr.to_string(), role));
                        }
                        let total = {
                            let mut s = sessions.lock().await;
                            if was_viewer {
                                s.observer_count = s.observer_count.saturating_sub(1);
                            } else {
                                s.controller_count = s.controller_count.saturating_sub(1);
                            }
                            s.controller_count + s.observer_count
                        };

                        // 所有连接已断开，启动 30 秒延迟清理
                        if total == 0 {
                            info!("所有连接已断开，30 秒后将停止捕获线程");
                            let sessions_for_cleanup = sessions.clone();
                            let fb = frame_broadcast.clone();
                            let stop = capture_stop_signal.clone();
                            tokio::spawn(async move {
                                tokio::time::sleep(Duration::from_secs(30)).await;
                                let s = sessions_for_cleanup.lock().await;
                                let current_total = s.controller_count + s.observer_count;
                                drop(s);
                                if current_total == 0 {
                                    info!("30 秒内无新连接，停止捕获线程");
                                    stop.store(true, Ordering::Release);
                                    // 清除广播 Sender，下次连接时将重新启动捕获线程
                                    let mut broadcast_lock = fb.lock().await;
                                    *broadcast_lock = None;
                                } else {
                                    info!("延迟清理期间有新连接（当前 {} 个），取消停止", current_total);
                                }
                            });
                        }
                    }
                    Err(e) => {
                        warn!("创建会话失败 {}: {}", addr, e);
                    }
                }
            });
                }
                _ = cleanup_interval.tick() => {
                    let mut limiter = self.rate_limiter.lock().await;
                    limiter.cleanup_expired();
                }
            }
        }
    }
}
