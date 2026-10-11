//! The Private guard's text patterns: payment card numbers (Luhn), IBANs
//! (mod 97) and the machine-readable lines of passports and ID cards (ICAO
//! 9303 check digits). Input is OCR text, so separators and line breaks
//! vary and `<` may come out as `«`. Findings describe what was seen without
//! repeating it: no full numbers end up in the database or the logs.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Card,
    Iban,
    Mrz,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    pub kind: Kind,
    /// "Payment card number ending 4242", safe to show and store.
    pub why: String,
}

/// Everything sensitive in `text`, at most one finding per kind.
pub fn scan(text: &str) -> Vec<Finding> {
    let mut out = Vec::new();
    if let Some(last4) = card(text) {
        out.push(Finding { kind: Kind::Card, why: format!("Payment card number ending {last4}") });
    }
    if let Some(cc) = iban(text) {
        out.push(Finding { kind: Kind::Iban, why: format!("Bank account number (IBAN, {cc})") });
    }
    if let Some(what) = mrz(text) {
        out.push(Finding { kind: Kind::Mrz, why: format!("Machine-readable lines of {what}") });
    }
    out
}

// ---- payment cards ---------------------------------------------------------

/// The last four digits of the first valid card number.
fn card(text: &str) -> Option<String> {
    for groups in digit_runs(text) {
        // Try every run of whole groups, longest first, so "Tel 044 …" next
        // to a card number doesn't hide it.
        for start in 0..groups.len() {
            let mut digits = String::new();
            for g in &groups[start..] {
                digits.push_str(g);
                if digits.len() > 19 {
                    break;
                }
                if (13..=19).contains(&digits.len()) && is_card(&digits) {
                    return Some(digits[digits.len() - 4..].to_string());
                }
            }
        }
    }
    None
}

/// Digit groups separated by at most one space or dash.
fn digit_runs(text: &str) -> Vec<Vec<String>> {
    let mut runs = Vec::new();
    let mut groups: Vec<String> = Vec::new();
    let mut cur = String::new();
    let mut gap = 0;
    for ch in text.chars() {
        if ch.is_ascii_digit() {
            if gap > 1 && !cur.is_empty() {
                groups.push(std::mem::take(&mut cur));
                runs.push(std::mem::take(&mut groups));
            } else if gap == 1 && !cur.is_empty() {
                groups.push(std::mem::take(&mut cur));
            }
            cur.push(ch);
            gap = 0;
        } else if (ch == ' ' || ch == '-') && gap == 0 && !cur.is_empty() {
            gap = 1;
        } else {
            gap = 2;
            if !cur.is_empty() {
                groups.push(std::mem::take(&mut cur));
            }
            if !groups.is_empty() {
                runs.push(std::mem::take(&mut groups));
            }
        }
    }
    if !cur.is_empty() {
        groups.push(cur);
    }
    if !groups.is_empty() {
        runs.push(groups);
    }
    runs
}

fn is_card(d: &str) -> bool {
    let n = d.len();
    let p = |k: usize| d[..k].parse::<u32>().unwrap_or(0);
    let brand = match d.as_bytes()[0] {
        b'4' => matches!(n, 13 | 16 | 19),
        b'5' => (51..=55).contains(&p(2)) && n == 16 || (p(2) == 50 || (56..=59).contains(&p(2))) && (12..=19).contains(&n),
        b'2' => (2221..=2720).contains(&p(4)) && n == 16,
        b'3' => {
            matches!(p(2), 34 | 37) && n == 15
                || (3528..=3589).contains(&p(4)) && (16..=19).contains(&n)
                || matches!(p(2), 30 | 36 | 38 | 39) && (14..=19).contains(&n)
        }
        b'6' => (60..=69).contains(&p(2)) && (16..=19).contains(&n),
        _ => false,
    };
    brand && d.bytes().any(|b| b != d.as_bytes()[1]) && luhn(d)
}

fn luhn(d: &str) -> bool {
    let sum: u32 = d
        .bytes()
        .rev()
        .enumerate()
        .map(|(i, b)| {
            let v = u32::from(b - b'0');
            if i % 2 == 1 { if v * 2 > 9 { v * 2 - 9 } else { v * 2 } } else { v }
        })
        .sum();
    sum.is_multiple_of(10)
}

// ---- IBAN ------------------------------------------------------------------

/// IBAN lengths by country (SWIFT registry); others are tried at 15–34.
#[rustfmt::skip]
const IBAN_LEN: &[(&str, usize)] = &[
    ("AD", 24), ("AE", 23), ("AL", 28), ("AT", 20), ("AZ", 28), ("BA", 20), ("BE", 16), ("BG", 22), ("BH", 22), ("BR", 29),
    ("CH", 21), ("CR", 22), ("CY", 28), ("CZ", 24), ("DE", 22), ("DK", 18), ("DO", 28), ("EE", 20), ("EG", 29), ("ES", 24),
    ("FI", 18), ("FO", 18), ("FR", 27), ("GB", 22), ("GE", 22), ("GI", 23), ("GL", 18), ("GR", 27), ("GT", 28), ("HR", 21),
    ("HU", 28), ("IE", 22), ("IL", 23), ("IS", 26), ("IT", 27), ("JO", 30), ("KW", 30), ("KZ", 20), ("LB", 28), ("LI", 21),
    ("LT", 20), ("LU", 20), ("LV", 21), ("MC", 27), ("MD", 24), ("ME", 22), ("MK", 19), ("MR", 27), ("MT", 31), ("MU", 30),
    ("NL", 18), ("NO", 15), ("PK", 24), ("PL", 28), ("PS", 29), ("PT", 25), ("QA", 29), ("RO", 24), ("RS", 22), ("SA", 24),
    ("SE", 24), ("SI", 19), ("SK", 24), ("SM", 27), ("TN", 24), ("TR", 26), ("UA", 29), ("VG", 24), ("XK", 20),
];

/// The country code of the first valid IBAN.
fn iban(text: &str) -> Option<String> {
    let chars: Vec<char> = text.chars().map(|c| c.to_ascii_uppercase()).collect();
    for i in 0..chars.len().saturating_sub(4) {
        let starts_word = i == 0 || !chars[i - 1].is_ascii_alphanumeric();
        if !(starts_word
            && chars[i].is_ascii_uppercase()
            && chars[i + 1].is_ascii_uppercase()
            && chars[i + 2].is_ascii_digit()
            && chars[i + 3].is_ascii_digit())
        {
            continue;
        }
        // Collect alphanumerics, allowing single spaces between groups.
        let mut s = String::new();
        let mut j = i;
        while j < chars.len() && s.len() < 34 {
            let c = chars[j];
            if c.is_ascii_alphanumeric() {
                s.push(c);
            } else if !(c == ' ' && j + 1 < chars.len() && chars[j + 1].is_ascii_alphanumeric()) {
                break;
            }
            j += 1;
        }
        let cc = &s[..2];
        let lens: Vec<usize> = match IBAN_LEN.iter().find(|(c, _)| *c == cc) {
            Some((_, n)) => vec![*n],
            None => (15..=34).collect(),
        };
        for n in lens {
            if s.len() >= n && mod97(&s[..n]) == 1 {
                return Some(cc.to_string());
            }
        }
    }
    None
}

fn mod97(s: &str) -> u32 {
    let (head, tail) = s.split_at(4);
    let mut r = 0u32;
    for c in tail.chars().chain(head.chars()) {
        let v = match c {
            '0'..='9' => c as u32 - '0' as u32,
            'A'..='Z' => c as u32 - 'A' as u32 + 10,
            _ => return 0,
        };
        r = if v >= 10 { (r * 100 + v) % 97 } else { (r * 10 + v) % 97 };
    }
    r
}

// ---- MRZ -------------------------------------------------------------------

/// "a passport" or "an ID card" when the text holds MRZ lines whose check
/// digits mostly agree.
fn mrz(text: &str) -> Option<&'static str> {
    let lines: Vec<String> = text
        .lines()
        .map(|l| l.chars().filter(|c| !c.is_whitespace()).map(|c| if c == '«' { '<' } else { c.to_ascii_uppercase() }).collect::<String>())
        .filter(|l| l.len() >= 28 && l.chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '<'))
        .collect();
    for w in 0..lines.len() {
        let l1 = &lines[w];
        let l2 = lines.get(w + 1);
        let l3 = lines.get(w + 2);
        // Passport (TD3): 2 × 44. Visa or older ID (TD2): 2 × 36.
        if let Some(l2) = l2
            && (42..=46).contains(&l1.len())
            && (42..=46).contains(&l2.len())
            && l1.starts_with('P')
            && votes(l2, &[(0, 9, 9), (13, 19, 19), (21, 27, 27)]) >= 2
        {
            return Some("a passport");
        }
        if let Some(l2) = l2
            && (34..=38).contains(&l1.len())
            && (34..=38).contains(&l2.len())
            && matches!(l1.as_bytes()[0], b'I' | b'A' | b'C' | b'V' | b'P')
            && votes(l2, &[(0, 9, 9), (13, 19, 19), (21, 27, 27)]) >= 2
        {
            return Some(if l1.starts_with('V') { "a visa" } else { "an ID card" });
        }
        // ID card (TD1): 3 × 30.
        if let (Some(l2), Some(_)) = (l2, l3)
            && (29..=31).contains(&l1.len())
            && (29..=31).contains(&l2.len())
            && matches!(l1.as_bytes()[0], b'I' | b'A' | b'C')
            && votes(l1, &[(5, 14, 14)]) + votes(l2, &[(0, 6, 6), (8, 14, 14)]) >= 2
        {
            return Some("an ID card");
        }
    }
    // Only the first passport line came through OCR.
    lines
        .iter()
        .any(|l| l.len() >= 40 && l.starts_with("P<") && l[2..5].chars().all(|c| c.is_ascii_uppercase()) && l.matches("<<").count() >= 3)
        .then_some("a passport")
}

/// How many of the `(start, end, check)` fields carry a correct check digit.
fn votes(line: &str, fields: &[(usize, usize, usize)]) -> usize {
    let b = line.as_bytes();
    fields.iter().filter(|(s, e, c)| *c < b.len() && b[*c].is_ascii_digit() && check_digit(&b[*s..*e]) == u32::from(b[*c] - b'0')).count()
}

fn check_digit(field: &[u8]) -> u32 {
    const W: [u32; 3] = [7, 3, 1];
    field
        .iter()
        .enumerate()
        .map(|(i, &c)| {
            let v = match c {
                b'0'..=b'9' => u32::from(c - b'0'),
                b'A'..=b'Z' => u32::from(c - b'A') + 10,
                _ => 0,
            };
            v * W[i % 3]
        })
        .sum::<u32>()
        % 10
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(t: &str) -> Vec<Kind> {
        scan(t).into_iter().map(|f| f.kind).collect()
    }

    #[test]
    fn card_numbers() {
        // Published test numbers.
        for (t, last) in [
            ("4242 4242 4242 4242", "4242"),
            ("Card: 5555-5555-5555-4444 exp 12/29", "4444"),
            ("3782 822463 10005", "0005"),
            ("6011111111111117", "1117"),
            ("2223003122003222", "3222"),
        ] {
            assert_eq!(card(t).as_deref(), Some(last), "{t}");
        }
        let f = scan("VISA 4242 4242 4242 4242");
        assert_eq!(f[0].why, "Payment card number ending 4242");
        assert!(!f[0].why.contains("4242 4242"));
        // Luhn-invalid, wrong brand, phone numbers, dates, all-same digits.
        for t in [
            "4242 4242 4242 4241",
            "1234 5678 9012 3452",
            "Tel. +41 44 668 18 00, Fax +41 44 668 18 01",
            "2019-08-03 09:08:28 2019-08-03 09:08:29",
            "0000 0000 0000 0000",
            "Order 4242424242424242424242",
        ] {
            assert_eq!(card(t), None, "{t}");
        }
        // A card number next to other digits still counts.
        assert_eq!(card("Ref 12 4242 4242 4242 4242").as_deref(), Some("4242"));
    }

    #[test]
    fn ibans() {
        assert_eq!(iban("IBAN CH93 0076 2011 6238 5295 7").as_deref(), Some("CH"));
        assert_eq!(iban("de89370400440532013000").as_deref(), Some("DE"));
        assert_eq!(iban("Konto: GB29 NWBK 6016 1331 9268 19, BIC NWBKGB2L").as_deref(), Some("GB"));
        assert_eq!(scan("NL91ABNA0417164300")[0].why, "Bank account number (IBAN, NL)");
        // One wrong digit, or a word that only looks like the start of one.
        assert_eq!(iban("CH93 0076 2011 6238 5295 8"), None);
        assert_eq!(iban("AB12 is a bus line, ZZ99 a model number 12345678901234"), None);
        assert_eq!(iban("ISO12345"), None);
    }

    #[test]
    fn passports_and_id_cards() {
        // ICAO 9303 specimens.
        let td3 = "PASSPORT\nP<UTOERIKSSON<<ANNA<MARIA<<<<<<<<<<<<<<<<<<<\nL898902C36UTO7408122F1204159ZE184226B<<<<<10";
        assert_eq!(mrz(td3), Some("a passport"));
        assert_eq!(kinds(td3), vec![Kind::Mrz]);
        // OCR noise: spaces, « for <, one misread check digit.
        let noisy = "P<UTOERIKSSON<<ANNA<MARIA«<<<<<<<<<<<<<<<<<<\nL898902C3 6UTO7408122F1204158ZE184226B<<<<<10";
        assert_eq!(mrz(noisy), Some("a passport"));
        let td1 = "I<UTOD231458907<<<<<<<<<<<<<<<\n7408122F1204159UTO<<<<<<<<<<<6\nERIKSSON<<ANNA<MARIA<<<<<<<<<<";
        assert_eq!(mrz(td1), Some("an ID card"));
        // Only the name line came through.
        assert_eq!(mrz("P<UTOERIKSSON<<ANNA<MARIA<<<<<<<<<<<<<<<<<<<"), Some("a passport"));
        // Look-alikes: a meme with a long word, code with angle brackets,
        // and two MRZ-shaped lines whose check digits are all wrong.
        assert_eq!(mrz("AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"), None);
        assert_eq!(mrz("Vec<Vec<String>> = Vec::new();"), None);
        let wrong = "PROGRAMMINGLANGUAGESANDTHEIRCOMPILERS1234567
L898902C38UTO7408121F1204150ZE184226B<<<<<10";
        assert_eq!(mrz(wrong), None);
    }

    #[test]
    fn ordinary_text_is_not_private() {
        for t in [
            "",
            "When you finally fix the bug at 3am",
            "Total CHF 42.50 MwSt 8.1% Beleg 2019-0803-0908",
            "Ticket 1234567 Gate B22 Seat 14C Flight LX 1612",
            "github.com/mrghxst/roll 1.0.104",
        ] {
            assert!(scan(t).is_empty(), "{t}");
        }
    }
}
