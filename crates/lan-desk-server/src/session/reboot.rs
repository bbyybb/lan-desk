//! 远程重启功能

#[allow(unreachable_code)]
pub fn reboot_system() -> anyhow::Result<()> {
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("shutdown")
            .args(["/r", "/t", "10"])
            .spawn()?;
        return Ok(());
    }

    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("sudo")
            .args(["shutdown", "-r", "+1"])
            .spawn()?;
        return Ok(());
    }

    #[cfg(target_os = "linux")]
    {
        std::process::Command::new("sudo")
            .args(["shutdown", "-r", "+1"])
            .spawn()?;
        return Ok(());
    }

    anyhow::bail!("不支持的平台")
}
