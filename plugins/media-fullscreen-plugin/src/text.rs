//! 利用者に見せる文の表(PROTOCOL.md §4.6)。ホストの `args.locale`(無ければ `ja`)で引く。
//!
//! 基準は `ja`。`en` / `zh-Hans` / `zh-Hant` / `ko` は同じキーを全部持つ(テストで確かめる)。

rasq_plugin_kit::i18n_catalog! {
    pub(crate) static TEXT = {
        "ja": {
            // 枠をタップしたときの FOW
            "fow.title" => "QR コード",
            "fow.kindLink" => "リンク",
            "fow.host" => "ホスト: {host}",
            "fow.kindText" => "文字列(URL ではありません)",
            "fow.copy" => "コピー",
            "fow.openBrowser" => "ブラウザで開く",
            "fow.muteLink" => "このリンクをミュート",
            "fow.muteText" => "この文字列をミュート",
            "fow.close" => "閉じる",
            // FOW のボタンを押したときのトースト
            "toast.copied" => "コピーしました",
            "toast.opening" => "ブラウザで開きます",
            "toast.muteRequested" => "ミュート語に追加を依頼しました",
            // 本体ツリー(このスロットでは使われない)
            "root.hello" => "QR コード検出(viewer.overlay)",
        },
        "en": {
            // 枠をタップしたときの FOW
            "fow.title" => "QR code",
            "fow.kindLink" => "Link",
            "fow.host" => "Host: {host}",
            "fow.kindText" => "Text (not a URL)",
            "fow.copy" => "Copy",
            "fow.openBrowser" => "Open in browser",
            "fow.muteLink" => "Mute this link",
            "fow.muteText" => "Mute this text",
            "fow.close" => "Close",
            // FOW のボタンを押したときのトースト
            "toast.copied" => "Copied",
            "toast.opening" => "Opening in browser",
            "toast.muteRequested" => "Requested adding to muted words",
            // 本体ツリー(このスロットでは使われない)
            "root.hello" => "QR code detection (viewer.overlay)",
        },
        "zh-Hans": {
            // 枠をタップしたときの FOW
            "fow.title" => "二维码",
            "fow.kindLink" => "链接",
            "fow.host" => "主机：{host}",
            "fow.kindText" => "文本（不是 URL）",
            "fow.copy" => "复制",
            "fow.openBrowser" => "在浏览器中打开",
            "fow.muteLink" => "屏蔽此链接",
            "fow.muteText" => "屏蔽此文本",
            "fow.close" => "关闭",
            // FOW のボタンを押したときのトースト
            "toast.copied" => "已复制",
            "toast.opening" => "正在浏览器中打开",
            "toast.muteRequested" => "已请求添加到屏蔽词",
            // 本体ツリー(このスロットでは使われない)
            "root.hello" => "二维码检测（viewer.overlay）",
        },
        "zh-Hant": {
            // 枠をタップしたときの FOW
            "fow.title" => "QR 碼",
            "fow.kindLink" => "連結",
            "fow.host" => "主機：{host}",
            "fow.kindText" => "文字（不是 URL）",
            "fow.copy" => "複製",
            "fow.openBrowser" => "在瀏覽器中開啟",
            "fow.muteLink" => "靜音此連結",
            "fow.muteText" => "靜音此文字",
            "fow.close" => "關閉",
            // FOW のボタンを押したときのトースト
            "toast.copied" => "已複製",
            "toast.opening" => "正在瀏覽器中開啟",
            "toast.muteRequested" => "已要求加入靜音字詞",
            // 本体ツリー(このスロットでは使われない)
            "root.hello" => "QR 碼偵測（viewer.overlay）",
        },
        "ko": {
            // 枠をタップしたときの FOW
            "fow.title" => "QR 코드",
            "fow.kindLink" => "링크",
            "fow.host" => "호스트: {host}",
            "fow.kindText" => "텍스트(URL이 아님)",
            "fow.copy" => "복사",
            "fow.openBrowser" => "브라우저에서 열기",
            "fow.muteLink" => "이 링크 뮤트",
            "fow.muteText" => "이 텍스트 뮤트",
            "fow.close" => "닫기",
            // FOW のボタンを押したときのトースト
            "toast.copied" => "복사했습니다",
            "toast.opening" => "브라우저에서 엽니다",
            "toast.muteRequested" => "뮤트 단어에 추가를 요청했습니다",
            // 本体ツリー(このスロットでは使われない)
            "root.hello" => "QR 코드 감지(viewer.overlay)",
        },
    };
}

#[cfg(test)]
mod tests {
    use super::TEXT;
    use std::collections::BTreeSet;

    /// この表の言語ごとのキー(`Catalog::check` は「基準に無いキー」と差し込みの食い違いしか見ないので、訳の抜けはここで見る)。
    fn table_keys() -> Vec<(String, BTreeSet<String>)> {
        let mut out: Vec<(String, BTreeSet<String>)> = Vec::new();
        for line in include_str!("text.rs").lines().map(str::trim) {
            if let Some(tag) = line.strip_prefix('"').and_then(|l| l.strip_suffix("\": {")) {
                out.push((tag.to_string(), BTreeSet::new()));
            } else if let Some((key, _)) = line.strip_prefix('"').and_then(|l| l.split_once("\" =>")) {
                out.last_mut().expect("表の外のキー").1.insert(key.to_string());
            }
        }
        out
    }

    #[test]
    fn every_locale_has_every_key() {
        assert!(TEXT.check().is_empty(), "{:?}", TEXT.check());
        let tables = table_keys();
        let tags: Vec<&str> = tables.iter().map(|(t, _)| t.as_str()).collect();
        assert_eq!(tags, TEXT.locales());
        assert_eq!(tags, ["ja", "en", "zh-Hans", "zh-Hant", "ko"]);
        for (tag, keys) in &tables[1..] {
            let missing: Vec<_> = tables[0].1.difference(keys).collect();
            assert!(missing.is_empty(), "{tag} に訳が無いキー: {missing:?}");
        }
    }
}
