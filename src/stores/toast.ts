import { defineStore } from "pinia";
import { ref } from "vue";
import { errMessage } from "../api";

export type ToastKind = "info" | "success" | "error";
export interface ToastItem { id: number; kind: ToastKind; message: string }

export const useToast = defineStore("toast", () => {
  const items = ref<ToastItem[]>([]);
  let seq = 0;

  function remove(id: number) {
    items.value = items.value.filter((t) => t.id !== id);
  }

  function push(message: string, kind: ToastKind = "info", ttl = 4000): number {
    seq += 1;
    const id = seq;
    items.value = [...items.value, { id, kind, message }].slice(-5);
    if (ttl > 0) setTimeout(() => remove(id), ttl);
    return id;
  }

  /** 所有 api 调用失败统一走这里。 */
  function error(e: unknown, prefix?: string) {
    const text = errMessage(e);
    push(prefix ? `${prefix}：${text}` : text, "error", 7000);
  }

  function success(message: string) {
    push(message, "success", 3000);
  }

  return { items, push, error, success, remove };
});
