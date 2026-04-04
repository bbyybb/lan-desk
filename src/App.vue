<script setup lang="ts">
import { ref, onMounted, onUnmounted } from "vue";
import { listen } from "@tauri-apps/api/event";
import { emit as tauriEmit } from "@tauri-apps/api/event";
import Discovery from "./views/Discovery.vue";
import RemoteView from "./views/RemoteView.vue";
import Settings from "./views/Settings.vue";
import Donate from "./views/Donate.vue";
import ChatPanel from "./views/ChatPanel.vue";
import { initIntegrityGuard, checkOnNavigation } from "./integrity";
import { useI18n } from "./i18n";

const { t } = useI18n();

// 内联检查入口（备份，即使 integrity.ts 被删除也有效）
function _inlineCheck() {
  if (typeof window.__ci === "function") {
    window.__ci();
  }
}

const currentView = ref<"discovery" | "remote" | "settings">("discovery");
const showDonateDialog = ref(false);
const remoteAddr = ref("");
const hasRemoteConnection = ref(false);
const showHostChat = ref(false);

// 被控端：谁连接了我（incoming sessions）
interface IncomingSession {
  hostname: string;
  addr: string;
  role: string;
}
const incomingSessions = ref<IncomingSession[]>([]);

// 多标签支持
interface Tab {
  id: string;
  addr: string;
  title: string;
  connectionId: string;
}
const tabs = ref<Tab[]>([]);
const activeTabId = ref("");

// 授权弹窗
const showAuthDialog = ref(false);
const authHostname = ref("");
const authAddr = ref("");
const authGrantedRole = ref("");
let authTimeout: ReturnType<typeof setTimeout> | null = null;
let unlistenAuthRequest: (() => void) | null = null;
let unlistenConnectionClosed: (() => void) | null = null;
let toastEventHandler: ((e: Event) => void) | null = null;

// Toast 通知
const toasts = ref<{ id: number; message: string; type: string }[]>([]);
let toastId = 0;

const MAX_TOASTS = 5;
function showToast(message: string, type = "info") {
  const id = ++toastId;
  toasts.value.push({ id, message, type });
  // 限制同时显示的 toast 数量
  if (toasts.value.length > MAX_TOASTS) {
    toasts.value = toasts.value.slice(-MAX_TOASTS);
  }
  setTimeout(() => {
    toasts.value = toasts.value.filter((item) => item.id !== id);
  }, 4000);
}

function onConnect(addr: string, connectionId?: string) {
  const tabId = `tab_${Date.now()}`;
  const cid = connectionId || "";
  tabs.value.push({
    id: tabId,
    addr,
    title: addr,
    connectionId: cid,
  });
  activeTabId.value = tabId;
  remoteAddr.value = addr;
  currentView.value = "remote";
  showToast(t("toast.connected", { addr }), "success");
  checkOnNavigation();
  _inlineCheck();
}

function onDisconnect(tabId?: string) {
  const tid = tabId || activeTabId.value;
  tabs.value = tabs.value.filter((tab) => tab.id !== tid);
  if (tabs.value.length > 0) {
    activeTabId.value = tabs.value[tabs.value.length - 1].id;
    remoteAddr.value = tabs.value[tabs.value.length - 1].addr;
  } else {
    currentView.value = "discovery";
    remoteAddr.value = "";
    activeTabId.value = "";
  }
  checkOnNavigation();
  _inlineCheck();
}

function switchTab(tabId: string) {
  activeTabId.value = tabId;
  const tab = tabs.value.find((tb) => tb.id === tabId);
  if (tab) {
    remoteAddr.value = tab.addr;
  }
}

function closeTab(tabId: string) {
  onDisconnect(tabId);
}

function handleAuthResponse(approved: boolean) {
  showAuthDialog.value = false;
  if (authTimeout) clearTimeout(authTimeout);
  tauriEmit("auth-response", String(approved));
  showToast(approved ? t("toast.auth_allowed") : t("toast.auth_denied"), approved ? "success" : "warning");
}

onMounted(async () => {
  unlistenAuthRequest = await listen<{ hostname: string; addr: string; granted_role: string }>("auth-request", (event) => {
    authHostname.value = event.payload.hostname;
    authAddr.value = event.payload.addr;
    authGrantedRole.value = event.payload.granted_role;
    showAuthDialog.value = true;

    if (authTimeout) clearTimeout(authTimeout);
    authTimeout = setTimeout(() => {
      if (showAuthDialog.value) {
        handleAuthResponse(false);
      }
    }, 30000);
  });

  unlistenConnectionClosed = await listen("connection-closed", () => {
    if (currentView.value === "remote") {
      showToast(t("toast.disconnected"), "warning");
    }
  });

  // 被控端：精确追踪谁连接了本机
  await listen<{ hostname: string; addr: string; role: string }>("session-connected", (event) => {
    const { hostname, addr, role } = event.payload;
    incomingSessions.value.push({ hostname, addr, role });
    hasRemoteConnection.value = true;
    showToast(`${hostname} (${addr}) ${t("toast.connected_as")} ${role}`, "info");
  });
  await listen<{ hostname: string; addr: string; role: string }>("session-disconnected", (event) => {
    const { addr } = event.payload;
    incomingSessions.value = incomingSessions.value.filter(s => s.addr !== addr);
    hasRemoteConnection.value = incomingSessions.value.length > 0;
    if (!hasRemoteConnection.value) {
      showHostChat.value = false;
    }
  });
  // 兼容：聊天消息也触发（覆盖旧版被控端无 session 事件的场景）
  await listen("chat-message", () => {
    hasRemoteConnection.value = true;
  });

  // 监听全局自定义 Toast 事件
  toastEventHandler = ((e: Event) => {
    const ce = e as CustomEvent;
    showToast(ce.detail.message, ce.detail.type || 'error');
  });
  window.addEventListener('lan-desk-toast', toastEventHandler);

  // 初始化防篡改守护
  initIntegrityGuard();
});

onUnmounted(() => {
  unlistenAuthRequest?.();
  unlistenConnectionClosed?.();
  if (authTimeout) clearTimeout(authTimeout);
  if (toastEventHandler) {
    window.removeEventListener('lan-desk-toast', toastEventHandler);
  }
});
</script>

<template>
  <div class="app">
    <Discovery
      v-if="currentView === 'discovery'"
      :incoming-sessions="incomingSessions"
      :outgoing-tabs="tabs"
      @connect="onConnect"
      @open-settings="currentView = 'settings'"
      @switch-to-tab="(tabId: string) => { switchTab(tabId); currentView = 'remote'; }"
    />
    <template v-else-if="currentView === 'remote'">
      <!-- Tab bar (only show if there are multiple tabs) -->
      <div
        v-if="tabs.length > 1"
        class="tab-bar"
      >
        <div
          v-for="tab in tabs"
          :key="tab.id"
          class="tab-item"
          :class="{ active: tab.id === activeTabId }"
          @click="switchTab(tab.id)"
        >
          <span class="tab-title">{{ tab.title }}</span>
          <span
            class="tab-close"
            @click.stop="closeTab(tab.id)"
          >X</span>
        </div>
      </div>
      <!-- Use v-show to keep inactive tabs alive -->
      <RemoteView
        v-for="tab in tabs"
        v-show="tab.id === activeTabId"
        :key="tab.id"
        :addr="tab.addr"
        :connection-id="tab.connectionId"
        @disconnect="onDisconnect(tab.id)"
      />
    </template>
    <Settings
      v-else-if="currentView === 'settings'"
      @back="currentView = 'discovery'"
    />

    <!-- 被控端聊天（Discovery 页面，有远程连接时显示） -->
    <div
      v-if="currentView === 'discovery' && hasRemoteConnection"
      class="host-chat-fab"
    >
      <button @click="showHostChat = !showHostChat">
        {{ showHostChat ? 'X' : t("remote.chat") }}
      </button>
    </div>
    <ChatPanel
      :visible="showHostChat"
      @close="showHostChat = false"
    />

    <!-- 作者页脚（远程连接时隐藏，释放屏幕空间） -->
    <div
      v-show="currentView !== 'remote'"
      id="appAuthorFooter"
      class="app-footer"
      data-sig="LANDESK-bbloveyy-2026"
    >
      Made by <b>白白LOVE尹尹</b>
      <span class="footer-sep">|</span>
      <a
        href="#donate"
        @click.prevent="showDonateDialog = true"
      >{{ t("footer.support") }}</a>
    </div>

    <!-- 打赏弹窗 -->
    <Teleport to="body">
      <Donate
        v-if="showDonateDialog"
        @close="showDonateDialog = false"
      />
    </Teleport>

    <!-- 授权确认弹窗 -->
    <Teleport to="body">
      <div
        v-if="showAuthDialog"
        class="modal-overlay"
      >
        <div class="auth-dialog">
          <h2>{{ t("auth.title") }}</h2>
          <p class="auth-info">
            <strong>{{ authHostname }}</strong> ({{ authAddr }})
            <br>{{ t("auth.request") }}
            <br><span
              class="auth-role"
              :class="authGrantedRole === 'Controller' ? 'role-controller' : 'role-viewer'"
            >
              {{ authGrantedRole === "Controller" ? t("auth.role_controller") : t("auth.role_viewer") }}
            </span>
          </p>
          <div class="auth-actions">
            <button
              class="btn-danger"
              @click="handleAuthResponse(false)"
            >
              {{ t("auth.deny") }}
            </button>
            <button
              class="btn-primary"
              @click="handleAuthResponse(true)"
            >
              {{ t("auth.allow") }}
            </button>
          </div>
          <div class="auth-timer">
            {{ t("auth.timeout") }}
          </div>
        </div>
      </div>
    </Teleport>

    <!-- Toast 通知 -->
    <Teleport to="body">
      <div class="toast-container">
        <transition-group name="toast">
          <div
            v-for="toast in toasts"
            :key="toast.id"
            class="toast"
            :class="'toast-' + toast.type"
          >
            {{ toast.message }}
          </div>
        </transition-group>
      </div>
    </Teleport>
  </div>
</template>

<style scoped>
.app { width: 100vw; height: 100vh; overflow: hidden; position: relative; display: flex; flex-direction: column; }

/* Tab bar */
.tab-bar {
  display: flex;
  background: var(--bg-secondary);
  border-bottom: 1px solid var(--border-color);
  flex-shrink: 0;
  overflow-x: auto;
}
.tab-item {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 6px 14px;
  font-size: 12px;
  color: var(--text-muted);
  cursor: pointer;
  border-right: 1px solid var(--border-color);
  white-space: nowrap;
  min-width: 100px;
}
.tab-item:hover {
  background: var(--bg-input);
}
.tab-item.active {
  background: var(--bg-primary, #0d1117);
  color: var(--text-primary);
  border-bottom: 2px solid var(--accent);
}
.tab-title {
  overflow: hidden;
  text-overflow: ellipsis;
}
.tab-close {
  font-size: 10px;
  padding: 2px 4px;
  border-radius: 3px;
  color: var(--text-dim);
}
.tab-close:hover {
  background: rgba(255,255,255,0.1);
  color: var(--danger);
}

/* 作者页脚 */
.host-chat-fab {
  position: fixed;
  bottom: 40px;
  right: 20px;
  z-index: 8000;
}
.host-chat-fab button {
  padding: 10px 20px;
  border-radius: 20px;
  background: var(--accent, #3b82f6);
  color: #fff;
  border: none;
  font-size: 14px;
  cursor: pointer;
  box-shadow: 0 2px 8px rgba(0,0,0,0.3);
}
.host-chat-fab button:hover { opacity: 0.9; }
.app-footer {
  position: fixed; bottom: 0; left: 0; right: 0;
  text-align: center; padding: 6px;
  background: var(--bg-footer);
  border-top: 1px solid var(--border-color);
  color: var(--text-dim); font-size: 11px; z-index: 100;
}
.app-footer b { color: var(--text-muted); }
.app-footer a { color: var(--accent); text-decoration: none; font-size: 11px; }
.app-footer a:hover { text-decoration: underline; }
.footer-sep { color: var(--sep-color); margin: 0 6px; }

/* 弹窗 */
.modal-overlay {
  position: fixed; top: 0; left: 0; right: 0; bottom: 0;
  background: rgba(0, 0, 0, 0.7);
  display: flex; align-items: center; justify-content: center;
  z-index: 9999;
}
.auth-dialog {
  background: var(--bg-secondary); border-radius: 12px; padding: 28px;
  min-width: 360px; text-align: center;
  border: 1px solid var(--border-color); box-shadow: 0 8px 32px rgba(0, 0, 0, 0.5);
}
.auth-dialog h2 { font-size: 18px; color: var(--text-primary); margin-bottom: 16px; }
.auth-info { font-size: 14px; color: var(--text-secondary); line-height: 1.6; margin-bottom: 20px; }
.auth-actions { display: flex; gap: 12px; justify-content: center; }
.auth-actions button { min-width: 100px; padding: 10px 24px; font-size: 15px; }
.auth-timer { font-size: 11px; color: var(--text-dim); margin-top: 12px; }
.auth-role {
  display: inline-block; margin-top: 8px; padding: 3px 12px;
  border-radius: 12px; font-size: 13px; font-weight: bold;
}
.role-controller { background: rgba(231, 76, 60, 0.2); color: var(--danger); }
.role-viewer { background: rgba(46, 204, 113, 0.2); color: var(--success); }

/* Toast */
.toast-container {
  position: fixed; top: 16px; right: 16px; z-index: 10000;
  display: flex; flex-direction: column; gap: 8px;
}
.toast {
  padding: 10px 18px; border-radius: 8px;
  font-size: 13px; color: #fff;
  box-shadow: 0 4px 12px rgba(0, 0, 0, 0.4);
  max-width: 320px;
}
.toast-info { background: var(--toast-info-bg); }
.toast-success { background: var(--toast-success-bg); }
.toast-warning { background: var(--toast-warning-bg); }
.toast-error { background: var(--toast-error-bg); }
.toast-enter-active { transition: all 0.3s ease; }
.toast-leave-active { transition: all 0.3s ease; }
.toast-enter-from { opacity: 0; transform: translateX(40px); }
.toast-leave-to { opacity: 0; transform: translateX(40px); }
</style>
