// タイマーの状態は storage に置き、バーとパネルで共有する
//   { endsAt: 終わる時刻(ms) | null, duration: 合計(ms), left: 一時停止中の残り(ms) | null, label }

export const EMPTY = { endsAt: null, duration: 0, left: null, label: "" };

export function remaining(t, now = Date.now()) {
  if (t.left != null) return t.left;
  if (t.endsAt == null) return 0;
  return Math.max(0, t.endsAt - now);
}

export function running(t) {
  return t.endsAt != null || t.left != null;
}

export function fmt(ms) {
  const s = Math.ceil(ms / 1000);
  const h = Math.floor(s / 3600), m = Math.floor((s % 3600) / 60), sec = s % 60;
  return h ? `${h}:${String(m).padStart(2, "0")}:${String(sec).padStart(2, "0")}` : `${m}:${String(sec).padStart(2, "0")}`;
}

export async function load() {
  return { ...EMPTY, ...((await SideDock.storage.get()) ?? {}) };
}
