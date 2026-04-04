<script setup lang="ts">
import { ref, onMounted, onUnmounted, nextTick } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { useI18n } from "../i18n";

const { t } = useI18n();

let sharedAudioCtx: AudioContext | null = null;

const props = defineProps<{
  visible: boolean;
}>();

const emit = defineEmits<{
  close: [];
}>();

interface ChatMsg {
  text: string;
  sender: string;
  timestamp_ms: number;
  isSelf: boolean;
}

const messages = ref<ChatMsg[]>([]);
const inputText = ref("");
const chatBody = ref<HTMLDivElement | null>(null);
let unlistenChat: UnlistenFn | null = null;
const selfHostname = ref("self");

async function sendMessage() {
  const text = inputText.value.trim();
  if (!text) return;
  try {
    await invoke("send_chat_message", { text });
    messages.value.push({
      text,
      sender: selfHostname.value,
      timestamp_ms: Date.now(),
      isSelf: true,
    });
    inputText.value = "";
    await nextTick();
    scrollToBottom();
  } catch (_) {
    // 忽略发送失败
  }
}

function scrollToBottom() {
  if (chatBody.value) {
    chatBody.value.scrollTop = chatBody.value.scrollHeight;
  }
}

function formatTime(ts: number): string {
  const d = new Date(ts);
  return d.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
}

function onKeydown(e: KeyboardEvent) {
  if (e.key === "Enter" && !e.shiftKey) {
    e.preventDefault();
    sendMessage();
  }
}

onMounted(async () => {
  unlistenChat = await listen<{ text: string; sender: string; timestamp_ms: number }>(
    "chat-message",
    (event) => {
      messages.value.push({
        ...event.payload,
        isSelf: false,
      });
      nextTick(() => scrollToBottom());
      // 通知音：窗口不聚焦或面板不可见时播放提示音
      if (!document.hasFocus() || !props.visible) {
        try {
          if (!sharedAudioCtx || sharedAudioCtx.state === "closed") {
            sharedAudioCtx = new AudioContext();
          }
          const osc = sharedAudioCtx.createOscillator();
          const gain = sharedAudioCtx.createGain();
          osc.connect(gain);
          gain.connect(sharedAudioCtx.destination);
          osc.frequency.value = 800;
          gain.gain.value = 0.1;
          osc.start();
          osc.stop(sharedAudioCtx.currentTime + 0.15);
        } catch (_) { /* ignored */ }
      }
    }
  );
});

onUnmounted(() => {
  unlistenChat?.();
  if (sharedAudioCtx) {
    sharedAudioCtx.close().catch(() => {});
    sharedAudioCtx = null;
  }
});
</script>

<template>
  <div
    v-if="visible"
    class="chat-panel"
  >
    <div class="chat-header">
      <span>{{ t("chat.title") }}</span>
      <button
        class="chat-close"
        @click="emit('close')"
      >
        &times;
      </button>
    </div>
    <div
      ref="chatBody"
      class="chat-body"
    >
      <div
        v-for="(msg, index) in messages"
        :key="msg.timestamp_ms + '-' + index"
        class="chat-msg"
        :class="{ 'chat-msg-self': msg.isSelf }"
      >
        <div class="chat-meta">
          <span class="chat-sender">{{ msg.isSelf ? t("chat.you") : msg.sender }}</span>
          <span class="chat-time">{{ formatTime(msg.timestamp_ms) }}</span>
        </div>
        <div class="chat-text">
          {{ msg.text }}
        </div>
      </div>
      <div
        v-if="messages.length === 0"
        class="chat-empty"
      >
        {{ t("chat.empty") }}
      </div>
    </div>
    <div class="chat-input">
      <input
        v-model="inputText"
        type="text"
        :placeholder="t('chat.placeholder')"
        @keydown="onKeydown"
      >
      <button
        class="btn-primary chat-send"
        @click="sendMessage"
      >
        {{ t("chat.send") }}
      </button>
    </div>
  </div>
</template>

<style scoped>
.chat-panel {
  position: fixed;
  right: 0;
  top: 0;
  bottom: 0;
  width: 300px;
  background: var(--bg-secondary);
  border-left: 1px solid var(--border-color);
  display: flex;
  flex-direction: column;
  z-index: 9000;
}

.chat-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
  padding: 10px 12px;
  border-bottom: 1px solid var(--border-color);
  font-size: 14px;
  color: var(--text-secondary);
  font-weight: 600;
}

.chat-close {
  background: none;
  border: none;
  color: var(--text-muted);
  font-size: 20px;
  cursor: pointer;
  padding: 0 4px;
  line-height: 1;
}
.chat-close:hover {
  color: var(--text-primary);
}

.chat-body {
  flex: 1;
  overflow-y: auto;
  padding: 10px;
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.chat-empty {
  text-align: center;
  color: var(--text-dim);
  font-size: 13px;
  margin-top: 40px;
}

.chat-msg {
  max-width: 85%;
  align-self: flex-start;
}

.chat-msg-self {
  align-self: flex-end;
}

.chat-meta {
  display: flex;
  gap: 8px;
  font-size: 11px;
  color: var(--text-dim);
  margin-bottom: 2px;
}

.chat-sender {
  font-weight: 600;
}

.chat-text {
  background: var(--bg-input);
  color: var(--text-secondary);
  padding: 6px 10px;
  border-radius: 8px;
  font-size: 13px;
  word-break: break-word;
  white-space: pre-wrap;
}

.chat-msg-self .chat-text {
  background: var(--chat-self-bg);
}

.chat-input {
  display: flex;
  gap: 6px;
  padding: 8px 10px;
  border-top: 1px solid var(--border-color);
}

.chat-input input {
  flex: 1;
  padding: 6px 10px;
  border: 1px solid var(--border-color);
  border-radius: 6px;
  background: var(--bg-input);
  color: var(--text-primary);
  font-size: 13px;
  outline: none;
}

.chat-send {
  padding: 6px 12px;
  font-size: 13px;
}
</style>
