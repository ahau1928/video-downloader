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

// Presentation groups do not change the automatic best-format selection.
export function groupFormats(formats: Format[], kind: 'video' | 'audio') {
  const groups = new Map<string, Format[]>();
  for (const f of formats) {
    const name = codecLabel(kind === 'video' ? f.vcodec : f.acodec);
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
export const hasAudio = (f?: Format) => !!f?.acodec && f.acodec !== 'none';
export const isAac = (f?: Format) => !!f && /^(mp4a|aac)/.test(codec(f.acodec)) && /^(m4a|mp4)$/i.test(f.ext || '');
export const mp4Video = (f?: Format) => !!f && /^(av01|av1|avc1|h264)/.test(codec(f.vcodec)) && /^mp4$/i.test(f.ext || '');
export const canMp4 = (v?: Format, a?: Format) => mp4Video(v) && (!a || isAac(a));
function rank(f: Format) {
  const c = codec(f.vcodec);
  return /^(av01|av1)/.test(c) ? 0 : /^(avc1|h264)/.test(c) ? 1 : /^(vp09|vp9)/.test(c) ? 2 : c.startsWith('vp8') ? 3 : 9;
}
export function audioFor(formats: Format[], v?: Format): Format | undefined {
  if (hasAudio(v)) return v;
  const audios = formats.filter(f => (!f.vcodec || f.vcodec === 'none') && hasAudio(f)).sort((a,b) => (b.abr || 0) - (a.abr || 0));
  const preferred = audios.find(f => /^(vp09|vp9)/.test(codec(v?.vcodec || null)) ? codec(f.acodec).startsWith('opus') : isAac(f));
  return preferred || audios.find(isAac) || audios[0];
}
export function orderedVideos(formats: Format[]): Format[] {
  return formats.filter(f => f.vcodec && f.vcodec !== 'none').sort((a,b) =>
    (b.height || 0) - (a.height || 0) ||
    Number(canMp4(b, audioFor(formats,b))) - Number(canMp4(a, audioFor(formats,a))) ||
    (b.fps || 0) - (a.fps || 0) || rank(a) - rank(b) ||
    Number(b.ext === 'mp4') - Number(a.ext === 'mp4') || (b.tbr || 0) - (a.tbr || 0));
}
