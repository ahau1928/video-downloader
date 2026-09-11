use crate::*;
use std::time::Duration;

#[derive(Debug,Clone,Serialize,Deserialize)]
#[serde(default)]
pub struct Options {
    pub codec:String,pub mode:String,pub crf_h264:f64,pub crf_h265:f64,pub preset_h264:String,pub preset_h265:String,
    pub target_mb:f64,pub audio:String,pub audio_kbps:u64,pub resolution:String,pub width:u32,pub height:u32,
    pub lock_ratio:bool,pub anchor:String,pub fit:String,pub suffix_mode:String,pub suffix:String,pub container:String,pub color:String,
}
impl Default for Options{fn default()->Self{Self{codec:"h264".into(),mode:"crf".into(),crf_h264:23.,crf_h265:28.,preset_h264:"medium".into(),preset_h265:"medium".into(),target_mb:100.,audio:"encode".into(),audio_kbps:192,resolution:"source".into(),width:1920,height:1080,lock_ratio:true,anchor:"width".into(),fit:"pad".into(),suffix_mode:"auto".into(),suffix:String::new(),container:"mp4".into(),color:"auto".into()}}}
#[derive(Debug,Clone,Serialize,Deserialize)]
pub struct Media {pub path:String,pub duration:f64,pub size:u64,pub width:u32,pub height:u32,pub codec:String,pub fps:f64,pub audio_codec:Option<String>,pub audio_kbps:Option<f64>,pub audio_bytes:Option<u64>,pub hdr:bool,pub depth:u32,pub video_index:u64,pub audio_index:Option<u64>,pub transfer:String,pub primaries:String,pub space:String,pub range:String,pub dynamic_hdr:bool}
#[derive(Debug,Clone,Serialize)]
pub struct Plan {pub media:Media,pub width:u32,pub height:u32,pub audio_kbps:f64,pub audio_mb:f64,pub video_kbps:Option<f64>,pub output_name:String,pub warning:String,pub filter:String,pub pixel_format:String}
fn num(v:&Value)->Option<f64>{v.as_f64().or_else(||v.as_str()?.parse().ok())}
fn ratio(s:&str)->f64{let parts:Vec<_>=s.split(['/',':']).collect();if parts.len()!=2{return 1.}let a=parts[0].parse::<f64>().unwrap_or(1.);let b=parts[1].parse::<f64>().unwrap_or(1.);if a>0.&&b>0.{a/b}else{1.}}
fn even(n:f64)->u32{((n/2.).round().max(1.)*2.) as u32}
pub fn probe(app:&AppHandle,path:&Path,measure_audio:bool)->Result<Media,String>{
    let raw=process::run(&resolve_tool(app,"ffprobe"),vec!["-v".into(),"error".into(),"-show_streams".into(),"-show_format".into(),"-of".into(),"json".into(),path.as_os_str().into()],Duration::from_secs(180),|_|{})?;
    let data:Value=serde_json::from_str(&raw.stdout).map_err(|_|"无法读取视频信息")?;let streams=data["streams"].as_array().ok_or("未找到媒体流")?;
    let v=streams.iter().find(|v|v["codec_type"]=="video"&&v["disposition"]["attached_pic"]!=1).ok_or("文件没有可转码的视频流")?;
    let a=streams.iter().find(|a|a["codec_type"]=="audio");let duration=num(&data["format"]["duration"]).or_else(||num(&v["duration"])).ok_or("无法读取视频时长")?;
    if !duration.is_finite()||duration<=0.{return Err("视频时长无效".into())}
    let mut width=even(v["width"].as_u64().unwrap_or(0) as f64*ratio(v["sample_aspect_ratio"].as_str().unwrap_or("1:1")));let mut height=even(v["height"].as_u64().unwrap_or(0) as f64);
    let sides=v["side_data_list"].as_array();let rotation=sides.and_then(|s|s.iter().find_map(|x|num(&x["rotation"]))).unwrap_or(0.);
    if (rotation.abs()%180.-90.).abs()<1.{std::mem::swap(&mut width,&mut height)}
    let pix=v["pix_fmt"].as_str().unwrap_or("");let depth=num(&v["bits_per_raw_sample"]).filter(|n|*n>0.).unwrap_or(if pix.contains("10"){10.}else if pix.contains("12"){12.}else{8.}) as u32;
    let transfer=v["color_transfer"].as_str().unwrap_or("unknown").to_string();let hdr=matches!(transfer.as_str(),"smpte2084"|"arib-std-b67");
    let audio_index=a.and_then(|a|a["index"].as_u64());let mut audio_bytes=None;
    if measure_audio&&audio_index.is_some(){let mut sum=0u64;process::run(&resolve_tool(app,"ffprobe"),vec!["-v".into(),"error".into(),"-select_streams".into(),audio_index.unwrap().to_string().into(),"-show_entries".into(),"packet=size".into(),"-of".into(),"csv=p=0".into(),path.as_os_str().into()],Duration::from_secs(1800),|line|{if let Some(n)=line.split(',').next().and_then(|v|v.parse::<u64>().ok()){sum=sum.saturating_add(n)}})?;audio_bytes=Some(sum);}
    Ok(Media{path:path.to_string_lossy().into(),duration,size:fs::metadata(path).map_err(|e|e.to_string())?.len(),width,height,codec:v["codec_name"].as_str().unwrap_or("未知").into(),fps:ratio(v["avg_frame_rate"].as_str().unwrap_or("0/1")),audio_codec:a.and_then(|a|a["codec_name"].as_str()).map(str::to_string),audio_kbps:audio_bytes.map(|b|b as f64*8./duration/1000.).or_else(||a.and_then(|a|num(&a["bit_rate"])).map(|n|n/1000.)),audio_bytes,hdr,depth,video_index:v["index"].as_u64().unwrap_or(0),audio_index,transfer,primaries:v["color_primaries"].as_str().unwrap_or("unknown").into(),space:v["color_space"].as_str().unwrap_or("unknown").into(),range:v["color_range"].as_str().unwrap_or("unknown").into(),dynamic_hdr:sides.is_some_and(|s|s.iter().any(|d|{let t=d["side_data_type"].as_str().unwrap_or("");t.contains("DOVI")||t.contains("HDR Dynamic")}))})
}
pub fn plan(m:Media,o:&Options)->Result<Plan,String>{
    if !["h264","h265"].contains(&o.codec.as_str())||!["crf","size"].contains(&o.mode.as_str())||!["mp4","mkv","mov"].contains(&o.container.as_str())||!["encode","copy","none"].contains(&o.audio.as_str())||!["source","custom"].contains(&o.resolution.as_str())||!["width","height"].contains(&o.anchor.as_str())||!["pad","crop","stretch"].contains(&o.fit.as_str())||!["auto","custom","none"].contains(&o.suffix_mode.as_str())||!["auto","preserve","sdr"].contains(&o.color.as_str()){return Err("转码设置无效".into())}
    let crf=if o.codec=="h264"{o.crf_h264}else{o.crf_h265};let preset=if o.codec=="h264"{&o.preset_h264}else{&o.preset_h265};
    if !crf.is_finite()||!(0.0..=51.0).contains(&crf)||!["ultrafast","superfast","veryfast","faster","fast","medium","slow","slower","veryslow"].contains(&preset.as_str()){return Err("CRF 或压制预设无效".into())}
    if (m.hdr||m.depth>8)&&o.color=="auto"{return Err("检测到 HDR / 高色深视频，请明确选择“保留色深与 HDR”或“转换为 SDR 8 位”".into())}
    if o.color=="preserve"&&(m.hdr||m.depth>8)&&o.codec!="h265"{return Err("保留高色深 / HDR 时请选择 H.265".into())}
    if o.color=="preserve"&&m.dynamic_hdr{return Err("当前不支持保留 Dolby Vision / HDR10+ 动态元数据；请保留原文件，或明确选择转换 SDR".into())}
    if o.color=="preserve"&&m.depth>10{return Err("当前仅支持保留最高 10 位色深，请选择转换 SDR 或保留原文件".into())}
    let (w,h)=if o.resolution=="source"{(m.width,m.height)}else{
        if !(2..=16384).contains(&o.width)||!(2..=16384).contains(&o.height){return Err("宽高需在 2～16384 像素之间".into())}
        if o.lock_ratio{if o.anchor=="height"{(even(o.height as f64*m.width as f64/m.height as f64),even(o.height as f64))}else{(even(o.width as f64),even(o.width as f64*m.height as f64/m.width as f64))}}else{(even(o.width as f64),even(o.height as f64))}
    };if w>16384||h>16384{return Err("计算后的分辨率过大".into())}
    let mut warning=String::new();if w>m.width||h>m.height{warning.push_str("输出尺寸大于源视频，放大不会增加真实细节。 ")}
    if m.hdr&&o.color=="preserve"{warning.push_str("保留 HDR 色彩与 10 位输出；不保证保留全部原始静态元数据。 ")}
    let audio_kbps=if m.audio_index.is_none()||o.audio=="none"{0.}else if o.audio=="copy"{
        let codec=m.audio_codec.as_deref().unwrap_or("");if o.container!="mkv"&&!matches!(codec,"aac"|"mp3"|"ac3"|"eac3"|"alac"){return Err(format!("当前不支持将 {codec} 直接复制到 {}；请选择 AAC 压制或 MKV",o.container.to_uppercase()))}
        if o.mode=="size"{m.audio_kbps.ok_or("无法测量音频体积，请改为指定音频码率")?}else{m.audio_kbps.unwrap_or(0.)}
    }else if o.audio_kbps==0{m.audio_kbps.ok_or("源音频码率未知，请指定音频码率")?}else{o.audio_kbps as f64};
    if o.audio=="encode"&&m.audio_index.is_some()&&!(16.0..=512.0).contains(&audio_kbps){return Err("AAC 音频码率需在 16～512 kbps 之间".into())}
    let audio_mb=if o.audio=="copy"{m.audio_bytes.map(|b|b as f64/1e6).unwrap_or(audio_kbps*m.duration/8000.)}else{audio_kbps*m.duration/8000.};
    let video_kbps=if o.mode=="size"{if !o.target_mb.is_finite()||!(0.1..=1_000_000.).contains(&o.target_mb){return Err("目标大小需在 0.1～1000000 MB 之间".into())}let kbps=(o.target_mb*0.98-audio_mb)*8000./m.duration;if kbps<50.{return Err("目标大小不足以容纳音频及有效视频，请增大目标或降低音频码率".into())}if kbps/(w as f64*h as f64*m.fps.max(1.))*1000.<0.03{warning.push_str("视频可用码率较低，可能出现明显画质损失。 ")}Some(kbps)}else{None};
    let mut filter=format!("scale={}:{},setsar=1",m.width,m.height);
    if m.hdr&&o.color=="sdr"{filter.push_str(",zscale=t=linear:npl=100,format=gbrpf32le,zscale=p=bt709,tonemap=tonemap=hable:desat=2,zscale=t=bt709:m=bt709:r=tv");}
    if o.resolution=="custom"{let scale=if o.lock_ratio||o.fit=="stretch"{format!("scale={w}:{h}")}else if o.fit=="crop"{format!("scale={w}:{h}:force_original_aspect_ratio=increase,crop={w}:{h}")}else{format!("scale={w}:{h}:force_original_aspect_ratio=decrease:force_divisible_by=2,pad={w}:{h}:(ow-iw)/2:(oh-ih)/2")};filter.push(',');filter.push_str(&scale)}filter.push_str(",setsar=1");
    let suffix=match o.suffix_mode.as_str(){"none"=>String::new(),"custom"=>{if o.suffix.chars().any(|c|c.is_control()||"<>:\"/\\|?*".contains(c))||o.suffix.len()>80{return Err("文件名后缀不能包含路径符号，且最多 80 字节".into())}o.suffix.trim().trim_matches(['.','_',' ']).to_string()},_=>{let mut s=format!("{}_{}",if o.codec=="h264"{"H264"}else{"H265"},if o.mode=="crf"{format!("CRF{crf}")}else{format!("{}MB",o.target_mb)});if o.resolution=="custom"{s.push_str(&format!("_{w}x{h}"))}s}};
    let stem=Path::new(&m.path).file_stem().unwrap_or_default().to_string_lossy();let output_name=format!("{}{}.{}",engine::safe_stem(&stem),if suffix.is_empty(){String::new()}else{format!("_{suffix}")},o.container);
    let pixel_format=if o.color=="preserve"&&(m.depth>8||m.hdr){"yuv420p10le"}else{"yuv420p"}.into();
    Ok(Plan{media:m,width:w,height:h,audio_kbps,audio_mb,video_kbps,output_name,warning,filter,pixel_format})
}
#[tauri::command]pub async fn inspect_transcode(app:AppHandle,path:String,options:Options)->Result<Plan,String>{tauri::async_runtime::spawn_blocking(move||{let q=app.state::<queue::Queue>();let _lock=q.tools.read().unwrap();let m=probe(&app,Path::new(&path),options.mode=="size"&&options.audio=="copy"||options.audio=="encode"&&options.audio_kbps==0)?;plan(m,&options)}).await.map_err(|e|e.to_string())?}
#[tauri::command]pub async fn inspect_media(app:AppHandle,path:String)->Result<Media,String>{tauri::async_runtime::spawn_blocking(move||{let q=app.state::<queue::Queue>();let _lock=q.tools.read().unwrap();probe(&app,Path::new(&path),false)}).await.map_err(|e|e.to_string())?}

struct Workspace(PathBuf);
impl Drop for Workspace{fn drop(&mut self){if self.0.file_name().is_some_and(|s|s.to_string_lossy().starts_with(".transcode-work-")){let _=fs::remove_dir_all(&self.0);}}}
pub fn run(app:AppHandle,request:TranscodeRequest,o:Options)->Result<DownloadResult,String>{
    let input=fs::canonicalize(&request.input_path).map_err(|e|e.to_string())?;
    let p=plan(probe(&app,&input,o.mode=="size"&&o.audio=="copy"||o.audio=="encode"&&o.audio_kbps==0)?,&o)?;
    let parent=request.output_path.as_ref().map(PathBuf::from).unwrap_or_else(||input.parent().unwrap().to_path_buf());if !parent.is_dir(){return Err("输出目录不存在".into())}
    let (target,_reservation)=engine::reserve(&parent.join(&p.output_name))?;
    let work=parent.join(format!(".transcode-work-{}",queue::unique_id()));fs::create_dir(&work).map_err(|e|e.to_string())?;let _workspace=Workspace(work.clone());let temp=work.join(format!("output.{}",o.container));let stats=work.join("pass");
    let passes=if o.mode=="size"{2}else{1};let mut logs=String::new();
    for pass in 1..=passes{
        let first=passes==2&&pass==1;let preset=if o.codec=="h264"{&o.preset_h264}else{&o.preset_h265};let crf=if o.codec=="h264"{o.crf_h264}else{o.crf_h265};
        let mut args:Vec<OsString>=vec!["-y".into(),"-nostdin".into(),"-hide_banner".into(),"-nostats".into(),"-i".into(),input.as_os_str().into(),"-map".into(),format!("0:{}",p.media.video_index).into(),"-c:v".into(),if o.codec=="h264"{"libx264"}else{"libx265"}.into(),"-preset".into(),preset.into(),"-pix_fmt".into(),p.pixel_format.clone().into(),"-vf".into(),p.filter.clone().into(),"-threads".into(),worker_thread_count().to_string().into(),"-fps_mode".into(),"passthrough".into(),"-map_metadata".into(),"-1".into(),"-map_chapters".into(),"-1".into(),"-progress".into(),"pipe:1".into()];
        if let Some(rate)=p.video_kbps{args.extend(["-b:v".into(),format!("{rate:.0}k").into(),"-pass".into(),pass.to_string().into(),"-passlogfile".into(),stats.as_os_str().into()])}else{args.extend(["-crf".into(),crf.to_string().into()])}
        if o.codec=="h265"{let mut params=format!("pools={}:frame-threads=2",worker_thread_count());if o.color=="sdr"&&p.media.hdr{params.push_str(":colorprim=bt709:transfer=bt709:colormatrix=bt709")}else{for (key,value) in [("colorprim",&p.media.primaries),("transfer",&p.media.transfer),("colormatrix",&p.media.space)]{if value!="unknown"{params.push_str(&format!(":{key}={value}"))}}}args.extend(["-x265-params".into(),params.into()]);if o.container!="mkv"{args.extend(["-tag:v".into(),"hvc1".into()])}}
        if o.color=="sdr"&&p.media.hdr{args.extend(["-color_primaries".into(),"bt709".into(),"-color_trc".into(),"bt709".into(),"-colorspace".into(),"bt709".into(),"-color_range".into(),"tv".into()])}else{for (key,value) in [("-color_primaries",&p.media.primaries),("-color_trc",&p.media.transfer),("-colorspace",&p.media.space),("-color_range",&p.media.range)]{if value!="unknown"{args.extend([key.into(),value.into()])}}}
        if first||o.audio=="none"||p.media.audio_index.is_none(){args.push("-an".into())}else{args.extend(["-map".into(),format!("0:{}",p.media.audio_index.unwrap()).into(),"-c:a".into(),if o.audio=="copy"{"copy"}else{"aac"}.into()]);if o.audio=="encode"{args.extend(["-b:a".into(),format!("{:.0}k",p.audio_kbps).into()])}}
        if first{args.extend(["-f".into(),"null".into(),"NUL".into()])}else{if o.container!="mkv"{args.extend(["-movflags".into(),"+faststart".into()])}args.push(temp.as_os_str().into())}
        let stage=if passes==1{"正在转码"}else if first{"第 1 / 2 遍：分析视频"}else{"第 2 / 2 遍：生成视频"};queue::progress(&app,(pass-1) as f64/passes as f64*100.,stage,"","");
        let output=process::run(&resolve_tool(&app,"ffmpeg"),args,Duration::from_secs(86400),|line|{if let Some(t)=line.strip_prefix("out_time_us=").and_then(|v|v.parse::<f64>().ok()){let progress=((pass-1) as f64+(t/1e6/p.media.duration).clamp(0.,1.))/passes as f64*99.;queue::progress(&app,progress,stage,"","")}})?;logs.push_str(&compact_log(output.stdout,output.stderr));
    }
    let check=probe(&app,&temp,false)?;if check.size==0||(check.duration-p.media.duration).abs()>p.media.duration*0.02+1.||check.width!=p.width||check.height!=p.height||check.codec!=if o.codec=="h265"{"hevc"}else{"h264"}{return Err("输出校验失败，源文件已保留".into())}
    if p.media.hdr&&((o.color=="preserve"&&(!check.hdr||check.depth!=10))||(o.color=="sdr"&&(check.hdr||check.depth!=8))){return Err("HDR / SDR 色彩校验失败，源文件已保留".into())}
    let expected_audio=p.media.audio_index.is_some()&&o.audio!="none";if check.audio_index.is_some()!=expected_audio{return Err("音轨校验失败，源文件已保留".into())}
    if o.mode=="size"{let actual=check.size as f64/1e6;logs.push_str(&format!("\n目标 {:.2} MB，实际 {:.2} MB，偏差 {:+.1}%。",o.target_mb,actual,(actual/o.target_mb-1.)*100.));}
    if process::cancelled(){return Err("任务已取消".into())}let final_path=engine::publish(&temp,&target)?;if request.delete_source.unwrap_or(false){if let Err(e)=fs::remove_file(&input){logs.push_str(&format!("\n输出完成，但无法删除源文件：{e}"));}}
    Ok(DownloadResult{output_path:final_path.to_string_lossy().into(),container:o.container,selected_vcodec:Some(check.codec),selected_acodec:check.audio_codec,skipped:false,log:logs})
}

#[cfg(test)]mod tests{
use super::*;
fn media()->Media{Media{path:"C:/Videos/test.mp4".into(),duration:600.,size:100000000,width:1920,height:1080,codec:"h264".into(),fps:30.,audio_codec:Some("aac".into()),audio_kbps:Some(192.),audio_bytes:Some(14400000),hdr:false,depth:8,video_index:0,audio_index:Some(1),transfer:"bt709".into(),primaries:"bt709".into(),space:"bt709".into(),range:"tv".into(),dynamic_hdr:false}}
#[test]fn size_deducts_audio_and_overhead(){let mut o=Options::default();o.mode="size".into();let a=plan(media(),&o).unwrap();assert!((a.audio_mb-14.4).abs()<0.01);o.audio_kbps=320;let b=plan(media(),&o).unwrap();assert!((a.video_kbps.unwrap()-b.video_kbps.unwrap()-128.).abs()<0.01);o.target_mb=1.;assert!(plan(media(),&o).is_err());}
#[test]fn dimensions_and_container_validation(){let mut o=Options::default();o.resolution="custom".into();o.width=1281;assert_eq!((plan(media(),&o).unwrap().width,plan(media(),&o).unwrap().height),(1282,720));o.audio="copy".into();let mut m=media();m.audio_codec=Some("opus".into());assert!(plan(m.clone(),&o).is_err());o.container="mkv".into();assert!(plan(m,&o).is_ok());}
#[test]fn hdr_needs_explicit_choice(){let mut m=media();m.hdr=true;m.depth=10;let mut o=Options::default();assert!(plan(m.clone(),&o).is_err());o.color="preserve".into();assert!(plan(m.clone(),&o).is_err());o.codec="h265".into();assert_eq!(plan(m.clone(),&o).unwrap().pixel_format,"yuv420p10le");m.dynamic_hdr=true;assert!(plan(m,&o).is_err());}
#[test]fn missing_source_bitrate_and_bad_suffix_rejected(){let mut o=Options::default();o.audio_kbps=0;let mut m=media();m.audio_kbps=None;assert!(plan(m,&o).is_err());o.audio_kbps=192;o.suffix_mode="custom".into();o.suffix="../overwrite".into();assert!(plan(media(),&o).is_err());}
}

#[cfg(debug_assertions)]
pub fn self_test(app:&AppHandle,dir:&Path)->Result<Vec<String>,String>{
    let source=dir.join("transcode-source.mp4");let ffmpeg=resolve_tool(app,"ffmpeg");
    process::run(&ffmpeg,vec!["-y".into(),"-f".into(),"lavfi".into(),"-i".into(),"testsrc2=size=640x360:rate=24".into(),"-f".into(),"lavfi".into(),"-i".into(),"sine=frequency=440:sample_rate=48000".into(),"-t".into(),"8".into(),"-c:v".into(),"libx264".into(),"-preset".into(),"ultrafast".into(),"-crf".into(),"18".into(),"-c:a".into(),"aac".into(),"-b:a".into(),"192k".into(),source.as_os_str().into()],Duration::from_secs(60),|_|{})?;
    let encode=|o:Options|run(app.clone(),TranscodeRequest{options:None,input_path:source.to_string_lossy().into(),output_path:Some(dir.to_string_lossy().into()),quality_mode:None,delete_source:Some(false)},o);
    let mut o=Options::default();o.resolution="custom".into();o.lock_ratio=false;o.width=320;o.height=240;o.audio_kbps=128;let a=encode(o.clone())?;
    let checked=probe(app,Path::new(&a.output_path),false)?;if checked.width!=320||checked.height!=240||checked.audio_codec.as_deref()!=Some("aac"){return Err("H264 缩放或 AAC 验证失败".into())}
    o.codec="h265".into();o.container="mkv".into();o.audio="copy".into();o.resolution="source".into();let b=encode(o.clone())?;
    let audio_hash=|p:&Path|->Result<String,String>{Ok(process::run(&ffmpeg,vec!["-v".into(),"error".into(),"-i".into(),p.as_os_str().into(),"-map".into(),"0:a:0".into(),"-c:a".into(),"copy".into(),"-f".into(),"hash".into(),"-hash".into(),"sha256".into(),"-".into()],Duration::from_secs(30),|_|{})?.stdout)};
    if audio_hash(&source)?!=audio_hash(Path::new(&b.output_path))?{return Err("复制音频发生数据变化".into())}
    o.codec="h264".into();o.container="mov".into();o.audio="none".into();o.suffix_mode="custom".into();o.suffix="剪辑版".into();let c=encode(o)?;if probe(app,Path::new(&c.output_path),false)?.audio_index.is_some()||!c.output_path.ends_with("_剪辑版.mov"){return Err("MOV 无音频或自定义后缀失败".into())}
    for codec in ["h264","h265"]{let mut o=Options::default();o.codec=codec.into();o.mode="size".into();o.target_mb=0.5;o.audio_kbps=128;let p=plan(probe(app,&source,false)?,&o)?;let out=encode(o)?;let size=fs::metadata(out.output_path).map_err(|e|e.to_string())?.len() as f64/1e6;if !(0.35..=0.54).contains(&size){return Err(format!("{codec} 两遍编码大小偏差过大：{size} MB，视频码率 {:?}",p.video_kbps))}}
    let hdr=dir.join("hdr-source.mp4");process::run(&ffmpeg,vec!["-y".into(),"-i".into(),source.as_os_str().into(),"-t".into(),"2".into(),"-an".into(),"-c:v".into(),"libx265".into(),"-preset".into(),"ultrafast".into(),"-pix_fmt".into(),"yuv420p10le".into(),"-color_primaries".into(),"bt2020".into(),"-color_trc".into(),"smpte2084".into(),"-colorspace".into(),"bt2020nc".into(),"-x265-params".into(),"colorprim=bt2020:transfer=smpte2084:colormatrix=bt2020nc:pools=2".into(),hdr.as_os_str().into()],Duration::from_secs(60),|_|{})?;
    for color in ["preserve","sdr"]{let mut o=Options::default();o.codec="h265".into();o.color=color.into();o.suffix_mode="custom".into();o.suffix=color.into();let out=run(app.clone(),TranscodeRequest{options:None,input_path:hdr.to_string_lossy().into(),output_path:None,quality_mode:None,delete_source:Some(false)},o)?;let m=probe(app,Path::new(&out.output_path),false)?;if (color=="preserve"&&(!m.hdr||m.depth!=10))||(color=="sdr"&&(m.hdr||m.depth!=8)){return Err("HDR / SDR 输出验证失败".into())}}
    if !source.exists()||fs::read_dir(dir).map_err(|e|e.to_string())?.flatten().any(|e|e.file_name().to_string_lossy().starts_with(".transcode-work-")){return Err("源文件保留或临时文件清理失败".into())}
    Ok(vec!["H264 / H265 编码、MP4 / MKV / MOV 封装、分辨率补边、自定义后缀".into(),"AAC 压制、音频复制逐字节哈希一致、无音频输出".into(),"H264 / H265 两遍目标大小实际输出误差校验".into(),"HDR 10 位保留、HDR 转 SDR 8 位及临时文件清理".into()])
}
