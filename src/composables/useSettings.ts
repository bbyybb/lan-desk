import { ref } from "vue";

export interface Settings {
  port: number;
  jpegQuality: number;
  maxFps: number;
  autoReconnect: boolean;
  maxReconnectAttempts: number;
  bandwidthLimit: number;
  clipboardSync: boolean;
  shellEnabled: boolean;
  idleTimeoutMinutes: number;
  lockOnDisconnect: boolean;
  autoAccept: boolean;
  fixedPassword: boolean;
  controlPassword: string;
  viewPassword: string;
  theme: "dark" | "light" | "system";
  audioQuality: "low" | "medium" | "high";
}

const STORAGE_KEY = "lan-desk-settings";

const DEFAULTS: Settings = {
  port: 25605,
  jpegQuality: 75,
  maxFps: 30,
  autoReconnect: true,
  maxReconnectAttempts: 5,
  bandwidthLimit: 0,
  clipboardSync: true,
  shellEnabled: false,
  idleTimeoutMinutes: 30,
  lockOnDisconnect: false,
  autoAccept: false,
  fixedPassword: false,
  controlPassword: "",
  viewPassword: "",
  theme: "dark",
  audioQuality: "medium",
};

// 模块级单例 -- 所有组件共享同一份设置
const settings = ref<Settings>({ ...DEFAULTS });
let loaded = false;

export function useSettings() {
  function load() {
    if (loaded) return;
    const saved = localStorage.getItem(STORAGE_KEY);
    if (saved) {
      try {
        const parsed = JSON.parse(saved);
        if (typeof parsed === "object" && parsed !== null) {
          const keys = Object.keys(DEFAULTS) as (keyof Settings)[];
          for (const key of keys) {
            if (key in parsed) {
              (settings.value as Record<string, unknown>)[key] = parsed[key];
            }
          }
          // 安全：非固定密码模式下清空密码，防止外部注入
          // 固定密码模式下保留（用户明确设置了无人值守密码）
          if (!settings.value.fixedPassword) {
            settings.value.controlPassword = "";
            settings.value.viewPassword = "";
          }
        }
      } catch (_) { /* ignored */ }
    }
    loaded = true;
  }

  function save() {
    // 固定密码模式下保留密码（用户明确选择持久化），否则清空
    const forStorage = settings.value.fixedPassword
      ? { ...settings.value }
      : { ...settings.value, controlPassword: "", viewPassword: "" };
    localStorage.setItem(STORAGE_KEY, JSON.stringify(forStorage));
  }

  /** 从 localStorage 重新加载（忽略 loaded 守卫），用于丢弃未保存的修改 */
  function reload() {
    loaded = false;
    Object.assign(settings.value, { ...DEFAULTS });
    load();
  }

  function reset() {
    Object.assign(settings.value, { ...DEFAULTS });
    loaded = false;
  }

  return { settings, load, save, reload, reset, DEFAULTS };
}
