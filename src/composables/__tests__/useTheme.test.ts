import { describe, it, expect, beforeEach, vi } from "vitest";

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

// 模拟 window.matchMedia
let prefersDark = false;
Object.defineProperty(window, "matchMedia", {
  writable: true,
  value: vi.fn((query: string) => ({
    matches: query === "(prefers-color-scheme: dark)" ? prefersDark : false,
    media: query,
    onchange: null,
    addListener: vi.fn(),
    removeListener: vi.fn(),
    addEventListener: vi.fn(),
    removeEventListener: vi.fn(),
    dispatchEvent: vi.fn(),
  })),
});

describe("useTheme", () => {
  let useTheme: typeof import("../useTheme").useTheme;
  let useSettings: typeof import("../useSettings").useSettings;

  beforeEach(async () => {
    localStorageMock.clear();
    vi.clearAllMocks();
    document.documentElement.removeAttribute("data-theme");

    // 每次重新加载模块，重置 useSettings 单例
    vi.resetModules();
    const settingsMod = await import("../useSettings");
    useSettings = settingsMod.useSettings;
    const themeMod = await import("../useTheme");
    useTheme = themeMod.useTheme;
  });

  it("theme 为 dark 时设置 data-theme=\"dark\"", () => {
    const { settings, load } = useSettings();
    load();
    settings.value.theme = "dark";

    const { applyTheme } = useTheme();
    applyTheme();

    expect(document.documentElement.getAttribute("data-theme")).toBe("dark");
  });

  it("theme 为 light 时设置 data-theme=\"light\"", () => {
    const { settings, load } = useSettings();
    load();
    settings.value.theme = "light";

    const { applyTheme } = useTheme();
    applyTheme();

    expect(document.documentElement.getAttribute("data-theme")).toBe("light");
  });

  it("theme 为 system 且系统偏好为 dark 时设置 data-theme=\"dark\"", () => {
    prefersDark = true;
    const { settings, load } = useSettings();
    load();
    settings.value.theme = "system";

    const { applyTheme } = useTheme();
    applyTheme();

    expect(document.documentElement.getAttribute("data-theme")).toBe("dark");
  });

  it("theme 为 system 且系统偏好为 light 时设置 data-theme=\"light\"", () => {
    prefersDark = false;
    const { settings, load } = useSettings();
    load();
    settings.value.theme = "system";

    const { applyTheme } = useTheme();
    applyTheme();

    expect(document.documentElement.getAttribute("data-theme")).toBe("light");
  });

  it("多次调用 applyTheme 正确切换", () => {
    const { settings, load } = useSettings();
    load();
    const { applyTheme } = useTheme();

    settings.value.theme = "dark";
    applyTheme();
    expect(document.documentElement.getAttribute("data-theme")).toBe("dark");

    settings.value.theme = "light";
    applyTheme();
    expect(document.documentElement.getAttribute("data-theme")).toBe("light");
  });

  it("默认 theme 为 dark", () => {
    const { settings, load } = useSettings();
    load();
    // useSettings 默认 theme 为 "dark"
    expect(settings.value.theme).toBe("dark");

    const { applyTheme } = useTheme();
    applyTheme();
    expect(document.documentElement.getAttribute("data-theme")).toBe("dark");
  });
});
