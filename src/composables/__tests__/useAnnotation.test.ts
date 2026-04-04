import { describe, it, expect, vi, beforeEach } from "vitest";
import { useAnnotation } from "../useAnnotation";

// 模拟 OffscreenCanvas
const mockOffscreenCtx = {
  clearRect: vi.fn(),
  save: vi.fn(),
  restore: vi.fn(),
  beginPath: vi.fn(),
  moveTo: vi.fn(),
  lineTo: vi.fn(),
  stroke: vi.fn(),
  strokeStyle: "",
  lineWidth: 1,
  lineCap: "butt" as CanvasLineCap,
  lineJoin: "miter" as CanvasLineJoin,
};

vi.stubGlobal(
  "OffscreenCanvas",
  vi.fn().mockImplementation((w: number, h: number) => ({
    width: w,
    height: h,
    getContext: vi.fn().mockReturnValue(mockOffscreenCtx),
  })),
);

describe("useAnnotation", () => {
  let annotation: ReturnType<typeof useAnnotation>;

  beforeEach(() => {
    vi.clearAllMocks();
    annotation = useAnnotation();
  });

  it("初始状态正确", () => {
    expect(annotation.isAnnotating.value).toBe(false);
    expect(annotation.annotationColor.value).toBe("#ff3333");
    expect(annotation.lineWidth.value).toBe(3);
    expect(annotation.isDrawing).toBe(false);
    expect(annotation.hasContent()).toBe(false);
  });

  it("startStroke 设置 isDrawing 为 true", () => {
    annotation.startStroke({ x: 0.1, y: 0.2 });
    expect(annotation.isDrawing).toBe(true);
  });

  it("addPoint 在非绘制状态下不生效", () => {
    annotation.addPoint({ x: 0.5, y: 0.5 });
    expect(annotation.isDrawing).toBe(false);
    expect(annotation.hasContent()).toBe(false);
  });

  it("endStroke 保存至少有 2 个点的笔划", () => {
    annotation.startStroke({ x: 0.1, y: 0.1 });
    annotation.addPoint({ x: 0.2, y: 0.2 });
    annotation.endStroke();

    expect(annotation.isDrawing).toBe(false);
    expect(annotation.hasContent()).toBe(true);
  });

  it("endStroke 丢弃只有 1 个点的笔划", () => {
    annotation.startStroke({ x: 0.1, y: 0.1 });
    annotation.endStroke();

    expect(annotation.isDrawing).toBe(false);
    expect(annotation.hasContent()).toBe(false);
  });

  it("clear() 清除所有标注", () => {
    annotation.startStroke({ x: 0, y: 0 });
    annotation.addPoint({ x: 0.5, y: 0.5 });
    annotation.endStroke();

    expect(annotation.hasContent()).toBe(true);
    annotation.clear();
    expect(annotation.hasContent()).toBe(false);
  });

  it("undo() 移除最后一条标注", () => {
    // 添加第一条
    annotation.startStroke({ x: 0, y: 0 });
    annotation.addPoint({ x: 0.1, y: 0.1 });
    annotation.endStroke();

    // 添加第二条
    annotation.startStroke({ x: 0.2, y: 0.2 });
    annotation.addPoint({ x: 0.3, y: 0.3 });
    annotation.endStroke();

    expect(annotation.hasContent()).toBe(true);

    annotation.undo();
    // 还有第一条
    expect(annotation.hasContent()).toBe(true);

    annotation.undo();
    // 全部撤销
    expect(annotation.hasContent()).toBe(false);
  });

  it("undo() 在无标注时不报错", () => {
    expect(() => annotation.undo()).not.toThrow();
    expect(annotation.hasContent()).toBe(false);
  });

  it("drawAll 在无标注时不调用 OffscreenCanvas", () => {
    const ctx = {
      drawImage: vi.fn(),
    } as unknown as CanvasRenderingContext2D;

    annotation.drawAll(ctx, 1920, 1080);
    expect(ctx.drawImage).not.toHaveBeenCalled();
  });

  it("drawAll 在有标注时调用 OffscreenCanvas 并绘制", () => {
    annotation.startStroke({ x: 0, y: 0 });
    annotation.addPoint({ x: 0.5, y: 0.5 });
    annotation.endStroke();

    const ctx = {
      drawImage: vi.fn(),
    } as unknown as CanvasRenderingContext2D;

    annotation.drawAll(ctx, 1920, 1080);

    // 创建 OffscreenCanvas
    expect(OffscreenCanvas).toHaveBeenCalledWith(1920, 1080);
    // 清除 + 绘制
    expect(mockOffscreenCtx.clearRect).toHaveBeenCalledWith(0, 0, 1920, 1080);
    expect(mockOffscreenCtx.beginPath).toHaveBeenCalled();
    expect(mockOffscreenCtx.moveTo).toHaveBeenCalled();
    expect(mockOffscreenCtx.lineTo).toHaveBeenCalled();
    expect(mockOffscreenCtx.stroke).toHaveBeenCalled();
    // 最终绘制到主 canvas
    expect(ctx.drawImage).toHaveBeenCalled();
  });

  it("drawAll 绘制正在进行中的笔划", () => {
    annotation.startStroke({ x: 0, y: 0 });
    annotation.addPoint({ x: 0.5, y: 0.5 });
    // 不调用 endStroke — 仍在绘制中

    const ctx = {
      drawImage: vi.fn(),
    } as unknown as CanvasRenderingContext2D;

    annotation.drawAll(ctx, 800, 600);
    expect(mockOffscreenCtx.stroke).toHaveBeenCalled();
    expect(ctx.drawImage).toHaveBeenCalled();
  });

  it("annotationColor 和 lineWidth 可修改", () => {
    annotation.annotationColor.value = "#00ff00";
    annotation.lineWidth.value = 6;

    expect(annotation.annotationColor.value).toBe("#00ff00");
    expect(annotation.lineWidth.value).toBe(6);
  });

  it("isAnnotating 是响应式 ref", () => {
    expect(annotation.isAnnotating.value).toBe(false);
    annotation.isAnnotating.value = true;
    expect(annotation.isAnnotating.value).toBe(true);
  });

  it("多次绘制后 hasContent 正确返回", () => {
    // 添加 3 条线
    for (let i = 0; i < 3; i++) {
      annotation.startStroke({ x: i * 0.1, y: 0 });
      annotation.addPoint({ x: i * 0.1 + 0.05, y: 0.1 });
      annotation.endStroke();
    }
    expect(annotation.hasContent()).toBe(true);

    // 撤销 3 次
    annotation.undo();
    annotation.undo();
    annotation.undo();
    expect(annotation.hasContent()).toBe(false);
  });

  // ──────────────── 文字工具测试 ────────────────

  it("annotationTool 默认值为 pen", () => {
    expect(annotation.annotationTool.value).toBe("pen");
  });

  it("可切换到 text 模式", () => {
    annotation.annotationTool.value = "text";
    expect(annotation.annotationTool.value).toBe("text");
  });

  it("text 模式下 startStroke 不设置 isDrawing", () => {
    annotation.annotationTool.value = "text";
    annotation.startStroke({ x: 0.1, y: 0.2 });
    expect(annotation.isDrawing).toBe(false);
  });

  it("addText 添加文字标注，hasContent 返回 true", () => {
    annotation.addText("Hello", { x: 0.5, y: 0.5 });
    expect(annotation.hasContent()).toBe(true);
  });

  it("undo 移除最后一条文字标注", () => {
    annotation.addText("Hello", { x: 0.5, y: 0.5 });
    expect(annotation.hasContent()).toBe(true);

    annotation.undo();
    expect(annotation.hasContent()).toBe(false);
  });

  it("clear 清除所有文字标注", () => {
    annotation.addText("Hello", { x: 0.3, y: 0.3 });
    annotation.addText("World", { x: 0.6, y: 0.6 });
    expect(annotation.hasContent()).toBe(true);

    annotation.clear();
    expect(annotation.hasContent()).toBe(false);
  });

  it("drawAll 使用正确的坐标乘以 canvas 尺寸", () => {
    annotation.startStroke({ x: 0.5, y: 0.25 });
    annotation.addPoint({ x: 0.75, y: 0.5 });
    annotation.endStroke();

    const ctx = {
      drawImage: vi.fn(),
    } as unknown as CanvasRenderingContext2D;

    annotation.drawAll(ctx, 1000, 800);

    // moveTo 应该被调用，坐标为 0.5*1000=500, 0.25*800=200
    expect(mockOffscreenCtx.moveTo).toHaveBeenCalledWith(500, 200);
    // lineTo 应该是 0.75*1000=750, 0.5*800=400
    expect(mockOffscreenCtx.lineTo).toHaveBeenCalledWith(750, 400);
  });
});
