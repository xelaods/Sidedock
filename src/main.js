// SideDock 本体の UI。バーとパネルを描き、アドオン（iframe）と Rust 側の橋渡しをする。

const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;
const appWindow = window.__TAURI__.window.getCurrentWindow();
const webview = window.__TAURI__.webview.getCurrentWebview();

const $ = (sel) => document.querySelector(sel);
const el = (tag, props = {}, ...children) => {
  const node = Object.assign(document.createElement(tag), props);
  for (const c of children) if (c != null) node.append(c);
  return node;
};

const PERMISSION_LABELS = {
  app: "アプリやファイルを開く",
  dialog: "ファイルを選ぶ画面を出す",
  drop: "ドロップされたファイルを受け取る",
  browser: "ブラウザでページを開く",
  volume: "音量を変える",
  network: "インターネットに接続する",
};

const state = {
  platform: "windows",
  config: null,
  addons: new Map(), // id → AddonInfo
  errors: [],
  frames: new Map(), // iframe の contentWindow → { addonId, slot, iframe }
  panelFrames: new Map(), // addonId → iframe
  active: null, // 開いているパネル（addonId か "settings"）
  holdOpen: 0, // ダイアログ表示中などでパネルを閉じない
  dark: false,
};

// --------------------------------------------------------------------- 共通

function addonUrl(id, path) {
  const base = state.platform === "windows" ? "http://addon.localhost" : "addon://localhost";
  return `${base}/${encodeURIComponent(id)}/${path.split("/").map(encodeURIComponent).join("/")}`;
}

function enabledAddons() {
  return state.config.addons
    .filter((a) => a.enabled && state.addons.has(a.id))
    .map((a) => state.addons.get(a.id));
}

function computeDark() {
  const t = state.config.theme;
  return t === "dark" || (t === "system" && matchMedia("(prefers-color-scheme: dark)").matches);
}

function applyTheme() {
  state.dark = computeDark();
  const root = document.documentElement;
  root.dataset.theme = state.dark ? "dark" : "light";
  root.style.colorScheme = state.dark ? "dark" : "light";
  broadcast("theme", { dark: state.dark });
}

function send(frame, msg) {
  frame.iframe.contentWindow?.postMessage({ __sd: 1, ...msg }, "*");
}

function sendEvent(frame, name, data) {
  send(frame, { type: "event", name, data });
}

/** すべての（または特定アドオンの）iframe にイベントを送る */
function broadcast(name, data, { addonId, except, permission } = {}) {
  for (const frame of state.frames.values()) {
    if (addonId && frame.addonId !== addonId) continue;
    if (except && frame === except) continue;
    if (permission && !hasPermission(frame.addonId, permission)) continue;
    sendEvent(frame, name, data);
  }
}

function hasPermission(addonId, perm) {
  return state.addons.get(addonId)?.permissions.includes(perm) ?? false;
}

function createFrame(addon, slot) {
  const entry = slot === "bar" ? addon.bar.entry : addon.panel.entry;
  const iframe = el("iframe", {
    className: slot === "bar" ? "bar-frame" : "panel-frame",
    title: addon.name,
  });
  // allow-same-origin を付けないので、アドオンは本体の API に直接触れない
  iframe.setAttribute("sandbox", "allow-scripts allow-forms");
  iframe.src = `${addonUrl(addon.id, entry)}?slot=${slot}&theme=${state.dark ? "dark" : "light"}`;
  return iframe;
}

function registerFrame(iframe, addonId, slot) {
  state.frames.set(iframe.contentWindow, { addonId, slot, iframe });
}

// --------------------------------------------------------------------- バー

function renderBar() {
  for (const [win, f] of state.frames) if (f.slot === "bar") state.frames.delete(win);
  const items = $("#bar-items");
  items.replaceChildren();

  for (const addon of enabledAddons()) {
    if (addon.bar) {
      const iframe = createFrame(addon, "bar");
      const slot = el("div", { className: "bar-slot" }, iframe);
      slot.dataset.addon = addon.id;
      items.append(slot);
      registerFrame(iframe, addon.id, "bar");
    } else if (addon.panel) {
      const title = addon.panel.title || addon.name;
      const btn = el("button", { className: "bar-icon", title, ariaLabel: title });
      btn.dataset.addon = addon.id;
      btn.setAttribute("aria-pressed", String(state.active === addon.id));
      if (addon.icon) {
        const glyph = el("span", { className: "glyph" });
        glyph.style.setProperty("--src", `url("${addonUrl(addon.id, addon.icon)}")`);
        btn.append(glyph);
      } else {
        btn.append(addon.name.slice(0, 1));
      }
      btn.addEventListener("click", () => togglePanel(addon.id));
      items.append(btn);
    }
  }
  syncPressed();
}

function syncPressed() {
  for (const btn of document.querySelectorAll(".bar-icon, .bar-slot")) {
    const id = btn.id === "btn-settings" ? "settings" : btn.dataset.addon;
    btn.classList.toggle("active", state.active === id);
    if (btn.matches("button")) btn.setAttribute("aria-pressed", String(state.active === id));
  }
}

// --------------------------------------------------------------------- パネル

async function openPanel(id) {
  const wasOpen = state.active !== null;
  const prev = state.active;
  let width = state.config.panelWidth;
  let title;

  if (id === "settings") {
    title = "設定";
    width = Math.max(width, 400);
    await renderSettings();
  } else {
    const addon = state.addons.get(id);
    if (!addon?.panel) return;
    title = addon.panel.title || addon.name;
    width = addon.panel.width || width;
    if (!state.panelFrames.has(id)) {
      // 一度開いたパネルは閉じても残しておき、状態を保つ
      const view = el("div", { className: "view" });
      view.dataset.view = id;
      const iframe = createFrame(addon, "panel");
      view.append(iframe);
      $("#panel-body").append(view);
      registerFrame(iframe, id, "panel");
      state.panelFrames.set(id, iframe);
    }
  }

  state.active = id;
  // 先にウィンドウを広げてから中身を見せる
  await invoke("set_panel", { open: true, width });

  $("#panel-title").textContent = title;
  for (const view of document.querySelectorAll("#panel-body > .view")) {
    view.hidden = (view.dataset.view ?? "settings") !== id;
  }
  const panel = $("#panel");
  panel.hidden = false;
  if (!wasOpen || prev !== id) {
    panel.classList.remove("opening");
    void panel.offsetWidth;
    panel.classList.add("opening");
  }
  syncPressed();

  if (prev && prev !== id && prev !== "settings") notifyPanel(prev, "panelHidden");
  if (id !== "settings") {
    const iframe = state.panelFrames.get(id);
    iframe.focus();
    notifyPanel(id, "panelShown");
  }
}

function notifyPanel(id, name) {
  const iframe = state.panelFrames.get(id);
  const frame = iframe && state.frames.get(iframe.contentWindow);
  if (frame?.ready) sendEvent(frame, name, {});
}

async function closePanel() {
  if (state.active === null) return;
  const prev = state.active;
  state.active = null;
  $("#panel").hidden = true;
  syncPressed();
  if (prev !== "settings") notifyPanel(prev, "panelHidden");
  await invoke("set_panel", { open: false });
}

function togglePanel(id) {
  return state.active === id ? closePanel() : openPanel(id);
}

/** ダイアログなどでフォーカスが外れてもパネルを閉じないようにする */
async function holdingOpen(promise) {
  state.holdOpen++;
  try {
    return await promise;
  } finally {
    // フォーカスが戻るのを少し待ってから解除
    setTimeout(() => state.holdOpen--, 400);
  }
}

// --------------------------------------------------------------------- アドオン API

const API = {
  "ui.setHeight": {
    run: (f, p) => {
      if (f.slot !== "bar") return;
      const h = Math.max(0, Math.min(2000, Number(p.height) || 0));
      f.iframe.style.height = `${h}px`;
    },
  },
  "ui.openPanel": { run: (f) => openPanel(f.addonId) },
  "ui.closePanel": { run: () => closePanel() },

  "storage.get": { run: (f) => invoke("storage_get", { addonId: f.addonId }) },
  "storage.set": {
    run: async (f, p) => {
      await invoke("storage_set", { addonId: f.addonId, data: p.data ?? null });
      // 同じアドオンの別の iframe（バーとパネル）に変更を知らせる
      broadcast("storageChanged", p.data ?? null, { addonId: f.addonId, except: f });
    },
  },

  "app.launch": {
    perm: "app",
    run: (f, p) => invoke("app_launch", { path: String(p.path), args: p.args ?? null }),
  },
  "app.icon": { perm: "app", run: (f, p) => invoke("app_icon", { path: String(p.path) }) },

  "dialog.pickFiles": {
    perm: "dialog",
    run: (f, p) => holdingOpen(invoke("pick_files", { filters: p.filters ?? null })),
  },
  "dialog.pickFolder": { perm: "dialog", run: () => holdingOpen(invoke("pick_folder")) },

  "browser.list": { perm: "browser", run: () => invoke("browser_list") },
  "browser.open": {
    perm: "browser",
    run: (f, p) => invoke("browser_open", { browserId: p.browserId || null, url: String(p.url) }),
  },

  "volume.get": { perm: "volume", run: () => invoke("volume_get") },
  "volume.set": { perm: "volume", run: (f, p) => invoke("volume_set", { level: Number(p.level) }) },
  "volume.setMute": { perm: "volume", run: (f, p) => invoke("volume_set_mute", { muted: !!p.muted }) },
};

window.addEventListener("message", async (e) => {
  const d = e.data;
  if (!d || d.__sd !== 1) return;
  const frame = state.frames.get(e.source);
  if (!frame) return;

  if (d.type === "ready") {
    frame.ready = true;
    const addon = state.addons.get(frame.addonId);
    sendEvent(frame, "init", {
      addonId: frame.addonId,
      slot: frame.slot,
      dark: state.dark,
      edge: state.config.edge,
      permissions: addon?.permissions ?? [],
    });
    // 初めて開いたパネルは読み込みが終わってから「表示された」ことを知らせる
    if (frame.slot === "panel" && state.active === frame.addonId) sendEvent(frame, "panelShown", {});
    return;
  }
  if (d.type !== "call") return;

  const reply = (ok, payload) =>
    send(frame, { type: "result", id: d.id, ok, ...(ok ? { value: payload } : { error: payload }) });
  const api = API[d.method];
  if (!api) return reply(false, `不明な API です: ${d.method}`);
  if (api.perm && !hasPermission(frame.addonId, api.perm)) {
    return reply(false, `権限「${api.perm}」が manifest.json にありません`);
  }
  try {
    reply(true, (await api.run(frame, d.params ?? {})) ?? null);
  } catch (err) {
    reply(false, String(err?.message ?? err));
  }
});

// --------------------------------------------------------------------- 設定画面

function seg(options, value, onPick) {
  const wrap = el("div", { className: "seg", role: "group" });
  for (const [v, label] of options) {
    const b = el("button", { textContent: label });
    b.setAttribute("aria-pressed", String(v === value));
    b.addEventListener("click", () => onPick(v));
    wrap.append(b);
  }
  return wrap;
}

function toggle(checked, label, onChange) {
  const input = el("input", { type: "checkbox", checked, ariaLabel: label });
  input.setAttribute("role", "switch");
  input.addEventListener("change", () => onChange(input.checked));
  return el("label", { className: "switch" }, input, el("span"));
}

function row(title, sub, control) {
  return el(
    "div",
    { className: "row" },
    el("div", { className: "row-main" },
      el("div", { className: "row-title", textContent: title }),
      sub ? el("div", { className: "row-sub", textContent: sub }) : null),
    control,
  );
}

function arrow(up) {
  const svg = `<svg viewBox="0 0 10 10" aria-hidden="true"><path d="${up ? "M2 6.5L5 3.5L8 6.5" : "M2 3.5L5 6.5L8 3.5"}" stroke="currentColor" stroke-width="1.3" fill="none" stroke-linecap="round" stroke-linejoin="round"/></svg>`;
  const b = el("button", { className: "sd-btn ghost", title: up ? "上へ" : "下へ", ariaLabel: up ? "上へ移動" : "下へ移動" });
  b.innerHTML = svg;
  return b;
}

async function saveConfig(patch) {
  const next = { ...state.config, ...patch };
  state.config = await invoke("save_config", { config: next });
}

async function renderSettings() {
  const root = $("#settings");
  const scroll = root.scrollTop;
  const c = state.config;
  const autostart = await invoke("autostart_get").catch(() => false);

  const layout = el("div", { className: "group" },
    row("表示する位置", null,
      seg([["left", "左"], ["right", "右"]], c.edge, async (v) => {
        await saveConfig({ edge: v });
        applyEdge();
        renderSettings();
      })),
    row("画面の領域を確保する", "最大化したウィンドウがバーと重ならなくなります",
      toggle(c.reserveSpace, "画面の領域を確保する", (v) => saveConfig({ reserveSpace: v }))),
    row("テーマ", null,
      seg([["system", "システム"], ["light", "ライト"], ["dark", "ダーク"]], c.theme, async (v) => {
        await saveConfig({ theme: v });
        applyTheme();
        renderSettings();
      })),
    row("Windows の起動時に開く", null,
      toggle(autostart, "Windows の起動時に開く", (v) => invoke("autostart_set", { enabled: v }))),
  );

  const list = el("div", { className: "group" });
  const entries = c.addons.filter((a) => state.addons.has(a.id));
  entries.forEach((entry, i) => {
    const addon = state.addons.get(entry.id);
    const glyph = el("span", { className: "glyph" });
    if (addon.icon) glyph.style.setProperty("--src", `url("${addonUrl(addon.id, addon.icon)}")`);

    const perms = addon.permissions.map((p) => PERMISSION_LABELS[p] ?? p).join("、");
    const up = arrow(true);
    const down = arrow(false);
    up.disabled = i === 0;
    down.disabled = i === entries.length - 1;
    const move = async (delta) => {
      const order = [...c.addons];
      const from = order.findIndex((a) => a.id === entry.id);
      const to = from + delta;
      if (to < 0 || to >= order.length) return;
      [order[from], order[to]] = [order[to], order[from]];
      await saveConfig({ addons: order });
      renderBar();
      renderSettings();
    };
    up.addEventListener("click", () => move(-1));
    down.addEventListener("click", () => move(1));

    const sw = toggle(entry.enabled, `${addon.name}を使う`, async (v) => {
      if (v && !addon.builtin && addon.permissions.length) {
        const names = addon.permissions.map((p) => `・${PERMISSION_LABELS[p] ?? p}`).join("\n");
        state.holdOpen++;
        const ok = confirm(`「${addon.name}」に次のことを許可しますか？\n\n${names}`);
        setTimeout(() => state.holdOpen--, 400);
        if (!ok) {
          sw.querySelector("input").checked = false;
          return;
        }
      }
      await saveConfig({ addons: c.addons.map((a) => (a.id === entry.id ? { ...a, enabled: v } : a)) });
      renderBar();
      renderSettings();
    });

    const title = el("div", { className: "row-title", textContent: addon.name });
    if (addon.version) title.append(el("span", { className: "ver", textContent: `${addon.version}${addon.builtin ? "" : "・自分で追加"}` }));
    const r = el("div", { className: `row addon${entry.enabled ? "" : " off"}` },
      glyph,
      el("div", { className: "row-main" },
        title,
        addon.description ? el("div", { className: "row-sub", textContent: addon.description }) : null,
        perms ? el("div", { className: "perms", textContent: `権限：${perms}` }) : null),
      el("div", { className: "move" }, up, down),
      sw,
    );
    list.append(r);
  });

  const openFolder = el("button", { className: "sd-btn", textContent: "アドオンフォルダを開く" });
  openFolder.addEventListener("click", () => invoke("open_addons_folder"));
  const reloadBtn = el("button", { className: "sd-btn", textContent: "アドオンを読み直す" });
  reloadBtn.addEventListener("click", () => location.reload());
  const quitBtn = el("button", { className: "sd-btn", textContent: "SideDock を終了" });
  quitBtn.addEventListener("click", () => invoke("quit"));

  root.replaceChildren(
    el("h2", { textContent: "表示" }),
    layout,
    el("h2", { textContent: "アドオン" }),
    entries.length ? list : el("div", { className: "sd-empty", textContent: "アドオンがありません" }),
    el("div", { className: "actions" }, openFolder, reloadBtn),
    state.errors.length
      ? el("div", { className: "notice", textContent: `読み込めなかったアドオン:\n${state.errors.join("\n")}` })
      : null,
    el("div", { className: "actions" }, quitBtn),
    el("p", {
      className: "foot-note",
      textContent: "アドオンフォルダに manifest.json を含むフォルダを置き、「アドオンを読み直す」を押すと一覧に出てきます。",
    }),
  );
  root.scrollTop = scroll;
}

function applyEdge() {
  document.body.dataset.edge = state.config.edge;
  document.documentElement.style.setProperty("--bar-w", `${state.config.barWidth}px`);
}

// --------------------------------------------------------------------- 起動

async function start() {
  const data = await invoke("init");
  state.platform = data.platform;
  state.config = data.config;
  state.errors = data.errors;
  state.addons = new Map(data.addons.map((a) => [a.id, a]));

  applyEdge();
  applyTheme();
  renderBar();

  // 設定ビューは常にパネルの中にある
  $("#settings").dataset.view = "settings";

  $("#btn-settings").addEventListener("click", () => togglePanel("settings"));
  $("#panel-close").addEventListener("click", () => closePanel());

  matchMedia("(prefers-color-scheme: dark)").addEventListener("change", applyTheme);

  // Esc でパネルを閉じる（アドオンの iframe 内では各アドオンが ui.closePanel を呼ぶ）
  document.addEventListener("keydown", (e) => {
    if (e.key === "Escape") closePanel();
  });

  // 他のウィンドウをクリックしたらパネルを閉じる
  await appWindow.onFocusChanged(({ payload: focused }) => {
    if (!focused && state.holdOpen === 0) closePanel();
  });

  // バーにファイルをドロップ → drop 権限のあるアドオンに渡す
  await webview.onDragDropEvent(({ payload }) => {
    if (payload.type === "drop" && payload.paths?.length) {
      broadcast("drop", { paths: payload.paths }, { permission: "drop" });
    }
  });

  await listen("open-settings", () => openPanel("settings"));
}

start().catch((err) => {
  document.body.textContent = `起動に失敗しました: ${err}`;
});
