<script setup lang="ts">
import { ref, onMounted } from "vue";
import { useI18n } from "../i18n";

const emit = defineEmits<{ close: [] }>();
const { t } = useI18n();
const activeTab = ref<"wechat" | "alipay" | "bmc">("wechat");

const _A = "白白LOVE尹尹";
const _D = { s: "LANDESK-bbloveyy-2026", u: "bbyybb" };
const _B = "https://www.buymeacoffee.com/bbyybb";
const _S = "https://github.com/sponsors/bbyybb/";

onMounted(() => {
  const el = document.querySelector('[data-author]');
  if (el && (!el.getAttribute('data-author')?.includes(_D.u) || !el.getAttribute('data-sig')?.includes(_D.s))) {
    while (document.body.firstChild) document.body.removeChild(document.body.firstChild);
    const div = document.createElement("div");
    div.style.cssText = "display:flex;align-items:center;justify-content:center;height:100vh;background:#1a1a2e;color:#e74c3c";
    const h2 = document.createElement("h2");
    h2.textContent = "\u5b8c\u6574\u6027\u9519\u8bef / Integrity Error";
    div.appendChild(h2);
    document.body.appendChild(div);
  }
});
</script>

<template>
  <div
    class="donate-overlay"
    @click.self="emit('close')"
  >
    <div
      class="donate-dialog"
      data-author="bbyybb"
      data-sig="LANDESK-bbloveyy-2026"
    >
      <div class="donate-header">
        <h2>{{ t("donate.title") }}</h2>
        <button
          class="close-btn"
          @click="emit('close')"
        >
          &times;
        </button>
      </div>

      <div class="donate-tabs">
        <button
          :class="{ active: activeTab === 'wechat' }"
          @click="activeTab = 'wechat'"
        >
          {{ t("donate.wechat") }}
        </button>
        <button
          :class="{ active: activeTab === 'alipay' }"
          @click="activeTab = 'alipay'"
        >
          {{ t("donate.alipay") }}
        </button>
        <button
          :class="{ active: activeTab === 'bmc' }"
          @click="activeTab = 'bmc'"
        >
          {{ t("donate.bmc") }}
        </button>
      </div>

      <div class="donate-content">
        <div
          v-if="activeTab === 'wechat'"
          class="qr-container"
        >
          <img
            src="/docs/wechat_pay.jpg"
            alt="微信支付"
            class="qr-img"
          >
          <p>{{ t("donate.wechat_hint") }}</p>
        </div>
        <div
          v-if="activeTab === 'alipay'"
          class="qr-container"
        >
          <img
            src="/docs/alipay.jpg"
            alt="支付宝"
            class="qr-img"
          >
          <p>{{ t("donate.alipay_hint") }}</p>
        </div>
        <div
          v-if="activeTab === 'bmc'"
          class="qr-container"
        >
          <a
            :href="_B"
            target="_blank"
            rel="noopener"
          >
            <img
              src="/docs/bmc_qr.png"
              alt="Buy Me a Coffee"
              class="qr-img"
            >
          </a>
          <p>
            <a
              :href="_B"
              target="_blank"
              rel="noopener"
            >buymeacoffee.com/bbyybb</a>
          </p>
        </div>
      </div>

      <div class="donate-links">
        <a
          :href="_B"
          target="_blank"
          rel="noopener"
        >☕ Buy Me a Coffee</a>
        <span class="sep">|</span>
        <a
          :href="_S"
          target="_blank"
          rel="noopener"
        >💖 GitHub Sponsors</a>
      </div>

      <div
        id="donateAuthorFooter"
        class="donate-footer"
        data-sig="LANDESK-bbloveyy-2026"
      >
        {{ t("donate.footer") }} <b>{{ _A }}</b>
      </div>
    </div>
  </div>
</template>

<style scoped>
.donate-overlay {
  position: fixed; top: 0; left: 0; right: 0; bottom: 0;
  background: rgba(0,0,0,0.75);
  display: flex; align-items: center; justify-content: center;
  z-index: 10001;
}
.donate-dialog {
  background: var(--bg-secondary); border-radius: 14px; padding: 0;
  min-width: 400px; max-width: 480px;
  border: 1px solid var(--border-color); box-shadow: 0 12px 40px rgba(0,0,0,0.6);
  overflow: hidden;
}
.donate-header {
  display: flex; justify-content: space-between; align-items: center;
  padding: 16px 20px; border-bottom: 1px solid var(--border-color);
}
.donate-header h2 { font-size: 17px; color: var(--text-primary); margin: 0; }
.close-btn {
  background: none; border: none; color: var(--text-muted); font-size: 24px;
  cursor: pointer; padding: 0 4px; line-height: 1;
}
.close-btn:hover { color: var(--text-primary); }
.donate-tabs {
  display: flex; border-bottom: 1px solid var(--border-color);
}
.donate-tabs button {
  flex: 1; padding: 10px; background: none; border: none;
  color: var(--text-muted); font-size: 13px; cursor: pointer;
  border-bottom: 2px solid transparent;
}
.donate-tabs button.active {
  color: var(--accent); border-bottom-color: var(--accent);
}
.donate-content { padding: 24px; }
.qr-container { text-align: center; }
.qr-img {
  width: 180px; height: 180px; border-radius: 8px;
  border: 2px solid var(--border-color); margin-bottom: 10px;
}
.qr-container p { color: var(--text-muted); font-size: 13px; }
.qr-container a { color: var(--accent); text-decoration: none; }
.donate-links {
  text-align: center; padding: 0 20px 16px; font-size: 13px;
}
.donate-links a { color: var(--accent); text-decoration: none; }
.donate-links a:hover { text-decoration: underline; }
.sep { color: var(--sep-color); margin: 0 8px; }
.donate-footer {
  text-align: center; padding: 12px;
  border-top: 1px solid var(--border-color);
  color: var(--text-dim); font-size: 12px;
}
.donate-footer b { color: var(--text-muted); }
</style>
