use crate::*;
use std::io::{Read,Write};
const HOST:&str="com.jasonpan.video_downloader";
pub const EXTENSION_ID:&str=include_str!("../../extension/id.txt");
const LIMIT:usize=1_000_000;

#[derive(Clone,Serialize,Deserialize)]
pub struct Cookie {domain:String,path:String,name:String,value:String,#[serde(default)] secure:bool,#[serde(default,rename="httpOnly")] http_only:bool,#[serde(default,rename="hostOnly")] host_only:bool,#[serde(default,rename="expirationDate")] expiration:Option<f64>}
#[derive(Serialize,Deserialize)]
struct Grant {site:String,source:String,updated:u64,cookies:Vec<Cookie>}
#[derive(Deserialize)]
struct Message {action:String,#[serde(default)]url:String,#[serde(default)]cookies:Vec<Cookie>}
fn root()->PathBuf {
    #[cfg(debug_assertions)] if let Some(p)=std::env::var_os("VIDEOTOOL_TEST_DIR"){return PathBuf::from(p).join("browser-auth")}
    PathBuf::from(std::env::var_os("APPDATA").unwrap_or_default()).join("com.jasonpan.video-downloader/browser-auth")
}
fn now()->u64{std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs()}
pub fn site(url:&str)->Option<&'static str>{let u=tauri::Url::parse(url).ok()?;if u.scheme()!="https"||!u.username().is_empty()||u.password().is_some()||u.port().is_some(){return None}let h=u.host_str()?;if h=="youtu.be"||h=="youtube.com"||h.ends_with(".youtube.com"){Some("youtube")}else if h=="vimeo.com"||h.ends_with(".vimeo.com"){Some("vimeo")}else{None}}
fn valid_site(s:&str)->Result<(),String>{if matches!(s,"youtube"|"vimeo"){Ok(())}else{Err("仅支持 YouTube 和 Vimeo 授权".into())}}
fn allowed_domain(site:&str,domain:&str)->bool{let d=domain.trim_start_matches('.').to_ascii_lowercase();let base=if site=="youtube"{"youtube.com"}else{"vimeo.com"};d==base||d.ends_with(&format!(".{base}"))}
fn clean_cookies(site:&str,cookies:Vec<Cookie>)->Result<Vec<Cookie>,String>{
    valid_site(site)?;if cookies.len()>2000{return Err("登录信息条目过多".into())}
    let mut out=Vec::new();for c in cookies{
        if !allowed_domain(site,&c.domain){return Err("Cookie 包含未授权的网站".into())}
        if [&c.domain,&c.path,&c.name,&c.value].iter().any(|s|s.len()>65536||s.contains(['\t','\r','\n','\0']))||!c.path.starts_with('/')||c.name.is_empty(){return Err("Cookie 格式无效".into())}
        if c.expiration.is_some_and(|e|!e.is_finite()||e<0.){return Err("Cookie 有效期无效".into())}
        if c.expiration.is_some_and(|e|e>0.&&e<=now() as f64){continue}out.push(c);
    }if out.is_empty(){return Err("未发现有效 Cookie，请先在当前网站登录后重新授权".into())}Ok(out)
}
fn crypt(data:&[u8],encrypt:bool)->Result<Vec<u8>,String>{
    use windows_sys::Win32::{Security::Cryptography::*,Foundation::LocalFree};
    let input=CRYPT_INTEGER_BLOB{cbData:data.len() as u32,pbData:data.as_ptr() as *mut u8};
    let mut output=CRYPT_INTEGER_BLOB{cbData:0,pbData:std::ptr::null_mut()};
    unsafe{let ok=if encrypt{CryptProtectData(&input,std::ptr::null(),std::ptr::null(),std::ptr::null(),std::ptr::null(),CRYPTPROTECT_UI_FORBIDDEN,&mut output)}else{CryptUnprotectData(&input,std::ptr::null_mut(),std::ptr::null(),std::ptr::null(),std::ptr::null(),CRYPTPROTECT_UI_FORBIDDEN,&mut output)};
    if ok==0{return Err("Windows 无法处理登录信息，请在当前 Windows 账号下重新授权".into())}let result=std::slice::from_raw_parts(output.pbData,output.cbData as usize).to_vec();LocalFree(output.pbData as _);Ok(result)}
}
fn save(site:&str,cookies:Vec<Cookie>,source:&str)->Result<(),String>{
    let cookies=clean_cookies(site,cookies)?;let grant=Grant{site:site.into(),cookies,source:source.into(),updated:now()};
    let bytes=crypt(&serde_json::to_vec(&grant).map_err(|_|"无法保存授权")?,true)?;
    let dir=root();fs::create_dir_all(&dir).map_err(|_|"无法创建授权目录")?;let temp=dir.join(format!("{}.tmp",queue::unique_id()));fs::write(&temp,bytes).map_err(|_|"无法保存授权")?;
    fs::rename(&temp,dir.join(format!("{site}.bin"))).map_err(|_|"无法更新授权")?;Ok(())
}
fn load(site:&str)->Result<Grant,String>{valid_site(site)?;let bytes=fs::read(root().join(format!("{site}.bin"))).map_err(|_|"尚未授权")?;let grant:Grant=serde_json::from_slice(&crypt(&bytes,false)?).map_err(|_|"授权文件损坏，请重新授权")?;if grant.site!=site{return Err("授权网站不匹配".into())}Ok(grant)}
fn netscape(cookies:&[Cookie])->String{let mut out=String::from("# Netscape HTTP Cookie File\n");for c in cookies{out.push_str(&format!("{}{}\t{}\t{}\t{}\t{}\t{}\t{}\n",if c.http_only{"#HttpOnly_"}else{""},c.domain,if c.host_only{"FALSE"}else{"TRUE"},c.path,if c.secure{"TRUE"}else{"FALSE"},c.expiration.unwrap_or(0.) as u64,c.name,c.value))}out}
pub struct CookieLease(pub PathBuf);
impl Drop for CookieLease{fn drop(&mut self){let _=fs::remove_file(&self.0);}}
pub fn lease(url:&str)->Result<Option<CookieLease>,String>{let Some(site)=site(url)else{return Ok(None)};if !root().join(format!("{site}.bin")).exists(){return Ok(None)}let grant=load(site)?;let cookies=clean_cookies(site,grant.cookies)?;let dir=root().join("temporary");fs::create_dir_all(&dir).map_err(|_|"无法准备登录信息")?;let p=dir.join(format!("{}.txt",queue::unique_id()));fs::write(&p,netscape(&cookies)).map_err(|_|"无法准备登录信息")?;Ok(Some(CookieLease(p)))}

pub fn native_entry()->bool{
    let Some(origin)=std::env::args().nth(1).filter(|s|s.starts_with("chrome-extension://"))else{return false};
    let result=(||->Result<Value,String>{if origin!=format!("chrome-extension://{}/",EXTENSION_ID.trim()){return Err("扩展未授权".into())}
        let mut input=std::io::stdin().lock();let mut length=[0u8;4];input.read_exact(&mut length).map_err(|_|"通信数据不完整")?;let length=u32::from_le_bytes(length) as usize;if length==0||length>LIMIT{return Err("授权消息过大".into())}let mut bytes=vec![0;length];input.read_exact(&mut bytes).map_err(|_|"通信数据不完整")?;
        let m:Message=serde_json::from_slice(&bytes).map_err(|_|"消息格式无效")?;handle_message(m)
    })();let answer=match result{Ok(v)=>v,Err(e)=>serde_json::json!({"ok":false,"message":e})};if let Ok(bytes)=serde_json::to_vec(&answer){let mut out=std::io::stdout().lock();let _=out.write_all(&(bytes.len() as u32).to_le_bytes());let _=out.write_all(&bytes);let _=out.flush();}true
}
fn handle_message(m:Message)->Result<Value,String>{
    if m.action=="ping"{return Ok(serde_json::json!({"ok":true,"message":"下载器已连接"}))}
    if !matches!(m.action.as_str(),"authorize"|"send"){return Err("不支持的操作".into())}let site=site(&m.url).ok_or("请打开 YouTube 或 Vimeo 的 HTTPS 视频页面")?;
    save(site,m.cookies,"浏览器扩展")?;
    if m.action=="send"{let inbox=root().join("inbox");fs::create_dir_all(&inbox).map_err(|_|"无法发送链接")?;atomic_json(&inbox.join(format!("{}.json",queue::unique_id())),&serde_json::json!({"url":m.url}))?;}
    Ok(serde_json::json!({"ok":true,"message":if m.action=="send"{"登录信息已更新，视频已发送。请在下载器的等待队列中开始任务。"}else{"登录信息已更新，请回到下载器重新解析视频。"}}))
}

#[tauri::command]
pub fn connect_browser(app:AppHandle)->Result<String,String>{
    let dir=root();fs::create_dir_all(&dir).map_err(|e|e.to_string())?;let manifest=dir.join("native-host.json");
    atomic_json(&manifest,&serde_json::json!({"name":HOST,"description":"视频下载器浏览器授权","path":std::env::current_exe().map_err(|e|e.to_string())?,"type":"stdio","allowed_origins":[format!("chrome-extension://{}/",EXTENSION_ID.trim())]}))?;
    for browser in ["Google\\Chrome","Microsoft\\Edge"]{let status=hidden_command(Path::new("reg.exe")).args(["ADD",&format!("HKCU\\Software\\{browser}\\NativeMessagingHosts\\{HOST}"),"/ve","/t","REG_SZ","/d",&manifest.to_string_lossy(),"/f"]).output().map_err(|e|e.to_string())?;if !status.status.success(){return Err("无法注册浏览器连接，请检查当前用户的注册表权限".into())}}
    let _=app;Ok("已启用 Edge / Chrome 连接，请安装配套扩展后点击检测连接".into())
}
#[tauri::command]
pub fn authorization_status()->Vec<Value>{["youtube","vimeo"].iter().map(|site|{match load(site){Ok(g)=>{let valid=g.cookies.iter().filter(|c|c.expiration.is_none_or(|e|e==0.||e>now() as f64)).count();serde_json::json!({"site":site,"saved":true,"updated":g.updated,"source":g.source,"status":if valid>0{"已保存，需通过视频解析确认有效"}else{"已过期，请重新授权"}})},Err(e)=>serde_json::json!({"site":site,"saved":root().join(format!("{site}.bin")).exists(),"status":e})}}).collect()}
#[tauri::command]
pub fn clear_authorization(site:String)->Result<(),String>{valid_site(&site)?;match fs::remove_file(root().join(format!("{site}.bin"))){Ok(())=>Ok(()),Err(e) if e.kind()==std::io::ErrorKind::NotFound=>Ok(()),Err(_)=>Err("无法清除授权".into())}}
fn parse_file(site:&str,text:&str)->Result<Vec<Cookie>,String>{
    if text.len()>LIMIT{return Err("Cookie 文件最多 1 MB".into())}let mut cookies=Vec::new();
    for line in text.trim_start_matches('\u{feff}').lines(){let http_only=line.starts_with("#HttpOnly_");if line.is_empty()||(line.starts_with('#')&&!http_only){continue}let line=line.strip_prefix("#HttpOnly_").unwrap_or(line);let fields:Vec<_>=line.split('\t').collect();if fields.len()!=7{return Err("请选择 Netscape 格式的 cookies.txt 文件".into())}if !allowed_domain(site,fields[0]){continue}let expiry=fields[4].parse::<f64>().map_err(|_|"Cookie 有效期格式错误")?;
    cookies.push(Cookie{domain:fields[0].into(),host_only:fields[1]=="FALSE",path:fields[2].into(),secure:fields[3]=="TRUE",expiration:if expiry==0.{None}else{Some(expiry)},name:fields[5].into(),value:fields[6].into(),http_only});}clean_cookies(site,cookies)
}
#[tauri::command]
pub async fn import_authorization(site:String)->Result<bool,String>{valid_site(&site)?;tauri::async_runtime::spawn_blocking(move||{let Some(p)=rfd::FileDialog::new().add_filter("Cookie 文件", &["txt"]).pick_file()else{return Ok(false)};if fs::metadata(&p).map_err(|_|"无法读取文件")?.len()>LIMIT as u64{return Err("Cookie 文件最多 1 MB".into())}let text=fs::read_to_string(p).map_err(|_|"请使用 UTF-8 文本文件")?;save(&site,parse_file(&site,&text)?,"文件导入")?;Ok(true)}).await.map_err(|e|e.to_string())?}
#[tauri::command]
pub fn extension_folder(app:AppHandle)->Result<String,String>{let p=app.path().resource_dir().map_err(|e|e.to_string())?.join("browser-extension");let p=if p.join("manifest.json").exists(){p}else{PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../extension")};if !p.join("manifest.json").exists(){return Err("未找到配套扩展，请重新安装软件".into())}Ok(p.to_string_lossy().into())}
fn process_inbox(app:&AppHandle){let inbox=root().join("inbox");for entry in fs::read_dir(&inbox).into_iter().flatten().flatten(){let p=entry.path();if p.extension().is_none_or(|s|s!="json")||p.file_name().unwrap_or_default().to_string_lossy().ends_with("next.json"){continue}let result=(||->Result<(),String>{let v:Value=serde_json::from_slice(&fs::read(&p).map_err(|_|"读取链接失败")?).map_err(|_|"链接格式错误")?;let url=v["url"].as_str().filter(|s|site(s).is_some()).ok_or("不支持的视频网站")?;let settings=queue::queue_snapshot(app.clone()).settings;queue::add_tasks(app.clone(),queue::AddRequest{sources:vec![url.into()],kind:"download".into(),settings,metadata:None,video_format_id:None,audio_format_id:None,start:false})?;Ok(())})();match result{Ok(())=>{let _=fs::remove_file(p);},Err(e)=>{let _=app.emit("browser-auth-error",format!("浏览器链接未加入队列：{e}。请设置有效下载目录后重新发送。"));let _=fs::rename(&p,p.with_extension("failed"));}}}}

pub fn start(app:&AppHandle){let app=app.clone();thread::spawn(move||loop{thread::sleep(std::time::Duration::from_secs(1));process_inbox(&app);});}
pub fn friendly_error(raw:&str)->String{
    let lower=raw.to_ascii_lowercase();
    let message=if lower.contains("could not copy chrome cookie database") {
        Some("无法读取 Edge / Chrome 的登录信息。请在“设置 → 网络与账号”启用浏览器助手，在视频页面重新授权；也可导入本站 Cookie 文件。无需反复关闭浏览器。")
    }else if lower.contains("could not find")&&lower.contains("cookies database") {
        Some("未找到所选浏览器的登录数据库。请使用浏览器助手授权，或确认选择了实际使用的浏览器与 Windows 账号。")
    }else if lower.contains("decrypt")&&(lower.contains("cookie")||lower.contains("dpapi")) {
        Some("Windows 无法解密浏览器的登录信息。请使用浏览器助手授权或导入本站 Cookie 文件。")
    }else if lower.contains("sign in to confirm") {
        Some("YouTube 要求验证登录或确认真人。请在使用同一代理线路的浏览器中完成验证，然后通过浏览器助手更新登录信息并重试。已授权仍可能遇到网站验证。")
    }else if lower.contains("web client only works when logged-in") {
        Some("Vimeo 当前解析方式需要有效登录信息。请在浏览器登录 Vimeo，通过浏览器助手更新本站登录信息，再重新解析。")
    }else{None};
    if let Some(message)=message{return message.into()}
    let mut lines=Vec::new();for line in raw.lines().filter(|l|!l.trim().is_empty()&&l.trim()!="null"){if !lines.contains(&line){lines.push(line)}}lines.join("\n")
}

#[cfg(debug_assertions)]
pub fn self_test(app:&AppHandle)->Result<(),String>{
    assert!(std::env::var_os("VIDEOTOOL_TEST_DIR").is_some());
    if root()!=app_data_dir(app).join("browser-auth"){return Err("主机与桌面授权目录不一致".into())}
    let cookies=parse_file("vimeo","#HttpOnly_.vimeo.com\tTRUE\t/\tTRUE\t0\tsession\tsynthetic-only")?;
    handle_message(Message{action:"send".into(),url:"https://vimeo.com/1191656107".into(),cookies})?;
    let lease=lease("https://vimeo.com/1191656107")?.ok_or("授权未接入")?;
    let temp=lease.0.clone();if !fs::read_to_string(&temp).map_err(|e|e.to_string())?.contains("synthetic-only"){return Err("授权解密结果不正确".into())}drop(lease);
    if temp.exists(){return Err("临时 Cookie 未清理".into())}
    process_inbox(app);
    let task=queue::queue_snapshot(app.clone()).tasks.into_iter().find(|t|t.source=="https://vimeo.com/1191656107").ok_or("浏览器链接未加入队列")?;
    if task.status!="waiting"{return Err("浏览器发送未经用户开始便下载了".into())}
    let mut args=Vec::new();let lease=push_cookies_args(app,&mut args,"https://vimeo.com/1191656107",Some("edge"),None)?;
    if args.first().is_none_or(|a|a!="--cookies")||args.iter().any(|a|a=="--cookies-from-browser"){return Err("扩展授权未优先于浏览器数据库".into())}drop(lease);
    clear_authorization("vimeo".into())?;
    if lease_check(){return Err("清除授权后仍可读取".into())}
    queue::task_action(app.clone(),task.id,"remove".into())?;
    Ok(())
}
#[cfg(debug_assertions)]fn lease_check()->bool{lease("https://vimeo.com/1191656107").ok().flatten().is_some()}

#[cfg(test)]mod tests{use super::*;
#[test]fn rejects_foreign_domains_and_injection(){assert_eq!(site("https://youtube.com.evil.test/watch"),None);assert_eq!(site("https://vimeo.com/123"),Some("vimeo"));assert!(parse_file("youtube",".youtube.com\tTRUE\t/\tTRUE\t0\ta\tb\n").is_ok());assert!(parse_file("vimeo",".youtube.com\tTRUE\t/\tTRUE\t0\ta\tb").is_err());}
#[test]fn windows_encryption_roundtrip(){let secret=b"test session - not a real cookie";let encrypted=crypt(secret,true).unwrap();assert_ne!(encrypted,secret);assert_eq!(crypt(&encrypted,false).unwrap(),secret);}
#[test]fn expires_and_httponly_roundtrip(){let c=parse_file("vimeo","#HttpOnly_.vimeo.com\tTRUE\t/\tTRUE\t0\tsession\tsample").unwrap();assert!(c[0].http_only);assert!(netscape(&c).contains("#HttpOnly_"));assert!(parse_file("vimeo",".vimeo.com\tTRUE\t/\tTRUE\t1\tsession\told").is_err());}
#[test]fn diagnoses_screenshot_errors(){assert!(friendly_error("ERROR: Could not copy Chrome cookie database").contains("浏览器助手"));assert!(friendly_error("The web client only works when logged-in").contains("Vimeo"));assert_eq!(friendly_error("null\nERROR one\nERROR one"),"ERROR one");}
}
