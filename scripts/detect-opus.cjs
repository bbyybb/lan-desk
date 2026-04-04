#!/usr/bin/env node
/**
 * 自动检测 cmake 是否可用，决定是否启用 Opus 音频编码。
 *
 * 用法：node scripts/detect-opus.cjs
 * 输出到 stdout：需要追加到 tauri build 的额外参数（如 "--features opus"）
 *
 * 集成方式：
 *   - npm run build:release（自动检测）
 *   - FORCE_OPUS=1 npm run build:release（强制启用）
 *   - FORCE_OPUS=0 npm run build:release（强制禁用）
 */

const { execSync } = require("child_process");

const MIN_CMAKE_VERSION = [3, 10]; // 最低 cmake 版本要求

function detectCmake() {
  // 检查环境变量强制覆盖
  if (process.env.FORCE_OPUS === "1") {
    console.error("[detect-opus] FORCE_OPUS=1, 强制启用 Opus 编码");
    return true;
  }
  if (process.env.FORCE_OPUS === "0") {
    console.error("[detect-opus] FORCE_OPUS=0, 强制禁用 Opus 编码");
    return false;
  }

  try {
    const output = execSync("cmake --version", {
      encoding: "utf-8",
      stdio: ["pipe", "pipe", "pipe"],
    });

    // 解析版本号：cmake version 3.28.1
    const match = output.match(/cmake version (\d+)\.(\d+)/);
    if (!match) {
      console.error("[detect-opus] cmake 输出格式无法识别，禁用 Opus");
      return false;
    }

    const major = parseInt(match[1], 10);
    const minor = parseInt(match[2], 10);

    if (
      major > MIN_CMAKE_VERSION[0] ||
      (major === MIN_CMAKE_VERSION[0] && minor >= MIN_CMAKE_VERSION[1])
    ) {
      console.error(
        `[detect-opus] cmake ${major}.${minor} 已检测到，启用 Opus 编码`
      );
      return true;
    } else {
      console.error(
        `[detect-opus] cmake ${major}.${minor} 版本过低（需要 >= ${MIN_CMAKE_VERSION.join(".")}），禁用 Opus`
      );
      return false;
    }
  } catch {
    console.error("[detect-opus] cmake 未安装，禁用 Opus 编码（音频将使用 PCM 传输）");
    return false;
  }
}

const hasOpus = detectCmake();
// 输出给调用者使用的参数
if (hasOpus) {
  process.stdout.write("--features opus");
}
