import { ref } from "vue";

/**
 * FPS / 带宽 / 延迟统计 composable
 */
export function useStats() {
  const fps = ref(0);
  const latency = ref(0);
  const bandwidth = ref("0");
  const rtt = ref(0);
  const networkQuality = ref<"good" | "fair" | "poor">("good");
  const cpuUsage = ref(0);
  const memUsage = ref(0);
  const memTotalMb = ref(0);

  // 延迟合理性上限（毫秒），超过此值的时间戳差视为无效
  const MAX_REASONABLE_LATENCY_MS = 10000;

  let frameCount = 0;
  let bytesReceived = 0;
  let fpsTimer: ReturnType<typeof setInterval> | null = null;

  function countFrame() {
    frameCount++;
  }

  function addBytes(n: number) {
    bytesReceived += n;
  }

  function updateLatency(timestampMs: number) {
    if (timestampMs > 0) {
      const diff = Date.now() - timestampMs;
      if (diff >= 0 && diff < MAX_REASONABLE_LATENCY_MS) {
        latency.value = diff;
      }
    }
  }

  const rttHistory = ref<number[]>([]);

  function updateRtt(value: number) {
    rtt.value = value;
    if (value < 30) networkQuality.value = "good";
    else if (value < 100) networkQuality.value = "fair";
    else networkQuality.value = "poor";
    // 保留最近 60 个采样点
    rttHistory.value.push(value);
    if (rttHistory.value.length > 60) rttHistory.value.shift();
  }

  function updateSystemInfo(cpu: number, memory: number, memTotal?: number) {
    cpuUsage.value = Math.round(cpu);
    memUsage.value = Math.round(memory);
    if (memTotal !== undefined && memTotal > 0) memTotalMb.value = memTotal;
  }

  function formatBandwidth(bytesPerSec: number): string {
    if (bytesPerSec > 1024 * 1024) {
      return (bytesPerSec / 1024 / 1024).toFixed(1) + " MB/s";
    }
    return (bytesPerSec / 1024).toFixed(0) + " KB/s";
  }

  function startTimer() {
    fpsTimer = setInterval(() => {
      fps.value = frameCount;
      frameCount = 0;
      bandwidth.value = formatBandwidth(bytesReceived);
      bytesReceived = 0;
    }, 1000);
  }

  function stopTimer() {
    if (fpsTimer) {
      clearInterval(fpsTimer);
      fpsTimer = null;
    }
  }

  return {
    fps,
    latency,
    bandwidth,
    rtt,
    rttHistory,
    networkQuality,
    cpuUsage,
    memUsage,
    memTotalMb,
    countFrame,
    addBytes,
    updateLatency,
    updateRtt,
    updateSystemInfo,
    startTimer,
    stopTimer,
  };
}
