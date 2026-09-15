use crate::*;
use std::{collections::{BTreeMap, BTreeSet}, time::Duration};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Options { pub enabled: bool, pub language: String, pub source: String }
impl Default for Options { fn default()->Self {Self{enabled:false,language:"bilingual".into(),source:"prefer".into()}} }
impl Options { pub fn validate(&self)->Result<(),String>{
    if !["en","zh","bilingual"].contains(&self.language.as_str()) || !["prefer","manual","auto"].contains(&self.source.as_str()) {return Err("无效字幕选项".into())} Ok(())
} }
#[derive(Clone, Debug)]
struct Track { language:String, automatic:bool }
fn language_rank(code:&str, family:&str)->Option<usize>{
    if !code.chars().all(|c|c.is_ascii_alphanumeric()||c=='-'||c=='_'){return None}
    let ranks=if family=="zh"{&["zh-Hans","zh-CN","zh","zh-Hant","zh-TW","zh-HK"][..]}else{&["en","en-orig","en-US","en-GB"][..]};
    ranks.iter().position(|v|*v==code).or_else(||code.starts_with(&format!("{family}-")).then_some(20))
}
fn choose(data:&Value,family:&str,source:&str)->Option<Track>{
    let kinds=match source{"manual"=>vec![false],"auto"=>vec![true],_=>vec![false,true]};
    for automatic in kinds {
        if let Some(tracks)=data[if automatic{"automatic_captions"}else{"subtitles"}].as_object(){
            let best=tracks.iter().filter(|(_,v)|v.as_array().is_some_and(|a|!a.is_empty()))
                .filter_map(|(code,_)|language_rank(code,family).map(|rank|(rank,code)))
                .min_by(|a,b|a.0.cmp(&b.0).then_with(||a.1.cmp(b.1)));
            if let Some((_,language))=best{return Some(Track{language:language.clone(),automatic})}
        }
    }None
}

#[derive(Debug,Clone,PartialEq)]
struct Cue { start:u64,end:u64,text:String }
fn timestamp(s:&str)->Option<u64>{
    let parts:Vec<_>=s.trim().split([':',',','.']).collect();if parts.len()!=4{return None}
    let n:Vec<u64>=parts.iter().map(|p|p.parse::<u64>()).collect::<Result<_,_>>().ok()?;
    if n[0]>999||n[1]>=60||n[2]>=60||n[3]>=1000{return None}Some(((n[0]*60+n[1])*60+n[2])*1000+n[3])
}
fn clean_text(s:&str)->String{
    let mut result=String::new();let mut tag=false;
    for c in s.chars(){if c=='<'{tag=true}else if c=='>'&&tag{tag=false}else if !tag{result.push(c)}}
    result.replace("&amp;","&").replace("&lt;","<").replace("&gt;",">").replace("&quot;","\"").replace("&#39;","'").replace("&nbsp;"," ").trim().to_string()
}
fn parse(s:&str)->Result<Vec<Cue>,String>{
    let normalized=s.trim_start_matches('\u{feff}').replace("\r\n","\n");let mut cues=Vec::new();
    for block in normalized.split("\n\n"){
        let lines:Vec<_>=block.lines().collect();let Some(index)=lines.iter().position(|l|l.contains(" --> ")) else{continue};
        let Some((a,b))=lines[index].split_once(" --> ")else{continue};
        if let (Some(start),Some(end))=(timestamp(a),timestamp(b.split_whitespace().next().unwrap_or(""))){
            let text=clean_text(&lines[index+1..].join("\n"));if end>start&&!text.is_empty(){cues.push(Cue{start,end,text})}
        }
    }
    cues.sort_by_key(|c|(c.start,c.end));cues.dedup();if cues.is_empty(){Err("字幕为空或时间轴无效".into())}else{Ok(cues)}
}
fn clock(ms:u64)->String{format!("{:02}:{:02}:{:02},{:03}",ms/3600000,ms/60000%60,ms/1000%60,ms%1000)}
fn render(cues:&[Cue])->String{cues.iter().enumerate().map(|(i,c)|format!("{}\n{} --> {}\n{}\n\n",i+1,clock(c.start),clock(c.end),c.text)).collect()}
// Merge on the union of cue boundaries, never by subtitle index. This preserves
// translations with different segmentation and retains one-language gaps.
fn merge(zh:&[Cue],en:&[Cue])->Vec<Cue>{
    let all:Vec<_>=zh.iter().chain(en).collect();let mut events:BTreeMap<u64,Vec<(usize,bool)>>=BTreeMap::new();
    for (i,c) in all.iter().enumerate(){events.entry(c.start).or_default().push((i,true));events.entry(c.end).or_default().push((i,false));}
    let times:Vec<_>=events.keys().copied().collect();let mut active=BTreeSet::new();let mut out:Vec<Cue>=Vec::new();
    for pair in times.windows(2){for (i,start) in &events[&pair[0]]{if *start{active.insert(*i);}else{active.remove(i);}}
        let mut lines=Vec::new();for i in &active{for line in all[*i].text.lines(){if !lines.contains(&line){lines.push(line)}}}
        let text=lines.join("\n");if text.is_empty(){continue}
        if let Some(last)=out.last_mut(){if last.end==pair[0]&&last.text==text{last.end=pair[1];continue}}
        out.push(Cue{start:pair[0],end:pair[1],text});
    }out
}
struct Scratch(PathBuf);
impl Drop for Scratch{fn drop(&mut self){let _=fs::remove_dir_all(&self.0);}}

pub fn download(app:&AppHandle,url:&str,settings:&queue::Settings,video:Option<&Path>)->Result<DownloadResult,String>{
    settings.subtitles.validate()?;validate_url(url)?;
    let dir=PathBuf::from(&settings.output_dir);if !dir.is_dir(){return Err("字幕保存目录不存在".into())}
    let scratch=Scratch(app_data_dir(app).join("subtitle-work").join(queue::unique_id()));fs::create_dir_all(&scratch.0).map_err(|e|e.to_string())?;
    let mut common:Vec<OsString>=vec!["--proxy".into(),if settings.use_proxy{settings.proxy.clone().into()}else{"".into()}];
    let _lease=push_cookies_args(app,&mut common,url,Some(&settings.cookies_browser),None)?;
    queue::progress(app,0.,"正在读取字幕列表","","");
    let mut args=common.clone();args.extend(["-J".into(),"--skip-download".into(),"--ignore-no-formats-error".into(),"--".into(),url.into()]);
    let output=process::run(&resolve_tool(app,"yt-dlp"),yt_args(app,args),Duration::from_secs(120),|_|{})
        .map_err(|_|if process::cancelled(){"任务已取消"}else{"字幕列表获取失败，请检查网站授权或网络后重试"}.to_string())?;
    let data:Value=serde_json::from_str(&output.stdout).map_err(|_|"字幕列表数据无效")?;
    if let Some(id)=queue::current(){if let Some(title)=data["title"].as_str(){queue::edit(app,&id,|t|t.title=title.into(),true);}}
    let info=scratch.0.join("info.json");atomic_json(&info,&data)?;
    let families=if settings.subtitles.language=="bilingual"{vec!["zh","en"]}else{vec![settings.subtitles.language.as_str()]};
    let mut downloaded=Vec::new();let mut descriptions=Vec::new();
    for family in families{
        let track=choose(&data,family,&settings.subtitles.source).ok_or_else(||format!("未找到所选来源的{}字幕；可尝试“优先原生，缺失时自动字幕”",if family=="zh"{"中文"}else{"英文"}))?;
        queue::progress(app,0.,&format!("正在下载{}字幕",if family=="zh"{"中文"}else{"英文"}),"","");
        let folder=scratch.0.join(family);fs::create_dir_all(&folder).map_err(|e|e.to_string())?;
        let mut args=common.clone();args.extend(["--load-info-json".into(),info.clone().into_os_string(),"--skip-download".into(),"--ignore-no-formats-error".into(),
            if track.automatic{"--no-write-subs".into()}else{"--write-subs".into()},if track.automatic{"--write-auto-subs".into()}else{"--no-write-auto-subs".into()},
            "--sub-langs".into(),track.language.clone().into(),"--sub-format".into(),"vtt/srt/best".into(),"--convert-subs".into(),"srt".into(),"--output".into(),folder.join("subtitle.%(ext)s").into_os_string()]);
        process::run(&resolve_tool(app,"yt-dlp"),yt_args(app,args),Duration::from_secs(120),|_|{})
            .map_err(|e|if process::cancelled(){"任务已取消"}else if e.contains("429"){"网站限制了字幕请求（HTTP 429），请稍后重试"}else if e.contains("403"){"网站拒绝了字幕请求（HTTP 403），请更新网站授权后重试"}else{"字幕下载或 SRT 转换失败，请稍后重试；网站可能限制字幕访问"}.to_string())?;
        let path=fs::read_dir(&folder).map_err(|e|e.to_string())?.flatten().map(|e|e.path()).find(|p|p.extension().is_some_and(|x|x=="srt")).ok_or("网站未返回可用的 SRT 字幕")?;
        let text=fs::read_to_string(path).map_err(|_|"字幕文件无法读取")?;
        downloaded.push((family.to_string(),parse(&text)?));descriptions.push(format!("{}：{}（{}）",family,track.language,if track.automatic{"网站自动生成/翻译"}else{"原生"}));
    }
    if settings.subtitles.language=="bilingual"{let bilingual=merge(&downloaded[0].1,&downloaded[1].1);downloaded.push(("zh-en".into(),bilingual));}
    if process::cancelled(){return Err("任务已取消".into())}
    let stem=video.and_then(|p|p.file_stem()).map(|s|s.to_string_lossy().to_string()).unwrap_or_else(||engine::safe_stem(data["title"].as_str().unwrap_or("字幕")));
    let target_dir=video.and_then(|p|p.parent()).unwrap_or(&dir);let mut paths=Vec::new();
    for (lang,cues) in downloaded{
        let text=render(&cues);let planned=target_dir.join(format!("{stem}.{lang}.srt"));
        if fs::read(&planned).is_ok_and(|b|b==text.as_bytes()){paths.push(planned);continue}
        let temp=scratch.0.join(format!("{lang}.srt"));fs::write(&temp,text).map_err(|e|e.to_string())?;
        let (target,_reservation)=engine::reserve(&planned)?;paths.push(engine::publish(&temp,&target)?);
    }
    let output_path=paths.last().ok_or("未生成字幕")?.to_string_lossy().to_string();
    Ok(DownloadResult{output_path,container:"srt".into(),selected_vcodec:None,selected_acodec:None,skipped:false,
        log:format!("字幕已保存（UTF-8 SRT）\n{}\n{}",descriptions.join("\n"),paths.iter().map(|p|p.display().to_string()).collect::<Vec<_>>().join("\n"))})
}

#[cfg(test)]mod tests{
    use super::*;
    #[test]fn selects_sources_and_languages(){let d=serde_json::json!({"subtitles":{"en-US":[{}]},"automatic_captions":{"en":[{}],"zh-Hans":[{}],"zh-Hant":[{}]}});
        assert!(!choose(&d,"en","prefer").unwrap().automatic);assert!(choose(&d,"en","auto").unwrap().automatic);
        assert_eq!(choose(&d,"zh","prefer").unwrap().language,"zh-Hans");assert!(choose(&d,"zh","manual").is_none());assert!(language_rank("en.*","en").is_none());}
    #[test]fn srt_roundtrip_and_different_timing(){let en=parse("1\n00:00:00,000 --> 00:00:02,000\nHello &amp; welcome\n\n2\n00:00:02,000 --> 00:00:04,000\nNext\n").unwrap();
        let zh=parse("1\n00:00:01,000 --> 00:00:03,000\n<b>你好</b>\n").unwrap();let merged=merge(&zh,&en);
        assert_eq!(merged.len(),4);assert_eq!(merged[1].text,"你好\nHello & welcome");assert_eq!(merged[2].text,"你好\nNext");assert_eq!(parse(&render(&merged)).unwrap(),merged);}
    #[test]fn rejects_empty_and_invalid_times(){assert!(parse("WEBVTT").is_err());assert!(timestamp("00:99:00,000").is_none());assert!(parse("1\n00:00:02,000 --> 00:00:01,000\nx").is_err());}
}
