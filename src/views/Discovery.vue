<script setup lang="ts">
import { ref, onMounted } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { useI18n, translateError } from "../i18n";
import { useSettings } from "../composables/useSettings";
import { useToast } from "../composables/useToast";

const { t } = useI18n();
const { settings, load } = useSettings();
const toast = useToast();

interface IncomingSession {
  hostname: string;
  addr: string;
  role: string;
}
interface OutgoingTab {
  id: string;
  addr: string;
  title: string;
  connectionId: string;
}

const props = defineProps<{
  incomingSessions?: IncomingSession[];
  outgoingTabs?: OutgoingTab[];
}>();

const emit = defineEmits<{
  connect: [addr: string];
  "open-settings": [];
  "switch-to-tab": [tabId: string];
}>();

interface Peer {
  addr: string;
  hostname: string;
  os: string;
  device_id: string;
}

interface HistoryItem {
  addr: string;
  hostname: string;
  mac?: string;
  lastConnected: number;
  alias?: string;
  savedPin?: string;
}

const wolMac = ref("");

const peers = ref<Peer[]>([]);
const manualAddr = ref("");
const pinInput = ref("");
const selectedRole = ref<"controller" | "viewer">("controller");
const localControlPin = ref("------");
const localViewPin = ref("------");
const deviceId = ref("---");
const isScanning = ref(false);
const serverStarted = ref(false);
const isConnecting = ref(false);
const connectError = ref("");
const rememberPin = ref(false);
let tofuRetryCount = 0;
const history = ref<HistoryItem[]>([]);

interface NetworkAddr {
  ip: string;
  name: string;
  net_type: string;
}
const networkAddrs = ref<NetworkAddr[]>([]);

function loadHistory() {
  try {
    const saved = localStorage.getItem("lan-desk-history");
    if (saved) history.value = JSON.parse(saved);
  } catch (_) { /* ignored */ }
}

function saveHistory(addr: string, hostname: string, pin?: string) {
  // 移除重复项
  history.value = history.value.filter((h) => h.addr !== addr);
  // 添加到头部，可选保存密码（base64 编码）
  const savedPin = pin && rememberPin.value ? btoa(pin) : undefined;
  history.value.unshift({ addr, hostname: hostname || addr, lastConnected: Date.now(), savedPin });
  // 最多保留 10 条
  if (history.value.length > 10) history.value = history.value.slice(0, 10);
  localStorage.setItem("lan-desk-history", JSON.stringify(history.value));
}

function removeHistory(addr: string) {
  history.value = history.value.filter((h) => h.addr !== addr);
  localStorage.setItem("lan-desk-history", JSON.stringify(history.value));
}

const editingAlias = ref<string | null>(null);
const aliasInput = ref("");

function startEditAlias(item: HistoryItem) {
  editingAlias.value = item.addr;
  aliasInput.value = item.alias || "";
}

function saveAlias(addr: string) {
  const item = history.value.find((h) => h.addr === addr);
  if (item) {
    item.alias = aliasInput.value.trim() || undefined;
    localStorage.setItem("lan-desk-history", JSON.stringify(history.value));
  }
  editingAlias.value = null;
}

function quickConnect(item: HistoryItem) {
  manualAddr.value = item.addr;
  // 如果有保存的密码，自动填充
  if (item.savedPin) {
    try { pinInput.value = atob(item.savedPin); } catch (_) { /* ignored */ }
    rememberPin.value = true;
  }
}

function copyConnectInfo() {
  const port = settings.value.port || 25605;
  const id = deviceId.value !== "---" ? deviceId.value : "";
  const info = `LAN-Desk${id ? ` | ID: ${id}` : ""} | Port: ${port}`;
  navigator.clipboard.writeText(info).then(() => {
    toast.success(t("discovery.connect_info_copied"));
  }).catch(() => {});
}

function exportHistory() {
  const header = "Address,Hostname,Alias,Last Connected\n";
  const rows = history.value.map(h =>
    `"${h.addr}","${h.alias || h.hostname || ""}","${h.alias || ""}","${new Date(h.lastConnected).toLocaleString()}"`
  ).join("\n");
  const blob = new Blob([header + rows], { type: "text/csv;charset=utf-8" });
  const url = URL.createObjectURL(blob);
  const a = document.createElement("a");
  a.href = url;
  a.download = `lan-desk-history-${Date.now()}.csv`;
  a.click();
  URL.revokeObjectURL(url);
}

function copyDeviceId() {
  navigator.clipboard.writeText(deviceId.value).then(() => {
    toast.success(t("discovery.id_copied"));
  }).catch(() => {});
}

async function startServer() {
  try {
    await invoke("start_server");
    serverStarted.value = true;
  } catch (e: unknown) {
    toast.error(t('discovery.server_start_failed') + ': ' + translateError(e));
  }
  // 无论服务器是新启动还是已在运行，都刷新 PIN 显示
  try {
    const pins: { control_pin: string; view_pin: string } = await invoke("get_pins");
    localControlPin.value = pins.control_pin;
    localViewPin.value = pins.view_pin;
    serverStarted.value = true;
  } catch (_) { /* ignored */ }
}

async function refreshPins() {
  try {
    const pins: { control_pin: string; view_pin: string } = await invoke("refresh_pins");
    localControlPin.value = pins.control_pin;
    localViewPin.value = pins.view_pin;
  } catch (e: unknown) {
    toast.error(translateError(e));
  }
}

async function scanPeers() {
  isScanning.value = true;
  try {
    peers.value = await invoke("discover_peers");
  } catch (e: unknown) {
    toast.error(translateError(e));
  } finally {
    isScanning.value = false;
  }
}

async function connectTo(addr: string, hostname?: string) {
  if (!pinInput.value.trim()) {
    connectError.value = t("discovery.enter_pin");
    return;
  }
  connectError.value = "";
  isConnecting.value = true;
  try {
    const pin = pinInput.value.trim();
    await invoke("connect_to_peer", { addr, pin, role: selectedRole.value });
    localStorage.setItem("lan-desk-last-role", selectedRole.value);
    saveHistory(addr, hostname || "", pin);
    emit("connect", addr);
  } catch (e: unknown) {
    const errMsg = translateError(e);
    // TOFU 证书指纹变更：弹出确认框
    if (typeof e === "string" && e.includes("[TOFU_MISMATCH]")) {
      const match = e.match(/host=(\S+)\s+old=(\S+)\s+new=(\S+)/);
      const host = match?.[1] || addr;
      const confirmed = confirm(
        t("error.tofu_mismatch_confirm", { host, old_fp: match?.[2] || "?", new_fp: match?.[3] || "?" })
      );
      if (confirmed && tofuRetryCount < 2) {
        tofuRetryCount++;
        try {
          await invoke("remove_trusted_host", { host });
          await connectTo(addr, hostname);
        } catch (_) { /* ignored */ }
      } else if (tofuRetryCount >= 2) {
        connectError.value = t("error.tofu_max_retry");
        tofuRetryCount = 0;
      }
    } else {
      connectError.value = errMsg;
    }
  } finally {
    isConnecting.value = false;
  }
}

async function connectManual() {
  if (manualAddr.value.trim()) {
    let addr = manualAddr.value.trim().replace(/\s/g, "");

    // 检测 9 位纯数字 → 视为设备 ID，通过 UDP 扫描匹配
    if (/^\d{9}$/.test(addr)) {
      connectError.value = "";
      isConnecting.value = true;
      try {
        const foundPeers: Peer[] = await invoke("discover_peers");
        const match = foundPeers.find(p => p.device_id === addr);
        if (match) {
          isConnecting.value = false;
          await connectTo(match.addr, match.hostname);
        } else {
          connectError.value = t("discovery.device_not_found");
          isConnecting.value = false;
        }
      } catch (e: unknown) {
        connectError.value = translateError(e);
        isConnecting.value = false;
      }
      return;
    }

    // Normalize bare IPv6 addresses
    let normalized = addr;
    if (normalized.includes(":") && !normalized.startsWith("[")) {
      const colonCount = (normalized.match(/:/g) || []).length;
      if (colonCount > 1) {
        normalized = `[${normalized}]`;
      }
    }
    const hasPort = normalized.startsWith("[")
      ? normalized.includes("]:")
      : /:\d+$/.test(normalized);
    if (!hasPort) {
      const port = settings.value.port || 25605;
      addr = normalized.startsWith("[") ? `${normalized}:${port}` : `${normalized}:${port}`;
    } else {
      addr = normalized;
    }
    connectTo(addr);
  }
}

function isValidMac(mac: string): boolean {
  return /^([0-9A-Fa-f]{2}[:-]){5}[0-9A-Fa-f]{2}$/.test(mac.trim());
}

async function sendWol() {
  if (!wolMac.value.trim()) return;
  if (!isValidMac(wolMac.value)) {
    connectError.value = t("discovery.invalid_mac");
    return;
  }
  try {
    await invoke("wake_on_lan", { macAddress: wolMac.value.trim() });
    connectError.value = "";
    toast.success(t('discovery.wol_sent'));
  } catch (e: unknown) {
    connectError.value = translateError(e);
  }
}

onMounted(async () => {
  load();
  loadHistory();

  // 关键：先将持久化的设置同步到后端，再启动服务器
  // 否则服务器会使用随机 PIN 启动，导致固定密码连接被拒绝
  try {
    if (settings.value.fixedPassword && settings.value.controlPassword && settings.value.viewPassword) {
      await invoke("set_unattended", { autoAccept: settings.value.autoAccept, fixedPassword: true });
      await invoke("set_fixed_pins", {
        controlPin: settings.value.controlPassword,
        viewPin: settings.value.viewPassword,
      });
    }
    await invoke("set_shell_enabled", { enabled: settings.value.shellEnabled });
    await invoke("set_idle_timeout", { minutes: settings.value.idleTimeoutMinutes });
    await invoke("set_lock_on_disconnect", { enabled: settings.value.lockOnDisconnect });
  } catch (_) { /* ignored */ }

  // 设置同步完成后再启动服务器
  await startServer();

  try {
    deviceId.value = await invoke<string>("get_device_id");
    networkAddrs.value = await invoke<NetworkAddr[]>("get_network_info");
  } catch (_) { /* ignored */ }
});
</script>

<template>
  <div class="discovery">
    <header class="header">
      <h1>{{ t("app.title") }}</h1>
      <div class="header-right">
        <span
          class="status"
          :class="{ active: serverStarted }"
        >
          {{ serverStarted ? t("discovery.waiting", { port: settings.port }) : t("discovery.not_started") }}
        </span>
        <button
          class="btn-secondary btn-settings"
          @click="emit('open-settings')"
        >
          {{ t("discovery.settings") }}
        </button>
      </div>
    </header>

    <div class="content">
      <!-- 设备 ID -->
      <div
        v-if="deviceId !== '---'"
        class="device-id-display"
      >
        <span class="device-id-label">{{ t("discovery.device_id") }}</span>
        <span
          class="device-id-code"
          :title="t('discovery.copy_id')"
          @click="copyDeviceId"
        >{{ deviceId.replace(/(\d{3})/g, '$1 ').trim() }}</span>
      </div>

      <!-- 本机网络地址 -->
      <div
        v-if="networkAddrs.length > 0"
        class="net-addrs"
      >
        <span
          v-for="a in networkAddrs"
          :key="a.ip"
          class="net-addr-item"
          :class="'net-' + a.net_type"
          :title="a.name"
        >
          <span
            v-if="a.net_type === 'tailscale'"
            class="net-badge"
          >Tailscale</span>
          <span
            v-else-if="a.net_type === 'zerotier'"
            class="net-badge"
          >ZeroTier</span>
          {{ a.ip }}
        </span>
      </div>

      <!-- 连接状态：谁连接了我（被控端） -->
      <div
        v-if="props.incomingSessions && props.incomingSessions.length > 0"
        class="connection-status incoming"
      >
        <h3>{{ t("discovery.incoming_sessions") }}</h3>
        <div
          v-for="s in props.incomingSessions"
          :key="s.addr"
          class="session-item"
        >
          <span class="session-icon">&#x1F4E5;</span>
          <span class="session-host">{{ s.hostname }}</span>
          <span class="session-addr">{{ s.addr }}</span>
          <span
            class="session-role"
            :class="s.role === 'Controller' ? 'role-ctrl' : 'role-view'"
          >{{ s.role }}</span>
        </div>
      </div>

      <!-- 连接状态：我控制了谁（控制端） -->
      <div
        v-if="props.outgoingTabs && props.outgoingTabs.length > 0"
        class="connection-status outgoing"
      >
        <h3>{{ t("discovery.outgoing_sessions") }}</h3>
        <div
          v-for="tab in props.outgoingTabs"
          :key="tab.id"
          class="session-item session-clickable"
          @click="emit('switch-to-tab', tab.id)"
        >
          <span class="session-icon">&#x1F4E4;</span>
          <span class="session-host">{{ tab.title }}</span>
          <span class="session-addr">{{ tab.addr }}</span>
        </div>
      </div>

      <!-- 本机 PIN 显示（双 PIN） -->
      <div class="pin-display">
        <div class="pin-label">
          <span>{{ t("discovery.control_pin") }}</span>
          <button
            class="btn-text"
            :title="t('discovery.refresh')"
            @click="refreshPins"
          >
            {{ t("discovery.refresh") }}
          </button>
        </div>
        <div class="pin-code">
          {{ localControlPin }}
        </div>
        <div class="pin-row-secondary">
          <span class="pin-label-small">{{ t("discovery.view_pin") }}</span>
          <span class="pin-code-small">{{ localViewPin }}</span>
        </div>
        <div class="pin-hint">
          {{ t("discovery.pin_hint") }}
        </div>
        <button
          class="btn-text"
          style="margin-top: 8px"
          @click="copyConnectInfo"
        >
          {{ t("discovery.copy_connect_info") }}
        </button>
      </div>

      <!-- 快速连接 -->
      <div class="manual-connect">
        <h3>{{ t("discovery.connect_remote") }}</h3>
        <div class="input-row">
          <input
            v-model="manualAddr"
            type="text"
            :placeholder="t('discovery.ip_placeholder')"
            @keyup.enter="connectManual"
          >
          <input
            v-model="pinInput"
            type="password"
            :placeholder="t('discovery.pin_placeholder')"
            maxlength="20"
            class="pin-input"
            @keyup.enter="connectManual"
          >
          <select
            v-model="selectedRole"
            class="role-select"
          >
            <option value="controller">
              {{ t("discovery.role_controller") }}
            </option>
            <option value="viewer">
              {{ t("discovery.role_viewer") }}
            </option>
          </select>
          <button
            class="btn-primary"
            :disabled="isConnecting"
            @click="connectManual"
          >
            {{ isConnecting ? t("discovery.connecting") : t("discovery.connect") }}
          </button>
        </div>
        <label class="remember-pin">
          <input
            v-model="rememberPin"
            type="checkbox"
          >
          <span>{{ t("discovery.remember_pin") }}</span>
        </label>
        <div
          v-if="connectError"
          class="error-msg"
        >
          {{ connectError }}
        </div>
      </div>

      <!-- 设备列表 -->
      <div class="peer-list">
        <div class="peer-list-header">
          <h3>{{ t("discovery.lan_devices") }}</h3>
          <button
            class="btn-secondary"
            :disabled="isScanning"
            @click="scanPeers"
          >
            {{ isScanning ? t("discovery.scanning") : t("discovery.scan") }}
          </button>
        </div>

        <div
          v-if="peers.length === 0"
          class="empty-state"
        >
          <p>{{ t("discovery.no_devices") }}</p>
        </div>

        <div
          v-for="peer in peers"
          :key="peer.addr"
          class="peer-card"
        >
          <div class="peer-info">
            <span class="peer-icon">{{ peer.os === "windows" ? "PC" : peer.os === "macos" ? "Mac" : "Linux" }}</span>
            <div>
              <div class="peer-name">
                {{ peer.hostname }}
              </div>
              <div class="peer-addr">
                {{ peer.addr }} - {{ peer.os }}{{ peer.device_id ? ' · ID:' + peer.device_id : '' }}
              </div>
            </div>
          </div>
          <button
            class="btn-primary"
            :disabled="isConnecting"
            @click="connectTo(peer.addr, peer.hostname)"
          >
            {{ t("discovery.connect") }}
          </button>
        </div>
      </div>

      <!-- Wake-on-LAN -->
      <div class="wol-section">
        <h3>{{ t("discovery.wol_title") }}</h3>
        <div class="input-row">
          <input
            v-model="wolMac"
            type="text"
            :placeholder="t('discovery.wol_placeholder')"
          >
          <button
            class="btn-secondary"
            @click="sendWol"
          >
            {{ t("discovery.wol_send") }}
          </button>
        </div>
      </div>

      <!-- 连接历史 -->
      <div
        v-if="history.length > 0"
        class="history-list"
      >
        <div class="history-header">
          <h3>{{ t("discovery.recent") }}</h3>
          <button
            class="btn-text"
            @click="exportHistory"
          >
            {{ t("discovery.export_csv") }}
          </button>
        </div>
        <div
          v-for="item in history"
          :key="item.addr"
          class="history-item"
        >
          <div
            class="history-info"
            @click="quickConnect(item)"
          >
            <div class="peer-name">
              {{ item.alias || item.hostname || item.addr }}
            </div>
            <div class="peer-addr">
              {{ item.addr }}
            </div>
          </div>
          <template v-if="editingAlias === item.addr">
            <input
              v-model="aliasInput"
              class="alias-input"
              :placeholder="t('discovery.alias_placeholder')"
              @keyup.enter="saveAlias(item.addr)"
            >
            <button
              class="btn-text"
              @click="saveAlias(item.addr)"
            >
              OK
            </button>
          </template>
          <template v-else>
            <button
              class="btn-text"
              :title="t('discovery.edit_alias')"
              @click="startEditAlias(item)"
            >
              &#9998;
            </button>
            <button
              class="btn-text btn-remove"
              @click="removeHistory(item.addr)"
            >
              {{ t("discovery.remove") }}
            </button>
          </template>
        </div>
      </div>
    </div>
  </div>
</template>

<style scoped>
.discovery {
  height: 100vh;
  display: flex;
  flex-direction: column;
  padding: 20px;
}

.header {
  display: flex;
  justify-content: space-between;
  align-items: center;
  margin-bottom: 20px;
}

.header h1 { font-size: 24px; color: var(--text-primary); }
.header-right { display: flex; align-items: center; gap: 8px; }
.btn-settings { font-size: 12px; padding: 4px 10px; }

.status {
  font-size: 12px;
  padding: 4px 12px;
  border-radius: 12px;
  background: var(--border-color);
  color: var(--text-muted);
}
.status.active { background: var(--badge-green-bg); color: var(--badge-green-text); }

.content { flex: 1; overflow-y: auto; padding-bottom: 32px; }

/* PIN 显示区 */
.device-id-display {
  background: var(--bg-secondary);
  border-radius: 10px;
  padding: 12px 16px;
  margin-bottom: 12px;
  display: flex;
  align-items: center;
  justify-content: center;
  gap: 12px;
}
.device-id-label {
  font-size: 13px;
  color: var(--text-muted);
}
.device-id-code {
  font-size: 20px;
  font-family: "Consolas", "Monaco", monospace;
  letter-spacing: 3px;
  color: var(--text-primary);
  font-weight: 600;
  cursor: pointer;
  padding: 4px 8px;
  border-radius: 6px;
}
.device-id-code:hover {
  background: var(--bg-input);
}
.net-addrs {
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
  justify-content: center;
  margin-bottom: 12px;
}
.net-addr-item {
  font-size: 12px;
  font-family: "Consolas", monospace;
  padding: 3px 10px;
  border-radius: 12px;
  background: var(--bg-secondary);
  color: var(--text-muted);
}
.net-addr-item.net-tailscale, .net-addr-item.net-zerotier {
  background: var(--badge-green-bg, #166534);
  color: var(--badge-green-text, #86efac);
  font-weight: 600;
}
.net-badge {
  font-size: 10px;
  opacity: 0.8;
  margin-right: 4px;
}
.pin-display {
  background: var(--bg-secondary);
  border-radius: 10px;
  padding: 16px;
  margin-bottom: 16px;
  text-align: center;
}
.pin-label {
  display: flex;
  justify-content: center;
  align-items: center;
  gap: 8px;
  font-size: 13px;
  color: var(--text-muted);
  margin-bottom: 8px;
}
.btn-text {
  background: none;
  border: none;
  color: var(--accent);
  font-size: 12px;
  cursor: pointer;
  padding: 2px 6px;
}
.btn-text:hover { text-decoration: underline; }
.pin-code {
  font-size: 36px;
  font-family: "Consolas", "Monaco", monospace;
  letter-spacing: 8px;
  color: var(--accent);
  font-weight: bold;
}
.pin-row-secondary {
  display: flex;
  justify-content: center;
  align-items: center;
  gap: 8px;
  margin-top: 8px;
}
.pin-label-small {
  font-size: 12px;
  color: var(--text-muted);
}
.pin-code-small {
  font-size: 18px;
  font-family: "Consolas", "Monaco", monospace;
  letter-spacing: 4px;
  color: var(--text-muted);
  font-weight: 600;
}
.pin-hint {
  font-size: 11px;
  color: var(--text-dim);
  margin-top: 6px;
}

/* 角色选择 */
.role-select {
  padding: 10px 8px;
  border: 1px solid var(--border-color);
  border-radius: 6px;
  background: var(--bg-input);
  color: var(--text-primary);
  font-size: 13px;
  outline: none;
  min-width: 90px;
}
.role-select:focus { border-color: var(--accent); }

/* 连接区 */
.manual-connect {
  background: var(--bg-secondary);
  border-radius: 10px;
  padding: 16px;
  margin-bottom: 16px;
}
.manual-connect h3 { font-size: 14px; margin-bottom: 10px; color: var(--text-muted); }
.input-row { display: flex; gap: 8px; }
.input-row input {
  flex: 1;
  padding: 10px 14px;
  border: 1px solid var(--border-color);
  border-radius: 6px;
  background: var(--bg-input);
  color: var(--text-primary);
  font-size: 14px;
  outline: none;
}
.input-row input:focus { border-color: var(--accent); }
.pin-input {
  max-width: 110px;
  text-align: center;
  letter-spacing: 3px;
  font-family: "Consolas", monospace;
}
.error-msg {
  color: var(--danger);
  font-size: 13px;
  margin-top: 8px;
}

/* 设备列表 */
.peer-list {
  background: var(--bg-secondary);
  border-radius: 10px;
  padding: 16px;
}
.peer-list-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
  margin-bottom: 12px;
}
.peer-list-header h3 { font-size: 14px; color: var(--text-muted); }
.empty-state { text-align: center; padding: 30px; color: var(--text-dim); }
.peer-card {
  display: flex;
  justify-content: space-between;
  align-items: center;
  padding: 12px;
  border-radius: 8px;
  background: var(--bg-input);
  margin-bottom: 8px;
}
.peer-info { display: flex; align-items: center; gap: 12px; }
.peer-icon {
  font-size: 12px;
  background: var(--btn-secondary-bg);
  padding: 6px 8px;
  border-radius: 6px;
  color: var(--text-muted);
}
.peer-name { font-size: 15px; font-weight: 600; color: var(--text-primary); }
.peer-addr { font-size: 12px; color: var(--text-muted); margin-top: 2px; }

/* WOL */
.wol-section {
  background: var(--bg-secondary);
  border-radius: 10px;
  padding: 16px;
  margin-top: 16px;
}
.wol-section h3 { font-size: 14px; color: var(--text-muted); margin-bottom: 10px; }

/* 历史记录 */
.history-list {
  background: var(--bg-secondary);
  border-radius: 10px;
  padding: 16px;
  margin-top: 16px;
}
.history-list h3 { font-size: 14px; color: var(--text-muted); margin-bottom: 12px; }
.history-item {
  display: flex;
  justify-content: space-between;
  align-items: center;
  padding: 8px 12px;
  border-radius: 6px;
  margin-bottom: 4px;
}
.history-item:hover { background: var(--bg-input); }
.history-info { cursor: pointer; flex: 1; }
.btn-remove { color: var(--text-muted); font-size: 11px; }
.btn-remove:hover { color: var(--danger); }
.alias-input {
  width: 100px;
  padding: 4px 8px;
  border: 1px solid var(--border-color);
  border-radius: 4px;
  background: var(--bg-input);
  color: var(--text-primary);
  font-size: 12px;
  outline: none;
}
.alias-input:focus { border-color: var(--accent); }
.history-header { display: flex; justify-content: space-between; align-items: center; margin-bottom: 12px; }
.history-header h3 { font-size: 14px; color: var(--text-muted); margin: 0; }
.remember-pin {
  display: flex;
  align-items: center;
  gap: 6px;
  font-size: 12px;
  color: var(--text-muted);
  margin-top: 6px;
  cursor: pointer;
}
.remember-pin input { cursor: pointer; }

/* ─── 连接状态面板 ─── */
.connection-status {
  background: var(--bg-secondary);
  border: 1px solid var(--border-color);
  border-radius: 10px;
  padding: 12px 16px;
  margin-bottom: 12px;
}
.connection-status h3 {
  font-size: 13px;
  color: var(--text-primary);
  margin: 0 0 8px 0;
}
.connection-status.incoming { border-left: 3px solid #22c55e; }
.connection-status.outgoing { border-left: 3px solid #3b82f6; }
.session-item {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 6px 8px;
  border-radius: 6px;
  font-size: 13px;
}
.session-item:hover { background: var(--bg-input); }
.session-clickable { cursor: pointer; }
.session-icon { font-size: 14px; }
.session-host {
  font-weight: 600;
  color: var(--text-primary);
}
.session-addr {
  color: var(--text-muted);
  font-family: "Consolas","Monaco",monospace;
  font-size: 12px;
}
.session-role {
  margin-left: auto;
  font-size: 11px;
  padding: 2px 8px;
  border-radius: 8px;
  font-weight: 600;
}
.session-role.role-ctrl {
  background: rgba(59, 130, 246, 0.15);
  color: #3b82f6;
}
.session-role.role-view {
  background: rgba(245, 158, 11, 0.15);
  color: #f59e0b;
}
</style>
