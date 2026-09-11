use std::{ffi::OsString, io::{BufRead, BufReader}, path::Path, process::Stdio, sync::{atomic::{AtomicBool, Ordering}, Arc}, thread, time::{Duration, Instant}};
use crate::{hidden_command, ProcessOutput};

#[cfg(windows)]
struct ProcessJob(windows_sys::Win32::Foundation::HANDLE);
#[cfg(windows)]
impl ProcessJob {
    fn attach(child:&std::process::Child)->Result<Self,String>{
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::System::JobObjects::*;
        unsafe {
            let handle=CreateJobObjectW(std::ptr::null(),std::ptr::null());
            if handle.is_null(){return Err(std::io::Error::last_os_error().to_string())}
            let job=Self(handle);
            let mut info:JOBOBJECT_EXTENDED_LIMIT_INFORMATION=std::mem::zeroed();
            info.BasicLimitInformation.LimitFlags=JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            if SetInformationJobObject(handle,JobObjectExtendedLimitInformation,&info as *const _ as *const _,std::mem::size_of_val(&info) as u32)==0 || AssignProcessToJobObject(handle,child.as_raw_handle())==0 {return Err(std::io::Error::last_os_error().to_string())}
            Ok(job)
        }
    }
}
#[cfg(windows)]impl Drop for ProcessJob{fn drop(&mut self){unsafe{windows_sys::Win32::Foundation::CloseHandle(self.0);}}}

thread_local! { static CANCEL: std::cell::RefCell<Option<Arc<AtomicBool>>> = const { std::cell::RefCell::new(None) }; }
pub fn set_cancel(flag: Option<Arc<AtomicBool>>) { CANCEL.with(|v| *v.borrow_mut() = flag); }
pub fn cancelled() -> bool { CANCEL.with(|v| v.borrow().as_ref().map(|f| f.load(Ordering::Relaxed)).unwrap_or(false)) }
pub fn run(program: &Path, args: Vec<OsString>, timeout: Duration, mut line: impl FnMut(&str)) -> Result<ProcessOutput, String> {
    if cancelled() { return Err("任务已取消".into()); }
    let mut child=hidden_command(program).args(args).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().map_err(|e|format!("无法启动 {}：{e}",program.display()))?;
    #[cfg(windows)]
    let _job=match ProcessJob::attach(&child){Ok(job)=>job,Err(e)=>{let _=child.kill();let _=child.wait();return Err(format!("无法管理子进程：{e}"))}};
    let (tx, rx)=std::sync::mpsc::sync_channel(256);
    let mut readers=Vec::new();
    for (is_err, pipe) in [(false,child.stdout.take().map(|p|Box::new(p) as Box<dyn std::io::Read+Send>)),(true,child.stderr.take().map(|p|Box::new(p) as Box<dyn std::io::Read+Send>))] {
        let tx=tx.clone();
        if let Some(pipe)=pipe { readers.push(thread::spawn(move|| {let mut reader=BufReader::new(pipe);let mut bytes=Vec::new();loop{bytes.clear();match reader.read_until(b'\n',&mut bytes){Ok(0)|Err(_)=>break,_=>{if tx.send((is_err,String::from_utf8_lossy(&bytes).into_owned())).is_err(){break}}}}})); }
    }
    drop(tx);
    let start=Instant::now();let mut stdout=String::new();let mut stderr=String::new();let mut forced=None;
    loop {
        match rx.recv_timeout(Duration::from_millis(80)) {
            Ok((err,text))=> {line(text.trim_end());let buf=if err{&mut stderr}else{&mut stdout};buf.push_str(&text);if buf.len()>4_000_000 { let mut at=buf.len()-3_000_000;while !buf.is_char_boundary(at){at+=1}buf.drain(..at); }},
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected)=>break,
            Err(_)=>{}
        }
        if forced.is_none() && (cancelled() || start.elapsed()>timeout) {
            forced=Some(if cancelled(){"任务已取消"}else{"操作超时，请检查网络或代理后重试"});
            #[cfg(windows)] {let _=hidden_command(Path::new("taskkill.exe")).args(["/PID",&child.id().to_string(),"/T","/F"]).output();}
            let _=child.kill();
        }
    }
    let status=child.wait().map_err(|e|e.to_string())?;for r in readers{let _=r.join();}
    if let Some(reason)=forced{return Err(reason.into())}
    if status.success(){Ok(ProcessOutput{stdout,stderr})}else{Err(crate::browser_auth::friendly_error(&format!("{} 执行失败\n{}\n{}",program.file_name().unwrap_or_default().to_string_lossy(),stdout.trim(),stderr.trim())))}
}
