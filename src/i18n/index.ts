import { ref } from "vue";
import zh from "./zh.json";
import en from "./en.json";

type Messages = Record<string, string>;

const messages: Record<string, Messages> = { zh, en };
// 自动检测系统语言：优先使用用户保存的偏好，否则检测浏览器/系统语言，默认英文
function detectLocale(): string {
  const saved = localStorage.getItem("lan-desk-locale");
  if (saved && (saved === "zh" || saved === "en")) return saved;
  try {
    const lang = navigator.language || navigator.languages?.[0] || "";
    if (lang.toLowerCase().startsWith("zh")) return "zh";
  } catch (_) { /* ignored */ }
  return "en";
}
const currentLocale = ref(detectLocale());
// 响应式版本号，每次 locale 变化时递增，触发所有使用 t() 的计算属性重新计算
const localeVersion = ref(0);

export function useI18n() {
  function t(key: string, params?: Record<string, string | number>): string {
    // 访问 localeVersion 以建立响应式依赖
    void localeVersion.value;
    let text = messages[currentLocale.value]?.[key] || messages["zh"][key] || key;
    if (params) {
      for (const [k, v] of Object.entries(params)) {
        text = text.replace(new RegExp(`\\{${k}\\}`, "g"), String(v));
      }
    }
    return text;
  }

  function setLocale(locale: string) {
    if (!messages[locale]) return; // ignore unsupported locales
    currentLocale.value = locale;
    localeVersion.value++;
    localStorage.setItem("lan-desk-locale", locale);
    document.documentElement.lang = locale === "zh" ? "zh-CN" : "en";
  }

  /** 仅预览语言切换（不写 localStorage），供设置页即时预览 */
  function previewLocale(locale: string) {
    if (!messages[locale]) return;
    currentLocale.value = locale;
    localeVersion.value++;
    document.documentElement.lang = locale === "zh" ? "zh-CN" : "en";
  }

  /** 从 localStorage 恢复语言，丢弃未持久化的预览 */
  function restoreLocale() {
    const saved = localStorage.getItem("lan-desk-locale") || "en";
    currentLocale.value = saved;
    localeVersion.value++;
    document.documentElement.lang = saved === "zh" ? "zh-CN" : "en";
  }

  return {
    t,
    locale: currentLocale,
    setLocale,
    previewLocale,
    restoreLocale,
    availableLocales: ["zh", "en"] as const,
  };
}

/** Error code to i18n key mapping */
const ERROR_CODE_MAP: Record<string, string> = {
  ERR_ALREADY_CONNECTED: "error.already_connected",
  ERR_CONNECT_FAILED: "error.connect_failed",
  ERR_TLS_DOMAIN: "error.tls_domain",
  ERR_TLS_HANDSHAKE: "error.tls_handshake",
  ERR_SEND_HELLO: "error.send_hello",
  ERR_HANDSHAKE_CLOSED: "error.handshake_closed",
  ERR_RECV_ACK: "error.recv_ack",
  ERR_REJECTED: "error.rejected",
  ERR_UNEXPECTED_RESPONSE: "error.unexpected_response",
  ERR_NO_PIN: "error.no_pin",
  ERR_WS_NOT_STARTED: "error.ws_not_started",
  ERR_DATA_DIR: "error.data_dir",
  ERR_HOST_NOT_FOUND: "error.host_not_found",
  ERR_TRUST_NOT_INIT: "error.trust_not_init",
  ERR_SERVER_NOT_RUNNING: "error.server_not_running",
  ERR_CONTROL_PIN_LENGTH: "error.control_pin_length",
  ERR_VIEW_PIN_LENGTH: "error.view_pin_length",
  ERR_PINS_SAME: "error.pins_same",
  ERR_DISCOVERY_BIND: "error.discovery_bind",
  ERR_DISCOVERY_FAILED: "error.discovery_failed",
  ERR_INVALID_MAC: "error.invalid_mac",
  ERR_UDP_BIND: "error.udp_bind",
  ERR_BROADCAST: "error.broadcast",
  ERR_WOL_SEND: "error.wol_send",
  ERR_LIST_MONITORS: "error.list_monitors",
  ERR_SEND_SHELL: "error.send_shell",
  ERR_NOT_CONNECTED: "error.not_connected",
  ERR_FILE_NOT_FOUND: "error.file_not_found",
  ERR_FILE_METADATA: "error.file_metadata",
  ERR_SEND_FAILED: "error.send_failed",
};

/**
 * Translate backend error messages to current locale.
 * Expects format: "[ERR_CODE] English description" or "[ERR_CODE] prefix: detail"
 */
export function translateError(error: unknown): string {
  const raw = String(error);
  const match = raw.match(/^\[([A-Z_]+)\]\s*(.*)$/);
  if (!match) return raw; // Not a coded error, return as-is

  const [, code, rest] = match;
  const i18nKey = ERROR_CODE_MAP[code];
  if (!i18nKey) return raw;

  // Extract detail after first ": "
  const colonIdx = rest.indexOf(": ");
  const detail = colonIdx >= 0 ? rest.slice(colonIdx + 2) : "";

  // Use module-level locale state directly (no need for useI18n() at call site)
  void localeVersion.value;
  let text = messages[currentLocale.value]?.[i18nKey] || messages["zh"][i18nKey] || i18nKey;
  if (detail) {
    text = text.replace(/\{detail\}/g, detail);
  }
  return text;
}
