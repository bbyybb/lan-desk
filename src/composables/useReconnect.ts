import { ref } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { useSettings } from "./useSettings";

/**
 * 自动重连 composable（指数退避 + 重启重连模式）
 */
export function useReconnect(addr: string, onGiveUp: () => void) {
  const { settings } = useSettings();
  const isReconnecting = ref(false);
  const reconnectCount = ref(0);
  const isRebootReconnecting = ref(false);
  const rebootCountdown = ref(0);
  let timerId: ReturnType<typeof setTimeout> | null = null;
  let countdownTimerId: ReturnType<typeof setInterval> | null = null;

  function attempt() {
    // 重连期间忽略重复触发
    if (isReconnecting.value) return;

    const maxAttempts = settings.value.maxReconnectAttempts;
    const autoReconnect = settings.value.autoReconnect;

    if (autoReconnect && reconnectCount.value < maxAttempts) {
      isReconnecting.value = true;
      reconnectCount.value++;
      const delay = Math.min(1000 * Math.pow(2, reconnectCount.value - 1), 16000);
      timerId = setTimeout(async () => {
        timerId = null;
        try {
          const role = localStorage.getItem("lan-desk-last-role") || "controller";
          await invoke("reconnect_to_peer", { addr, role });
          isReconnecting.value = false;
          reconnectCount.value = 0;
        } catch (_) {
          isReconnecting.value = false;
          if (reconnectCount.value >= maxAttempts) {
            onGiveUp();
          } else {
            // 未达最大次数，继续下一次重连尝试
            attempt();
          }
        }
      }, delay);
    } else {
      onGiveUp();
    }
  }

  /**
   * 重启重连模式：等待 estimatedSeconds 后以固定 5 秒间隔重试，最大 60 次
   */
  function rebootReconnect(estimatedSeconds: number) {
    cancel();
    isRebootReconnecting.value = true;
    reconnectCount.value = 0;
    rebootCountdown.value = estimatedSeconds;

    // 倒计时
    countdownTimerId = setInterval(() => {
      rebootCountdown.value--;
      if (rebootCountdown.value <= 0) {
        if (countdownTimerId) clearInterval(countdownTimerId);
        countdownTimerId = null;
      }
    }, 1000);

    // 等待预计时间后开始重连
    timerId = setTimeout(() => {
      if (countdownTimerId) clearInterval(countdownTimerId);
      countdownTimerId = null;
      rebootCountdown.value = 0;
      doRebootRetry();
    }, estimatedSeconds * 1000);
  }

  function doRebootRetry() {
    const maxRebootAttempts = 60;

    if (reconnectCount.value >= maxRebootAttempts) {
      isRebootReconnecting.value = false;
      onGiveUp();
      return;
    }

    isReconnecting.value = true;
    reconnectCount.value++;

    timerId = setTimeout(async () => {
      timerId = null;
      try {
        const role = localStorage.getItem("lan-desk-last-role") || "controller";
        await invoke("reconnect_to_peer", { addr, role });
        isReconnecting.value = false;
        isRebootReconnecting.value = false;
        reconnectCount.value = 0;
      } catch (_) {
        isReconnecting.value = false;
        doRebootRetry();
      }
    }, 5000);
  }

  function resetCount() {
    reconnectCount.value = 0;
    isReconnecting.value = false;
    isRebootReconnecting.value = false;
    rebootCountdown.value = 0;
  }

  function cancel() {
    if (timerId) {
      clearTimeout(timerId);
      timerId = null;
    }
    if (countdownTimerId) {
      clearInterval(countdownTimerId);
      countdownTimerId = null;
    }
    isReconnecting.value = false;
    isRebootReconnecting.value = false;
    rebootCountdown.value = 0;
  }

  return {
    isReconnecting,
    reconnectCount,
    isRebootReconnecting,
    rebootCountdown,
    attempt,
    rebootReconnect,
    resetCount,
    cancel,
  };
}
