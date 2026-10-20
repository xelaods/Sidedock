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
  const COLOR = /^#[0-9a-f]{6}$/i;
  const applyTheme = (dark, skin, accent) => {
    const root = document.documentElement;
    root.dataset.theme = dark ? "dark" : "light";
    root.style.colorScheme = skin === "classic" ? "" : dark ? "dark" : "light";
    if (typeof skin === "string" && /^[a-z]{1,16}$/.test(skin)) root.dataset.skin = skin;
    // アクセントカラーは本体が実際の色で渡す（アドオンは色の名前を知らなくてよい）
    if (accent && COLOR.test(accent.accent ?? "") && COLOR.test(accent.ink ?? "")) {
      root.style.setProperty("--sd-accent", accent.accent);
      root.style.setProperty("--sd-accent-ink", accent.ink);
      const n = parseInt(accent.accent.slice(1), 16);
      root.style.setProperty("--sd-glow", `rgba(${n >> 16}, ${(n >> 8) & 255}, ${n & 255}, ${dark ? 0.45 : 0.3})`);
    }
  };
  applyTheme(params.get("theme") !== "light", params.get("skin"), { accent: params.get("accent"), ink: params.get("ink") });
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
  // カプセルが今出ているか。隠れている間はアニメーションを止め、ポーリングを減らす（軽量化）
  let visible = false;
  const setVisible = (v) => {
    visible = !!v;
    document.documentElement.dataset.visible = String(visible);
  };
  setVisible(false);

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
      if (d.name === "init" || d.name === "theme") applyTheme(d.data.dark, d.data.skin, d.data.accent);
      if (d.name === "panelShown") panelOpen = true;
      if (d.name === "init") setVisible(d.data.visible);
      if (d.name === "revealed") setVisible(true);
      if (d.name === "concealed") setVisible(false);
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

  /**
   * fn を繰り返し呼ぶ。カプセルが出ている間は visible ミリ秒ごと、隠れている間は hidden ミリ秒ごと
   * （hidden を 0 にすると隠れている間は止める）。出てきた瞬間にもすぐ 1 回呼ぶ
   */
  function poll(fn, { visible: shownMs = 1000, hidden: hiddenMs = 0 } = {}) {
    let timer = 0, stopped = false, running = false;
    const run = async () => {
      clearTimeout(timer);
      if (stopped) return;
      if (!running) {
        running = true;
        try { await fn(); } catch (e) { console.error(e); }
        running = false;
      }
      const wait = visible ? shownMs : hiddenMs;
      if (wait > 0 && !stopped) timer = setTimeout(run, wait);
    };
    const off = on("revealed", () => run());
    const offHide = on("concealed", () => { if (!hiddenMs) clearTimeout(timer); });
    ready.then(run);
    return {
      now: run,
      stop() { stopped = true; clearTimeout(timer); off(); offHide(); },
    };
  }

  window.SideDock = Object.freeze({
    /** "bar"（カプセルに並ぶウィジェット）か "panel"（下に開くパネル） */
    slot,
    /** 本体との接続が済むと解決する。{ addonId, slot, dark, skin, accent, permissions } */
    ready,
    get context() { return context; },
    /** カプセルが今出ているか */
    get visible() { return visible; },
    poll,
    call,
    /** イベント：init / theme / panelShown / panelHidden / storageChanged / drop / revealed / concealed / clipboardChanged / trayChanged / windowsChanged / volumeChanged / mediaChanged / networkChanged */
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
      /** { cpu, cpuName, cores, memUsed, memTotal, diskUsed, diskTotal, battery, charging, uptime, netRx, netTx } */
      stats: () => call("system.stats"),
      /** 今の通信速度（バイト/秒）{ rx, tx }。続けて呼ぶと、その間の平均になる */
      netSpeed: () => call("system.netSpeed"),
    },
    files: {
      /** 最近使ったファイル：[{ name, path, at, isDir }]（権限 files） */
      recent: (limit = 30) => call("files.recent", { limit }),
      /** ファイルが今もあるか：[true, false, …]（権限 files） */
      exists: (paths) => call("files.exists", { paths }),
      /** ファイルをつかんで他のアプリへドラッグで渡す。マウスのボタンを押している間に呼ぶ。
          ドロップされると "copy" | "move" | "link" | "none" で解決する（権限 files） */
      drag: (paths) => call("files.drag", { paths }),
    },
    games: {
      /** 全画面で遊んだゲームの記録：{ days: { "YYYY-MM-DD": { exe: 秒 } }, names: { exe: 名前 }, sessions: [{ exe, start, end }], today, current }（権限 activity） */
      log: () => call("games.log"),
      /** 記録を消す。exe を渡すとそのアプリの分だけ（権限 activity） */
      clear: (exe) => call("games.clear", { exe: exe ?? null }),
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
      /** 文字をコピーする（権限なしで使える） */
      write: (text) => call("clipboard.write", { text }),
    },
    windows: {
      /** 開いているウィンドウ：[{ id, title, exe, minimized, active }] */
      list: () => call("windows.list"),
      icon: (id, exe) => call("windows.icon", { id, exe }),
      /** 使っていたウィンドウなら最小化、そうでなければ前面へ */
      activate: (id) => call("windows.activate", { id }),
      focus: (id) => call("windows.focus", { id }),
      close: (id) => call("windows.close", { id }),
    },
    /** クイック設定（権限 settings） */
    net: {
      /** { kind: "wifi" | "ethernet" | "none", ssid, bars: 0〜5, internet } */
      status: () => call("net.status"),
    },
    wifi: {
      /** [{ ssid, signal: 0〜100, secured, connected, saved }] */
      list: () => call("wifi.list"),
      /** 保存済みのネットワークにつなぐ（保存されていなければ "unsaved" のエラー） */
      connect: (ssid) => call("wifi.connect", { ssid }),
    },
    radios: {
      /** [{ kind: "wifi" | "bluetooth", on }] */
      get: () => call("radios.get"),
      set: (kind, on) => call("radios.set", { kind, on }),
    },
    brightness: {
      /** 0〜100（変えられない画面なら null） */
      get: () => call("brightness.get"),
      set: (value) => call("brightness.set", { value }),
    },
    theme: {
      getDark: () => call("theme.getDark"),
      setDark: (dark) => call("theme.setDark", { dark }),
    },
    /** バッテリー（権限なしで使える）。{ level, charging } かバッテリーがなければ null */
    battery: {
      get: () => call("battery.get"),
    },
    tray: {
      /** 他のアプリのトレイアイコン：[{ key, tip, icon, exe }]（タスクバーの代わりにしている間だけ） */
      list: () => call("tray.list"),
      /** トレイを受け取っているか */
      running: () => call("tray.running"),
      /** "left" | "right" | "double" */
      click: (key, button = "left") => call("tray.click", { key, button }),
    },
    shell: {
      /** "start" | "quick" | "notifications" | "desktop" | "tray" | "taskview" */
      action: (action) => call("shell.action", { action }),
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
