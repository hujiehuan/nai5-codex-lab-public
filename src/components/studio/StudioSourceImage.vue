<script setup lang="ts">
import { onBeforeUnmount, onMounted, ref, watch } from "vue";
import type { DragDropEvent } from "@tauri-apps/api/webview";
import { api, errMessage, imgSrc } from "../../api";
import { isImagePath } from "../../lib/dnd";
import VButton from "../../ui/VButton.vue";

const props = defineProps<{ modelValue: string; disabled?: boolean }>();
const emit = defineEmits<{ "update:modelValue": [path: string]; importing: [value: boolean] }>();
const zone = ref<HTMLElement | null>(null);
const dragOver = ref(false), importing = ref(false), error = ref<string | null>(null);
const dropUnavailable = ref(false);
let alive = true, generation = 0;
let unlisten: (() => void) | undefined;
let cancelPreview: (() => void) | undefined;

function active(ticket: number) { return alive && ticket === generation && !props.disabled; }
// Validate the copied file before replacing the previous usable image. The
// shared Rust import command copies bytes; a filename alone is not validation.
async function readableImage(path: string) {
  await new Promise<void>((resolve, reject) => {
    const image = new Image();
    const finish = (ok: boolean) => {
      clearTimeout(timer); image.onload = image.onerror = null;
      cancelPreview = undefined;
      if (ok) resolve();
      else reject(new Error("图片无法读取，请选择有效的 PNG、JPEG、WebP 或 GIF 图片。"));
    };
    const timer = setTimeout(() => finish(false), 15000);
    cancelPreview = () => finish(false);
    image.onload = () => finish(image.naturalWidth > 0 && image.naturalHeight > 0);
    image.onerror = () => finish(false);
    image.src = imgSrc(path) ?? "";
  });
}
async function importImage(read: () => Promise<string | null>) {
  if (!alive || props.disabled || importing.value) return;
  const ticket = ++generation;
  error.value = null; dragOver.value = false; importing.value = true; emit("importing", true);
  try {
    const path = await read();
    if (!path || !active(ticket)) return;
    await readableImage(path);
    if (active(ticket)) emit("update:modelValue", path);
  } catch (e) { if (active(ticket)) error.value = errMessage(e); }
  finally { if (alive && ticket === generation) { importing.value = false; emit("importing", false); } }
}
function pickImage() {
  // The picker already copies into local_images; don't import that copy again.
  void importImage(() => api.reversePickLocalImage());
}
function inside(position: { x: number; y: number }) {
  const element = zone.value;
  if (!element || props.disabled || importing.value) return false;
  // Locked Wry 0.55.1 passes AppKit draggingLocation (logical points) on Mac,
  // despite the SDK's PhysicalPosition type. Other backends use physical pixels.
  // DOM rectangles already include CSS zoom: do not divide by app zoom again.
  const mac = navigator.platform.startsWith("Mac");
  const scale = mac ? 1 : window.devicePixelRatio;
  const x = position.x / scale, y = position.y / scale;
  const rect = element.getBoundingClientRect();
  if (x < rect.left || x >= rect.right || y < rect.top || y >= rect.bottom) return false;
  const target = document.elementFromPoint(x, y);
  return !!target && element.contains(target);
}
function onNativeDrop(event: DragDropEvent) {
  if (!alive) return;
  if (event.type === "leave") { dragOver.value = false; return; }
  const hit = inside(event.position);
  dragOver.value = event.type !== "drop" && hit;
  if (event.type !== "drop" || !hit) return;
  if (event.paths.length !== 1) { error.value = "每次请拖入一张反推原图。"; return; }
  const path = event.paths[0];
  if (!isImagePath(path)) { error.value = "只接受 PNG、JPEG、WebP 或 GIF 图片文件。"; return; }
  void importImage(() => api.reverseImportLocalImage(path));
}
watch(() => props.disabled, (disabled) => {
  if (!disabled) return;
  dragOver.value = false;
  if (importing.value) {
    ++generation; cancelPreview?.(); importing.value = false; emit("importing", false);
  }
});
onMounted(async () => {
  try {
    const { getCurrentWebview } = await import("@tauri-apps/api/webview");
    const off = await getCurrentWebview().onDragDropEvent((event) => onNativeDrop(event.payload));
    if (!alive) off(); else unlisten = off;
  } catch { if (alive) dropUnavailable.value = true; }
});
onBeforeUnmount(() => {
  alive = false; ++generation; dragOver.value = false;
  cancelPreview?.(); unlisten?.(); emit("importing", false);
});
</script>

<template>
  <section ref="zone" class="source-drop" :class="{ 'drag-over': dragOver, disabled: disabled || importing }"
    data-testid="studio-source-drop" aria-label="反推原图" :aria-busy="importing" @dragover.prevent @drop.prevent>
    <div class="source-copy">
      <strong>{{ dragOver ? '松开以使用这张原图' : '将反推原图拖到这里' }}</strong>
      <p class="hint">一次一张 · PNG / JPEG / WebP / GIF</p>
      <VButton :disabled="disabled || importing" @click="pickImage">{{ modelValue ? '更换反推原图' : '选择反推原图' }}</VButton>
    </div>
    <img v-if="modelValue" class="source-image" :src="imgSrc(modelValue) ?? undefined" alt="反推原图" draggable="false" />
    <p v-if="importing" class="hint" role="status">读取图片中…</p>
    <p v-else-if="disabled" class="hint">原图已锁定，暂不能更换。</p>
    <p v-if="dropUnavailable" class="hint">拖放暂不可用，仍可点击选择图片。</p>
    <p v-if="error" class="error" role="alert">{{ error }}</p>
  </section>
</template>

<style scoped>
.source-drop { min-width: 0; display: grid; gap: 12px; padding: 20px; border: 1px dashed var(--line-strong); border-radius: var(--radius); background: var(--field); }
.source-drop.drag-over { border-color: var(--accent); background: var(--accent-soft); outline: 2px solid var(--accent); outline-offset: -2px; }
.source-copy { display: grid; gap: 8px; justify-items: start; min-width: 0; }
.source-copy :deep(button) { max-width: 100%; }
.source-image { display: block; max-width: 100%; width: auto; max-height: 220px; object-fit: contain; border-radius: var(--radius); }
.hint, .error { margin: 0; font-size: 13px; overflow-wrap: anywhere; }
.hint { color: var(--muted); }
.error { color: var(--danger); }
</style>
