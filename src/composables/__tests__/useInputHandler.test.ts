import { describe, it, expect, beforeEach, vi } from "vitest";
import { ref } from "vue";

// 模拟 @tauri-apps/api/core
const mockInvoke = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => mockInvoke(...args),
}));

import { useInputHandler } from "../useInputHandler";

// 创建模拟 canvas 元素，支持捕获 addEventListener 注册的回调
function createMockCanvas(rect = { left: 0, top: 0, width: 800, height: 600 }) {
  const listeners: Record<string, (...args: any[]) => any> = {};
  return {
    getBoundingClientRect: () => rect,
    focus: vi.fn(),
    addEventListener: vi.fn((event: string, handler: (...args: any[]) => any) => {
      listeners[event] = handler;
    }),
    removeEventListener: vi.fn(),
    __listeners: listeners,
  } as unknown as HTMLCanvasElement & {
    __listeners: Record<string, (...args: any[]) => any>;
  };
}

// 创建模拟 MouseEvent
function createMouseEvent(
  overrides: Partial<MouseEvent> = {},
): MouseEvent {
  return {
    clientX: 400,
    clientY: 300,
    button: 0,
    preventDefault: vi.fn(),
    ...overrides,
  } as unknown as MouseEvent;
}

// 创建模拟 KeyboardEvent
function createKeyboardEvent(
  overrides: Partial<KeyboardEvent> = {},
): KeyboardEvent {
  return {
    code: "KeyA",
    shiftKey: false,
    ctrlKey: false,
    altKey: false,
    metaKey: false,
    preventDefault: vi.fn(),
    ...overrides,
  } as unknown as KeyboardEvent;
}

// 创建模拟 WheelEvent
function createWheelEvent(
  overrides: Partial<WheelEvent> = {},
): WheelEvent {
  return {
    deltaY: 100,
    preventDefault: vi.fn(),
    ...overrides,
  } as unknown as WheelEvent;
}

describe("useInputHandler", () => {
  let mockCanvas: ReturnType<typeof createMockCanvas>;
  let canvasRef: ReturnType<typeof ref<HTMLCanvasElement | null>>;
  let isControlMode: ReturnType<typeof ref<boolean>>;
  let isAnnotating: ReturnType<typeof ref<boolean>>;
  let annotation: {
    isAnnotating: ReturnType<typeof ref<boolean>>;
    isDrawing: boolean;
    startStroke: ReturnType<typeof vi.fn>;
    addPoint: ReturnType<typeof vi.fn>;
    endStroke: ReturnType<typeof vi.fn>;
  };
  let onAnnotationDraw: ReturnType<typeof vi.fn>;

  beforeEach(() => {
    vi.clearAllMocks();
    mockCanvas = createMockCanvas();
    canvasRef = ref(mockCanvas as unknown as HTMLCanvasElement) as ReturnType<
      typeof ref<HTMLCanvasElement | null>
    >;
    isControlMode = ref(true);
    isAnnotating = ref(false);
    annotation = {
      isAnnotating,
      isDrawing: false,
      startStroke: vi.fn(),
      addPoint: vi.fn(),
      endStroke: vi.fn(),
    };
    onAnnotationDraw = vi.fn();
  });

  function createHandler() {
    return useInputHandler(
      canvasRef,
      ref("controller"),
      isControlMode,
      annotation,
      onAnnotationDraw,
    );
  }

  /** 辅助方法：通过 setupListeners 注册键盘事件后获取 keydown 处理器 */
  function getKeydownHandler() {
    const handler = createHandler();
    handler.setupListeners();
    return (mockCanvas as any).__listeners["keydown"] as (
      e: KeyboardEvent,
    ) => void;
  }

  /** 辅助方法：通过 setupListeners 注册键盘事件后获取 keyup 处理器 */
  function getKeyupHandler() {
    const handler = createHandler();
    handler.setupListeners();
    return (mockCanvas as any).__listeners["keyup"] as (
      e: KeyboardEvent,
    ) => void;
  }

  describe("getModifiers 修饰键位掩码计算", () => {
    it("无修饰键时返回 0", () => {
      const keydown = getKeydownHandler();
      keydown(createKeyboardEvent({ code: "KeyA" }));
      expect(mockInvoke).toHaveBeenCalledWith("send_key_event", {
        code: "KeyA",
        pressed: true,
        modifiers: 0,
      });
    });

    it("shift 键 = 0x01", () => {
      const keydown = getKeydownHandler();
      keydown(createKeyboardEvent({ code: "KeyA", shiftKey: true }));
      expect(mockInvoke).toHaveBeenCalledWith("send_key_event", {
        code: "KeyA",
        pressed: true,
        modifiers: 0x01,
      });
    });

    it("ctrl 键 = 0x02", () => {
      const keydown = getKeydownHandler();
      keydown(createKeyboardEvent({ code: "KeyA", ctrlKey: true }));
      expect(mockInvoke).toHaveBeenCalledWith("send_key_event", {
        code: "KeyA",
        pressed: true,
        modifiers: 0x02,
      });
    });

    it("alt 键 = 0x04", () => {
      const keydown = getKeydownHandler();
      keydown(createKeyboardEvent({ code: "KeyA", altKey: true }));
      expect(mockInvoke).toHaveBeenCalledWith("send_key_event", {
        code: "KeyA",
        pressed: true,
        modifiers: 0x04,
      });
    });

    it("meta 键 = 0x08", () => {
      const keydown = getKeydownHandler();
      keydown(createKeyboardEvent({ code: "KeyA", metaKey: true }));
      expect(mockInvoke).toHaveBeenCalledWith("send_key_event", {
        code: "KeyA",
        pressed: true,
        modifiers: 0x08,
      });
    });

    it("组合修饰键：ctrl+shift = 0x03", () => {
      const keydown = getKeydownHandler();
      keydown(
        createKeyboardEvent({
          code: "KeyC",
          ctrlKey: true,
          shiftKey: true,
        }),
      );
      expect(mockInvoke).toHaveBeenCalledWith("send_key_event", {
        code: "KeyC",
        pressed: true,
        modifiers: 0x03,
      });
    });

    it("组合修饰键：ctrl+alt+shift = 0x07", () => {
      const keydown = getKeydownHandler();
      keydown(
        createKeyboardEvent({
          code: "Delete",
          ctrlKey: true,
          altKey: true,
          shiftKey: true,
        }),
      );
      expect(mockInvoke).toHaveBeenCalledWith("send_key_event", {
        code: "Delete",
        pressed: true,
        modifiers: 0x07,
      });
    });

    it("全部修饰键 = 0x0f", () => {
      const keydown = getKeydownHandler();
      keydown(
        createKeyboardEvent({
          code: "KeyA",
          shiftKey: true,
          ctrlKey: true,
          altKey: true,
          metaKey: true,
        }),
      );
      expect(mockInvoke).toHaveBeenCalledWith("send_key_event", {
        code: "KeyA",
        pressed: true,
        modifiers: 0x0f,
      });
    });

    it("keyup 事件 pressed 参数为 false", () => {
      const keyup = getKeyupHandler();
      keyup(createKeyboardEvent({ code: "KeyA", shiftKey: true }));
      expect(mockInvoke).toHaveBeenCalledWith("send_key_event", {
        code: "KeyA",
        pressed: false,
        modifiers: 0x01,
      });
    });
  });

  describe("getRelativeCoords 坐标计算", () => {
    it("鼠标在 canvas 中心时返回 (0.5, 0.5)", () => {
      const handler = createHandler();
      // canvas 区域: left=0, top=0, width=800, height=600
      // 鼠标 clientX=400, clientY=300 => (400/800, 300/600) = (0.5, 0.5)
      handler.onMouseMove(createMouseEvent({ clientX: 400, clientY: 300 }));
      expect(mockInvoke).toHaveBeenCalledWith("send_mouse_move", {
        x: 0.5,
        y: 0.5,
      });
    });

    it("鼠标在左上角时返回 (0, 0)", () => {
      const handler = createHandler();
      handler.onMouseMove(createMouseEvent({ clientX: 0, clientY: 0 }));
      expect(mockInvoke).toHaveBeenCalledWith("send_mouse_move", {
        x: 0,
        y: 0,
      });
    });

    it("鼠标在右下角时返回 (1, 1)", () => {
      const handler = createHandler();
      handler.onMouseMove(createMouseEvent({ clientX: 800, clientY: 600 }));
      expect(mockInvoke).toHaveBeenCalledWith("send_mouse_move", {
        x: 1,
        y: 1,
      });
    });

    it("鼠标超出 canvas 范围时坐标被 clamp 到 [0, 1]", () => {
      const handler = createHandler();
      // clientX=-100 => (-100 - 0) / 800 = -0.125 => clamp 到 0
      handler.onMouseMove(createMouseEvent({ clientX: -100, clientY: -50 }));
      expect(mockInvoke).toHaveBeenCalledWith("send_mouse_move", {
        x: 0,
        y: 0,
      });
    });

    it("鼠标超出右下范围时坐标被 clamp 到 1", () => {
      const handler = createHandler();
      handler.onMouseMove(createMouseEvent({ clientX: 1600, clientY: 1200 }));
      expect(mockInvoke).toHaveBeenCalledWith("send_mouse_move", {
        x: 1,
        y: 1,
      });
    });
  });

  describe("鼠标移动节流", () => {
    it("16ms 内的连续鼠标移动事件被丢弃", () => {
      vi.useFakeTimers();
      // 手动模拟 performance.now
      let now = 1000;
      vi.spyOn(performance, "now").mockImplementation(() => now);

      const handler = createHandler();

      // 第一次事件正常发送
      handler.onMouseMove(createMouseEvent({ clientX: 100, clientY: 100 }));
      expect(mockInvoke).toHaveBeenCalledTimes(1);

      // 10ms 后的事件被节流丢弃
      now += 10;
      handler.onMouseMove(createMouseEvent({ clientX: 200, clientY: 200 }));
      expect(mockInvoke).toHaveBeenCalledTimes(1);

      // 再过 10ms (总共 20ms)，超过 16ms，事件正常发送
      now += 10;
      handler.onMouseMove(createMouseEvent({ clientX: 300, clientY: 300 }));
      expect(mockInvoke).toHaveBeenCalledTimes(2);

      vi.useRealTimers();
    });
  });

  describe("滚轮节流", () => {
    it("50ms 内的连续滚轮事件被丢弃", () => {
      vi.useFakeTimers();
      let now = 1000;
      vi.spyOn(performance, "now").mockImplementation(() => now);

      const handler = createHandler();

      // 第一次滚轮事件正常发送
      handler.onWheel(createWheelEvent({ deltaY: 100 }));
      expect(mockInvoke).toHaveBeenCalledTimes(1);

      // 30ms 后的事件被节流丢弃
      now += 30;
      handler.onWheel(createWheelEvent({ deltaY: 100 }));
      expect(mockInvoke).toHaveBeenCalledTimes(1);

      // 再过 30ms (总共 60ms)，超过 50ms，事件正常发送
      now += 30;
      handler.onWheel(createWheelEvent({ deltaY: -100 }));
      expect(mockInvoke).toHaveBeenCalledTimes(2);

      vi.useRealTimers();
    });
  });

  describe("标注模式", () => {
    it("标注模式下鼠标移动事件被拦截，不发送 invoke", () => {
      isAnnotating.value = true;
      annotation.isDrawing = false;
      const handler = createHandler();

      handler.onMouseMove(createMouseEvent());
      expect(mockInvoke).not.toHaveBeenCalled();
    });

    it("标注模式下正在绘制时，addPoint 被调用", () => {
      isAnnotating.value = true;
      annotation.isDrawing = true;
      const handler = createHandler();

      handler.onMouseMove(createMouseEvent({ clientX: 400, clientY: 300 }));
      expect(annotation.addPoint).toHaveBeenCalledWith({ x: 0.5, y: 0.5 });
      expect(onAnnotationDraw).toHaveBeenCalled();
      expect(mockInvoke).not.toHaveBeenCalled();
    });

    it("标注模式下 mouseDown 调用 startStroke", () => {
      isAnnotating.value = true;
      const handler = createHandler();

      const event = createMouseEvent({ clientX: 200, clientY: 150 });
      handler.onMouseDown(event);

      expect(annotation.startStroke).toHaveBeenCalledWith({
        x: 0.25,
        y: 0.25,
      });
      expect(event.preventDefault).toHaveBeenCalled();
      expect(mockInvoke).not.toHaveBeenCalled();
    });

    it("标注模式下文字工具 mouseDown 不调用 startStroke", () => {
      isAnnotating.value = true;
      // 添加 annotationTool 和 addText 到 annotation 对象
      const annotationTool = ref("text");
      const addText = vi.fn();
      const annotationWithText = {
        ...annotation,
        annotationTool,
        addText,
      };
      // 使用带 annotationTool 的 annotation 重新创建 handler
      const handler = useInputHandler(
        canvasRef,
        ref("controller"),
        isControlMode,
        annotationWithText,
        onAnnotationDraw,
      );

      // 模拟 prompt 返回 null（用户取消）
      vi.stubGlobal("prompt", vi.fn().mockReturnValue(null));

      const event = createMouseEvent({ clientX: 200, clientY: 150 });
      handler.onMouseDown(event);

      // 文字工具模式下不应调用 startStroke
      expect(annotation.startStroke).not.toHaveBeenCalled();
      expect(event.preventDefault).toHaveBeenCalled();
      expect(mockInvoke).not.toHaveBeenCalled();

      vi.unstubAllGlobals();
    });

    it("标注模式下 mouseUp 调用 endStroke", () => {
      isAnnotating.value = true;
      const handler = createHandler();

      const event = createMouseEvent();
      handler.onMouseUp(event);

      expect(annotation.endStroke).toHaveBeenCalled();
      expect(event.preventDefault).toHaveBeenCalled();
      expect(mockInvoke).not.toHaveBeenCalled();
    });
  });

  describe("非控制模式", () => {
    it("非控制模式下鼠标移动不发送 invoke", () => {
      isControlMode.value = false;
      const handler = createHandler();
      handler.onMouseMove(createMouseEvent());
      expect(mockInvoke).not.toHaveBeenCalled();
    });

    it("非控制模式下键盘事件不发送 invoke", () => {
      isControlMode.value = false;
      const keydown = getKeydownHandler();
      const keyup = getKeyupHandler();
      keydown(createKeyboardEvent());
      keyup(createKeyboardEvent());
      expect(mockInvoke).not.toHaveBeenCalled();
    });

    it("非控制模式下滚轮事件不发送 invoke", () => {
      isControlMode.value = false;
      const handler = createHandler();
      handler.onWheel(createWheelEvent());
      expect(mockInvoke).not.toHaveBeenCalled();
    });
  });

  describe("onContextMenu", () => {
    it("阻止默认右键菜单", () => {
      const handler = createHandler();
      const event = { preventDefault: vi.fn() } as unknown as Event;
      handler.onContextMenu(event);
      expect(event.preventDefault).toHaveBeenCalled();
    });
  });
});
