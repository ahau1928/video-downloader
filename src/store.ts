import { reactive, ref } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { Metadata, Settings, Snapshot, Task } from "./types";
import {defaultTranscode} from './transcode';
export const desktop = Boolean(
  (window as unknown as { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__,
);
export const settings = reactive<Settings>({
  subtitles: {enabled:false,language:'bilingual',source:'prefer'},
  transcode: {...defaultTranscode},
  output_dir: "",
  proxy: "http://127.0.0.1:7897",
  use_proxy: true,
  cookies_browser: "",
  quality_height: null,
  output_mode: "original",
  quality_mode: "balanced",
  keep_source: true,
  concurrency: 2,
  auto_update: true,
  batch_output_dir: "",
  filename_suffix: false,
  filename_codecs: false,
});
export const snapshot = reactive<Snapshot>({
  tasks: [],
  settings: { ...settings },
  paused: false,
  persistence_error: "",
});
export const activePage = ref('download');
export const messagePage = ref('download');
export const message = ref("");
let messageTimer: ReturnType<typeof setTimeout> | undefined;
export function notify(text: string, page = activePage.value) {
  clearTimeout(messageTimer);
  messagePage.value = page;
  message.value = text;
  messageTimer = setTimeout(() => { message.value = ''; }, 4500);
}
export const error = ref("");
export const toolsMessage = ref("");
export async function command<T>(
  name: string,
  args?: Record<string, unknown>,
): Promise<T> {
  if (!desktop) throw new Error("网页预览不能运行桌面命令，请使用桌面软件。");
  return invoke<T>(name, args);
}
export function showError(e: unknown) {
  error.value = String(e);
}
export async function initialize() {
  if (!desktop) return () => {};
  const cleanup = await listen<Snapshot>("queue-changed", (event) => {
    Object.assign(snapshot, event.payload);
  });
  const tools = await listen<string>("tools-status", (event) => {
    toolsMessage.value = event.payload;
  });
  const browserErrors = await listen<string>('browser-auth-error', event => showError(event.payload));
  const state = await command<Snapshot>("queue_snapshot");
  Object.assign(snapshot, state);
  Object.assign(settings, state.settings);
  return () => {
    cleanup();
    tools();
    browserErrors();
  };
}
export function frozenSettings() {
  return JSON.parse(JSON.stringify(settings)) as Settings;
}
export async function saveSettings() {
  const origin = activePage.value;
  await command("save_settings", { settings: frozenSettings() });
  notify("默认设置已保存，仅影响之后添加的任务。", origin);
}
export async function action(id: string, action: string) {
  try {
    await command("task_action", { id, action });
  } catch (e) {
    showError(e);
  }
}
export async function chooseDirectory(batch = false) {
  const path = await command<string | null>("select_download_dir");
  if (path) {
    if (batch) settings.batch_output_dir = path;
    else settings.output_dir = path;
    await saveSettings();
  }
}
export async function addTasks(
  sources: string[],
  kind: string,
  start: boolean,
  metadata: Metadata | null = null,
  video: string | null = null,
  audio: string | null = null,
  overrides: Partial<Settings> = {},
) {
  const count = await command<number>("add_tasks", {
    request: {
      sources,
      kind,
      start,
      metadata,
      video_format_id: video,
      audio_format_id: audio,
      settings: { ...frozenSettings(), ...overrides },
    },
  });
  notify(`已添加 ${count} 项${
    count < sources.length ? "，相同等待任务已去重" : ""
  }`, kind);
  return count;
}
export function parseLinks(text: string) {
  const lines = text.replace(/^\uFEFF/, "").split(/\r?\n/).map((s) => s.trim())
    .filter(Boolean);
  const unique = [...new Set(lines)];
  const valid = unique.filter((s) => {
    try {
      const u = new URL(s);
      return ["https:", "http:"].includes(u.protocol);
    } catch {
      return false;
    }
  });
  return {
    valid,
    invalid: unique.filter((s) => !valid.includes(s)),
    duplicates: lines.length - unique.length,
  };
}
export function label(t: Task) {
  return ({
    awaiting_name: "等待选择文件名",
    paused: t.stage,
    pausing: "正在暂停",
    queued: "排队中",
    waiting: "等待开始",
    interrupted: "待恢复",
    running: t.stage,
    done: t.output?.skipped ? "已跳过重复" : "已完成",
    error: t.stage,
    cancelled: "已取消",
  } as Record<string, string>)[t.status] || t.stage;
}
export async function openFolder(path: string) {
  try {
    await command("open_path", { path });
  } catch (e) {
    showError(e);
  }
}
