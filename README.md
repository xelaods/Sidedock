# SideDock

画面の端に常駐する、アドオンで機能を増やせる Windows 用サイドバーです。
Edge にあった「スタンドアロン サイドバー」のように、ほかのアプリを使っている間も画面の右端（または左端）に表示され続けます。

- **アプリのショートカット**：よく使うアプリやフォルダをバーに並べる（ドラッグ＆ドロップでも追加）
- **検索**：選んだブラウザ・検索エンジンで検索、URL ならそのまま開く
- **音量**：バーの上でスクロールして調整、クリックでスライダー、中クリックでミュート
- **時計**：時刻と日付
- **アドオン**：上の機能もすべてアドオンとして作られていて、自分でも追加できる

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

- 一番下の歯車、またはタスクトレイのアイコンから **設定** を開く
- 設定でできること：左右の切り替え、画面の領域の確保（最大化したウィンドウとの重なり防止）、テーマ、Windows 起動時に開く、アドオンの並べ替えと有効/無効
- パネルは、ほかのウィンドウをクリックするか Esc で閉じる
- 全画面の動画やゲームが出ている間は、自動で最前面をやめる
- 終了はタスクトレイのメニューか、設定の一番下から

## アドオンの作り方

`%APPDATA%\com.xelaods.sidedock\addons` に 1 アドオン 1 フォルダで置きます（設定の「アドオンフォルダを開く」で開けます）。
置いたら「アドオンを読み直す」を押し、設定で有効にします。自分で入れたアドオンは、有効にするときに権限の確認が出ます。

### フォルダの中身

```
my-addon/
├── manifest.json
├── icon.svg        … バーと設定に出るアイコン（形だけ使われ、色は本体に合わせて塗られる）
├── bar.html        … バーに埋め込むウィジェット（任意）
└── panel.html      … クリックで開くパネル（任意）
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
  - `bar` だけ：バーに常に表示される（例：時計）
  - `panel` だけ：バーにアイコンが出て、押すとパネルが開く（例：検索）
  - 両方：バーのウィジェットから `SideDock.ui.openPanel()` でパネルを開く（例：音量）

### 権限

| 権限 | できること |
| --- | --- |
| `app` | `SideDock.app.launch()` でアプリ・ファイル・フォルダを開く、`app.icon()` でアイコンを取得 |
| `dialog` | `SideDock.dialog.pickFiles()` / `pickFolder()` でファイル選択画面を出す |
| `drop` | バーにドロップされたファイルを `drop` イベントで受け取る |
| `browser` | `SideDock.browser.list()` でブラウザ一覧、`browser.open(url, browserId)` でページを開く |
| `volume` | `SideDock.volume.get()` / `set(0〜1)` / `setMute(true/false)` |
| `network` | インターネットへの接続（ないと外部のサーバーへの通信はすべて止められる） |

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

- 色は `var(--sd-ink)`、`var(--sd-accent)` などの CSS 変数を使うと、ライト/ダークに自動で合う
- 部品のクラス：`sd-btn`（`primary` `ghost` `icon`）、`sd-input`、`sd-select`、`sd-range`、`sd-bar-btn`（バー用 40×40）
- バーのウィジェットは中身の高さに合わせて自動で伸び縮みする
- イベント：`init` `theme` `panelShown` `panelHidden` `storageChanged` `drop`
- アドオンはサンドボックス化された iframe で動き、本体とは `postMessage` 経由でしかやり取りできない。`alert()` などのダイアログは使えない

## 構成

```
src/                本体の UI（バー、パネル、設定、アドオンとの橋渡し）
src-tauri/src/
  lib.rs            起動、タスクトレイ、コマンド
  appbar.rs         画面端への固定（Windows の AppBar）
  system.rs         音量、アプリ起動、アイコン取得、ブラウザ一覧
  addons.rs         アドオンの読み込み、addon:// の配信、保存領域
  config.rs         設定ファイル
src-tauri/sdk/      アドオン用 SDK（sidedock.js / sidedock.css）
addons/             同梱アドオン（インストール先にコピーされる）
```

設定は `%APPDATA%\com.xelaods.sidedock\config.json`、アドオンの保存データは `%APPDATA%\com.xelaods.sidedock\addon-data\` にあります。

## 今後のアイデア

- ホットキー（例：Ctrl+Alt+Space）で検索パネルを開く
- マルチモニター対応（表示するディスプレイを選ぶ）
- 使っていないときは自動で隠す
- Edge Drop のような PC ⇔ スマホのファイル・メモ送受信アドオン
