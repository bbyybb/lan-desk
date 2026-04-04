<script setup lang="ts">
import { ref, onMounted, onUnmounted, watch, nextTick } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import "@xterm/xterm/css/xterm.css";
import { useI18n, translateError } from "../i18n";

const { t } = useI18n();

const props = defineProps<{
  visible: boolean;
}>();

const emit = defineEmits<{
  close: [];
}>();

const terminalRef = ref<HTMLDivElement>();
let terminal: Terminal | null = null;
let fitAddon: FitAddon | null = null;
let unlistenOutput: UnlistenFn | null = null;
let unlistenStartAck: UnlistenFn | null = null;
let resizeObserver: ResizeObserver | null = null;
const isReady = ref(false);
const errorMsg = ref("");

async function initTerminal() {
  if (!terminalRef.value || terminal) return;

  terminal = new Terminal({
    fontSize: 14,
    fontFamily: '"Consolas", "Monaco", "Courier New", monospace',
    theme: {
      background: "#0f1629",
      foreground: "#e0e0e0",
      cursor: "#4361ee",
      selectionBackground: "#4361ee44",
    },
    cursorBlink: true,
    convertEol: true,
  });

  fitAddon = new FitAddon();
  terminal.loadAddon(fitAddon);
  terminal.open(terminalRef.value);
  fitAddon.fit();

  const cols = terminal.cols;
  const rows = terminal.rows;

  // 监听 Shell 输出
  unlistenOutput = await listen<string>("shell-output", (event) => {
    if (terminal) {
      const bytes = Uint8Array.from(atob(event.payload), (c) => c.charCodeAt(0));
      terminal.write(bytes);
    }
  });

  // 监听 Shell 启动结果
  unlistenStartAck = await listen<{ success: boolean; error: string }>(
    "shell-start-ack",
    (event) => {
      if (event.payload.success) {
        isReady.value = true;
        errorMsg.value = "";
      } else {
        errorMsg.value = event.payload.error;
      }
    }
  );

  // 用户输入 → Shell stdin
  terminal.onData((data) => {
    const encoder = new TextEncoder();
    invoke("send_shell_input", { data: Array.from(encoder.encode(data)) }).catch(() => {});
  });

  // 窗口大小变化 → Shell resize
  resizeObserver = new ResizeObserver(() => {
    if (fitAddon && terminal) {
      fitAddon.fit();
      invoke("resize_shell", { cols: terminal.cols, rows: terminal.rows }).catch(() => {});
    }
  });
  resizeObserver.observe(terminalRef.value);

  // 启动远程 Shell
  try {
    await invoke("start_shell", { cols, rows });
  } catch (e: unknown) {
    errorMsg.value = translateError(e);
  }
}

async function cleanup() {
  try {
    await invoke("close_shell");
  } catch (_) { /* ignored */ }
  unlistenOutput?.();
  unlistenStartAck?.();
  resizeObserver?.disconnect();
  terminal?.dispose();
  terminal = null;
  fitAddon = null;
  isReady.value = false;
  errorMsg.value = "";
}

function handleClose() {
  cleanup();
  emit("close");
}

watch(
  () => props.visible,
  (val) => {
    if (val) {
      // 延迟初始化，等待 DOM 渲染
      nextTick(initTerminal);
    } else {
      cleanup();
    }
  }
);

onMounted(() => {
  if (props.visible) {
    nextTick(initTerminal);
  }
});

onUnmounted(() => {
  cleanup();
});

// 拖拽调整高度
const panelRef = ref<HTMLElement | null>(null);
const panelHeight = ref(280);

function onResizeStart(e: MouseEvent) {
  e.preventDefault();
  const startY = e.clientY;
  const startHeight = panelHeight.value;
  function onMove(ev: MouseEvent) {
    const delta = startY - ev.clientY;
    panelHeight.value = Math.max(150, Math.min(600, startHeight + delta));
  }
  function onUp() {
    document.removeEventListener("mousemove", onMove);
    document.removeEventListener("mouseup", onUp);
  }
  document.addEventListener("mousemove", onMove);
  document.addEventListener("mouseup", onUp);
}
</script>

<template>
  <div
    v-if="visible"
    ref="panelRef"
    class="terminal-panel"
    :style="{ height: panelHeight + 'px' }"
  >
    <div
      class="resize-handle"
      @mousedown="onResizeStart"
    />
    <div class="terminal-header">
      <span class="terminal-title">{{ t("remote.terminal") }}</span>
      <span
        v-if="errorMsg"
        class="terminal-error"
      >{{ errorMsg }}</span>
      <button
        class="btn-text terminal-close"
        @click="handleClose"
      >
        {{ t("remote.close_terminal") }}
      </button>
    </div>
    <div
      ref="terminalRef"
      class="terminal-container"
    />
  </div>
</template>

<style scoped>
.terminal-panel {
  border-top: 1px solid var(--border-color);
  background: var(--bg-input);
  display: flex;
  flex-direction: column;
  min-height: 150px;
}
.resize-handle {
  height: 4px;
  cursor: ns-resize;
  background: var(--border-color);
  flex-shrink: 0;
}
.resize-handle:hover {
  background: var(--accent);
}

.terminal-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 4px 12px;
  background: var(--bg-secondary);
  border-bottom: 1px solid var(--border-color);
}

.terminal-title {
  font-size: 12px;
  color: var(--text-muted);
  font-weight: 600;
}

.terminal-error {
  font-size: 11px;
  color: var(--danger);
}

.terminal-close {
  font-size: 11px;
  color: var(--text-muted);
  cursor: pointer;
  background: none;
  border: none;
  padding: 2px 6px;
}
.terminal-close:hover {
  color: var(--danger);
}

.terminal-container {
  flex: 1;
  overflow: hidden;
  padding: 4px;
}
</style>
