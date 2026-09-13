use crate::*;
fn format(id:&str,vc:&str,ac:&str,height:u64)->FormatInfo{FormatInfo{format_id:id.into(),ext:Some(if vc=="none"{"m4a"}else{"mp4"}.into()),vcodec:Some(vc.into()),acodec:Some(ac.into()),height:Some(height),fps:Some(30.),tbr:Some(1000.),filesize:None,filesize_approx:None,abr:Some(128.)}}
fn metadata(formats:Vec<FormatInfo>)->ProbeResult{ProbeResult{id:"one".into(),title:"test".into(),webpage_url:"https://example.com/video".into(),extractor:"test".into(),thumbnail:None,duration_string:None,anonymous:false,best_height:None,best_vcodec:None,should_save_mkv:false,formats}}
#[test]fn combined_stream_is_video(){let m=metadata(vec![format("combined","avc1","mp4a",1080),format("audio","none","mp4a",0)]);let s=select_download_formats(&m,None,None,None,&SaveStrategy::Auto);assert!(!s.audio_only);assert!(!s.use_mkv);assert_eq!(s.format_selector,"combined");}
#[test]fn separated_streams_are_merged(){let m=metadata(vec![format("video","avc1","none",1080),format("audio","none","mp4a",0)]);let s=select_download_formats(&m,None,None,None,&SaveStrategy::Auto);assert_eq!(s.format_selector,"video+audio");}
#[test]fn vimeo_hls_unknown_audio_is_not_discarded(){
    let audio=parse_format(&serde_json::json!({"format_id":"hls-audio-English","vcodec":"none","acodec":null,"ext":"mp4"})).unwrap();
    assert!(format_has_audio(&audio));assert_eq!(audio.acodec,None);
    let m=metadata(vec![format("video","avc1","none",1080),audio]);
    let s=select_download_formats(&m,None,None,None,&SaveStrategy::Auto);
    assert_eq!(s.format_selector,"video+hls-audio-English");assert!(s.use_mkv);
    let manual=select_download_formats(&m,None,Some("video"),Some("hls-audio-English"),&SaveStrategy::Auto);
    assert_eq!(manual.format_selector,s.format_selector);
    assert_eq!(best_audio_format(&m,false).unwrap().format_id,"hls-audio-English");
}
#[test]fn missing_codec_does_not_override_explicit_silence(){
    assert!(!format_has_audio(&format("silent","none","none",0)));
    let unknown=parse_format(&serde_json::json!({"format_id":"unknown"})).unwrap();assert!(!format_has_audio(&unknown));
    let missing=parse_format(&serde_json::json!({"format_id":"audio","vcodec":"none"})).unwrap();assert!(format_has_audio(&missing));
}
#[test]fn height_limit_preserves_best_resolution(){let m=metadata(vec![format("vp9","vp9","none",1080),format("h264","avc1","none",720),format("audio","none","mp4a",0)]);let s=select_download_formats(&m,Some(1080),None,None,&SaveStrategy::Auto);assert_eq!(s.video.unwrap().format_id,"vp9");let limited=select_download_formats(&m,Some(720),None,None,&SaveStrategy::Auto);assert_eq!(limited.video.unwrap().height,Some(720));}
#[test]fn audio_only_is_detected(){let m=metadata(vec![format("audio","none","mp4a",0)]);assert!(select_download_formats(&m,None,None,None,&SaveStrategy::Auto).audio_only);}
#[test]fn invalid_explicit_video_id_cannot_choose_audio(){let m=metadata(vec![format("video","avc1","none",1080),format("audio","none","mp4a",0)]);let s=select_download_formats(&m,None,Some("audio"),None,&SaveStrategy::Auto);assert_eq!(s.video.unwrap().format_id,"video");}
#[test]fn url_arguments_are_validated(){assert!(validate_url("--exec=calc").is_err());assert!(validate_url("file:///C:/secret").is_err());assert!(validate_url("https://www.youtube.com/watch?v=abc").is_ok());}
#[test]fn atomic_json_replaces_existing(){let root=std::env::temp_dir().join(queue::unique_id());let path=root.join("state.json");atomic_json(&path,&serde_json::json!({"n":1})).unwrap();atomic_json(&path,&serde_json::json!({"n":2})).unwrap();let v:Value=serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();assert_eq!(v["n"],2);fs::remove_dir_all(root).unwrap();}
#[test]
fn equal_resolution_prefers_mp4_pair_over_vp9() {
    let mut vp = format("625", "vp09.00.50.08", "none", 2160); vp.fps=Some(60.); vp.tbr=Some(20000.);
    let m=metadata(vec![vp, format("401","av01.0.12M.08","none",2160),format("251","none","opus",0),format("140","none","mp4a.40.2",0)]);
    let s=select_download_formats(&m,None,None,None,&SaveStrategy::Auto);
    assert_eq!(s.format_selector,"401+140"); assert!(!s.use_mkv);
    let manual=select_download_formats(&m,None,Some("625"),Some("251"),&SaveStrategy::Auto);
    assert_eq!(manual.format_selector,"625+251"); assert!(manual.use_mkv);
}
#[test]
fn mp4_preference_never_lowers_resolution() {
    let m=metadata(vec![format("vp9","vp9","none",2160),format("av1","av01","none",1080),format("aac","none","mp4a",0)]);
    let s=select_download_formats(&m,None,None,None,&SaveStrategy::Auto);
    assert_eq!(s.video.unwrap().format_id,"vp9"); assert!(s.use_mkv);
}
#[test]
fn av1_with_opus_uses_mkv_and_silent_av1_uses_mp4() {
    let m=metadata(vec![format("av1","av01","none",2160),format("opus","none","opus",0)]);
    assert!(select_download_formats(&m,None,None,None,&SaveStrategy::Auto).use_mkv);
    let silent=metadata(vec![format("av1","av01","none",2160)]);
    assert!(!select_download_formats(&silent,None,None,None,&SaveStrategy::Auto).use_mkv);
}
#[test]
fn old_settings_default_to_clean_names() {
    let settings:queue::Settings=serde_json::from_value(serde_json::json!({"output_dir":"C:/Videos"})).unwrap();
    assert!(!settings.filename_suffix);
}
#[test]
fn format_sizes_are_preserved() {
    let f=parse_format(&serde_json::json!({"format_id":"401","filesize":1234,"filesize_approx":1500})).unwrap();
    assert_eq!(f.filesize,Some(1234));assert_eq!(f.filesize_approx,Some(1500));
}
#[test]
fn filename_suffix_is_human_readable() {
    let v=format("401","av01.0.12M.08","none",2160);
    let a=format("140","none","mp4a.40.2",0);
    assert_eq!(engine::format_suffix(Some(&v),Some(&a),false,true),"2160p_AV1_AAC");
}
#[test]
fn bili_access_errors_are_not_login_requirements() {
    assert!(bili_retryable("HTTP Error 412"));
    assert!(bili_error("HTTP Error 412").contains("不代表必须登录"));
    assert!(!is_bilibili("https://bilibili.com.example.org/video/test"));
}
#[test]
fn suffix_defaults_to_resolution_without_codecs() {
    let v=format("401","av01.0.12M.08","none",2160);
    let a=format("140","none","mp4a.40.2",0);
    assert_eq!(engine::format_suffix(Some(&v),Some(&a),false,false),"2160p");
    let settings:queue::Settings=serde_json::from_value(serde_json::json!({"filename_suffix":true})).unwrap();
    assert!(settings.filename_suffix);assert!(!settings.filename_codecs);
}
