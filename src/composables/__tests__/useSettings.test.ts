import { describe, it, expect, beforeEach, vi } from "vitest";

// 使用动态 import 以便在 load() 测试中重置模块单例
let useSettings: typeof import("../useSettings").useSettings;

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

describe("useSettings", () => {
  beforeEach(async () => {
    localStorageMock.clear();
    vi.clearAllMocks();
    // 重新加载模块以重置模块级单例状态（loaded = false）
    vi.resetModules();
    const mod = await import("../useSettings");
    useSettings = mod.useSettings;
    // 也通过 reset 恢复默认值
    const { reset } = useSettings();
    reset();
  });

  it("返回默认设置", () => {
    const { settings, DEFAULTS } = useSettings();
    expect(settings.value.port).toBe(DEFAULTS.port);
    expect(settings.value.jpegQuality).toBe(DEFAULTS.jpegQuality);
    expect(settings.value.maxFps).toBe(DEFAULTS.maxFps);
    expect(settings.value.autoReconnect).toBe(true);
    expect(settings.value.clipboardSync).toBe(true);
  });

  it("save 将设置持久化到 localStorage（密码字段清空）", () => {
    const { settings, save } = useSettings();
    settings.value.port = 12345;
    settings.value.controlPassword = "secret";
    save();

    expect(localStorageMock.setItem).toHaveBeenCalled();
    const saved = JSON.parse(
      localStorageMock.setItem.mock.calls[0][1] as string
    );
    expect(saved.port).toBe(12345);
    // 密码不应被持久化
    expect(saved.controlPassword).toBe("");
    expect(saved.viewPassword).toBe("");
  });

  it("reset 恢复默认值", () => {
    const { settings, reset, DEFAULTS } = useSettings();
    settings.value.port = 9999;
    settings.value.maxFps = 60;
    reset();

    expect(settings.value.port).toBe(DEFAULTS.port);
    expect(settings.value.maxFps).toBe(DEFAULTS.maxFps);
  });

  describe("load", () => {
    it("从 localStorage 加载有效保存的设置", async () => {
      // 重置模块以获得全新的 loaded = false 状态
      vi.resetModules();
      const savedSettings = {
        port: 12345,
        jpegQuality: 90,
        maxFps: 60,
        autoReconnect: false,
        maxReconnectAttempts: 10,
        bandwidthLimit: 1000,
        clipboardSync: false,
        shellEnabled: false,
        autoAccept: true,
        fixedPassword: true,
        controlPassword: "",
        viewPassword: "",
      };
      store["lan-desk-settings"] = JSON.stringify(savedSettings);

      const mod = await import("../useSettings");
      const { settings, load } = mod.useSettings();
      load();

      expect(settings.value.port).toBe(12345);
      expect(settings.value.jpegQuality).toBe(90);
      expect(settings.value.maxFps).toBe(60);
      expect(settings.value.autoReconnect).toBe(false);
      expect(settings.value.maxReconnectAttempts).toBe(10);
      expect(settings.value.clipboardSync).toBe(false);
      expect(settings.value.autoAccept).toBe(true);
    });

    it("部分保存的设置只覆盖已有键，其余保持默认", async () => {
      vi.resetModules();
      store["lan-desk-settings"] = JSON.stringify({ port: 8080, maxFps: 15 });

      const mod = await import("../useSettings");
      const { settings, load, DEFAULTS } = mod.useSettings();
      load();

      expect(settings.value.port).toBe(8080);
      expect(settings.value.maxFps).toBe(15);
      // 未保存的键应保持默认值
      expect(settings.value.jpegQuality).toBe(DEFAULTS.jpegQuality);
      expect(settings.value.autoReconnect).toBe(DEFAULTS.autoReconnect);
      expect(settings.value.clipboardSync).toBe(DEFAULTS.clipboardSync);
    });

    it("损坏的 JSON 不会导致崩溃，保持默认值", async () => {
      vi.resetModules();
      store["lan-desk-settings"] = "这不是有效的 JSON {{{";

      const mod = await import("../useSettings");
      const { settings, load, DEFAULTS } = mod.useSettings();

      expect(() => load()).not.toThrow();
      expect(settings.value.port).toBe(DEFAULTS.port);
      expect(settings.value.maxFps).toBe(DEFAULTS.maxFps);
    });

    it("非对象 JSON（字符串）不会覆盖设置", async () => {
      vi.resetModules();
      store["lan-desk-settings"] = JSON.stringify("hello");

      const mod = await import("../useSettings");
      const { settings, load, DEFAULTS } = mod.useSettings();

      expect(() => load()).not.toThrow();
      expect(settings.value.port).toBe(DEFAULTS.port);
    });

    it("非对象 JSON（null）不会覆盖设置", async () => {
      vi.resetModules();
      store["lan-desk-settings"] = JSON.stringify(null);

      const mod = await import("../useSettings");
      const { settings, load, DEFAULTS } = mod.useSettings();

      expect(() => load()).not.toThrow();
      expect(settings.value.port).toBe(DEFAULTS.port);
    });

    it("非对象 JSON（数组）不会覆盖设置", async () => {
      vi.resetModules();
      store["lan-desk-settings"] = JSON.stringify([1, 2, 3]);

      const mod = await import("../useSettings");
      const { settings, load, DEFAULTS } = mod.useSettings();

      expect(() => load()).not.toThrow();
      // 数组虽然 typeof 是 object 且不是 null，但数组的 key 是索引
      // 不会匹配 DEFAULTS 的 key，所以设置保持默认
      expect(settings.value.port).toBe(DEFAULTS.port);
    });

    it("密码不从 localStorage 加载（save 时已清空）", async () => {
      vi.resetModules();
      // 模拟保存时密码被清空的数据
      store["lan-desk-settings"] = JSON.stringify({
        port: 25605,
        controlPassword: "",
        viewPassword: "",
      });

      const mod = await import("../useSettings");
      const { settings, load } = mod.useSettings();
      load();

      expect(settings.value.controlPassword).toBe("");
      expect(settings.value.viewPassword).toBe("");
    });

    it("load 时强制清空密码字段，防止 localStorage 注入", async () => {
      vi.resetModules();
      // 假设有人手动修改了 localStorage
      store["lan-desk-settings"] = JSON.stringify({
        controlPassword: "hackedvalue",
        viewPassword: "hackedvalue2",
      });

      const mod = await import("../useSettings");
      const { settings, load } = mod.useSettings();
      load();

      // load 会强制清空密码字段，防止外部注入
      expect(settings.value.controlPassword).toBe("");
      expect(settings.value.viewPassword).toBe("");
    });

    it("单例行为：多次调用 load 只加载一次", async () => {
      vi.resetModules();
      store["lan-desk-settings"] = JSON.stringify({ port: 11111 });

      const mod = await import("../useSettings");
      const { settings, load } = mod.useSettings();
      load();
      expect(settings.value.port).toBe(11111);

      // 修改 localStorage 中的值
      store["lan-desk-settings"] = JSON.stringify({ port: 22222 });
      load(); // 第二次调用不应重新加载
      expect(settings.value.port).toBe(11111); // 仍为第一次加载的值
    });

    it("单例行为：两次 useSettings() 返回同一个 ref", async () => {
      vi.resetModules();
      const mod = await import("../useSettings");
      const { settings: s1 } = mod.useSettings();
      const { settings: s2 } = mod.useSettings();

      expect(s1).toBe(s2); // 同一个 ref 引用
      s1.value.port = 54321;
      expect(s2.value.port).toBe(54321);
    });

    it("localStorage 为空时使用默认值", async () => {
      vi.resetModules();
      // store 已被 clear，没有 lan-desk-settings

      const mod = await import("../useSettings");
      const { settings, load, DEFAULTS } = mod.useSettings();
      load();

      expect(settings.value.port).toBe(DEFAULTS.port);
      expect(settings.value.jpegQuality).toBe(DEFAULTS.jpegQuality);
      expect(settings.value.maxFps).toBe(DEFAULTS.maxFps);
      expect(settings.value.autoReconnect).toBe(DEFAULTS.autoReconnect);
    });
  });
});
