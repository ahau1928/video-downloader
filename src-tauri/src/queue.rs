use crate::*;
use std::{collections::HashMap, sync::atomic::{AtomicBool, Ordering}, time::Duration};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {pub transcode:crate::transcode::Options,pub output_dir:String,pub proxy:String,pub use_proxy:bool,pub cookies_browser:String,pub quality_height:Option<u64>,pub output_mode:String,pub quality_mode:String,pub keep_source:bool,pub concurrency:usize,pub auto_update:bool,pub batch_output_dir:String,pub filename_suffix:bool,pub filename_codecs:bool}
impl Default for Settings {fn default()->Self{Self{transcode:crate::transcode::Options::default(),output_dir:get_default_download_dir(),proxy:"http://127.0.0.1:7897".into(),use_proxy:true,cookies_browser:String::new(),quality_height:None,output_mode:"original".into(),quality_mode:"balanced".into(),keep_source:true,concurrency:2,auto_update:true,batch_output_dir:String::new(),filename_suffix:false,filename_codecs:false}}}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {pub id:String,pub kind:String,pub source:String,pub title:String,pub status:String,pub stage:String,pub percent:f64,pub speed:String,pub eta:String,pub error:String,pub created:u64,pub settings:Settings,pub metadata:Option<ProbeResult>,pub video_format_id:Option<String>,pub audio_format_id:Option<String>,pub output:Option<DownloadResult>,pub downloaded:Option<DownloadResult>,#[serde(default)] pub name_conflict:Option<engine::NameConflict>,#[serde(default)] pub conflict_action:String}
#[derive(Clone, Serialize, Deserialize)]
pub struct Snapshot {pub tasks:Vec<Task>,pub settings:Settings,pub paused:bool,#[serde(default)] pub persistence_error:String}
impl Default for Snapshot {fn default()->Self{Self{tasks:vec![],settings:Settings::default(),paused:false,persistence_error:String::new()}}}
pub struct Queue {pub data:Mutex<Snapshot>,pub running:Mutex<HashMap<String,Arc<AtomicBool>>>,pub transcode:Mutex<()>,pub tools:std::sync::RwLock<()>,pub store:PathBuf,pub closing:AtomicBool}
thread_local! {static TASK:std::cell::RefCell<Option<String>>=const{std::cell::RefCell::new(None)};}
pub fn current()->Option<String>{TASK.with(|v|v.borrow().clone())}
pub fn unique_id()->String{format!("{}-{}",std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_nanos(),std::process::id())}
fn save(q:&Queue,d:&mut Snapshot){if let Err(e)=crate::atomic_json(&q.store,d){d.persistence_error=format!("任务保存失败：{e}")}else{d.persistence_error.clear()}}
fn publish(app:&AppHandle,q:&Queue,d:&mut Snapshot,persist:bool){if persist{save(q,d)}let _=app.emit("queue-changed",d.clone());}
pub fn edit(app:&AppHandle,id:&str,f:impl FnOnce(&mut Task),persist:bool){let q=app.state::<Queue>();let mut d=q.data.lock().unwrap();if let Some(t)=d.tasks.iter_mut().find(|t|t.id==id){f(t)}publish(app,&q,&mut d,persist)}
pub fn progress(app:&AppHandle,percent:f64,stage:&str,speed:&str,eta:&str){if let Some(id)=current(){edit(app,&id,|t|{if t.status=="running" && !process::cancelled(){t.percent=percent;t.stage=stage.into();t.speed=speed.into();t.eta=eta.into()}},false)}}
pub fn restore(data:&mut Snapshot){for task in &mut data.tasks {if matches!(task.status.as_str(),"running"|"queued"|"pausing"|"paused"){task.status="interrupted".into();task.stage="待恢复".into();}}data.paused=false;}
pub fn init(app:&AppHandle){let store=app_data_dir(app).join("queue.json");let mut data:Snapshot=fs::read(&store).ok().and_then(|v|serde_json::from_slice(&v).ok()).unwrap_or_default();restore(&mut data);app.manage(Queue{data:Mutex::new(data),running:Mutex::new(HashMap::new()),transcode:Mutex::new(()),tools:std::sync::RwLock::new(()),store,closing:AtomicBool::new(false)});let app=app.clone();thread::spawn(move||loop{thread::sleep(Duration::from_millis(350));dispatch(&app)});}
fn dispatch(app:&AppHandle){let q=app.state::<Queue>();let Ok(_tools)=q.tools.try_read() else{return};let mut d=q.data.lock().unwrap();if d.paused||q.closing.load(Ordering::Relaxed){return}let mut running=q.running.lock().unwrap();let downloads=d.tasks.iter().filter(|t|running.contains_key(&t.id)&&t.kind=="download"&&t.downloaded.is_none()).count();let transcodes=d.tasks.iter().filter(|t|running.contains_key(&t.id)&&t.kind=="transcode").count();let limit=d.settings.concurrency.clamp(1,4);let Some(index)=d.tasks.iter().position(|t|t.status=="queued"&&!running.contains_key(&t.id)&&if t.kind=="download"{downloads<limit}else{transcodes<1})else{return};let t=&mut d.tasks[index];t.status="running".into();t.stage="正在准备".into();let task=t.clone();let flag=Arc::new(AtomicBool::new(false));running.insert(task.id.clone(),flag.clone());publish(app,&q,&mut d,true);drop(d);drop(running);let app=app.clone();thread::spawn(move||{TASK.with(|v|*v.borrow_mut()=Some(task.id.clone()));process::set_cancel(Some(flag.clone()));let q=app.state::<Queue>();let _guard=q.tools.read().unwrap();let result=execute(&app,task.clone());// Complete under the same data lock used by pause/resume/cancel. A fast
// resume cannot dispatch this task until its old worker has stopped.
let mut data=q.data.lock().unwrap();
let queue_paused=data.paused;
if let Some(t)=data.tasks.iter_mut().find(|t|t.id==task.id){
    match result {
        Ok(output)=>{t.name_conflict=None;t.output=Some(output);t.status="done".into();t.stage="已完成".into();t.percent=100.;t.error.clear()},
        Err(e)=>{
            if flag.load(Ordering::Relaxed) {
                if q.closing.load(Ordering::Relaxed){t.status="interrupted".into();t.stage="待恢复".into()}
                else if t.status=="pausing" {
                    t.status=if queue_paused{"paused"}else{"queued"}.into();
                    t.stage=if t.kind=="transcode"||t.downloaded.is_some(){"已暂停，继续后重新转码"}else{"已暂停"}.into();
                } else {t.status="cancelled".into();t.stage="已取消".into()}
                t.error.clear();
            } else if let Some(conflict)=engine::parse_conflict(&e) {
                t.status="awaiting_name".into();t.stage="等待选择文件名".into();t.name_conflict=Some(conflict);t.error.clear();
            } else {t.status="error".into();t.stage=if t.downloaded.is_some(){"下载成功，转码失败"}else{"失败"}.into();t.error=e}
        }
    }
    t.speed.clear();t.eta.clear();
}
q.running.lock().unwrap().remove(&task.id);
publish(&app,&q,&mut data,true);
drop(data);
process::set_cancel(None);TASK.with(|v|*v.borrow_mut()=None);});}
fn execute(app:&AppHandle,mut task:Task)->Result<DownloadResult,String>{
    if task.kind=="transcode" {return transcode(app,&task,&task.source)}
    if let Some(downloaded)=task.downloaded.clone(){return transcode(app,&task,&downloaded.output_path)}
    let proxy=task.settings.use_proxy.then_some(task.settings.proxy.as_str());
    let metadata=match task.metadata.take(){Some(m)=>m,None=>{progress(app,0.,"正在解析","","");let m=probe_metadata(app,&task.source,proxy,Some(&task.settings.cookies_browser),None)?;edit(app,&task.id,|t|{t.title=m.title.clone();t.metadata=Some(m.clone())},true);m}};
    let audio_only=task.settings.output_mode=="audio";

    let resolved=select_download_formats(&metadata,task.settings.quality_height,task.video_format_id.as_deref(),task.audio_format_id.as_deref(),&SaveStrategy::Auto);
    edit(app,&task.id,|t|{t.video_format_id=resolved.video.map(|f|f.format_id.clone());t.audio_format_id=resolved.audio.map(|f|f.format_id.clone());},true);
    let request=DownloadRequest{filename_codecs:task.settings.filename_codecs,filename_suffix:task.settings.filename_suffix,conflict_action:task.conflict_action.clone(),audio_only,url:task.source.clone(),output_dir:task.settings.output_dir.clone(),proxy:proxy.map(str::to_string),cookies_browser:Some(task.settings.cookies_browser.clone()),cookies_file:None,quality_height:task.settings.quality_height,video_format_id:if audio_only{None}else{task.video_format_id.clone()},audio_format_id:task.audio_format_id.clone(),save_strategy:SaveStrategy::Auto,metadata:Some(metadata)};
    let output=download_video_blocking(app.clone(),request)?;
    let compatible=task.settings.output_mode=="compatible"&&(output.selected_vcodec.as_deref().map(|c|!c.starts_with("avc1")&&!c.starts_with("h264")).unwrap_or(true)||output.container!="mp4");
    if compatible {edit(app,&task.id,|t|t.downloaded=Some(output.clone()),true);transcode(app,&task,&output.output_path)}else{Ok(output)}
}
fn transcode(app:&AppHandle,task:&Task,input:&str)->Result<DownloadResult,String>{progress(app,0.,"等待转码位置","","");let q=app.state::<Queue>();let guard=loop{if process::cancelled(){return Err("任务已取消".into())}if let Ok(g)=q.transcode.try_lock(){break g}thread::sleep(Duration::from_millis(150));};let output_path=if task.kind=="transcode"&&!task.settings.batch_output_dir.is_empty(){Some(task.settings.batch_output_dir.clone())}else{None};let result=transcode_to_h264_blocking(app.clone(),TranscodeRequest{options:if task.kind=="transcode"{Some(task.settings.transcode.clone())}else{None},input_path:input.into(),output_path,quality_mode:Some(task.settings.quality_mode.clone()),delete_source:Some(!task.settings.keep_source)});drop(guard);result}

#[tauri::command]
pub fn clear_tasks(app: AppHandle, kind: String) -> Result<usize,String> {
    let q=app.state::<Queue>();
    let mut data=q.data.lock().unwrap();
    let running=q.running.lock().unwrap();
    let before=data.tasks.len();
    // Clearing history never cancels or removes unfinished work.
    data.tasks.retain(|t| t.kind!=kind || running.contains_key(&t.id) || !matches!(t.status.as_str(),"done"|"error"|"cancelled"));
    let removed=before-data.tasks.len();
    publish(&app,&q,&mut data,true);
    Ok(removed)
}
#[tauri::command] pub fn queue_snapshot(app:AppHandle)->Snapshot{app.state::<Queue>().data.lock().unwrap().clone()}
#[tauri::command] pub fn save_settings(app:AppHandle,mut settings:Settings)->Result<(),String>{settings.concurrency=settings.concurrency.clamp(1,4);let q=app.state::<Queue>();let mut d=q.data.lock().unwrap();d.settings=settings;publish(&app,&q,&mut d,true);if d.persistence_error.is_empty(){Ok(())}else{Err(d.persistence_error.clone())}}
#[derive(Deserialize)] pub struct AddRequest{pub sources:Vec<String>,pub kind:String,pub settings:Settings,pub metadata:Option<ProbeResult>,pub video_format_id:Option<String>,pub audio_format_id:Option<String>,pub start:bool}
#[tauri::command] pub fn add_tasks(app:AppHandle,request:AddRequest)->Result<usize,String>{if request.sources.len()>1000{return Err("每批最多 1000 个任务".into())}if !matches!(request.kind.as_str(),"download"|"transcode"){return Err("无效任务类型".into())}if request.kind=="download"&&!Path::new(&request.settings.output_dir).is_dir(){return Err("保存目录不存在".into())}for s in &request.sources{if request.kind=="download"{validate_url(s)?;}else if !Path::new(s).is_file(){return Err(format!("文件不存在：{s}"))}}let q=app.state::<Queue>();let mut d=q.data.lock().unwrap();let mut added=0;for source in request.sources{let source=source.trim().to_string();if d.tasks.iter().any(|t|t.source==source&&matches!(t.status.as_str(),"queued"|"running"|"pausing"|"paused"|"waiting"|"awaiting_name")&&serde_json::to_string(&t.settings).ok()==serde_json::to_string(&request.settings).ok()&&t.video_format_id==request.video_format_id&&t.audio_format_id==request.audio_format_id){continue}let metadata=request.metadata.clone().filter(|m|m.webpage_url==source);let title=metadata.as_ref().map(|m|m.title.clone()).unwrap_or_else(||if request.kind=="transcode"{Path::new(&source).file_name().unwrap_or_default().to_string_lossy().into()}else{source.clone()});d.tasks.push(Task{id:unique_id(),kind:request.kind.clone(),source,title,status:if request.start{"queued"}else{"waiting"}.into(),stage:"等待下载".into(),percent:0.,speed:String::new(),eta:String::new(),error:String::new(),created:std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs(),settings:request.settings.clone(),metadata,video_format_id:request.video_format_id.clone(),audio_format_id:request.audio_format_id.clone(),output:None,downloaded:None,name_conflict:None,conflict_action:String::new()});added+=1}publish(&app,&q,&mut d,true);Ok(added)}
#[tauri::command] pub fn task_action(app:AppHandle,id:String,action:String)->Result<(),String>{let q=app.state::<Queue>();let mut d=q.data.lock().unwrap();if action=="pause"{
    d.paused=true;
    let running=q.running.lock().unwrap();
    for t in &mut d.tasks {
        if let Some(flag)=running.get(&t.id){
            // Do not turn an already requested cancellation into a pause.
            if !flag.load(Ordering::Relaxed){t.status="pausing".into();t.stage="正在暂停".into();t.speed.clear();t.eta.clear();flag.store(true,Ordering::Relaxed);}
        }
    }
}else if action=="resume"{d.paused=false;for t in &mut d.tasks{if matches!(t.status.as_str(),"waiting"|"interrupted"|"paused"){t.status="queued".into()}}}else{let i=d.tasks.iter().position(|t|t.id==id).ok_or("找不到任务")?;match action.as_str(){"name_suffix"|"name_number"=>{if d.tasks[i].status!="awaiting_name"||q.running.lock().unwrap().contains_key(&id){return Err("任务仍在准备，请稍后选择".into())}d.tasks[i].conflict_action=if action=="name_suffix"{"suffix"}else{"number"}.into();d.tasks[i].name_conflict=None;d.tasks[i].status="queued".into();},"cancel"=>{if let Some(flag)=q.running.lock().unwrap().get(&id){flag.store(true,Ordering::Relaxed);d.tasks[i].status="running".into();d.tasks[i].stage="正在取消".into()}else{d.tasks[i].status="cancelled".into();d.tasks[i].stage="已取消".into()}},"retry"=>{if q.running.lock().unwrap().contains_key(&id){return Err("任务仍在运行".into())}d.tasks[i].status="queued".into();d.tasks[i].error.clear();d.tasks[i].percent=0.;if d.tasks[i].downloaded.is_none(){d.tasks[i].metadata=None}},"remove"=>{if q.running.lock().unwrap().contains_key(&id){return Err("请先取消任务".into())}d.tasks.remove(i);},_=>return Err("无效操作".into())}}publish(&app,&q,&mut d,true);Ok(())}
