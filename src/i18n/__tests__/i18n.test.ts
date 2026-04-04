import { describe, it, expect, beforeEach, vi } from "vitest";
import zh from "../zh.json";
import en from "../en.json";

// 模拟 localStorage
const store: Record<string, string> = {};
const localStorageMock = {
  getItem: vi.fn((key: string) => store[key] ?? null),
  setItem: vi.fn((key: string, value: string) => {
    store[key] = value;
  }),
  removeItem: vi.fn((key: string) => {
    delete store[key];
  }),
  clear: vi.fn(() => {
    for (const key of Object.keys(store)) delete store[key];
  }),
  get length() {
    return Object.keys(store).length;
  },
  key: vi.fn((_index: number) => null),
};

Object.defineProperty(globalThis, "localStorage", { value: localStorageMock });

// 模拟 document.documentElement.lang
Object.defineProperty(document.documentElement, "lang", {
  value: "",
  writable: true,
});

describe("i18n 模块", () => {
  beforeEach(async () => {
    localStorageMock.clear();
    // 设置默认中文（测试环境 navigator.language 为 en-US，需要显式指定）
    store["lan-desk-locale"] = "zh";
    vi.clearAllMocks();
    // 每次测试前重新加载模块，确保 currentLocale 从 localStorage 重新读取
    vi.resetModules();
  });

  async function getI18n() {
    const mod = await import("../index");
    return mod.useI18n();
  }

  it("默认语言为中文，t() 返回中文文本", async () => {
    const { t } = await getI18n();
    expect(t("app.title")).toBe("LAN-Desk");
    expect(t("discovery.refresh")).toBe("刷新");
    expect(t("settings.save")).toBe("保存");
  });

  it("未知 key 返回 key 本身", async () => {
    const { t } = await getI18n();
    expect(t("unknown.key.not_exist")).toBe("unknown.key.not_exist");
    expect(t("")).toBe("");
  });

  it("参数替换正确执行", async () => {
    const { t } = await getI18n();
    // zh.json 中 "discovery.waiting": "等待连接中 (端口 {port})"
    expect(t("discovery.waiting", { port: 25605 })).toBe(
      "等待连接中 (端口 25605)",
    );
    // 测试多参数替换
    expect(t("remote.reconnecting", { count: 3 })).toBe(
      "正在重新连接... (第 3 次)",
    );
    // 字符串参数
    expect(t("toast.connected", { addr: "192.168.1.10" })).toBe(
      "已连接到 192.168.1.10",
    );
  });

  it("setLocale('en') 切换到英文后 t() 返回英文文本", async () => {
    const { t, setLocale, locale } = await getI18n();
    setLocale("en");

    expect(locale.value).toBe("en");
    expect(t("discovery.refresh")).toBe("Refresh");
    expect(t("settings.save")).toBe("Save");
    expect(t("remote.disconnect")).toBe("Disconnect");
    // 验证参数替换在英文模式下也正常
    expect(t("discovery.waiting", { port: 8080 })).toBe(
      "Listening on port 8080",
    );
  });

  it("setLocale('zh') 切换回中文", async () => {
    const { t, setLocale, locale } = await getI18n();
    setLocale("en");
    expect(t("discovery.refresh")).toBe("Refresh");

    setLocale("zh");
    expect(locale.value).toBe("zh");
    expect(t("discovery.refresh")).toBe("刷新");
  });

  it("setLocale 将 locale 持久化到 localStorage", async () => {
    const { setLocale } = await getI18n();
    setLocale("en");
    expect(localStorageMock.setItem).toHaveBeenCalledWith(
      "lan-desk-locale",
      "en",
    );
  });

  it("setLocale 更新 document.documentElement.lang", async () => {
    const { setLocale } = await getI18n();
    setLocale("en");
    expect(document.documentElement.lang).toBe("en");
    setLocale("zh");
    expect(document.documentElement.lang).toBe("zh-CN");
  });

  it("zh.json 和 en.json 拥有完全相同的 key 集合（无遗漏翻译）", () => {
    const zhKeys = Object.keys(zh).sort();
    const enKeys = Object.keys(en).sort();

    // 检查中文有但英文没有的 key
    const missingInEn = zhKeys.filter((k) => !enKeys.includes(k));
    // 检查英文有但中文没有的 key
    const missingInZh = enKeys.filter((k) => !zhKeys.includes(k));

    expect(missingInEn).toEqual([]);
    expect(missingInZh).toEqual([]);
    expect(zhKeys).toEqual(enKeys);
  });

  it("availableLocales 包含 ['zh', 'en']", async () => {
    const { availableLocales } = await getI18n();
    expect(availableLocales).toEqual(["zh", "en"]);
    expect(availableLocales).toContain("zh");
    expect(availableLocales).toContain("en");
  });

  it("setLocale 传入不支持的语言不会改变当前 locale", async () => {
    const { setLocale, locale } = await getI18n();
    expect(locale.value).toBe("zh"); // 默认中文

    setLocale("fr"); // 法语不支持
    expect(locale.value).toBe("zh"); // 应保持不变

    setLocale("de"); // 德语不支持
    expect(locale.value).toBe("zh");

    // 验证切换到有效 locale 仍然工作
    setLocale("en");
    expect(locale.value).toBe("en");

    setLocale("xyz"); // 无效 locale
    expect(locale.value).toBe("en"); // 应保持 en
  });
});

describe("translateError", () => {
  beforeEach(async () => {
    localStorageMock.clear();
    store["lan-desk-locale"] = "zh";
    vi.clearAllMocks();
    vi.resetModules();
  });

  async function getTranslateError() {
    const mod = await import("../index");
    return mod.translateError;
  }

  async function getI18n() {
    const mod = await import("../index");
    return mod.useI18n();
  }

  it("已知错误码翻译为中文（默认 locale）", async () => {
    const translateError = await getTranslateError();
    const result = translateError("[ERR_ALREADY_CONNECTED] Already connected");
    expect(result).toBe("已有活跃连接，请先断开");
  });

  it("带 detail 后缀的错误码正确替换 {detail}", async () => {
    const translateError = await getTranslateError();
    const result = translateError(
      "[ERR_CONNECT_FAILED] Connection failed: timeout after 5s",
    );
    expect(result).toBe("连接失败: timeout after 5s");
  });

  it("ERR_TLS_HANDSHAKE 带 detail 正确翻译", async () => {
    const translateError = await getTranslateError();
    const result = translateError(
      "[ERR_TLS_HANDSHAKE] TLS handshake: certificate expired",
    );
    expect(result).toBe("TLS 握手失败: certificate expired");
  });

  it("未知错误码返回原始消息", async () => {
    const translateError = await getTranslateError();
    const result = translateError("[ERR_UNKNOWN_CODE] Something happened");
    expect(result).toBe("[ERR_UNKNOWN_CODE] Something happened");
  });

  it("非编码格式的错误返回原始消息", async () => {
    const translateError = await getTranslateError();
    const result = translateError("Some plain error message");
    expect(result).toBe("Some plain error message");
  });

  it("非 Error 类型输入被 String() 转换后返回", async () => {
    const translateError = await getTranslateError();
    expect(translateError(42)).toBe("42");
    expect(translateError(true)).toBe("true");
    expect(translateError({ toString: () => "custom obj" })).toBe("custom obj");
  });

  it("null 和 undefined 输入被 String() 转换", async () => {
    const translateError = await getTranslateError();
    expect(translateError(null)).toBe("null");
    expect(translateError(undefined)).toBe("undefined");
  });

  it("没有 detail 部分的错误码正确翻译（无 {detail} 占位符）", async () => {
    const translateError = await getTranslateError();
    // ERR_ALREADY_CONNECTED 的翻译没有 {detail} 占位符
    const result = translateError("[ERR_ALREADY_CONNECTED] some description");
    expect(result).toBe("已有活跃连接，请先断开");
  });

  it("切换到英文 locale 后翻译为英文", async () => {
    const translateError = await getTranslateError();
    const { setLocale } = await getI18n();

    setLocale("en");
    const result = translateError("[ERR_NOT_CONNECTED] Not connected");
    expect(result).toBe("Not connected");
  });

  it("带 detail 的错误在英文 locale 下正确替换", async () => {
    const translateError = await getTranslateError();
    const { setLocale } = await getI18n();

    setLocale("en");
    const result = translateError(
      "[ERR_CONNECT_FAILED] Connection failed: host unreachable",
    );
    expect(result).toBe("Connection failed: host unreachable");
  });
});
