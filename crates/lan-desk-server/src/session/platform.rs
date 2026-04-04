/// 跨平台锁屏
pub fn lock_screen() -> anyhow::Result<()> {
    #[cfg(target_os = "windows")]
    {
        #[link(name = "user32")]
        extern "system" {
            fn LockWorkStation() -> i32;
        }
        unsafe {
            LockWorkStation();
        }
        return Ok(());
    }
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new(
            "/System/Library/CoreServices/Menu Extras/User.menu/Contents/Resources/CGSession",
        )
        .arg("-suspend")
        .spawn()
        .map_err(|e| anyhow::anyhow!("macOS 锁屏失败: {}", e))?;
        return Ok(());
    }
    #[cfg(target_os = "linux")]
    {
        if std::process::Command::new("loginctl")
            .arg("lock-session")
            .status()
            .is_ok()
        {
            return Ok(());
        }
        if std::process::Command::new("xdg-screensaver")
            .arg("lock")
            .status()
            .is_ok()
        {
            return Ok(());
        }
        anyhow::bail!("Linux 锁屏失败：loginctl 和 xdg-screensaver 均不可用");
    }
    #[allow(unreachable_code)]
    {
        anyhow::bail!("当前平台不支持锁屏")
    }
}
