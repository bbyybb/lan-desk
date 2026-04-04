<script setup lang="ts">
import { ref, onMounted, onUnmounted } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { useI18n, translateError } from "../i18n";
import { useAudio } from "../composables/useAudio";
import { useAnnotation } from "../composables/useAnnotation";
import { useRecording } from "../composables/useRecording";
import { useFrameRenderer, type FrameRegion } from "../composables/useFrameRenderer";
import { useInputHandler } from "../composables/useInputHandler";
import { useRemoteCursor } from "../composables/useRemoteCursor";
import { useReconnect } from "../composables/useReconnect";
import { useSettings } from "../composables/useSettings";
import { useStats } from "../composables/useStats";
import Terminal from "./Terminal.vue";
import ChatPanel from "./ChatPanel.vue";
import FileBrowser from "./FileBrowser.vue";
import { useTouchInput } from "../composables/useTouchInput";
import { useToast } from "../composables/useToast";

const { t } = useI18n();
const toast = useToast();
const { settings } = useSettings();

// 右侧面板
const showSidePanel = ref(false);
// 屏幕遮蔽
const isScreenBlanked = ref(false);
// 远程重启确认
const showRebootConfirm = ref(false);
const audio = useAudio();
const annotation = useAnnotation();
const recording = useRecording();

const props = defineProps<{
  addr: string;
  connectionId?: string;
}>();

const emit = defineEmits<{
  disconnect: [];
}>();

const canvasRef = ref<HTMLCanvasElement | null>(null);
const overlayRef = ref<HTMLCanvasElement | null>(null);
const isControlMode = ref(true);
const scaleMode = ref<"fit" | "original" | "stretch">("fit");
const grantedRole = ref<"Controller" | "Viewer">("Controller");
const showTerminal = ref(false);
const showChat = ref(false);
const showFileBrowser = ref(false);
const showRecordings = ref(false);
const showKeyHelp = ref(false);
const playbackUrl = ref("");
const playbackName = ref("");
const isFullscreen = ref(false);
const isDragOver = ref(false);
const currentEncoder = ref("");
const monitors = ref<{ index: number; name: string; width: number; height: number; is_primary: boolean }[]>([]);
const selectedMonitor = ref(0);

// 组装 composables
const renderer = useFrameRenderer(canvasRef);
const cursor = useRemoteCursor();
const stats = useStats();
const reconnect = useReconnect(props.addr, () => emit("disconnect"));
const input = useInputHandler(
  overlayRef,
  grantedRole,
  isControlMode,
  annotation,
  () => redrawAnnotations(),
);
const touch = useTouchInput(overlayRef);

let unlistenRebootPending: UnlistenFn | null = null;
let unlistenDragDrop: UnlistenFn | null = null;
let keyHelpCleanup: (() => void) | null = null;

async function sendSpecialKey(key: string) {
  try {
    await invoke("send_special_key", { key, connectionId: props.connectionId });
  } catch (err: unknown) {
    toast.error(translateError(err));
  }
}

async function toggleScreenBlank() {
  const enable = !isScreenBlanked.value;
  try {
    await invoke("toggle_screen_blank", { enable, connectionId: props.connectionId });
    isScreenBlanked.value = enable;
  } catch (err: unknown) {
    toast.error(translateError(err));
  }
}

async function confirmReboot() {
  showRebootConfirm.value = false;
  try {
    await invoke("remote_reboot", { connectionId: props.connectionId });
  } catch (err: unknown) {
    toast.error(translateError(err));
  }
}

// Tauri event listener 句柄
let unlistenFrame: UnlistenFn | null = null;
let unlistenClosed: UnlistenFn | null = null;
let unlistenAudioFormat: UnlistenFn | null = null;
let unlistenAudioData: UnlistenFn | null = null;
let unlistenSystemInfo: UnlistenFn | null = null;
let unlistenNetworkRtt: UnlistenFn | null = null;
let unlistenRoleGranted: UnlistenFn | null = null;
let wsConnection: WebSocket | null = null;

interface FrameEvent {
  seq: number;
  timestamp_ms: number;
  regions: FrameRegion[];
  cursor_x: number;
  cursor_y: number;
  cursor_shape: string;
}

function redrawAnnotations() {
  drawCursorAndAnnotations();
}

function drawCursorAndAnnotations() {
  const ov = overlayRef.value;
  if (!ov || !canvasRef.value) return;
  // overlay canvas 尺寸与主 canvas 同步
  if (ov.width !== canvasRef.value.width || ov.height !== canvasRef.value.height) {
    ov.width = canvasRef.value.width;
    ov.height = canvasRef.value.height;
  }
  const ctx = ov.getContext("2d");
  if (!ctx) return;
  ctx.clearRect(0, 0, ov.width, ov.height);
  cursor.drawRemoteCursor(ctx, ov.width, ov.height);
  if (annotation.hasContent()) {
    annotation.drawAll(ctx, ov.width, ov.height);
  }
}

async function handleDisconnect() {
  try {
    await invoke("disconnect", { connectionId: props.connectionId });
  } catch (_e) {
    // ignore
  }
  emit("disconnect");
}

let unlistenMonitorList: UnlistenFn | null = null;

async function loadMonitors() {
  // 远程显示器列表由被控端通过 MonitorList 协议消息推送
  // 不再调用本地 list_monitors（那只会列出本机显示器）
}

async function onSwitchMonitor(index: number) {
  selectedMonitor.value = index;
  invoke("switch_monitor", { index, connectionId: props.connectionId });
}

function openFileBrowser() {
  showFileBrowser.value = true;
}

function toggleRecording() {
  const canvas = canvasRef.value;
  if (!canvas) return;
  recording.toggle(canvas, settings.value.maxFps);
}

function cycleScaleMode() {
  const modes: Array<"fit" | "original" | "stretch"> = ["fit", "original", "stretch"];
  const idx = modes.indexOf(scaleMode.value);
  scaleMode.value = modes[(idx + 1) % modes.length];
}

async function toggleFullscreen() {
  const { getCurrentWindow } = await import("@tauri-apps/api/window");
  const win = getCurrentWindow();
  isFullscreen.value = !isFullscreen.value;
  await win.setFullscreen(isFullscreen.value);
}

async function openPlayback(id: string, name: string) {
  const url = await recording.playRecording(id);
  if (url) {
    playbackUrl.value = url;
    playbackName.value = name;
  } else {
    toast.error(t("error.recording_not_found"));
  }
}

function closePlayback() {
  if (playbackUrl.value) URL.revokeObjectURL(playbackUrl.value);
  playbackUrl.value = "";
  playbackName.value = "";
}

function saveScreenshot() {
  const c = canvasRef.value;
  if (!c) return;
  c.toBlob((blob) => {
    if (!blob) return;
    const url = URL.createObjectURL(blob);
    const a = document.createElement("a");
    a.href = url;
    a.download = `lan-desk-screenshot-${new Date().toISOString().replace(/[:.]/g, "-").slice(0, 19)}.png`;
    a.click();
    URL.revokeObjectURL(url);
    toast.success(t("remote.screenshot_saved"));
  }, "image/png");
}

// ──────────────── WebSocket 二进制帧解析 ────────────────

const CURSOR_SHAPES = ["Arrow","IBeam","Hand","Crosshair","ResizeNS","ResizeEW","ResizeNESW","ResizeNWSE","Move","Wait","Help","NotAllowed","Hidden"];

// 二进制帧头固定大小：1(msgType) + 8(seq) + 8(timestamp) + 8(cursor_x) + 8(cursor_y) + 1(cursorShape) + 1(regionCount)
const FRAME_HEADER_SIZE = 35;

function handleBinaryFrame(buf: Uint8Array) {
  if (buf.byteLength < FRAME_HEADER_SIZE) return;
  const view = new DataView(buf.buffer, buf.byteOffset, buf.byteLength);
  let offset = 1;
  offset += 8; // seq (u64)
  const timestamp_ms = Number(view.getBigUint64(offset, true)); offset += 8;
  const cursor_x_val = view.getFloat64(offset, true); offset += 8;
  const cursor_y_val = view.getFloat64(offset, true); offset += 8;
  const cursorShapeId = buf[offset]; offset += 1;
  const regionCount = buf[offset]; offset += 1;

  stats.countFrame();
  stats.updateLatency(timestamp_ms);

  // After reading regionCount, validate remaining buffer size
  // Each region needs at minimum 18 bytes (4+4+4+4+1+1 header + variable data)
  // We can't pre-validate exact size since dataLen is variable, but check minimum
  if (regionCount > 0 && buf.byteLength - offset < 18) return;

  for (let i = 0; i < regionCount; i++) {
    if (offset + 18 > buf.byteLength) break; // safety check before each region
    const x = view.getUint32(offset, true); offset += 4;
    const y = view.getUint32(offset, true); offset += 4;
    const w = view.getUint32(offset, true); offset += 4;
    const h = view.getUint32(offset, true); offset += 4;
    const encType = buf[offset]; offset += 1;
    const encMeta = buf[offset]; offset += 1;
    const dataLen = view.getUint32(offset, true); offset += 4;
    const data = buf.slice(offset, offset + dataLen); offset += dataLen;

    stats.addBytes(dataLen);
    renderer.autoResize(x, y, w, h);
    renderer.drawBinaryRegion(x, y, w, h, encType, encMeta, data);
  }

  cursor.updateCursor(cursor_x_val, cursor_y_val, CURSOR_SHAPES[cursorShapeId] || "Arrow");
  drawCursorAndAnnotations();
}

function handleBinaryAudio(buf: Uint8Array) {
  if (!audio.audioEnabled.value) return;
  const pcmBytes = buf.slice(1);
  audio.playPcm(pcmBytes);
}

function handleBinaryAudioFormat(buf: Uint8Array) {
  if (buf.length < 8) return;
  const view = new DataView(buf.buffer, buf.byteOffset, buf.byteLength);
  const sampleRate = view.getUint32(1, true);
  const channels = view.getUint16(5, true);
  audio.setFormat(sampleRate, channels);
}

onMounted(async () => {
  // 监听角色授予事件
  unlistenRoleGranted = await listen<string>("role-granted", (event) => {
    grantedRole.value = event.payload.includes("Viewer") ? "Viewer" : "Controller";
    if (grantedRole.value === "Viewer") {
      isControlMode.value = false;
    }
  });

  // 初始化 canvas 和 H.264 解码器
  renderer.setupCanvas(t("remote.waiting"));
  renderer.initVideoDecoder();

  // 尝试通过 WebSocket 接收高带宽数据（帧 + 音频），需要 token 认证
  //
  // 安全模型说明：
  // - 端口号来自 Tauri invoke("get_ws_port")，该调用仅在本进程内可用，外部无法伪造
  // - WebSocket 仅绑定 127.0.0.1，不接受远程连接
  // - 连接后需发送 32 字节随机 token 进行身份验证，防止本机其他进程连接
  // - CSP 中使用 ws://127.0.0.1:* 通配端口，因为端口由 OS 随机分配，无法静态限定
  try {
    const wsInfo: { port: number; token: string } = await invoke("get_ws_port");

    // 验证端口在有效的临时端口范围内（1024-65535），防止异常值
    if (!Number.isInteger(wsInfo.port) || wsInfo.port < 1024 || wsInfo.port > 65535) {
      throw new Error(`WebSocket port out of valid range: ${wsInfo.port}`);
    }

    wsConnection = new WebSocket(`ws://127.0.0.1:${wsInfo.port}`);
    wsConnection.binaryType = "arraybuffer";
    wsConnection.onopen = () => {
      wsConnection?.send(wsInfo.token);
    };
    wsConnection.onmessage = (event) => {
      const buf = new Uint8Array(event.data as ArrayBuffer);
      if (buf.length < 1) return;
      const msgType = buf[0];
      if (msgType === 0x01) handleBinaryFrame(buf);
      else if (msgType === 0x02) handleBinaryAudio(buf);
      else if (msgType === 0x03) handleBinaryAudioFormat(buf);
    };
    wsConnection.onclose = () => { wsConnection = null; };
  } catch (_) {
    // WebSocket 不可用，继续用 Tauri emit
  }

  unlistenFrame = await listen<FrameEvent>("frame-update", (event) => {
    if (wsConnection) return;
    const frame = event.payload;
    stats.countFrame();
    stats.updateLatency(frame.timestamp_ms);

    if (frame.regions.length > 0) {
      const r = frame.regions[0];
      renderer.autoResize(r.x, r.y, r.width, r.height);
      // 追踪当前编码器类型用于状态栏显示
      const enc = r.encoding;
      if (enc.startsWith("H264")) currentEncoder.value = "H.264";
      else if (enc.startsWith("H265")) currentEncoder.value = "HEVC";
      else if (enc.startsWith("Av1")) currentEncoder.value = "AV1";
      else if (enc.startsWith("Jpeg")) currentEncoder.value = "JPEG";
      else if (enc.startsWith("Raw")) currentEncoder.value = "Raw";
    }

    for (const region of frame.regions) {
      renderer.drawRegion(region, stats.addBytes);
    }

    cursor.updateCursor(frame.cursor_x, frame.cursor_y, frame.cursor_shape);
    drawCursorAndAnnotations();
  });

  unlistenClosed = await listen("connection-closed", () => {
    reconnect.attempt();
  });

  // 监听音频格式
  unlistenAudioFormat = await listen<{ sample_rate: number; channels: number; bits_per_sample: number }>(
    "audio-format",
    (event) => {
      if (wsConnection) return;
      audio.setFormat(event.payload.sample_rate, event.payload.channels);
    }
  );

  // 监听音频数据
  unlistenAudioData = await listen<string>("audio-data", (event) => {
    if (wsConnection) return;
    if (!audio.audioEnabled.value) return;
    const binaryStr = atob(event.payload);
    const bytes = new Uint8Array(binaryStr.length);
    for (let i = 0; i < binaryStr.length; i++) {
      bytes[i] = binaryStr.charCodeAt(i);
    }
    audio.playPcm(bytes);
  });

  // 监听系统信息
  unlistenSystemInfo = await listen<{ cpu: number; memory: number; memory_total_mb?: number }>("system-info", (event) => {
    stats.updateSystemInfo(event.payload.cpu, event.payload.memory, event.payload.memory_total_mb);
  });

  // 监听 RTT
  unlistenNetworkRtt = await listen<number>("network-rtt", (event) => {
    stats.updateRtt(event.payload);
  });

  // 监听远程重启通知
  unlistenRebootPending = await listen<{ estimated_seconds: number }>("reboot-pending", (event) => {
    toast.info(t("remote.reboot_pending", { seconds: event.payload.estimated_seconds }));
    reconnect.rebootReconnect(event.payload.estimated_seconds);
  });

  // 监听远程显示器列表（后续更新）
  unlistenMonitorList = await listen<{ monitors: typeof monitors.value }>("monitor-list", (event) => {
    monitors.value = event.payload.monitors;
    const primary = event.payload.monitors.find((m: { is_primary: boolean }) => m.is_primary);
    if (primary) selectedMonitor.value = primary.index;
  });

  // 主动获取缓存的远程显示器列表（解决 MonitorList 在 RemoteView 挂载前到达的竞态）
  try {
    const cached: typeof monitors.value = await invoke("get_remote_monitors");
    if (cached && cached.length > 0) {
      monitors.value = cached;
      const primary = cached.find(m => m.is_primary);
      if (primary) selectedMonitor.value = primary.index;
    }
  } catch (_) { /* ignored */ }

  loadMonitors();
  stats.startTimer();
  input.setupListeners();
  touch.setupTouchListeners();

  // F1 快捷键帮助面板
  const onKeyHelp = (e: KeyboardEvent) => {
    if (e.key === "F1" || (e.key === "?" && !isControlMode.value)) {
      e.preventDefault();
      showKeyHelp.value = !showKeyHelp.value;
    }
  };
  document.addEventListener("keydown", onKeyHelp);
  keyHelpCleanup = () => document.removeEventListener("keydown", onKeyHelp);

  // 拖拽上传：监听 Tauri 原生文件拖放事件
  try {
    unlistenDragDrop = await getCurrentWebview().onDragDropEvent(async (event) => {
      if (event.payload.type === "over") {
        isDragOver.value = true;
      } else if (event.payload.type === "drop") {
        isDragOver.value = false;
        const paths = event.payload.paths;
        for (const filePath of paths) {
          try {
            const stat = await invoke<{ is_dir: boolean }>("stat_path", { path: filePath });
            if (stat.is_dir) {
              await invoke("send_directory", { dirPath: filePath });
            } else {
              await invoke("send_file", { filePath });
            }
            toast.info(t("file_browser.upload_started"));
          } catch (err: unknown) {
            toast.error(translateError(err));
          }
        }
      } else {
        isDragOver.value = false;
      }
    });
  } catch (_) { /* ignored */ }
});

onUnmounted(() => {
  unlistenFrame?.();
  unlistenClosed?.();
  unlistenAudioFormat?.();
  unlistenAudioData?.();
  unlistenSystemInfo?.();
  unlistenNetworkRtt?.();
  unlistenRoleGranted?.();
  unlistenRebootPending?.();
  unlistenDragDrop?.();
  unlistenMonitorList?.();
  keyHelpCleanup?.();

  reconnect.cancel();
  stats.stopTimer();
  renderer.destroy();
  audio.destroy();
  recording.destroy();
  input.removeListeners();
  touch.removeTouchListeners();

  if (wsConnection) {
    wsConnection.close();
    wsConnection = null;
  }
});
</script>

<template>
  <div class="remote-view">
    <!-- 精简顶栏：仅状态信息 -->
    <header class="toolbar">
      <button
        class="btn-danger btn-sm"
        @click="handleDisconnect"
      >
        {{ t("remote.disconnect") }}
      </button>
      <span class="remote-addr">{{ addr }}</span>
      <span
        class="tls-badge"
        :title="t('remote.tls_encrypted')"
      >TLS</span>
      <div class="stats">
        <span class="stat-item">FPS:{{ stats.fps.value }}</span>
        <span class="stat-item">{{ stats.latency.value }}ms</span>
        <span class="stat-item">{{ stats.bandwidth.value }}</span>
        <span class="stat-item">RTT:{{ stats.rtt.value }}ms</span>
        <span
          class="quality-dot"
          :class="'q-' + stats.networkQuality.value"
          :title="stats.networkQuality.value"
        />
        <span class="stat-item">CPU:{{ stats.cpuUsage.value }}%</span>
        <span class="stat-item">MEM:{{ stats.memUsage.value }}%{{ stats.memTotalMb.value > 0 ? '(' + (stats.memTotalMb.value / 1024).toFixed(0) + 'G)' : '' }}</span>
        <span
          v-if="currentEncoder"
          class="stat-item"
        >{{ currentEncoder }}</span>
        <span
          class="rtt-sparkline"
          :title="'RTT history'"
        >
          <span
            v-for="(val, i) in stats.rttHistory.value.slice(-20)"
            :key="i"
            class="rtt-bar"
            :class="val < 30 ? 'q-good' : val < 100 ? 'q-fair' : 'q-poor'"
            :style="{ height: Math.min(16, Math.max(2, val / 10)) + 'px' }"
          />
        </span>
      </div>
      <span
        v-if="grantedRole === 'Viewer'"
        class="viewer-badge"
      >{{ t("remote.viewer_mode") }}</span>
    </header>

    <!-- 重连提示 -->
    <div
      v-if="reconnect.isReconnecting.value && !reconnect.isRebootReconnecting.value"
      class="reconnect-bar"
    >
      {{ t("remote.reconnecting", { count: reconnect.reconnectCount.value }) }}
    </div>
    <div
      v-if="reconnect.isRebootReconnecting.value"
      class="reconnect-bar"
    >
      <template v-if="reconnect.rebootCountdown.value > 0">
        {{ t("remote.reboot_pending", { seconds: reconnect.rebootCountdown.value }) }}
      </template>
      <template v-else>
        {{ t("remote.reboot_reconnecting", { count: reconnect.reconnectCount.value }) }}
      </template>
    </div>

    <!-- 主内容区：canvas 全屏 + 右侧面板 -->
    <div class="main-area">
      <div class="canvas-container">
        <div class="canvas-wrapper">
          <canvas
            ref="canvasRef"
            :class="'scale-' + scaleMode"
          />
          <canvas
            ref="overlayRef"
            class="cursor-overlay"
            :class="'scale-' + scaleMode"
            :style="{ cursor: annotation.isAnnotating.value ? 'crosshair' : isControlMode ? 'none' : 'default' }"
            tabindex="0"
            @mousemove="input.onMouseMove"
            @mousedown="input.onMouseDown"
            @mouseup="input.onMouseUp"
            @wheel="input.onWheel"
            @contextmenu="input.onContextMenu"
          />
        </div>
        <div
          v-if="isDragOver"
          class="drop-overlay"
        >
          <div class="drop-message">
            {{ t("remote.drop_to_upload") }}
          </div>
        </div>
      </div>

      <!-- 右侧面板展开按钮（固定在面板左侧边缘） -->
      <button
        class="side-panel-toggle"
        :class="{ 'panel-open': showSidePanel }"
        :title="t('remote.tools')"
        @click="showSidePanel = !showSidePanel"
      >
        {{ showSidePanel ? '&#x25B6;' : '&#x25C0;' }}
      </button>

      <!-- 右侧可折叠工具面板 -->
      <aside
        v-if="showSidePanel"
        class="side-panel"
      >
        <div class="side-section">
          <div class="side-label">
            {{ t("remote.control_mode") }}
          </div>
          <button
            v-if="grantedRole === 'Controller'"
            class="side-btn"
            :class="{ active: isControlMode }"
            @click="isControlMode = !isControlMode"
          >
            {{ isControlMode ? t("remote.control_mode") : t("remote.view_only") }}
          </button>
        </div>

        <!-- 多显示器 -->
        <div
          v-if="monitors.length > 1"
          class="side-section"
        >
          <div class="side-label">
            {{ t("remote.monitors") }}
          </div>
          <button
            v-for="m in monitors"
            :key="m.index"
            class="side-btn"
            :class="{ active: m.index === selectedMonitor }"
            @click="onSwitchMonitor(m.index)"
          >
            {{ m.is_primary ? "★ " : "" }}{{ m.name }} ({{ m.width }}x{{ m.height }})
          </button>
        </div>

        <!-- 媒体 -->
        <div class="side-section">
          <div class="side-label">
            {{ t("remote.tools") }}
          </div>
          <button
            class="side-btn"
            @click="audio.audioEnabled.value = !audio.audioEnabled.value"
          >
            {{ audio.audioEnabled.value ? t("remote.mute") : t("remote.unmute") }}
          </button>
          <button
            v-if="recording.canRecord.value"
            class="side-btn"
            :class="{ 'btn-rec': recording.isRecording.value }"
            @click="toggleRecording"
          >
            {{ recording.isRecording.value ? t("remote.stop_record") : t("remote.record") }}
          </button>
          <button
            class="side-btn"
            @click="cycleScaleMode()"
          >
            {{ t("remote.scale_" + scaleMode) }}
          </button>
          <button
            class="side-btn"
            @click="toggleFullscreen()"
          >
            {{ isFullscreen ? t("remote.exit_fullscreen") : t("remote.fullscreen") }}
          </button>
          <button
            class="side-btn"
            @click="saveScreenshot()"
          >
            {{ t("remote.screenshot") }}
          </button>
          <button
            v-if="recording.recordings.value.length > 0"
            class="side-btn"
            @click="showRecordings = true"
          >
            {{ t("remote.recording_history") }}
          </button>
        </div>

        <!-- 功能 -->
        <div
          v-if="grantedRole === 'Controller'"
          class="side-section"
        >
          <div class="side-label">
            {{ t("remote.file") }} / {{ t("remote.chat") }}
          </div>
          <button
            class="side-btn"
            @click="openFileBrowser()"
          >
            {{ t("remote.file") }}
          </button>
          <button
            class="side-btn"
            @click="showChat = !showChat"
          >
            {{ t("remote.chat") }}
          </button>
          <button
            class="side-btn"
            @click="showTerminal = !showTerminal"
          >
            {{ t("remote.terminal") }}
          </button>
        </div>

        <!-- 标注 -->
        <div
          v-if="grantedRole === 'Controller'"
          class="side-section"
        >
          <div class="side-label">
            {{ t("remote.annotate") }}
          </div>
          <button
            class="side-btn"
            :class="{ active: annotation.isAnnotating.value }"
            @click="annotation.isAnnotating.value = !annotation.isAnnotating.value"
          >
            {{ annotation.isAnnotating.value ? t("remote.annotating") : t("remote.annotate") }}
          </button>
          <template v-if="annotation.isAnnotating.value">
            <div class="side-btn-row">
              <button
                class="side-btn-sm"
                :class="{ active: annotation.annotationTool.value === 'pen' }"
                @click="annotation.annotationTool.value = 'pen'"
              >
                {{ t("annotation.pen") }}
              </button>
              <button
                class="side-btn-sm"
                :class="{ active: annotation.annotationTool.value === 'text' }"
                @click="annotation.annotationTool.value = 'text'"
              >
                {{ t("annotation.text") }}
              </button>
            </div>
            <button
              class="side-btn"
              @click="annotation.clear(); redrawAnnotations()"
            >
              {{ t("remote.clear_annotation") }}
            </button>
            <button
              v-if="annotation.hasContent()"
              class="side-btn"
              @click="annotation.undo(); redrawAnnotations()"
            >
              {{ t("annotation.undo") }}
            </button>
            <label
              class="annotation-color-picker"
              :title="t('annotation.color')"
            >
              <input
                v-model="annotation.annotationColor.value"
                type="color"
              >
            </label>
            <div
              v-if="annotation.annotationTool.value === 'pen'"
              class="side-btn-row"
            >
              <button
                class="side-btn-sm"
                :class="{ active: annotation.lineWidth.value === 1 }"
                @click="annotation.lineWidth.value = 1"
              >
                {{ t("annotation.thin") }}
              </button>
              <button
                class="side-btn-sm"
                :class="{ active: annotation.lineWidth.value === 3 }"
                @click="annotation.lineWidth.value = 3"
              >
                {{ t("annotation.medium") }}
              </button>
              <button
                class="side-btn-sm"
                :class="{ active: annotation.lineWidth.value === 6 }"
                @click="annotation.lineWidth.value = 6"
              >
                {{ t("annotation.thick") }}
              </button>
            </div>
          </template>
        </div>

        <!-- 远程控制 -->
        <div
          v-if="grantedRole === 'Controller'"
          class="side-section"
        >
          <div class="side-label">
            {{ t("remote.remote_ctrl") }}
          </div>
          <button
            class="side-btn"
            @click="sendSpecialKey('CtrlAltDel')"
          >
            Ctrl+Alt+Del
          </button>
          <button
            class="side-btn"
            @click="sendSpecialKey('AltTab')"
          >
            Alt+Tab
          </button>
          <button
            class="side-btn"
            @click="sendSpecialKey('AltF4')"
          >
            Alt+F4
          </button>
          <button
            class="side-btn"
            @click="sendSpecialKey('WinKey')"
          >
            Win
          </button>
          <button
            class="side-btn"
            @click="sendSpecialKey('WinL')"
          >
            Win+L
          </button>
          <button
            class="side-btn"
            @click="sendSpecialKey('PrintScreen')"
          >
            PrtSc
          </button>
          <button
            class="side-btn"
            @click="sendSpecialKey('CtrlEsc')"
          >
            Ctrl+Esc
          </button>
          <hr class="side-hr">
          <button
            class="side-btn"
            @click="toggleScreenBlank()"
          >
            {{ isScreenBlanked ? t("remote.screen_blank_off") : t("remote.screen_blank") }}
          </button>
          <button
            class="side-btn btn-rec"
            @click="showRebootConfirm = true"
          >
            {{ t("remote.reboot") }}
          </button>
        </div>
      </aside>
    </div>

    <!-- 移动端虚拟键盘 -->
    <div
      v-if="touch.isMobile.value"
      class="mobile-fab"
    >
      <button @click="touch.showVirtualKeyboard.value = !touch.showVirtualKeyboard.value">
        &#x2328;
      </button>
    </div>
    <div
      v-if="touch.isMobile.value && touch.showVirtualKeyboard.value"
      class="virtual-keyboard"
    >
      <button
        v-for="k in ['Escape','Tab','Enter','Backspace','Space','ArrowUp','ArrowDown','ArrowLeft','ArrowRight']"
        :key="k"
        @click="touch.onVirtualKey(k)"
      >
        {{ k.replace('Arrow','') }}
      </button>
    </div>

    <!-- 文件浏览器 -->
    <Teleport to="body">
      <FileBrowser
        v-if="showFileBrowser"
        @close="showFileBrowser = false"
      />

      <!-- 录制历史面板 -->
      <div
        v-if="showRecordings"
        class="recording-overlay"
        @click.self="showRecordings = false"
      >
        <div class="recording-dialog">
          <header class="recording-header">
            <h3>{{ t("remote.recording_history") }}</h3>
            <button
              class="btn-text"
              @click="showRecordings = false"
            >
              X
            </button>
          </header>
          <div class="recording-list">
            <div
              v-for="rec in recording.recordings.value"
              :key="rec.id"
              class="recording-item"
            >
              <span class="recording-name">{{ rec.name }}</span>
              <span class="recording-size">{{ (rec.size / 1024 / 1024).toFixed(1) }} MB</span>
              <button
                class="btn-small btn-primary"
                @click="openPlayback(rec.id, rec.name)"
              >
                {{ t("remote.play") }}
              </button>
              <button
                class="btn-small btn-secondary"
                @click="recording.downloadRecording(rec.id)"
              >
                {{ t("file_browser.download") }}
              </button>
              <button
                class="btn-small btn-cancel"
                @click="recording.removeRecording(rec.id)"
              >
                {{ t("file_browser.cancel") }}
              </button>
            </div>
          </div>
        </div>
      </div>

      <!-- 视频播放器 -->
      <div
        v-if="playbackUrl"
        class="recording-overlay"
        @click.self="closePlayback"
      >
        <div class="playback-dialog">
          <header class="recording-header">
            <h3>{{ playbackName }}</h3>
            <button
              class="btn-text"
              @click="closePlayback"
            >
              X
            </button>
          </header>
          <video
            :src="playbackUrl"
            controls
            autoplay
            class="playback-video"
          />
        </div>
      </div>
    </Teleport>

    <!-- 快捷键帮助面板 -->
    <Teleport to="body">
      <div
        v-if="showKeyHelp"
        class="reboot-confirm-overlay"
        @click.self="showKeyHelp = false"
      >
        <div
          class="reboot-confirm-dialog"
          style="min-width: 350px"
        >
          <h3>{{ t("remote.key_help_title") }}</h3>
          <div class="key-help-list">
            <div class="key-help-row">
              <kbd>F1</kbd><span>{{ t("remote.key_help_title") }}</span>
            </div>
          </div>
          <div style="text-align: center; margin-top: 16px">
            <button
              class="btn-secondary"
              @click="showKeyHelp = false"
            >
              OK
            </button>
          </div>
        </div>
      </div>
    </Teleport>

    <!-- 聊天面板 -->
    <ChatPanel
      :visible="showChat"
      @close="showChat = false"
    />

    <!-- 远程终端 -->
    <Terminal
      :visible="showTerminal"
      @close="showTerminal = false"
    />

    <!-- 远程重启确认弹窗 -->
    <Teleport to="body">
      <div
        v-if="showRebootConfirm"
        class="reboot-confirm-overlay"
        @click.self="showRebootConfirm = false"
      >
        <div class="reboot-confirm-dialog">
          <h3>{{ t("remote.reboot_confirm_title") }}</h3>
          <p>{{ t("remote.reboot_confirm_msg") }}</p>
          <div class="reboot-confirm-actions">
            <button
              class="btn-secondary"
              @click="showRebootConfirm = false"
            >
              {{ t("remote.reboot_confirm_no") }}
            </button>
            <button
              class="btn-danger"
              @click="confirmReboot"
            >
              {{ t("remote.reboot_confirm_yes") }}
            </button>
          </div>
        </div>
      </div>
    </Teleport>
  </div>
</template>

<style scoped>
.remote-view {
  height: 100vh;
  display: flex;
  flex-direction: column;
  background: #000;
}

/* ─── 精简顶栏 ─── */
.toolbar {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 3px 8px;
  background: var(--bg-secondary);
  border-bottom: 1px solid var(--border-color);
  flex-shrink: 0;
  height: 28px;
  min-height: 28px;
}
.btn-sm { font-size: 11px; padding: 2px 8px; }
.remote-addr { font-size: 11px; color: var(--text-muted); }
.tls-badge {
  font-size: 9px; padding: 1px 5px; border-radius: 3px;
  background: var(--badge-green-bg); color: var(--badge-green-text);
  font-weight: 600; letter-spacing: 1px;
}
.stats { margin-left: auto; display: flex; gap: 10px; }
.stat-item { font-size: 11px; color: var(--text-muted); font-family: "Consolas","Monaco",monospace; }
.viewer-badge {
  font-size: 10px; padding: 2px 6px; border-radius: 8px;
  background: #b45309; color: #fff; font-weight: 600;
}
.quality-dot { width: 8px; height: 8px; border-radius: 50%; display: inline-block; }
.q-good { background: #4caf50; }
.q-fair { background: #ff9800; }
.q-poor { background: #f44336; }
.rtt-sparkline { display: inline-flex; align-items: flex-end; gap: 1px; height: 14px; margin-left: 2px; vertical-align: middle; }
.rtt-bar { width: 2px; border-radius: 1px; }
.rtt-bar.q-good { background: var(--badge-green-bg, #22c55e); }
.rtt-bar.q-fair { background: #f59e0b; }
.rtt-bar.q-poor { background: var(--danger, #ef4444); }

.reconnect-bar {
  background: #b45309; color: #fff; text-align: center;
  padding: 4px; font-size: 12px; flex-shrink: 0;
}

/* ─── 主内容区：横向布局 ─── */
.main-area {
  flex: 1;
  display: flex;
  position: relative;
  overflow: hidden;
}
.canvas-container {
  flex: 1;
  position: relative;
  min-height: 0;
  min-width: 0;
  overflow: hidden;
}
.canvas-wrapper {
  position: absolute;
  top: 0;
  left: 0;
  right: 0;
  bottom: 0;
  display: flex;
  align-items: center;
  justify-content: center;
  overflow: auto;
}
.cursor-overlay { position: absolute; top: 0; left: 0; pointer-events: auto; }
canvas.scale-fit {
  max-width: 100%;
  max-height: 100%;
  width: auto;
  height: auto;
}
canvas.scale-original {
  /* 原始像素尺寸，wrapper overflow:auto 提供滚动条 */
  max-width: none;
  max-height: none;
}
canvas.scale-stretch {
  width: 100% !important;
  height: 100% !important;
  max-width: 100% !important;
  max-height: 100% !important;
}

.drop-overlay {
  position: absolute; top: 0; left: 0; right: 0; bottom: 0;
  background: rgba(59, 130, 246, 0.3);
  display: flex; align-items: center; justify-content: center;
  z-index: 100; pointer-events: none;
}
.drop-message {
  font-size: 22px; color: #fff;
  background: rgba(0,0,0,0.6); padding: 16px 32px;
  border-radius: 12px; border: 2px dashed rgba(255,255,255,0.6);
}

/* ─── 右侧可折叠面板 ─── */
.side-panel-toggle {
  position: absolute;
  right: 0;
  top: 50%;
  transform: translateY(-50%);
  z-index: 500;
  width: 20px;
  height: 60px;
  background: rgba(30, 30, 40, 0.85);
  color: var(--text-muted);
  border: 1px solid var(--border-color);
  border-right: none;
  border-radius: 6px 0 0 6px;
  cursor: pointer;
  font-size: 10px;
  display: flex;
  align-items: center;
  justify-content: center;
  padding: 0;
  transition: right 0.15s ease;
}
.side-panel-toggle.panel-open {
  right: 180px;
}
.side-panel-toggle:hover {
  background: rgba(50, 50, 65, 0.95);
  color: #fff;
}

.side-panel {
  width: 180px;
  min-width: 180px;
  background: var(--bg-secondary);
  border-left: 1px solid var(--border-color);
  overflow-y: auto;
  overflow-x: hidden;
  padding: 6px;
  flex-shrink: 0;
  z-index: 400;
}
.side-section {
  margin-bottom: 8px;
  padding-bottom: 6px;
  border-bottom: 1px solid var(--border-color);
}
.side-section:last-child { border-bottom: none; }
.side-label {
  font-size: 10px;
  color: var(--text-muted);
  text-transform: uppercase;
  letter-spacing: 0.5px;
  margin-bottom: 4px;
  padding: 0 2px;
}
.side-btn {
  display: block;
  width: 100%;
  text-align: left;
  padding: 5px 8px;
  font-size: 11px;
  background: var(--bg-input);
  color: var(--text-primary);
  border: 1px solid transparent;
  border-radius: 4px;
  cursor: pointer;
  margin-bottom: 2px;
}
.side-btn:hover { background: var(--btn-secondary-bg, #333); border-color: var(--border-color); }
.side-btn.active {
  background: var(--btn-active-bg, #2563eb);
  color: #fff;
  border-color: var(--btn-active-border, #3b82f6);
}
.side-btn.btn-rec {
  background: var(--danger, #ef4444);
  color: #fff;
}
.side-btn-row { display: flex; gap: 2px; margin-bottom: 2px; }
.side-btn-sm {
  flex: 1;
  padding: 3px 4px;
  font-size: 10px;
  background: var(--bg-input);
  color: var(--text-muted);
  border: 1px solid transparent;
  border-radius: 3px;
  cursor: pointer;
  text-align: center;
}
.side-btn-sm.active {
  background: var(--btn-active-bg);
  color: #fff;
  border-color: var(--btn-active-border);
}
.side-hr { margin: 4px 0; border: none; border-top: 1px solid var(--border-color); }

.annotation-color-picker { display: inline-flex; align-items: center; cursor: pointer; margin-bottom: 2px; }
.annotation-color-picker input[type="color"] {
  width: 100%; height: 24px;
  border: 1px solid var(--btn-secondary-border);
  border-radius: 4px; padding: 0; background: none; cursor: pointer;
}

/* ─── 弹窗（录制/回放/重启确认/快捷键帮助）─── */
.recording-overlay, .reboot-confirm-overlay {
  position: fixed; top: 0; left: 0; right: 0; bottom: 0;
  background: rgba(0,0,0,0.5);
  display: flex; align-items: center; justify-content: center;
  z-index: 9999;
}
.recording-dialog {
  background: var(--bg-primary); border-radius: 10px;
  width: 400px; max-height: 500px; overflow: auto;
}
.recording-header {
  display: flex; justify-content: space-between; align-items: center;
  padding: 12px 16px; border-bottom: 1px solid var(--border-color);
}
.recording-header h3 { font-size: 14px; color: var(--text-primary); margin: 0; }
.recording-list { padding: 8px; }
.recording-item {
  display: flex; align-items: center; gap: 8px; padding: 8px; border-radius: 6px;
}
.recording-item:hover { background: var(--bg-secondary); }
.recording-name { flex: 1; font-size: 13px; color: var(--text-primary); }
.recording-size { font-size: 11px; color: var(--text-muted); }
.playback-dialog {
  background: var(--bg-primary); border-radius: 10px; width: 720px; max-width: 90vw;
}
.playback-video { width: 100%; max-height: 60vh; border-radius: 0 0 10px 10px; }
.key-help-list { margin-top: 12px; }
.key-help-row {
  display: flex; justify-content: space-between; align-items: center;
  padding: 6px 0; border-bottom: 1px solid var(--border-color);
}
.key-help-row kbd {
  background: var(--bg-input); padding: 2px 8px; border-radius: 4px;
  font-size: 12px; font-family: monospace; border: 1px solid var(--border-color);
}
.key-help-row span { font-size: 13px; color: var(--text-muted); }

.reboot-confirm-overlay { background: rgba(0,0,0,0.7); }
.reboot-confirm-dialog {
  background: var(--bg-secondary); border-radius: 12px; padding: 28px;
  min-width: 320px; text-align: center;
  border: 1px solid var(--border-color); box-shadow: 0 8px 32px rgba(0,0,0,0.5);
}
.reboot-confirm-dialog h3 { font-size: 16px; color: var(--text-primary); margin-bottom: 12px; }
.reboot-confirm-dialog p { font-size: 13px; color: var(--text-secondary); margin-bottom: 20px; }
.reboot-confirm-actions { display: flex; gap: 12px; justify-content: center; }
.reboot-confirm-actions button { min-width: 80px; padding: 8px 20px; font-size: 14px; }

/* ─── 移动端 ─── */
.mobile-fab { position: fixed; bottom: 24px; right: 24px; z-index: 200; }
.mobile-fab button {
  width: 48px; height: 48px; border-radius: 50%;
  background: var(--accent, #3b82f6); color: #fff; font-size: 22px;
  border: none; cursor: pointer; box-shadow: 0 2px 8px rgba(0,0,0,0.3);
}
.virtual-keyboard {
  position: fixed; bottom: 80px; right: 12px;
  display: flex; flex-wrap: wrap; gap: 4px; max-width: 320px;
  background: var(--bg-secondary); border: 1px solid var(--border-color);
  border-radius: 8px; padding: 8px; z-index: 200;
  box-shadow: 0 4px 16px rgba(0,0,0,0.3);
}
.virtual-keyboard button {
  padding: 6px 10px; font-size: 12px;
  background: var(--bg-input); color: var(--text-primary);
  border: 1px solid var(--border-color); border-radius: 4px; cursor: pointer;
}
.virtual-keyboard button:active { background: var(--accent, #3b82f6); color: #fff; }
</style>
