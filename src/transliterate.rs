use std::sync::mpsc::{channel, Sender};
use std::sync::OnceLock;
use windows::core::HSTRING;
use windows::Globalization::JapanesePhoneticAnalyzer;

struct StaTransliterateRequest {
    texts: Vec<String>,
    resp: Sender<Vec<Option<String>>>,
}

static STA_WORKER: OnceLock<Sender<StaTransliterateRequest>> = OnceLock::new();

fn get_sta_worker() -> &'static Sender<StaTransliterateRequest> {
    STA_WORKER.get_or_init(|| {
        let (tx, rx) = channel::<StaTransliterateRequest>();
        std::thread::Builder::new()
            .name("JapanesePhoneticSTA".to_string())
            .spawn(move || {
                unsafe {
                    use windows::Win32::System::Com::{CoInitializeEx, COINIT_APARTMENTTHREADED};
                    let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
                }
                while let Ok(req) = rx.recv() {
                    let mut results = Vec::with_capacity(req.texts.len());
                    for text in req.texts {
                        results.push(transliterate_japanese_impl(&text));
                    }
                    let _ = req.resp.send(results);

                    // Strictly pump and dispatch pending Windows COM messages on this STA thread!
                    // Without message pumping, COM marshaling release callbacks and RPC cleanup stubs
                    // accumulate indefinitely in the STA thread's message queue, leaking native memory.
                    unsafe {
                        use windows::Win32::UI::WindowsAndMessaging::{
                            DispatchMessageW, PeekMessageW, TranslateMessage, MSG, PM_REMOVE,
                        };
                        let mut msg = MSG::default();
                        while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                            let _ = TranslateMessage(&msg);
                            DispatchMessageW(&msg);
                        }
                    }
                }
            })
            .expect("Failed to spawn STA thread");
        tx
    })
}

/// Checks if a string contains CJK characters (Japanese Kana/Kanji, Korean Hangul, Chinese Hanzi).
pub fn contains_cjk(text: &str) -> bool {
    text.chars().any(|c| {
        let u = c as u32;
        // Hiragana: 0x3040..=0x309F
        // Katakana: 0x30A0..=0x30FF
        // CJK Unified Ideographs (Kanji/Hanzi): 0x4E00..=0x9FFF
        // Hangul Syllables: 0xAC00..=0xD7AF
        // Hangul Jamo: 0x1100..=0x11FF, 0x3130..=0x318F
        (0x3040..=0x30FF).contains(&u)
            || (0x4E00..=0x9FFF).contains(&u)
            || (0xAC00..=0xD7AF).contains(&u)
            || (0x1100..=0x11FF).contains(&u)
            || (0x3130..=0x318F).contains(&u)
    })
}

/// Converts Korean Hangul syllables to Revised Romanization.
pub fn romanize_hangul_char(c: char) -> Option<String> {
    let u = c as u32;
    if !(0xAC00..=0xD7A3).contains(&u) {
        return None;
    }

    let code = u - 0xAC00;
    let l_idx = (code / 588) as usize;
    let v_idx = ((code % 588) / 28) as usize;
    let t_idx = (code % 28) as usize;

    const CHOSEONG: [&str; 19] = [
        "g", "kk", "n", "d", "tt", "r", "m", "b", "pp", "s", "ss", "", "j", "jj", "ch", "k", "t", "p", "h",
    ];
    const JUNGSEONG: [&str; 21] = [
        "a", "ae", "ya", "yae", "eo", "e", "yeo", "ye", "o", "wa", "wae", "oe", "yo", "u", "wo", "we", "wi", "yu", "eu", "ui", "i",
    ];
    const JONGSEONG: [&str; 28] = [
        "", "k", "k", "ks", "n", "nj", "nh", "t", "l", "lg", "lm", "lb", "ls", "lt", "lp", "lh", "m", "p", "bs", "t", "ss", "ng", "t", "t", "k", "t", "p", "h",
    ];

    let l = CHOSEONG.get(l_idx).copied().unwrap_or("");
    let v = JUNGSEONG.get(v_idx).copied().unwrap_or("");
    let t = JONGSEONG.get(t_idx).copied().unwrap_or("");

    Some(format!("{}{}{}", l, v, t))
}

/// Converts standard Hiragana / Katakana string to Hepburn Romaji.
pub fn kana_to_romaji(kana: &str) -> String {
    let mut out = String::with_capacity(kana.len() * 2);
    let chars: Vec<char> = kana.chars().collect();
    let len = chars.len();
    let mut i = 0;

    while i < len {
        let c = chars[i];
        let next = chars.get(i + 1).copied();

        // Check digraphs (e.g., きゃ, しゅ, チョ)
        if let Some(n) = next {
            if let Some(rom) = match_digraph(c, n) {
                out.push_str(rom);
                i += 2;
                continue;
            }
        }

        // Sokuon (small tsu っ or ッ) doubles the next consonant
        if c == 'っ' || c == 'ッ' {
            if let Some(n) = next {
                if let Some(rom) = single_kana_to_romaji(n) {
                    if let Some(first_char) = rom.chars().next() {
                        if first_char != 'a' && first_char != 'i' && first_char != 'u' && first_char != 'e' && first_char != 'o' {
                            out.push(first_char);
                            i += 1;
                            continue;
                        }
                    }
                }
            }
            out.push('t');
            i += 1;
            continue;
        }

        // Long vowel mark ー
        if c == 'ー' {
            if let Some(last) = out.chars().last() {
                if matches!(last, 'a' | 'i' | 'u' | 'e' | 'o') {
                    out.push(last);
                    i += 1;
                    continue;
                }
            }
            out.push('-');
            i += 1;
            continue;
        }

        if let Some(rom) = single_kana_to_romaji(c) {
            out.push_str(rom);
        } else {
            out.push(c);
        }
        i += 1;
    }

    out
}

fn single_kana_to_romaji(c: char) -> Option<&'static str> {
    Some(match c {
        // Hiragana
        'あ' | 'ア' => "a", 'い' | 'イ' => "i", 'う' | 'ウ' => "u", 'え' | 'エ' => "e", 'お' | 'オ' => "o",
        'か' | 'カ' => "ka", 'き' | 'キ' => "ki", 'く' | 'ク' => "ku", 'け' | 'ケ' => "ke", 'こ' | 'コ' => "ko",
        'が' | 'ガ' => "ga", 'ぎ' | 'ギ' => "gi", 'ぐ' | 'グ' => "gu", 'げ' | 'ゲ' => "ge", 'ご' | 'ゴ' => "go",
        'さ' | 'サ' => "sa", 'し' | 'シ' => "shi", 'す' | 'ス' => "su", 'せ' | 'セ' => "se", 'そ' | 'ソ' => "so",
        'ざ' | 'ザ' => "za", 'じ' | 'ジ' => "ji", 'ず' | 'ズ' => "zu", 'ぜ' | 'ゼ' => "ze", 'ぞ' | 'ゾ' => "zo",
        'た' | 'タ' => "ta", 'ち' | 'チ' => "chi", 'つ' | 'ツ' => "tsu", 'て' | 'テ' => "te", 'と' | 'ト' => "to",
        'だ' | 'ダ' => "da", 'ぢ' | 'ヂ' => "ji", 'づ' | 'ヅ' => "zu", 'で' | 'デ' => "de", 'ど' | 'ド' => "do",
        'な' | 'ナ' => "na", 'に' | 'ニ' => "ni", 'ぬ' | 'ヌ' => "nu", 'ね' | 'ネ' => "ne", 'の' | 'ノ' => "no",
        'は' | 'ハ' => "ha", 'ひ' | 'ヒ' => "hi", 'ふ' | 'フ' => "fu", 'へ' | 'ヘ' => "he", 'ほ' | 'ホ' => "ho",
        'ば' | 'バ' => "ba", 'び' | 'ビ' => "bi", 'ぶ' | 'ブ' => "bu", 'べ' | 'ベ' => "be", 'ぼ' | 'ボ' => "bo",
        'ぱ' | 'パ' => "pa", 'ぴ' | 'ピ' => "pi", 'ぷ' | 'プ' => "pu", 'ぺ' | 'ペ' => "pe", 'ぽ' | 'ポ' => "po",
        'ま' | 'マ' => "ma", 'み' | 'ミ' => "mi", 'む' | 'ム' => "mu", 'め' | 'メ' => "me", 'も' | 'モ' => "mo",
        'や' | 'ヤ' => "ya", 'ゆ' | 'ユ' => "yu", 'よ' | 'ヨ' => "yo",
        'ら' | 'ラ' => "ra", 'り' | 'リ' => "ri", 'る' | 'ル' => "ru", 'れ' | 'レ' => "re", 'ろ' | 'ロ' => "ro",
        'わ' | 'ワ' => "wa", 'を' | 'ヲ' => "wo", 'ん' | 'ン' => "n",
        // Small kana
        'ぁ' | 'ァ' => "a", 'ぃ' | 'ィ' => "i", 'ぅ' | 'ゥ' => "u", 'ぇ' | 'ェ' => "e", 'ぉ' | 'ォ' => "o",
        'ゃ' | 'ャ' => "ya", 'ゅ' | 'ュ' => "yu", 'ょ' | 'ョ' => "yo",
        'ゔ' | 'ヴ' => "vu",
        _ => return None,
    })
}

fn match_digraph(c1: char, c2: char) -> Option<&'static str> {
    match (c1, c2) {
        ('き' | 'キ', 'ゃ' | 'ャ') => Some("kya"),
        ('き' | 'キ', 'ゅ' | 'ュ') => Some("kyu"),
        ('き' | 'キ', 'ょ' | 'ョ') => Some("kyo"),
        ('ぎ' | 'ギ', 'ゃ' | 'ャ') => Some("gya"),
        ('ぎ' | 'ギ', 'ゅ' | 'ュ') => Some("gyu"),
        ('ぎ' | 'ギ', 'ょ' | 'ョ') => Some("gyo"),
        ('し' | 'シ', 'ゃ' | 'ャ') => Some("sha"),
        ('し' | 'シ', 'ゅ' | 'ュ') => Some("shu"),
        ('し' | 'シ', 'ょ' | 'ョ') => Some("sho"),
        ('じ' | 'ジ', 'ゃ' | 'ャ') => Some("ja"),
        ('じ' | 'ジ', 'ゅ' | 'ュ') => Some("ju"),
        ('じ' | 'ジ', 'ょ' | 'ョ') => Some("jo"),
        ('ち' | 'チ', 'ゃ' | 'ャ') => Some("cha"),
        ('ち' | 'チ', 'ゅ' | 'ュ') => Some("chu"),
        ('ち' | 'チ', 'ょ' | 'ョ') => Some("cho"),
        ('に' | 'ニ', 'ゃ' | 'ャ') => Some("nya"),
        ('に' | 'ニ', 'ゅ' | 'ュ') => Some("nyu"),
        ('に' | 'ニ', 'ょ' | 'ョ') => Some("nyo"),
        ('ひ' | 'ヒ', 'ゃ' | 'ャ') => Some("hya"),
        ('ひ' | 'ヒ', 'ゅ' | 'ュ') => Some("hyu"),
        ('ひ' | 'ヒ', 'ょ' | 'ョ') => Some("hyo"),
        ('び' | 'ビ', 'ゃ' | 'ャ') => Some("bya"),
        ('び' | 'ビ', 'ゅ' | 'ュ') => Some("byu"),
        ('び' | 'ビ', 'ょ' | 'ョ') => Some("byo"),
        ('ぴ' | 'ピ', 'ゃ' | 'ャ') => Some("pya"),
        ('ぴ' | 'ピ', 'ゅ' | 'ュ') => Some("pyu"),
        ('ぴ' | 'ピ', 'ょ' | 'ョ') => Some("pyo"),
        ('み' | 'ミ', 'ゃ' | 'ャ') => Some("mya"),
        ('み' | 'ミ', 'ゅ' | 'ュ') => Some("myu"),
        ('み' | 'ミ', 'ょ' | 'ョ') => Some("myo"),
        ('り' | 'リ', 'ゃ' | 'ャ') => Some("rya"),
        ('り' | 'リ', 'ゅ' | 'ュ') => Some("ryu"),
        ('り' | 'リ', 'ょ' | 'ョ') => Some("ryo"),
        _ => None,
    }
}

fn normalize_fullwidth(s: &str) -> String {
    s.chars().map(|c| {
        let u = c as u32;
        if (0xFF01..=0xFF5E).contains(&u) {
            char::from_u32(u - 0xFEE0).unwrap_or(c)
        } else if u == 0x3000 {
            ' '
        } else {
            c
        }
    }).collect()
}

fn transliterate_japanese_impl(text: &str) -> Option<String> {
    let hstr = HSTRING::from(text);
    let words = JapanesePhoneticAnalyzer::GetWords(&hstr).ok()?;

    let mut out = String::with_capacity(text.len() * 2);
    for word in words.into_iter() {
        let raw_disp = word.DisplayText().map(|h| h.to_string()).unwrap_or_default();
        let disp = normalize_fullwidth(&raw_disp);
        let yomi = word.YomiText().map(|h| h.to_string()).unwrap_or_default();

        // If display text is pure ASCII / Latin / digits / punctuation (e.g. "Glitters", "123", "!"),
        // preserve original display text instead of full-width yomi
        if disp.chars().all(|c| c.is_ascii()) {
            let trimmed = disp.trim();
            if !trimmed.is_empty() {
                if !out.is_empty() && !out.ends_with(' ') {
                    out.push(' ');
                }
                out.push_str(trimmed);
            }
            continue;
        }

        let has_kana_kanji = raw_disp.chars().any(|c| {
            let u = c as u32;
            (0x3040..=0x30FF).contains(&u) || (0x4E00..=0x9FFF).contains(&u)
        });

        if has_kana_kanji && !yomi.is_empty() {
            let rom = kana_to_romaji(&yomi);
            let trimmed = rom.trim();
            if !trimmed.is_empty() {
                if !out.is_empty() && !out.ends_with(' ') {
                    out.push(' ');
                }
                out.push_str(trimmed);
            }
        } else {
            let rom = kana_to_romaji(&disp);
            let trimmed = rom.trim();
            if !trimmed.is_empty() {
                if !out.is_empty() && !out.ends_with(' ') {
                    out.push(' ');
                }
                out.push_str(trimmed);
            }
        }
    }

    if out.trim().is_empty() {
        None
    } else {
        Some(out.trim().to_string())
    }
}

/// Transliterates Japanese text (Kanji + Kana) into Romaji using Windows JapanesePhoneticAnalyzer.
pub fn transliterate_japanese(text: &str) -> Option<String> {
    let (resp_tx, resp_rx) = channel();
    let req = StaTransliterateRequest {
        texts: vec![text.to_string()],
        resp: resp_tx,
    };
    let worker = get_sta_worker();
    if worker.send(req).is_ok() {
        resp_rx.recv().ok().and_then(|mut v| v.pop().flatten())
    } else {
        None
    }
}

/// Batch transliterates a list of strings efficiently on the STA worker thread.
#[allow(dead_code)]
pub fn batch_transliterate_japanese(texts: Vec<String>) -> Vec<Option<String>> {
    let (resp_tx, resp_rx) = channel();
    let req = StaTransliterateRequest {
        texts,
        resp: resp_tx,
    };
    let worker = get_sta_worker();
    if worker.send(req).is_ok() {
        resp_rx.recv().unwrap_or_default()
    } else {
        Vec::new()
    }
}

/// Transliterates Korean Hangul text into Romanized Latin (Romaja).
pub fn transliterate_korean(text: &str) -> String {
    let mut out = String::with_capacity(text.len() * 2);
    for c in text.chars() {
        if let Some(rom) = romanize_hangul_char(c) {
            out.push_str(&rom);
        } else {
            out.push(c);
        }
    }
    out
}

/// Automatically romanizes CJK text (Japanese Kanji/Kana, Korean Hangul) to Latin.
pub fn romanize_line(text: &str) -> String {
    if !contains_cjk(text) {
        return text.to_string();
    }

    // 1. If text has Hangul, transliterate Korean
    let has_hangul = text.chars().any(|c| (0xAC00..=0xD7AF).contains(&(c as u32)));
    if has_hangul {
        return transliterate_korean(text);
    }

    // 2. If text has Japanese Kana/Kanji, use Windows native phonetic analyzer
    if let Some(rom) = transliterate_japanese(text) {
        return rom;
    }

    // 3. Fallback: Kana conversion
    kana_to_romaji(text)
}
