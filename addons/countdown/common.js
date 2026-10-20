// 日付の計算（その日の 0 時どうしで比べる）
export function daysUntil(dateStr) {
  const [y, m, d] = dateStr.split("-").map(Number);
  const target = new Date(y, m - 1, d);
  const today = new Date(); today.setHours(0, 0, 0, 0);
  return Math.round((target - today) / 86400000);
}
export function label(n) {
  return n === 0 ? "今日" : n === 1 ? "明日" : n > 0 ? `あと ${n} 日` : `${-n} 日前`;
}
/** これからのもの（今日を含む）を近い順に */
export function upcoming(events) {
  return events
    .map((e) => ({ ...e, days: daysUntil(e.date) }))
    .filter((e) => e.days >= 0)
    .sort((a, b) => a.days - b.days);
}
