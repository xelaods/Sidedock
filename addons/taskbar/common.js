// 開いているウィンドウをアプリ（実行ファイル）ごとにまとめ、アイコンを覚えておく

const icons = new Map(); // key → data URL | null（取得中は "pending"）

export const keyOf = (w) => (w.exe || `#${w.id}`).toLowerCase();

export function appName(w) {
  const base = (w.exe || "").split("\\").pop().replace(/\.exe$/i, "");
  return base || w.title;
}

/** ウィンドウの一覧をアプリごとにまとめる（並び順は最初に見つかった順を保つ） */
export function group(list, order) {
  const groups = new Map();
  for (const w of list) {
    const k = keyOf(w);
    if (!groups.has(k)) groups.set(k, { key: k, exe: w.exe, windows: [] });
    groups.get(k).windows.push(w);
  }
  // 前回の並び順を守る（アプリを切り替えるたびに順番が変わらないように）
  for (const k of groups.keys()) if (!order.includes(k)) order.push(k);
  for (let i = order.length - 1; i >= 0; i--) if (!groups.has(order[i])) order.splice(i, 1);
  return order.map((k) => groups.get(k));
}

/** アイコンを取ってくる。取れたら onReady を呼ぶ */
export function iconFor(g, onReady) {
  const k = g.key;
  if (icons.has(k)) return icons.get(k) === "pending" ? null : icons.get(k);
  icons.set(k, "pending");
  const w = g.windows[0];
  SideDock.windows.icon(w.id, w.exe)
    .then((url) => { icons.set(k, url); onReady(); })
    .catch(() => { icons.set(k, null); onReady(); });
  return null;
}
