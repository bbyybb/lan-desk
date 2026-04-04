import { createApp } from "vue";
import App from "./App.vue";
import "./styles/main.css";
import { verifyIntegrity, showTamperWarning } from "./integrity";
import { useTheme } from "./composables/useTheme";
import { useSettings } from "./composables/useSettings";

// 根据保存的语言设置 HTML lang 属性
const savedLocale = localStorage.getItem("lan-desk-locale") || "zh";
document.documentElement.lang = savedLocale === "zh" ? "zh-CN" : "en";

// 初始化主题
const { load: loadSettings } = useSettings();
loadSettings();
const { applyTheme } = useTheme();
applyTheme();

const app = createApp(App);
app.config.errorHandler = (err, _instance, info) => {
  console.error(`[Vue Error] ${info}:`, err);
};
app.mount("#app");

// 完整性检查钩子供内联调用
let _c = 0;
window.__ci = () => { _c++; if (_c % 5 === 0 && !verifyIntegrity()) showTamperWarning("ci"); };
