import type { Format } from './types';

export function codecLabel(value: string | null | undefined): string {
  const c=(value || '').toLowerCase();
  if (/^(av01|av1)/.test(c)) return 'AV1';
  if (/^(avc1|h264)/.test(c)) return 'H.264';
  if (/^(hvc1|hev1|hevc|h265)/.test(c)) return 'H.265';
  if (/^(vp09|vp9)/.test(c)) return 'VP9';
  if (c.startsWith('vp8')) return 'VP8';
  if (/^(mp4a|aac)/.test(c)) return 'AAC';
  if (c.startsWith('opus')) return 'Opus';
  if (c==='none'||!c) return '未知';
  return c.split('.')[0].toUpperCase();
}
export function sizeLabel(f: Format): string {
  const bytes=f.filesize || f.filesize_approx;
  if (!bytes) return '大小未知';
  const value=bytes>=1024**3?`${(bytes/1024**3).toFixed(2)} GB`:`${(bytes/1024**2).toFixed(1)} MB`;
  return `${f.filesize?'':'约 '}${value}`;
}

export function audioLabel(f: Format): string {
  const label = codecLabel(f.acodec);
  const parts = [label === '未知' ? '音轨' : label];
  if(label==='未知') {
    const note=f.format_note||'';
    if(/\blow\b/i.test(note))parts.push('低音质');
    else if(/\bhigh\b/i.test(note))parts.push('高音质');
  }
  const identity=audioIdentity(f);if(identity) parts.push(identity);
  if (audioDrc(f)) parts.push('音量动态压缩');
  if (f.abr && f.abr > 0) parts.push(`约 ${Math.round(f.abr)} kbps`);
  if (f.filesize || f.filesize_approx) parts.push(sizeLabel(f));
  if (!f.abr && !f.filesize && !f.filesize_approx) parts.push('参数暂未获取');
  return parts.join(' · ');
}

// Presentation groups do not change the automatic best-format selection.
export function groupFormats(formats: Format[], kind: 'video' | 'audio') {
  const groups = new Map<string, Format[]>();
  for (const f of formats) {
    const rawName = codecLabel(kind === 'video' ? f.vcodec : f.acodec);
    const name = kind === 'audio' && rawName === '未知' ? '音轨' : rawName;
    const group = groups.get(name) || [];
    group.push(f);
    groups.set(name, group);
  }
  const order = kind === 'video' ? ['AV1','H.264','H.265','VP9','VP8'] : ['AAC','Opus'];
  const rank = (name: string) => order.indexOf(name) < 0 ? 99 : order.indexOf(name);
  return [...groups].sort(([a],[b]) => rank(a)-rank(b) || a.localeCompare(b)).map(([codec, items]) => ({
    codec,
    formats: items.slice().sort((a,b) => kind === 'video'
      ? (b.height||0)-(a.height||0) || (b.fps||0)-(a.fps||0) || (b.tbr||0)-(a.tbr||0)
      : (b.abr||0)-(a.abr||0)),
  }));
}

const codec = (value: string | null) => (value || '').toLowerCase();
export const hasAudio = (f?: Format) => !!f && (f.acodec ? f.acodec !== 'none' : f.vcodec === 'none');
export const isAac = (f?: Format) => !!f && /^(mp4a|aac)/.test(codec(f.acodec)) && /^(m4a|mp4)$/i.test(f.ext || '');
export const mp4Video = (f?: Format) => !!f && /^(av01|av1|avc1|h264)/.test(codec(f.vcodec)) && /^mp4$/i.test(f.ext || '');
export const canMp4 = (v?: Format, a?: Format) => mp4Video(v) && (!a || isAac(a));
function rank(f: Format) {
  const c = codec(f.vcodec);
  return /^(av01|av1)/.test(c) ? 0 : /^(avc1|h264)/.test(c) ? 1 : /^(vp09|vp9)/.test(c) ? 2 : c.startsWith('vp8') ? 3 : 9;
}
export function audioFor(formats: Format[], v?: Format): Format | undefined {
  if (hasAudio(v)) return v;
  const opus=/^(vp09|vp9)/.test(codec(v?.vcodec || null));
  const preference=(f:Format)=>opus&&codec(f.acodec).startsWith('opus')?2:isAac(f)?(opus?1:2):0;
  return formats.filter(f => (!f.vcodec || f.vcodec === 'none') && hasAudio(f)).sort((a,b) =>
    audioLanguageRank(b)-audioLanguageRank(a) || Number(audioDrc(a))-Number(audioDrc(b)) || preference(b)-preference(a) || (b.abr||0)-(a.abr||0) || (a.format_id<b.format_id?-1:a.format_id>b.format_id?1:0))[0];
}
export function audioLanguageRank(f:Format):number {
  const note=(f.format_note||'').toLowerCase();
  if(note.includes('original')||(f.language_preference??-1)>=10)return 3;
  if(note.includes('descriptive')||(f.language_preference??-1)<=-10)return -1;
  if(note.includes('(default)')||(f.language_preference??-1)>=5)return 2;
  return 0;
}
export const audioDrc=(f:Format)=>f.format_id.includes('-drc')||/drc/i.test(f.format_note||'');
export function languageLabel(language:string):string {
  try{return new Intl.DisplayNames(['zh-CN'],{type:'language'}).of(language==='iw'?'he':language)||language}catch{return language}
}
export function audioIdentity(f:Format):string {
  const parts=f.language?[languageLabel(f.language)]:[];
  const rank=audioLanguageRank(f);
  if(rank===3)parts.push('原声');else if(rank===2)parts.push('默认');else if(rank===-1)parts.push('口述影像');
  else if(/dubbed-auto/i.test(f.format_note||''))parts.push('自动配音');
  return parts.join(' · ');
}
export function orderedVideos(formats: Format[]): Format[] {
  return formats.filter(f => f.vcodec && f.vcodec !== 'none').sort((a,b) =>
    (b.height || 0) - (a.height || 0) ||
    Number(canMp4(b, audioFor(formats,b))) - Number(canMp4(a, audioFor(formats,a))) ||
    (b.fps || 0) - (a.fps || 0) || rank(a) - rank(b) ||
    Number(b.ext === 'mp4') - Number(a.ext === 'mp4') || (b.tbr || 0) - (a.tbr || 0));
}
