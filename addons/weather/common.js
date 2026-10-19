// 天気の取得と、天気コード（WMO）の読み替え。データは storage に置いてバーとパネルで共有する
//   { place: { name, lat, lon } | null, cache: { at, data } | null }

const PATHS = {
  sun: '<circle cx="12" cy="12" r="4"/><path d="M12 2.5v2M12 19.5v2M4.6 4.6l1.4 1.4M18 18l1.4 1.4M2.5 12h2M19.5 12h2M4.6 19.4L6 18M18 6l1.4-1.4"/>',
  moon: '<path d="M19.5 14.5A7.5 7.5 0 0 1 9.5 4.5a7.5 7.5 0 1 0 10 10z"/>',
  partly: '<path d="M8.5 5.5a3.5 3.5 0 0 1 5.9 1.6"/><path d="M5 8.5H3.5M6.2 4.2l-1-1M10.5 2.5v-1"/><path d="M8 19h9a3.5 3.5 0 0 0 .4-7A5 5 0 0 0 7.8 12.4 3.3 3.3 0 0 0 8 19z"/>',
  cloud: '<path d="M7 18h10a4 4 0 0 0 .5-7.97A6 6 0 0 0 6.1 11 3.5 3.5 0 0 0 7 18z"/>',
  fog: '<path d="M4 9h16M3 13h18M5 17h14"/>',
  rain: '<path d="M7 14h10a4 4 0 0 0 .5-7.97A6 6 0 0 0 6.1 7 3.5 3.5 0 0 0 7 14z"/><path d="M9 17l-1 3M13 17l-1 3M17 17l-1 3"/>',
  snow: '<path d="M7 14h10a4 4 0 0 0 .5-7.97A6 6 0 0 0 6.1 7 3.5 3.5 0 0 0 7 14z"/><path d="M9 18h.01M13 19h.01M17 18h.01M11 21h.01M15 21h.01"/>',
  storm: '<path d="M7 14h10a4 4 0 0 0 .5-7.97A6 6 0 0 0 6.1 7 3.5 3.5 0 0 0 7 14z"/><path d="M12.5 15l-2 3.5h3l-2 3.5"/>',
};

export function weather(code, day = true) {
  const c = Number(code);
  if (c === 0) return { kind: day ? "sun" : "moon", label: "快晴" };
  if (c === 1) return { kind: day ? "sun" : "moon", label: "晴れ" };
  if (c === 2) return { kind: "partly", label: "晴れ時々くもり" };
  if (c === 3) return { kind: "cloud", label: "くもり" };
  if (c === 45 || c === 48) return { kind: "fog", label: "霧" };
  if (c >= 51 && c <= 57) return { kind: "rain", label: "霧雨" };
  if ((c >= 61 && c <= 67) || (c >= 80 && c <= 82)) return { kind: "rain", label: c >= 80 ? "にわか雨" : c >= 65 ? "強い雨" : "雨" };
  if ((c >= 71 && c <= 77) || c === 85 || c === 86) return { kind: "snow", label: "雪" };
  if (c >= 95) return { kind: "storm", label: "雷雨" };
  return { kind: "cloud", label: "くもり" };
}

export function icon(kind, size = 18) {
  return `<svg viewBox="0 0 24 24" width="${size}" height="${size}" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">${PATHS[kind] ?? PATHS.cloud}</svg>`;
}

const FRESH = 15 * 60_000;

export async function load() {
  return { place: null, cache: null, ...((await SideDock.storage.get()) ?? {}) };
}

/** 古ければ取り直す。新しいデータを返す（場所が未設定なら null） */
export async function refresh(state, force = false) {
  if (!state.place) return null;
  if (!force && state.cache && Date.now() - state.cache.at < FRESH && state.cache.lat === state.place.lat) return state.cache.data;
  const { lat, lon } = state.place;
  const url = `https://api.open-meteo.com/v1/forecast?latitude=${lat}&longitude=${lon}` +
    "&current=temperature_2m,apparent_temperature,relative_humidity_2m,weather_code,wind_speed_10m,is_day" +
    "&hourly=temperature_2m,weather_code,precipitation_probability,is_day" +
    "&daily=weather_code,temperature_2m_max,temperature_2m_min,precipitation_probability_max" +
    "&timezone=auto&forecast_days=6";
  const res = await fetch(url);
  if (!res.ok) throw new Error(`天気を取得できませんでした（${res.status}）`);
  const data = await res.json();
  state.cache = { at: Date.now(), lat, data };
  await SideDock.storage.set(state);
  return data;
}

export async function searchPlace(q) {
  const url = `https://geocoding-api.open-meteo.com/v1/search?name=${encodeURIComponent(q)}&count=6&language=ja&format=json`;
  const res = await fetch(url);
  if (!res.ok) throw new Error("地名を検索できませんでした");
  const json = await res.json();
  return (json.results ?? []).map((r) => ({
    name: r.name,
    detail: [r.admin1, r.country].filter(Boolean).join("・"),
    lat: Math.round(r.latitude * 100) / 100,
    lon: Math.round(r.longitude * 100) / 100,
  }));
}
