use std::collections::HashMap;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use tokio::sync::RwLock;
use tokio_util::sync::CancellationToken;
use zeroize::Zeroizing;

use crate::ws_bridge::WsBridge;
use lan_desk_protocol::generate_pin_pair;
use lan_desk_protocol::message::Message;
#[cfg(feature = "desktop")]
use lan_desk_protocol::DEFAULT_TCP_PORT;

/// 敏感字符串包装器：基于 zeroize crate，Drop 时自动清零内存，Debug 隐藏内容
#[derive(Clone)]
pub struct SecurePin(Zeroizing<String>);

impl SecurePin {
    pub fn new(s: String) -> Self {
        Self(Zeroizing::new(s))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// 将内部值包装为 Zeroizing<String> 传递给外部 crate（如 lan-desk-server）
    pub fn to_zeroizing(&self) -> Zeroizing<String> {
        Zeroizing::new(String::clone(&self.0))
    }
}

impl std::fmt::Debug for SecurePin {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "SecurePin(****)")
    }
}

/// 连接信息
pub struct ConnectionInfo {
    pub remote_addr: String,
}

/// PIN 和认证相关状态（统一加锁，操作原子）
pub struct AuthState {
    pub control_pin: SecurePin,
    pub view_pin: SecurePin,
    pub auto_accept: bool,
    pub fixed_password: bool,
    pub last_pin: SecurePin,
}

/// 服务器运行状态
#[cfg(feature = "desktop")]
pub struct ServerState {
    pub running: bool,
    pub stop_tx: Option<tokio::sync::oneshot::Sender<()>>,
    pub port: u16,
    /// 共享原子标志：是否允许远程 Shell（Server 运行循环直接读取）
    pub shell_enabled: Arc<std::sync::atomic::AtomicBool>,
    pub idle_timeout_secs: u64,
    pub lock_on_disconnect: bool,
    /// 被控端聊天广播发送端（向所有远程控制端广播消息）
    pub host_chat_tx: Option<tokio::sync::broadcast::Sender<(String, String, u64)>>,
}

/// 单个连接的状态
#[allow(dead_code)]
pub struct SingleConnState {
    pub info: Option<ConnectionInfo>,
    pub input_tx: Option<tokio::sync::mpsc::Sender<Message>>,
    pub ws_bridge: Option<WsBridge>,
    pub clipboard_enabled: Arc<AtomicBool>,
}

/// 多连接管理器
pub struct ConnState {
    pub connections: HashMap<String, SingleConnState>,
    next_id: u32,
    /// 兼容旧代码：保留默认连接字段（单连接模式时使用）
    pub info: Option<ConnectionInfo>,
    pub input_tx: Option<tokio::sync::mpsc::Sender<Message>>,
    pub ws_bridge: Option<WsBridge>,
    pub clipboard_enabled: Arc<AtomicBool>,
    /// 活跃传输的取消令牌（transfer_id -> CancellationToken）
    pub active_transfers: HashMap<u32, CancellationToken>,
    /// 缓存的远程显示器列表（被控端推送，解决事件竞态）
    pub remote_monitors: Vec<lan_desk_protocol::message::MonitorInfo>,
}

impl ConnState {
    pub fn new() -> Self {
        Self {
            connections: HashMap::new(),
            next_id: 1,
            info: None,
            input_tx: None,
            ws_bridge: None,
            clipboard_enabled: Arc::new(AtomicBool::new(true)),
            active_transfers: HashMap::new(),
            remote_monitors: Vec::new(),
        }
    }

    /// 注册一个活跃传输并返回取消令牌
    pub fn register_transfer(&mut self, id: u32) -> CancellationToken {
        let token = CancellationToken::new();
        self.active_transfers.insert(id, token.clone());
        token
    }

    /// 取消指定传输
    pub fn cancel_transfer(&mut self, id: u32) -> bool {
        if let Some(token) = self.active_transfers.remove(&id) {
            token.cancel();
            true
        } else {
            false
        }
    }

    /// 移除已完成的传输
    pub fn remove_transfer(&mut self, id: u32) {
        self.active_transfers.remove(&id);
    }

    /// 创建新连接并返回 connection_id
    pub fn create_connection(&mut self) -> String {
        let id = format!("conn_{}", self.next_id);
        self.next_id += 1;
        self.connections.insert(
            id.clone(),
            SingleConnState {
                info: None,
                input_tx: None,
                ws_bridge: None,
                clipboard_enabled: Arc::new(AtomicBool::new(true)),
            },
        );
        id
    }

    /// 获取指定连接
    #[allow(dead_code)]
    pub fn get_connection(&self, id: &str) -> Option<&SingleConnState> {
        if id.is_empty() {
            // 空 id 表示使用默认连接（向后兼容）
            None
        } else {
            self.connections.get(id)
        }
    }

    /// 获取指定连接（可变）
    pub fn get_connection_mut(&mut self, id: &str) -> Option<&mut SingleConnState> {
        if id.is_empty() {
            None
        } else {
            self.connections.get_mut(id)
        }
    }

    /// 移除连接
    pub fn remove_connection(&mut self, id: &str) {
        self.connections.remove(id);
    }

    /// 获取 input_tx（兼容：先检查 connection_id，再检查默认）
    pub fn get_input_tx(&self, connection_id: &str) -> Option<&tokio::sync::mpsc::Sender<Message>> {
        if !connection_id.is_empty() {
            if let Some(conn) = self.connections.get(connection_id) {
                return conn.input_tx.as_ref();
            }
        }
        self.input_tx.as_ref()
    }
}

/// 应用全局状态（按语义分组）
pub struct AppState {
    pub auth: Arc<RwLock<AuthState>>,
    #[cfg(feature = "desktop")]
    pub server: Arc<RwLock<ServerState>>,
    pub conn: Arc<RwLock<ConnState>>,
}

impl AppState {
    pub fn new() -> Self {
        let (control_pin, view_pin) = generate_pin_pair();
        Self {
            auth: Arc::new(RwLock::new(AuthState {
                control_pin: SecurePin::new(control_pin),
                view_pin: SecurePin::new(view_pin),
                auto_accept: false,
                fixed_password: false,
                last_pin: SecurePin::new(String::new()),
            })),
            #[cfg(feature = "desktop")]
            server: Arc::new(RwLock::new(ServerState {
                running: false,
                stop_tx: None,
                port: DEFAULT_TCP_PORT,
                shell_enabled: Arc::new(std::sync::atomic::AtomicBool::new(false)),
                idle_timeout_secs: 0,
                lock_on_disconnect: false,
                host_chat_tx: None,
            })),
            conn: Arc::new(RwLock::new(ConnState::new())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_app_state_new_has_valid_pins() {
        let state = AppState::new();
        let auth = state.auth.read().await;
        assert_eq!(auth.control_pin.as_str().len(), 6);
        assert_eq!(auth.view_pin.as_str().len(), 6);
        assert_ne!(auth.control_pin.as_str(), auth.view_pin.as_str());
    }

    #[cfg(feature = "desktop")]
    #[tokio::test]
    async fn test_app_state_default_values() {
        let state = AppState::new();
        let server = state.server.read().await;
        assert!(!server.running);
        assert_eq!(server.port, lan_desk_protocol::DEFAULT_TCP_PORT);
        // 验证新增字段默认值
        assert!(!server
            .shell_enabled
            .load(std::sync::atomic::Ordering::Relaxed));
        drop(server);

        let conn = state.conn.read().await;
        assert!(conn.info.is_none());
        assert!(conn.input_tx.is_none());
        assert!(conn
            .clipboard_enabled
            .load(std::sync::atomic::Ordering::Relaxed));
        assert!(conn.connections.is_empty());
    }

    #[test]
    fn test_zeroize_string_as_str() {
        let s = SecurePin::new("secret123".to_string());
        assert_eq!(s.as_str(), "secret123");
        assert!(!s.is_empty());
    }

    #[test]
    fn test_zeroize_string_empty() {
        let s = SecurePin::new(String::new());
        assert!(s.is_empty());
        assert_eq!(s.as_str(), "");
    }

    #[test]
    fn test_zeroize_string_debug_hides_content() {
        let s = SecurePin::new("my_secret".to_string());
        let debug_output = format!("{:?}", s);
        assert!(!debug_output.contains("my_secret"));
        assert!(debug_output.contains("***"));
    }

    #[test]
    fn test_zeroize_string_debug_masked() {
        let s = SecurePin::new("password".to_string());
        assert_eq!(format!("{:?}", s), "SecurePin(****)");
    }

    #[test]
    fn test_zeroize_string_clone() {
        let s1 = SecurePin::new("test".to_string());
        let s2 = s1.clone();
        assert_eq!(s1.as_str(), s2.as_str());
    }

    #[tokio::test]
    async fn test_last_pin_is_zeroize_string() {
        let state = AppState::new();
        let auth = state.auth.read().await;
        assert!(auth.last_pin.is_empty());
        // 确认 Debug 输出不泄露内容
        let debug = format!("{:?}", auth.last_pin);
        assert!(debug.contains("***"));
    }

    #[test]
    fn test_conn_state_create_connection() {
        let mut conn = ConnState::new();
        let id = conn.create_connection();
        assert_eq!(id, "conn_1");
        assert!(conn.connections.contains_key("conn_1"));

        let id2 = conn.create_connection();
        assert_eq!(id2, "conn_2");
        assert_eq!(conn.connections.len(), 2);
    }

    #[test]
    fn test_conn_state_get_connection() {
        let mut conn = ConnState::new();
        let id = conn.create_connection();

        // 正常获取
        assert!(conn.get_connection(&id).is_some());

        // 空 id 返回 None（向后兼容）
        assert!(conn.get_connection("").is_none());

        // 不存在的 id 返回 None
        assert!(conn.get_connection("conn_999").is_none());
    }

    #[test]
    fn test_conn_state_remove_connection() {
        let mut conn = ConnState::new();
        let id = conn.create_connection();
        assert_eq!(conn.connections.len(), 1);

        conn.remove_connection(&id);
        assert_eq!(conn.connections.len(), 0);
        assert!(conn.get_connection(&id).is_none());
    }

    #[test]
    fn test_conn_state_multiple_connections() {
        let mut conn = ConnState::new();
        let ids: Vec<String> = (0..5).map(|_| conn.create_connection()).collect();

        assert_eq!(conn.connections.len(), 5);
        for id in &ids {
            assert!(conn.get_connection(id).is_some());
        }

        // 移除中间一个
        conn.remove_connection(&ids[2]);
        assert_eq!(conn.connections.len(), 4);
        assert!(conn.get_connection(&ids[2]).is_none());
        // 其他仍在
        assert!(conn.get_connection(&ids[0]).is_some());
        assert!(conn.get_connection(&ids[4]).is_some());
    }

    #[test]
    fn test_register_and_cancel_transfer() {
        let mut conn = ConnState::new();
        let token = conn.register_transfer(100);
        assert!(!token.is_cancelled());
        assert_eq!(conn.active_transfers.len(), 1);

        // 取消成功
        assert!(conn.cancel_transfer(100));
        assert!(token.is_cancelled());
        assert_eq!(conn.active_transfers.len(), 0);
    }

    #[test]
    fn test_cancel_nonexistent_transfer() {
        let mut conn = ConnState::new();
        assert!(!conn.cancel_transfer(999));
    }

    #[test]
    fn test_remove_transfer() {
        let mut conn = ConnState::new();
        let token = conn.register_transfer(200);
        assert_eq!(conn.active_transfers.len(), 1);

        conn.remove_transfer(200);
        assert_eq!(conn.active_transfers.len(), 0);
        // token 不应被取消（仅移除）
        assert!(!token.is_cancelled());
    }

    #[test]
    fn test_multiple_transfers() {
        let mut conn = ConnState::new();
        let t1 = conn.register_transfer(1);
        let t2 = conn.register_transfer(2);
        let t3 = conn.register_transfer(3);
        assert_eq!(conn.active_transfers.len(), 3);

        conn.cancel_transfer(2);
        assert!(!t1.is_cancelled());
        assert!(t2.is_cancelled());
        assert!(!t3.is_cancelled());
        assert_eq!(conn.active_transfers.len(), 2);
    }
}
