import { describe, it, expect, vi, beforeEach } from "vitest";
import { useRemoteCursor } from "../useRemoteCursor";

function createMockCtx() {
  return {
    save: vi.fn(),
    restore: vi.fn(),
    beginPath: vi.fn(),
    moveTo: vi.fn(),
    lineTo: vi.fn(),
    stroke: vi.fn(),
    fill: vi.fn(),
    arc: vi.fn(),
    strokeStyle: "",
    fillStyle: "",
    lineWidth: 1,
  } as unknown as CanvasRenderingContext2D;
}

describe("useRemoteCursor", () => {
  let cursor: ReturnType<typeof useRemoteCursor>;
  let ctx: ReturnType<typeof createMockCtx>;
  const W = 1920;
  const H = 1080;

  beforeEach(() => {
    cursor = useRemoteCursor();
    ctx = createMockCtx();
  });

  it("updateCursor 和 drawRemoteCursor 正常工作", () => {
    cursor.updateCursor(0.5, 0.5, "Arrow");
    cursor.drawRemoteCursor(ctx, W, H);

    expect(ctx.save).toHaveBeenCalled();
    expect(ctx.restore).toHaveBeenCalled();
    // Arrow 默认绘制：圈 + 点
    expect(ctx.arc).toHaveBeenCalled();
    expect(ctx.stroke).toHaveBeenCalled();
    expect(ctx.fill).toHaveBeenCalled();
  });

  it("Hidden 光标不进行任何绘制", () => {
    cursor.updateCursor(0.5, 0.5, "Hidden");
    cursor.drawRemoteCursor(ctx, W, H);

    expect(ctx.save).toHaveBeenCalled();
    expect(ctx.restore).toHaveBeenCalled();
    // Hidden 跳过所有绘制
    expect(ctx.arc).not.toHaveBeenCalled();
    expect(ctx.stroke).not.toHaveBeenCalled();
    expect(ctx.fill).not.toHaveBeenCalled();
    expect(ctx.moveTo).not.toHaveBeenCalled();
  });

  it("IBeam 光标绘制竖线", () => {
    cursor.updateCursor(0.3, 0.4, "IBeam");
    cursor.drawRemoteCursor(ctx, W, H);

    expect(ctx.stroke).toHaveBeenCalled();
    // IBeam 使用 moveTo/lineTo 绘制竖线
    expect(ctx.moveTo).toHaveBeenCalled();
    expect(ctx.lineTo).toHaveBeenCalled();
  });

  it("Hand 光标绘制圆形", () => {
    cursor.updateCursor(0.2, 0.8, "Hand");
    cursor.drawRemoteCursor(ctx, W, H);

    expect(ctx.arc).toHaveBeenCalled();
    expect(ctx.fill).toHaveBeenCalled();
  });

  it("Crosshair 光标绘制十字", () => {
    cursor.updateCursor(0.5, 0.5, "Crosshair");
    cursor.drawRemoteCursor(ctx, W, H);

    expect(ctx.stroke).toHaveBeenCalled();
    expect(ctx.moveTo).toHaveBeenCalled();
    expect(ctx.lineTo).toHaveBeenCalled();
  });

  it("Wait 光标绘制双圈", () => {
    cursor.updateCursor(0.5, 0.5, "Wait");
    cursor.drawRemoteCursor(ctx, W, H);

    // Wait 绘制两个 arc
    expect(ctx.arc).toHaveBeenCalledTimes(2);
    expect(ctx.stroke).toHaveBeenCalledTimes(2);
  });

  it("ResizeNS 光标绘制垂直双向箭头", () => {
    cursor.updateCursor(0.5, 0.5, "ResizeNS");
    cursor.drawRemoteCursor(ctx, W, H);

    expect(ctx.stroke).toHaveBeenCalled();
    expect(ctx.moveTo).toHaveBeenCalled();
    expect(ctx.lineTo).toHaveBeenCalled();
  });

  it("ResizeEW 光标绘制水平双向箭头", () => {
    cursor.updateCursor(0.5, 0.5, "ResizeEW");
    cursor.drawRemoteCursor(ctx, W, H);

    expect(ctx.stroke).toHaveBeenCalled();
  });

  it("ResizeNESW 光标绘制对角箭头", () => {
    cursor.updateCursor(0.5, 0.5, "ResizeNESW");
    cursor.drawRemoteCursor(ctx, W, H);

    expect(ctx.stroke).toHaveBeenCalled();
  });

  it("ResizeNWSE 光标绘制对角箭头", () => {
    cursor.updateCursor(0.5, 0.5, "ResizeNWSE");
    cursor.drawRemoteCursor(ctx, W, H);

    expect(ctx.stroke).toHaveBeenCalled();
  });

  it("Move 光标绘制四向箭头", () => {
    cursor.updateCursor(0.5, 0.5, "Move");
    cursor.drawRemoteCursor(ctx, W, H);

    expect(ctx.stroke).toHaveBeenCalled();
    // Move 有大量 moveTo/lineTo 调用
    expect(ctx.moveTo).toHaveBeenCalled();
    expect(ctx.lineTo).toHaveBeenCalled();
  });

  it("Help 光标绘制问号", () => {
    cursor.updateCursor(0.5, 0.5, "Help");
    cursor.drawRemoteCursor(ctx, W, H);

    // Help 有 arc（问号弧线）+ fill（底部点）
    expect(ctx.arc).toHaveBeenCalled();
    expect(ctx.fill).toHaveBeenCalled();
    expect(ctx.stroke).toHaveBeenCalled();
  });

  it("NotAllowed 光标绘制禁止符号", () => {
    cursor.updateCursor(0.5, 0.5, "NotAllowed");
    cursor.drawRemoteCursor(ctx, W, H);

    // 圆圈 + 斜线
    expect(ctx.arc).toHaveBeenCalled();
    expect(ctx.stroke).toHaveBeenCalledTimes(2);
  });

  it("光标坐标正确映射到 canvas 像素", () => {
    cursor.updateCursor(0.25, 0.75, "Arrow");
    cursor.drawRemoteCursor(ctx, 1000, 800);

    // Arrow 绘制 arc(x, y, ...) 其中 x=0.25*1000=250, y=0.75*800=600
    const arcCalls = (ctx.arc as ReturnType<typeof vi.fn>).mock.calls;
    expect(arcCalls[0][0]).toBe(250);
    expect(arcCalls[0][1]).toBe(600);
  });

  it("未知光标类型使用默认 Arrow 样式", () => {
    cursor.updateCursor(0.5, 0.5, "UnknownType");
    cursor.drawRemoteCursor(ctx, W, H);

    // 默认箭头：arc + fill
    expect(ctx.arc).toHaveBeenCalled();
    expect(ctx.fill).toHaveBeenCalled();
  });

  it("连续更新光标位置", () => {
    cursor.updateCursor(0.1, 0.1, "Arrow");
    cursor.drawRemoteCursor(ctx, 1000, 1000);

    let arcCalls = (ctx.arc as ReturnType<typeof vi.fn>).mock.calls;
    expect(arcCalls[0][0]).toBe(100);
    expect(arcCalls[0][1]).toBe(100);

    vi.clearAllMocks();
    ctx = createMockCtx();

    cursor.updateCursor(0.9, 0.9, "Arrow");
    cursor.drawRemoteCursor(ctx, 1000, 1000);

    arcCalls = (ctx.arc as ReturnType<typeof vi.fn>).mock.calls;
    expect(arcCalls[0][0]).toBe(900);
    expect(arcCalls[0][1]).toBe(900);
  });

  it("12 种光标形状全部可正常绘制不报错", () => {
    const shapes = [
      "Arrow", "IBeam", "Hand", "Crosshair", "Wait",
      "ResizeNS", "ResizeEW", "ResizeNESW", "ResizeNWSE",
      "Move", "Help", "NotAllowed", "Hidden",
    ];

    for (const shape of shapes) {
      const c = createMockCtx();
      cursor.updateCursor(0.5, 0.5, shape);
      expect(() => cursor.drawRemoteCursor(c, W, H)).not.toThrow();
    }
  });
});
