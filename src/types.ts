export interface Format {
  language?: string | null;
  language_preference?: number | null;
  format_note?: string | null;
  format_id: string;
  ext: string | null;
  vcodec: string | null;
  acodec: string | null;
  height: number | null;
  fps: number | null;
  tbr: number | null;
  abr: number | null;
  filesize?: number | null;
  filesize_approx?: number | null;
}
export interface Metadata {
  id: string;
  title: string;
  webpage_url: string;
  extractor: string;
  thumbnail: string | null;
  duration_string: string | null;
  formats: Format[];
  best_height: number | null;
  best_vcodec: string | null;
  should_save_mkv: boolean;
}
export interface Settings {
  subtitles: { enabled: boolean; language: string; source: string };
  transcode: import('./transcode').TranscodeOptions;
  output_dir: string;
  proxy: string;
  use_proxy: boolean;
  cookies_browser: string;
  quality_height: number | null;
  output_mode: string;
  quality_mode: string;
  keep_source: boolean;
  concurrency: number;
  auto_update: boolean;
  batch_output_dir: string;
  filename_suffix: boolean;
  filename_codecs: boolean;
}
export interface Output {
  output_path: string;
  container: string;
  selected_vcodec: string | null;
  selected_acodec: string | null;
  skipped: boolean;
  log: string;
}
export interface Task {
  subtitle_pending?: boolean;
  id: string;
  kind: string;
  source: string;
  title: string;
  status: string;
  stage: string;
  percent: number;
  speed: string;
  eta: string;
  error: string;
  created: number;
  settings: Settings;
  metadata: Metadata | null;
  video_format_id: string | null;
  audio_format_id: string | null;
  output: Output | null;
  downloaded: Output | null;
  name_conflict?: { existing: string; suggested: string } | null;
  conflict_action?: string;
}
export interface Snapshot {
  tasks: Task[];
  settings: Settings;
  paused: boolean;
  persistence_error: string;
}
export interface ToolStatus {
  available: boolean;
  version: string | null;
  message: string | null;
}
export interface Environment {
  yt_dlp: ToolStatus;
  ffmpeg: ToolStatus;
  ffprobe: ToolStatus;
  deno: ToolStatus;
}
