#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // Windows 终端设置 UTF-8 代码页，防止中文日志乱码
    #[cfg(target_os = "windows")]
    unsafe {
        #[link(name = "kernel32")]
        extern "system" {
            fn SetConsoleOutputCP(code_page: u32) -> i32;
        }
        SetConsoleOutputCP(65001); // UTF-8
    }

    lan_desk_lib::run()
}
