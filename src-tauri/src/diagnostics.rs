//! Debug-only end-to-end checks. Never included in release installers.
use crate::*;
use std::{io::{Read,Write},time::{Duration,Instant}};
pub fn start(app:&AppHandle)->bool{if std::env::var_os("VIDEOTOOL_SELF_TEST").is_none(){return false}let app=app.clone();for window in app.webview_windows().values(){let _=window.hide();}thread::spawn(move||{let result=run(&app);let report=match &result{Ok(checks)=>serde_json::json!({"passed":true,"checks":checks}),Err(e)=>serde_json::json!({"passed":false,"error":e})};let _=atomic_json(&app_data_dir(&app).join("self-test-result.json"),&report);app.exit(if result.is_ok(){0}else{1});});true}
fn ensure(value:bool,message:&str)->Result<(),String>{if value{Ok(())}else{Err(message.into())}}
fn run(app:&AppHandle)->Result<Vec<String>,String>{
    let dir=app_data_dir(app);fs::create_dir_all(&dir).map_err(|e|e.to_string())?;let output_dir=dir.join("outputs");fs::create_dir_all(&output_dir).map_err(|e|e.to_string())?;
    {let q=app.state::<queue::Queue>();let mut state=q.data.lock().unwrap();state.settings.auto_update=false;state.settings.use_proxy=false;state.settings.output_dir=output_dir.to_string_lossy().into();}
    let mut checks=Vec::new();let fixture=dir.join("fixture.mp4");
    process::run(&resolve_tool(app,"ffmpeg"),vec!["-y".into(),"-f".into(),"lavfi".into(),"-i".into(),"testsrc=size=160x90:rate=15".into(),"-f".into(),"lavfi".into(),"-i".into(),"sine=frequency=440:sample_rate=44100".into(),"-t".into(),"2".into(),"-c:v".into(),"libx264".into(),"-pix_fmt".into(),"yuv420p".into(),"-c:a".into(),"aac".into(),fixture.as_os_str().into()],Duration::from_secs(30),|_|{})?;
    let original=fs::read(&fixture).map_err(|e|e.to_string())?;
    let first=engine::transcode(app.clone(),TranscodeRequest{options:None,input_path:fixture.to_string_lossy().into(),output_path:None,quality_mode:Some("balanced".into()),delete_source:Some(false)})?;
    let second=engine::transcode(app.clone(),TranscodeRequest{options:None,input_path:fixture.to_string_lossy().into(),output_path:None,quality_mode:Some("size".into()),delete_source:Some(false)})?;
    ensure(first.output_path!=second.output_path,"同名转码结果未避让")?;ensure(fs::read(&fixture).map_err(|e|e.to_string())?==original,"源文件被修改")?;checks.push("MP4 源目录转码、同名自动避让、源文件保持原样".into());
    let fail=engine::transcode(app.clone(),TranscodeRequest{options:None,input_path:fixture.to_string_lossy().into(),output_path:Some(fixture.to_string_lossy().into()),quality_mode:None,delete_source:Some(true)});
    ensure(fail.is_err()&&fixture.exists(),"输入输出相同未拒绝")?;checks.push("拒绝输入输出相同，失败不删除源文件".into());
    let listener=std::net::TcpListener::bind("127.0.0.1:0").map_err(|e|e.to_string())?;let port=listener.local_addr().unwrap().port();thread::spawn(move||{for stream in listener.incoming(){let Ok(mut stream)=stream else{break};let _=stream.set_read_timeout(Some(Duration::from_secs(5)));let mut request=[0;4096];let n=stream.read(&mut request).unwrap_or(0);let head=String::from_utf8_lossy(&request[..n]);let response=format!("HTTP/1.1 200 OK\r\nContent-Type: video/mp4\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",original.len());let _=stream.write_all(response.as_bytes());if !head.starts_with("HEAD "){let _=stream.write_all(&original);}}});
    let url=format!("http://127.0.0.1:{port}/sample.mp4");let metadata=probe_metadata(app,&url,None,None,None)?;ensure(!metadata.formats.is_empty(),"直链视频格式为空")?;
    let request=|audio_only|DownloadRequest{filename_codecs:false,filename_suffix:false,conflict_action:String::new(),audio_only,url:url.clone(),output_dir:output_dir.to_string_lossy().into(),proxy:None,cookies_browser:None,cookies_file:None,quality_height:None,video_format_id:None,audio_format_id:None,save_strategy:SaveStrategy::Auto,metadata:Some(metadata.clone())};
    let legacy=output_dir.join(".video-downloader");fs::create_dir_all(&legacy).map_err(|e|e.to_string())?;fs::write(legacy.join("preserve.txt"),b"resume data").map_err(|e|e.to_string())?;
    let downloaded=engine::download(app.clone(),request(false))?;
    ensure(!Path::new(&downloaded.output_path).file_name().unwrap().to_string_lossy().contains('['),"默认文件名仍含身份后缀")?;
    ensure(!legacy.exists(),"旧辅助目录未迁移")?;
    let store=fs::read_dir(dir.join("downloads")).map_err(|e|e.to_string())?.next().unwrap().map_err(|e|e.to_string())?.path();
    ensure(store.join("legacy/preserve.txt").exists(),"迁移丢失旧数据")?;
    ensure(!fs::read_dir(&store).unwrap().flatten().any(|e|e.file_name().to_string_lossy().ends_with("-work")),"成功后未清理工作目录")?;
    checks.push("干净文件名、旧辅助目录无损迁移、完成后临时片段清理".into());ensure(Path::new(&downloaded.output_path).is_file(),"未保存下载文件")?;let repeated=engine::download(app.clone(),request(false))?;ensure(repeated.skipped&&repeated.output_path==downloaded.output_path,"重复身份未匹配")?;
    let duplicate=engine::download(app.clone(),request(true)).unwrap_err();
    ensure(engine::parse_conflict(&duplicate).is_some(),"不同版本重名未提示")?;
    let mut audio_request=request(true);audio_request.conflict_action="suffix".into();
    let audio=engine::download(app.clone(),audio_request)?;ensure(audio.container=="mp3"&&Path::new(&audio.output_path).is_file(),"合一视频提取音频失败")?;checks.push("真实 yt-dlp 直链解析、下载、同格式去重及 MP3 提取".into());
    let mut variant=metadata.clone();variant.id="queue-variant".into();
    let before_bytes=fs::read(&downloaded.output_path).map_err(|e|e.to_string())?;
    let variant_settings=app.state::<queue::Queue>().data.lock().unwrap().settings.clone();
    queue::add_tasks(app.clone(),queue::AddRequest{sources:vec![url.clone()],kind:"download".into(),settings:variant_settings,metadata:Some(variant),video_format_id:None,audio_format_id:None,start:true})?;
    let variant_id=queue::queue_snapshot(app.clone()).tasks.last().unwrap().id.clone();
    let before=Instant::now();
    loop {let state=queue::queue_snapshot(app.clone());let task=state.tasks.iter().find(|t|t.id==variant_id).unwrap();if task.status=="awaiting_name"{break}if task.status=="error"||before.elapsed()>Duration::from_secs(20){return Err(format!("重名等待状态失败：{}",task.error))}thread::sleep(Duration::from_millis(100));}
    queue::task_action(app.clone(),variant_id.clone(),"name_suffix".into())?;
    let before=Instant::now();
    loop {let state=queue::queue_snapshot(app.clone());let task=state.tasks.iter().find(|t|t.id==variant_id).unwrap();if task.status=="done"{ensure(task.output.as_ref().unwrap().output_path!=downloaded.output_path,"重名覆盖了输出")?;break}if task.status=="error"||before.elapsed()>Duration::from_secs(30){return Err(format!("重名继续失败：{}",task.error))}thread::sleep(Duration::from_millis(100));}
    ensure(fs::read(&downloaded.output_path).map_err(|e|e.to_string())?==before_bytes,"重名流程修改了原文件")?;
    checks.push("真实队列重名暂停、选择后缀后继续完成且不覆盖".into());
    test_pause_resume(app,&fixture,&output_dir)?;
    checks.push("真实 yt-dlp 限速下载暂停：片段停止增长、队列停止调度、HTTP Range 续传、快速暂停继续、暂停后取消".into());
    let mut options=app.state::<queue::Queue>().data.lock().unwrap().settings.clone();options.quality_mode="balanced".into();
    let tasks=queue::AddRequest{sources:vec![fixture.to_string_lossy().into()],kind:"transcode".into(),settings:options.clone(),metadata:None,video_format_id:None,audio_format_id:None,start:false};queue::add_tasks(app.clone(),tasks)?;
    let id=queue::queue_snapshot(app.clone()).tasks.last().unwrap().id.clone();thread::sleep(Duration::from_millis(700));ensure(queue::queue_snapshot(app.clone()).tasks.last().unwrap().status=="waiting","仅入队时启动了任务")?;
    let mut changed=options;changed.quality_mode="quality".into();queue::save_settings(app.clone(),changed)?;ensure(queue::queue_snapshot(app.clone()).tasks.last().unwrap().settings.quality_mode=="balanced","设置快照被修改")?;
    queue::task_action(app.clone(),String::new(),"resume".into())?;let start=Instant::now();loop{let state=queue::queue_snapshot(app.clone());let task=state.tasks.iter().find(|t|t.id==id).unwrap();if task.status=="done"{break}if task.status=="error"{return Err(task.error.clone())}if start.elapsed()>Duration::from_secs(40){return Err("队列执行超时".into())}thread::sleep(Duration::from_millis(100));}
    let failed_id={let q=app.state::<queue::Queue>();let mut state=q.data.lock().unwrap();let mut sample=state.tasks.last().unwrap().clone();sample.id=queue::unique_id();sample.status="waiting".into();let id=sample.id.clone();state.tasks.push(sample);id};
    let cleared=queue::clear_tasks(app.clone(),"transcode".into())?;
    ensure(cleared>0 && Path::new(&first.output_path).exists(),"清除记录影响输出文件")?;
    ensure(queue::queue_snapshot(app.clone()).tasks.iter().any(|t|t.id==failed_id&&t.status=="waiting"),"清除记录移除了等待任务")?;
    checks.push("重名提示与后缀选择、清除已结束记录保留等待任务与文件".into());
    let persisted:queue::Snapshot=serde_json::from_slice(&fs::read(dir.join("queue.json")).map_err(|e|e.to_string())?).map_err(|e|e.to_string())?;ensure(persisted.tasks.iter().any(|t|t.id==failed_id&&t.status=="waiting"),"队列状态未持久化")?;checks.push("后端队列启动、等待任务、设置快照、完成历史持久化".into());
    let flag=Arc::new(std::sync::atomic::AtomicBool::new(false));let cancel=flag.clone();process::set_cancel(Some(flag));thread::spawn(move||{thread::sleep(Duration::from_millis(300));cancel.store(true,std::sync::atomic::Ordering::Relaxed)});let before=Instant::now();let cancelled=process::run(Path::new("powershell.exe"),vec!["-NoProfile".into(),"-Command".into(),"Start-Sleep -Seconds 30".into()],Duration::from_secs(35),|_|{});process::set_cancel(None);ensure(cancelled.is_err()&&before.elapsed()<Duration::from_secs(8),"子进程取消未及时生效")?;checks.push("取消子进程及其进程树".into());
    if std::env::var_os("VIDEOTOOL_NETWORK_TEST").is_some(){let m=probe_metadata(app,"https://www.youtube.com/watch?v=v1W9X60jc8Q",Some("http://127.0.0.1:7897"),None,None)?;let downloaded=engine::download(app.clone(),DownloadRequest{filename_codecs:false,filename_suffix:false,conflict_action:String::new(),audio_only:false,url:m.webpage_url.clone(),output_dir:output_dir.to_string_lossy().into(),proxy:Some("http://127.0.0.1:7897".into()),cookies_browser:None,cookies_file:None,quality_height:Some(360),video_format_id:None,audio_format_id:None,save_strategy:SaveStrategy::Auto,metadata:Some(m)})?;ensure(probe_media_stats(app,Path::new(&downloaded.output_path))?.duration_seconds>150.,"YouTube 输出时长不足")?;checks.push("用户提供 YouTube 视频：实际解析、完整下载、音视频输出验证".into());}
    browser_auth::self_test(app)?; checks.push("浏览器授权加密保存、解密临时文件清理、发送链接入等待队列、授权优先级及清除".into());
    checks.extend(transcode::self_test(app,&dir)?);
    Ok(checks)
}

fn wait_task(app:&AppHandle,id:&str,predicate:impl Fn(&queue::Task)->bool)->Result<queue::Task,String>{
    let start=Instant::now();
    loop {
        let state=queue::queue_snapshot(app.clone());let t=state.tasks.iter().find(|t|t.id==id).ok_or("测试任务丢失")?;
        if predicate(t){return Ok(t.clone())}
        if t.status=="error"||start.elapsed()>Duration::from_secs(45){return Err(format!("等待任务超时或失败：{} {}",t.status,t.error))}
        thread::sleep(Duration::from_millis(50));
    }
}
fn partial_bytes(path:&Path)->u64 {
    fs::read_dir(path).into_iter().flatten().flatten().map(|e|{
        let p=e.path();if p.is_dir(){partial_bytes(&p)}else if p.extension().is_some_and(|s|s=="part"){e.metadata().map(|m|m.len()).unwrap_or(0)}else{0}
    }).sum()
}
fn test_pause_resume(app:&AppHandle,fixture:&Path,output_dir:&Path)->Result<(),String>{
    use std::sync::atomic::{AtomicUsize,Ordering};
    let mut bytes=fs::read(fixture).map_err(|e|e.to_string())?;bytes.resize(6*1024*1024,0);
    let bytes=Arc::new(bytes);let expected=bytes.clone();
    let ranges=Arc::new(AtomicUsize::new(0));let range_count=ranges.clone();
    let listener=std::net::TcpListener::bind("127.0.0.1:0").map_err(|e|e.to_string())?;let port=listener.local_addr().unwrap().port();
    thread::spawn(move||{for stream in listener.incoming(){let Ok(mut stream)=stream else{break};let bytes=bytes.clone();let ranges=ranges.clone();thread::spawn(move||{
        let _=stream.set_read_timeout(Some(Duration::from_secs(5)));let _=stream.set_write_timeout(Some(Duration::from_secs(2)));
        let mut request=[0;4096];let n=stream.read(&mut request).unwrap_or(0);let head=String::from_utf8_lossy(&request[..n]);
        let offset=head.lines().find_map(|l|l.to_ascii_lowercase().strip_prefix("range: bytes=").and_then(|v|v.split('-').next()).and_then(|v|v.parse::<usize>().ok())).unwrap_or(0).min(bytes.len());
        let range=if offset>0{ranges.fetch_add(1,Ordering::Relaxed);format!("Content-Range: bytes {}-{}/{}\r\n",offset,bytes.len()-1,bytes.len())}else{String::new()};
        let response=format!("HTTP/1.1 {}\r\nContent-Type: video/mp4\r\nAccept-Ranges: bytes\r\n{}Content-Length: {}\r\nConnection: close\r\n\r\n",if offset>0{"206 Partial Content"}else{"200 OK"},range,bytes.len()-offset);
        if stream.write_all(response.as_bytes()).is_err()||head.starts_with("HEAD "){return}
        for chunk in bytes[offset..].chunks(32768){if stream.write_all(chunk).is_err(){break}thread::sleep(Duration::from_millis(35));}
    });}});
    let url=format!("http://127.0.0.1:{port}/pause.mp4");let mut metadata=probe_metadata(app,&url,None,None,None)?;
    // Generic URLs omit codecs; supply the known fixture codecs so this test
    // checks exact resumed bytes without an unrelated MP4-to-MKV remux.
    metadata.formats[0].vcodec=Some("avc1".into());metadata.formats[0].acodec=Some("mp4a".into());
    let mut settings=queue::queue_snapshot(app.clone()).settings;settings.output_dir=output_dir.to_string_lossy().into();settings.concurrency=1;
    queue::save_settings(app.clone(),settings.clone())?;
    let add=|source:String,metadata:Option<ProbeResult>|queue::add_tasks(app.clone(),queue::AddRequest{sources:vec![source],kind:"download".into(),settings:settings.clone(),metadata,video_format_id:None,audio_format_id:None,start:true});
    add(url.clone(),Some(metadata.clone()))?;
    let id=queue::queue_snapshot(app.clone()).tasks.last().unwrap().id.clone();let downloads=app_data_dir(app).join("downloads");
    wait_task(app,&id,|t|t.status=="running"&&partial_bytes(&downloads)>131072)?;
    queue::task_action(app.clone(),String::new(),"pause".into())?;
    let paused=wait_task(app,&id,|t|t.status=="paused")?;
    ensure(paused.error.is_empty()&&paused.speed.is_empty(),"暂停状态仍显示错误或下载速度")?;
    let size=partial_bytes(&downloads);ensure(size>0,"暂停没有保留下载片段")?;
    add(format!("http://127.0.0.1:{port}/pending.mp4"),None)?;
    let pending=queue::queue_snapshot(app.clone()).tasks.last().unwrap().id.clone();
    thread::sleep(Duration::from_secs(2));
    ensure(size==partial_bytes(&downloads),"暂停后仍在写入下载片段")?;
    ensure(queue::queue_snapshot(app.clone()).tasks.iter().find(|t|t.id==pending).unwrap().status=="queued","暂停后仍启动新任务")?;
    queue::task_action(app.clone(),pending,"cancel".into())?;
    queue::task_action(app.clone(),String::new(),"resume".into())?;
    wait_task(app,&id,|t|t.status=="running"&&range_count.load(Ordering::Relaxed)>0&&partial_bytes(&downloads)>size)?;
    queue::task_action(app.clone(),String::new(),"pause".into())?;
    queue::task_action(app.clone(),String::new(),"resume".into())?;
    let done=wait_task(app,&id,|t|t.status=="done")?;
    ensure(fs::read(&done.output.unwrap().output_path).map_err(|e|e.to_string())?==*expected,"续传后的文件不完整")?;
    let url=format!("http://127.0.0.1:{port}/cancel-paused.mp4");
    add(url,None)?;let id=queue::queue_snapshot(app.clone()).tasks.last().unwrap().id.clone();
    wait_task(app,&id,|t|t.status=="running"&&partial_bytes(&downloads)>131072)?;
    queue::task_action(app.clone(),String::new(),"pause".into())?;
    wait_task(app,&id,|t|t.status=="paused")?;
    queue::task_action(app.clone(),id.clone(),"cancel".into())?;
    ensure(queue::queue_snapshot(app.clone()).tasks.iter().find(|t|t.id==id).unwrap().status=="cancelled","暂停任务无法取消")?;
    queue::task_action(app.clone(),String::new(),"resume".into())?;
    Ok(())
}

