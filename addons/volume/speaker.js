// 音量に合わせたスピーカーのアイコン（SVG 文字列）
export function speaker(level, muted, size = 20) {
  const waves = muted
    ? '<path d="M15.5 9.5l5 5M20.5 9.5l-5 5"/>'
    : level === 0
      ? ""
      : level < 0.34
        ? '<path d="M15 10a2.8 2.8 0 0 1 0 4"/>'
        : level < 0.67
          ? '<path d="M15 9a4 4 0 0 1 0 6"/>'
          : '<path d="M15 9a4 4 0 0 1 0 6M17.5 6.5a7.5 7.5 0 0 1 0 11"/>';
  return `<svg viewBox="0 0 24 24" width="${size}" height="${size}" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linejoin="round" stroke-linecap="round" aria-hidden="true"><path d="M4 9.5h3l4.5-4v13L7 14.5H4z"/>${waves}</svg>`;
}
