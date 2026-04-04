import { ref, type Ref } from "vue";
import { invoke } from "@tauri-apps/api/core";

export function useTouchInput(canvas: Ref<HTMLCanvasElement | null>) {
  const isMobile = ref(navigator.maxTouchPoints > 0);
  const showVirtualKeyboard = ref(false);

  let lastTouchTime = 0;
  let touchStartPos = { x: 0, y: 0 };
  let isPanning = false;

  function getRelativeCoords(touch: Touch) {
    const c = canvas.value;
    if (!c) return null;
    const rect = c.getBoundingClientRect();
    return {
      x: Math.max(0, Math.min(1, (touch.clientX - rect.left) / rect.width)),
      y: Math.max(0, Math.min(1, (touch.clientY - rect.top) / rect.height)),
    };
  }

  function onTouchStart(e: TouchEvent) {
    if (e.touches.length === 1) {
      const coords = getRelativeCoords(e.touches[0]);
      if (coords) {
        touchStartPos = coords;
        isPanning = true;
        invoke("send_mouse_move", { x: coords.x, y: coords.y });
      }
      e.preventDefault();
    }
  }

  function onTouchMove(e: TouchEvent) {
    if (e.touches.length === 1 && isPanning) {
      const coords = getRelativeCoords(e.touches[0]);
      if (coords) {
        invoke("send_mouse_move", { x: coords.x, y: coords.y });
      }
      e.preventDefault();
    } else if (e.touches.length === 2) {
      // Two-finger scroll
      e.preventDefault();
    }
  }

  function onTouchEnd(e: TouchEvent) {
    if (isPanning && e.changedTouches.length === 1) {
      const coords = getRelativeCoords(e.changedTouches[0]);
      const now = Date.now();
      const dx = coords ? Math.abs(coords.x - touchStartPos.x) : 1;
      const dy = coords ? Math.abs(coords.y - touchStartPos.y) : 1;

      // Tap (short distance) = click
      if (dx < 0.02 && dy < 0.02) {
        invoke("send_mouse_button", { button: "left", pressed: true });
        setTimeout(() => invoke("send_mouse_button", { button: "left", pressed: false }), 50);

        // Double tap detection
        if (now - lastTouchTime < 300) {
          invoke("send_mouse_button", { button: "left", pressed: true });
          setTimeout(() => invoke("send_mouse_button", { button: "left", pressed: false }), 50);
        }
        lastTouchTime = now;
      }
      isPanning = false;
    }
    e.preventDefault();
  }

  // Long press = right click
  let longPressTimer: ReturnType<typeof setTimeout> | null = null;

  function onTouchStartLongPress(e: TouchEvent) {
    if (e.touches.length === 1) {
      longPressTimer = setTimeout(() => {
        invoke("send_mouse_button", { button: "right", pressed: true });
        setTimeout(() => invoke("send_mouse_button", { button: "right", pressed: false }), 50);
      }, 500);
    }
  }

  function cancelLongPress() {
    if (longPressTimer) {
      clearTimeout(longPressTimer);
      longPressTimer = null;
    }
  }

  function setupTouchListeners() {
    const c = canvas.value;
    if (!c || !isMobile.value) return;
    c.addEventListener("touchstart", onTouchStart, { passive: false });
    c.addEventListener("touchstart", onTouchStartLongPress, { passive: false });
    c.addEventListener("touchmove", onTouchMove, { passive: false });
    c.addEventListener("touchend", onTouchEnd, { passive: false });
    c.addEventListener("touchend", cancelLongPress);
    c.addEventListener("touchmove", cancelLongPress);
  }

  function removeTouchListeners() {
    const c = canvas.value;
    if (!c) return;
    c.removeEventListener("touchstart", onTouchStart);
    c.removeEventListener("touchstart", onTouchStartLongPress);
    c.removeEventListener("touchmove", onTouchMove);
    c.removeEventListener("touchend", onTouchEnd);
    c.removeEventListener("touchend", cancelLongPress);
    c.removeEventListener("touchmove", cancelLongPress);
  }

  // Virtual keyboard handler
  function onVirtualKey(key: string) {
    invoke("send_key_event", { code: key, pressed: true, modifiers: 0 });
    setTimeout(() => invoke("send_key_event", { code: key, pressed: false, modifiers: 0 }), 50);
  }

  return {
    isMobile,
    showVirtualKeyboard,
    setupTouchListeners,
    removeTouchListeners,
    onVirtualKey,
  };
}
