**中文** | [English](#contributing-english)

# 贡献指南

感谢你对 LAN-Desk 的关注！欢迎提交 Issue 和 Pull Request。

## 如何贡献

### 报告 Bug

1. 先搜索 [已有 Issue](https://github.com/bbyybb/lan-desk/issues) 确认是否已被报告
2. 使用 [Bug 报告模板](https://github.com/bbyybb/lan-desk/issues/new?template=bug_report.md) 提交
3. 尽量提供完整的环境信息和复现步骤

### 提出功能建议

1. 先搜索已有 Issue 确认是否已被建议
2. 使用 [功能建议模板](https://github.com/bbyybb/lan-desk/issues/new?template=feature_request.md) 提交
3. 描述清楚使用场景和期望行为

### 提交代码

1. Fork 本仓库
2. 创建特性分支：`git checkout -b feature/your-feature`
3. 提交改动：`git commit -m "Add: your feature description"`
4. 推送分支：`git push origin feature/your-feature`
5. 创建 Pull Request

### 代码规范

- **编码**：所有文件使用 UTF-8 编码
- **Rust**：遵循 `cargo fmt` 和 `cargo clippy` 规范
- **前端**：Vue 3 Composition API + TypeScript
- **兼容性**：确保代码在 Windows、macOS 和 Linux 上都能编译
- **双语**：如果修改了用户可见的文本，请同时更新 `src/i18n/zh.json` 和 `en.json`
- **测试**：新增功能请附带单元测试
- **CI 检查**：CI 会运行 `cargo test`（Rust 后端，160 个测试）和 Vitest（前端，207 个测试），以及 ESLint 检查。建议提交前先本地执行 `cargo test`、`npm run test` 和 `npm run lint` 确保通过

### 重要说明

- **作者署名和打赏信息**受完整性保护，请勿修改
- 如果你的改动涉及 `README.md` 或 `docs/` 下的收款码图片，请联系维护者更新完整性哈希（需运行内部工具 `update_seal_hashes.py`）
- 其他所有功能代码、UI、配置均可自由修改

## 开发环境

```bash
# 前置依赖
# - Rust (stable): https://rustup.rs/
# - Node.js (v18+): https://nodejs.org/
# - Linux: sudo apt-get install libx11-dev libxext-dev libxtst-dev libxrandr-dev
#          libxcb-shm0-dev libxcb-randr0-dev libxcb1-dev libasound2-dev
#          libwebkit2gtk-4.1-dev libgtk-3-dev libayatana-appindicator3-dev librsvg2-dev
# - Linux (PipeWire 屏幕捕获，推荐): sudo apt-get install libpipewire-0.3-dev
#   （编译时需要，运行时需要 libpipewire-0.3；feature gate pipewire-capture 默认启用）
# - Linux (可选 VAAPI): sudo apt-get install libva-dev
#   （VAAPI 通过 libloading 运行时加载 libva.so，编译时不需要，仅开发调试时有用）
# - Linux (可选 Wayland 运行时依赖): sudo apt-get install grim ydotool dotool wlr-randr
# - macOS: Xcode Command Line Tools (xcode-select --install)
#   已知: core-graphics 0.24 移除了部分 Rust 封装，项目已通过直接调用 C API 解决，无需额外操作

# 克隆仓库
git clone https://github.com/bbyybb/lan-desk.git
cd lan-desk

# 安装前端依赖
npm install

# 开发模式运行
npm run tauri dev

# 运行 Rust 测试
cargo test

# 运行前端单元测试
npm run test

# 前端代码检查
npm run lint

# 代码格式化
cargo fmt --all
```

> **Wayland 原生支持（可选）**：如需测试原生 Wayland 功能，PipeWire/Portal 捕获需安装 `libpipewire-0.3-dev`（编译）和 `libpipewire-0.3`（运行时），feature gate `pipewire-capture` 默认启用。外部工具降级方案需安装 `grim`（屏幕截图）、`ydotool` 或 `dotool`（输入注入）。VAAPI GPU 编码通过 `libloading` 运行时动态加载 `libva.so`，编译时不需要额外依赖。

## 许可证

提交贡献即表示你同意你的代码以 [MIT 许可证](LICENSE) 发布。

---

<a id="contributing-english"></a>

# Contributing (English)

Thank you for your interest in LAN-Desk! Issues and Pull Requests are welcome.

## How to Contribute

### Report Bugs

1. Search [existing Issues](https://github.com/bbyybb/lan-desk/issues) first
2. Use the [Bug Report template](https://github.com/bbyybb/lan-desk/issues/new?template=bug_report.md)
3. Provide complete environment information and reproduction steps

### Suggest Features

1. Search existing Issues first
2. Use the [Feature Request template](https://github.com/bbyybb/lan-desk/issues/new?template=feature_request.md)
3. Clearly describe the use case and expected behavior

### Submit Code

1. Fork this repository
2. Create a feature branch: `git checkout -b feature/your-feature`
3. Commit changes: `git commit -m "Add: your feature description"`
4. Push the branch: `git push origin feature/your-feature`
5. Create a Pull Request

### Code Guidelines

- **Encoding**: All files use UTF-8
- **Rust**: Follow `cargo fmt` and `cargo clippy` standards
- **Frontend**: Vue 3 Composition API + TypeScript
- **Compatibility**: Ensure code compiles on Windows, macOS, and Linux
- **Bilingual**: If modifying user-visible text, update both `src/i18n/zh.json` and `en.json`
- **Tests**: Include unit tests for new features
- **CI Linting**: CI runs `cargo test` (Rust backend, 160 tests) and Vitest (frontend, 207 tests), plus ESLint checks; please run `cargo test`, `npm run test`, and `npm run lint` locally before submitting

### Important Notes

- **Author attribution and donation info** are integrity-protected — do not modify
- If your changes involve `README.md` or donation QR code images under `docs/`, please contact the maintainer to update integrity hashes (requires internal tool `update_seal_hashes.py`)
- All other feature code, UI, and configuration can be freely modified

## Development Setup

```bash
# Prerequisites: Rust (stable), Node.js (v18+)
# Linux: sudo apt-get install libx11-dev libxext-dev libxtst-dev libxrandr-dev \
#   libxcb-shm0-dev libxcb-randr0-dev libxcb1-dev libasound2-dev \
#   libwebkit2gtk-4.1-dev libgtk-3-dev libayatana-appindicator3-dev librsvg2-dev
# Linux (PipeWire capture, recommended): sudo apt-get install libpipewire-0.3-dev
#   (needed at compile time; libpipewire-0.3 needed at runtime; feature gate pipewire-capture enabled by default)
# Linux (optional VAAPI): sudo apt-get install libva-dev
#   (VAAPI loads libva.so at runtime via libloading; not needed for compilation, only useful for dev/debug)
# Linux (optional Wayland runtime deps): sudo apt-get install grim ydotool dotool wlr-randr

git clone https://github.com/bbyybb/lan-desk.git
cd lan-desk
npm install
npm run tauri dev
cargo test
npm run test      # Vitest frontend unit tests
npm run lint      # ESLint frontend linting
cargo fmt --all
```

> **Native Wayland Support (optional)**: To test native Wayland features, PipeWire/Portal capture requires `libpipewire-0.3-dev` (compile time) and `libpipewire-0.3` (runtime); the `pipewire-capture` feature gate is enabled by default. External tool fallback requires `grim` (screen capture) and `ydotool` or `dotool` (input injection). VAAPI GPU encoding dynamically loads `libva.so` at runtime via `libloading` — no extra build-time dependencies required.

## License

By contributing, you agree that your contributions will be licensed under the [MIT License](LICENSE).
