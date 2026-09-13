use crate::*;
use std::time::{Duration, Instant};

// Enrichment is optional: a slow or inaccessible rendition must remain downloadable.
pub fn enrich(app: &AppHandle, json: &Value, formats: &mut [FormatInfo], proxy: Option<&str>) {
    let Some(raw) = json["formats"].as_array() else { return };
    let started = Instant::now();
    for format in formats.iter_mut().filter(|f| f.vcodec.as_deref() == Some("none") && format_has_audio(f)
        && (f.acodec.is_none() || f.abr.is_none())).take(2) {
        let remaining = Duration::from_secs(16).saturating_sub(started.elapsed());
        if remaining.is_zero() || process::cancelled() { break }
        let Some(source) = raw.iter().find(|r| r["format_id"].as_str() == Some(&format.format_id)) else { continue };
        let Some(url) = source["url"].as_str() else { continue };
        if !tauri::Url::parse(url).is_ok_and(|u| u.scheme() == "https") { continue }
        let mut args: Vec<OsString> = ["-v", "error", "-rw_timeout", "5000000", "-protocol_whitelist", "https,http,httpproxy,tcp,tls,crypto",
            "-probesize", "524288", "-analyzeduration", "2000000", "-read_intervals", "%+5",
            "-select_streams", "a:0", "-show_entries", "stream=codec_name:packet=size,duration_time", "-of", "json"]
            .into_iter().map(Into::into).collect();
        if let Some(proxy) = clean_proxy(proxy) { args.extend(["-http_proxy".into(), proxy.into()]); }
        // Use only non-secret playback headers. Signed media URLs provide access;
        // never forward the website Cookie jar to arbitrary CDN hosts.
        for (header, option) in [("User-Agent", "-user_agent"), ("Referer", "-referer")] {
            if let Some(value) = source["http_headers"][header].as_str().or_else(||json["http_headers"][header].as_str()) {
                if !value.contains(['\r','\n']) { args.extend([option.into(), value.into()]); }
            }
        }
        args.push(url.into());
        let result = process::run(&resolve_tool(app, "ffprobe"), args, remaining.min(Duration::from_secs(8)), |_| {});
        if let Ok(output) = result {
            if let Ok(data) = serde_json::from_str::<Value>(&output.stdout) { apply(format, &data, json["duration"].as_f64()); }
        }
    }
}

fn apply(format: &mut FormatInfo, data: &Value, duration: Option<f64>) {
    if let Some(codec) = data["streams"].as_array().and_then(|s|s.first()).and_then(|s|s["codec_name"].as_str()) {
        if !codec.is_empty() && codec != "unknown" && codec != "none" { format.acodec = Some(codec.into()); }
    }
    let Some(packets) = data["packets"].as_array() else { return };
    let mut bytes = 0.; let mut seconds = 0.;
    for packet in packets {
        let size = packet["size"].as_str().and_then(|s|s.parse::<f64>().ok());
        let time = packet["duration_time"].as_str().and_then(|s|s.parse::<f64>().ok());
        if let (Some(size), Some(time)) = (size, time) {
            if size.is_finite() && size > 0. && time.is_finite() && time > 0. { bytes += size; seconds += time; }
        }
    }
    let rate = bytes * 8. / seconds / 1000.;
    if seconds >= 1. && rate.is_finite() && rate > 0. && rate <= 10000. {
        if format.abr.is_none() { format.abr = Some(rate); }
        if format.filesize.is_none() && format.filesize_approx.is_none() {
            if let Some(duration) = duration.filter(|d|d.is_finite() && *d > 0.) {
                format.filesize_approx = Some((duration * format.abr.unwrap_or(rate) * 1000. / 8.) as u64);
            }
        }
    }
}

#[cfg(test)] mod tests {
    use super::*;
    #[test] fn sample_is_estimated_and_codec_is_observed() {
        let mut f = parse_format(&serde_json::json!({"format_id":"a","vcodec":"none","ext":"mp4"})).unwrap();
        apply(&mut f, &serde_json::json!({"streams":[{"codec_name":"aac"}],"packets":[{"size":"16000","duration_time":"1.0"}]}), Some(20.));
        assert_eq!(f.acodec.as_deref(),Some("aac")); assert_eq!(f.abr,Some(128.));
        assert_eq!(f.filesize,None); assert_eq!(f.filesize_approx,Some(320000));
        assert!(can_mux_mp4(Some(&parse_format(&serde_json::json!({"format_id":"v","vcodec":"h264","ext":"mp4"})).unwrap()),Some(&f)));
    }
    #[test] fn incomplete_sample_does_not_invent_rate() {
        let mut f = parse_format(&serde_json::json!({"format_id":"a","vcodec":"none"})).unwrap();
        apply(&mut f, &serde_json::json!({"streams":[],"packets":[{"size":"16000","duration_time":"0"}]}), Some(20.));
        assert_eq!(f.abr,None); assert_eq!(f.acodec,None); assert_eq!(f.filesize_approx,None);
    }
}
