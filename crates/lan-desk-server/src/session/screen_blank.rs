//! 屏幕遮蔽功能：在被控端遮挡屏幕内容，防止旁人窥视

use tracing::{info, warn};

/// 屏幕遮蔽器
pub struct ScreenBlanker {
    active: bool,
}

impl ScreenBlanker {
    pub fn new() -> Self {
        Self { active: false }
    }

    pub fn set_blank(&mut self, enable: bool) -> anyhow::Result<()> {
        if enable == self.active {
            return Ok(());
        }

        if enable {
            self.enable_blank()?;
        } else {
            self.disable_blank()?;
        }
        self.active = enable;
        Ok(())
    }

    #[cfg(target_os = "windows")]
    fn enable_blank(&mut self) -> anyhow::Result<()> {
        // Windows: 关闭显示器（低功耗模式），简单可靠
        // SC_MONITORPOWER: 2 = 关闭
        #[link(name = "user32")]
        extern "system" {
            fn SendMessageW(hwnd: isize, msg: u32, wparam: usize, lparam: isize) -> isize;
        }
        const HWND_BROADCAST: isize = 0xFFFF;
        const WM_SYSCOMMAND: u32 = 0x0112;
        const SC_MONITORPOWER: usize = 0xF170;
        unsafe {
            SendMessageW(HWND_BROADCAST, WM_SYSCOMMAND, SC_MONITORPOWER, 2);
        }
        info!("屏幕遮蔽已启用 (Windows: 显示器关闭)");
        Ok(())
    }

    #[cfg(target_os = "windows")]
    fn disable_blank(&mut self) -> anyhow::Result<()> {
        // 唤醒显示器：SC_MONITORPOWER: -1 = 开启
        #[link(name = "user32")]
        extern "system" {
            fn SendMessageW(hwnd: isize, msg: u32, wparam: usize, lparam: isize) -> isize;
        }
        const HWND_BROADCAST: isize = 0xFFFF;
        const WM_SYSCOMMAND: u32 = 0x0112;
        const SC_MONITORPOWER: usize = 0xF170;
        unsafe {
            SendMessageW(HWND_BROADCAST, WM_SYSCOMMAND, SC_MONITORPOWER, -1);
        }
        info!("屏幕遮蔽已禁用 (Windows: 显示器唤醒)");
        Ok(())
    }

    #[cfg(target_os = "macos")]
    fn enable_blank(&mut self) -> anyhow::Result<()> {
        // macOS: 使用 pmset 关闭显示器
        std::process::Command::new("pmset")
            .args(["displaysleepnow"])
            .spawn()?;
        info!("屏幕遮蔽已启用 (macOS)");
        Ok(())
    }

    #[cfg(target_os = "macos")]
    fn disable_blank(&mut self) -> anyhow::Result<()> {
        // macOS: 使用 caffeinate 唤醒
        std::process::Command::new("caffeinate")
            .args(["-u", "-t", "1"])
            .spawn()?;
        info!("屏幕遮蔽已禁用 (macOS)");
        Ok(())
    }

    #[cfg(target_os = "linux")]
    fn enable_blank(&mut self) -> anyhow::Result<()> {
        std::process::Command::new("xset")
            .args(["dpms", "force", "off"])
            .spawn()?;
        info!("屏幕遮蔽已启用 (Linux)");
        Ok(())
    }

    #[cfg(target_os = "linux")]
    fn disable_blank(&mut self) -> anyhow::Result<()> {
        std::process::Command::new("xset")
            .args(["dpms", "force", "on"])
            .spawn()?;
        info!("屏幕遮蔽已禁用 (Linux)");
        Ok(())
    }

    #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
    fn enable_blank(&mut self) -> anyhow::Result<()> {
        anyhow::bail!("当前平台不支持屏幕遮蔽")
    }

    #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
    fn disable_blank(&mut self) -> anyhow::Result<()> {
        Ok(())
    }
}

impl Drop for ScreenBlanker {
    fn drop(&mut self) {
        if self.active {
            if let Err(e) = self.set_blank(false) {
                warn!("屏幕遮蔽器 Drop 时恢复显示失败: {}", e);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_screen_blanker_new_inactive() {
        let blanker = ScreenBlanker::new();
        assert!(!blanker.active);
    }

    #[test]
    fn test_screen_blanker_set_same_state_noop() {
        let mut blanker = ScreenBlanker::new();
        // 初始状态为 false，再次设置 false 应直接返回 Ok，不执行任何操作
        let result = blanker.set_blank(false);
        assert!(result.is_ok());
        assert!(!blanker.active);
    }
}
