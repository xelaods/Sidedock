# SideDock

画面の上端に隠れていて、カーソルを近づけるとヌルっと出てくる、Windows 用のコンパクトなランチャーです。
画面の大きさ（作業領域）は変えず、いま開いているアプリの上に重なって出てきます。全画面のアプリ（動画・ゲーム・F11 のブラウザなど）を使っている間は出てきません。

同梱のアドオン（設定でオン/オフ・並べ替えできます。★は最初から有効）

| アドオン | できること |
| --- | --- |
| タスクバー | スタートボタンと、開いているアプリのアイコン。クリックで切り替え・最小化、中クリックで新しく開く、右クリックでウィンドウ一覧（閉じるボタン付き） |
| 通知とトレイ | 他のアプリのトレイアイコン（タスクバーの代わりにしている間）と通知センター |
| ★ 時計 | 日付と時刻。押すとカレンダー（日本の祝日つき）が開く |
| クイック設定 | Wi-Fi・音量・バッテリーの今の状態をアイコンで表示。押すと Wi-Fi / Bluetooth のオン・オフ、Wi-Fi の一覧とつなぎ替え、ダークモード、音量、画面の明るさ |
| ★ 再生中の音楽 | Spotify・Apple Music・YouTube などの曲名とジャケット、再生/一時停止・曲送り・再生位置の変更。カプセルの上でホイールを回すと曲送り |
| ★ アプリのショートカット | よく使うアプリやフォルダを並べる（ドラッグ＆ドロップでも追加） |
| ★ 検索 | 近づけると検索欄が開き、そのまま打ち込める。ブラウザと検索エンジンを選べる |
| ★ クリップボード履歴 | コピーした文字を最大 60 件覚えておく。ピン留め・絞り込み。パスワード管理アプリからのコピーは記録しない |
| ★ メモ | 3 枚のメモ。自動で保存 |
| ★ 音量 | スクロールで調整、近づけるとスライダー、クリックでミュート |
| タイマー | 残り時間がカプセルに出る。終わると音と知らせ |
| システムモニター | CPU・メモリ・ディスク・バッテリー |
| 電卓 | 式をそのまま打ち込める（キーボードでも） |
| 天気 | 今の気温と 6 日間の予報（Open-Meteo） |
| 電源 | ロック・スリープ・再起動・シャットダウン・サインアウト（危ないものは 2 回押し） |
| クイックツール | スクショ・設定・タスク管理・ごみ箱・Wi-Fi などをすぐ開く |
| Todo | やることリスト。残りの数がカプセルに出る |
| カウントダウン | 旅行・試験・締め切りまでの残り日数をカプセルに表示 |
| 世界時計 | 世界の都市の今の時刻と、日本との時差 |
| 通貨換算 | 円・ウォン・ドルなどを今のレートで換算（欧州中央銀行のレート） |
| 単位換算 | 長さ・重さ・温度・面積・容量・速さ・データ量 |
| 翻訳 | 日本語・英語・韓国語などをすぐに翻訳（MyMemory） |
| QR コード | コピーした URL や文字を QR コードにして、スマホで開く |
| 最近のファイル | 最近開いたファイルやフォルダをすぐ開く |
| ネット速度 | 今のダウンロード・アップロードの速さをカプセルに表示 |

### デザイン

設定の「デザイン」で見た目を切り替えられます。アドオンの中のボタンやスライダーも同じデザインに揃います。

| デザイン | 見た目 |
| --- | --- |
| グラス | すりガラスの質感と光の演出（既定） |
| ミニマル | 光や質感のない、平らで静かな見た目。動きも控えめ |
| ネオン | 真っ暗な面の縁がアクセントカラーで光る（いつもダーク） |
| ソフト | 大きな丸みと、アクセントカラーをほんのり混ぜた柔らかい面 |
| クラシック | 昔の Windows のような、灰色の立体的な縁と四角い角 |

アクセントカラーは 青・紫・ピンク・オレンジ・緑・モノクロ から選べます。

技術：Tauri 2（Rust）＋ HTML/CSS/JS。メモリが軽く、UI とアドオンは Web 技術で書けます。

## ビルドの方法

### GitHub Actions（PC に開発環境を入れなくてよい）

1. このフォルダを GitHub のリポジトリに push する
2. 「Actions」タブで `build` が走り、終わると成果物 `SideDock-setup` にインストーラー（.exe）が入る
3. `v0.1.0` のようなタグを push すると、リリースにもインストーラーが載る

### 自分の PC でビルドする

必要なもの：[Rust](https://rustup.rs/)、Node.js 20 以上、Visual Studio Build Tools（「C++ によるデスクトップ開発」）

```powershell
npm install
npm run dev      # 開発用に起動
npm run build    # インストーラーを作る（src-tauri\target\release\bundle\nsis）
```

## 使い方

- 画面の上端（カプセルの位置）にカーソルを近づけると出てくる。離れると引っ込む
  - 隠れている間は上端に小さな印があり、カーソルが近づくほど明るく伸びる
- 検索や音量などは、アイコンにカーソルをとどめるとパネルが開く（クリックでもすぐ開く）
- 歯車から **設定**：出てくる場所（画面の上・下＝タスクバーの位置）、横の位置（左・中央・右）、表示のしかた（近づけたとき・常に表示）、反応する範囲（狭い・普通・広い）、テーマ、デザインとアクセントカラー、Windows の起動時に開く、Windows のタスクバーを隠す、アドオンの並べ替えと有効/無効
- 下に出すときは、パネルが上に向かって開きます
- Esc で引っ込める

### Windows のタスクバーの代わりに使う

設定の「**タスクバーの代わりにする**」をオンにすると、Windows のタスクバーがなくなり、SideDock が画面の下に常に表示されます（「タスクバー」「通知とトレイ」「時計」のアドオンも有効になります）。

- **トレイアイコン**：他のアプリのトレイアイコンを SideDock が自分で受け取って並べます。クリック・右クリック（メニュー）・ダブルクリックはそのアプリに伝わります。6 個目からは ∧ のパネルに入ります
- **画面の領域**：最大化したウィンドウが SideDock の下に潜り込まないよう、画面の下を確保します
- **スタートメニュー**：スタートボタンか Win キーで開きます。開いている間も Windows のタスクバーは出しません
- SideDock を終了すると、Windows のタスクバーとトレイは元に戻ります
- もし SideDock が落ちてタスクバーが隠れたままになったら、SideDock をもう一度起動するか、タスクマネージャー（Ctrl+Shift+Esc）で「エクスプローラー」を再起動すると戻ります

しくみ：アプリはトレイアイコンを "Shell_TrayWnd" という名前のウィンドウに登録します。SideDock も同じ名前の見えないウィンドウを作って手前に置き、各アプリに登録し直してもらいます。受け取った内容はエクスプローラーにもそのまま中継するので、SideDock を止めてもトレイは失われません（RetroBar などと同じ方式です）。

### 軽さのための工夫

- WebView2 のサイト分離を切り、アドオンの iframe を本体と同じ描画プロセスにまとめる。裏での通信・更新確認も止める
- カプセルやパネルが隠れたら描画そのものをやめる（中のアドオンの描画も止まる）。アニメーションも止める
- 閉じたパネルは 90 秒使わなければ破棄して、メモリを空ける（次に開くと作り直す）
- 音量・再生中の曲・ウィンドウの一覧・ネットワークは、Windows から「変わった」と知らせてもらう方式にし、定期的な取り直しをほぼなくす
- カーソルが画面の端から離れている間は、カーソルを調べる回数を約 11 回/秒に落とす
- 実行ファイルの場所やディスクの使用量など、重い問い合わせは覚えておく
- 隠れてしばらくすると、WebView2 に使っていないメモリを手放してもらう

## アドオンの作り方

`%APPDATA%\com.xelaods.sidedock\addons` に 1 アドオン 1 フォルダで置きます（設定の「アドオンフォルダを開く」で開けます）。
置いたら「アドオンを読み直す」を押し、設定で有効にします。自分で入れたアドオンは、有効にするときに権限の確認が出ます。

### フォルダの中身

```
my-addon/
├── manifest.json
├── icon.svg        … バーと設定に出るアイコン（形だけ使われ、色は本体に合わせて塗られる）
├── bar.html        … カプセルに並ぶウィジェット（任意、高さ 32px の横並び）
└── panel.html      … カプセルの下に開くパネル（任意）
```

### manifest.json

```json
{
  "id": "me.memo",
  "name": "メモ",
  "version": "1.0.0",
  "description": "ちょっとしたメモを残します",
  "icon": "icon.svg",
  "panel": { "entry": "panel.html", "title": "メモ", "width": 360 },
  "permissions": []
}
```

- `id`：英小文字・数字・`.` `-` `_` だけ。同じ id のアドオンを置くと同梱のものを上書きできる
- `bar` と `panel` はどちらか一方だけでも、両方でもよい
  - `bar` だけ：カプセルに常に表示される（例：時計）
  - `panel` だけ：カプセルにアイコンが出て、近づけるとパネルが開く（例：検索）
  - 両方：ウィジェットに近づけるとパネルが開く（例：音量）
- `panel.trigger` を `"manual"` にすると近づけても開かず、アドオンが `SideDock.ui.openPanel()` で開く（例：ショートカットの編集）
- `panel.width` はパネルの幅。高さは中身に合わせて自動で決まる（最大 380px）

### 権限

| 権限 | できること |
| --- | --- |
| `app` | `SideDock.app.launch()` でアプリ・ファイル・フォルダを開く、`app.icon()` でアイコンを取得 |
| `dialog` | `SideDock.dialog.pickFiles()` / `pickFolder()` でファイル選択画面を出す |
| `drop` | バーにドロップされたファイルを `drop` イベントで受け取る |
| `browser` | `SideDock.browser.list()` でブラウザ一覧、`browser.open(url, browserId)` でページを開く |
| `volume` | `SideDock.volume.get()` / `set(0〜1)` / `setMute(true/false)` |
| `network` | インターネットへの接続（ないと外部のサーバーへの通信はすべて止められる） |
| `media` | `SideDock.media.get()` / `art()` / `control("toggle" \| "next" \| "prev" …)` / `seek(秒)` |
| `system` | `SideDock.system.stats()` で CPU・メモリ・ディスク・バッテリー、`system.netSpeed()` で通信の速さ（`{ rx, tx }` バイト/秒） |
| `clipboard` | `SideDock.clipboard.write(文字)`（これだけは権限なしでも使える）、`SideDock.clipboard.list()` / `copy(id)` / `pin(id, true)` / `remove(id)` / `clear()`、`clipboardChanged` イベント |
| `power` | `SideDock.power.action("lock" \| "sleep" \| "restart" \| "shutdown" \| "signout")` |
| `settings` | `SideDock.net.status()`、`SideDock.wifi.list()` / `connect(ssid)`、`SideDock.radios.get()` / `set("wifi" \| "bluetooth", true)`、`SideDock.brightness.get()` / `set(0〜100)`、`SideDock.theme.getDark()` / `setDark(true)`、`networkChanged` イベント |
| `files` | `SideDock.files.recent(件数)` で最近開いたファイル・フォルダ（`{ name, path, at, isDir }`） |
| `windows` | `SideDock.windows.list()` / `icon(id, exe)` / `activate(id)` / `focus(id)` / `close(id)`、`SideDock.tray.list()` / `click(key, "left" \| "right" \| "double")`、`SideDock.shell.action("start" \| "quick" \| "notifications" \| "desktop" \| "taskview")`、`trayChanged` イベント |

権限がなくても、`SideDock.storage.get()` / `set(data)` でアドオン専用の保存領域（JSON）が使えます。

### HTML のひな形

```html
<!doctype html>
<html lang="ja">
<head>
<meta charset="utf-8">
<link rel="stylesheet" href="http://addon.localhost/_sdk/sidedock.css">
<script src="http://addon.localhost/_sdk/sidedock.js"></script>
</head>
<body>
  <textarea id="memo" class="sd-input" rows="10"></textarea>
<script type="module">
  await SideDock.ready;
  const memo = document.getElementById("memo");
  memo.value = (await SideDock.storage.get())?.text ?? "";
  memo.addEventListener("input", () => SideDock.storage.set({ text: memo.value }));
  SideDock.on("panelShown", () => memo.focus());
</script>
</body>
</html>
```

- 色は `var(--sd-ink)`、`var(--sd-accent)` などの CSS 変数を使うと、ライト/ダーク・デザイン・アクセントカラーに自動で合う（角の丸みは `var(--sd-radius)` `var(--sd-btn-radius)`。`:root[data-skin="neon"]` などで、デザインごとに見た目を変えることもできる）
- 部品のクラス：`sd-btn`（`primary` `ghost` `icon`）、`sd-input`、`sd-select`、`sd-range`、`sd-bar-btn`（カプセル用の丸いボタン 32×32）
- ウィジェットの幅・パネルの高さは中身に合わせて自動で伸び縮みする
- 要素に `data-hint="説明"` を付けると、カーソルを乗せたときカプセルの下に説明が出る
- UI の操作：`ui.openPanel()` `ui.closePanel()` `ui.togglePanel()` `ui.focus()`（キーボード入力を受け取る）`ui.dismiss()`（引っ込める）`ui.notify(文字, ミリ秒)`（カプセルを出して知らせる）
- manifest.json に `"defaultEnabled": false` と書くと、最初はオフの状態で一覧に出る
- 権限なしで使えるもの：`SideDock.battery.get()`（`{ level, charging }` か null）
- 変化の知らせ：`volumeChanged`（音量）・`mediaChanged`（曲）・`windowsChanged`（ウィンドウ）は、それぞれの権限があるアドオンに届く。定期的に取り直すより軽い
- `SideDock.poll(関数, { visible: 1000, hidden: 0 })` で、カプセルが出ている間だけ定期的に呼べる（隠れている間は止まり、出てきた瞬間に 1 回呼ばれる）。`SideDock.visible` で今出ているか分かる
- イベント：`init` `theme` `panelShown` `panelHidden` `storageChanged` `drop` `revealed` `concealed`
- アドオンはサンドボックス化された iframe で動き、本体とは `postMessage` 経由でしかやり取りできない。`alert()` などのダイアログは使えない

## 構成

```
src/                本体の UI（カプセル、パネル、設定、アドオンとの橋渡し）
src-tauri/src/
  lib.rs            起動、タスクトレイ、コマンド
  overlay.rs        画面上部への重ね表示、カーソルの監視、全画面の検出
  media.rs          再生中のメディア（Windows のメディアコントロール）
  sysmon.rs         CPU・メモリ・バッテリー、電源の操作
  clipboard.rs      クリップボードの履歴
  taskbar.rs        ウィンドウの一覧・切り替え、スタートメニュー、タスクバーを隠す
  tray.rs           トレイアイコンの受け取り・クリックの転送、画面の領域の確保
  quick.rs          ネットワーク・Wi-Fi・Bluetooth・明るさ・ダークモード
  log.rs            診断ログ
  system.rs         音量、アプリ起動、アイコン取得、ブラウザ一覧
  addons.rs         アドオンの読み込み、addon:// の配信、保存領域
  config.rs         設定ファイル
src-tauri/sdk/      アドオン用 SDK（sidedock.js / sidedock.css）
addons/             同梱アドオン（インストール先にコピーされる）
```

設定は `%APPDATA%\com.xelaods.sidedock\config.json`、アドオンの保存データは `%APPDATA%\com.xelaods.sidedock\addon-data\` にあります。

## 今後のアイデア

- ホットキー（例：Ctrl+Alt+Space）で検索を開く
- マルチモニター対応（表示するディスプレイを選ぶ）
- Edge Drop のような PC ⇔ スマホのファイル・メモ送受信アドオン
