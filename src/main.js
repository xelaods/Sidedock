// SideDock 本体の UI。
// 画面上部のカプセルとパネルを描き、カーソルの近さに合わせて出し入れし、
// アドオン（iframe）と Rust 側の橋渡しをする。

const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;
const webview = window.__TAURI__.webview.getCurrentWebview();

const $ = (sel) => document.querySelector(sel);
const el = (tag, props = {}, ...children) => {
  const node = Object.assign(document.createElement(tag), props);
  for (const c of children) if (c != null && c !== false) node.append(c);
  return node;
};
const clamp = (v, lo, hi) => Math.min(hi, Math.max(lo, v));

// 近づいたときの反応。h：上端から何 px で反応するか、spread：カプセルの左右に広げる幅、dwell：とどまる時間
const TRIGGERS = {
  narrow: { h: 3, spread: 0, dwell: 150 },
  normal: { h: 10, spread: 56, dwell: 70 },
  wide: { h: 30, spread: 160, dwell: 20 },
};
const HIDE_DELAY = 420; // カーソルが離れてから引っ込むまで
const HOVER_OPEN = 230; // 項目にとどまってパネルが開くまで
const HOVER_SWITCH = 80; // パネルが開いている間に隣へ移ったとき
const PANEL_TOP = 58;
const PANEL_MAX_H = 380;
const SIDE_MARGIN = 16;
const LEAVE_PAD = 22; // カプセル・パネルの外側にこれだけ余裕を持たせる
const DEFAULT_PANEL_W = 300;
const SETTINGS_W = 340;

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
  frames: new Map(), // iframe の contentWindow → { addonId, slot, iframe, ready }
  barSlots: new Map(), // addonId → バーの要素（アイコンかウィジェット）
  views: new Map(), // addonId → { view, iframe }
  heights: new Map(), // addonId → パネルの中身の高さ
  phase: "hidden", // "hidden" | "shown"
  suspended: false,
  pinned: false, // トレイなどから開いたとき：一度カーソルが入るまで引っ込めない
  holdOpen: 0, // ファイル選択の画面などを出している間は引っ込めない
  active: null, // 開いているパネル（addonId か "settings"）
  hover: null, // カーソルの下にあるバーの項目
  panelRect: null,
  cursor: null,
  dark: true,
  timers: { dwell: 0, hide: 0, hover: 0, settle: 0 },
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

function hasPermission(addonId, perm) {
  return state.addons.get(addonId)?.permissions.includes(perm) ?? false;
}

function send(frame, msg) {
  frame.iframe.contentWindow?.postMessage({ __sd: 1, ...msg }, "*");
}
function sendEvent(frame, name, data) {
  if (frame.ready) send(frame, { type: "event", name, data });
}
/** iframe にイベントを送る（絞り込み：addonId / except / permission） */
function broadcast(name, data, { addonId, except, permission } = {}) {
  for (const frame of state.frames.values()) {
    if (addonId && frame.addonId !== addonId) continue;
    if (except && frame === except) continue;
    if (permission && !hasPermission(frame.addonId, permission)) continue;
    sendEvent(frame, name, data);
  }
}

function clearTimer(name) {
  clearTimeout(state.timers[name]);
  state.timers[name] = 0;
}

function trigger() {
  return TRIGGERS[state.config.trigger] ?? TRIGGERS.normal;
}

// --------------------------------------------------------------------- 配色

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

// --------------------------------------------------------------------- 位置と当たり判定

const capsule = () => $("#capsule");

/** カプセルの位置（変形の影響を受けない、最終的な位置） */
function capsuleRect() {
  const dock = $("#dock");
  const c = capsule();
  return { x: dock.offsetLeft + c.offsetLeft, y: dock.offsetTop + c.offsetTop, w: c.offsetWidth, h: c.offsetHeight };
}

function placeDock() {
  const w = capsule().offsetWidth;
  const pos = state.config.position;
  const x =
    pos === "left" ? SIDE_MARGIN : pos === "right" ? innerWidth - SIDE_MARGIN - w : Math.round((innerWidth - w) / 2);
  document.documentElement.style.setProperty("--dock-x", `${x}px`);
  syncHitRects();
}

function grow(r, pad) {
  return { x: r.x - pad, y: r.y - pad, w: r.w + pad * 2, h: r.h + pad * 2 };
}
function inside(p, r) {
  return p.x >= r.x && p.x <= r.x + r.w && p.y >= r.y && p.y <= r.y + r.h;
}

/** カーソルがカプセルかパネルの近くにあるか */
function nearInteractive(p, pad) {
  if (!p) return false;
  const c = capsuleRect();
  // カプセルの上は画面の端まで含める（上端に押し付けたままでも引っ込まない）
  const top = { x: c.x - pad, y: -50, w: c.w + pad * 2, h: c.y + c.h + pad + 50 };
  if (inside(p, top)) return true;
  if (state.panelRect && inside(p, grow(state.panelRect, pad))) return true;
  // カプセルとパネルの間の隙間
  if (state.panelRect) {
    const r = state.panelRect;
    const gap = { x: Math.min(c.x, r.x), y: c.y + c.h, w: Math.max(c.x + c.w, r.x + r.w) - Math.min(c.x, r.x), h: r.y - (c.y + c.h) };
    if (inside(p, gap)) return true;
  }
  return false;
}

/** クリックを受け取る範囲を Rust 側に伝える */
function syncHitRects() {
  const rects = [];
  if (state.phase === "shown" && !state.suspended) {
    const c = capsuleRect();
    // 上端に押し付けたままでもクリックできるよう、カプセルの上は画面の端まで
    rects.push({ x: c.x - 4, y: 0, w: c.w + 8, h: c.y + c.h + 4 });
    if (state.panelRect) rects.push(grow(state.panelRect, 2));
  }
  invoke("set_hit_rects", { rects }).catch(() => {});
}

// --------------------------------------------------------------------- 出し入れ

function reveal({ pinned = false } = {}) {
  if (state.suspended) return;
  if (pinned) state.pinned = true;
  clearTimer("hide");
  if (state.phase === "shown") return;
  state.phase = "shown";
  document.body.classList.remove("settled");
  document.body.classList.add("is-shown");
  clearTimer("settle");
  // 中身が出そろったら遅延をなくす
  state.timers.settle = setTimeout(() => document.body.classList.add("settled"), 80 + 28 * 12 + 420);
  syncHitRects();
  broadcast("revealed", {});
}

function conceal() {
  if (state.phase !== "shown" || state.holdOpen > 0) return;
  closePanel();
  hideHint();
  state.phase = "hidden";
  state.pinned = false;
  clearTimer("settle");
  setHover(null);
  document.body.classList.remove("is-shown", "settled");
  $("#spot").style.setProperty("--spot-o", "0");
  syncHitRects();
  invoke("release_focus").catch(() => {});
  broadcast("concealed", {});
}

function scheduleHide() {
  if (state.timers.hide || state.pinned || state.holdOpen > 0) return;
  state.timers.hide = setTimeout(() => {
    state.timers.hide = 0;
    if (!nearInteractive(state.cursor, LEAVE_PAD)) conceal();
  }, HIDE_DELAY);
}

function inTriggerZone(p) {
  const t = trigger();
  const c = capsuleRect();
  return p.y <= t.h && p.y >= -4 && p.x >= c.x - t.spread && p.x <= c.x + c.w + t.spread;
}

/** 隠れている間：上端の印をカーソルの近さで光らせる */
function updateProximity(p) {
  let prox = 0;
  if (p) {
    const c = capsuleRect();
    const dx = Math.max(0, Math.abs(p.x - (c.x + c.w / 2)) - c.w / 2);
    const dy = Math.max(0, p.y);
    prox = clamp(1 - Math.hypot(dx, dy * 1.6) / 180, 0, 1);
    prox = prox * prox; // 近づくほど急に明るく
  }
  $("#handle").style.setProperty("--prox", prox.toFixed(3));
}

/** 出ている間：カーソルの位置に柔らかい光を当てる */
function updateSpot(p) {
  const spot = $("#spot");
  if (!p || state.phase !== "shown") return spot.style.setProperty("--spot-o", "0");
  const c = capsuleRect();
  const onBar = p.y <= c.y + c.h + 10 && p.x >= c.x - 30 && p.x <= c.x + c.w + 30;
  spot.style.setProperty("--mx", `${p.x - c.x}px`);
  spot.style.setProperty("--spot-o", onBar ? "1" : "0");
}

/** Rust 側から約 60 回/秒で届くカーソルの位置（離れたら null） */
function onCursor(p) {
  state.cursor = p;
  if (state.suspended) return;
  if (state.phase === "hidden") {
    updateProximity(p);
    if (p && inTriggerZone(p)) {
      if (!state.timers.dwell) {
        state.timers.dwell = setTimeout(() => {
          state.timers.dwell = 0;
          if (state.cursor && inTriggerZone(state.cursor)) reveal();
        }, trigger().dwell);
      }
    } else {
      clearTimer("dwell");
    }
    return;
  }
  updateSpot(p);
  setHover(itemAt(p));
  if (nearInteractive(p, LEAVE_PAD)) {
    state.pinned = false;
    clearTimer("hide");
  } else {
    scheduleHide();
  }
}

function onSuspend(on) {
  state.suspended = on;
  document.body.classList.toggle("is-suspended", on);
  if (on) {
    state.holdOpen = 0;
    conceal();
    updateProximity(null);
  }
  syncHitRects();
}

// --------------------------------------------------------------------- ヒント

function showHint(text, x) {
  const hint = $("#hint");
  if (!text || state.active) return hideHint();
  const wasShown = hint.classList.contains("show");
  hint.textContent = text;
  hint.style.setProperty("--hx", `${Math.round(x)}px`);
  hint.classList.toggle("moving", wasShown);
  hint.classList.add("show");
}
function hideHint() {
  $("#hint").classList.remove("show", "moving");
}

/** バーの要素の中心（ウィンドウ座標） */
function centerOf(node) {
  const r = node.getBoundingClientRect();
  return r.left + r.width / 2;
}

// --------------------------------------------------------------------- バー

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
  state.frames.set(iframe.contentWindow, { addonId, slot, iframe, ready: false });
}

function panelTrigger(addon) {
  return addon.panel?.trigger === "manual" ? "manual" : "hover";
}

/**
 * カーソルの下にある項目。DOM のホバーは iframe をまたぐと取りこぼすので、
 * Rust 側から届くカーソルの位置で判定する
 */
function itemAt(p) {
  if (!p) return null;
  const c = capsuleRect();
  if (p.y < -50 || p.y > c.y + c.h + 2) return null;
  for (const node of [...state.barSlots.values(), $("#gear")]) {
    const r = node.getBoundingClientRect();
    if (p.x >= r.left && p.x <= r.right) return node;
  }
  return null;
}

function setHover(node) {
  if (node === state.hover) return;
  const prev = state.hover;
  state.hover = node;
  if (prev) {
    prev.classList.remove("hover");
    if (prev.dataset.label) hideHint();
  }
  clearTimer("hover");
  if (!node) return;
  node.classList.add("hover");
  if (node.dataset.label) showHint(node.dataset.label, centerOf(node));
  // 近づけると開く項目は、少しとどまったらパネルを開く
  const id = node.dataset.hoverPanel;
  if (!id || state.active === id) return;
  state.timers.hover = setTimeout(() => {
    state.timers.hover = 0;
    if (state.hover === node) openPanel(id);
  }, state.active ? HOVER_SWITCH : HOVER_OPEN);
}

function renderBar() {
  for (const [win, f] of state.frames) if (f.slot === "bar") state.frames.delete(win);
  state.barSlots.clear();
  state.hover = null;
  const items = $("#items");
  items.replaceChildren();

  let i = 0;
  for (const addon of enabledAddons()) {
    let node;
    if (addon.bar) {
      const iframe = createFrame(addon, "bar");
      node = el("div", { className: "bar-slot item" }, iframe);
      items.append(node);
      registerFrame(iframe, addon.id, "bar");
      if (addon.panel && panelTrigger(addon) === "hover") node.dataset.hoverPanel = addon.id;
    } else if (addon.panel) {
      const title = addon.panel.title || addon.name;
      node = el("button", { className: "icon-btn item", ariaLabel: title });
      if (addon.icon) {
        const glyph = el("span", { className: "glyph" });
        glyph.style.setProperty("--src", `url("${addonUrl(addon.id, addon.icon)}")`);
        node.append(glyph);
      } else {
        node.append(el("span", { className: "letter", textContent: addon.name.slice(0, 1) }));
      }
      node.addEventListener("click", () => {
        clearTimer("hover");
        togglePanel(addon.id);
      });
      node.dataset.label = title;
      if (panelTrigger(addon) === "hover") node.dataset.hoverPanel = addon.id;
      items.append(node);
    } else {
      continue;
    }
    node.style.setProperty("--i", i++);
    state.barSlots.set(addon.id, node);
  }
  $(".divider").style.setProperty("--i", i++);
  $("#gear").style.setProperty("--i", i++);
  syncActive();
  requestAnimationFrame(placeDock);
}

function syncActive() {
  for (const [id, node] of state.barSlots) node.classList.toggle("active", state.active === id);
  $("#gear").classList.toggle("active", state.active === "settings");
}

// --------------------------------------------------------------------- パネル

function panelWidthOf(id) {
  if (id === "settings") return SETTINGS_W;
  return clamp(state.addons.get(id)?.panel?.width ?? DEFAULT_PANEL_W, 200, 560);
}

function contentHeightOf(id) {
  if (id === "settings") return $("#settings-inner").offsetHeight;
  return state.heights.get(id) ?? 0;
}

function ensureView(id) {
  if (id === "settings" || state.views.has(id)) return;
  const addon = state.addons.get(id);
  const view = el("div", { className: "view" });
  view.dataset.view = id;
  const iframe = createFrame(addon, "panel");
  view.append(iframe);
  $("#views").append(view);
  registerFrame(iframe, id, "panel");
  state.views.set(id, { view, iframe });
}

function viewOf(id) {
  return id === "settings" ? $("#settings") : state.views.get(id)?.view;
}

/** パネルの位置と大きさを決める（開いているパネルが入れ替わるときは形が滑らかに変わる） */
function layoutPanel({ morph = true } = {}) {
  const id = state.active;
  if (!id) return;
  const panel = $("#panel");
  const anchor = id === "settings" ? $("#gear") : state.barSlots.get(id);
  const w = panelWidthOf(id);
  const h = clamp(contentHeightOf(id) || 120, 48, PANEL_MAX_H);
  const cx = anchor ? centerOf(anchor) : innerWidth / 2;
  const x = Math.round(clamp(cx - w / 2, 8, innerWidth - 8 - w));

  panel.classList.toggle("no-morph", !morph);
  const s = panel.style;
  s.setProperty("--px", `${x}px`);
  s.setProperty("--pw", `${w}px`);
  s.setProperty("--ph", `${h}px`);
  s.setProperty("--ox", `${clamp(cx - x, 16, w - 16)}px`);
  const view = viewOf(id);
  view.style.setProperty("--vw", `${w}px`);
  view.style.setProperty("--vmax", `${PANEL_MAX_H}px`);
  state.panelRect = { x, y: PANEL_TOP, w, h };
  syncHitRects();
}

async function openPanel(id, { focus = false } = {}) {
  if (state.phase !== "shown") reveal();
  const wasOpen = state.active !== null;
  const prev = state.active;
  if (prev === id) return;
  if (id === "settings") renderSettings();
  else {
    const addon = state.addons.get(id);
    if (!addon?.panel) return;
    ensureView(id);
  }

  state.active = id;
  hideHint();
  for (const v of document.querySelectorAll(".view")) v.classList.toggle("active", v.dataset.view === id);
  layoutPanel({ morph: wasOpen });
  const panel = $("#panel");
  if (!wasOpen) {
    // 初めて開くときは位置を決めてから現す
    void panel.offsetWidth;
  }
  panel.classList.add("open");
  $("#panel-title").textContent = id === "settings" ? "設定" : state.addons.get(id).panel.title || state.addons.get(id).name;
  syncActive();

  if (prev && prev !== "settings") notifyPanel(prev, "panelHidden");
  if (id !== "settings") notifyPanel(id, "panelShown");
  if (focus) invoke("take_focus").catch(() => {});
}

function notifyPanel(id, name) {
  const v = state.views.get(id);
  const frame = v && state.frames.get(v.iframe.contentWindow);
  if (frame) sendEvent(frame, name, {});
}

function closePanel() {
  if (state.active === null) return;
  const prev = state.active;
  state.active = null;
  state.panelRect = null;
  $("#panel").classList.remove("open");
  for (const v of document.querySelectorAll(".view")) v.classList.remove("active");
  syncActive();
  if (prev !== "settings") notifyPanel(prev, "panelHidden");
  syncHitRects();
}

function togglePanel(id, opts) {
  return state.active === id ? closePanel() : openPanel(id, opts);
}

/** ダイアログなどを出している間は引っ込めない */
async function holdingOpen(promise) {
  state.holdOpen++;
  clearTimer("hide");
  try {
    return await promise;
  } finally {
    setTimeout(() => {
      state.holdOpen = Math.max(0, state.holdOpen - 1);
      if (!nearInteractive(state.cursor, LEAVE_PAD)) scheduleHide();
    }, 400);
  }
}

// --------------------------------------------------------------------- アドオン API

const API = {
  "ui.setWidth": {
    run: (f, p) => {
      if (f.slot !== "bar") return;
      f.iframe.style.width = `${clamp(Number(p.width) || 0, 0, 640)}px`;
      placeDock();
    },
  },
  "ui.setHeight": {
    run: (f, p) => {
      if (f.slot !== "panel") return;
      const h = clamp(Number(p.height) || 0, 0, 4000);
      f.iframe.style.height = `${h}px`;
      state.heights.set(f.addonId, h);
      if (state.active === f.addonId) layoutPanel();
    },
  },
  "ui.openPanel": { run: (f) => openPanel(f.addonId) },
  "ui.closePanel": { run: () => closePanel() },
  "ui.togglePanel": { run: (f) => togglePanel(f.addonId) },
  "ui.focus": {
    run: async (f) => {
      await invoke("take_focus");
      f.iframe.focus();
    },
  },
  "ui.dismiss": {
    run: () => {
      state.holdOpen = 0;
      state.pinned = false;
      conceal();
    },
  },
  "ui.hint": {
    run: (f, p) => {
      if (f.slot !== "bar" || !p.text) return hideHint();
      const r = f.iframe.getBoundingClientRect();
      showHint(String(p.text).slice(0, 80), r.left + (Number(p.x) || r.width / 2));
    },
  },

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
      permissions: addon?.permissions ?? [],
    });
    // 初めて開いたパネルは、読み込みが終わってから「表示された」ことを知らせる
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

/** 選択肢。つまみが滑って移動する */
function seg(options, value, onPick) {
  const wrap = el("div", { className: "seg", role: "group" });
  const thumb = el("span", { className: "seg-thumb" });
  wrap.append(thumb);
  const buttons = options.map(([v, label]) => {
    const b = el("button", { textContent: label });
    b.dataset.value = v;
    b.addEventListener("click", () => {
      select(v);
      onPick(v);
    });
    wrap.append(b);
    return b;
  });
  const select = (v) => {
    for (const b of buttons) {
      const on = b.dataset.value === v;
      b.setAttribute("aria-pressed", String(on));
      if (on) {
        thumb.style.setProperty("--tx", `${b.offsetLeft}px`);
        thumb.style.setProperty("--tw", `${b.offsetWidth}px`);
      }
    }
  };
  // 描画が済んでから位置を測る
  requestAnimationFrame(() => {
    thumb.style.transition = "none";
    select(value);
    void thumb.offsetWidth;
    thumb.style.transition = "";
  });
  return wrap;
}

function toggle(checked, label, onChange) {
  const input = el("input", { type: "checkbox", checked, ariaLabel: label });
  input.setAttribute("role", "switch");
  input.addEventListener("change", () => onChange(input.checked, input));
  return el("label", { className: "switch" }, input, el("span"));
}

function row(title, sub, control, className = "") {
  return el(
    "div",
    { className: `s-row ${className}` },
    el("div", { className: "s-main" },
      el("div", { className: "s-label", textContent: title }),
      sub ? el("div", { className: "s-sub", textContent: sub }) : null),
    control,
  );
}

function arrowBtn(up) {
  const b = el("button", { className: "sd-btn", ariaLabel: up ? "上へ移動" : "下へ移動" });
  b.innerHTML = `<svg viewBox="0 0 10 10" aria-hidden="true"><path d="${up ? "M2 6.5L5 3.5L8 6.5" : "M2 3.5L5 6.5L8 3.5"}" stroke="currentColor" stroke-width="1.4" fill="none" stroke-linecap="round" stroke-linejoin="round"/></svg>`;
  return b;
}

async function saveConfig(patch) {
  state.config = await invoke("save_config", { config: { ...state.config, ...patch } });
}

function glyphFor(addon) {
  if (!addon.icon) return el("span", { className: "letter", textContent: addon.name.slice(0, 1) });
  const g = el("span", { className: "glyph" });
  g.style.setProperty("--src", `url("${addonUrl(addon.id, addon.icon)}")`);
  return g;
}

function renderAddonList() {
  const c = state.config;
  const entries = c.addons.filter((a) => state.addons.has(a.id));
  if (!entries.length) return el("div", { className: "sd-empty", textContent: "アドオンがありません" });
  const list = el("div", { className: "s-group" });
  entries.forEach((entry, i) => {
    const addon = state.addons.get(entry.id);
    const up = arrowBtn(true);
    const down = arrowBtn(false);
    up.disabled = i === 0;
    down.disabled = i === entries.length - 1;
    const move = async (delta) => {
      const order = [...state.config.addons];
      const from = order.findIndex((a) => a.id === entry.id);
      const to = from + delta;
      if (to < 0 || to >= order.length) return;
      [order[from], order[to]] = [order[to], order[from]];
      await saveConfig({ addons: order });
      renderBar();
      refreshSettings();
    };
    up.addEventListener("click", () => move(-1));
    down.addEventListener("click", () => move(1));

    const sw = toggle(entry.enabled, `${addon.name}を使う`, async (v, input) => {
      if (v && !addon.builtin && addon.permissions.length) {
        const names = addon.permissions.map((p) => `・${PERMISSION_LABELS[p] ?? p}`).join("\n");
        state.holdOpen++;
        const ok = confirm(`「${addon.name}」に次のことを許可しますか？\n\n${names}`);
        setTimeout(() => (state.holdOpen = Math.max(0, state.holdOpen - 1)), 400);
        if (!ok) {
          input.checked = false;
          return;
        }
      }
      await saveConfig({ addons: state.config.addons.map((a) => (a.id === entry.id ? { ...a, enabled: v } : a)) });
      renderBar();
      refreshSettings();
    });

    const perms = addon.permissions.map((p) => PERMISSION_LABELS[p] ?? p).join("、");
    const sub = [addon.description, perms && `権限：${perms}`, !addon.builtin && "自分で追加"].filter(Boolean).join("  ");
    const r = el("div", { className: `s-row addon${entry.enabled ? "" : " off"}`, title: sub },
      glyphFor(addon),
      el("div", { className: "s-main" },
        el("div", { className: "s-label", textContent: addon.name }),
        el("div", { className: "s-sub", textContent: sub })),
      el("div", { className: "move" }, up, down),
      sw,
    );
    list.append(r);
  });
  return list;
}

/** アドオン一覧だけを描き直す（つまみの位置などは保つ） */
function refreshSettings() {
  const holder = $("#s-addons");
  if (holder) holder.replaceChildren(renderAddonList());
  if (state.active === "settings") layoutPanel();
}

function renderSettings() {
  const root = $("#settings-inner");
  const c = state.config;

  const autostart = toggle(false, "Windows の起動時に開く", (v) => invoke("autostart_set", { enabled: v }));
  invoke("autostart_get").then((on) => (autostart.querySelector("input").checked = on)).catch(() => {});

  const display = el("div", { className: "s-group" },
    row("出てくる位置", null,
      seg([["left", "左"], ["center", "中央"], ["right", "右"]], c.position, async (v) => {
        await saveConfig({ position: v });
        placeDock();
        // 位置が変わるとパネルもついていく
        requestAnimationFrame(() => layoutPanel());
      })),
    row("反応する範囲", "上端に近づけたとき",
      seg([["narrow", "狭い"], ["normal", "普通"], ["wide", "広い"]], c.trigger, (v) => saveConfig({ trigger: v }))),
    row("テーマ", null,
      seg([["system", "自動"], ["light", "ライト"], ["dark", "ダーク"]], c.theme, async (v) => {
        await saveConfig({ theme: v });
        applyTheme();
      })),
    row("Windows の起動時に開く", null, autostart),
  );

  const openFolder = el("button", { className: "sd-btn", textContent: "アドオンフォルダを開く" });
  openFolder.addEventListener("click", () => invoke("open_addons_folder"));
  const reloadBtn = el("button", { className: "sd-btn", textContent: "読み直す" });
  reloadBtn.addEventListener("click", () => location.reload());
  const quitBtn = el("button", { className: "sd-btn ghost quit", textContent: "終了" });
  quitBtn.addEventListener("click", () => invoke("quit"));

  root.replaceChildren(
    el("div", { className: "s-head" },
      el("h2", { className: "s-title", textContent: "SideDock" }),
      el("span", { className: "s-ver", textContent: "0.2.0" })),
    el("div", { className: "s-sec", textContent: "表示" }),
    display,
    el("div", { className: "s-sec", textContent: "アドオン" }),
    el("div", { id: "s-addons" }, renderAddonList()),
    el("div", { className: "s-actions" }, openFolder, reloadBtn, quitBtn),
    state.errors.length
      ? el("div", { className: "s-notice", textContent: `読み込めなかったアドオン:\n${state.errors.join("\n")}` })
      : null,
    el("p", {
      className: "s-foot",
      textContent: "アドオンフォルダに manifest.json を含むフォルダを置いて「読み直す」を押すと、一覧に出てきます。",
    }),
  );
}

// --------------------------------------------------------------------- 起動

async function start() {
  const data = await invoke("init");
  state.platform = data.platform;
  state.config = data.config;
  state.errors = data.errors;
  state.addons = new Map(data.addons.map((a) => [a.id, a]));

  applyTheme();
  renderBar();
  onSuspend(!!data.suspended);

  const gear = $("#gear");
  gear.addEventListener("click", () => togglePanel("settings"));

  // カプセルの幅が変わったら位置と当たり判定を合わせ直す
  new ResizeObserver(() => placeDock()).observe(capsule());
  new ResizeObserver(() => state.active === "settings" && layoutPanel()).observe($("#settings-inner"));
  addEventListener("resize", () => {
    placeDock();
    layoutPanel({ morph: false });
  });

  matchMedia("(prefers-color-scheme: dark)").addEventListener("change", applyTheme);
  document.addEventListener("contextmenu", (e) => {
    if (!e.target.closest("input, textarea")) e.preventDefault();
  });
  document.addEventListener("keydown", (e) => {
    if (e.key !== "Escape") return;
    if (state.active) closePanel();
    else conceal();
  });

  // ドロップされたファイルは drop 権限のあるアドオンへ
  await webview.onDragDropEvent(({ payload }) => {
    if (payload.type === "drop" && payload.paths?.length) {
      broadcast("drop", { paths: payload.paths }, { permission: "drop" });
    }
  });

  await listen("cursor", ({ payload }) => onCursor(payload));
  await listen("suspend", ({ payload }) => onSuspend(!!payload));
  await listen("geometry", () => {
    placeDock();
    layoutPanel({ morph: false });
  });
  await listen("open-settings", () => {
    reveal({ pinned: true });
    openPanel("settings", { focus: true });
  });

  // 起動したら一度だけ姿を見せて、ここにいることを知らせる
  setTimeout(() => {
    reveal({ pinned: true });
    setTimeout(() => {
      if (!state.pinned) return;
      state.pinned = false;
      if (!nearInteractive(state.cursor, LEAVE_PAD) && !state.active) conceal();
    }, 1900);
  }, 450);
}

start().catch((err) => {
  document.body.textContent = `起動に失敗しました: ${err}`;
});
