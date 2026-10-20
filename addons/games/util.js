/** 秒を「1時間20分」のように */
export function fmt(secs, { short = false } = {}) {
  const m = Math.round(secs / 60);
  if (m < 1) return secs > 0 ? "1分未満" : "0分";
  const h = Math.floor(m / 60), r = m % 60;
  if (!h) return `${r}分`;
  if (short) return r ? `${h}:${String(r).padStart(2, "0")}` : `${h}時間`;
  return r ? `${h}時間${r}分` : `${h}時間`;
}

/** "YYYY-MM-DD" から n 日ずらした日付 */
export function shift(date, n) {
  const [y, m, d] = date.split("-").map(Number);
  const t = new Date(y, m - 1, d + n);
  return `${t.getFullYear()}-${String(t.getMonth() + 1).padStart(2, "0")}-${String(t.getDate()).padStart(2, "0")}`;
}

/** 除外したアプリを除いた、その日の合計 */
export function dayTotal(log, date, hidden) {
  const d = log.days?.[date] ?? {};
  return Object.entries(d).reduce((a, [exe, s]) => a + (hidden.has(exe) ? 0 : s), 0);
}

export async function hiddenSet() {
  const d = (await SideDock.storage.get()) ?? {};
  return new Set(Array.isArray(d.hidden) ? d.hidden : []);
}
