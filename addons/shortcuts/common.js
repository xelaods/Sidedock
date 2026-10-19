// バーとパネルで共有する処理
export async function load() {
  const data = await SideDock.storage.get();
  return Array.isArray(data?.items) ? data.items : [];
}

export async function save(items) {
  await SideDock.storage.set({ items });
}

export function nameOf(path) {
  const base = path.replace(/[\\/]+$/, "").split(/[\\/]/).pop() || path;
  // フォルダ名に「.」が入っていることもあるので、よくある拡張子だけ外す
  return base.replace(/\.(lnk|exe|url|bat|cmd|appref-ms)$/i, "");
}

export async function addPaths(items, paths) {
  const next = [...items];
  for (const path of paths) {
    if (next.some((i) => i.path.toLowerCase() === path.toLowerCase())) continue;
    const icon = await SideDock.app.icon(path).catch(() => null);
    next.push({ id: Date.now().toString(36) + Math.random().toString(36).slice(2, 8), path, name: nameOf(path), icon });
  }
  return next;
}
