/// <reference types="vite/client" />

declare module "*.vue" {
  import type { DefineComponent } from "vue";
  const component: DefineComponent<object, object, unknown>;
  export default component;
}

interface Window {
  /** 完整性检查钩子（integrity.ts） */
  __ci?: () => void;
}
