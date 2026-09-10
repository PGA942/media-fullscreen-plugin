# media-fullscreen-plugin — QR コード検出(`viewer.overlay` サンプル)

全画面ビューワーで開いた画像の中から **QR コード**を見つけて枠を重ね、枠をタップすると
FOW で中身(リンク / 文字列)を表示して「コピー / ブラウザで開く / ミュート」できる
スロットプラグイン(`CONTRACT.md` §2.4)。

- id `sample.media-fullscreen` / kind `mediaViewer` / slots `["viewer.overlay"]`
- permissions `["fow.widget", "mute.words"]`
- 依存: `image`(PNG / JPEG 復号)+ `rqrr`(QR 検出・復号)。ネットワークは使わない。

## 流れ

```
ホスト                                   プラグイン
viewer.open {token, mimeType, width,     → base64 を復号 → 輝度化 → 長辺 800 に縮小
             height, bytesB64}             → rqrr で検出 → 枠を width/height 画素系に戻す
                                         ← {root: Stack[ Box(qr-0, left/top/width/height, on.tap = qr.0), … ]}
                                            (QR が無い / 画像でない / bytesB64 が無い → {})
枠をタップ → call("qr.0")                → fow.open「QR コード」(+ viewer.overlay.set で選択枠を黄色に)
  FOW: コピー          → call("qr.copy")     → clipboard.set {text} + toast
       ブラウザで開く  → call("qr.browser")  → url.open {url} + toast(http/https のときだけ表示)
       …をミュート     → call("qr.mute")     → mute.add {text} + toast(ホストが確認してから追加)
       閉じる          → call("qr.close")    → fow.close + 枠を元の色に
viewer.close {token}                     → FOW を閉じて状態を捨てる → {}
```

- 中身が `http://` / `https://` で始まれば「リンク」として青字(`#1e88e5`)+ホスト名を出し、
  それ以外は「文字列(URL ではありません)」として生テキストを出す。
- 座標系はホストが `viewer.open` で渡す `width/height`(ホストが長辺 1024 に縮小した後の画素数)。
  表示倍率への拡縮はホスト側。
- 安全側: 4096×4096 画素を超える画像は復号せず `{}`、壊れた base64 / 画像は `D01`。枠は最大 16 個。

## ビルド / テスト

```sh
cargo test -p media-fullscreen-plugin                                       # native テスト(qrcode で QR 画像を生成して検出)
cargo build --release --target wasm32-unknown-unknown -p media-fullscreen-plugin
# → target/wasm32-unknown-unknown/release/media_fullscreen_plugin.wasm(約 0.7 MB)
```

## 端末での試し方

1. rasq-client のカタログ →「WASMプラグイン試験 (dev)」→ `.wasm` を読み込む → **スロットに登録**
   (権限 `fow.widget` / `mute.words` を許可)。
2. TL などで QR コードが写っている画像(PNG / JPEG)を全画面ビューワーで開く。
3. QR の上に水色の枠が出るのでタップ → 下に FOW「QR コード」が出る。
4. 「コピー」「ブラウザで開く」「このリンクをミュート」を押すとトーストが出る
   (ミュートはホストの確認ダイアログ経由)。「閉じる」または ✕ で FOW を閉じる。
5. ビューワーを閉じると FOW も閉じる。

---

## このリポジトリの形

- クレートはルート(`src/`)、共通部品 [`rasq-plugin-kit`](kit) は `kit/` に同梱(開発用モノレポから写している)。
- 契約は [`CONTRACT.md`](CONTRACT.md)(スロットプラグイン契約 v0.1)。
- ビルド済み `.wasm` は [`dist/`](dist)。`cargo build --release --target wasm32-unknown-unknown` で更新。
