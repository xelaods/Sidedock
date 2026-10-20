// トレイアイコンのボタンを作る（バーとパネルで共有）

export function trayButton(t, { size = 18 } = {}) {
  const b = document.createElement("button");
  b.className = "sd-bar-btn ti";
  b.dataset.key = t.key;
  update(b, t, size);
  b.addEventListener("click", () => SideDock.tray.click(b.dataset.key, "left").catch(() => {}));
  b.addEventListener("dblclick", () => SideDock.tray.click(b.dataset.key, "double").catch(() => {}));
  b.addEventListener("contextmenu", (e) => {
    e.preventDefault();
    SideDock.tray.click(b.dataset.key, "right").catch(() => {});
  });
  return b;
}

export function update(b, t, size = 18) {
  const tip = (t.tip || t.exe.split("\\").pop().replace(/\.exe$/i, "") || "アイコン").split("\n")[0];
  b.dataset.hint = tip;
  b.setAttribute("aria-label", tip);
  if (b._icon === t.icon) return;
  b._icon = t.icon;
  b.innerHTML = "";
  if (t.icon) {
    const img = document.createElement("img");
    img.src = t.icon;
    img.alt = "";
    img.width = size; img.height = size;
    img.style.width = img.style.height = `${size}px`;
    b.append(img);
  } else {
    const s = document.createElement("span");
    s.className = "ti-letter";
    s.textContent = tip.slice(0, 1);
    b.append(s);
  }
}
