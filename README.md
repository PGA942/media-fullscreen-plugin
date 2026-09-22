# media-fullscreen-plugin — QR コード検出(`viewer.overlay` サンプル)

全画面ビューワーで開いた画像の中から **QR コード**を見つけて枠を重ね、枠をタップすると
FOW で中身(リンク / 文字列)を表示して「コピー / ブラウザで開く / ミュート」できる
スロットプラグイン(契約 v0.13、[`docs/slots.md`](https://gitlab.com/orgate-question/rasq-sdk-sample/-/blob/main/docs/slots.md) §2.4)。

- id `sample.media-fullscreen` / kind `mediaViewer` / slots `["viewer.overlay"]`
- permissions `["fow.widget", "mute.words"]`
- 全画面アクションバーのボタン(v0.9): `actions[].slot = "viewer.overlay"` を 1 つ宣言(`toggle` / 「QR コード検出」/ `qr_code_scanner`)
- 依存: `image`(PNG / JPEG 復号)+ `rqrr`(QR 検出・復号)。ネットワークは使わない。

## このリポジトリの形(`rasq` ワークスペース)

```
rasq.toml            schema = "rasq.workspace/1" / [author] namespace = "sample"
                     [[plugins]] name = "media-fullscreen-plugin" / slot = "viewer.overlay" / id = "sample.media-fullscreen"
Cargo.toml           rasq CLI が rasq.toml から作り直す(手で直さない)
kit/                 rasq-sdk-sample の rust/rasq-plugin-kit の写し
plugins/media-fullscreen-plugin/
  Cargo.toml         crate-type = ["cdylib", "rlib"] / rasq-plugin-kit = { path = "../../kit" }
  manifest.json      plugin.manifest の答えそのもの
  src/lib.rs         include_str!("../manifest.json") + handle の match + #[cfg(test)]
  tests/fixtures/    rasq call 用の画像(qr-url.png)
dist/                rasq build --raw --no-sign の出力(.rasqpkg と生の .wasm)
```

マニフェストは **`plugins/media-fullscreen-plugin/manifest.json` の 1 か所**。`src/lib.rs` は
`include_str!` でそれをそのまま返すので、`rasq check` の往復検査(`plugin.manifest` ⇔ `manifest.json`)が常に一致する。

## 契約(`docs/slots.md` §2.4 `viewer.overlay`)

| fn | args | 戻り |
|---|---|---|
| `plugin.manifest` | `{}` | `manifest.json` そのもの |
| `viewer.open` | `{token, url, mimeType, width, height, bytesB64?}` | `{root: Stack[Box…]}` / QR が無ければ `{}` |
| `qr.<i>` | `{}`(枠の `on.tap`) | `{opened}` — `fow.open`「QR コード」+ `viewer.overlay.set` で選択枠を強調 |
| `qr.copy` | `{}` | `{}` — `clipboard.set` + `toast` |
| `qr.browser` | `{}` | `{}` — `url.open` + `toast`(http(s) のときだけボタンが出る) |
| `qr.mute` | `{}` | `{}` — `mute.add` + `toast`(ホストが確認してから追加) |
| `qr.close` / `qr.closed` | `{}` | `{}` — FOW を閉じて枠を元の色に |
| `viewer.close` | `{token}` | `{}` — FOW を閉じて状態を捨てる |

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
- ボタンの状態(処理中 / 検出あり / 検出なし / 失敗)はホストが出す(D-246)。押したときの `call` は無い
  — ホストが ON / OFF を持ち、ON のときだけ `viewer.open` に画像を渡す。

## ビルドと確認

`rasq` CLI(`rasq-sdk-sample/rust/rasq-cli`)をパスに置いて、リポジトリのルートで:

```sh
rasq check                 # wasm32 ビルド → export 検査 → plugin.manifest 往復 → スキーマ / 登録簿 → cargo test
rasq test                  # cargo test だけ
rasq build --raw --no-sign # dist/sample.media-fullscreen-0.1.0.rasqpkg と dist/media_fullscreen_plugin.wasm
rasq inspect dist/sample.media-fullscreen-0.1.0.rasqpkg
```

```
$ rasq check
$ cargo build --release --target wasm32-unknown-unknown
plugins/media-fullscreen-plugin:
  ok: target/wasm32-unknown-unknown/release/media_fullscreen_plugin.wasm (688 KB)
1 targets, 0 errors, 0 warnings
$ cargo test
... test result: ok. 10 passed; 0 failed

$ rasq inspect dist/sample.media-fullscreen-0.1.0.rasqpkg
bundle: rasq.bundle/1 sdk 0.1.0
root:   sample.media-fullscreen 0.1.0 (mediaViewer)
files:  bundle.json / manifest.json / media_fullscreen_plugin.wasm / SHA512SUMS
目次と中身: ok
```

### `rasq call` — wasm をそのまま叩く

`--args` の `"@b64:<path>"` はそのファイルの base64 に置き換わる。QR 入りの画像は
`plugins/media-fullscreen-plugin/tests/fixtures/qr-url.png`(600×400、`https://example.com/path?x=1`)。

```sh
cat > /tmp/open.json <<'JSON'
{"token":"tok-1","url":"https://example.com/a.png","mimeType":"image/png",
 "width":600,"height":400,
 "bytesB64":"@b64:plugins/media-fullscreen-plugin/tests/fixtures/qr-url.png"}
JSON

rasq call media-fullscreen-plugin viewer.open --args @/tmp/open.json \
  --reply 'fow.open={"opened":true}' \
  --then qr.0 --then qr.copy --then qr.browser --then qr.mute --then viewer.close
```

| call | 戻り / ホスト関数 |
|---|---|
| `viewer.open` | `{"root":{"t":"Stack","k":"qr-overlay","c":[{"t":"Box","k":"qr-0","on":{"tap":"qr.0"},"p":{"left":284,"top":94,"width":240,"height":240,"borderColor":"#00e5ff",…}}]}}` |
| `qr.0` | `{"opened":true}` / `fow.open`(タイトル「QR コード」、本文に URL とホスト名)+ `viewer.overlay.set`(枠が `#ffd600` に) |
| `qr.copy` | `{}` / `clipboard.set {"text":"https://example.com/path?x=1"}` + `toast` |
| `qr.browser` | `{}` / `url.open {"url":"https://example.com/path?x=1"}` + `toast` |
| `qr.mute` | `{}` / `mute.add {"text":"https://example.com/path?x=1"}` + `toast` |
| `viewer.close` | `{}` / `fow.close {"id":"qr-link"}` |

`plugin.manifest` は `manifest.json` と一致する(`rasq call media-fullscreen-plugin plugin.manifest`)。

## 端末での試し方(rasq-client)

1. `rasq build --raw --no-sign` のあと、出力の最後の行が出す `dist/` の**絶対パス**を控える。
2. rasq-client のカタログ →「WASMプラグイン試験 (dev)」→ `.wasm` のパスからインポート → **スロットに登録**
   (権限 `fow.widget` / `mute.words` を許可)。
3. TL などで QR コードが写っている画像(PNG / JPEG)を全画面ビューワーで開き、ヘッダーの
   全画面アクションバーの「QR コード検出」を ON にする。
4. QR の上に水色の枠が出るのでタップ → 下に FOW「QR コード」が出る。
5. 「コピー」「ブラウザで開く」「このリンクをミュート」を押すとトーストが出る
   (ミュートはホストの確認ダイアログ経由)。「閉じる」または ✕ で FOW を閉じる。
6. ビューワーを閉じると FOW も閉じる。

## 掲載(審査)に出す

`rasq build` が出す `dist/sample.media-fullscreen-0.1.0.rasqpkg` が**そのまま掲載できる形**
(zstd tar + `bundle.json` `rasq.bundle/1` + `SHA512SUMS`)。

```sh
rasq publish --dry-run     # 出す前の確認
```

- 開発者鍵と `dist/*.sig.json` は**コミットしない**(`.gitignore` の `*.sig.json`)。
  CI は `--no-sign` で組む。
- 契約の正典は [`CONTRACT.md`](CONTRACT.md) が指す `rasq-sdk-sample/docs/slots.md`。
