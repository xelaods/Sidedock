/*
 * SideDock アドオン SDK
 * アドオンの HTML の <head> で読み込むと window.SideDock が使えるようになります。
 *   <link rel="stylesheet" href="http://addon.localhost/_sdk/sidedock.css">
 *   <script src="http://addon.localhost/_sdk/sidedock.js"></script>
 * アドオンはサンドボックス化された iframe で動き、本体とは postMessage でやり取りします。
 */
(() => {
  "use strict";
  const params = new URLSearchParams(location.search);
  const slot = params.get("slot") === "bar" ? "bar" : "panel";

  // 本体の配色をすぐ反映（読み込み直後に白く光らないように）
  const applyTheme = (dark) => {
    const root = document.documentElement;
    root.dataset.theme = dark ? "dark" : "light";
    root.style.colorScheme = dark ? "dark" : "light";
  };
  applyTheme(params.get("theme") !== "light");
  document.documentElement.dataset.slot = slot;

  let seq = 0;
  const pending = new Map();
  const handlers = new Map();
  let context = null;

  const post = (msg) => parent.postMessage({ __sd: 1, ...msg }, "*");

  function call(method, params) {
    return new Promise((resolve, reject) => {
      const id = ++seq;
      pending.set(id, { resolve, reject });
      post({ type: "call", id, method, params: params ?? {} });
    });
  }

  // パネルが今開いているか。あとから on("panelShown") したときにもすぐ知らせるため
  let panelOpen = false;

  function on(name, fn) {
    if (!handlers.has(name)) handlers.set(name, new Set());
    handlers.get(name).add(fn);
    if (name === "panelShown" && panelOpen) queueMicrotask(() => fn({}));
    return () => handlers.get(name)?.delete(fn);
  }

  function emit(name, data) {
    for (const fn of handlers.get(name) ?? []) {
      try { fn(data); } catch (e) { console.error(e); }
    }
  }

  window.addEventListener("message", (e) => {
    if (e.source !== parent) return;
    const d = e.data;
    if (!d || d.__sd !== 1) return;
    if (d.type === "result") {
      const p = pending.get(d.id);
      if (!p) return;
      pending.delete(d.id);
      d.ok ? p.resolve(d.value) : p.reject(new Error(d.error));
    } else if (d.type === "event") {
      if (d.name === "init") context = d.data;
      if (d.name === "init" || d.name === "theme") applyTheme(d.data.dark);
      if (d.name === "panelShown") panelOpen = true;
      if (d.name === "panelHidden") panelOpen = false;
      emit(d.name, d.data);
    }
  });

  const ready = new Promise((resolve) => {
    const off = on("init", (data) => { off(); resolve(data); });
  });

  // 中身の大きさに合わせて iframe の大きさを本体に伝える
  //   bar   … 幅（高さは 32px 固定）
  //   panel … 高さ（幅は manifest の panel.width）
  const measure = () => {
    const r = document.body.getBoundingClientRect();
    return Math.ceil(slot === "bar" ? r.width : r.height);
  };
  let last = -1;
  const report = () => {
    const v = measure();
    if (v === last) return;
    last = v;
    const method = slot === "bar" ? "ui.setWidth" : "ui.setHeight";
    call(method, slot === "bar" ? { width: v } : { height: v }).catch(() => {});
  };
  const start = () => {
    new ResizeObserver(report).observe(document.body);
    report();
  };
  document.readyState === "loading" ? document.addEventListener("DOMContentLoaded", start) : start();

  // data-hint を付けた要素にカーソルを乗せると、カプセルの下に小さな説明が出る
  let hinted = null;
  document.addEventListener("pointerover", (e) => {
    const el = e.target.closest?.("[data-hint]");
    if (el === hinted) return;
    hinted = el;
    if (!el) return call("ui.hint", { text: null }).catch(() => {});
    const r = el.getBoundingClientRect();
    call("ui.hint", { text: el.dataset.hint, x: r.left + r.width / 2 }).catch(() => {});
  });
  document.documentElement.addEventListener("pointerleave", () => {
    if (hinted) call("ui.hint", { text: null }).catch(() => {});
    hinted = null;
  });

  // Esc で閉じる／右クリックメニューは入力欄以外では出さない
  document.addEventListener("keydown", (e) => {
    if (e.key === "Escape") call("ui.dismiss").catch(() => {});
  });
  document.addEventListener("contextmenu", (e) => {
    if (!e.target.closest?.("input, textarea, [contenteditable]")) e.preventDefault();
  });

  // スライダーの塗り（.sd-range の --v）を値に合わせる
  const paintRange = (el) => {
    const min = Number(el.min || 0), max = Number(el.max || 100);
    el.style.setProperty("--v", `${((Number(el.value) - min) / (max - min || 1)) * 100}%`);
  };
  document.addEventListener("input", (e) => {
    if (e.target.matches?.(".sd-range")) paintRange(e.target);
  });

  window.SideDock = Object.freeze({
    /** "bar"（カプセルに並ぶウィジェット）か "panel"（下に開くパネル） */
    slot,
    /** 本体との接続が済むと解決する。{ addonId, slot, dark, permissions } */
    ready,
    get context() { return context; },
    call,
    /** イベント：init / theme / panelShown / panelHidden / storageChanged / drop / revealed / concealed / clipboardChanged */
    on,
    ui: {
      openPanel: () => call("ui.openPanel"),
      closePanel: () => call("ui.closePanel"),
      togglePanel: () => call("ui.togglePanel"),
      /** キーボード入力を受け取れるようにする（検索欄など） */
      focus: () => call("ui.focus"),
      /** SideDock ごと引っ込める */
      dismiss: () => call("ui.dismiss"),
      hint: (text, x) => call("ui.hint", { text, x }),
      /** カプセルを出して知らせを表示する（ms ミリ秒） */
      notify: (text, ms) => call("ui.notify", { text, ms }),
      paintRange,
    },
    storage: {
      get: () => call("storage.get"),
      set: (data) => call("storage.set", { data }),
    },
    app: {
      launch: (path, args) => call("app.launch", { path, args }),
      icon: (path) => call("app.icon", { path }),
    },
    dialog: {
      pickFiles: (filters) => call("dialog.pickFiles", { filters }),
      pickFolder: () => call("dialog.pickFolder"),
    },
    browser: {
      list: () => call("browser.list"),
      open: (url, browserId) => call("browser.open", { url, browserId }),
    },
    media: {
      /** 再生中のメディア（なければ null）。{ title, artist, album, app, status, position, duration, canSeek, canNext, canPrev, artKey } */
      get: () => call("media.get"),
      /** ジャケット画像の data URL（なければ null） */
      art: () => call("media.art"),
      /** "toggle" | "play" | "pause" | "next" | "prev" */
      control: (action) => call("media.control", { action }),
      seek: (seconds) => call("media.seek", { seconds }),
    },
    system: {
      /** { cpu, cpuName, cores, memUsed, memTotal, diskUsed, diskTotal, battery, charging, uptime } */
      stats: () => call("system.stats"),
    },
    power: {
      /** "lock" | "sleep" | "restart" | "shutdown" | "signout" */
      action: (action) => call("power.action", { action }),
    },
    clipboard: {
      list: () => call("clipboard.list"),
      copy: (id) => call("clipboard.copy", { id }),
      remove: (id) => call("clipboard.remove", { id }),
      pin: (id, pinned) => call("clipboard.pin", { id, pinned }),
      clear: () => call("clipboard.clear"),
    },
    volume: {
      get: () => call("volume.get"),
      set: (level) => call("volume.set", { level }),
      setMute: (muted) => call("volume.setMute", { muted }),
    },
  });

  // アドオンのスクリプト（type="module" を含む）がイベントを受け取る準備を終えてから知らせる
  if (document.readyState === "loading") {
    document.addEventListener("DOMContentLoaded", () => post({ type: "ready" }));
  } else {
    post({ type: "ready" });
  }
})();
