import { describe, it, expect, vi } from "vitest";
import { useToast } from "../useToast";

describe("useToast", () => {
  it("success() 派发 CustomEvent，type 为 success", () => {
    const spy = vi.fn();
    window.addEventListener("lan-desk-toast", spy);

    const toast = useToast();
    toast.success("操作成功");

    expect(spy).toHaveBeenCalledTimes(1);
    const event = spy.mock.calls[0][0] as CustomEvent;
    expect(event.detail).toEqual({ message: "操作成功", type: "success" });

    window.removeEventListener("lan-desk-toast", spy);
  });

  it("error() 派发 CustomEvent，type 为 error", () => {
    const spy = vi.fn();
    window.addEventListener("lan-desk-toast", spy);

    const toast = useToast();
    toast.error("连接失败");

    expect(spy).toHaveBeenCalledTimes(1);
    const event = spy.mock.calls[0][0] as CustomEvent;
    expect(event.detail).toEqual({ message: "连接失败", type: "error" });

    window.removeEventListener("lan-desk-toast", spy);
  });

  it("info() 派发 CustomEvent，type 为 info", () => {
    const spy = vi.fn();
    window.addEventListener("lan-desk-toast", spy);

    const toast = useToast();
    toast.info("提示信息");

    expect(spy).toHaveBeenCalledTimes(1);
    const event = spy.mock.calls[0][0] as CustomEvent;
    expect(event.detail).toEqual({ message: "提示信息", type: "info" });

    window.removeEventListener("lan-desk-toast", spy);
  });

  it("warning() 派发 CustomEvent，type 为 warning", () => {
    const spy = vi.fn();
    window.addEventListener("lan-desk-toast", spy);

    const toast = useToast();
    toast.warning("警告信息");

    expect(spy).toHaveBeenCalledTimes(1);
    const event = spy.mock.calls[0][0] as CustomEvent;
    expect(event.detail).toEqual({ message: "警告信息", type: "warning" });

    window.removeEventListener("lan-desk-toast", spy);
  });

  it("事件名称为 lan-desk-toast", () => {
    const spy = vi.fn();
    const wrongSpy = vi.fn();
    window.addEventListener("lan-desk-toast", spy);
    window.addEventListener("toast", wrongSpy);

    const toast = useToast();
    toast.info("test");

    expect(spy).toHaveBeenCalledTimes(1);
    expect(wrongSpy).not.toHaveBeenCalled();

    window.removeEventListener("lan-desk-toast", spy);
    window.removeEventListener("toast", wrongSpy);
  });

  it("空消息也能正常派发", () => {
    const spy = vi.fn();
    window.addEventListener("lan-desk-toast", spy);

    const toast = useToast();
    toast.success("");

    expect(spy).toHaveBeenCalledTimes(1);
    const event = spy.mock.calls[0][0] as CustomEvent;
    expect(event.detail).toEqual({ message: "", type: "success" });

    window.removeEventListener("lan-desk-toast", spy);
  });

  it("连续调用多次，每次都派发独立事件", () => {
    const spy = vi.fn();
    window.addEventListener("lan-desk-toast", spy);

    const toast = useToast();
    toast.success("第一条");
    toast.error("第二条");
    toast.info("第三条");

    expect(spy).toHaveBeenCalledTimes(3);
    expect((spy.mock.calls[0][0] as CustomEvent).detail.type).toBe("success");
    expect((spy.mock.calls[1][0] as CustomEvent).detail.type).toBe("error");
    expect((spy.mock.calls[2][0] as CustomEvent).detail.type).toBe("info");

    window.removeEventListener("lan-desk-toast", spy);
  });

  it("detail 中包含正确的 message 和 type 字段", () => {
    const spy = vi.fn();
    window.addEventListener("lan-desk-toast", spy);

    const toast = useToast();
    toast.error("详细错误信息：网络超时");

    const event = spy.mock.calls[0][0] as CustomEvent;
    expect(event.detail).toHaveProperty("message", "详细错误信息：网络超时");
    expect(event.detail).toHaveProperty("type", "error");
    // 不应有多余的字段
    expect(Object.keys(event.detail)).toEqual(["message", "type"]);

    window.removeEventListener("lan-desk-toast", spy);
  });
});
