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
  applyTheme(params.get("theme") === "dark");
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

  function on(name, fn) {
    if (!handlers.has(name)) handlers.set(name, new Set());
    handlers.get(name).add(fn);
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
      emit(d.name, d.data);
    }
  });

  const ready = new Promise((resolve) => {
    const off = on("init", (data) => { off(); resolve(data); });
  });

  // バーに置くウィジェットは、中身の高さに合わせて iframe の高さを自動で変える
  if (slot === "bar") {
    let last = -1;
    const report = () => {
      const h = Math.ceil(document.body?.getBoundingClientRect().height ?? 0);
      if (h !== last) {
        last = h;
        call("ui.setHeight", { height: h }).catch(() => {});
      }
    };
    const start = () => {
      new ResizeObserver(report).observe(document.body);
      report();
    };
    document.readyState === "loading"
      ? document.addEventListener("DOMContentLoaded", start)
      : start();
  }

  window.SideDock = Object.freeze({
    /** "bar"（バーのウィジェット）か "panel"（パネル） */
    slot,
    /** 本体との接続が済むと解決する。{ addonId, slot, dark, permissions } */
    ready,
    get context() { return context; },
    call,
    /** イベント：init / theme / panelShown / panelHidden / storageChanged / drop */
    on,
    ui: {
      openPanel: () => call("ui.openPanel"),
      closePanel: () => call("ui.closePanel"),
      setHeight: (height) => call("ui.setHeight", { height }),
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
    volume: {
      get: () => call("volume.get"),
      set: (level) => call("volume.set", { level }),
      setMute: (muted) => call("volume.setMute", { muted }),
    },
  });

  post({ type: "ready" });
})();
