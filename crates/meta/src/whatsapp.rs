//! WhatsApp chat exports: the chat text (`WhatsApp Chat with Alice.txt` on
//! Android, `_chat.txt` on iPhone) says when each attached file was sent
//! and by whom. The media files themselves carry no capture date.
//!
//! Lines look like (the date order follows the phone's region):
//!
//! ```text
//! 12.08.19, 14:32 - Alice: IMG-20190812-WA0007.jpg (file attached)
//! 8/12/19, 2:32 PM - Alice: IMG-20190812-WA0007.jpg (file attached)
//! [12.08.19, 14:32:10] Alice: <attached: 00000012-PHOTO-2019-08-12-14-32-10.jpg>
//! ```

use std::collections::HashMap;
use std::sync::LazyLock;

use chrono::{NaiveDate, NaiveDateTime, NaiveTime};
use regex::Regex;

#[derive(Debug, Clone, PartialEq)]
pub struct Sent {
    /// Wall-clock time on the exporting phone.
    pub local: NaiveDateTime,
    pub sender: String,
}

#[derive(Debug, Clone, Default)]
pub struct Chat {
    /// "Alice", from the file or folder name.
    pub name: Option<String>,
    /// Lower-case file name → when and by whom it was sent.
    pub files: HashMap<String, Sent>,
}

static LINE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"^\u{200e}?\[?(\d{1,4})[./-](\d{1,2})[./-](\d{1,4}),?\s+(\d{1,2})[:.](\d{2})(?:[:.](\d{2}))?\s*([AaPp]\.?\s?[Mm]\.?)?\]?\s*(?:-\s*)?([^:]{1,80}?):\s(.*)$",
    )
    .unwrap()
});

static FILE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)[\w\-.()]+\.(jpe?g|png|gif|webp|heic|heif|mp4|3gp|mov|m4v|avi|mkv|opus|m4a|aac|mp3|pdf|webm)\b").unwrap()
});

/// Prefixes of the chat text file name, by app language.
const PREFIXES: &[&str] = &["whatsapp chat with ", "whatsapp chat - ", "whatsapp-chat mit ", "whatsapp chat mit ", "whatsapp-chat - "];

/// The chat partner or group from `WhatsApp Chat with Alice.txt`, or from
/// the folder of an iPhone `_chat.txt`.
pub fn chat_name(text_file: &str, folder: &str) -> Option<String> {
    let stem = text_file.rsplit_once('.').map(|(s, _)| s).unwrap_or(text_file);
    let from = if stem.eq_ignore_ascii_case("_chat") { folder } else { stem };
    let lower = from.to_lowercase();
    PREFIXES.iter().find_map(|p| lower.starts_with(p).then(|| from[p.len()..].trim().to_string())).filter(|n| !n.is_empty())
}

/// Whether a text file looks like a WhatsApp chat export.
pub fn is_chat_file(name: &str) -> bool {
    let l = name.to_lowercase();
    l == "_chat.txt" || (l.ends_with(".txt") && PREFIXES.iter().any(|p| l.starts_with(p)))
}

struct Raw {
    a: u32,
    b: u32,
    y: i32,
    h: u32,
    m: u32,
    s: u32,
    pm: Option<bool>,
    sender: String,
    files: Vec<String>,
}

/// Read a chat text. `known` are the media names in the export (lower
/// case), used to pick the date order when the text alone cannot tell.
pub fn parse(text: &str, known: &[String]) -> Chat {
    let mut raws = Vec::new();
    for line in text.lines() {
        let Some(c) = LINE.captures(line.trim_start_matches('\u{feff}')) else { continue };
        let files: Vec<String> =
            FILE.find_iter(&c[9]).map(|m| m.as_str().to_lowercase()).filter(|f| known.is_empty() || known.contains(f)).collect();
        if files.is_empty() {
            continue;
        }
        let num = |i: usize| c.get(i).and_then(|m| m.as_str().parse::<u32>().ok()).unwrap_or(0);
        let (mut a, mut b, mut y) = (num(1), num(2), num(3) as i32);
        // Year first (2019-08-12) in some locales.
        if a > 31 {
            (a, b, y) = (num(3), num(2), num(1) as i32);
        }
        if y < 100 {
            y += 2000;
        }
        let pm = c.get(7).map(|m| m.as_str().to_lowercase().starts_with('p'));
        raws.push(Raw {
            a,
            b,
            y,
            h: num(4),
            m: num(5),
            s: num(6),
            pm,
            sender: c[8].trim().trim_start_matches('\u{200e}').to_string(),
            files,
        });
    }
    let day_first = day_first(&raws);
    let mut files = HashMap::new();
    for r in raws {
        let (d, mo) = if day_first { (r.a, r.b) } else { (r.b, r.a) };
        let h = match r.pm {
            Some(true) if r.h < 12 => r.h + 12,
            Some(false) if r.h == 12 => 0,
            _ => r.h,
        };
        let (Some(date), Some(time)) = (NaiveDate::from_ymd_opt(r.y, mo, d), NaiveTime::from_hms_opt(h, r.m, r.s)) else { continue };
        for f in r.files {
            files.entry(f).or_insert(Sent { local: date.and_time(time), sender: r.sender.clone() });
        }
    }
    Chat { name: None, files }
}

/// Day before month? A component above 12 decides; otherwise the dates in
/// Android file names (`IMG-20190812-WA0007.jpg`) do; day first by default.
fn day_first(raws: &[Raw]) -> bool {
    if raws.iter().any(|r| r.a > 12) {
        return true;
    }
    if raws.iter().any(|r| r.b > 12) {
        return false;
    }
    // Android `IMG-20190812-WA0007.jpg`, iPhone `00000012-PHOTO-2019-08-12-14-32-10.jpg`.
    static NAME_DATE: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"-(\d{4})(\d{2})(\d{2})-wa|-(?:photo|video|audio|gif|sticker)-(\d{4})-(\d{2})-(\d{2})-").unwrap());
    let (mut df, mut mf) = (0, 0);
    for r in raws {
        for f in &r.files {
            if let Some(c) = NAME_DATE.captures(f) {
                let part = |a: usize, b: usize| c.get(a).or(c.get(b)).and_then(|m| m.as_str().parse::<u32>().ok()).unwrap_or(0);
                let (mo, d) = (part(2, 5), part(3, 6));
                if (r.a, r.b) == (d, mo) {
                    df += 1;
                }
                if (r.a, r.b) == (mo, d) {
                    mf += 1;
                }
            }
        }
    }
    df >= mf
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(s: &str) -> NaiveDateTime {
        NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S").unwrap()
    }

    #[test]
    fn android_day_first() {
        let text = "12.08.19, 14:32 - Alice: IMG-20190812-WA0007.jpg (Datei angehängt)\n\
                    12.08.19, 14:33 - Bob: Schön!\n\
                    13.08.19, 09:05 - Bob: VID-20190813-WA0002.mp4 (Datei angehängt)\n\
                    Ein Satz über zwei Zeilen\n";
        let c = parse(text, &[]);
        assert_eq!(c.files.len(), 2);
        assert_eq!(c.files["img-20190812-wa0007.jpg"], Sent { local: at("2019-08-12 14:32:00"), sender: "Alice".into() });
        assert_eq!(c.files["vid-20190813-wa0002.mp4"].local, at("2019-08-13 09:05:00"));
    }

    #[test]
    fn android_month_first_with_am_pm() {
        let text = "8/12/19, 2:32 PM - Alice: IMG-20190812-WA0007.jpg (file attached)\n\
                    8/12/19, 12:05 AM - Alice: IMG-20190812-WA0008.jpg (file attached)\n";
        let c = parse(text, &[]);
        assert_eq!(c.files["img-20190812-wa0007.jpg"].local, at("2019-08-12 14:32:00"));
        assert_eq!(c.files["img-20190812-wa0008.jpg"].local, at("2019-08-12 00:05:00"));
    }

    #[test]
    fn ambiguous_dates_follow_the_file_names() {
        // 3/4 could be 3 April or 4 March: the name says 4 March.
        let text = "3/4/20, 10:00 - Alice: IMG-20200304-WA0001.jpg (file attached)\n";
        assert_eq!(parse(text, &[]).files["img-20200304-wa0001.jpg"].local, at("2020-03-04 10:00:00"));
        let text = "4/3/20, 10:00 - Alice: IMG-20200304-WA0001.jpg (file attached)\n";
        assert_eq!(parse(text, &[]).files["img-20200304-wa0001.jpg"].local, at("2020-03-04 10:00:00"));
    }

    #[test]
    fn iphone_month_first_from_the_names() {
        let text = "[3/7/21, 6:20:11 PM] Bob: \u{200e}<attached: 00000003-PHOTO-2021-07-03-18-20-11.jpg>\n";
        let c = parse(text, &[]);
        assert_eq!(c.files["00000003-photo-2021-07-03-18-20-11.jpg"].local, at("2021-07-03 18:20:11"));
    }

    #[test]
    fn iphone_export() {
        let text = "\u{feff}[12.08.19, 14:32:10] Alice: \u{200e}<attached: 00000012-PHOTO-2019-08-12-14-32-10.jpg>\n\
                    \u{200e}[12.08.19, 14:40:01] Bob: \u{200e}<Anhang: 00000013-VIDEO-2019-08-12-14-40-01.mp4>\n";
        let known = vec!["00000012-photo-2019-08-12-14-32-10.jpg".to_string(), "00000013-video-2019-08-12-14-40-01.mp4".to_string()];
        let c = parse(text, &known);
        assert_eq!(c.files["00000012-photo-2019-08-12-14-32-10.jpg"], Sent { local: at("2019-08-12 14:32:10"), sender: "Alice".into() });
        assert_eq!(c.files["00000013-video-2019-08-12-14-40-01.mp4"].sender, "Bob");
    }

    #[test]
    fn names() {
        assert_eq!(chat_name("WhatsApp Chat with Alice.txt", "x").as_deref(), Some("Alice"));
        assert_eq!(chat_name("WhatsApp-Chat mit Familie.txt", "x").as_deref(), Some("Familie"));
        assert_eq!(chat_name("_chat.txt", "WhatsApp Chat - Bob").as_deref(), Some("Bob"));
        assert_eq!(chat_name("_chat.txt", "export"), None);
        assert!(is_chat_file("_chat.txt") && is_chat_file("WhatsApp Chat with Alice.txt") && !is_chat_file("notes.txt"));
    }
}
