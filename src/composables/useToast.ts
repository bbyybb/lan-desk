export function useToast() {
  function dispatch(message: string, type: "success" | "error" | "info" | "warning") {
    window.dispatchEvent(
      new CustomEvent("lan-desk-toast", { detail: { message, type } })
    );
  }

  return {
    success(message: string) {
      dispatch(message, "success");
    },
    error(message: string) {
      dispatch(message, "error");
    },
    info(message: string) {
      dispatch(message, "info");
    },
    warning(message: string) {
      dispatch(message, "warning");
    },
  };
}
