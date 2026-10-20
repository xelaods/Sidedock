// 置き場の中身：{ items: [{ id, kind: "file" | "text", path, name, isDir, text, at }], removeAfter }
export const MAX_ITEMS = 60;

export const baseName = (p) => p.replace(/[\\/]+$/, "").split(/[\\/]/).pop() || p;

export async function load() {
  const d = (await SideDock.storage.get()) ?? {};
  return { items: Array.isArray(d.items) ? d.items : [], removeAfter: d.removeAfter !== false };
}

export async function save(d) {
  await SideDock.storage.set({ items: d.items.slice(0, MAX_ITEMS), removeAfter: d.removeAfter });
}

const uid = () => Date.now().toString(36) + Math.random().toString(36).slice(2, 7);

/** ファイルを預ける。同じものは先頭へ移すだけ。新しく入った数を返す */
export async function addPaths(paths) {
  const d = await load();
  let added = 0;
  for (const path of paths) {
    const i = d.items.findIndex((x) => x.kind === "file" && x.path.toLowerCase() === path.toLowerCase());
    const item = i >= 0 ? d.items.splice(i, 1)[0] : (added++, { id: uid(), kind: "file", path, name: baseName(path) });
    item.at = Date.now();
    d.items.unshift(item);
  }
  await save(d);
  return added;
}

export async function addText(text) {
  const d = await load();
  d.items = d.items.filter((x) => !(x.kind === "text" && x.text === text));
  d.items.unshift({ id: uid(), kind: "text", text, at: Date.now() });
  await save(d);
}
