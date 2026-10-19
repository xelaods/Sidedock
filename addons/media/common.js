// バーとパネルで共有する処理

/** 再生しているアプリの ID から、分かりやすい名前を作る */
export function appName(id = "") {
  const s = id.toLowerCase();
  const known = [
    ["spotify", "Spotify"], ["applemusic", "Apple Music"], ["zunemusic", "メディア プレーヤー"],
    ["chrome", "Chrome"], ["msedge", "Edge"], ["firefox", "Firefox"], ["dia", "Dia"],
    ["brave", "Brave"], ["opera", "Opera"], ["vivaldi", "Vivaldi"], ["arc", "Arc"],
    ["amazonmusic", "Amazon Music"], ["youtube", "YouTube Music"], ["vlc", "VLC"], ["foobar", "foobar2000"],
    ["musicbee", "MusicBee"], ["itunes", "iTunes"], ["discord", "Discord"], ["kanade", "Kanade"],
  ];
  for (const [k, v] of known) if (s.includes(k)) return v;
  const base = id.split("!")[0].split("\\").pop().replace(/\.exe$/i, "").split("_")[0];
  return base.split(".").pop() || "アプリ";
}

export function time(sec) {
  if (!isFinite(sec) || sec <= 0) return "0:00";
  sec = Math.floor(sec);
  const h = Math.floor(sec / 3600), m = Math.floor((sec % 3600) / 60), s = sec % 60;
  return h ? `${h}:${String(m).padStart(2, "0")}:${String(s).padStart(2, "0")}` : `${m}:${String(s).padStart(2, "0")}`;
}

export const ICONS = {
  play: '<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M8 5.5v13a1 1 0 0 0 1.5.86l10.5-6.5a1 1 0 0 0 0-1.72L9.5 4.64A1 1 0 0 0 8 5.5z" fill="currentColor"/></svg>',
  pause: '<svg viewBox="0 0 24 24" aria-hidden="true"><rect x="6.5" y="5" width="4" height="14" rx="1.2" fill="currentColor"/><rect x="13.5" y="5" width="4" height="14" rx="1.2" fill="currentColor"/></svg>',
  next: '<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M5 6.2v11.6a.8.8 0 0 0 1.2.7L15 13v4.5a1 1 0 0 0 2 0v-11a1 1 0 0 0-2 0V11L6.2 5.5A.8.8 0 0 0 5 6.2z" fill="currentColor"/></svg>',
  prev: '<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M19 6.2v11.6a.8.8 0 0 1-1.2.7L9 13v4.5a1 1 0 0 1-2 0v-11a1 1 0 0 1 2 0V11l8.8-5.5a.8.8 0 0 1 1.2.7z" fill="currentColor"/></svg>',
  note: '<svg viewBox="0 0 24 24" aria-hidden="true" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"><path d="M9 18V6l10-2v12"/><circle cx="6.5" cy="18" r="2.5"/><circle cx="16.5" cy="16" r="2.5"/></svg>',
};

/** 再生中のメディアを見張る。変わるたびに onChange(info, art) を呼ぶ */
export function watch(onChange, interval = 1000) {
  let artKey = null, art = null, timer = 0, stopped = false;
  async function tick() {
    try {
      const info = await SideDock.media.get();
      if (info && info.artKey !== artKey) {
        artKey = info.artKey;
        art = await SideDock.media.art().catch(() => null);
      }
      if (!info) { artKey = null; art = null; }
      onChange(info, art);
    } catch {
      onChange(null, null);
    }
    if (!stopped) timer = setTimeout(tick, interval);
  }
  tick();
  return {
    stop() { stopped = true; clearTimeout(timer); },
    now() { clearTimeout(timer); tick(); },
  };
}
