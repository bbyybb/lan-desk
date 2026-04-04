import { describe, it, expect, beforeEach, vi } from "vitest";
import { ref } from "vue";

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

// 模拟 @tauri-apps/api/core
const mockInvoke = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => mockInvoke(...args),
}));

// 模拟 useSettings 返回可控值
const mockSettings = ref({
  autoReconnect: true,
  maxReconnectAttempts: 5,
});

vi.mock("../useSettings", () => ({
  useSettings: () => ({
    settings: mockSettings,
  }),
}));

import { useReconnect } from "../useReconnect";

describe("useReconnect", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.useFakeTimers();
    localStorageMock.clear();
    // 恢复默认设置
    mockSettings.value = {
      autoReconnect: true,
      maxReconnectAttempts: 5,
    };
    mockInvoke.mockResolvedValue(undefined);
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it("初始状态：isReconnecting 为 false，reconnectCount 为 0", () => {
    const { isReconnecting, reconnectCount } = useReconnect(
      "192.168.1.1",
      vi.fn(),
    );
    expect(isReconnecting.value).toBe(false);
    expect(reconnectCount.value).toBe(0);
  });

  it("resetCount 重置 isReconnecting 和 reconnectCount", () => {
    const { isReconnecting, reconnectCount, attempt, resetCount } =
      useReconnect("192.168.1.1", vi.fn());

    // 触发一次重连以改变状态
    attempt();
    expect(isReconnecting.value).toBe(true);
    expect(reconnectCount.value).toBe(1);

    resetCount();
    expect(isReconnecting.value).toBe(false);
    expect(reconnectCount.value).toBe(0);
  });

  it("指数退避延迟计算：Math.min(1000 * 2^(n-1), 16000)", () => {
    // 验证延迟公式：第1次=1000, 第2次=2000, 第3次=4000, 第4次=8000, 第5次=16000
    const expectedDelays = [1000, 2000, 4000, 8000, 16000];

    for (let n = 1; n <= 5; n++) {
      const delay = Math.min(1000 * Math.pow(2, n - 1), 16000);
      expect(delay).toBe(expectedDelays[n - 1]);
    }
  });

  it("attempt 使用正确的指数退避延迟调度重连", async () => {
    const onGiveUp = vi.fn();
    const { attempt, reconnectCount, isReconnecting } = useReconnect(
      "192.168.1.1",
      onGiveUp,
    );

    // 第1次重连尝试，延迟应为 1000ms
    attempt();
    expect(isReconnecting.value).toBe(true);
    expect(reconnectCount.value).toBe(1);

    // 在 999ms 时 invoke 还不应被调用
    await vi.advanceTimersByTimeAsync(999);
    expect(mockInvoke).not.toHaveBeenCalled();

    // 在 1000ms 时 invoke 应被调用
    await vi.advanceTimersByTimeAsync(1);
    expect(mockInvoke).toHaveBeenCalledWith("reconnect_to_peer", {
      addr: "192.168.1.1",
      role: "controller",
    });
  });

  it("重连成功后重置状态", async () => {
    mockInvoke.mockResolvedValue(undefined);
    const { attempt, isReconnecting, reconnectCount } = useReconnect(
      "192.168.1.1",
      vi.fn(),
    );

    attempt();
    await vi.advanceTimersByTimeAsync(1000);

    // 等待 Promise 解析
    await vi.waitFor(() => {
      expect(isReconnecting.value).toBe(false);
      expect(reconnectCount.value).toBe(0);
    });
  });

  it("autoReconnect 为 false 时，attempt 直接调用 onGiveUp", () => {
    mockSettings.value.autoReconnect = false;
    const onGiveUp = vi.fn();
    const { attempt } = useReconnect("192.168.1.1", onGiveUp);

    attempt();
    expect(onGiveUp).toHaveBeenCalledTimes(1);
  });

  it("达到 maxReconnectAttempts 后调用 onGiveUp", async () => {
    mockSettings.value.maxReconnectAttempts = 2;
    mockInvoke.mockRejectedValue(new Error("连接失败"));
    const onGiveUp = vi.fn();
    const { attempt } = useReconnect("192.168.1.1", onGiveUp);

    // 第1次尝试
    attempt();
    await vi.advanceTimersByTimeAsync(1000);
    // 等待 Promise 拒绝
    await vi.waitFor(() => {
      expect(onGiveUp).not.toHaveBeenCalled();
    });

    // 第2次尝试（达到上限）
    attempt();
    await vi.advanceTimersByTimeAsync(2000);
    await vi.waitFor(() => {
      expect(onGiveUp).toHaveBeenCalledTimes(1);
    });
  });

  it("重连期间重复调用 attempt 被忽略", () => {
    const { attempt, reconnectCount } = useReconnect(
      "192.168.1.1",
      vi.fn(),
    );

    attempt();
    attempt(); // 应被忽略
    attempt(); // 应被忽略

    expect(reconnectCount.value).toBe(1);
  });

  it("使用 localStorage 中存储的 role 进行重连", async () => {
    store["lan-desk-last-role"] = "viewer";
    const { attempt } = useReconnect("192.168.1.1", vi.fn());

    attempt();
    await vi.advanceTimersByTimeAsync(1000);

    expect(mockInvoke).toHaveBeenCalledWith("reconnect_to_peer", {
      addr: "192.168.1.1",
      role: "viewer",
    });
  });
});

describe("rebootReconnect", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.useFakeTimers();
    localStorageMock.clear();
    mockSettings.value = {
      autoReconnect: true,
      maxReconnectAttempts: 5,
    };
    mockInvoke.mockResolvedValue(undefined);
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it("调用 rebootReconnect(30) 应设置 isRebootReconnecting 为 true", () => {
    const { rebootReconnect, isRebootReconnecting, rebootCountdown } =
      useReconnect("192.168.1.1", vi.fn());

    rebootReconnect(30);
    expect(isRebootReconnecting.value).toBe(true);
    expect(rebootCountdown.value).toBe(30);
  });

  it("应在等待后以固定间隔重试", async () => {
    const { rebootReconnect } = useReconnect("192.168.1.1", vi.fn());

    rebootReconnect(10);

    // 等待预计的 10 秒
    await vi.advanceTimersByTimeAsync(10000);

    // 等待第一次重试的 5 秒间隔
    await vi.advanceTimersByTimeAsync(5000);

    expect(mockInvoke).toHaveBeenCalledWith("reconnect_to_peer", {
      addr: "192.168.1.1",
      role: "controller",
    });
  });

  it("cancel 应停止重连", () => {
    const { rebootReconnect, cancel, isRebootReconnecting, rebootCountdown } =
      useReconnect("192.168.1.1", vi.fn());

    rebootReconnect(30);
    expect(isRebootReconnecting.value).toBe(true);

    cancel();
    expect(isRebootReconnecting.value).toBe(false);
    expect(rebootCountdown.value).toBe(0);
  });
});
