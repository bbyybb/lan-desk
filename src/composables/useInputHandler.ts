import { type Ref } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { useI18n } from "../i18n";

/**
 * 输入事件处理 composable -- 鼠标、键盘、滚轮
 */
export function useInputHandler(
  canvas: Ref<HTMLCanvasElement | null>,
  _role: Ref<string>,
  isControlMode: Ref<boolean>,
  annotation: {
    isAnnotating: Ref<boolean>;
    isDrawing: boolean;
    annotationTool?: Ref<string>;
    startStroke: (pt: { x: number; y: number }) => void;
    addPoint: (pt: { x: number; y: number }) => void;
    endStroke: () => void;
    addText: (text: string, position: { x: number; y: number }) => void;
  },
  onAnnotationDraw: () => void,
) {
  const { t } = useI18n();
  let lastMoveTime = 0;
  let lastWheelTime = 0;

  function getRelativeCoords(e: MouseEvent): { x: number; y: number } | null {
    const c = canvas.value;
    if (!c) return null;
    const rect = c.getBoundingClientRect();
    return {
      x: Math.max(0, Math.min(1, (e.clientX - rect.left) / rect.width)),
      y: Math.max(0, Math.min(1, (e.clientY - rect.top) / rect.height)),
    };
  }

  function getModifiers(e: KeyboardEvent): number {
    let m = 0;
    if (e.shiftKey) m |= 0x01;
    if (e.ctrlKey) m |= 0x02;
    if (e.altKey) m |= 0x04;
    if (e.metaKey) m |= 0x08;
    return m;
  }

  function onMouseMove(e: MouseEvent) {
    if (annotation.isAnnotating.value) {
      if (annotation.isDrawing) {
        const coords = getRelativeCoords(e);
        if (coords) {
          annotation.addPoint({ x: coords.x, y: coords.y });
          onAnnotationDraw();
        }
      }
      return;
    }
    if (!isControlMode.value) return;
    const now = performance.now();
    if (now - lastMoveTime < 16) return;
    lastMoveTime = now;
    const coords = getRelativeCoords(e);
    if (coords) {
      invoke("send_mouse_move", { x: coords.x, y: coords.y });
    }
  }

  function onMouseDown(e: MouseEvent) {
    if (annotation.isAnnotating.value) {
      const coords = getRelativeCoords(e);
      if (!coords) return;
      // 文字工具模式：弹出输入框
      if (annotation.annotationTool?.value === "text") {
        e.preventDefault();
        const text = prompt(t("annotation.enter_text"));
        if (text) {
          annotation.addText(text, { x: coords.x, y: coords.y });
          onAnnotationDraw();
        }
        return;
      }
      annotation.startStroke({ x: coords.x, y: coords.y });
      e.preventDefault();
      return;
    }
    if (!isControlMode.value) return;
    canvas.value?.focus();
    const btn = ["left", "middle", "right"][e.button] || "left";
    invoke("send_mouse_button", { button: btn, pressed: true });
    e.preventDefault();
  }

  function onMouseUp(e: MouseEvent) {
    if (annotation.isAnnotating.value) {
      annotation.endStroke();
      e.preventDefault();
      return;
    }
    if (!isControlMode.value) return;
    const btn = ["left", "middle", "right"][e.button] || "left";
    invoke("send_mouse_button", { button: btn, pressed: false });
    e.preventDefault();
  }

  function onWheel(e: WheelEvent) {
    if (!isControlMode.value) return;
    const now = performance.now();
    if (now - lastWheelTime < 50) {
      e.preventDefault();
      return;
    }
    lastWheelTime = now;
    const dy = e.deltaY > 0 ? -1 : 1;
    invoke("send_mouse_scroll", { dx: 0, dy });
    e.preventDefault();
  }

  function onKeyDown(e: KeyboardEvent) {
    if (!isControlMode.value) return;
    invoke("send_key_event", {
      code: e.code,
      pressed: true,
      modifiers: getModifiers(e),
    });
    e.preventDefault();
  }

  function onKeyUp(e: KeyboardEvent) {
    if (!isControlMode.value) return;
    invoke("send_key_event", {
      code: e.code,
      pressed: false,
      modifiers: getModifiers(e),
    });
    e.preventDefault();
  }

  function onContextMenu(e: Event) {
    e.preventDefault();
  }

  function setupListeners() {
    canvas.value?.addEventListener("keydown", onKeyDown);
    canvas.value?.addEventListener("keyup", onKeyUp);
  }

  function removeListeners() {
    canvas.value?.removeEventListener("keydown", onKeyDown);
    canvas.value?.removeEventListener("keyup", onKeyUp);
  }

  return {
    onMouseMove,
    onMouseDown,
    onMouseUp,
    onWheel,
    onContextMenu,
    setupListeners,
    removeListeners,
  };
}
