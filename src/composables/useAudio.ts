/**
 * 远程音频播放 composable
 *
 * 管理 AudioContext 生命周期，处理 PCM 16-bit LE 数据播放，
 * 内置 jitter buffer 平滑网络抖动。
 */

import { ref } from "vue";

const AUDIO_JITTER_BUFFER_MS = 100;

export function useAudio() {
  const audioEnabled = ref(true);

  let audioCtx: AudioContext | null = null;
  let audioSampleRate = 48000;
  let audioChannels = 2;
  let audioNextTime = 0;

  function setFormat(sampleRate: number, channels: number) {
    audioSampleRate = sampleRate;
    audioChannels = channels;
    // 关闭旧的 AudioContext 防止资源泄漏
    if (audioCtx && audioCtx.state !== "closed") {
      audioCtx.close().catch(() => {});
    }
    audioCtx = new AudioContext({ sampleRate: audioSampleRate });
    // 浏览器/WebView 策略要求用户交互后才能播放音频，主动 resume
    if (audioCtx.state === "suspended") {
      audioCtx.resume().catch(() => {});
    }
    audioNextTime = 0;
  }

  function playPcm(pcmBytes: Uint8Array) {
    if (!audioCtx || !audioEnabled.value) return;
    if (pcmBytes.length < 2) return;
    // 确保 AudioContext 处于运行状态
    if (audioCtx.state === "suspended") {
      audioCtx.resume().catch(() => {});
      // Don't return - continue to queue audio, it will play once resumed
    }
    const samples = pcmBytes.length / 2;
    const buffer = audioCtx.createBuffer(
      audioChannels,
      Math.floor(samples / audioChannels),
      audioSampleRate
    );
    const view = new DataView(pcmBytes.buffer, pcmBytes.byteOffset, pcmBytes.byteLength);

    for (let ch = 0; ch < audioChannels; ch++) {
      const channelData = buffer.getChannelData(ch);
      for (let i = 0; i < channelData.length; i++) {
        const idx = (i * audioChannels + ch) * 2;
        if (idx + 1 < pcmBytes.length) {
          channelData[i] = view.getInt16(idx, true) / 32768.0;
        }
      }
    }

    const source = audioCtx.createBufferSource();
    source.buffer = buffer;
    source.connect(audioCtx.destination);
    const now = audioCtx.currentTime;
    if (audioNextTime < now + AUDIO_JITTER_BUFFER_MS / 1000) {
      audioNextTime = now + AUDIO_JITTER_BUFFER_MS / 1000;
    }
    source.start(audioNextTime);
    audioNextTime += buffer.duration;
  }

  function destroy() {
    if (audioCtx && audioCtx.state !== "closed") {
      audioCtx.close().catch(() => {});
    }
    audioCtx = null;
  }

  return {
    audioEnabled,
    setFormat,
    playPcm,
    destroy,
  };
}
