**中文** | [English](#code-signing-guide-english)

# 代码签名配置指南

本指南说明如何为 CI 自动构建配置各平台的代码签名，使 Release 产物经过官方签名认证。

> 未配置签名时，桌面端（Windows/macOS/Linux）和 Android 仍可正常构建和使用，iOS 构建需要签名才能成功。

---

## iOS 签名配置（必需）

iOS 构建**必须**有 Apple 开发者签名，否则无法生成 IPA。

### 前置条件

- [Apple Developer Program](https://developer.apple.com/programs/) 会员（$99/年）

### 步骤

**1. 导出签名证书**

在 macOS 的「钥匙串访问」中：
1. 打开 Apple Developer 网站 → Certificates → 创建 iOS Distribution 证书
2. 下载 `.cer` 文件并双击导入钥匙串
3. 在钥匙串中右键该证书 → 导出 → 保存为 `.p12` 文件（设置一个密码）
4. 将 `.p12` 文件转为 base64：
   ```bash
   base64 -i certificate.p12 | tr -d '\n' > cert_base64.txt
   ```

**2. 配置 GitHub Secrets**

在仓库 Settings → Secrets and variables → Actions 中添加：

| Secret 名称 | 值 | 说明 |
|-------------|-----|------|
| `APPLE_CERTIFICATE` | `cert_base64.txt` 的内容 | 签名证书 (base64) |
| `APPLE_CERTIFICATE_PASSWORD` | 导出 .p12 时设置的密码 | 证书密码 |
| `APPLE_ID` | 你的 Apple ID 邮箱 | 用于 Notarize 公证 |
| `APPLE_TEAM_ID` | 开发者团队 ID（10 位字母数字） | Apple Developer 后台可查 |
| `APPLE_PASSWORD` | App 专用密码 | 在 [appleid.apple.com](https://appleid.apple.com) → 安全 → App 专用密码 生成 |

**3. 获取 Team ID**

登录 [Apple Developer](https://developer.apple.com/account) → Membership → Team ID。

**4. 生成 App 专用密码**

1. 打开 [appleid.apple.com](https://appleid.apple.com)
2. 登录 → 安全 → App 专用密码 → 生成密码
3. 将生成的密码填入 `APPLE_PASSWORD` Secret

配置完成后，推送 `v*` tag 即可自动构建签名后的 iOS IPA。

---

## macOS 签名配置（可选）

未签名的 macOS DMG 可以使用，但用户打开时会看到「无法验证开发者」提示。

配置方式与 iOS 相同的 5 个 Secrets（共用同一套 Apple 开发者账号）。签名 + Notarize 公证后，用户可以直接打开无警告。

---

## Windows 签名配置（可选）

未签名的 Windows 安装包可以使用，但用户安装时会看到 SmartScreen 警告。

### 步骤

**1. 获取代码签名证书**

从受信任的 CA 购买代码签名证书（如 DigiCert、Sectigo、GlobalSign），或使用自签名证书（仅供测试）。

**2. 配置 GitHub Secrets**

| Secret 名称 | 值 | 说明 |
|-------------|-----|------|
| `WINDOWS_CERT_BASE64` | `.pfx` 文件的 base64 编码 | 签名证书 |
| `WINDOWS_CERT_PASSWORD` | 证书密码 | — |

转换命令：
```bash
base64 -i certificate.pfx | tr -d '\n' > cert_base64.txt
```

---

## Android 签名配置（可选）

未签名的 APK 可以直接安装使用（需开启「安装未知应用」），但无法上架 Google Play。

### 步骤

**1. 生成 Keystore**

```bash
keytool -genkey -v \
  -keystore lan-desk.keystore \
  -alias lan-desk \
  -keyalg RSA \
  -keysize 2048 \
  -validity 10000 \
  -storepass YOUR_STORE_PASSWORD \
  -keypass YOUR_KEY_PASSWORD \
  -dname "CN=LAN-Desk, O=bbyybb, L=City, ST=State, C=CN"
```

> 请妥善保管 keystore 文件和密码，丢失后无法更新已发布的 APK。

**2. 转换为 base64**

```bash
base64 -i lan-desk.keystore | tr -d '\n' > keystore_base64.txt
```

**3. 配置 GitHub Secrets**

| Secret 名称 | 值 | 说明 |
|-------------|-----|------|
| `ANDROID_KEYSTORE_BASE64` | `keystore_base64.txt` 的内容 | Keystore 文件 (base64) |
| `ANDROID_KEYSTORE_PASSWORD` | 上面设置的 `YOUR_STORE_PASSWORD` | Keystore 密码 |
| `ANDROID_KEY_ALIAS` | `lan-desk` | 密钥别名 |
| `ANDROID_KEY_PASSWORD` | 上面设置的 `YOUR_KEY_PASSWORD` | 密钥密码 |

**GitHub Secrets 配置步骤：**

1. 打开你的 GitHub 仓库页面
2. 点击 **Settings**（顶部标签栏）
3. 左侧菜单 → **Secrets and variables** → **Actions**
4. 点击 **New repository secret**
5. 依次添加上面 4 个 Secret（每次填 Name 和 Value，点击 Add secret）

配置完成后，下次推送 `v*` tag 时 CI 会自动签名 APK。未配置 Secrets 时 CI 仍会正常构建，只是产出未签名的 APK。

**4. 手动签名已有的 APK（无需重新构建）**

如果你已经有一个 unsigned APK，可以用命令行直接签名：

```bash
# 签名
apksigner sign \
  --ks lan-desk.keystore \
  --ks-key-alias lan-desk \
  --ks-pass pass:YOUR_STORE_PASSWORD \
  --key-pass pass:YOUR_KEY_PASSWORD \
  --out lan-desk-signed.apk \
  app-universal-release-unsigned.apk

# 验证签名
apksigner verify --verbose lan-desk-signed.apk
```

`apksigner` 工具在 Android SDK Build Tools 中：`$ANDROID_HOME/build-tools/<version>/apksigner`

> Windows 用户如果没有 Android SDK，也可以用 [uber-apk-signer](https://github.com/nicehash/uber-apk-signer) 替代：
> ```bash
> java -jar uber-apk-signer.jar --apks app-universal-release-unsigned.apk --ks lan-desk.keystore --ksAlias lan-desk --ksPass YOUR_STORE_PASSWORD --ksKeyPass YOUR_KEY_PASSWORD
> ```

---

## Tauri 更新签名（可选）

如果需要 Tauri 内置的应用更新功能：

| Secret 名称 | 值 | 说明 |
|-------------|-----|------|
| `TAURI_SIGNING_PRIVATE_KEY` | Tauri CLI 生成的私钥 | `npx tauri signer generate` |

---

## 验证签名配置

配置 Secrets 后无需修改任何代码，推送 `v*` tag 即可触发自动签名：

```bash
git tag v1.0.1
git push origin v1.0.1
```

在 GitHub Actions → Release workflow 中查看各平台构建是否成功。

---
---

<a id="code-signing-guide-english"></a>

# Code Signing Configuration Guide

This guide explains how to configure code signing for CI automated builds across all platforms.

> Without signing, desktop builds (Windows/macOS/Linux) and Android still work normally. iOS builds require signing to succeed.

---

## iOS Signing (Required)

iOS builds **require** Apple Developer signing to generate an IPA.

### Prerequisites

- [Apple Developer Program](https://developer.apple.com/programs/) membership ($99/year)

### Steps

**1. Export Signing Certificate**

In macOS Keychain Access:
1. Go to Apple Developer website → Certificates → Create an iOS Distribution certificate
2. Download the `.cer` file and double-click to import into Keychain
3. Right-click the certificate in Keychain → Export → Save as `.p12` (set a password)
4. Convert to base64:
   ```bash
   base64 -i certificate.p12 | tr -d '\n' > cert_base64.txt
   ```

**2. Configure GitHub Secrets**

In repo Settings → Secrets and variables → Actions, add:

| Secret Name | Value | Description |
|------------|-------|-------------|
| `APPLE_CERTIFICATE` | Contents of `cert_base64.txt` | Signing certificate (base64) |
| `APPLE_CERTIFICATE_PASSWORD` | Password set when exporting .p12 | Certificate password |
| `APPLE_ID` | Your Apple ID email | For Notarization |
| `APPLE_TEAM_ID` | Developer Team ID (10-char alphanumeric) | Found in Apple Developer dashboard |
| `APPLE_PASSWORD` | App-specific password | Generate at [appleid.apple.com](https://appleid.apple.com) → Security → App-Specific Passwords |

**3. Find Your Team ID**

Log in to [Apple Developer](https://developer.apple.com/account) → Membership → Team ID.

**4. Generate App-Specific Password**

1. Go to [appleid.apple.com](https://appleid.apple.com)
2. Sign in → Security → App-Specific Passwords → Generate Password
3. Enter the generated password as the `APPLE_PASSWORD` Secret

Once configured, pushing a `v*` tag will automatically build a signed iOS IPA.

---

## macOS Signing (Optional)

Unsigned macOS DMGs work but users see a "cannot verify developer" warning on first launch.

Uses the same 5 Secrets as iOS (shared Apple Developer account). After signing + notarization, users can open the app without warnings.

---

## Windows Signing (Optional)

Unsigned Windows installers work but trigger SmartScreen warnings during installation.

### Steps

**1. Obtain a Code Signing Certificate**

Purchase from a trusted CA (DigiCert, Sectigo, GlobalSign) or use a self-signed certificate (testing only).

**2. Configure GitHub Secrets**

| Secret Name | Value | Description |
|------------|-------|-------------|
| `WINDOWS_CERT_BASE64` | Base64-encoded `.pfx` file | Signing certificate |
| `WINDOWS_CERT_PASSWORD` | Certificate password | — |

Convert command:
```bash
base64 -i certificate.pfx | tr -d '\n' > cert_base64.txt
```

---

## Android Signing (Optional)

Unsigned APKs can be installed directly (requires "Install unknown apps" permission) but cannot be published to Google Play.

### Steps

**1. Generate Keystore**

```bash
keytool -genkey -v \
  -keystore lan-desk.keystore \
  -alias lan-desk \
  -keyalg RSA \
  -keysize 2048 \
  -validity 10000 \
  -storepass YOUR_STORE_PASSWORD \
  -keypass YOUR_KEY_PASSWORD \
  -dname "CN=LAN-Desk, O=bbyybb, L=City, ST=State, C=CN"
```

> Keep the keystore file and passwords safe — you cannot update a published APK if you lose them.

**2. Convert to base64**

```bash
base64 -i lan-desk.keystore | tr -d '\n' > keystore_base64.txt
```

**3. Configure GitHub Secrets**

| Secret Name | Value | Description |
|------------|-------|-------------|
| `ANDROID_KEYSTORE_BASE64` | Contents of `keystore_base64.txt` | Keystore file (base64) |
| `ANDROID_KEYSTORE_PASSWORD` | `YOUR_STORE_PASSWORD` from step 1 | Keystore password |
| `ANDROID_KEY_ALIAS` | `lan-desk` | Key alias |
| `ANDROID_KEY_PASSWORD` | `YOUR_KEY_PASSWORD` from step 1 | Key password |

**How to add GitHub Secrets:**

1. Open your GitHub repository page
2. Click **Settings** (top tab bar)
3. Left sidebar → **Secrets and variables** → **Actions**
4. Click **New repository secret**
5. Add each of the 4 Secrets above (enter Name and Value, click Add secret)

Once configured, the next `v*` tag push will automatically sign the APK. Without Secrets, CI still builds normally but produces an unsigned APK.

**4. Sign an existing APK manually (no rebuild needed)**

If you already have an unsigned APK:

```bash
# Sign
apksigner sign \
  --ks lan-desk.keystore \
  --ks-key-alias lan-desk \
  --ks-pass pass:YOUR_STORE_PASSWORD \
  --key-pass pass:YOUR_KEY_PASSWORD \
  --out lan-desk-signed.apk \
  app-universal-release-unsigned.apk

# Verify
apksigner verify --verbose lan-desk-signed.apk
```

`apksigner` is in Android SDK Build Tools: `$ANDROID_HOME/build-tools/<version>/apksigner`

> Windows users without Android SDK can use [uber-apk-signer](https://github.com/nicehash/uber-apk-signer) instead:
> ```bash
> java -jar uber-apk-signer.jar --apks app-universal-release-unsigned.apk --ks lan-desk.keystore --ksAlias lan-desk --ksPass YOUR_STORE_PASSWORD --ksKeyPass YOUR_KEY_PASSWORD
> ```

---

## Tauri Update Signing (Optional)

For Tauri's built-in app update feature:

| Secret Name | Value | Description |
|------------|-------|-------------|
| `TAURI_SIGNING_PRIVATE_KEY` | Private key from Tauri CLI | `npx tauri signer generate` |

---

## Verify Signing Configuration

After configuring Secrets, no code changes needed. Push a `v*` tag to trigger automatic signing:

```bash
git tag v1.0.1
git push origin v1.0.1
```

Check GitHub Actions → Release workflow for build results on each platform.
