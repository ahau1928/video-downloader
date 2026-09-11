#![cfg_attr(all(not(debug_assertions), target_os = "windows"), windows_subsystem = "windows")]

#[cfg(debug_assertions)]
mod diagnostics;
mod engine;
mod transcode;
mod browser_auth;
mod process;
mod queue;
mod updater;
#[cfg(test)]
mod tests;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::{Arc, Mutex},
    thread,
};
use tauri::{AppHandle, Emitter, Manager};

#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;

#[cfg(target_os = "windows")]
const CREATE_NO_WINDOW: u32 = 0x08000000;

#[derive(Debug, Serialize)]
struct ToolStatus {
    available: bool,
    version: Option<String>,
    message: Option<String>,
}

#[derive(Debug, Serialize)]
struct EnvironmentReport {
    yt_dlp: ToolStatus,
    ffmpeg: ToolStatus,
    recommended_proxy: String,
    deno: ToolStatus,
    ffprobe: ToolStatus,
}

#[derive(Debug, Deserialize)]
struct ProbeRequest {
    url: String,
    proxy: Option<String>,
    cookies_browser: Option<String>,
    cookies_file: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct FormatInfo {
    format_id: String,
    ext: Option<String>,
    vcodec: Option<String>,
    acodec: Option<String>,
    height: Option<u64>,
    fps: Option<f64>,
    tbr: Option<f64>,
    abr: Option<f64>,
    #[serde(default)]
    filesize: Option<u64>,
    #[serde(default)]
    filesize_approx: Option<u64>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct ProbeResult {
    #[serde(default)]
    id: String,
    title: String,
    webpage_url: String,
    extractor: String,
    thumbnail: Option<String>,
    duration_string: Option<String>,
    #[serde(default)]
    anonymous: bool,
    best_height: Option<u64>,
    best_vcodec: Option<String>,
    should_save_mkv: bool,
    formats: Vec<FormatInfo>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
enum SaveStrategy {
    Auto,
    Mp4,
    Mkv,
}

#[derive(Debug, Deserialize)]
struct DownloadRequest {
    #[serde(default)]
    audio_only: bool,
    #[serde(default)]
    filename_suffix: bool,
    #[serde(default)]
    filename_codecs: bool,
    #[serde(default)]
    conflict_action: String,
    url: String,
    output_dir: String,
    proxy: Option<String>,
    cookies_browser: Option<String>,
    cookies_file: Option<String>,
    quality_height: Option<u64>,
    video_format_id: Option<String>,
    audio_format_id: Option<String>,
    save_strategy: SaveStrategy,
    metadata: Option<ProbeResult>,
}

#[derive(Debug, Deserialize)]
struct TranscodeRequest {
    #[serde(default)]
    options: Option<transcode::Options>,
    input_path: String,
    output_path: Option<String>,
    quality_mode: Option<String>,
    delete_source: Option<bool>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct DownloadResult {
    output_path: String,
    container: String,
    selected_vcodec: Option<String>,
    selected_acodec: Option<String>,
    skipped: bool,
    log: String,
}

#[derive(Debug, Serialize)]
struct BilibiliQrStart {
    url: String,
    qrcode_key: String,
}

#[derive(Debug, Serialize)]
struct BilibiliQrPoll {
    status: String,
    message: String,
    logged_in: bool,
    cookie_file: Option<String>,
}

#[derive(Debug, Serialize)]
struct BilibiliLoginStatus {
    logged_in: bool,
    cookie_file: Option<String>,
}

#[derive(Debug, Serialize, Clone)]
struct DownloadProgress {
    percent: f64,
    stage: String,
    file_name: String,
    speed: String,
    eta: String,
}

#[derive(Debug)]
struct MediaStats {
    duration_seconds: f64,
    size_bytes: u64,
}

#[derive(Debug)]
struct BitratePlan {
    audio_kbps: u64,
    video_kbps: u64,
    crf: Option<u8>,
    preset: &'static str,
}

struct ProcessOutput {
    stdout: String,
    stderr: String,
}

struct DownloadSelection<'a> {
    format_selector: String,
    use_mkv: bool,
    audio_only: bool,
    video: Option<&'a FormatInfo>,
    audio: Option<&'a FormatInfo>,
}

#[tauri::command]
async fn check_environment(app: AppHandle) -> Result<EnvironmentReport, String> {
    tauri::async_runtime::spawn_blocking(move || check_environment_blocking(app))
        .await
        .map_err(|error| format!("环境检查线程失败：{error}"))
}

fn check_environment_blocking(app: AppHandle) -> EnvironmentReport {
    EnvironmentReport {
        yt_dlp: tool_status(&app, "yt-dlp", "--version"),
        ffmpeg: tool_status(&app, "ffmpeg", "-version"),
        deno: tool_status(&app, "deno", "--version"),
        ffprobe: tool_status(&app, "ffprobe", "-version"),
        recommended_proxy: "http://127.0.0.1:7897".to_string(),
    }
}

#[tauri::command]
fn get_default_download_dir() -> String {
    std::env::var_os("USERPROFILE")
        .map(PathBuf::from)
        .map(|path| path.join("Desktop"))
        .filter(|path| path.is_dir())
        .or_else(|| std::env::var_os("USERPROFILE").map(PathBuf::from))
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
        .to_string_lossy()
        .to_string()
}

#[tauri::command]
fn select_download_dir() -> Option<String> {
    rfd::FileDialog::new()
        .set_title("选择视频保存目录")
        .pick_folder()
        .map(|path| path.to_string_lossy().to_string())
}

#[tauri::command]
fn select_video_files() -> Vec<String> {
    rfd::FileDialog::new()
        .set_title("选择需要转码的视频文件")
        .add_filter(
            "视频文件",
            &["mkv", "webm", "mp4", "mov", "avi", "flv", "m4v", "ts"],
        )
        .pick_files()
        .unwrap_or_default()
        .into_iter()
        .map(|path| path.to_string_lossy().to_string())
        .collect()
}

#[tauri::command]
fn get_bilibili_login_status(app: AppHandle) -> BilibiliLoginStatus {
    let cookie_file = bilibili_cookies_path(&app);
    let logged_in = cookie_file.is_file();
    BilibiliLoginStatus {
        logged_in,
        cookie_file: logged_in.then(|| cookie_file.to_string_lossy().to_string()),
    }
}

#[tauri::command]
fn clear_bilibili_login(app: AppHandle) -> Result<(), String> {
    let cookie_file = bilibili_cookies_path(&app);
    if cookie_file.is_file() {
        fs::remove_file(&cookie_file)
            .map_err(|error| format!("无法清除 Bilibili 登录信息：{error}"))?;
    }
    Ok(())
}

#[tauri::command]
async fn start_bilibili_qr_login(app: AppHandle) -> Result<BilibiliQrStart, String> {
    tauri::async_runtime::spawn_blocking(move || start_bilibili_qr_login_blocking(&app))
        .await
        .map_err(|error| format!("Bilibili 登录线程失败：{error}"))?
}

#[tauri::command]
async fn poll_bilibili_qr_login(app: AppHandle, qrcode_key: String) -> Result<BilibiliQrPoll, String> {
    tauri::async_runtime::spawn_blocking(move || poll_bilibili_qr_login_blocking(&app, &qrcode_key))
        .await
        .map_err(|error| format!("Bilibili 登录轮询线程失败：{error}"))?
}

#[tauri::command]
fn open_path(path: String) -> Result<(), String> {
    let path = PathBuf::from(path.trim());
    if !path.exists() {
        return Err("目标路径不存在。".to_string());
    }
    let target = if path.is_file() {
        path.parent().unwrap_or(&path).to_path_buf()
    } else {
        path
    };

    #[cfg(target_os = "windows")]
    {
        hidden_command(Path::new("explorer.exe"))
            .arg(target)
            .spawn()
            .map_err(|error| format!("无法打开目标文件夹：{error}"))?;
    }

    #[cfg(target_os = "macos")]
    {
        Command::new("open")
            .arg(target)
            .spawn()
            .map_err(|error| format!("无法打开目标文件夹：{error}"))?;
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    {
        Command::new("xdg-open")
            .arg(target)
            .spawn()
            .map_err(|error| format!("无法打开目标文件夹：{error}"))?;
    }

    Ok(())
}

#[tauri::command]
async fn read_clipboard_text() -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let output = run_process(
            Path::new("powershell.exe"),
            "PowerShell",
            vec![
                OsString::from("-NoProfile"),
                OsString::from("-Command"),
                OsString::from("[Console]::OutputEncoding = [System.Text.UTF8Encoding]::new(); Get-Clipboard -Raw"),
            ],
        )?;
        Ok(output.stdout.trim().to_string())
    })
    .await
    .map_err(|error| format!("读取剪贴板线程失败：{error}"))?
}

#[tauri::command]
async fn probe_video(app: AppHandle, request: ProbeRequest) -> Result<ProbeResult, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state=app.state::<queue::Queue>();
        let _guard=state.tools.read().unwrap();
        probe_metadata(
            &app,
            &request.url,
            request.proxy.as_deref(),
            request.cookies_browser.as_deref(),
            request.cookies_file.as_deref(),
        )
    })
    .await
    .map_err(|error| format!("解析线程失败：{error}"))?
}

fn download_video_blocking(app: AppHandle, request: DownloadRequest) -> Result<DownloadResult, String> { engine::download(app,request) }

fn transcode_to_h264_blocking(app: AppHandle, request: TranscodeRequest) -> Result<DownloadResult, String> { engine::transcode(app,request) }

fn tool_status(app: &AppHandle, tool_name: &str, version_arg: &str) -> ToolStatus {
    let program = resolve_tool(app, tool_name);
    let mut command = hidden_command(&program);
    match command.arg(version_arg).output() {
        Ok(output) if output.status.success() => {
            let version = String::from_utf8_lossy(&output.stdout)
                .lines()
                .next()
                .map(|line| line.trim().to_string())
                .filter(|line| !line.is_empty());
            ToolStatus {
                available: true,
                version,
                message: None,
            }
        }
        Ok(output) => ToolStatus {
            available: false,
            version: None,
            message: Some(String::from_utf8_lossy(&output.stderr).trim().to_string()),
        },
        Err(error) => ToolStatus {
            available: false,
            version: None,
            message: Some(error.to_string()),
        },
    }
}

fn probe_metadata(
    app: &AppHandle,
    url: &str,
    proxy: Option<&str>,
    cookies_browser: Option<&str>,
    cookies_file: Option<&str>,
) -> Result<ProbeResult, String> {
    validate_url(url)?;
    let mut args = vec![
        OsString::from("-J"),
        OsString::from("--no-playlist"),
        OsString::from("--format"),
        OsString::from("bestvideo*+bestaudio/best"),
    ];
    if let Some(proxy) = clean_proxy(proxy) {
        args.push(OsString::from("--proxy"));
        args.push(OsString::from(proxy));
    }
    if clean_proxy(proxy).is_none(){args.extend(["--proxy".into(),"".into()]);}
    let _cookie_lease=push_cookies_args(app, &mut args, url, cookies_browser, cookies_file)?;
    args.push(OsString::from("--"));
    args.push(OsString::from(url));

    let yt_dlp = resolve_tool(app, "yt-dlp");
    let mut anonymous = false;
    let output = match run_process(&yt_dlp, "yt-dlp", yt_args(app,args.clone())) {
        Ok(output) => output,
        Err(first) if is_bilibili(url) && bili_retryable(&first) && !process::cancelled() => {
            // Retry public access once without stale login cookies. Never loop on site blocks.
            let mut guest = Vec::new();
            let mut skip = false;
            for arg in args { if skip {skip=false;continue} if arg=="--cookies" || arg=="--cookies-from-browser" {skip=true;continue} guest.push(arg); }
            thread::sleep(std::time::Duration::from_millis(350));
            if process::cancelled() {return Err("任务已取消".into())}
            anonymous = true;
            run_process(&yt_dlp, "yt-dlp", yt_args(app,guest)).map_err(|e|bili_error(&e))?
        },
        Err(e) => return Err(if is_bilibili(url) {bili_error(&e)} else {e}),
    };
    let json: Value = serde_json::from_str(&output.stdout)
        .map_err(|error| format!("yt-dlp 返回的数据无法解析：{error}"))?;

    let extractor = json
        .get("extractor_key")
        .or_else(|| json.get("extractor"))
        .and_then(Value::as_str)
        .unwrap_or("unknown")
        .to_string();
    let title = json
        .get("title")
        .and_then(Value::as_str)
        .unwrap_or("untitled")
        .to_string();
    let webpage_url = json
        .get("webpage_url")
        .and_then(Value::as_str)
        .unwrap_or(url)
        .to_string();
    let duration_string = json
        .get("duration_string")
        .and_then(Value::as_str)
        .map(str::to_string);
    let thumbnail = json
        .get("thumbnail")
        .and_then(Value::as_str)
        .map(normalize_thumbnail_url)
        .map(|url| fetch_thumbnail_data_url(&url, &webpage_url).unwrap_or(url));

    let mut formats = json
        .get("formats")
        .and_then(Value::as_array)
        .map(|items| items.iter().filter_map(parse_format).collect::<Vec<_>>())
        .unwrap_or_default();

    for format in &mut formats {
        if format.filesize.is_none() && format.filesize_approx.is_none() {
            if let (Some(duration),Some(rate))=(json.get("duration").and_then(Value::as_f64),format.tbr) {
                format.filesize_approx=Some((duration*rate*1000./8.) as u64);
            }
        }
    }

    let is_youtube = extractor.to_ascii_lowercase().contains("youtube")
        || webpage_url.to_ascii_lowercase().contains("youtube.com")
        || webpage_url.to_ascii_lowercase().contains("youtu.be");


    formats.sort_by(|a, b| {
        b.height
            .unwrap_or(0)
            .cmp(&a.height.unwrap_or(0))
            .then_with(|| b.tbr.partial_cmp(&a.tbr).unwrap_or(std::cmp::Ordering::Equal))
    });

    let best_video = formats
        .iter()
        .filter(|format| format.vcodec.as_deref().unwrap_or("none") != "none")
        .max_by(|a, b| {
            a.height
                .unwrap_or(0)
                .cmp(&b.height.unwrap_or(0))
                .then_with(|| a.tbr.partial_cmp(&b.tbr).unwrap_or(std::cmp::Ordering::Equal))
        });

    let best_height = best_video.and_then(|format| format.height);
    let best_vcodec = best_video.and_then(|format| format.vcodec.clone());
    let codec_lower = best_vcodec
        .as_deref()
        .unwrap_or_default()
        .to_ascii_lowercase();
    let should_save_mkv =
        is_youtube && (codec_lower.starts_with("vp8") || codec_lower.starts_with("vp9"));

    Ok(ProbeResult {
        id: json.get("id").and_then(Value::as_str).unwrap_or_default().to_string(),
        title,
        webpage_url,
        extractor,
        thumbnail,
        duration_string,
        anonymous,
        best_height,
        best_vcodec,
        should_save_mkv,
        formats,
    })
}

fn is_bilibili(url: &str) -> bool {
    tauri::Url::parse(url).ok().and_then(|u|u.host_str().map(str::to_owned))
        .map(|h|h=="bilibili.com"||h.ends_with(".bilibili.com")||h=="b23.tv").unwrap_or(false)
}
fn bili_retryable(error: &str) -> bool {
    let value=error.to_lowercase();
    value.contains("412")||value.contains("403")||value.contains("cookie")||value.contains("login")||value.contains("requested format")
}
fn bili_error(error: &str) -> String {
    if error.contains("412") {
        "B 站暂时拒绝了当前网络的访问（HTTP 412），不代表必须登录。请检查代理或稍后重试；公开视频可匿名下载可用清晰度。".into()
    } else {error.into()}
}

fn parse_format(value: &Value) -> Option<FormatInfo> {
    let format_id = value.get("format_id")?.as_str()?.to_string();
    Some(FormatInfo {
        format_id,
        ext: value.get("ext").and_then(Value::as_str).map(str::to_string),
        vcodec: value.get("vcodec").and_then(Value::as_str).map(str::to_string),
        acodec: value.get("acodec").and_then(Value::as_str).map(str::to_string),
        height: value.get("height").and_then(Value::as_u64),
        fps: value.get("fps").and_then(Value::as_f64),
        tbr: value.get("tbr").and_then(Value::as_f64),
        abr: value.get("abr").and_then(Value::as_f64),
        filesize: value.get("filesize").and_then(Value::as_u64),
        filesize_approx: value.get("filesize_approx").and_then(Value::as_u64).or_else(|| {
            let rate=value.get("tbr").and_then(Value::as_f64)?;
            let duration=value.get("duration").and_then(Value::as_f64)?;
            Some((rate*1000.*duration/8.) as u64)
        }),
    })
}

fn select_download_formats<'a>(
    metadata: &'a ProbeResult,
    height: Option<u64>,
    video_format_id: Option<&str>,
    audio_format_id: Option<&str>,
    save_strategy: &SaveStrategy,
) -> DownloadSelection<'a> {
    let video = video_format_id
        .and_then(|id| metadata.formats.iter().find(|format| format.format_id == id && format.vcodec.as_deref().unwrap_or("none") != "none"))
        .or_else(|| {
            if height.is_some() {
                best_video_format(metadata, height, false)
            } else {
                best_video_format(metadata, None, false)
            }
        });
    let audio = video.filter(|v| v.acodec.as_deref().unwrap_or("none") != "none").or_else(|| audio_format_id
        .and_then(|id| metadata.formats.iter().find(|format| format.format_id == id))
        .or_else(|| preferred_audio_for_video(metadata, video))
        .or_else(|| best_audio_format(metadata, true))
        .or_else(|| best_audio_format(metadata, false)));

    let can_mp4 = can_mux_mp4(video, audio);

    let audio_only = video.is_none() && audio.is_some();
    let use_mkv = if audio_only {
        false
    } else {
        match save_strategy {
        SaveStrategy::Mkv => true,
        SaveStrategy::Mp4 => false,
        SaveStrategy::Auto => !can_mp4,
        }
    };

    let format_selector = match (video, audio) {
        (Some(video), Some(_)) if video.acodec.as_deref().unwrap_or("none") != "none" => video.format_id.clone(),
        (Some(video), Some(audio)) => format!("{}+{}", video.format_id, audio.format_id),
        (Some(video), None) => video.format_id.clone(),
        (None, Some(audio)) => audio.format_id.clone(),
        _ => build_fallback_format_selector(use_mkv, height),
    };

    DownloadSelection {
        format_selector,
        use_mkv,
        audio_only,
        video,
        audio,
    }
}

fn can_mux_mp4(video: Option<&FormatInfo>, audio: Option<&FormatInfo>) -> bool {
    let Some(video) = video else { return false };
    let vc = video.vcodec.as_deref().unwrap_or_default().to_ascii_lowercase();
    let video_ok = video.ext.as_deref().unwrap_or_default().eq_ignore_ascii_case("mp4")
        && ["av01", "av1", "avc1", "h264"].iter().any(|prefix| vc.starts_with(prefix));
    let audio_ok = audio.map(|a| {
        let ac = a.acodec.as_deref().unwrap_or_default().to_ascii_lowercase();
        matches!(a.ext.as_deref().unwrap_or_default().to_ascii_lowercase().as_str(), "mp4" | "m4a")
            && (ac.starts_with("mp4a") || ac.starts_with("aac"))
    }).unwrap_or(true);
    video_ok && audio_ok
}

fn preferred_mp4_pair(metadata: &ProbeResult, video: &FormatInfo) -> bool {
    let audio = if video.acodec.as_deref().unwrap_or("none") != "none" { Some(video) }
        else { preferred_audio_for_video(metadata, Some(video)).or_else(|| best_audio_format(metadata, true)).or_else(|| best_audio_format(metadata, false)) };
    can_mux_mp4(Some(video), audio)
}

fn best_video_format(metadata: &ProbeResult, height: Option<u64>, mp4_h264_only: bool) -> Option<&FormatInfo> {
    metadata
        .formats
        .iter()
        .filter(|format| format.vcodec.as_deref().unwrap_or("none") != "none")
        .filter(|format| height.map(|value| format.height.unwrap_or(0) <= value).unwrap_or(true))
        .filter(|format| {
            if !mp4_h264_only {
                return true;
            }
            let ext_ok = format.ext.as_deref().map(|ext| ext.eq_ignore_ascii_case("mp4")).unwrap_or(false);
            let codec = format.vcodec.as_deref().unwrap_or_default().to_ascii_lowercase();
            ext_ok && (codec.starts_with("av01") || codec.starts_with("avc1") || codec.starts_with("h264"))
        })
        .max_by(|a, b| {
            a.height
                .unwrap_or(0)
                .cmp(&b.height.unwrap_or(0))
                .then_with(|| preferred_mp4_pair(metadata, a).cmp(&preferred_mp4_pair(metadata, b)))
                .then_with(|| a.fps.partial_cmp(&b.fps).unwrap_or(std::cmp::Ordering::Equal))
                .then_with(|| video_codec_rank(b.vcodec.as_deref()).cmp(&video_codec_rank(a.vcodec.as_deref())))
                .then_with(|| container_rank(b.ext.as_deref()).cmp(&container_rank(a.ext.as_deref())))
                .then_with(|| a.tbr.partial_cmp(&b.tbr).unwrap_or(std::cmp::Ordering::Equal))
        })
}

fn container_rank(ext: Option<&str>) -> u8 {
    if ext.unwrap_or_default().eq_ignore_ascii_case("mp4") {
        0
    } else {
        1
    }
}

fn video_codec_rank(codec: Option<&str>) -> u8 {
    let value = codec.unwrap_or_default().to_ascii_lowercase();
    if value.starts_with("av01") || value.starts_with("av1") {
        0
    } else if value.starts_with("avc1") || value.starts_with("h264") {
        1
    } else if value.starts_with("vp09") || value.starts_with("vp9") {
        2
    } else if value.starts_with("vp8") {
        3
    } else {
        9
    }
}

fn best_audio_format(metadata: &ProbeResult, aac_only: bool) -> Option<&FormatInfo> {
    metadata
        .formats
        .iter()
        .filter(|format| format.vcodec.as_deref().unwrap_or("none") == "none")
        .filter(|format| format.acodec.as_deref().unwrap_or("none") != "none")
        .filter(|format| {
            if !aac_only {
                return true;
            }
            let ext_ok = format.ext.as_deref().map(|ext| ext.eq_ignore_ascii_case("m4a") || ext.eq_ignore_ascii_case("mp4")).unwrap_or(false);
            let codec = format.acodec.as_deref().unwrap_or_default().to_ascii_lowercase();
            ext_ok && (codec.starts_with("mp4a") || codec.starts_with("aac"))
        })
        .max_by(|a, b| a.abr.partial_cmp(&b.abr).unwrap_or(std::cmp::Ordering::Equal))
}

fn preferred_audio_for_video<'a>(
    metadata: &'a ProbeResult,
    video: Option<&FormatInfo>,
) -> Option<&'a FormatInfo> {
    let video_codec = video
        .and_then(|format| format.vcodec.as_deref())
        .unwrap_or_default()
        .to_ascii_lowercase();
    let prefer_opus = video_codec.starts_with("vp09") || video_codec.starts_with("vp9");

    metadata
        .formats
        .iter()
        .filter(|format| format.vcodec.as_deref().unwrap_or("none") == "none")
        .filter(|format| format.acodec.as_deref().unwrap_or("none") != "none")
        .filter(|format| {
            let codec = format.acodec.as_deref().unwrap_or_default().to_ascii_lowercase();
            if prefer_opus {
                codec.starts_with("opus")
            } else {
                let ext_ok = format
                    .ext
                    .as_deref()
                    .map(|ext| ext.eq_ignore_ascii_case("m4a") || ext.eq_ignore_ascii_case("mp4"))
                    .unwrap_or(false);
                ext_ok && (codec.starts_with("mp4a") || codec.starts_with("aac"))
            }
        })
        .max_by(|a, b| a.abr.partial_cmp(&b.abr).unwrap_or(std::cmp::Ordering::Equal))
}

fn build_fallback_format_selector(use_mkv: bool, height: Option<u64>) -> String {
    let height_filter = height
        .map(|value| format!("[height<={value}]"))
        .unwrap_or_default();

    if use_mkv {
        format!("bestvideo{height_filter}+bestaudio/best{height_filter}")
    } else {
        format!(
            "bestvideo[ext=mp4][vcodec^=avc1]{height_filter}+bestaudio[ext=m4a]/best[ext=mp4]{height_filter}/best{height_filter}"
        )
    }
}

fn run_process<I,S>(program:&Path,_display_name:&str,args:I)->Result<ProcessOutput,String> where I:IntoIterator<Item=S>,S:Into<OsString>{process::run(program,args.into_iter().map(Into::into).collect(),std::time::Duration::from_secs(180),|_|{})}

fn compact_log(stdout: String, stderr: String) -> String {
    [stdout.trim(), stderr.trim()]
        .into_iter()
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

fn clean_proxy(proxy: Option<&str>) -> Option<String> {
    proxy
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn normalize_thumbnail_url(url: &str) -> String {
    let trimmed = url.trim();
    if let Some(path) = trimmed.strip_prefix("//") {
        format!("https://{path}")
    } else if let Some(path) = trimmed.strip_prefix("http://") {
        format!("https://{path}")
    } else {
        trimmed.to_string()
    }
}

fn fetch_thumbnail_data_url(url: &str, referer: &str) -> Option<String> {
    if url.starts_with("data:") {
        return Some(url.to_string());
    }

    let mut args = vec![
        OsString::from("-L"),
        OsString::from("-sS"),
        OsString::from("--max-time"),
        OsString::from("10"),
        OsString::from("-A"),
        OsString::from("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36"),
    ];

    if !referer.trim().is_empty() {
        args.push(OsString::from("-e"));
        args.push(OsString::from(referer));
    }

    args.push(OsString::from("--"));
    args.push(OsString::from(url));
    let bytes = run_curl_bytes(args).ok()?;
    if bytes.is_empty() || bytes.len() > 8 * 1024 * 1024 {
        return None;
    }
    let mime = image_mime_from_bytes(&bytes).or_else(|| image_mime_from_url(url))?;
    Some(format!("data:{mime};base64,{}", base64_encode(&bytes)))
}

fn run_curl_bytes(args: Vec<OsString>) -> Result<Vec<u8>, String> {
    let output = hidden_command(Path::new("curl.exe"))
        .args(["--connect-timeout","15","--max-time","30"])
        .args(args)
        .output()
        .map_err(|error| format!("无法启动 curl：{error}"))?;
    if output.status.success() {
        Ok(output.stdout)
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_string())
    }
}

fn image_mime_from_bytes(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some("image/jpeg")
    } else if bytes.starts_with(&[0x89, b'P', b'N', b'G']) {
        Some("image/png")
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        Some("image/gif")
    } else if bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WEBP") {
        Some("image/webp")
    } else {
        None
    }
}

fn image_mime_from_url(url: &str) -> Option<&'static str> {
    let lower = url.to_ascii_lowercase();
    if lower.contains(".jpg") || lower.contains(".jpeg") {
        Some("image/jpeg")
    } else if lower.contains(".png") {
        Some("image/png")
    } else if lower.contains(".webp") {
        Some("image/webp")
    } else if lower.contains(".gif") {
        Some("image/gif")
    } else {
        None
    }
}

fn base64_encode(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut output = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b0 = chunk[0];
        let b1 = *chunk.get(1).unwrap_or(&0);
        let b2 = *chunk.get(2).unwrap_or(&0);
        output.push(TABLE[(b0 >> 2) as usize] as char);
        output.push(TABLE[(((b0 & 0b0000_0011) << 4) | (b1 >> 4)) as usize] as char);
        if chunk.len() > 1 {
            output.push(TABLE[(((b1 & 0b0000_1111) << 2) | (b2 >> 6)) as usize] as char);
        } else {
            output.push('=');
        }
        if chunk.len() > 2 {
            output.push(TABLE[(b2 & 0b0011_1111) as usize] as char);
        } else {
            output.push('=');
        }
    }
    output
}

fn start_bilibili_qr_login_blocking(app: &AppHandle) -> Result<BilibiliQrStart, String> {
    let output = run_curl(vec![
        OsString::from("-sS"),
        OsString::from("https://passport.bilibili.com/x/passport-login/web/qrcode/generate?source=main-fe-header"),
    ])?;
    let json: Value = serde_json::from_str(output.trim())
        .map_err(|error| format!("Bilibili 二维码响应解析失败：{error}"))?;
    if json.get("code").and_then(Value::as_i64).unwrap_or(-1) != 0 {
        return Err(json
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("Bilibili 二维码生成失败")
            .to_string());
    }
    let data = json.get("data").ok_or_else(|| "Bilibili 二维码响应缺少 data。".to_string())?;
    let url = data
        .get("url")
        .and_then(Value::as_str)
        .ok_or_else(|| "Bilibili 二维码响应缺少 url。".to_string())?
        .to_string();
    let qrcode_key = data
        .get("qrcode_key")
        .and_then(Value::as_str)
        .or_else(|| query_value(&url, "qrcode_key"))
        .ok_or_else(|| "Bilibili 二维码响应缺少 qrcode_key。".to_string())?
        .to_string();
    let _ = fs::create_dir_all(app_data_dir(app));
    Ok(BilibiliQrStart { url, qrcode_key })
}

fn poll_bilibili_qr_login_blocking(app: &AppHandle, qrcode_key: &str) -> Result<BilibiliQrPoll, String> {
    let api = format!(
        "https://passport.bilibili.com/x/passport-login/web/qrcode/poll?qrcode_key={}&source=main-fe-header",
        qrcode_key.trim()
    );
    let raw = run_curl(vec![OsString::from("-sS"), OsString::from("-i"), OsString::from(api)])?;
    let (headers, body) = split_http_response(&raw);
    let json: Value = serde_json::from_str(body.trim())
        .map_err(|error| format!("Bilibili 扫码状态解析失败：{error}"))?;
    let data = json.get("data").unwrap_or(&Value::Null);
    let code = data.get("code").and_then(Value::as_i64).unwrap_or(-1);

    match code {
        0 => {
            let login_url = data.get("url").and_then(Value::as_str).unwrap_or_default();
            let mut cookies = cookies_from_set_cookie_headers(headers);
            cookies.extend(cookies_from_login_url(login_url));
            let expires = query_value(login_url, "Expires")
                .and_then(|value| value.parse::<u64>().ok())
                .unwrap_or(1_893_456_000);
            save_bilibili_cookies(app, &cookies, expires)?;
            let cookie_file = bilibili_cookies_path(app).to_string_lossy().to_string();
            Ok(BilibiliQrPoll {
                status: "success".to_string(),
                message: "Bilibili 登录成功".to_string(),
                logged_in: true,
                cookie_file: Some(cookie_file),
            })
        }
        86101 => Ok(BilibiliQrPoll {
            status: "waiting".to_string(),
            message: "等待 Bilibili APP 扫码".to_string(),
            logged_in: false,
            cookie_file: None,
        }),
        86090 => Ok(BilibiliQrPoll {
            status: "confirming".to_string(),
            message: "已扫码，请在 Bilibili APP 上确认登录".to_string(),
            logged_in: false,
            cookie_file: None,
        }),
        86038 => Ok(BilibiliQrPoll {
            status: "expired".to_string(),
            message: "二维码已过期，请重新生成".to_string(),
            logged_in: false,
            cookie_file: None,
        }),
        _ => Ok(BilibiliQrPoll {
            status: "error".to_string(),
            message: data
                .get("message")
                .and_then(Value::as_str)
                .or_else(|| json.get("message").and_then(Value::as_str))
                .unwrap_or("Bilibili 登录状态未知")
                .to_string(),
            logged_in: false,
            cookie_file: None,
        }),
    }
}

fn run_curl(args: Vec<OsString>) -> Result<String, String> {
    let output = hidden_command(Path::new("curl.exe"))
        .args(args)
        .output()
        .map_err(|error| format!("无法启动 curl：{error}"))?;
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    if output.status.success() {
        Ok(stdout)
    } else {
        Err(format!("curl 执行失败：{}", stderr.trim()))
    }
}

fn split_http_response(raw: &str) -> (&str, &str) {
    raw.rsplit_once("\r\n\r\n")
        .or_else(|| raw.rsplit_once("\n\n"))
        .unwrap_or(("", raw))
}

fn cookies_from_set_cookie_headers(headers: &str) -> Vec<(String, String)> {
    headers
        .lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            if !trimmed.to_ascii_lowercase().starts_with("set-cookie:") {
                return None;
            }
            let cookie = trimmed.split_once(':')?.1.trim().split(';').next()?.trim();
            let (name, value) = cookie.split_once('=')?;
            Some((name.to_string(), value.to_string()))
        })
        .collect()
}

fn cookies_from_login_url(url: &str) -> Vec<(String, String)> {
    ["SESSDATA", "bili_jct", "DedeUserID", "DedeUserID__ckMd5", "sid"]
        .into_iter()
        .filter_map(|name| query_value(url, name).map(|value| (name.to_string(), value.to_string())))
        .collect()
}

fn query_value<'a>(url: &'a str, key: &str) -> Option<&'a str> {
    let query = url.split_once('?')?.1;
    query
        .split('&')
        .filter_map(|part| part.split_once('='))
        .find_map(|(name, value)| (name == key).then_some(value))
}

fn app_data_dir(app: &AppHandle) -> PathBuf {
    #[cfg(debug_assertions)]
    if let Some(path)=std::env::var_os("VIDEOTOOL_TEST_DIR"){return PathBuf::from(path)}
    app.path()
        .app_data_dir()
        .unwrap_or_else(|_| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
}

fn bilibili_cookies_path(app: &AppHandle) -> PathBuf {
    app_data_dir(app).join("bilibili.cookies.txt")
}

fn save_bilibili_cookies(app: &AppHandle, cookies: &[(String, String)], expires: u64) -> Result<(), String> {
    if cookies.is_empty() {
        return Err("Bilibili 登录成功，但没有拿到 cookies。".to_string());
    }
    let path = bilibili_cookies_path(app);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| format!("无法创建登录信息目录：{error}"))?;
    }
    let mut lines = vec![
        "# Netscape HTTP Cookie File".to_string(),
        "# Generated by Video Downloader".to_string(),
    ];
    for (name, value) in cookies {
        if value.trim().is_empty() {
            continue;
        }
        lines.push(format!(".bilibili.com\tTRUE\t/\tTRUE\t{expires}\t{name}\t{value}"));
    }
    fs::write(&path, lines.join("\n")).map_err(|error| format!("无法保存 Bilibili cookies：{error}"))
}

fn clean_cookies_browser(browser: Option<&str>) -> Option<String> {
    let value = browser?.trim().to_ascii_lowercase();
    match value.as_str() {
        "edge" | "chrome" | "chromium" | "firefox" | "brave" | "vivaldi" | "opera" => Some(value),
        _ => None,
    }
}

fn clean_cookies_file(path: Option<&str>) -> Option<PathBuf> {
    let path = PathBuf::from(path?.trim());
    path.is_file().then_some(path)
}

fn push_cookies_args(
    app: &AppHandle,
    args: &mut Vec<OsString>,
    url: &str,
    browser: Option<&str>,
    cookies_file: Option<&str>,
) -> Result<Option<browser_auth::CookieLease>,String> {
    let lease=browser_auth::lease(url)?;
    if let Some(ref active)=lease {args.extend([OsString::from("--cookies"),active.0.clone().into_os_string()]);return Ok(lease)}
    let explicit_file = clean_cookies_file(cookies_file);
    let stored_bilibili_file = tauri::Url::parse(url).ok().and_then(|u|u.host_str().map(str::to_owned)).map(|h|h=="bilibili.com"||h.ends_with(".bilibili.com")||h=="b23.tv").unwrap_or(false).then(|| {
        let path = bilibili_cookies_path(app);
        path.is_file().then_some(path)
    }).flatten();

    if let Some(path) = explicit_file.or(stored_bilibili_file) {
        args.push(OsString::from("--cookies"));
        args.push(path.into_os_string());
    } else if let Some(browser) = clean_cookies_browser(browser) {
        args.push(OsString::from("--cookies-from-browser"));
        args.push(OsString::from(browser));
    }
    Ok(None)
}

fn h264_output_path(input_path:&Path)->PathBuf{let stem=input_path.file_stem().unwrap_or_default().to_string_lossy();input_path.with_file_name(format!("{}.h264.mp4",engine::safe_stem(&stem)))}

fn probe_media_stats(app: &AppHandle, input_path: &Path) -> Result<MediaStats, String> {
    let ffprobe = resolve_tool(app, "ffprobe");
    let output = run_process(
        &ffprobe,
        "ffprobe",
        vec![
            OsString::from("-v"),
            OsString::from("error"),
            OsString::from("-show_entries"),
            OsString::from("format=duration"),
            OsString::from("-of"),
            OsString::from("default=noprint_wrappers=1:nokey=1"),
            input_path.as_os_str().to_os_string(),
        ],
    )?;
    let duration_seconds = output
        .stdout
        .trim()
        .parse::<f64>()
        .map_err(|error| format!("无法读取视频时长：{error}"))?;
    if !duration_seconds.is_finite() || duration_seconds <= 0.0 {
        return Err("视频时长无效，无法计算目标码率。".to_string());
    }

    let size_bytes = fs::metadata(input_path)
        .map_err(|error| format!("无法读取源文件大小：{error}"))?
        .len();

    Ok(MediaStats {
        duration_seconds,
        size_bytes,
    })
}

fn build_bitrate_plan(stats: &MediaStats, quality_mode: &str) -> BitratePlan {
    let total_kbps = ((stats.size_bytes as f64 * 8.0) / stats.duration_seconds / 1000.0)
        .round()
        .max(1_200.0) as u64;
    let audio_kbps = if total_kbps >= 2_500 { 192 } else { 160 };
    let video_kbps = total_kbps.saturating_sub(audio_kbps).max(1_000);
    let (crf, preset, _multiplier) = match quality_mode {
        "size" => (Some(23), "veryfast", 1.25),
        "balanced" => (None, "medium", 1.0),
        _ => (Some(18), "slow", 2.4),
    };

    BitratePlan {
        audio_kbps,
        video_kbps,
        crf,
        preset,
    }
}

fn parse_download_progress_line(line: &str, fallback_file_name: &str) -> Option<DownloadProgress> {
    let marker = "__VD_PROGRESS__";
    let marker_start = line.find(marker)?;
    let raw = &line[marker_start + marker.len()..];
    let mut parts = raw.split('|');
    let percent_text = parts.next().unwrap_or_default().trim().trim_end_matches('%').trim();
    let speed = parts.next().unwrap_or_default().trim().to_string();
    let eta = parts.next().unwrap_or_default().trim().to_string();
    let percent = percent_text.parse::<f64>().ok()?;

    Some(DownloadProgress {
        percent: percent.clamp(0.0, 99.0),
        stage: "正在下载视频".to_string(),
        file_name: fallback_file_name.to_string(),
        speed: if speed.is_empty() || speed == "N/A" { "计算中".to_string() } else { speed },
        eta: if eta.is_empty() || eta == "N/A" { "计算中".to_string() } else { eta },
    })
}

fn resolve_tool(app: &AppHandle, tool_name: &str) -> PathBuf {
    if let Some(path)=updater::managed_path(app,tool_name){return path}
    let bundled=updater::resource_path(app,&format!("bin/{tool_name}.exe"));
    if bundled.is_file(){return bundled}
    let exe_name = if cfg!(target_os = "windows") {
        format!("{tool_name}.exe")
    } else {
        tool_name.to_string()
    };

    if let Ok(resource_dir) = app.path().resource_dir() {
        let candidate = resource_dir.join("bin").join(&exe_name);
        if candidate.is_file() {
            return candidate;
        }
    }

    resolve_tool_from_cwd(tool_name)
}

fn resolve_tool_from_cwd(tool_name: &str) -> PathBuf {
    let exe_name = if cfg!(target_os = "windows") {
        format!("{tool_name}.exe")
    } else {
        tool_name.to_string()
    };
    let local_candidate = std::env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(|parent| parent.join("resources").join("bin").join(&exe_name)));
    if let Some(candidate) = local_candidate {
        if candidate.is_file() {
            return candidate;
        }
    }
    PathBuf::from(tool_name)
}

fn hidden_command(program: &Path) -> Command {
    let mut command = Command::new(program);
    command.env("PYTHONIOENCODING","utf-8").env("PYTHONUTF8","1");
    #[cfg(target_os = "windows")]
    {
        command.creation_flags(CREATE_NO_WINDOW);
    }
    command
}

fn worker_thread_count() -> usize {
    std::thread::available_parallelism()
        .map(|count| count.get().saturating_sub(1).max(2))
        .unwrap_or(2)
}

fn main() {
    if browser_auth::native_entry(){return}
    tauri::Builder::default()
        .setup(|app| { queue::init(app.handle());
            #[cfg(debug_assertions)] if diagnostics::start(app.handle()){return Ok(())}
            browser_auth::start(app.handle()); updater::start_auto(app.handle()); Ok(()) })
        .on_window_event(|window,event| {if let tauri::WindowEvent::CloseRequested{api,..}=event {let q=window.state::<queue::Queue>();if !q.running.lock().unwrap().is_empty(){api.prevent_close();q.closing.store(true,std::sync::atomic::Ordering::Relaxed);q.data.lock().unwrap().paused=true;for flag in q.running.lock().unwrap().values(){flag.store(true,std::sync::atomic::Ordering::Relaxed);}let app=window.app_handle().clone();let _=window.hide();thread::spawn(move||{for _ in 0..100 {if app.state::<queue::Queue>().running.lock().unwrap().is_empty(){break}thread::sleep(std::time::Duration::from_millis(100));}app.exit(0);});}}})
        .invoke_handler(tauri::generate_handler![
            browser_auth::connect_browser, browser_auth::authorization_status, browser_auth::clear_authorization, browser_auth::import_authorization, browser_auth::extension_folder,
            transcode::inspect_transcode, transcode::inspect_media,
            check_environment,
            get_default_download_dir,
            select_download_dir,
            select_video_files,
            get_bilibili_login_status,
            clear_bilibili_login,
            start_bilibili_qr_login,
            poll_bilibili_qr_login,
            open_path,
            read_clipboard_text,
            probe_video,
            queue::clear_tasks,
            queue::queue_snapshot, queue::save_settings, queue::add_tasks, queue::task_action, updater::manage_tools, import_links
        ])
        .run(tauri::generate_context!())
        .expect("error while running 视频下载器");
}

fn validate_url(url:&str)->Result<(),String>{let parsed=tauri::Url::parse(url.trim()).map_err(|_|"视频链接无效")?;if !matches!(parsed.scheme(),"http"|"https")||parsed.host_str().is_none(){return Err("仅支持 HTTP / HTTPS 视频链接".into())}Ok(())}
fn yt_args(app:&AppHandle,args:Vec<OsString>)->Vec<OsString>{let mut base=vec!["--ignore-config".into(),"--no-playlist".into(),"--no-remote-components".into(),"--no-js-runtimes".into(),"--js-runtimes".into(),format!("deno:{}",resolve_tool(app,"deno").display()).into(),"--ffmpeg-location".into(),resolve_tool(app,"ffmpeg").into_os_string(),"--socket-timeout".into(),"20".into()];base.extend(args);base}
fn atomic_json(path:&Path,value:&impl Serialize)->Result<(),String>{let parent=path.parent().ok_or("无效存储路径")?;fs::create_dir_all(parent).map_err(|e|e.to_string())?;let temp=path.with_extension("next.json");let data=serde_json::to_vec_pretty(value).map_err(|e|e.to_string())?;{use std::io::Write;let mut file=fs::File::create(&temp).map_err(|e|e.to_string())?;file.write_all(&data).and_then(|_|file.sync_all()).map_err(|e|e.to_string())?;}fs::rename(&temp,path).map_err(|e|e.to_string())}
#[tauri::command] fn import_links()->Result<Option<String>,String>{let Some(path)=rfd::FileDialog::new().add_filter("链接列表", &["txt"]).pick_file()else{return Ok(None)};if fs::metadata(&path).map_err(|e|e.to_string())?.len()>2_000_000{return Err("TXT 文件过大，最多 2 MB".into())}let text=fs::read_to_string(path).map_err(|e|format!("请使用 UTF-8 编码的 TXT：{e}"))?;Ok(Some(text.trim_start_matches('\u{feff}').to_string()))}
