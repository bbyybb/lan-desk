import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { useStats } from "../useStats";

describe("useStats", () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  describe("formatBandwidth", () => {
    // formatBandwidth 是内部函数，通过 startTimer 间接测试带宽格式化
    // 但我们可以通过 addBytes + startTimer 来观察 bandwidth ref 的变化

    it("0 字节显示为 0 KB/s", () => {
      const stats = useStats();
      stats.startTimer();
      // 不添加任何字节
      vi.advanceTimersByTime(1000);
      expect(stats.bandwidth.value).toBe("0 KB/s");
      stats.stopTimer();
    });

    it("小于 1 MB/s 时显示 KB/s", () => {
      const stats = useStats();
      stats.startTimer();
      stats.addBytes(1024); // 1 KB
      vi.advanceTimersByTime(1000);
      expect(stats.bandwidth.value).toBe("1 KB/s");
      stats.stopTimer();
    });

    it("100 字节显示为 0 KB/s（向下取整）", () => {
      const stats = useStats();
      stats.startTimer();
      stats.addBytes(100);
      vi.advanceTimersByTime(1000);
      expect(stats.bandwidth.value).toBe("0 KB/s");
      stats.stopTimer();
    });

    it("512 字节显示为 1 KB/s（toFixed(0) 四舍五入）", () => {
      const stats = useStats();
      stats.startTimer();
      stats.addBytes(512);
      vi.advanceTimersByTime(1000);
      // 512 / 1024 = 0.5, toFixed(0) 在 V8 中将 0.5 入为 "1"
      expect(stats.bandwidth.value).toBe("1 KB/s");
      stats.stopTimer();
    });

    it("超过 1 MB/s 时显示 MB/s", () => {
      const stats = useStats();
      stats.startTimer();
      stats.addBytes(1024 * 1024 + 1); // 刚好超过 1 MB
      vi.advanceTimersByTime(1000);
      expect(stats.bandwidth.value).toBe("1.0 MB/s");
      stats.stopTimer();
    });

    it("大带宽正确格式化", () => {
      const stats = useStats();
      stats.startTimer();
      stats.addBytes(5 * 1024 * 1024); // 5 MB
      vi.advanceTimersByTime(1000);
      expect(stats.bandwidth.value).toBe("5.0 MB/s");
      stats.stopTimer();
    });

    it("2.5 MB/s 正确显示", () => {
      const stats = useStats();
      stats.startTimer();
      stats.addBytes(2.5 * 1024 * 1024);
      vi.advanceTimersByTime(1000);
      expect(stats.bandwidth.value).toBe("2.5 MB/s");
      stats.stopTimer();
    });
  });

  describe("updateLatency", () => {
    it("正常延迟值被正确记录", () => {
      const stats = useStats();
      const now = Date.now();
      vi.setSystemTime(now);
      // timestampMs 比 now 早 50ms
      stats.updateLatency(now - 50);
      expect(stats.latency.value).toBe(50);
    });

    it("负 diff（时间戳在未来）被过滤", () => {
      const stats = useStats();
      const now = Date.now();
      vi.setSystemTime(now);
      // timestampMs 在未来 → diff < 0 → 不更新
      stats.updateLatency(now + 1000);
      expect(stats.latency.value).toBe(0); // 保持初始值
    });

    it("零时间戳被忽略（timestampMs <= 0 条件）", () => {
      const stats = useStats();
      stats.updateLatency(0);
      expect(stats.latency.value).toBe(0);
    });

    it("负时间戳被忽略", () => {
      const stats = useStats();
      stats.updateLatency(-100);
      expect(stats.latency.value).toBe(0);
    });

    it("diff 等于 0 时被接受", () => {
      const stats = useStats();
      const now = Date.now();
      vi.setSystemTime(now);
      stats.updateLatency(now); // diff = 0, 应该被接受 (diff >= 0 && diff < 10000)
      expect(stats.latency.value).toBe(0);
    });

    it("超大延迟（>= 10000ms）被过滤", () => {
      const stats = useStats();
      const now = Date.now();
      vi.setSystemTime(now);
      // timestampMs 比 now 早 10000ms → diff = 10000 → 不满足 diff < 10000
      stats.updateLatency(now - 10000);
      expect(stats.latency.value).toBe(0);
    });

    it("延迟 9999ms 被接受（刚好在阈值内）", () => {
      const stats = useStats();
      const now = Date.now();
      vi.setSystemTime(now);
      stats.updateLatency(now - 9999);
      expect(stats.latency.value).toBe(9999);
    });
  });

  describe("updateRtt / getQuality (networkQuality)", () => {
    it("RTT < 30ms 时网络质量为 good", () => {
      const stats = useStats();
      stats.updateRtt(0);
      expect(stats.networkQuality.value).toBe("good");

      stats.updateRtt(10);
      expect(stats.networkQuality.value).toBe("good");

      stats.updateRtt(29);
      expect(stats.networkQuality.value).toBe("good");
    });

    it("RTT = 30ms 时网络质量为 fair（边界值）", () => {
      const stats = useStats();
      stats.updateRtt(30);
      expect(stats.networkQuality.value).toBe("fair");
    });

    it("30 <= RTT < 100 时网络质量为 fair", () => {
      const stats = useStats();
      stats.updateRtt(50);
      expect(stats.networkQuality.value).toBe("fair");

      stats.updateRtt(99);
      expect(stats.networkQuality.value).toBe("fair");
    });

    it("RTT = 100ms 时网络质量为 poor（边界值）", () => {
      const stats = useStats();
      stats.updateRtt(100);
      expect(stats.networkQuality.value).toBe("poor");
    });

    it("RTT >= 100 时网络质量为 poor", () => {
      const stats = useStats();
      stats.updateRtt(200);
      expect(stats.networkQuality.value).toBe("poor");

      stats.updateRtt(1000);
      expect(stats.networkQuality.value).toBe("poor");
    });

    it("rtt ref 值正确更新", () => {
      const stats = useStats();
      stats.updateRtt(42);
      expect(stats.rtt.value).toBe(42);
    });
  });

  describe("rttHistory", () => {
    it("记录 RTT 历史并限制长度为 60", () => {
      const stats = useStats();
      for (let i = 0; i < 65; i++) {
        stats.updateRtt(i * 10);
      }
      expect(stats.rttHistory.value.length).toBe(60);
      // 最早的 5 个应被移除，第一个应是 50
      expect(stats.rttHistory.value[0]).toBe(50);
      expect(stats.rttHistory.value[59]).toBe(640);
    });

    it("初始为空数组", () => {
      const stats = useStats();
      expect(stats.rttHistory.value).toEqual([]);
    });
  });

  describe("addBytes", () => {
    it("字节数正确累加并在定时器周期后重置", () => {
      const stats = useStats();
      stats.startTimer();
      stats.addBytes(100);
      stats.addBytes(200);
      stats.addBytes(300);
      // 累计 600 字节
      vi.advanceTimersByTime(1000);
      expect(stats.bandwidth.value).toBe("1 KB/s"); // 600/1024 ≈ 0.59 → "1 KB/s"
      // 第二个周期没有新数据
      vi.advanceTimersByTime(1000);
      expect(stats.bandwidth.value).toBe("0 KB/s");
      stats.stopTimer();
    });
  });

  describe("fps 计数器 (countFrame)", () => {
    it("正确统计每秒帧数", () => {
      const stats = useStats();
      stats.startTimer();

      // 模拟 30 帧
      for (let i = 0; i < 30; i++) {
        stats.countFrame();
      }

      vi.advanceTimersByTime(1000);
      expect(stats.fps.value).toBe(30);

      stats.stopTimer();
    });

    it("每个周期帧数独立计算", () => {
      const stats = useStats();
      stats.startTimer();

      // 第一秒: 10 帧
      for (let i = 0; i < 10; i++) stats.countFrame();
      vi.advanceTimersByTime(1000);
      expect(stats.fps.value).toBe(10);

      // 第二秒: 60 帧
      for (let i = 0; i < 60; i++) stats.countFrame();
      vi.advanceTimersByTime(1000);
      expect(stats.fps.value).toBe(60);

      // 第三秒: 0 帧
      vi.advanceTimersByTime(1000);
      expect(stats.fps.value).toBe(0);

      stats.stopTimer();
    });
  });

  describe("startTimer / stopTimer", () => {
    it("startTimer 启动后定时更新 fps 和 bandwidth", () => {
      const stats = useStats();
      stats.startTimer();

      stats.countFrame();
      stats.addBytes(2048);
      vi.advanceTimersByTime(1000);

      expect(stats.fps.value).toBe(1);
      expect(stats.bandwidth.value).toBe("2 KB/s");

      stats.stopTimer();
    });

    it("stopTimer 后不再更新", () => {
      const stats = useStats();
      stats.startTimer();

      stats.countFrame();
      vi.advanceTimersByTime(1000);
      expect(stats.fps.value).toBe(1);

      stats.stopTimer();

      // 继续添加帧但 timer 已停止
      for (let i = 0; i < 100; i++) stats.countFrame();
      vi.advanceTimersByTime(5000);
      // fps 不应更新，仍为上次记录的值
      expect(stats.fps.value).toBe(1);
    });

    it("多次调用 stopTimer 不会报错", () => {
      const stats = useStats();
      stats.startTimer();
      stats.stopTimer();
      stats.stopTimer(); // 第二次调用应安全
      stats.stopTimer();
    });

    it("未启动 timer 时调用 stopTimer 不报错", () => {
      const stats = useStats();
      stats.stopTimer(); // 未启动时调用应安全
    });
  });

  describe("updateSystemInfo", () => {
    it("CPU 和内存使用率正确更新并取整", () => {
      const stats = useStats();
      stats.updateSystemInfo(45.6, 78.3);
      expect(stats.cpuUsage.value).toBe(46);
      expect(stats.memUsage.value).toBe(78);
    });

    it("整数值保持不变", () => {
      const stats = useStats();
      stats.updateSystemInfo(50, 60);
      expect(stats.cpuUsage.value).toBe(50);
      expect(stats.memUsage.value).toBe(60);
    });
  });
});
