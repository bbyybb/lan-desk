<script setup lang="ts">
import { onMounted, reactive, ref } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { useI18n, translateError } from "../i18n";
import { useSettings, type Settings } from "../composables/useSettings";
import { useTheme } from "../composables/useTheme";
import { useToast } from "../composables/useToast";

const autoStartEnabled = ref(false);

const emit = defineEmits<{
  back: [];
}>();

const { t, locale, setLocale, previewLocale, restoreLocale } = useI18n();
const { settings, load, save, reload, DEFAULTS } = useSettings();
const toast = useToast();
const { applyTheme } = useTheme();

// 本地编辑副本：避免 v-model 直接修改共享 settings，点保存才写回
const draft = reactive<Settings>({ ...DEFAULTS });

function initDraft() {
  Object.assign(draft, settings.value);
}

function commitDraft() {
  Object.assign(settings.value, draft);
}

function onThemeChange(value: string) {
  draft.theme = value as "dark" | "light" | "system";
  // 主题即时预览（但不持久化，返回时会恢复）
  settings.value.theme = draft.theme;
  applyTheme();
}

function handleBack() {
  // 丢弃未保存的修改，恢复共享 settings 到上次持久化的状态
  reload();
  applyTheme();
  restoreLocale();
  emit("back");
}

// TOFU 已信任主机管理
interface TrustedHostInfo {
  host: string;
  fingerprint: string;
}
const trustedHosts = ref<TrustedHostInfo[]>([]);

async function loadTrustedHosts() {
  try {
    trustedHosts.value = await invoke<TrustedHostInfo[]>("list_trusted_hosts");
  } catch (e: unknown) {
    toast.error(t('settings.trusted_hosts_load_failed') + ': ' + translateError(e));
  }
}

async function removeTrustedHost(host: string) {
  try {
    await invoke("remove_trusted_host", { host });
    trustedHosts.value = trustedHosts.value.filter(h => h.host !== host);
  } catch (e: unknown) {
    toast.error(t('settings.trusted_hosts_remove_failed') + ': ' + translateError(e));
  }
}

async function saveSettings() {
  // 先校验所有输入，再执行任何后端调用，避免部分保存的不一致状态
  if (draft.fixedPassword) {
    if (draft.controlPassword.length < 6 || draft.controlPassword.length > 20) {
      toast.error(t('settings.password_length_error'));
      return;
    }
    if (draft.viewPassword.length < 6 || draft.viewPassword.length > 20) {
      toast.error(t('settings.password_length_error'));
      return;
    }
    if (draft.controlPassword === draft.viewPassword) {
      toast.error(t('settings.password_same_error'));
      return;
    }
  }
  if (!Number.isFinite(draft.port) || draft.port < 1024 || draft.port > 65535) {
    toast.error(t('settings.save_failed') + ': ' + t('settings.port_range_error'));
    return;
  }

  // 校验通过后，将 draft 写回共享 settings，持久化并推送到后端
  commitDraft();
  save();

  try {
    await invoke("set_bandwidth_limit", {
      mbps: settings.value.bandwidthLimit,
    });
    await invoke("set_clipboard_sync", {
      enabled: settings.value.clipboardSync,
    });
    await invoke("apply_capture_settings", {
      jpegQuality: settings.value.jpegQuality,
      maxFps: settings.value.maxFps,
      port: settings.value.port,
    });
    await invoke("set_unattended", {
      autoAccept: settings.value.autoAccept,
      fixedPassword: settings.value.fixedPassword,
    });
    if (settings.value.fixedPassword) {
      await invoke("set_fixed_pins", {
        controlPin: settings.value.controlPassword,
        viewPin: settings.value.viewPassword,
      });
      // 重启服务器以使新密码对后续连接生效
      try {
        await invoke("stop_server");
        await invoke("start_server");
      } catch (_) { /* ignored */ }
    }
    await invoke("set_shell_enabled", {
      enabled: settings.value.shellEnabled,
    });
    await invoke("set_idle_timeout", {
      minutes: settings.value.idleTimeoutMinutes,
    });
    await invoke("set_lock_on_disconnect", {
      enabled: settings.value.lockOnDisconnect,
    });
    // 自启动设置
    try {
      const { enable, disable } = await import("@tauri-apps/plugin-autostart");
      if (autoStartEnabled.value) {
        await enable();
      } else {
        await disable();
      }
    } catch (_) { /* ignored */ }
    // 持久化语言选择（预览阶段仅内存修改，此处写入 localStorage）
    setLocale(locale.value);
    emit("back");
  } catch (e: unknown) {
    toast.error(t('settings.save_failed') + ': ' + translateError(e));
  }
}

async function resetDefaults() {
  // 重置 draft 为默认值（不直接改共享 settings，等用户点保存）
  Object.assign(draft, { ...DEFAULTS });
}

onMounted(async () => {
  load();
  initDraft();
  loadTrustedHosts();
  try {
    const { isEnabled } = await import("@tauri-apps/plugin-autostart");
    autoStartEnabled.value = await isEnabled();
  } catch (_) { /* ignored */ }
});
</script>

<template>
  <div class="settings">
    <header class="header">
      <button
        class="btn-secondary"
        @click="handleBack"
      >
        {{ t('settings.back') }}
      </button>
      <h1>{{ t('settings.title') }}</h1>
      <button
        class="btn-primary"
        @click="saveSettings"
      >
        {{ t('settings.save') }}
      </button>
    </header>

    <div class="content">
      <div class="section">
        <h3>{{ t('settings.network') }}</h3>
        <div class="setting-row">
          <label>{{ t('settings.tcp_port') }}</label>
          <input
            v-model.number="draft.port"
            type="number"
            min="1024"
            max="65535"
          >
        </div>
      </div>

      <div class="section">
        <h3>{{ t('settings.quality') }}</h3>
        <div class="setting-row">
          <label>{{ t('settings.jpeg_quality') }} ({{ draft.jpegQuality }})</label>
          <input
            v-model.number="draft.jpegQuality"
            type="range"
            min="20"
            max="95"
          >
        </div>
        <div class="setting-row">
          <label>{{ t('settings.max_fps') }} ({{ draft.maxFps }} fps)</label>
          <input
            v-model.number="draft.maxFps"
            type="range"
            min="5"
            max="60"
          >
        </div>
        <div class="setting-row">
          <label>{{ t('settings.audio_quality') }}</label>
          <select v-model="draft.audioQuality">
            <option value="low">
              {{ t('settings.audio_low') }}
            </option>
            <option value="medium">
              {{ t('settings.audio_medium') }}
            </option>
            <option value="high">
              {{ t('settings.audio_high') }}
            </option>
          </select>
        </div>
        <div class="setting-hint">
          {{ t('settings.audio_quality_hint') }}
        </div>
      </div>

      <div class="section">
        <h3>{{ t('settings.connection') }}</h3>
        <div class="setting-row">
          <label>{{ t('settings.auto_reconnect') }}</label>
          <label class="toggle">
            <input
              v-model="draft.autoReconnect"
              type="checkbox"
            >
            <span class="slider" />
          </label>
        </div>
        <div
          v-if="draft.autoReconnect"
          class="setting-row"
        >
          <label>{{ t('settings.max_retry') }}</label>
          <input
            v-model.number="draft.maxReconnectAttempts"
            type="number"
            min="1"
            max="20"
          >
        </div>
      </div>

      <div class="section">
        <h3>{{ t('settings.general') }}</h3>
        <div class="setting-row">
          <label>{{ t('settings.bandwidth') }} ({{ draft.bandwidthLimit === 0 ? t('settings.bandwidth_unlimited') : draft.bandwidthLimit + ' Mbps' }})</label>
          <input
            v-model.number="draft.bandwidthLimit"
            type="range"
            min="0"
            max="100"
            step="5"
          >
        </div>
        <div class="setting-row">
          <label>{{ t('settings.clipboard_sync') }}</label>
          <label class="toggle">
            <input
              v-model="draft.clipboardSync"
              type="checkbox"
            >
            <span class="slider" />
          </label>
        </div>
        <div class="setting-hint">
          {{ t('settings.clipboard_sync_hint') }}
        </div>
        <div class="setting-row">
          <label>{{ t('settings.language') }}</label>
          <select
            :value="locale"
            @change="previewLocale(($event.target as HTMLSelectElement).value)"
          >
            <option value="zh">
              简体中文
            </option>
            <option value="en">
              English
            </option>
          </select>
        </div>
        <div class="setting-row">
          <label>{{ t('settings.theme') }}</label>
          <select
            :value="draft.theme"
            @change="onThemeChange(($event.target as HTMLSelectElement).value)"
          >
            <option value="dark">
              {{ t('settings.theme_dark') }}
            </option>
            <option value="light">
              {{ t('settings.theme_light') }}
            </option>
            <option value="system">
              {{ t('settings.theme_system') }}
            </option>
          </select>
        </div>
        <div class="setting-row">
          <label>{{ t('settings.autostart') }}</label>
          <label class="toggle">
            <input
              v-model="autoStartEnabled"
              type="checkbox"
            >
            <span class="slider" />
          </label>
        </div>
        <div class="setting-hint">
          {{ t('settings.autostart_hint') }}
        </div>
      </div>

      <div class="section">
        <h3>{{ t('settings.security') }}</h3>
        <div class="setting-row">
          <label>{{ t('settings.shell_enabled') }}</label>
          <label class="toggle">
            <input
              v-model="draft.shellEnabled"
              type="checkbox"
            >
            <span class="slider" />
          </label>
        </div>
        <div class="setting-hint">
          {{ t('settings.shell_enabled_hint') }}
        </div>
        <div class="setting-row">
          <label>{{ t('settings.idle_timeout') }} ({{ draft.idleTimeoutMinutes === 0 ? t('settings.idle_timeout_disabled') : draft.idleTimeoutMinutes + ' min' }})</label>
          <input
            v-model.number="draft.idleTimeoutMinutes"
            type="range"
            min="0"
            max="120"
            step="5"
          >
        </div>
        <div class="setting-hint">
          {{ t('settings.idle_timeout_hint') }}
        </div>
        <div class="setting-row">
          <label>{{ t('settings.lock_on_disconnect') }}</label>
          <label class="toggle">
            <input
              v-model="draft.lockOnDisconnect"
              type="checkbox"
            >
            <span class="slider" />
          </label>
        </div>
        <div class="setting-hint">
          {{ t('settings.lock_on_disconnect_hint') }}
        </div>
      </div>

      <div class="section">
        <h3>{{ t('settings.trusted_hosts') }}</h3>
        <div class="setting-hint">
          {{ t('settings.trusted_hosts_hint') }}
        </div>
        <div
          v-if="trustedHosts.length === 0"
          class="trusted-empty"
        >
          {{ t('settings.trusted_hosts_empty') }}
        </div>
        <div
          v-for="item in trustedHosts"
          :key="item.host"
          class="trusted-host-row"
        >
          <div class="trusted-host-info">
            <span class="trusted-host-name">{{ item.host }}</span>
            <span class="trusted-host-fp">{{ t('settings.trusted_hosts_fingerprint') }}: {{ item.fingerprint.substring(0, 16) }}...</span>
          </div>
          <button
            class="btn-remove"
            @click="removeTrustedHost(item.host)"
          >
            {{ t('settings.trusted_hosts_remove') }}
          </button>
        </div>
      </div>

      <div class="section">
        <h3>{{ t('settings.unattended') }}</h3>
        <div class="setting-row">
          <label>{{ t('settings.auto_accept') }}</label>
          <label class="toggle">
            <input
              v-model="draft.autoAccept"
              type="checkbox"
            >
            <span class="slider" />
          </label>
        </div>
        <div class="setting-hint">
          {{ t('settings.auto_accept_hint') }}
        </div>
        <div class="setting-row">
          <label>{{ t('settings.fixed_password') }}</label>
          <label class="toggle">
            <input
              v-model="draft.fixedPassword"
              type="checkbox"
            >
            <span class="slider" />
          </label>
        </div>
        <div class="setting-hint">
          {{ t('settings.fixed_password_hint') }}
        </div>
        <template v-if="draft.fixedPassword">
          <div class="setting-row">
            <label>{{ t('settings.control_password') }}</label>
            <input
              v-model="draft.controlPassword"
              type="password"
              minlength="6"
              maxlength="20"
              :placeholder="t('settings.password_placeholder')"
            >
          </div>
          <div class="setting-row">
            <label>{{ t('settings.view_password') }}</label>
            <input
              v-model="draft.viewPassword"
              type="password"
              minlength="6"
              maxlength="20"
              :placeholder="t('settings.password_placeholder')"
            >
          </div>
        </template>
      </div>

      <div class="section">
        <button
          class="btn-secondary"
          @click="resetDefaults"
        >
          {{ t('settings.reset') }}
        </button>
      </div>
    </div>
  </div>
</template>

<style scoped>
.settings {
  height: 100vh;
  display: flex;
  flex-direction: column;
  padding: 20px;
}

.header {
  display: flex;
  justify-content: space-between;
  align-items: center;
  margin-bottom: 24px;
}

.header h1 {
  font-size: 20px;
  color: var(--text-primary);
}

.content {
  flex: 1;
  overflow-y: auto;
  padding-bottom: 32px;
}

.section {
  background: var(--bg-secondary);
  border-radius: 10px;
  padding: 16px;
  margin-bottom: 16px;
}

.section h3 {
  font-size: 13px;
  color: var(--text-muted);
  text-transform: uppercase;
  letter-spacing: 1px;
  margin-bottom: 12px;
}

.setting-row {
  display: flex;
  justify-content: space-between;
  align-items: center;
  padding: 8px 0;
  border-bottom: 1px solid var(--border-color);
}

.setting-row:last-child {
  border-bottom: none;
}

.setting-row label {
  font-size: 14px;
  color: var(--text-secondary);
}

.setting-hint {
  font-size: 11px;
  color: var(--text-dim);
  padding: 0 0 8px 0;
}

.setting-row input[type="number"] {
  width: 80px;
  padding: 6px 10px;
  border: 1px solid var(--border-color);
  border-radius: 6px;
  background: var(--bg-input);
  color: var(--text-primary);
  font-size: 14px;
  text-align: center;
  outline: none;
}

.setting-row input[type="range"] {
  width: 160px;
  accent-color: var(--accent);
}

.setting-row select {
  padding: 6px 10px;
  border: 1px solid var(--border-color);
  border-radius: 6px;
  background: var(--bg-input);
  color: var(--text-primary);
  font-size: 14px;
  outline: none;
}

/* Toggle switch */
.toggle {
  position: relative;
  width: 44px;
  height: 24px;
}
.toggle input {
  opacity: 0;
  width: 0;
  height: 0;
}
.slider {
  position: absolute;
  cursor: pointer;
  top: 0; left: 0; right: 0; bottom: 0;
  background: var(--border-color);
  border-radius: 24px;
  transition: 0.3s;
}
.slider::before {
  content: "";
  position: absolute;
  height: 18px;
  width: 18px;
  left: 3px;
  bottom: 3px;
  background: white;
  border-radius: 50%;
  transition: 0.3s;
}
.toggle input:checked + .slider {
  background: var(--accent);
}
.toggle input:checked + .slider::before {
  transform: translateX(20px);
}

/* Trusted hosts */
.trusted-empty {
  font-size: 13px;
  color: var(--text-dim);
  padding: 12px 0;
  text-align: center;
}

.trusted-host-row {
  display: flex;
  justify-content: space-between;
  align-items: center;
  padding: 8px 0;
  border-bottom: 1px solid var(--border-color);
}

.trusted-host-row:last-child {
  border-bottom: none;
}

.trusted-host-info {
  display: flex;
  flex-direction: column;
  gap: 2px;
  min-width: 0;
  flex: 1;
}

.trusted-host-name {
  font-size: 13px;
  color: var(--text-secondary);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.trusted-host-fp {
  font-size: 11px;
  color: var(--text-dim);
  font-family: monospace;
}

.btn-remove {
  flex-shrink: 0;
  margin-left: 12px;
  padding: 4px 10px;
  font-size: 12px;
  color: #ff6b6b;
  background: transparent;
  border: 1px solid #ff6b6b;
  border-radius: 4px;
  cursor: pointer;
  transition: 0.2s;
}

.btn-remove:hover {
  background: rgba(255, 107, 107, 0.15);
}
</style>
