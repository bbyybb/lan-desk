use tokio::sync::mpsc as tokio_mpsc;
use tracing::info;

use super::Session;

impl<S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static> Session<S> {
    /// 启动 PTY 子进程
    pub(super) fn start_pty(&mut self, cols: u16, rows: u16) -> anyhow::Result<()> {
        use portable_pty::{native_pty_system, CommandBuilder, PtySize};

        let pty_system = native_pty_system();
        let size = PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        };
        let pair = pty_system.openpty(size)?;

        // 选择 shell
        // Windows 下优先使用 PowerShell，默认 UTF-8 输出，避免 cmd.exe 的 GBK 乱码问题
        #[cfg(windows)]
        let shell = {
            // 检查可执行文件是否存在于 PATH 中
            fn which_exists(name: &str) -> bool {
                if let Ok(path) = std::env::var("PATH") {
                    for dir in path.split(';') {
                        if std::path::Path::new(dir).join(name).exists() {
                            return true;
                        }
                    }
                }
                false
            }
            // 优先 pwsh (PowerShell 7+)，其次 powershell.exe (Windows PowerShell 5.1)
            if which_exists("pwsh.exe") {
                "pwsh.exe".to_string()
            } else {
                "powershell.exe".to_string()
            }
        };
        #[cfg(not(windows))]
        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string());

        // Windows 下为 PowerShell 添加 UTF-8 编码初始化参数
        #[cfg(windows)]
        let mut cmd = {
            let mut c = CommandBuilder::new(&shell);
            // 使用 -NoExit -Command 在启动时设置控制台编码为 UTF-8，
            // 然后保持交互式会话，避免 GBK/其他非 UTF-8 编码导致的乱码问题
            if shell.contains("pwsh") || shell.contains("powershell") {
                c.arg("-NoExit");
                c.arg("-Command");
                c.arg("[Console]::OutputEncoding = [System.Text.Encoding]::UTF8; [Console]::InputEncoding = [System.Text.Encoding]::UTF8");
            }
            c
        };
        #[cfg(not(windows))]
        let mut cmd = CommandBuilder::new(&shell);

        cmd.cwd(dirs_next::home_dir().unwrap_or_else(|| std::path::PathBuf::from(".")));

        // Windows 下设置环境变量强制 UTF-8 编码
        #[cfg(windows)]
        {
            cmd.env("PYTHONIOENCODING", "utf-8");
            cmd.env("LANG", "en_US.UTF-8");
            // 保留 PSModulePath 让 PowerShell 正常加载模块
            cmd.env(
                "PSModulePath",
                std::env::var("PSModulePath").unwrap_or_default(),
            );
        }

        // 非 Windows 平台：确保 LANG 和 LC_ALL 设置为 UTF-8（如果未设置）
        #[cfg(not(windows))]
        {
            if std::env::var("LANG").unwrap_or_default().is_empty() {
                cmd.env("LANG", "en_US.UTF-8");
            }
            if std::env::var("LC_ALL").unwrap_or_default().is_empty() {
                cmd.env("LC_ALL", "en_US.UTF-8");
            }
        }

        let child = pair.slave.spawn_command(cmd)?;
        let writer = pair.master.take_writer()?;
        let mut reader = pair.master.try_clone_reader()?;

        // 独立线程读取 PTY 输出，转发到 tokio channel
        let (pty_tx, pty_rx) = tokio_mpsc::channel::<Vec<u8>>(64);
        std::thread::Builder::new()
            .name("pty-reader".to_string())
            .spawn(move || {
                use std::io::Read;
                let mut buf = [0u8; 4096];
                loop {
                    match reader.read(&mut buf) {
                        Ok(0) => break,
                        Ok(n) => {
                            if pty_tx.blocking_send(buf[..n].to_vec()).is_err() {
                                break;
                            }
                        }
                        Err(_) => break,
                    }
                }
            })?;

        self.pty_writer = Some(writer);
        self.pty_child = Some(child);
        self.pty_master = Some(pair.master);
        self.pty_output_rx = Some(pty_rx);
        info!("PTY 已启动: {}x{}", cols, rows);
        Ok(())
    }

    /// 关闭 PTY
    pub(super) fn close_pty(&mut self) {
        if let Some(mut child) = self.pty_child.take() {
            let _ = child.kill();
            let _ = child.wait(); // 回收子进程，避免僵尸进程
        }
        self.pty_writer = None;
        self.pty_master = None;
        self.pty_output_rx = None;
    }
}
