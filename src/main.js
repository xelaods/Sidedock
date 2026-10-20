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
/** 診断ログ（トレイの「診断ログを開く」で見られる） */
const log = (msg) => invoke("log_ui", { message: String(msg) }).catch(() => {});
addEventListener("error", (e) => log(`エラー: ${e.message} (${e.filename}:${e.lineno})`));
addEventListener("unhandledrejection", (e) => log(`エラー: ${e.reason?.message ?? e.reason}`));

// 近づいたときの反応。h：上端から何 px で反応するか、spread：カプセルの左右に広げる幅、dwell：とどまる時間
const TRIGGERS = {
  narrow: { h: 3, spread: 0, dwell: 150 },
  normal: { h: 10, spread: 56, dwell: 70 },
  wide: { h: 30, spread: 160, dwell: 20 },
};
const HIDE_DELAY = 420; // カーソルが離れてから引っ込むまで
const HOVER_OPEN = 230; // 項目にとどまってパネルが開くまで
const HOVER_SWITCH = 80; // パネルが開いている間に隣へ移ったとき
const PANEL_GAP = 10; // カプセルとパネルの間
const PANEL_MAX_H = 430;
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
  windows: "開いているウィンドウの一覧・切り替え、スタートメニューを開く",
  settings: "Wi-Fi・Bluetooth・明るさ・ダークモードを変える",
  files: "最近使ったファイルの一覧を見る・ファイルをドラッグで渡す",
  activity: "全画面で遊んだゲームの記録を見る",
  media: "再生中の音楽を見る・操作する",
  system: "CPU・メモリ・バッテリーの状態を見る",
  clipboard: "コピーした文字の履歴を見る",
  power: "ロック・スリープ・シャットダウンする",
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
  timers: { dwell: 0, hide: 0, hover: 0, settle: 0, notice: 0, lowmem: 0 },
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

/** 見た目の種類。neon は暗い面が前提なので、いつもダーク */
const SKINS = [
  ["easy", "EasyUI"],
  ["obsidian", "オブシディアン"],
  ["glass", "グラス"],
  ["minimal", "ミニマル"],
  ["neon", "ネオン"],
  ["soft", "ソフト"],
  ["classic", "クラシック"],
];
/** アクセントカラー：[ダークのとき, ダークのときの文字, ライトのとき, ライトのときの文字] */
const ACCENTS = {
  blue: ["#8cc8ff", "#06121f", "#0a66c2", "#ffffff"],
  violet: ["#b8a2ff", "#140b2e", "#6b4fd8", "#ffffff"],
  pink: ["#ff9ccf", "#2b0a1a", "#d63384", "#ffffff"],
  orange: ["#ffb36b", "#2a1404", "#d9661f", "#ffffff"],
  green: ["#7fe0a8", "#04200f", "#1f9d55", "#ffffff"],
  mono: ["#fafafa", "#050505", "#0a0a0a", "#ffffff"],
};
const ACCENT_LABELS = { blue: "青", violet: "紫", pink: "ピンク", orange: "オレンジ", green: "緑", mono: "モノクロ" };

function computeDark() {
  if (state.config.skin === "neon") return true;
  const t = state.config.theme;
  return t === "dark" || (t === "system" && matchMedia("(prefers-color-scheme: dark)").matches);
}

/** 今の配色でのアクセントカラー { accent, ink } */
function accentColors() {
  const a = ACCENTS[state.config.accent] ?? ACCENTS.blue;
  return state.dark ? { accent: a[0], ink: a[1] } : { accent: a[2], ink: a[3] };
}

function themePayload() {
  return { dark: state.dark, skin: state.config.skin || "easy", accent: accentColors() };
}

function applyTheme() {
  state.dark = computeDark();
  const root = document.documentElement;
  root.dataset.theme = state.dark ? "dark" : "light";
  root.dataset.skin = state.config.skin || "easy";
  root.style.colorScheme = state.config.skin === "classic" ? "" : state.dark ? "dark" : "light";
  const { accent, ink } = accentColors();
  const n = parseInt(accent.slice(1), 16);
  root.style.setProperty("--sd-accent", accent);
  root.style.setProperty("--sd-accent-ink", ink);
  root.style.setProperty("--sd-glow", `rgba(${n >> 16}, ${(n >> 8) & 255}, ${n & 255}, ${state.dark ? 0.45 : 0.3})`);
  broadcast("theme", themePayload());
}

// --------------------------------------------------------------------- 位置と当たり判定

const capsule = () => $("#capsule");

/** カプセルの位置（変形の影響を受けない、最終的な位置） */
function capsuleRect() {
  const dock = $("#dock");
  const c = capsule();
  return { x: dock.offsetLeft + c.offsetLeft, y: dock.offsetTop + c.offsetTop, w: c.offsetWidth, h: c.offsetHeight };
}

/** 画面の下端（タスクバーの位置）に出しているか */
const atBottom = () => state.config?.edge === "bottom";

function applyEdge() {
  document.body.dataset.edge = atBottom() ? "bottom" : "top";
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
  // カプセルから画面の端までを含める（端に押し付けたままでも引っ込まない）
  const edge = atBottom()
    ? { x: c.x - pad, y: c.y - pad, w: c.w + pad * 2, h: innerHeight - c.y + pad + 50 }
    : { x: c.x - pad, y: -50, w: c.w + pad * 2, h: c.y + c.h + pad + 50 };
  if (inside(p, edge)) return true;
  if (state.panelRect && inside(p, grow(state.panelRect, pad))) return true;
  // カプセルとパネルの間の隙間
  if (state.panelRect) {
    const r = state.panelRect;
    const x0 = Math.min(c.x, r.x), x1 = Math.max(c.x + c.w, r.x + r.w);
    const gap = atBottom()
      ? { x: x0, y: r.y + r.h, w: x1 - x0, h: c.y - (r.y + r.h) }
      : { x: x0, y: c.y + c.h, w: x1 - x0, h: r.y - (c.y + c.h) };
    if (inside(p, gap)) return true;
  }
  return false;
}

/** クリックを受け取る範囲を Rust 側に伝える */
function syncHitRects() {
  const rects = [];
  if (state.phase === "shown" && !state.suspended) {
    const c = capsuleRect();
    // 端に押し付けたままでもクリックできるよう、カプセルから画面の端まで
    rects.push(atBottom()
      ? { x: c.x - 4, y: c.y - 4, w: c.w + 8, h: innerHeight - c.y + 4 }
      : { x: c.x - 4, y: 0, w: c.w + 8, h: c.y + c.h + 4 });
    if (state.panelRect) rects.push(grow(state.panelRect, 2));
  }
  invoke("set_hit_rects", { rects }).catch(() => {});
}

// --------------------------------------------------------------------- 出し入れ

/** 「常に表示」の設定か（タスクバーの代わりに使うとき向け） */
const alwaysShow = () => state.config?.show === "always";

function reveal({ pinned = false } = {}) {
  if (state.suspended) return;
  if (pinned) state.pinned = true;
  clearTimer("hide");
  if (state.phase === "shown") return;
  state.phase = "shown";
  // 出ている間は WebView2 に普段どおりメモリを使ってもらう
  clearTimer("lowmem");
  invoke("set_low_memory", { low: false }).catch(() => {});
  log("出てきた");
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
  if (alwaysShow() && !state.suspended) return;
  closePanel();
  hideHint();
  state.phase = "hidden";
  log("引っ込んだ");
  state.pinned = false;
  clearTimer("settle");
  setHover(null);
  document.body.classList.remove("is-shown", "settled");
  $("#spot").style.setProperty("--spot-o", "0");
  syncHitRects();
  invoke("release_focus").catch(() => {});
  broadcast("concealed", {});
  // 隠れてしばらくしたら、WebView2 に使っていないメモリを手放してもらう（軽量化）
  clearTimer("lowmem");
  state.timers.lowmem = setTimeout(() => invoke("set_low_memory", { low: true }).catch(() => {}), 2500);
}

function scheduleHide() {
  if (state.timers.hide || state.pinned || state.holdOpen > 0 || alwaysShow()) return;
  state.timers.hide = setTimeout(() => {
    state.timers.hide = 0;
    if (!nearInteractive(state.cursor, LEAVE_PAD)) conceal();
  }, HIDE_DELAY);
}

function inTriggerZone(p) {
  const t = trigger();
  const c = capsuleRect();
  const inX = p.x >= c.x - t.spread && p.x <= c.x + c.w + t.spread;
  if (atBottom()) return inX && p.y >= innerHeight - t.h && p.y <= innerHeight + 4;
  return inX && p.y <= t.h && p.y >= -4;
}

/** 隠れている間：上端の印をカーソルの近さで光らせる */
function updateProximity(p) {
  let prox = 0;
  if (p) {
    const c = capsuleRect();
    const dx = Math.max(0, Math.abs(p.x - (c.x + c.w / 2)) - c.w / 2);
    const dy = Math.max(0, atBottom() ? innerHeight - p.y : p.y);
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
  const onBar = (atBottom() ? p.y >= c.y - 10 : p.y <= c.y + c.h + 10) && p.x >= c.x - 30 && p.x <= c.x + c.w + 30;
  spot.style.setProperty("--mx", `${p.x - c.x}px`);
  spot.style.setProperty("--spot-o", onBar ? "1" : "0");
}

/** Rust 側から約 60 回/秒で届くカーソルの位置（離れたら null） */
function onCursor(p) {
  if (!state.gotCursor && p) {
    state.gotCursor = true;
    const c = capsuleRect();
    log(`カーソルの位置を受信: (${p.x}, ${p.y}) カプセル: x=${c.x} y=${c.y} w=${c.w} h=${c.h} 画面幅=${innerWidth}`);
  }
  state.cursor = p;
  if (state.suspended) return;
  detectGesture(p);
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
  if (on !== state.suspended) log(on ? "全画面のため無効" : "有効に戻った");
  state.suspended = on;
  document.body.classList.toggle("is-suspended", on);
  if (on) {
    state.holdOpen = 0;
    conceal();
    updateProximity(null);
  }
  syncHitRects();
  if (!on && alwaysShow()) reveal();
}

// --------------------------------------------------------------------- ドロップ

const SHELF = "sidedock.shelf";

/** ドロップされた位置（物理ピクセル）にあるアドオンの id */
function dropTarget(pos) {
  if (!pos) return null;
  const p = { x: pos.x / devicePixelRatio, y: pos.y / devicePixelRatio };
  if (state.active && state.active !== "settings" && state.panelRect && inside(p, state.panelRect)) return state.active;
  for (const [id, node] of state.barSlots) {
    const r = node.getBoundingClientRect();
    if (p.x >= r.left && p.x <= r.right && p.y >= r.top - 6 && p.y <= r.bottom + 6) return id;
  }
  return null;
}

// --------------------------------------------------------------------- ジェスチャー
// 画面の端（カーソルがそれ以上進めない所）での動きを見分ける。
// 端に沿って素早く払う・小刻みに振る・角に押し当てる、の 3 種類

const GESTURES = [
  ["swipeRight", "右へ払う", "端に沿って右へ素早く動かす"],
  ["swipeLeft", "左へ払う", "端に沿って左へ素早く動かす"],
  ["shake", "小刻みに振る", "端で左右に 2 往復ほど振る"],
  ["cornerLeft", "左の角", "左の角に押し当てて少し止める"],
  ["cornerRight", "右の角", "右の角に押し当てて少し止める"],
];

const media = (action, label) => async () => {
  await invoke("media_control", { action });
  return label;
};
const shell = (action) => async () => {
  state.holdOpen = 0;
  state.pinned = false;
  conceal();
  await invoke("shell_action", { action });
};
const openAddon = (id, name) => async () => {
  if (id !== "settings" && !enabledAddons().some((a) => a.id === id)) return `「${name}」アドオンが無効です`;
  reveal({ pinned: true });
  await openPanel(id);
};

/** 割り当てられる操作：[名前, 実行（戻り値の文字は知らせとして出す）] */
const GESTURE_ACTIONS = {
  none: ["なし", null],
  "media.toggle": ["再生 / 一時停止", media("toggle", "⏯ 再生 / 一時停止")],
  "media.next": ["次の曲", media("next", "⏭ 次の曲")],
  "media.prev": ["前の曲", media("prev", "⏮ 前の曲")],
  mute: ["ミュートの切り替え", async () => {
    const v = await invoke("volume_get");
    await invoke("volume_set_mute", { muted: !v.muted });
    return v.muted ? "🔊 ミュートを解除" : "🔇 ミュート";
  }],
  desktop: ["デスクトップを表示", shell("desktop")],
  taskview: ["タスクビュー", shell("taskview")],
  start: ["スタートメニュー", shell("start")],
  notifications: ["通知センター", shell("notifications")],
  screenshot: ["スクショ（範囲を選ぶ）", async () => {
    state.holdOpen = 0;
    state.pinned = false;
    conceal();
    await invoke("app_launch", { path: "ms-screenclip:", args: null });
  }],
  lock: ["画面をロック", () => invoke("power_action", { action: "lock" })],
  "open.shelf": ["置き場を開く", openAddon(SHELF, "置き場")],
  "open.search": ["検索を開く", async () => {
    const r = await openAddon("sidedock.search", "検索")();
    if (!r) invoke("take_focus").catch(() => {});
    return r;
  }],
  "open.clipboard": ["クリップボード履歴を開く", openAddon("sidedock.clipboard", "クリップボード履歴")],
  "open.settings": ["SideDock の設定", openAddon("settings", "設定")],
};

const gst = { stroke: null, corner: null, cool: 0 };

/** 端からの距離（上に出すときは上端、下に出すときは下端） */
function edgeDist(p) {
  return atBottom() ? innerHeight - p.y : p.y;
}

function runGesture(name) {
  const now = performance.now();
  if (now < gst.cool) return;
  const action = GESTURE_ACTIONS[state.config.gestures?.[name]];
  if (!action?.[1]) return;
  gst.cool = now + 700;
  log(`ジェスチャー：${name} → ${state.config.gestures[name]}`);
  Promise.resolve()
    .then(action[1])
    .then((text) => text && notify(text, 1300))
    .catch((e) => notify(String((typeof e === "string" ? e : e?.message) || "うまくいきませんでした").slice(0, 60), 1800));
}

function endStroke() {
  clearTimer("gstroke");
  const s = gst.stroke;
  gst.stroke = null;
  if (!s || s.fired) return;
  const total = s.lastX - s.x0;
  if (s.reversals === 0 && Math.abs(total) >= 200 && s.lastT - s.t0 <= 450) {
    runGesture(total > 0 ? "swipeRight" : "swipeLeft");
  }
}

function detectGesture(p) {
  const g = state.config.gestures;
  if (!g?.enabled) return;
  if (!p || edgeDist(p) > 3) {
    endStroke();
    gst.corner = null;
    clearTimer("gcorner");
    return;
  }
  const now = performance.now();

  // 角：押し当てたまま少し止める（カーソルが動かないと位置が届かないので、時間で確かめる）
  const corner = p.x <= 2 ? "cornerLeft" : p.x >= innerWidth - 3 ? "cornerRight" : null;
  if (corner !== gst.corner) {
    gst.corner = corner;
    clearTimer("gcorner");
    if (corner && g[corner] && g[corner] !== "none") {
      state.timers.gcorner = setTimeout(() => {
        state.timers.gcorner = 0;
        const c = state.cursor;
        if (gst.corner === corner && c && edgeDist(c) <= 3) runGesture(corner);
      }, 450);
    }
  }

  // 払う・振る
  let s = gst.stroke;
  if (!s || now - s.t0 > 1200) {
    s = gst.stroke = { x0: p.x, t0: now, lastX: p.x, lastT: now, dir: 0, legStart: p.x, reversals: 0, fired: false };
  }
  const dx = p.x - s.lastX;
  if (Math.abs(dx) >= 2) {
    const d = Math.sign(dx);
    if (s.dir && d !== s.dir) {
      if (Math.abs(s.lastX - s.legStart) >= 30) s.reversals++;
      s.legStart = s.lastX;
    }
    s.dir = d;
    s.lastX = p.x;
    s.lastT = now;
    if (!s.fired && s.reversals >= 3 && now - s.t0 <= 1000) {
      s.fired = true;
      runGesture("shake");
    }
  }
  // 止まったら（位置が届かなくなったら）ひと続きの動きとして見分ける
  clearTimer("gstroke");
  state.timers.gstroke = setTimeout(endStroke, 130);
}

// --------------------------------------------------------------------- ヒント

function showHint(text, x) {
  const hint = $("#hint");
  if (hint.classList.contains("notice")) return;
  if (!text || state.active) return hideHint();
  const wasShown = hint.classList.contains("show");
  reelText(hint, text);
  hint.style.setProperty("--hx", `${Math.round(x)}px`);
  hint.classList.toggle("moving", wasShown);
  hint.classList.add("show");
}
/** 文字を一つずつ、下からぼけた状態で現す（同じ文字なら何もしない） */
function reelText(node, text) {
  if (node.dataset.text === text) return;
  node.dataset.text = text;
  node.setAttribute("aria-label", text);
  node.replaceChildren(...[...text].map((ch, i) => {
    const r = el("span", { className: "r", textContent: ch });
    r.setAttribute("aria-hidden", "true");
    r.style.setProperty("--r", String(Math.min(i, 30)));
    return r;
  }));
}

function hideHint() {
  $("#hint").classList.remove("show", "moving", "notice");
}

/** カプセルを出して、知らせを数秒表示する */
function notify(text, ms, frame) {
  if (state.suspended || !text) return;
  reveal({ pinned: true });
  const slot = frame && state.barSlots.get(frame.addonId);
  const x = slot ? centerOf(slot) : (() => { const c = capsuleRect(); return c.x + c.w / 2; })();
  requestAnimationFrame(() => {
    const hint = $("#hint");
    reelText(hint, text);
    hint.style.setProperty("--hx", `${Math.round(x)}px`);
    hint.classList.add("show", "notice");
  });
  clearTimer("notice");
  state.timers.notice = setTimeout(() => {
    $("#hint").classList.remove("notice");
    hideHint();
    if (state.pinned) {
      state.pinned = false;
      if (!nearInteractive(state.cursor, LEAVE_PAD) && !state.active) conceal();
    }
  }, ms);
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
  const t = themePayload();
  const q = new URLSearchParams({ slot, theme: t.dark ? "dark" : "light", skin: t.skin, accent: t.accent.accent, ink: t.accent.ink });
  iframe.src = `${addonUrl(addon.id, entry)}?${q}`;
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

/** 乗せた項目の後ろへ、面を滑らせる。初めて乗せたときは、その場でふわっと現す */
function movePill(node) {
  const pill = $("#pill");
  if (!node) return pill.classList.remove("on");
  const fresh = !pill.classList.contains("on");
  pill.classList.toggle("snap", fresh);
  pill.style.setProperty("--pl", `${node.offsetLeft}px`);
  pill.style.setProperty("--pt", `${node.offsetTop}px`);
  pill.style.setProperty("--pw2", `${node.offsetWidth}px`);
  pill.style.setProperty("--ph2", `${node.offsetHeight}px`);
  if (fresh) void pill.offsetWidth;
  pill.classList.add("on");
}

function setHover(node) {
  if (node === state.hover) return;
  const prev = state.hover;
  state.hover = node;
  movePill(node);
  document.body.classList.toggle("engaged", !!node);
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

/** 閉じてしばらく使っていないパネルの iframe は捨てて、メモリを空ける（次に開くと作り直す） */
const PANEL_UNLOAD_MS = 90_000;
const unloadTimers = new Map();

function scheduleUnload(id) {
  if (id === "settings") return;
  clearTimeout(unloadTimers.get(id));
  unloadTimers.set(id, setTimeout(() => {
    unloadTimers.delete(id);
    if (state.active === id) return;
    const v = state.views.get(id);
    if (!v) return;
    state.frames.delete(v.iframe.contentWindow);
    v.view.remove();
    state.views.delete(id);
    state.heights.delete(id);
  }, PANEL_UNLOAD_MS));
}

function ensureView(id) {
  clearTimeout(unloadTimers.get(id));
  unloadTimers.delete(id);
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
  // 上端ならカプセルの下へ、下端ならカプセルの上へ開く
  const c = capsuleRect();
  const y = atBottom() ? c.y - PANEL_GAP - h : c.y + c.h + PANEL_GAP;
  s.setProperty("--py", `${y}px`);
  const view = viewOf(id);
  view.style.setProperty("--vw", `${w}px`);
  view.style.setProperty("--vmax", `${PANEL_MAX_H}px`);
  state.panelRect = { x, y, w, h };
  syncHitRects();
}

async function openPanel(id, { focus = false } = {}) {
  if (state.phase !== "shown") reveal();
  const wasOpen = state.active !== null;
  const prev = state.active;
  if (prev === id) return;
  if (id === "settings") {
    renderSettings();
    // 設定の中身も、上から順に現す
    [...$("#settings-inner").children].slice(0, 14).forEach((n, i) => {
      n.style.setProperty("--sd-i", String(i));
      n.classList.add("sd-intro");
      n.addEventListener("animationend", () => n.classList.remove("sd-intro"), { once: true });
    });
  }
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

  if (prev && prev !== "settings") {
    notifyPanel(prev, "panelHidden");
    scheduleUnload(prev);
  }
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
  scheduleUnload(prev);
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

  "ui.notify": {
    // カプセルを出して、下に短い知らせを表示する（タイマーの終了など）
    run: (f, p) => notify(String(p.text ?? "").slice(0, 80), clamp(Number(p.ms) || 4000, 1000, 15000), f),
  },

  "media.get": { perm: "media", run: () => invoke("media_get") },
  "media.art": { perm: "media", run: () => invoke("media_art") },
  "media.control": { perm: "media", run: (f, p) => invoke("media_control", { action: String(p.action) }) },
  "media.seek": { perm: "media", run: (f, p) => invoke("media_seek", { seconds: Number(p.seconds) || 0 }) },

  "system.stats": { perm: "system", run: () => invoke("system_stats") },
  "power.action": { perm: "power", run: (f, p) => invoke("power_action", { action: String(p.action) }) },

  "clipboard.list": { perm: "clipboard", run: () => invoke("clipboard_list") },
  "clipboard.copy": { perm: "clipboard", run: (f, p) => invoke("clipboard_copy", { id: String(p.id) }) },
  "clipboard.remove": { perm: "clipboard", run: (f, p) => invoke("clipboard_remove", { id: String(p.id) }) },
  "clipboard.pin": { perm: "clipboard", run: (f, p) => invoke("clipboard_pin", { id: String(p.id), pinned: !!p.pinned }) },
  "clipboard.clear": { perm: "clipboard", run: () => invoke("clipboard_clear") },
  // 文字をコピーするだけなら権限はいらない（読み取りはできない）
  "clipboard.write": { run: (f, p) => invoke("clipboard_write", { text: String(p.text ?? "").slice(0, 100000) }) },

  "windows.list": { perm: "windows", run: () => invoke("windows_list") },
  "windows.icon": { perm: "windows", run: (f, p) => invoke("windows_icon", { id: Number(p.id), exe: String(p.exe ?? "") }) },
  "windows.activate": { perm: "windows", run: (f, p) => invoke("windows_activate", { id: Number(p.id) }) },
  "windows.focus": { perm: "windows", run: (f, p) => invoke("windows_focus", { id: Number(p.id) }) },
  "windows.close": { perm: "windows", run: (f, p) => invoke("windows_close", { id: Number(p.id) }) },
  "net.status": { perm: "settings", run: () => invoke("net_status") },
  "wifi.list": { perm: "settings", run: () => invoke("wifi_list") },
  "wifi.connect": { perm: "settings", run: (f, p) => invoke("wifi_connect", { ssid: String(p.ssid) }) },
  "radios.get": { perm: "settings", run: () => invoke("radios_get") },
  "radios.set": { perm: "settings", run: (f, p) => invoke("radio_set", { kind: String(p.kind), on: !!p.on }) },
  "brightness.get": { perm: "settings", run: () => invoke("brightness_get") },
  "brightness.set": { perm: "settings", run: (f, p) => invoke("brightness_set", { value: clamp(Math.round(Number(p.value) || 0), 0, 100) }) },
  "theme.getDark": { perm: "settings", run: () => invoke("dark_get") },
  "theme.setDark": { perm: "settings", run: (f, p) => invoke("dark_set", { dark: !!p.dark }) },
  "battery.get": {
    run: async () => {
      const [level, charging] = await invoke("battery_get");
      return level == null ? null : { level, charging };
    },
  },

  "files.recent": { perm: "files", run: (f, p) => invoke("recent_files", { limit: Number(p.limit) || 30 }) },
  "files.exists": {
    perm: "files",
    run: (f, p) => invoke("files_exist", { paths: (Array.isArray(p.paths) ? p.paths : []).slice(0, 300).map(String) }),
  },
  "files.drag": {
    perm: "files",
    run: (f, p) => {
      const paths = (Array.isArray(p.paths) ? p.paths : []).slice(0, 300).map(String);
      if (!paths.length) throw new Error("ファイルがありません");
      // 渡す先が隠れないよう、カーソルが離れたら普段どおり引っ込める
      state.pinned = false;
      return invoke("files_drag", { paths });
    },
  },
  "games.log": { perm: "activity", run: () => invoke("game_log") },
  "games.clear": { perm: "activity", run: (f, p) => invoke("game_log_clear", { exe: p.exe == null ? null : String(p.exe) }) },
  "system.netSpeed": {
    perm: "system",
    run: async () => {
      const [rx, tx] = await invoke("net_speed");
      return { rx, tx };
    },
  },

  "tray.list": { perm: "windows", run: () => invoke("tray_list") },
  "tray.running": { perm: "windows", run: () => invoke("tray_running") },
  "tray.click": {
    perm: "windows",
    run: (f, p) => invoke("tray_click", { key: String(p.key), button: String(p.button ?? "left") }),
  },
  "shell.action": {
    perm: "windows",
    run: async (f, p) => {
      const action = String(p.action);
      // スタートや通知センターは SideDock の上に重なるので、先に引っ込める
      if (action !== "tray") { state.holdOpen = 0; state.pinned = false; conceal(); }
      await invoke("shell_action", { action });
    },
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
      ...themePayload(),
      permissions: addon?.permissions ?? [],
      visible: state.phase === "shown",
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

function refreshShowSub(v) {
  const sub = document.querySelector(".s-show .s-sub");
  if (sub) sub.textContent = v === "always" ? "いつもカプセルを出しておく" : "上端に近づけたときだけ出す";
}

function renderSettings() {
  const root = $("#settings-inner");
  const c = state.config;

  const autostart = toggle(false, "Windows の起動時に開く", (v) => invoke("autostart_set", { enabled: v }));
  invoke("autostart_get").then((on) => (autostart.querySelector("input").checked = on)).catch(() => {});

  const display = el("div", { className: "s-group" },
    row("出てくる場所", c.edge === "bottom" ? "画面の下（タスクバーの位置）" : "画面の上",
      seg([["top", "上"], ["bottom", "下"]], c.edge, async (v) => {
        await saveConfig({ edge: v });
        applyEdge();
        const sub = document.querySelector(".s-edge .s-sub");
        if (sub) sub.textContent = v === "bottom" ? "画面の下（タスクバーの位置）" : "画面の上";
        requestAnimationFrame(() => { placeDock(); layoutPanel({ morph: false }); });
      }), "s-edge"),
    row("横の位置", null,
      seg([["left", "左"], ["center", "中央"], ["right", "右"]], c.position, async (v) => {
        await saveConfig({ position: v });
        placeDock();
        // 位置が変わるとパネルもついていく
        requestAnimationFrame(() => layoutPanel());
      })),
    row("反応する範囲", "上端に近づけたとき",
      seg([["narrow", "狭い"], ["normal", "普通"], ["wide", "広い"]], c.trigger, (v) => saveConfig({ trigger: v }))),
    row("表示のしかた", c.show === "always" ? "いつもカプセルを出しておく" : "上端に近づけたときだけ出す",
      seg([["hover", "近づけたとき"], ["always", "常に表示"]], c.show, async (v) => {
        await saveConfig({ show: v });
        if (v === "always") reveal();
        else scheduleHide();
        refreshShowSub(v);
      }), "s-show"),
    row("テーマ", null,
      seg([["system", "自動"], ["light", "ライト"], ["dark", "ダーク"]], c.theme, async (v) => {
        await saveConfig({ theme: v });
        applyTheme();
      })),
    row("Windows の起動時に開く", null, autostart),
    row("タスクバーの代わりにする", "Windows のタスクバーをなくし、SideDock を画面の下に常に表示します。終了すると元に戻ります",
      toggle(c.hideTaskbar, "タスクバーの代わりにする", async (v) => {
        // 置き換えるときは、下に常に表示して、タスクバーの代わりになるアドオンも有効にする
        const patch = v ? { hideTaskbar: true, edge: "bottom", show: "always" } : { hideTaskbar: false, show: "hover" };
        if (v) {
          const want = ["sidedock.taskbar", "sidedock.tray", "sidedock.quick", "sidedock.clock"];
          patch.addons = state.config.addons.map((a) => (want.includes(a.id) ? { ...a, enabled: true } : a));
        }
        await saveConfig(patch);
        applyEdge();
        renderBar();
        if (v) reveal(); else scheduleHide();
        // 位置などのつまみも合わせて描き直す
        requestAnimationFrame(() => { renderSettings(); placeDock(); layoutPanel({ morph: false }); });
      })),
  );

  const openFolder = el("button", { className: "sd-btn", textContent: "アドオンフォルダを開く" });
  openFolder.addEventListener("click", () => invoke("open_addons_folder"));
  const reloadBtn = el("button", { className: "sd-btn", textContent: "読み直す" });
  reloadBtn.addEventListener("click", () => location.reload());
  const quitBtn = el("button", { className: "sd-btn ghost quit", textContent: "終了" });
  quitBtn.addEventListener("click", () => invoke("quit"));

  root.replaceChildren(...[
    el("div", { className: "s-head" },
      el("h2", { className: "s-title", textContent: "SideDock" }),
      el("span", { className: "s-ver", textContent: "0.10.0" })),
    el("div", { className: "s-sec", textContent: "表示" }),
    display,
    el("div", { className: "s-sec", textContent: "デザイン" }),
    renderDesign(),
    el("div", { className: "s-sec", textContent: "ジェスチャー" }),
    renderGestures(),
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
  ].filter(Boolean));
}

/** 設定の「ジェスチャー」：画面の端での動きと、割り当てる操作 */
function renderGestures() {
  const g = { ...state.config.gestures };
  const rows = GESTURES.map(([key, title, sub]) => {
    const select = el("select", { className: "sd-select s-select", ariaLabel: title });
    for (const [id, [label]] of Object.entries(GESTURE_ACTIONS)) {
      select.append(el("option", { value: id, textContent: label, selected: g[key] === id }));
    }
    select.addEventListener("change", () => {
      saveConfig({ gestures: { ...state.config.gestures, [key]: select.value } });
    });
    return row(title, sub, select, "s-gesture");
  });
  const setDisabled = (on) => { for (const r of rows) r.classList.toggle("off", !on); };
  setDisabled(g.enabled);
  return el("div", { className: "s-group" },
    row("画面の端のジェスチャー", "端（カーソルがそれ以上進めない所）での動きで操作します。全画面のアプリを使っている間は止まります",
      toggle(g.enabled, "画面の端のジェスチャー", (v) => {
        saveConfig({ gestures: { ...state.config.gestures, enabled: v } });
        setDisabled(v);
      })),
    ...rows,
  );
}

/** 設定の「デザイン」：見た目の見本とアクセントカラー */
function renderDesign() {
  const c = state.config;
  const cards = SKINS.map(([id, label]) => {
    const b = el("button", { className: "skin-card", ariaLabel: `デザイン：${label}` },
      el("span", { className: `mini ${id}` }, el("i"), el("i"), el("i")),
      el("span", { textContent: label }));
    b.setAttribute("aria-pressed", String((c.skin || "easy") === id));
    b.addEventListener("click", async () => {
      for (const x of cards) x.setAttribute("aria-pressed", String(x === b));
      await saveConfig({ skin: id });
      applyTheme();
      // カプセルの大きさが変わるデザインもあるので、置き直す
      requestAnimationFrame(() => { placeDock(); layoutPanel({ morph: false }); });
      updateThemeNote();
    });
    return b;
  });
  const dots = Object.keys(ACCENTS).map((id) => {
    const b = el("button", { className: "accent-dot", ariaLabel: `アクセントカラー：${ACCENT_LABELS[id]}`, title: ACCENT_LABELS[id] });
    b.style.setProperty("--c", state.dark ? ACCENTS[id][0] : ACCENTS[id][2]);
    b.setAttribute("aria-pressed", String((c.accent || "blue") === id));
    b.addEventListener("click", async () => {
      for (const x of dots) x.setAttribute("aria-pressed", String(x === b));
      await saveConfig({ accent: id });
      applyTheme();
    });
    return b;
  });
  const note = el("div", { className: "s-sub s-theme-note" });
  const updateThemeNote = () => {
    note.textContent = state.config.skin === "neon" ? "ネオンはいつもダークで表示します" : "";
    note.hidden = state.config.skin !== "neon";
  };
  updateThemeNote();
  return el("div", { className: "s-group" },
    el("div", { className: "s-design" }, el("div", { className: "skins" }, ...cards)),
    row("アクセントカラー", null, el("div", { className: "accents" }, ...dots)),
    note,
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
  applyEdge();
  renderBar();
  log(`UI を起動: アドオン ${data.addons.length} 個、全画面中=${data.suspended}、画面 ${innerWidth}×${innerHeight}、エラー ${data.errors.length} 件`);
  for (const e of data.errors) log(`アドオンの読み込みエラー: ${e}`);
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
    if (alwaysShow()) invoke("release_focus").catch(() => {});
  });

  // ドロップされたファイルは drop 権限のあるアドオンへ
  // 落とした場所のアドオン（バーの項目か、開いているパネル）にだけ渡す。
  // どれでもない所なら置き場へ。置き場がなければ、受け取れるアドオンすべてへ
  await webview.onDragDropEvent(({ payload }) => {
    if (payload.type !== "drop" || !payload.paths?.length) return;
    const data = { paths: payload.paths };
    const target = dropTarget(payload.position);
    if (target && hasPermission(target, "drop")) return broadcast("drop", data, { addonId: target });
    if (enabledAddons().some((a) => a.id === SHELF)) return broadcast("drop", data, { addonId: SHELF });
    broadcast("drop", data, { permission: "drop" });
  });

  // クリップボードの履歴を使うアドオンが有効なら、起動直後から記録を始める
  if (enabledAddons().some((a) => a.permissions.includes("clipboard"))) invoke("clipboard_watch").catch(() => {});
  await listen("clipboard-changed", () => broadcast("clipboardChanged", {}, { permission: "clipboard" }));
  await listen("tray-changed", () => broadcast("trayChanged", {}, { permission: "windows" }));
  // 変化があったときだけアドオンに知らせる（各アドオンが何度も取り直さなくてよいように）
  await listen("windows-changed", () => broadcast("windowsChanged", {}, { permission: "windows" }));
  await listen("volume-changed", ({ payload }) => broadcast("volumeChanged", payload ?? null, { permission: "volume" }));
  await listen("media-changed", () => broadcast("mediaChanged", {}, { permission: "media" }));
  await listen("network-changed", () => broadcast("networkChanged", {}, { permission: "settings" }));

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

  // 「常に表示」ならそのまま出しておく
  if (alwaysShow()) {
    setTimeout(() => reveal(), 300);
    return;
  }
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
  log(`起動に失敗: ${err?.message ?? err}`);
  document.body.textContent = `起動に失敗しました: ${err}`;
});
