// 状態に合わせたアイコン（SVG 文字列）

const svg = (inner, size = 16) =>
  `<svg viewBox="0 0 24 24" width="${size}" height="${size}" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">${inner}</svg>`;

/** Wi-Fi は電波の強さで弧の数を変える。つながっていない弧は薄く */
export function netIcon(n, size = 16) {
  if (!n || n.kind === "none") {
    return svg('<path d="M3 9a13.5 13.5 0 0 1 18 0" opacity=".3"/><path d="M6 12.5a9 9 0 0 1 12 0" opacity=".3"/><path d="M4 4l16 16"/>', size);
  }
  if (n.kind === "ethernet") {
    return svg('<rect x="3.5" y="4" width="17" height="11" rx="2"/><path d="M8 19h8M12 15v4"/>', size);
  }
  const bars = n.bars ?? 4;
  const on = (k) => (bars >= k ? "1" : ".28");
  return svg(
    `<path d="M3 9a13.5 13.5 0 0 1 18 0" opacity="${on(4)}"/><path d="M6 12.5a9 9 0 0 1 12 0" opacity="${on(3)}"/><path d="M9 16a4.5 4.5 0 0 1 6 0" opacity="${on(2)}"/><circle cx="12" cy="19" r=".9" fill="currentColor"/>` +
      (n.internet ? "" : '<path d="M19 15v3M19 20.5v.01" stroke="#f5b14c"/>'),
    size,
  );
}

export function volIcon(v, size = 16) {
  const base = '<path d="M4 9.5h3l4.5-4v13L7 14.5H4z"/>';
  if (!v) return svg(base, size);
  if (v.muted || v.level === 0) return svg(base + '<path d="M15.5 9.5l5 5M20.5 9.5l-5 5"/>', size);
  if (v.level < 0.34) return svg(base + '<path d="M15 10a2.8 2.8 0 0 1 0 4"/>', size);
  if (v.level < 0.67) return svg(base + '<path d="M15 9a4 4 0 0 1 0 6"/>', size);
  return svg(base + '<path d="M15 9a4 4 0 0 1 0 6M17.5 6.5a7.5 7.5 0 0 1 0 11"/>', size);
}

/** 電池は残量の分だけ中を塗る */
export function batIcon(b, size = 16) {
  if (!b) return "";
  const w = Math.max(1, Math.round((b.level / 100) * 12));
  const color = b.charging ? "var(--sd-accent)" : b.level <= 15 ? "#ff6b6b" : b.level <= 30 ? "#f5b14c" : "currentColor";
  return svg(
    `<rect x="3" y="7.5" width="16" height="9" rx="2.2"/><path d="M21 10.5v3"/><rect x="5" y="9.5" width="${w}" height="5" rx="1" fill="${color}" stroke="none"/>` +
      (b.charging ? '<path d="M12.5 8.5l-2 3.5h2.5l-2 3.5" stroke="var(--sd-surface-2)" stroke-width="1.4"/>' : ""),
    size,
  );
}
