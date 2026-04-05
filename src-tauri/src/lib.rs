mod commands;
mod portable;
mod state;
mod ws_bridge;

#[cfg(feature = "desktop")]
use tauri::Manager;
#[cfg(feature = "desktop")]
use tauri::{
    menu::{MenuBuilder, MenuEvent, MenuItemBuilder},
    tray::TrayIconBuilder,
    AppHandle,
};
#[cfg(feature = "desktop")]
use tauri_plugin_autostart::MacosLauncher;
use tracing_subscriber::EnvFilter;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // 安装 rustls crypto provider
    rustls::crypto::ring::default_provider()
        .install_default()
        .ok();

    // 初始化日志
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let mut builder = tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_dialog::init());

    #[cfg(feature = "desktop")]
    {
        builder = builder.plugin(tauri_plugin_autostart::init(
            MacosLauncher::LaunchAgent,
            Some(vec![]),
        ));
    }

    builder = builder.manage(state::AppState::new());

    // 桌面端包含全部命令，移动端排除服务器控制和多显示器等桌面专属命令
    #[cfg(feature = "desktop")]
    {
        builder = builder.invoke_handler(tauri::generate_handler![
            commands::discover_peers,
            commands::connect_to_peer,
            commands::disconnect,
            commands::send_mouse_move,
            commands::send_mouse_button,
            commands::send_mouse_scroll,
            commands::send_key_event,
            commands::get_status,
            commands::start_server,
            commands::get_device_id,
            commands::get_pins,
            commands::refresh_pins,
            commands::set_unattended,
            commands::set_fixed_pins,
            commands::list_monitors,
            commands::switch_monitor,
            commands::send_file,
            commands::cancel_transfer,
            commands::stat_path,
            commands::set_bandwidth_limit,
            commands::apply_capture_settings,
            commands::wake_on_lan,
            commands::get_network_info,
            commands::start_shell,
            commands::send_shell_input,
            commands::resize_shell,
            commands::close_shell,
            commands::stop_server,
            commands::get_ws_port,
            commands::reconnect_to_peer,
            commands::set_clipboard_sync,
            commands::get_remote_monitors,
            commands::set_shell_enabled,
            commands::set_idle_timeout,
            commands::set_lock_on_disconnect,
            commands::list_trusted_hosts,
            commands::remove_trusted_host,
            commands::request_file_list,
            commands::download_remote_file,
            commands::download_remote_directory,
            commands::send_directory,
            commands::chat::send_chat_message,
            commands::special_keys::send_special_key,
            commands::screen_blank::toggle_screen_blank,
            commands::remote_control::remote_reboot,
            commands::remote_control::remote_lock_screen,
        ]);
    }
    #[cfg(not(feature = "desktop"))]
    {
        builder = builder.invoke_handler(tauri::generate_handler![
            // 连接和输入（移动端核心功能）
            commands::discover_peers,
            commands::connect_to_peer,
            commands::disconnect,
            commands::send_mouse_move,
            commands::send_mouse_button,
            commands::send_mouse_scroll,
            commands::send_key_event,
            commands::get_status,
            commands::get_device_id,
            commands::get_pins,
            commands::refresh_pins,
            commands::get_ws_port,
            commands::reconnect_to_peer,
            commands::get_remote_monitors,
            commands::get_network_info,
            commands::wake_on_lan,
            // 设置（移动端 stub）
            commands::set_unattended,
            commands::set_fixed_pins,
            commands::set_bandwidth_limit,
            commands::apply_capture_settings,
            commands::set_clipboard_sync,
            // 文件传输
            commands::send_file,
            commands::cancel_transfer,
            commands::stat_path,
            commands::request_file_list,
            commands::download_remote_file,
            commands::download_remote_directory,
            commands::send_directory,
            // TOFU 证书
            commands::list_trusted_hosts,
            commands::remove_trusted_host,
            // 聊天 + 特殊键
            commands::chat::send_chat_message,
            commands::special_keys::send_special_key,
            // 桌面端专属命令的移动端 stub（返回错误，避免前端调用时 command not found）
            commands::mobile_stubs::start_server,
            commands::mobile_stubs::stop_server,
            commands::mobile_stubs::set_shell_enabled,
            commands::mobile_stubs::set_idle_timeout,
            commands::mobile_stubs::set_lock_on_disconnect,
            commands::mobile_stubs::start_shell,
            commands::mobile_stubs::send_shell_input,
            commands::mobile_stubs::resize_shell,
            commands::mobile_stubs::close_shell,
            commands::mobile_stubs::list_monitors,
            commands::mobile_stubs::switch_monitor,
            commands::mobile_stubs::toggle_screen_blank,
            commands::mobile_stubs::remote_reboot,
            commands::mobile_stubs::remote_lock_screen,
        ]);
    }

    builder = builder
        .setup(|app| {
            // 初始化数据目录：便携模式用 exe 同级 data/，否则用系统默认
            let data_dir = portable::data_dir();
            std::fs::create_dir_all(&data_dir).ok();
            commands::init_tofu_data_dir(data_dir);

            // 完整性校验
            // 开发模式 / 源码分发：检查文件 SHA-256 哈希
            // 生产模式（exe）：文件已内嵌到二进制中无法被单独篡改，
            //                 前端 4 层 JS 检查保护署名和打赏信息
            #[cfg(debug_assertions)]
            {
                let check_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                    .parent()
                    .unwrap_or(std::path::Path::new("."))
                    .to_path_buf();
                if let Err(tampered) = lan_desk_protocol::integrity::check_sealed_files(&check_dir) {
                    eprintln!("\n[LAN-Desk] Protected files have been modified: {:?}", tampered);
                    eprintln!("[LAN-Desk] If you made legitimate changes to README.md,");
                    eprintln!("[LAN-Desk] run: python scripts/update_seal_hashes.py");
                    eprintln!("[LAN-Desk] Note: donation info in README must be preserved.\n");
                    std::process::exit(1);
                }
            }
            #[cfg(not(debug_assertions))]
            {
                // 生产模式：验证关键标记是否存在于编译产物中
                // （编译时嵌入的字符串常量，修改需要逆向二进制）
                let markers_ok = lan_desk_protocol::integrity::check_markers_in_text(
                    "LANDESK-bbloveyy-2026 bbyybb buymeacoffee.com/bbyybb sponsors/bbyybb \u{767d}\u{767d}LOVE\u{5c39}\u{5c39}"
                );
                if !markers_ok {
                    eprintln!("Integrity check failed. LAN-Desk by bbyybb.");
                    std::process::exit(1);
                }
            }

            // 系统托盘 — 根据系统语言选择中文或英文（仅桌面端）
            #[cfg(feature = "desktop")]
            {
                let is_chinese = sys_locale::get_locale()
                    .map(|l| l.to_lowercase().contains("zh"))
                    .unwrap_or(false);

                let portable_tag = if crate::portable::is_portable() { " [P]" } else { "" };
                let (show_label, status_label, quit_label, tooltip_text) = if is_chinese {
                    (
                        format!("打开 LAN-Desk{}", portable_tag),
                        if crate::portable::is_portable() { "便携模式 · 服务运行中" } else { "服务运行中" }.to_string(),
                        "退出".to_string(),
                        format!("LAN-Desk{} - 局域网远程桌面", portable_tag),
                    )
                } else {
                    (
                        format!("Open LAN-Desk{}", portable_tag),
                        if crate::portable::is_portable() { "Portable · Server Running" } else { "Server Running" }.to_string(),
                        "Quit".to_string(),
                        format!("LAN-Desk{} - LAN Remote Desktop", portable_tag),
                    )
                };

                let show = MenuItemBuilder::with_id("show", &show_label).build(app)?;
                let status = MenuItemBuilder::with_id("status", &status_label)
                    .enabled(false)
                    .build(app)?;
                let quit = MenuItemBuilder::with_id("quit", &quit_label).build(app)?;
                let menu = MenuBuilder::new(app)
                    .item(&show)
                    .item(&status)
                    .separator()
                    .item(&quit)
                    .build()?;

                let _tray = TrayIconBuilder::new()
                    .icon(app.default_window_icon().cloned().expect("应用图标缺失"))
                    .menu(&menu)
                    .tooltip(&tooltip_text)
                    .on_menu_event(move |app: &AppHandle, event: MenuEvent| {
                        match event.id().as_ref() {
                            "show" => {
                                if let Some(window) = app.get_webview_window("main") {
                                    let _ = window.show();
                                    let _ = window.set_focus();
                                }
                            }
                            "quit" => {
                                app.exit(0);
                            }
                            _ => {}
                        }
                    })
                    .build(app)?;

                // 根据系统语言设置窗口标题
                let window_title = if is_chinese {
                    format!("LAN-Desk{} - 局域网远程桌面", portable_tag)
                } else {
                    format!("LAN-Desk{} - LAN Remote Desktop", portable_tag)
                };
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.set_title(&window_title);
                }
            }

            Ok(())
        });

    // 桌面端：点击关闭按钮时隐藏到托盘而非退出
    #[cfg(feature = "desktop")]
    {
        builder = builder.on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                let _ = window.hide();
                api.prevent_close();
            }
        });
    }

    builder
        .run(tauri::generate_context!())
        .expect("启动 Tauri 应用失败");
}
