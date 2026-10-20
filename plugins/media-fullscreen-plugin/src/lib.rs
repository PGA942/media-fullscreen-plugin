//! media-fullscreen-plugin — 全画面ビューワーの重ね(スロット `viewer.overlay`)サンプル。
//!
//! 画像の中の **QR コード**を見つけて枠(`Box`)を重ね、タップすると FOW で中身を出す:
//!
//! | 項目 | 入口 |
//! |---|---|
//! | マニフェスト | `plugin.manifest` |
//! | 全画面が開いた(画像 bytes 付き) | `viewer.open` → `image` で復号 → `rqrr` で検出 → `{root: Stack[Box…]}` |
//! | 枠タップ | `qr.<i>` → FOW「QR コード」(`fow.open`)+ 選択枠を `viewer.overlay.set` で強調 |
//! | FOW のボタン | `qr.copy` / `qr.browser` / `qr.mute` / `qr.close` → `clipboard.set` / `url.open` / `mute.add` / `fow.close`(+ `toast`) |
//! | 全画面が閉じた | `viewer.close` → FOW を閉じて状態を捨てる |
//!
//! 契約: 契約 v0.14、`../../CONTRACT.md` → SDK の `docs/slots.md` §2.4(viewer.overlay)/ §3(`Box` ノード、
//! Stack 内の `left/top`)/ §4(ホスト関数)。全画面アクションバーのボタン宣言(v0.9)は `manifest.json` の
//! `actions[].slot = "viewer.overlay"`。初期状態(v0.14 temp、§2.11)は同じ項目の `auto: true`(全画面が開いた瞬間にオン)。
//! ビルド・確認: `rasq check` / `rasq test` / `rasq call media-fullscreen-plugin viewer.open …` / `rasq build --raw`

use base64::Engine;
use rasq_plugin_kit::{
    err, fow_spec, host, i18n, rasq_plugin, rrl,
    serde_json::{json, Value},
    unsupported, Surface,
};
use std::sync::Mutex;

mod text;
use text::TEXT;

/// `plugin.manifest` の答え = `manifest.json`(名前 / 版 / 説明 / 権限 / ボタン宣言はあちらを直す)。
fn manifest() -> Value {
    rasq_plugin_kit::serde_json::from_str(include_str!("../manifest.json")).expect("manifest.json")
}

/// 検出時にこの長辺まで縮小してから探す(速度のため。座標は元の `width/height` 画素系に戻す)。
/// ホストは長辺 1024 で渡す約束なので、ホストから来た画像は**縮めない**(D-519)。以前は 800 で、毎回 1024 → 800 の
/// 縮小(Triangle)をしていた。wasm のインタープリタではこれが一番重く(復号・検出を合わせたぶんより重い)、
/// 縮めたぶん小さな QR を見落としやすくもなっていた。
const DETECT_MAX_SIDE: u32 = 1024;
/// これより大きい画像は復号せずに諦める(wasm の線形メモリを守る。ホストは長辺 1024 で渡す約束)。
const MAX_SOURCE_PIXELS: u64 = 4096 * 4096;
/// 見つけた QR の上限(枠の数)。
const MAX_CODES: usize = 16;

const COLOR_BOX: &str = "#00e5ff";
const COLOR_BOX_FILL: &str = "#00e5ff22";
const COLOR_BOX_SELECTED: &str = "#ffd600";
const COLOR_BOX_SELECTED_FILL: &str = "#ffd60033";
const COLOR_LINK: &str = "#1e88e5";

// ---------------------------------------------------------------------------
// 状態: 直近の viewer.open で見つけた QR と、開いている FOW。
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
struct Detected {
    /// QR の中身(文字列)。
    text: String,
    /// 枠(`viewer.open` の `width/height` 画素系)。
    left: f64,
    top: f64,
    width: f64,
    height: f64,
}

#[derive(Default)]
struct State {
    token: String,
    codes: Vec<Detected>,
    /// FOW で表示中の QR の添字。
    selected: Option<usize>,
    fow_open: bool,
}

static STATE: Mutex<State> = Mutex::new(State { token: String::new(), codes: Vec::new(), selected: None, fow_open: false });
static FOW: Surface = Surface::new("qr-link");

fn state() -> std::sync::MutexGuard<'static, State> {
    STATE.lock().unwrap_or_else(|p| p.into_inner())
}

// ---------------------------------------------------------------------------
// 本体ツリー(このスロットでは使われない — 最小の Column)。
// ---------------------------------------------------------------------------

fn root() -> Value {
    rrl::column("root", vec![rrl::text("hello", TEXT.t(&i18n::current(), "root.hello"))])
}

// ---------------------------------------------------------------------------
// viewer.open: 画像 → QR 検出 → 重ねる RRL
// ---------------------------------------------------------------------------

fn viewer_open(args: &Value) -> Result<Value, Value> {
    if !args.is_object() {
        return Err(err("D01", "viewer.open: args must be an object"));
    }
    let token = args["token"].as_str().unwrap_or("").to_string();

    // 前回の FOW が残っていれば閉じてから状態を作り直す。
    {
        let mut st = state();
        if st.fow_open {
            FOW.close(json!({}));
        }
        *st = State { token: token.clone(), ..State::default() };
    }

    let mime = args["mimeType"].as_str().unwrap_or("").to_ascii_lowercase();
    let format = match mime.as_str() {
        "image/png" => image::ImageFormat::Png,
        "image/jpeg" | "image/jpg" => image::ImageFormat::Jpeg,
        _ => return Ok(json!({})),
    };
    let Some(b64) = args["bytesB64"].as_str() else {
        return Ok(json!({}));
    };
    let bytes = decode_base64(b64).ok_or_else(|| err("D01", "viewer.open: bytesB64 is not valid base64"))?;

    // 寸法だけ先に読んで、大きすぎる画像は復号しない(メモリ保護)。
    let reader = image::ImageReader::with_format(std::io::Cursor::new(&bytes), format);
    let (src_w, src_h) = reader.into_dimensions().map_err(|e| err("D01", format!("viewer.open: cannot read image header: {e}")))?;
    if src_w == 0 || src_h == 0 || (src_w as u64) * (src_h as u64) > MAX_SOURCE_PIXELS {
        return Ok(json!({}));
    }
    let img = image::load_from_memory_with_format(&bytes, format).map_err(|e| err("D01", format!("viewer.open: cannot decode image: {e}")))?;

    // 枠の座標系はホストが args で言う width/height(無ければ復号した画素数)。
    let out_w = args["width"].as_f64().filter(|v| *v > 0.0).unwrap_or(src_w as f64);
    let out_h = args["height"].as_f64().filter(|v| *v > 0.0).unwrap_or(src_h as f64);

    let codes = detect_codes(to_luma(img), out_w, out_h);
    if codes.is_empty() {
        return Ok(json!({}));
    }
    let root = overlay_root(&codes, None);
    state().codes = codes;
    Ok(json!({"root": root}))
}

/// base64(標準 / URL-safe、`data:` プレフィックスや空白も許す)。
fn decode_base64(s: &str) -> Option<Vec<u8>> {
    let s = match s.find("base64,") {
        Some(i) if s.starts_with("data:") => &s[i + 7..],
        _ => s,
    };
    let cleaned: String = s.chars().filter(|c| !c.is_whitespace()).collect();
    base64::engine::general_purpose::STANDARD
        .decode(&cleaned)
        .or_else(|_| base64::engine::general_purpose::STANDARD_NO_PAD.decode(&cleaned))
        .or_else(|_| base64::engine::general_purpose::URL_SAFE.decode(&cleaned))
        .or_else(|_| base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(&cleaned))
        .ok()
}

/// 輝度(グレー)にする。RGB / RGBA は整数だけで(`to_luma8` は画素ごとに浮動小数の計算をするので、wasm の
/// インタープリタでは重い。ホストは RGBA の PNG で渡す)。係数は `image` と同じ Rec.709(0.2126 / 0.7152 / 0.0722)。
fn to_luma(img: image::DynamicImage) -> image::GrayImage {
    fn from_channels(w: u32, h: u32, raw: &[u8], channels: usize) -> image::GrayImage {
        let mut out = Vec::with_capacity((w as usize) * (h as usize));
        for px in raw.chunks_exact(channels) {
            let (r, g, b) = (px[0] as u32, px[1] as u32, px[2] as u32);
            out.push(((r * 2126 + g * 7152 + b * 722 + 5000) / 10000) as u8);
        }
        image::GrayImage::from_raw(w, h, out).expect("buffer size matches")
    }
    match img {
        image::DynamicImage::ImageLuma8(l) => l,
        image::DynamicImage::ImageRgba8(rgba) => from_channels(rgba.width(), rgba.height(), rgba.as_raw(), 4),
        image::DynamicImage::ImageRgb8(rgb) => from_channels(rgb.width(), rgb.height(), rgb.as_raw(), 3),
        other => other.to_luma8(),
    }
}

/// 輝度画像から QR を探し、枠を `out_w × out_h` の画素系に写して返す。
fn detect_codes(luma: image::GrayImage, out_w: f64, out_h: f64) -> Vec<Detected> {
    let (w, h) = luma.dimensions();
    if w < 8 || h < 8 {
        return vec![];
    }
    // 長辺 DETECT_MAX_SIDE まで縮小(拡大はしない)。
    let longest = w.max(h);
    let luma = if longest > DETECT_MAX_SIDE {
        let nw = ((w as u64 * DETECT_MAX_SIDE as u64) / longest as u64).max(1) as u32;
        let nh = ((h as u64 * DETECT_MAX_SIDE as u64) / longest as u64).max(1) as u32;
        image::imageops::resize(&luma, nw, nh, image::imageops::FilterType::Triangle)
    } else {
        luma
    };
    let (dw, dh) = luma.dimensions();
    if (dw as u64) * (dh as u64) > (DETECT_MAX_SIDE as u64) * (DETECT_MAX_SIDE as u64) {
        return vec![]; // 念のため(縮小後に大きいことはないが、仕事量を上から抑える)
    }
    // 縮小後の画素 → 出力画素系。
    let sx = out_w / dw as f64;
    let sy = out_h / dh as f64;

    let mut prepared = rqrr::PreparedImage::prepare(luma);
    let mut out = Vec::new();
    for grid in prepared.detect_grids() {
        if out.len() >= MAX_CODES {
            break;
        }
        let Ok((_meta, text)) = grid.decode() else { continue };
        if text.is_empty() {
            continue;
        }
        let xs = grid.bounds.iter().map(|p| p.x as f64);
        let ys = grid.bounds.iter().map(|p| p.y as f64);
        let (min_x, max_x) = xs.fold((f64::MAX, f64::MIN), |(lo, hi), v| (lo.min(v), hi.max(v)));
        let (min_y, max_y) = ys.fold((f64::MAX, f64::MIN), |(lo, hi), v| (lo.min(v), hi.max(v)));
        let left = (min_x * sx).clamp(0.0, out_w);
        let top = (min_y * sy).clamp(0.0, out_h);
        let right = (max_x * sx).clamp(0.0, out_w);
        let bottom = (max_y * sy).clamp(0.0, out_h);
        let (bw, bh) = (right - left, bottom - top);
        if bw < 1.0 || bh < 1.0 {
            continue;
        }
        out.push(Detected { text, left: left.round(), top: top.round(), width: bw.round(), height: bh.round() });
    }
    out
}

/// 重ねる RRL: `Stack` の子に QR ごとの `Box`(`left/top/width/height`、`on.tap = qr.<i>`)。
fn overlay_root(codes: &[Detected], selected: Option<usize>) -> Value {
    let boxes = codes
        .iter()
        .enumerate()
        .map(|(i, d)| {
            let (border, fill) = if selected == Some(i) { (COLOR_BOX_SELECTED, COLOR_BOX_SELECTED_FILL) } else { (COLOR_BOX, COLOR_BOX_FILL) };
            let props = json!({
                "width": d.width, "height": d.height,
                "borderColor": border, "borderWidth": 3, "radius": 6, "color": fill,
            });
            let mut node = rrl::positioned(rrl::boxed(&format!("qr-{i}"), props, vec![]), d.left, d.top);
            node["on"] = json!({"tap": format!("qr.{i}")});
            node
        })
        .collect();
    rrl::stack("qr-overlay", boxes)
}

// ---------------------------------------------------------------------------
// qr.<i>: FOW「QR コード」
// ---------------------------------------------------------------------------

/// `http(s)://` で始まれば `Some(host)`。
fn url_host(text: &str) -> Option<String> {
    let lower = text.to_ascii_lowercase();
    let rest = if lower.starts_with("https://") {
        &text[8..]
    } else if lower.starts_with("http://") {
        &text[7..]
    } else {
        return None;
    };
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    let host = authority.rsplit('@').next().unwrap_or(authority);
    if host.is_empty() || host.chars().any(char::is_whitespace) {
        return None;
    }
    Some(host.to_string())
}

/// 表示用に長い文字列を切る。
fn ellipsize(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        s.to_string()
    } else {
        let head: String = s.chars().take(max_chars).collect();
        format!("{head}…")
    }
}

fn fow_root(loc: &str, d: &Detected) -> Value {
    let host_name = url_host(&d.text);
    let mut children = vec![];
    match &host_name {
        Some(h) => {
            children.push(rrl::styled_text("qr-kind", TEXT.t(loc, "fow.kindLink"), 12.0, false, Some("#888888")));
            let mut link = rrl::styled_text("qr-content", ellipsize(&d.text, 400), 15.0, false, Some(COLOR_LINK));
            link["on"] = json!({"tap": "qr.browser"});
            children.push(link);
            children.push(rrl::styled_text("qr-host", TEXT.f(loc, "fow.host", &[("host", h)]), 12.0, false, Some("#888888")));
        }
        None => {
            children.push(rrl::styled_text("qr-kind", TEXT.t(loc, "fow.kindText"), 12.0, false, Some("#888888")));
            children.push(rrl::styled_text("qr-content", ellipsize(&d.text, 400), 15.0, false, None));
        }
    }
    children.push(rrl::spacer("qr-sp", 8.0));
    children.push(rrl::button("qr-copy", TEXT.t(loc, "fow.copy"), "qr.copy"));
    if host_name.is_some() {
        children.push(rrl::button("qr-browser", TEXT.t(loc, "fow.openBrowser"), "qr.browser"));
    }
    let mute_label = if host_name.is_some() { TEXT.t(loc, "fow.muteLink") } else { TEXT.t(loc, "fow.muteText") };
    children.push(rrl::button("qr-mute", mute_label, "qr.mute"));
    children.push(rrl::button("qr-close", TEXT.t(loc, "fow.close"), "qr.close"));
    rrl::column("qr-root", children)
}

fn qr_tap(index: usize) -> Result<Value, Value> {
    let (d, token, root) = {
        let mut st = state();
        let Some(d) = st.codes.get(index).cloned() else {
            return Err(err("D01", format!("qr.{index}: no such code")));
        };
        st.selected = Some(index);
        (d, st.token.clone(), overlay_root(&st.codes, Some(index)))
    };
    let loc = i18n::current();
    let opened = FOW.open(fow_spec(TEXT.t(&loc, "fow.title"), 0.4), fow_root(&loc, &d), Some("qr.closed"), json!({}));
    state().fow_open = opened;
    // 選ばれた枠を強調(古いホストでは E01 が返るだけ)。
    host::viewer_overlay_set(&token, root);
    Ok(json!({"opened": opened}))
}

fn selected_code() -> Result<Detected, Value> {
    let st = state();
    st.selected.and_then(|i| st.codes.get(i).cloned()).ok_or_else(|| err("D01", "no QR code selected"))
}

fn qr_copy() -> Result<Value, Value> {
    let d = selected_code()?;
    host::clipboard_set(&d.text);
    host::toast(TEXT.t(&i18n::current(), "toast.copied"));
    Ok(json!({}))
}

fn qr_browser() -> Result<Value, Value> {
    let d = selected_code()?;
    if url_host(&d.text).is_none() {
        return Err(err("D01", "not an http(s) URL"));
    }
    host::url_open(&d.text);
    host::toast(TEXT.t(&i18n::current(), "toast.opening"));
    Ok(json!({}))
}

fn qr_mute() -> Result<Value, Value> {
    let d = selected_code()?;
    host::mute_add(&d.text); // ホストが mute.words の確認を出してから追加する
    host::toast(TEXT.t(&i18n::current(), "toast.muteRequested"));
    Ok(json!({}))
}

fn close_fow() {
    let mut st = state();
    if st.fow_open {
        FOW.close(json!({}));
        st.fow_open = false;
    }
    st.selected = None;
}

fn qr_close() -> Result<Value, Value> {
    close_fow();
    let (token, root) = {
        let st = state();
        (st.token.clone(), overlay_root(&st.codes, None))
    };
    if root["c"].as_array().is_some_and(|c| !c.is_empty()) {
        host::viewer_overlay_set(&token, root);
    }
    Ok(json!({}))
}

/// FOW の onClosed(ユーザーが ✕ / 余白タップで閉じた)。
fn qr_closed() -> Result<Value, Value> {
    let mut st = state();
    st.fow_open = false;
    st.selected = None;
    Ok(json!({}))
}

fn viewer_close() -> Result<Value, Value> {
    close_fow();
    *state() = State::default();
    Ok(json!({}))
}

// ---------------------------------------------------------------------------
// call の振り分け
// ---------------------------------------------------------------------------

fn handle(fn_name: &str, args: Value) -> Result<Value, Value> {
    match fn_name {
        "plugin.manifest" => Ok(manifest()),
        "viewer.open" => viewer_open(&args),
        "viewer.close" => viewer_close(),
        "qr.copy" => qr_copy(),
        "qr.browser" => qr_browser(),
        "qr.mute" => qr_mute(),
        "qr.close" => qr_close(),
        "qr.closed" => qr_closed(),
        "lifecycle" => Ok(json!({})),
        other => match other.strip_prefix("qr.").and_then(|s| s.parse::<usize>().ok()) {
            Some(i) => qr_tap(i),
            None => Err(unsupported(other)),
        },
    }
}

rasq_plugin! { scope: "sample.media-fullscreen:main", root: root, call: handle }

// ---------------------------------------------------------------------------
// native テスト: cargo test -p media-fullscreen-plugin
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use image::{ImageBuffer, Luma, Rgba};
    use rasq_plugin_kit::{set_host_hook, take_host_log};
    use std::io::Cursor;

    /// STATE はグローバルなので、テストは直列に走らせる。
    static TEST_LOCK: Mutex<()> = Mutex::new(());
    fn lock() -> std::sync::MutexGuard<'static, ()> {
        TEST_LOCK.lock().unwrap_or_else(|p| p.into_inner())
    }

    const URL: &str = "https://example.com/path?x=1";
    const QR_SIZE: u32 = 300;
    const OFF_X: u32 = 250;
    const OFF_Y: u32 = 60;
    const CANVAS_W: u32 = 600;
    const CANVAS_H: u32 = 400;

    /// qrcode で QR を作り、静止ゾーン付きの QR_SIZE×QR_SIZE の Luma 画像に描く。
    /// 戻り: (画像, 暗モジュールの実際の外接矩形 (left, top, right, bottom))。
    fn qr_luma(text: &str) -> (ImageBuffer<Luma<u8>, Vec<u8>>, (u32, u32, u32, u32)) {
        let code = qrcode::QrCode::new(text.as_bytes()).unwrap();
        let n = code.width() as u32;
        let colors = code.to_colors();
        let quiet = 4;
        let module = QR_SIZE / (n + quiet * 2);
        assert!(module >= 2, "QR too dense for the canvas");
        let origin = (QR_SIZE - module * n) / 2;
        let mut img = ImageBuffer::from_pixel(QR_SIZE, QR_SIZE, Luma([255u8]));
        for y in 0..n {
            for x in 0..n {
                if colors[(y * n + x) as usize] == qrcode::Color::Dark {
                    for dy in 0..module {
                        for dx in 0..module {
                            img.put_pixel(origin + x * module + dx, origin + y * module + dy, Luma([0u8]));
                        }
                    }
                }
            }
        }
        (img, (origin, origin, origin + module * n, origin + module * n))
    }

    fn png_b64(img: &image::DynamicImage) -> String {
        let mut buf = Cursor::new(Vec::new());
        img.write_to(&mut buf, image::ImageFormat::Png).unwrap();
        base64::engine::general_purpose::STANDARD.encode(buf.into_inner())
    }

    /// 600×400 の白い RGBA 画像に QR を (OFF_X, OFF_Y) で置いて PNG/base64 にする。
    /// 戻り: (base64, 期待する枠 (left, top, width, height))。
    fn qr_canvas_b64(text: &str) -> (String, (f64, f64, f64, f64)) {
        let (qr, (l, t, r, b)) = qr_luma(text);
        let mut canvas = ImageBuffer::from_pixel(CANVAS_W, CANVAS_H, Rgba([255u8, 255, 255, 255]));
        for (x, y, p) in qr.enumerate_pixels() {
            let v = p.0[0];
            canvas.put_pixel(OFF_X + x, OFF_Y + y, Rgba([v, v, v, 255]));
        }
        let b64 = png_b64(&image::DynamicImage::ImageRgba8(canvas));
        let expect = ((OFF_X + l) as f64, (OFF_Y + t) as f64, (r - l) as f64, (b - t) as f64);
        (b64, expect)
    }

    fn open_args(b64: &str, mime: &str) -> Value {
        json!({"token": "tok-1", "url": "https://example.com/a.png", "mimeType": mime, "width": CANVAS_W, "height": CANVAS_H, "bytesB64": b64})
    }

    #[test]
    fn manifest_is_declared() {
        let _g = lock();
        let m = call_native("plugin.manifest", json!({})).unwrap();
        assert_eq!(m["id"], json!("sample.media-fullscreen"));
        assert_eq!(m["kind"], json!("mediaViewer"));
        assert_eq!(m["slots"], json!(["viewer.overlay"]));
        assert_eq!(m["permissions"], json!(["fow.widget", "mute.words"]));
    }

    /// `plugin.manifest` は `manifest.json` そのもの(`rasq check` の往復検査と同じ突き合わせ)。
    #[test]
    fn manifest_matches_manifest_json() {
        let _g = lock();
        let m = call_native("plugin.manifest", json!({})).unwrap();
        let file: Value = rasq_plugin_kit::serde_json::from_str(include_str!("../manifest.json")).unwrap();
        assert_eq!(m, file);
    }

    /// 全画面アクションバーのボタン宣言(v0.9): `actions[].slot` は宣言したスロットのものだけ。
    /// 初期状態(v0.14 temp): `auto: true` = 全画面が開いた瞬間にオン(ホスト既定と同じだが、宣言で明示する)。
    #[test]
    fn declared_actions_are_viewer_overlay() {
        let _g = lock();
        let m = call_native("plugin.manifest", json!({})).unwrap();
        let actions = m["actions"].as_array().expect("actions が宣言されていない");
        assert!(!actions.is_empty());
        for a in actions {
            assert_eq!(a["slot"], json!("viewer.overlay"), "actions[].slot: {a}");
            assert!(a["id"].is_string() && a["label"].is_string(), "id / label が要る: {a}");
            assert_eq!(a["auto"], json!(true), "actions[].auto(初期状態)を明示する: {a}");
        }
    }

    #[test]
    fn detects_qr_then_fow_and_actions() {
        let _g = lock();
        let (b64, (el, et, ew, eh)) = qr_canvas_b64(URL);
        set_host_hook(|f, _| if f == "fow.open" { json!({"opened": true}) } else { json!({}) });
        take_host_log();

        // 1. viewer.open → Stack に Box が 1 つ、位置は QR の外接矩形 ±20px
        let r = call_native("viewer.open", open_args(&b64, "image/png")).unwrap();
        let root = &r["root"];
        assert_eq!(root["t"], json!("Stack"), "root: {root}");
        let boxes = root["c"].as_array().expect("children");
        assert_eq!(boxes.len(), 1, "exactly one Box: {root}");
        let b = &boxes[0];
        assert_eq!(b["t"], json!("Box"));
        assert_eq!(b["on"]["tap"], json!("qr.0"));
        let p = &b["p"];
        let tol = 20.0;
        for (name, got, want) in [
            ("left", p["left"].as_f64().unwrap(), el),
            ("top", p["top"].as_f64().unwrap(), et),
            ("width", p["width"].as_f64().unwrap(), ew),
            ("height", p["height"].as_f64().unwrap(), eh),
        ] {
            assert!((got - want).abs() <= tol, "{name}: got {got}, want {want} ±{tol} (box {p})");
        }
        assert_eq!(p["borderColor"], json!("#00e5ff"));

        // 2. タップ → fow.open に URL 本文が入っている
        let r = call_native("qr.0", json!({"nodeId": "x", "kind": "tap"})).unwrap();
        assert_eq!(r["opened"], json!(true));
        let log = take_host_log();
        let fow = log.iter().find(|(f, _)| f == "fow.open").expect("fow.open called");
        assert_eq!(fow.1["id"], json!("qr-link"));
        assert_eq!(fow.1["spec"]["title"], json!("QR コード"));
        let fow_s = fow.1["root"].to_string();
        assert!(fow_s.contains(URL), "fow root contains url: {fow_s}");
        assert!(fow_s.contains("example.com"), "host shown");
        assert!(fow_s.contains("qr.browser"), "browser button present for URL");
        assert!(log.iter().any(|(f, a)| f == "viewer.overlay.set" && a["token"] == json!("tok-1")), "selected box re-set via viewer.overlay.set");

        // 3. コピー → clipboard.set + toast
        call_native("qr.copy", json!({})).unwrap();
        let log = take_host_log();
        assert!(log.iter().any(|(f, a)| f == "clipboard.set" && a["text"] == json!(URL)), "clipboard.set: {log:?}");
        assert!(log.iter().any(|(f, _)| f == "toast"));

        // 4. ブラウザで開く / ミュート
        call_native("qr.browser", json!({})).unwrap();
        call_native("qr.mute", json!({})).unwrap();
        let log = take_host_log();
        assert!(log.iter().any(|(f, a)| f == "url.open" && a["url"] == json!(URL)));
        assert!(log.iter().any(|(f, a)| f == "mute.add" && a["text"] == json!(URL)));

        // 5. viewer.close → fow.close、状態クリア
        call_native("viewer.close", json!({"token": "tok-1"})).unwrap();
        let log = take_host_log();
        assert!(log.iter().any(|(f, a)| f == "fow.close" && a["id"] == json!("qr-link")));
        assert!(call_native("qr.copy", json!({})).is_err(), "nothing selected after close");
        assert!(call_native("qr.0", json!({})).is_err(), "codes cleared after close");
    }

    /// 長辺 > 1024(DETECT_MAX_SIDE)の画像は縮小して検出し、枠は元の width/height 画素系に戻る。
    #[test]
    fn large_image_is_downscaled_and_mapped_back() {
        let _g = lock();
        let (qr, (l, t, r, b)) = qr_luma(URL);
        let (big_w, big_h, ox, oy) = (1600u32, 1000u32, 900u32, 500u32);
        let mut canvas = ImageBuffer::from_pixel(big_w, big_h, Rgba([255u8, 255, 255, 255]));
        for (x, y, p) in qr.enumerate_pixels() {
            let v = p.0[0];
            canvas.put_pixel(ox + x, oy + y, Rgba([v, v, v, 255]));
        }
        let b64 = png_b64(&image::DynamicImage::ImageRgba8(canvas));
        let args = json!({"token": "tok-2", "url": "x", "mimeType": "image/png", "width": big_w, "height": big_h, "bytesB64": b64});
        let res = call_native("viewer.open", args).unwrap();
        let p = &res["root"]["c"][0]["p"];
        let tol = 20.0;
        for (name, got, want) in [
            ("left", p["left"].as_f64().unwrap(), (ox + l) as f64),
            ("top", p["top"].as_f64().unwrap(), (oy + t) as f64),
            ("width", p["width"].as_f64().unwrap(), (r - l) as f64),
            ("height", p["height"].as_f64().unwrap(), (b - t) as f64),
        ] {
            assert!((got - want).abs() <= tol, "{name}: got {got}, want {want} ±{tol} (box {p})");
        }
        call_native("viewer.close", json!({"token": "tok-2"})).unwrap();
    }

    /// ホストが渡す形(長辺 1024 の RGBA の PNG)は縮めずに探す: 1 モジュール 3 画素の小さな QR も見つかる
    /// (以前は 800 へ縮めていて 2.3 画素になり、見落としやすかった)。
    #[test]
    fn host_sized_image_is_not_downscaled() {
        let _g = lock();
        let code = qrcode::QrCode::new(URL.as_bytes()).unwrap();
        let n = code.width() as u32;
        let colors = code.to_colors();
        let (module, ox, oy) = (3u32, 700u32, 120u32);
        let mut canvas = ImageBuffer::from_pixel(1024, 768, Rgba([250u8, 248, 240, 255]));
        for y in 0..n {
            for x in 0..n {
                if colors[(y * n + x) as usize] == qrcode::Color::Dark {
                    for dy in 0..module {
                        for dx in 0..module {
                            canvas.put_pixel(ox + x * module + dx, oy + y * module + dy, Rgba([20, 20, 30, 255]));
                        }
                    }
                }
            }
        }
        let b64 = png_b64(&image::DynamicImage::ImageRgba8(canvas));
        let args = json!({"token": "tok-3", "url": "x", "mimeType": "image/png", "width": 1024, "height": 768, "bytesB64": b64});
        let res = call_native("viewer.open", args).unwrap();
        let p = &res["root"]["c"][0]["p"];
        assert!(p.is_object(), "small QR must be detected: {res}");
        let tol = 6.0;
        for (name, got, want) in [
            ("left", p["left"].as_f64().unwrap(), ox as f64),
            ("top", p["top"].as_f64().unwrap(), oy as f64),
            ("width", p["width"].as_f64().unwrap(), (module * n) as f64),
        ] {
            assert!((got - want).abs() <= tol, "{name}: got {got}, want {want} ±{tol} (box {p})");
        }
        call_native("viewer.close", json!({"token": "tok-3"})).unwrap();
    }

    /// 整数の輝度化は `image` の `to_luma8` と ±1 で一致する(RGB / RGBA / グレー)。
    #[test]
    fn integer_luma_matches_image_crate() {
        let mut rgba = ImageBuffer::new(64, 4);
        for (x, y, p) in rgba.enumerate_pixels_mut() {
            *p = Rgba([(x * 4) as u8, (255 - x * 3) as u8, (y * 60 + x) as u8, (x * 2) as u8]);
        }
        let dynimg = image::DynamicImage::ImageRgba8(rgba);
        for img in [dynimg.clone(), image::DynamicImage::ImageRgb8(dynimg.to_rgb8()), image::DynamicImage::ImageLuma8(dynimg.to_luma8())] {
            let want = img.to_luma8();
            let got = to_luma(img);
            assert_eq!(got.dimensions(), want.dimensions());
            for (a, b) in got.pixels().zip(want.pixels()) {
                assert!((a.0[0] as i32 - b.0[0] as i32).abs() <= 1, "luma {} vs {}", a.0[0], b.0[0]);
            }
        }
    }

    /// `args.locale` の言語で FOW の文とトーストを出す(無い言語は ja)。
    #[test]
    fn texts_follow_locale() {
        let _g = lock();
        let _ = i18n::take_misses();
        let (b64, _) = qr_canvas_b64(URL);
        set_host_hook(|f, _| if f == "fow.open" { json!({"opened": true}) } else { json!({}) });
        take_host_log();
        call_native("viewer.open", open_args(&b64, "image/png")).unwrap();
        call_native("qr.0", json!({"nodeId": "x", "kind": "tap", "locale": "en"})).unwrap();
        let log = take_host_log();
        let fow = log.iter().find(|(f, _)| f == "fow.open").unwrap();
        assert_eq!(fow.1["spec"]["title"], json!("QR code"));
        let s = fow.1["root"].to_string();
        assert!(s.contains("Host: example.com") && s.contains("Open in browser") && s.contains("Mute this link"), "{s}");
        call_native("qr.copy", json!({"locale": "zh-Hant"})).unwrap();
        let log = take_host_log();
        assert!(log.iter().any(|(f, a)| f == "toast" && a["message"] == json!("已複製")), "{log:?}");
        call_native("qr.mute", json!({"locale": "ko"})).unwrap();
        let log = take_host_log();
        assert!(log.iter().any(|(f, a)| f == "toast" && a["message"] == json!("뮤트 단어에 추가를 요청했습니다")), "{log:?}");
        call_native("qr.close", json!({"locale": "zh-Hans"})).unwrap();
        call_native("qr.0", json!({"locale": "zh-Hans"})).unwrap();
        let log = take_host_log();
        let fow = log.iter().find(|(f, _)| f == "fow.open").unwrap();
        assert_eq!(fow.1["spec"]["title"], json!("二维码"));
        call_native("viewer.close", json!({"token": "tok-1"})).unwrap();
        take_host_log();
        assert!(i18n::take_misses().is_empty());
    }

    #[test]
    fn plain_text_qr_hides_browser_button() {
        let _g = lock();
        let (b64, _) = qr_canvas_b64("こんにちは RASQ");
        set_host_hook(|f, _| if f == "fow.open" { json!({"opened": true}) } else { json!({}) });
        take_host_log();
        let r = call_native("viewer.open", open_args(&b64, "image/png")).unwrap();
        assert_eq!(r["root"]["c"].as_array().unwrap().len(), 1);
        call_native("qr.0", json!({})).unwrap();
        let log = take_host_log();
        let fow = log.iter().find(|(f, _)| f == "fow.open").unwrap();
        let s = fow.1["root"].to_string();
        assert!(s.contains("こんにちは RASQ"));
        assert!(s.contains("文字列"));
        assert!(!s.contains("qr.browser"), "no browser button for plain text: {s}");
        assert!(call_native("qr.browser", json!({})).is_err());
        call_native("viewer.close", json!({"token": "tok-1"})).unwrap();
    }

    #[test]
    fn image_without_qr_returns_empty() {
        let _g = lock();
        let mut canvas = ImageBuffer::from_pixel(CANVAS_W, CANVAS_H, Rgba([255u8, 255, 255, 255]));
        // ノイズを少し入れておく(真っ白だけでない)
        for y in (0..CANVAS_H).step_by(17) {
            for x in (0..CANVAS_W).step_by(13) {
                canvas.put_pixel(x, y, Rgba([0u8, 0, 0, 255]));
            }
        }
        let b64 = png_b64(&image::DynamicImage::ImageRgba8(canvas));
        let r = call_native("viewer.open", open_args(&b64, "image/png")).unwrap();
        assert_eq!(r, json!({}));

        // JPEG 経路でも同じ
        let mut buf = Cursor::new(Vec::new());
        image::DynamicImage::ImageRgb8(ImageBuffer::from_pixel(64, 64, image::Rgb([200u8, 200, 200])))
            .write_to(&mut buf, image::ImageFormat::Jpeg)
            .unwrap();
        let jb64 = base64::engine::general_purpose::STANDARD.encode(buf.into_inner());
        let r = call_native("viewer.open", open_args(&jb64, "image/jpeg")).unwrap();
        assert_eq!(r, json!({}));
    }

    #[test]
    fn non_image_or_missing_bytes_returns_empty() {
        let _g = lock();
        let (b64, _) = qr_canvas_b64(URL);
        assert_eq!(call_native("viewer.open", open_args(&b64, "video/mp4")).unwrap(), json!({}));
        assert_eq!(call_native("viewer.open", open_args(&b64, "image/gif")).unwrap(), json!({}));
        let mut a = open_args(&b64, "image/png");
        a.as_object_mut().unwrap().remove("bytesB64");
        assert_eq!(call_native("viewer.open", a).unwrap(), json!({}));
    }

    #[test]
    fn bad_input_does_not_panic() {
        let _g = lock();
        assert!(call_native("viewer.open", json!(null)).is_err());
        assert!(call_native("viewer.open", open_args("!!!not base64!!!", "image/png")).is_err());
        assert!(call_native("viewer.open", open_args("aGVsbG8=", "image/png")).is_err()); // "hello" は PNG ではない
        assert!(call_native("qr.99", json!({})).is_err());
        assert!(call_native("qr.abc", json!({})).is_err());
        assert_eq!(call_native("qr.closed", json!({})).unwrap(), json!({}));
        assert_eq!(call_native("viewer.close", json!({})).unwrap(), json!({}));
        assert_eq!(call_native("nope", json!({})).unwrap_err()["code"], json!("C01"));
    }

    #[test]
    fn url_host_parsing() {
        assert_eq!(url_host("https://example.com/a?b#c").as_deref(), Some("example.com"));
        assert_eq!(url_host("HTTP://user@Host.jp:8080/").as_deref(), Some("Host.jp:8080"));
        assert_eq!(url_host("ftp://x"), None);
        assert_eq!(url_host("https://"), None);
        assert_eq!(url_host("hello"), None);
    }
}
