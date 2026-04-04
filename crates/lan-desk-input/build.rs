fn main() {
    // macOS: 确保所有需要的框架被链接
    // CGEventCreateScrollEvent / CGEventPost → CoreGraphics
    // AXIsProcessTrusted → ApplicationServices
    // CFRelease → CoreFoundation
    #[cfg(target_os = "macos")]
    {
        println!("cargo:rustc-link-lib=framework=CoreGraphics");
        println!("cargo:rustc-link-lib=framework=ApplicationServices");
        println!("cargo:rustc-link-lib=framework=CoreFoundation");
    }
}
