import { describe, it, expect, vi, beforeEach } from "vitest";

// 模拟 AudioContext
const mockChannelData = new Float32Array(100);
const mockAudioBuffer = {
  getChannelData: vi.fn().mockReturnValue(mockChannelData),
  duration: 100 / 48000, // samples / sampleRate
  numberOfChannels: 2,
  length: 100,
  sampleRate: 48000,
  copyFromChannel: vi.fn(),
  copyToChannel: vi.fn(),
};
const mockSource = {
  buffer: null as typeof mockAudioBuffer | null,
  connect: vi.fn(),
  start: vi.fn(),
  stop: vi.fn(),
  disconnect: vi.fn(),
  addEventListener: vi.fn(),
  removeEventListener: vi.fn(),
  dispatchEvent: vi.fn(),
  onended: null,
  channelCount: 2,
  channelCountMode: "max" as ChannelCountMode,
  channelInterpretation: "speakers" as ChannelInterpretation,
  context: {} as BaseAudioContext,
  numberOfInputs: 0,
  numberOfOutputs: 1,
  loop: false,
  loopStart: 0,
  loopEnd: 0,
  playbackRate: { value: 1 } as AudioParam,
  detune: { value: 0 } as AudioParam,
};
const mockAudioContext = {
  state: "running" as AudioContextState,
  currentTime: 0,
  sampleRate: 48000,
  createBuffer: vi.fn().mockReturnValue(mockAudioBuffer),
  createBufferSource: vi.fn().mockReturnValue(mockSource),
  destination: {} as AudioDestinationNode,
  resume: vi.fn().mockResolvedValue(undefined),
  close: vi.fn().mockResolvedValue(undefined),
};

vi.stubGlobal(
  "AudioContext",
  vi.fn().mockImplementation(() => ({
    ...mockAudioContext,
    state: "running" as AudioContextState,
    currentTime: 0,
    createBuffer: vi.fn().mockReturnValue({
      ...mockAudioBuffer,
      getChannelData: vi.fn().mockReturnValue(new Float32Array(100)),
    }),
    createBufferSource: vi.fn().mockReturnValue({
      ...mockSource,
      buffer: null,
      connect: vi.fn(),
      start: vi.fn(),
    }),
    resume: vi.fn().mockResolvedValue(undefined),
    close: vi.fn().mockResolvedValue(undefined),
  })),
);

import { useAudio } from "../useAudio";

describe("useAudio", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  describe("setFormat", () => {
    it("创建新的 AudioContext 并初始化状态", () => {
      const { setFormat } = useAudio();
      setFormat(44100, 2);
      expect(AudioContext).toHaveBeenCalledWith({ sampleRate: 44100 });
    });

    it("使用不同采样率调用", () => {
      const { setFormat } = useAudio();
      setFormat(48000, 1);
      expect(AudioContext).toHaveBeenCalledWith({ sampleRate: 48000 });
    });

    it("多次调用 setFormat 关闭旧的 AudioContext", () => {
      const { setFormat } = useAudio();
      setFormat(44100, 2);
      const firstCtx = (AudioContext as unknown as ReturnType<typeof vi.fn>).mock
        .results[0].value;

      setFormat(48000, 1);
      // 旧的 context 应被关闭
      expect(firstCtx.close).toHaveBeenCalled();
    });

    it("suspended 状态下自动 resume", () => {
      // 让 AudioContext 返回 suspended 状态
      (AudioContext as unknown as ReturnType<typeof vi.fn>).mockImplementationOnce(
        () => ({
          ...mockAudioContext,
          state: "suspended" as AudioContextState,
          currentTime: 0,
          createBuffer: vi.fn().mockReturnValue(mockAudioBuffer),
          createBufferSource: vi.fn().mockReturnValue({ ...mockSource }),
          resume: vi.fn().mockResolvedValue(undefined),
          close: vi.fn().mockResolvedValue(undefined),
        }),
      );

      const { setFormat } = useAudio();
      setFormat(48000, 2);

      const ctx = (AudioContext as unknown as ReturnType<typeof vi.fn>).mock
        .results[0].value;
      expect(ctx.resume).toHaveBeenCalled();
    });
  });

  describe("destroy", () => {
    it("关闭 AudioContext", () => {
      const { setFormat, destroy } = useAudio();
      setFormat(48000, 2);
      const ctx = (AudioContext as unknown as ReturnType<typeof vi.fn>).mock
        .results[0].value;

      destroy();
      expect(ctx.close).toHaveBeenCalled();
    });

    it("未初始化时调用 destroy 不报错", () => {
      const { destroy } = useAudio();
      // 没有调用 setFormat，audioCtx 为 null
      expect(() => destroy()).not.toThrow();
    });

    it("多次调用 destroy 不报错", () => {
      const { setFormat, destroy } = useAudio();
      setFormat(48000, 2);
      destroy();
      // 第二次调用，audioCtx 已经为 null
      expect(() => destroy()).not.toThrow();
    });
  });

  describe("playPcm", () => {
    it("未初始化时调用 playPcm 不报错", () => {
      const { playPcm } = useAudio();
      const pcm = new Uint8Array([0x00, 0x40, 0x00, 0xc0]);
      expect(() => playPcm(pcm)).not.toThrow();
    });

    it("audioEnabled 为 false 时不播放", () => {
      const { setFormat, playPcm, audioEnabled } = useAudio();
      setFormat(48000, 1);
      audioEnabled.value = false;

      const pcm = new Uint8Array([0x00, 0x40, 0x00, 0xc0]);
      playPcm(pcm);

      const ctx = (AudioContext as unknown as ReturnType<typeof vi.fn>).mock
        .results[0].value;
      expect(ctx.createBuffer).not.toHaveBeenCalled();
    });

    it("空 PCM 数据（< 2 字节）不处理", () => {
      const { setFormat, playPcm } = useAudio();
      setFormat(48000, 1);

      playPcm(new Uint8Array([]));
      playPcm(new Uint8Array([0x00]));

      const ctx = (AudioContext as unknown as ReturnType<typeof vi.fn>).mock
        .results[0].value;
      expect(ctx.createBuffer).not.toHaveBeenCalled();
    });

    it("有效 PCM 数据会创建缓冲区并播放", () => {
      const { setFormat, playPcm } = useAudio();
      setFormat(48000, 1);

      // 4 字节 = 2 个 16-bit 样本，单声道
      const pcm = new Uint8Array([0x00, 0x40, 0x00, 0xc0]);
      playPcm(pcm);

      const ctx = (AudioContext as unknown as ReturnType<typeof vi.fn>).mock
        .results[0].value;
      expect(ctx.createBuffer).toHaveBeenCalledWith(
        1, // channels
        2, // Math.floor(2 samples / 1 channel)
        48000, // sampleRate
      );
      expect(ctx.createBufferSource).toHaveBeenCalled();
    });

    it("双声道 PCM 数据正确分配通道", () => {
      const { setFormat, playPcm } = useAudio();
      setFormat(48000, 2);

      // 8 字节 = 4 个 16-bit 样本，双声道 → 每通道 2 个样本
      const pcm = new Uint8Array([
        0x00, 0x40, // L sample 0
        0x00, 0xc0, // R sample 0
        0x00, 0x20, // L sample 1
        0x00, 0xe0, // R sample 1
      ]);
      playPcm(pcm);

      const ctx = (AudioContext as unknown as ReturnType<typeof vi.fn>).mock
        .results[0].value;
      expect(ctx.createBuffer).toHaveBeenCalledWith(
        2, // channels
        2, // Math.floor(4 / 2)
        48000,
      );
    });
  });

  describe("PCM 16-bit LE 到 Float32 转换逻辑", () => {
    it("0x4000 (16384) 转换为 16384/32768 = 0.5", () => {
      // 直接验证转换公式：getInt16(idx, true) / 32768.0
      const pcm = new Uint8Array([0x00, 0x40]); // LE: 0x4000 = 16384
      const view = new DataView(pcm.buffer, pcm.byteOffset, pcm.byteLength);
      const sample = view.getInt16(0, true) / 32768.0;
      expect(sample).toBeCloseTo(0.5, 5);
    });

    it("0xC000 (-16384) 转换为 -16384/32768 = -0.5", () => {
      const pcm = new Uint8Array([0x00, 0xc0]); // LE: 0xC000 = -16384
      const view = new DataView(pcm.buffer, pcm.byteOffset, pcm.byteLength);
      const sample = view.getInt16(0, true) / 32768.0;
      expect(sample).toBeCloseTo(-0.5, 5);
    });

    it("0x7FFF (32767) 转换为接近 1.0", () => {
      const pcm = new Uint8Array([0xff, 0x7f]); // LE: 0x7FFF = 32767
      const view = new DataView(pcm.buffer, pcm.byteOffset, pcm.byteLength);
      const sample = view.getInt16(0, true) / 32768.0;
      expect(sample).toBeCloseTo(32767 / 32768, 5);
    });

    it("0x8000 (-32768) 转换为 -1.0", () => {
      const pcm = new Uint8Array([0x00, 0x80]); // LE: 0x8000 = -32768
      const view = new DataView(pcm.buffer, pcm.byteOffset, pcm.byteLength);
      const sample = view.getInt16(0, true) / 32768.0;
      expect(sample).toBe(-1.0);
    });

    it("0x0000 (静音) 转换为 0.0", () => {
      const pcm = new Uint8Array([0x00, 0x00]);
      const view = new DataView(pcm.buffer, pcm.byteOffset, pcm.byteLength);
      const sample = view.getInt16(0, true) / 32768.0;
      expect(sample).toBe(0.0);
    });

    it("Little-endian 字节序正确解析", () => {
      // 0x0100 LE → 0x0001 = 1 → 1/32768
      const pcm = new Uint8Array([0x01, 0x00]);
      const view = new DataView(pcm.buffer, pcm.byteOffset, pcm.byteLength);
      const sample = view.getInt16(0, true) / 32768.0;
      expect(sample).toBeCloseTo(1 / 32768, 10);
    });
  });

  describe("audioEnabled", () => {
    it("默认为 true", () => {
      const { audioEnabled } = useAudio();
      expect(audioEnabled.value).toBe(true);
    });

    it("可以切换为 false", () => {
      const { audioEnabled } = useAudio();
      audioEnabled.value = false;
      expect(audioEnabled.value).toBe(false);
    });
  });
});
