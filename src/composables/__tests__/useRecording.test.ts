import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { useRecording } from "../useRecording";

// 模拟 MediaRecorder
const mockMediaRecorder = {
  start: vi.fn(),
  stop: vi.fn(),
  state: "inactive" as RecordingState,
  ondataavailable: null as ((e: BlobEvent) => void) | null,
  onstop: null as (() => void) | null,
};

const mockStream = {
  getTracks: vi.fn().mockReturnValue([
    { stop: vi.fn() },
  ]),
};

const mockCanvas = {
  captureStream: vi.fn().mockReturnValue(mockStream),
} as unknown as HTMLCanvasElement;

vi.stubGlobal(
  "MediaRecorder",
  Object.assign(
    vi.fn().mockImplementation(() => {
      const instance = { ...mockMediaRecorder, state: "inactive" as RecordingState };
      instance.start = vi.fn().mockImplementation(() => {
        instance.state = "recording";
      });
      instance.stop = vi.fn().mockImplementation(() => {
        instance.state = "inactive";
        if (instance.onstop) instance.onstop();
      });
      return instance;
    }),
    {
      isTypeSupported: vi.fn().mockReturnValue(true),
    },
  ),
);

vi.stubGlobal("URL", {
  createObjectURL: vi.fn().mockReturnValue("blob:mock-url"),
  revokeObjectURL: vi.fn(),
});

describe("useRecording", () => {
  let recording: ReturnType<typeof useRecording>;

  beforeEach(() => {
    vi.clearAllMocks();
    vi.useFakeTimers();
    recording = useRecording();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it("初始状态正确", () => {
    expect(recording.isRecording.value).toBe(false);
    expect(recording.canRecord.value).toBe(true);
  });

  it("toggle 开始录制", () => {
    recording.toggle(mockCanvas);
    expect(recording.isRecording.value).toBe(true);
    expect(mockCanvas.captureStream).toHaveBeenCalledWith(30);
  });

  it("toggle 接受自定义 fps 参数", () => {
    recording.toggle(mockCanvas, 60);
    expect(mockCanvas.captureStream).toHaveBeenCalledWith(60);
  });

  it("toggle 两次：开始然后停止", () => {
    recording.toggle(mockCanvas);
    expect(recording.isRecording.value).toBe(true);

    recording.toggle(mockCanvas);
    expect(recording.isRecording.value).toBe(false);
  });

  it("MediaRecorder 不可用时 canRecord 设为 false", () => {
    const origMR = globalThis.MediaRecorder;
    // @ts-expect-error -- 临时移除 MediaRecorder
    delete globalThis.MediaRecorder;
    vi.stubGlobal("MediaRecorder", undefined);

    const rec = useRecording();
    rec.toggle(mockCanvas);
    expect(rec.canRecord.value).toBe(false);
    expect(rec.isRecording.value).toBe(false);

    vi.stubGlobal("MediaRecorder", origMR);
  });

  it("captureStream 不可用时 canRecord 设为 false", () => {
    const badCanvas = {} as HTMLCanvasElement;
    recording.toggle(badCanvas);
    expect(recording.canRecord.value).toBe(false);
  });

  it("停止录制时触发文件下载", async () => {
    const clickSpy = vi.fn();
    vi.spyOn(document, "createElement").mockReturnValue({
      href: "",
      download: "",
      click: clickSpy,
    } as unknown as HTMLAnchorElement);

    recording.toggle(mockCanvas);
    recording.toggle(mockCanvas); // stop
    // onstop 是 async，等待 microtask 完成
    await vi.advanceTimersByTimeAsync(0);

    expect(clickSpy).toHaveBeenCalled();
    expect(URL.createObjectURL).toHaveBeenCalled();
  });

  it("revokeObjectURL 在 5 秒后被调用", async () => {
    const clickSpy = vi.fn();
    vi.spyOn(document, "createElement").mockReturnValue({
      href: "",
      download: "",
      click: clickSpy,
    } as unknown as HTMLAnchorElement);

    recording.toggle(mockCanvas);
    recording.toggle(mockCanvas); // stop
    await vi.advanceTimersByTimeAsync(0);

    expect(URL.revokeObjectURL).not.toHaveBeenCalled();
    await vi.advanceTimersByTimeAsync(5000);
    expect(URL.revokeObjectURL).toHaveBeenCalledWith("blob:mock-url");
  });

  it("destroy 清理资源", () => {
    recording.toggle(mockCanvas);
    recording.destroy();

    // stream tracks 应被停止
    for (const track of mockStream.getTracks()) {
      expect(track.stop).toHaveBeenCalled();
    }
  });

  it("destroy 在未录制时不报错", () => {
    expect(() => recording.destroy()).not.toThrow();
  });

  it("MIME 类型降级：VP9 不支持时使用 video/webm", () => {
    (MediaRecorder.isTypeSupported as ReturnType<typeof vi.fn>).mockImplementation(
      (type: string) => type === "video/webm",
    );

    recording.toggle(mockCanvas);
    expect(recording.isRecording.value).toBe(true);
  });

  it("所有 MIME 类型都不支持时 canRecord 设为 false", () => {
    (MediaRecorder.isTypeSupported as ReturnType<typeof vi.fn>).mockReturnValue(false);

    recording.toggle(mockCanvas);
    expect(recording.canRecord.value).toBe(false);
    expect(recording.isRecording.value).toBe(false);
  });

  it("默认 fps 为 30", () => {
    recording.toggle(mockCanvas);
    expect(mockCanvas.captureStream).toHaveBeenCalledWith(30);
  });
});
