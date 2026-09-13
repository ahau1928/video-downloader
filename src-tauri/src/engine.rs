use crate::*;
use sha2::{Digest,Sha256};
use std::{fs::OpenOptions,time::Duration};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NameConflict { pub existing: String, pub suggested: String }
pub fn parse_conflict(error: &str) -> Option<NameConflict> {
    serde_json::from_str(error.strip_prefix("NAME_CONFLICT:")?).ok()
}
fn conflict_error(existing: &Path, suggested: &Path) -> String {
    format!("NAME_CONFLICT:{}", serde_json::to_string(&NameConflict {
        existing: existing.to_string_lossy().into(), suggested: suggested.to_string_lossy().into(),
    }).unwrap())
}
pub fn codec_name(codec: &str) -> &str {
    if codec.starts_with("av01") || codec.starts_with("av1") {"AV1"}
    else if codec.starts_with("avc1") || codec.starts_with("h264") {"H264"}
    else if codec.starts_with("hvc1") || codec.starts_with("hev1") || codec.starts_with("hevc") {"H265"}
    else if codec.starts_with("vp09") || codec.starts_with("vp9") {"VP9"}
    else if codec.starts_with("vp8") {"VP8"}
    else if codec.starts_with("mp4a") || codec.starts_with("aac") {"AAC"}
    else if codec.starts_with("opus") {"Opus"} else {"Unknown"}
}
pub fn format_suffix(video: Option<&FormatInfo>, audio: Option<&FormatInfo>, audio_only: bool, include_codecs: bool) -> String {
    if audio_only {return "MP3".into()}
    let mut parts=Vec::new();
    if let Some(v)=video {
        if let Some(h)=v.height {parts.push(format!("{h}p"))}
        if include_codecs {parts.push(codec_name(v.vcodec.as_deref().unwrap_or_default()).into());}
    }
    if include_codecs {if let Some(a)=audio {parts.push(codec_name(a.acodec.as_deref().unwrap_or_default()).into());}}
    if parts.is_empty(){"video".into()}else{parts.join("_")}
}
fn download_store(app: &AppHandle, dir: &Path) -> Result<PathBuf,String> {
    static MIGRATE: Mutex<()> = Mutex::new(());
    let _guard=MIGRATE.lock().unwrap();
    let absolute=fs::canonicalize(dir).map_err(|e|e.to_string())?;
    let key=format!("{:x}",Sha256::digest(absolute.to_string_lossy().to_lowercase().as_bytes()));
    let records=app_data_dir(app).join("downloads").join(key);
    fs::create_dir_all(&records).map_err(|e|e.to_string())?;
    let old=absolute.join(".video-downloader");
    // Preserve old receipts and resumable parts by moving, never deleting, this directory.
    if old.is_dir() && !records.join("legacy").exists() {
        if let Ok(meta)=fs::symlink_metadata(&old) {
            if !meta.file_type().is_symlink() && fs::canonicalize(&old).ok().and_then(|p|p.parent().map(Path::to_path_buf)).as_ref()==Some(&absolute) {
                let _=fs::rename(&old,records.join("legacy"));
            }
        }
    }
    Ok(records)
}
fn cleanup_work(root: &Path, work: &Path) {
    if let (Ok(root),Ok(work))=(fs::canonicalize(root),fs::canonicalize(work)) {
        if work.parent()==Some(root.as_path()) && work.file_name().unwrap_or_default().to_string_lossy().ends_with("-work") {
            let _=fs::remove_dir_all(work);
        }
    }
}
struct DownloadLock { file: Option<fs::File>, path: PathBuf }
impl Drop for DownloadLock { fn drop(&mut self) {self.file.take();let _=fs::remove_file(&self.path);} }

pub fn unique_path(path:&Path)->PathBuf{if !path.exists(){return path.to_path_buf()}let stem=path.file_stem().unwrap_or_default().to_string_lossy();let ext=path.extension().unwrap_or_default().to_string_lossy();for n in 2..100_000{let p=path.with_file_name(format!("{stem} ({n}).{ext}"));if !p.exists(){return p}}path.with_file_name(format!("{stem}-{}.{}",queue::unique_id(),ext))}
pub fn safe_stem(s:&str)->String{let mut value=String::new();for ch in s.chars(){if value.len()+ch.len_utf8()>140{break}value.push(if ch.is_control()||"<>:\"/\\|?*".contains(ch){'_'}else{ch})}let value=value.trim().trim_end_matches(['.',' ']);if value.is_empty(){"video".into()}else{value.into()}}
pub(crate) struct Reservation(PathBuf);
impl Drop for Reservation{fn drop(&mut self){let _=fs::remove_file(&self.0);}}
pub(crate) fn reserve(path:&Path)->Result<(PathBuf,Reservation),String>{let mut candidate=unique_path(path);loop{let lock=candidate.with_extension(format!("{}.reserve",candidate.extension().unwrap_or_default().to_string_lossy()));match OpenOptions::new().create_new(true).write(true).open(&lock){Ok(_)=>return Ok((candidate,Reservation(lock))),Err(e) if e.kind()==std::io::ErrorKind::AlreadyExists=>{candidate=unique_path(&candidate.with_file_name(format!("{}-{}.{}",candidate.file_stem().unwrap_or_default().to_string_lossy(),queue::unique_id(),candidate.extension().unwrap_or_default().to_string_lossy())));},Err(e)=>return Err(e.to_string())}}}
// Hard-link publication is atomic and refuses an existing target on Windows and Unix.
pub(crate) fn publish(temp:&Path,planned:&Path)->Result<PathBuf,String>{let mut out=planned.to_path_buf();loop{match fs::hard_link(temp,&out){Ok(())=>{fs::remove_file(temp).map_err(|e|e.to_string())?;return Ok(out)},Err(e) if e.kind()==std::io::ErrorKind::AlreadyExists=>out=unique_path(&out),Err(_)=>{let mut src=fs::File::open(temp).map_err(|e|e.to_string())?;match OpenOptions::new().write(true).create_new(true).open(&out){Ok(mut dst)=>{if let Err(e)=std::io::copy(&mut src,&mut dst).and_then(|_|dst.sync_all()){drop(dst);let _=fs::remove_file(&out);return Err(e.to_string())}fs::remove_file(temp).map_err(|e|e.to_string())?;return Ok(out)},Err(e) if e.kind()==std::io::ErrorKind::AlreadyExists=>out=unique_path(&out),Err(e)=>return Err(e.to_string())}}}}}
pub fn download(app:AppHandle,request:DownloadRequest)->Result<DownloadResult,String>{validate_url(&request.url)?;let dir=PathBuf::from(request.output_dir.trim());if !dir.is_dir(){return Err("保存目录不存在".into())}let metadata=match request.metadata{Some(m)=>m,None=>probe_metadata(&app,&request.url,request.proxy.as_deref(),request.cookies_browser.as_deref(),request.cookies_file.as_deref())?};let mut selection=select_download_formats(&metadata,request.quality_height,request.video_format_id.as_deref(),request.audio_format_id.as_deref(),&request.save_strategy);if request.audio_only {selection.audio_only=true;selection.video=None;selection.audio=best_audio_format(&metadata,false);selection.format_selector=request.audio_format_id.clone().filter(|id|metadata.formats.iter().any(|f|&f.format_id==id&&format_has_audio(f))).or_else(||selection.audio.map(|f|f.format_id.clone())).unwrap_or_else(||"bestaudio/best".into());}let audio_only=selection.audio_only;let expected_audio=audio_only||selection.audio.is_some();let container=if audio_only{"mp3"}else if selection.use_mkv{"mkv"}else{"mp4"};let identity=if metadata.id.is_empty(){request.url.as_str()}else{metadata.id.as_str()};let identity=format!("{}|{}|{}|{}",metadata.extractor,identity,selection.format_selector,container);let key=format!("{:x}",Sha256::digest(identity.as_bytes()));let records=download_store(&app,&dir)?;
// Identical requests share a lock even if added through different URL spellings.
let lock=records.join(format!("{key}.lock"));let mut locked=None;for _ in 0..1200{if process::cancelled(){return Err("任务已取消".into())}let mut options=OpenOptions::new();options.write(true).create(true);#[cfg(windows)]{use std::os::windows::fs::OpenOptionsExt;options.share_mode(0);}match options.open(&lock){Ok(file)=>{locked=Some(file);break},Err(e) if e.raw_os_error()==Some(32)=>thread::sleep(Duration::from_millis(100)),Err(e)=>return Err(e.to_string())}}let _locked=DownloadLock{file:Some(locked.ok_or("相同视频正在下载，请稍后重试")?),path:lock};
let receipt=records.join(format!("{key}.json"));if let Ok(bytes)=fs::read(&receipt).or_else(|_|fs::read(records.join("legacy").join(format!("{key}.json")))).or_else(|_|fs::read(dir.join(".video-downloader").join(format!("{key}.json")))){if let Ok(mut output)=serde_json::from_slice::<DownloadResult>(&bytes){if let Ok(stats)=probe_media_stats(&app,Path::new(&output.output_path)){if stats.size_bytes>0&&(!expected_audio||stats.audio_codec.is_some()){output.skipped=true;output.log="同一视频及格式的有效输出已存在，已跳过".into();return Ok(output)}}}}
let clean=safe_stem(&metadata.title);
let suffix=format_suffix(selection.video,selection.audio,audio_only,request.filename_codecs);
let stem=if request.filename_suffix {format!("{clean}_{suffix}")}else{clean.clone()};
let mut planned=dir.join(format!("{stem}.{container}"));
let suggested=dir.join(format!("{clean}_{suffix}.{container}"));
let collision=if planned.exists(){Some(planned.clone())}else if !request.filename_suffix { ["mp4","mkv","mp3"].iter().map(|ext|dir.join(format!("{clean}.{ext}"))).find(|p|p.exists()) }else{None};
if let Some(existing)=collision {
    match request.conflict_action.as_str() {
        "suffix"=>planned=unique_path(&suggested),
        "number"=>planned=unique_path(&planned),
        _=>return Err(conflict_error(&existing,&suggested)),
    }
}
// Reservation collisions also ask before concurrent variants are published.
let marker=planned.with_extension(format!("{container}.reserve"));
if marker.exists() && request.conflict_action.is_empty(){return Err(conflict_error(&planned,&suggested))}
let (planned,_reserve)=reserve(&planned)?;
let work=records.join(format!("{key}-work"));
if !work.exists(){let _=fs::rename(records.join("legacy").join(format!("{key}-work")),&work);}
fs::create_dir_all(&work).map_err(|e|e.to_string())?;let template=work.join("media.%(ext)s");let mut args=vec!["--newline".into(),"--progress".into(),"--progress-template".into(),"download:__VD_PROGRESS__%(progress._percent_str)s|%(progress._speed_str)s|%(progress._eta_str)s".into(),"--format".into(),selection.format_selector.clone().into(),"--output".into(),template.into_os_string(),"--print".into(),"after_move:filepath".into(),"--no-simulate".into(),"--continue".into(),"--retries".into(),"3".into(),"--fragment-retries".into(),"3".into()];if audio_only{args.extend(["--extract-audio".into(),"--audio-format".into(),"mp3".into(),"--audio-quality".into(),"0".into()])}else{args.extend(["--merge-output-format".into(),container.into(),"--remux-video".into(),container.into()])}if let Some(proxy)=clean_proxy(request.proxy.as_deref()){args.extend(["--proxy".into(),proxy.into()]);}else{args.extend(["--proxy".into(),"".into()])}let _cookie_lease=if !metadata.anonymous {push_cookies_args(&app,&mut args,&request.url,request.cookies_browser.as_deref(),request.cookies_file.as_deref())?}else{None};args.extend(["--".into(),request.url.into()]);let mut last_emit=std::time::Instant::now()-Duration::from_secs(1);let output=process::run(&resolve_tool(&app,"yt-dlp"),yt_args(&app,args),Duration::from_secs(86400),|line|{if let Some(p)=parse_download_progress_line(line,&stem){if last_emit.elapsed()>Duration::from_millis(200){queue::progress(&app,p.percent.min(99.),&p.stage,&p.speed,&p.eta);last_emit=std::time::Instant::now();}}else if line.contains("[Merger]")||line.contains("[VideoRemuxer]"){queue::progress(&app,99.,"正在合并与封装","","")}})?;let actual=work.join(format!("media.{container}"));let stats=probe_media_stats(&app,&actual)?;if stats.size_bytes==0{return Err("下载输出为空".into())}if expected_audio&&stats.audio_codec.is_none(){return Err("下载结果缺少所选音轨，未将任务标记为完成；请重新解析并重试".into())}if process::cancelled(){return Err("任务已取消".into())}if planned.exists() && request.conflict_action.is_empty(){return Err(conflict_error(&planned,&suggested))}let final_path=publish(&actual,&planned)?;cleanup_work(&records,&work);let result=DownloadResult{output_path:final_path.to_string_lossy().into_owned(),container:container.into(),selected_vcodec:selection.video.and_then(|f|f.vcodec.clone()),selected_acodec:stats.audio_codec,skipped:false,log:compact_log(output.stdout,output.stderr)};atomic_json(&receipt,&result)?;Ok(result)}
pub fn transcode(app:AppHandle,mut request:TranscodeRequest)->Result<DownloadResult,String>{if let Some(options)=request.options.take(){return crate::transcode::run(app,request,options)}let input=fs::canonicalize(request.input_path.trim()).map_err(|e|format!("找不到源文件：{e}"))?;let target=request.output_path.as_ref().filter(|p|!p.trim().is_empty()).map(PathBuf::from).unwrap_or_else(||h264_output_path(&input));if target.exists()&&fs::canonicalize(&target).ok().as_ref()==Some(&input){return Err("输入和输出不能是同一个文件".into())}let parent=target.parent().ok_or("输出路径无效")?;if !parent.is_dir(){return Err("输出目录不存在".into())}let (target,_reserve)=reserve(&target)?;let temp=parent.join(format!(".transcode-{}.mp4",queue::unique_id()));let stats=probe_media_stats(&app,&input)?;let bitrate=build_bitrate_plan(&stats,request.quality_mode.as_deref().unwrap_or("balanced"));let mut args:Vec<OsString>=vec!["-n".into(),"-nostdin".into(),"-hide_banner".into(),"-nostats".into(),"-i".into(),input.as_os_str().into(),"-map".into(),"0:v:0".into(),"-map".into(),"0:a:0?".into(),"-c:v".into(),"libx264".into(),"-preset".into(),bitrate.preset.into(),"-pix_fmt".into(),"yuv420p".into(),"-vf".into(),"scale=trunc(iw/2)*2:trunc(ih/2)*2".into(),"-c:a".into(),"aac".into(),"-b:a".into(),format!("{}k",bitrate.audio_kbps).into(),"-threads".into(),worker_thread_count().to_string().into(),"-movflags".into(),"+faststart".into(),"-progress".into(),"pipe:1".into()];if let Some(crf)=bitrate.crf{args.extend(["-crf".into(),crf.to_string().into()])}else{args.extend(["-b:v".into(),format!("{}k",bitrate.video_kbps).into()])}args.push(temp.as_os_str().into());let result=process::run(&resolve_tool(&app,"ffmpeg"),args,Duration::from_secs(86400),|line|{if let Some(t)=line.strip_prefix("out_time_us=").and_then(|s|s.parse::<f64>().ok()){queue::progress(&app,(t/1e6/stats.duration_seconds*100.).clamp(0.,99.),"正在转码","","")}});let output=match result{Ok(o)=>o,Err(e)=>{let _=fs::remove_file(&temp);return Err(e)}};let checked=probe_media_stats(&app,&temp)?;if checked.size_bytes==0||(checked.duration_seconds-stats.duration_seconds).abs()>stats.duration_seconds.mul_add(0.02,1.){return Err(format!("转码输出未通过时长验证，源文件已保留；临时文件：{}",temp.display()))}if process::cancelled(){let _=fs::remove_file(&temp);return Err("任务已取消".into())}let final_path=publish(&temp,&target)?;let mut log=compact_log(output.stdout,output.stderr);if request.delete_source.unwrap_or(false){if let Err(e)=fs::remove_file(&input){log.push_str(&format!("\n输出成功，但源文件删除失败：{e}"));}}Ok(DownloadResult{output_path:final_path.to_string_lossy().into_owned(),container:"mp4".into(),selected_vcodec:Some("h264".into()),selected_acodec:Some("aac".into()),skipped:false,log})}

#[cfg(test)]mod tests{use super::*;#[test]fn names_are_safe(){assert_eq!(safe_stem("a/b:c?"),"a_b_c_");assert!(safe_stem(&"中".repeat(100)).len()<=140)}#[test]fn publish_never_overwrites(){let dir=std::env::temp_dir().join(queue::unique_id());fs::create_dir(&dir).unwrap();let target=dir.join("test.mp4");fs::write(&target,b"original").unwrap();let temp=dir.join("tmp");fs::write(&temp,b"new").unwrap();let out=publish(&temp,&target).unwrap();assert_ne!(out,target);assert_eq!(fs::read(&target).unwrap(),b"original");assert_eq!(fs::read(out).unwrap(),b"new");fs::remove_dir_all(dir).unwrap();}#[test]fn mp4_output_is_distinct(){let input=Path::new("C:/videos/movie.mp4");assert_ne!(input,h264_output_path(input));}}

