import { useSettings } from "./useSettings";

export function useTheme() {
  const { settings } = useSettings();

  let mediaQuery: MediaQueryList | null = null;
  let mediaHandler: ((e: MediaQueryListEvent) => void) | null = null;

  function applyTheme() {
    // 清理之前的监听器
    if (mediaQuery && mediaHandler) {
      mediaQuery.removeEventListener("change", mediaHandler);
      mediaHandler = null;
    }

    let theme = settings.value.theme;
    if (theme === "system") {
      mediaQuery = window.matchMedia("(prefers-color-scheme: dark)");
      theme = mediaQuery.matches ? "dark" : "light";

      // 监听系统主题变化
      mediaHandler = (e: MediaQueryListEvent) => {
        document.documentElement.setAttribute("data-theme", e.matches ? "dark" : "light");
      };
      mediaQuery.addEventListener("change", mediaHandler);
    }
    document.documentElement.setAttribute("data-theme", theme);
  }

  return { applyTheme };
}
