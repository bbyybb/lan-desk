<script setup lang="ts">
import { ref, onMounted, onUnmounted, computed } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { useI18n, translateError } from "../i18n";
import { useToast } from "../composables/useToast";

const { t } = useI18n();
const toast = useToast();

const emit = defineEmits<{
  close: [];
}>();

interface FileEntry {
  name: string;
  is_dir: boolean;
  size: number;
  modified_ms: number;
}

interface FileListResponse {
  request_id: number;
  path: string;
  entries: FileEntry[];
  error: string;
}

interface TransferProgress {
  transfer_id: number;
  filename: string;
  total: number;
  transferred: number;
  startTime: number;
  lastTransferred: number;
  lastTime: number;
  speed: number;
}

const remotePath = ref("");
const remoteEntries = ref<FileEntry[]>([]);
const remoteLoading = ref(false);
const remoteError = ref("");
const transfers = ref<Map<number, TransferProgress>>(new Map());

let unlistenFileList: UnlistenFn | null = null;
let unlistenProgress: UnlistenFn | null = null;
let unlistenComplete: UnlistenFn | null = null;
let unlistenCancelled: UnlistenFn | null = null;

// 面包屑路径
const breadcrumbs = computed(() => {
  if (!remotePath.value) return [{ name: t("file_browser.home"), path: "" }];
  const parts = remotePath.value.replace(/\\/g, "/").split("/").filter(Boolean);
  const crumbs = [{ name: t("file_browser.home"), path: "" }];
  let current = "";
  for (const part of parts) {
    current += (current ? "/" : "") + part;
    // Windows drive letter fix
    if (part.endsWith(":")) {
      current = part + "/";
    }
    crumbs.push({ name: part, path: current });
  }
  return crumbs;
});

async function cancelTransfer(transferId: number) {
  try {
    await invoke("cancel_transfer", { transferId });
  } catch (err: unknown) {
    toast.error(translateError(err));
  }
}

function formatEta(seconds: number): string {
  if (!isFinite(seconds) || seconds <= 0) return "--";
  if (seconds < 60) return `${Math.ceil(seconds)}s`;
  if (seconds < 3600) return `${Math.floor(seconds / 60)}m ${Math.ceil(seconds % 60)}s`;
  return `${Math.floor(seconds / 3600)}h ${Math.floor((seconds % 3600) / 60)}m`;
}

function formatSize(bytes: number): string {
  if (bytes === 0) return "-";
  const units = ["B", "KB", "MB", "GB", "TB"];
  let i = 0;
  let size = bytes;
  while (size >= 1024 && i < units.length - 1) {
    size /= 1024;
    i++;
  }
  return `${size.toFixed(i > 0 ? 1 : 0)} ${units[i]}`;
}

function formatTime(ms: number): string {
  if (ms === 0) return "-";
  const d = new Date(ms);
  return d.toLocaleString();
}

async function loadRemoteDir(path: string) {
  remoteLoading.value = true;
  remoteError.value = "";
  try {
    await invoke("request_file_list", { path });
  } catch (err: unknown) {
    remoteLoading.value = false;
    remoteError.value = translateError(err);
    toast.error(translateError(err));
  }
}

function navigateRemote(entry: FileEntry) {
  if (!entry.is_dir) return;
  const sep = remotePath.value.includes("\\") ? "\\" : "/";
  const newPath = remotePath.value ? remotePath.value + sep + entry.name : entry.name;
  loadRemoteDir(newPath);
}

function navigateBreadcrumb(path: string) {
  loadRemoteDir(path);
}

async function uploadFile() {
  try {
    const { open } = await import("@tauri-apps/plugin-dialog");
    const selected = await open({
      multiple: false,
      title: t("file_browser.select_upload"),
    });
    if (selected) {
      await invoke("send_file", { filePath: selected });
      toast.info(t("file_browser.upload_started"));
    }
  } catch (err: unknown) {
    toast.error(translateError(err));
  }
}

async function uploadDirectory() {
  try {
    const { open } = await import("@tauri-apps/plugin-dialog");
    const selected = await open({
      directory: true,
      title: t("file_browser.select_upload_dir"),
    });
    if (selected) {
      await invoke("send_directory", { dirPath: selected });
      toast.info(t("file_browser.upload_dir_started"));
    }
  } catch (err: unknown) {
    toast.error(translateError(err));
  }
}

async function downloadFile(entry: FileEntry) {
  const sep = remotePath.value.includes("\\") ? "\\" : "/";
  const fullPath = remotePath.value ? remotePath.value + sep + entry.name : entry.name;
  try {
    if (entry.is_dir) {
      await invoke("download_remote_directory", { path: fullPath });
      toast.info(t("file_browser.download_dir_started") + ": " + entry.name);
    } else {
      await invoke("download_remote_file", { path: fullPath });
      toast.info(t("file_browser.download_started") + ": " + entry.name);
    }
  } catch (err: unknown) {
    toast.error(translateError(err));
  }
}

const activeTransfers = computed(() => {
  return Array.from(transfers.value.values());
});

onMounted(async () => {
  unlistenFileList = await listen<FileListResponse>("file-list-response", (event) => {
    remoteLoading.value = false;
    const resp = event.payload;
    if (resp.error) {
      remoteError.value = resp.error;
    } else {
      remotePath.value = resp.path;
      remoteEntries.value = resp.entries;
      remoteError.value = "";
    }
  });

  unlistenProgress = await listen<{ transfer_id: number; filename: string; total: number; transferred: number }>("file-transfer-progress", (event) => {
    const p = event.payload;
    const now = Date.now();
    const existing = transfers.value.get(p.transfer_id);
    const startTime = existing?.startTime || now;
    const lastTransferred = existing?.transferred || 0;
    const lastTime = existing?.lastTime || now;
    const elapsed = (now - lastTime) / 1000;
    const speed = elapsed > 0.1 ? (p.transferred - lastTransferred) / elapsed : (existing?.speed || 0);
    transfers.value.set(p.transfer_id, {
      ...p,
      startTime,
      lastTransferred: p.transferred,
      lastTime: now,
      speed,
    });
    transfers.value = new Map(transfers.value);
  });

  unlistenComplete = await listen<{ transfer_id: number; filename?: string }>("file-transfer-complete", (event) => {
    const name = transfers.value.get(event.payload.transfer_id)?.filename || event.payload.filename || "";
    transfers.value.delete(event.payload.transfer_id);
    transfers.value = new Map(transfers.value);
    toast.success(t("file_browser.transfer_complete"));
    // 系统通知（窗口后台时尤其有用）
    try {
      if (Notification.permission === "granted") {
        new Notification("LAN-Desk", { body: t("file_browser.transfer_complete") + (name ? `: ${name}` : "") });
      } else if (Notification.permission !== "denied") {
        Notification.requestPermission();
      }
    } catch (_) { /* ignored */ }
  });

  unlistenCancelled = await listen<{ transfer_id: number }>("file-transfer-cancelled", (event) => {
    transfers.value.delete(event.payload.transfer_id);
    transfers.value = new Map(transfers.value);
    toast.info(t("file_browser.transfer_cancelled"));
  });

  // Load initial remote directory
  loadRemoteDir("");
});

onUnmounted(() => {
  unlistenFileList?.();
  unlistenProgress?.();
  unlistenComplete?.();
  unlistenCancelled?.();
});
</script>

<template>
  <div
    class="file-browser-overlay"
    @click.self="emit('close')"
  >
    <div class="file-browser-dialog">
      <header class="fb-header">
        <h2>{{ t("file_browser.title") }}</h2>
        <button
          class="btn-close"
          @click="emit('close')"
        >
          X
        </button>
      </header>

      <div class="fb-body">
        <!-- Left: Local actions -->
        <div class="fb-panel fb-local">
          <h3>{{ t("file_browser.local") }}</h3>
          <div class="fb-actions">
            <button
              class="btn-primary"
              @click="uploadFile"
            >
              {{ t("file_browser.upload_file") }}
            </button>
            <button
              class="btn-secondary"
              @click="uploadDirectory"
            >
              {{ t("file_browser.upload_dir") }}
            </button>
          </div>

          <!-- Transfer progress -->
          <div
            v-if="activeTransfers.length > 0"
            class="fb-transfers"
          >
            <h4>{{ t("file_browser.transfers") }}</h4>
            <div
              v-for="tr in activeTransfers"
              :key="tr.transfer_id"
              class="fb-transfer-item"
            >
              <span class="fb-transfer-name">{{ tr.filename }}</span>
              <div class="fb-progress-bar">
                <div
                  class="fb-progress-fill"
                  :style="{ width: (tr.total > 0 ? (tr.transferred / tr.total * 100) : 0) + '%' }"
                />
              </div>
              <span class="fb-transfer-size">{{ formatSize(tr.transferred) }} / {{ formatSize(tr.total) }} · {{ formatSize(tr.speed) }}/s · {{ formatEta(tr.speed > 0 ? (tr.total - tr.transferred) / tr.speed : 0) }}</span>
              <button
                class="btn-small btn-cancel"
                @click.stop="cancelTransfer(tr.transfer_id)"
              >
                {{ t("file_browser.cancel") }}
              </button>
            </div>
          </div>
        </div>

        <!-- Right: Remote file list -->
        <div class="fb-panel fb-remote">
          <h3>{{ t("file_browser.remote") }}</h3>

          <!-- Breadcrumb -->
          <div class="fb-breadcrumb">
            <span
              v-for="(crumb, idx) in breadcrumbs"
              :key="idx"
              class="fb-crumb"
              @click="navigateBreadcrumb(crumb.path)"
            >
              {{ crumb.name }}
              <span
                v-if="idx < breadcrumbs.length - 1"
                class="fb-crumb-sep"
              >/</span>
            </span>
          </div>

          <div
            v-if="remoteLoading"
            class="fb-loading"
          >
            {{ t("file_browser.loading") }}
          </div>
          <div
            v-else-if="remoteError"
            class="fb-error"
          >
            {{ remoteError }}
          </div>
          <div
            v-else
            class="fb-file-list"
          >
            <div
              v-if="remotePath"
              class="fb-file-item fb-dir"
              @dblclick="navigateBreadcrumb(breadcrumbs.length > 1 ? breadcrumbs[breadcrumbs.length - 2].path : '')"
            >
              <span class="fb-icon">📁</span>
              <span class="fb-name">..</span>
              <span class="fb-size" />
              <span class="fb-time" />
              <span class="fb-action" />
            </div>
            <div
              v-for="entry in remoteEntries"
              :key="entry.name"
              class="fb-file-item"
              :class="{ 'fb-dir': entry.is_dir }"
              @dblclick="navigateRemote(entry)"
            >
              <span class="fb-icon">{{ entry.is_dir ? "📁" : "📄" }}</span>
              <span class="fb-name">{{ entry.name }}</span>
              <span class="fb-size">{{ entry.is_dir ? "" : formatSize(entry.size) }}</span>
              <span class="fb-time">{{ formatTime(entry.modified_ms) }}</span>
              <button
                v-if="!entry.is_dir"
                class="btn-small btn-secondary"
                @click.stop="downloadFile(entry)"
              >
                {{ t("file_browser.download") }}
              </button>
            </div>
          </div>
        </div>
      </div>
    </div>
  </div>
</template>

<style scoped>
.file-browser-overlay {
  position: fixed;
  top: 0; left: 0; right: 0; bottom: 0;
  background: rgba(0, 0, 0, 0.7);
  display: flex;
  align-items: center;
  justify-content: center;
  z-index: 9999;
}

.file-browser-dialog {
  background: var(--bg-secondary);
  border-radius: 12px;
  width: 90vw;
  max-width: 1000px;
  height: 75vh;
  display: flex;
  flex-direction: column;
  border: 1px solid var(--border-color);
  box-shadow: 0 8px 32px rgba(0, 0, 0, 0.5);
}

.fb-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 12px 20px;
  border-bottom: 1px solid var(--border-color);
}

.fb-header h2 {
  font-size: 16px;
  color: var(--text-primary);
  margin: 0;
}

.btn-close {
  background: none;
  border: none;
  color: var(--text-muted);
  font-size: 16px;
  cursor: pointer;
  padding: 4px 8px;
}
.btn-close:hover {
  color: var(--text-primary);
}

.fb-body {
  display: flex;
  flex: 1;
  overflow: hidden;
}

.fb-panel {
  flex: 1;
  padding: 12px;
  overflow-y: auto;
}

.fb-panel h3 {
  font-size: 14px;
  color: var(--text-primary);
  margin: 0 0 10px 0;
}

.fb-local {
  border-right: 1px solid var(--border-color);
  max-width: 300px;
}

.fb-actions {
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.fb-actions button {
  padding: 8px 12px;
  font-size: 13px;
}

.fb-transfers {
  margin-top: 16px;
}

.fb-transfers h4 {
  font-size: 13px;
  color: var(--text-muted);
  margin: 0 0 8px 0;
}

.fb-transfer-item {
  margin-bottom: 8px;
}

.fb-transfer-name {
  font-size: 12px;
  color: var(--text-secondary);
  display: block;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.fb-progress-bar {
  height: 6px;
  background: var(--bg-input);
  border-radius: 3px;
  margin: 4px 0;
  overflow: hidden;
}

.fb-progress-fill {
  height: 100%;
  background: var(--accent);
  border-radius: 3px;
  transition: width 0.2s;
}

.fb-transfer-size {
  font-size: 11px;
  color: var(--text-dim);
}

/* Breadcrumb */
.fb-breadcrumb {
  display: flex;
  flex-wrap: wrap;
  gap: 2px;
  padding: 6px 0;
  margin-bottom: 8px;
  border-bottom: 1px solid var(--border-color);
  font-size: 12px;
}

.fb-crumb {
  color: var(--accent);
  cursor: pointer;
}
.fb-crumb:hover {
  text-decoration: underline;
}
.fb-crumb-sep {
  color: var(--text-dim);
  margin: 0 2px;
}

.fb-loading, .fb-error {
  font-size: 13px;
  color: var(--text-muted);
  text-align: center;
  padding: 20px;
}

.fb-error {
  color: var(--danger);
}

.fb-file-list {
  display: flex;
  flex-direction: column;
}

.fb-file-item {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 6px 8px;
  border-radius: 4px;
  cursor: default;
  font-size: 13px;
}

.fb-file-item:hover {
  background: var(--bg-input);
}

.fb-dir {
  cursor: pointer;
}

.fb-icon {
  flex-shrink: 0;
  width: 20px;
  text-align: center;
}

.fb-name {
  flex: 1;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  color: var(--text-secondary);
}

.fb-dir .fb-name {
  color: var(--accent);
}

.fb-size {
  width: 80px;
  text-align: right;
  color: var(--text-dim);
  font-size: 12px;
  flex-shrink: 0;
}

.fb-time {
  width: 140px;
  text-align: right;
  color: var(--text-dim);
  font-size: 11px;
  flex-shrink: 0;
}

.btn-small {
  padding: 3px 8px;
  font-size: 11px;
  flex-shrink: 0;
}
</style>
