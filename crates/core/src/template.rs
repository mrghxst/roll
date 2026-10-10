//! Folder and file name templates: `{year}/{yyyy-mm} {event}`,
//! `{date} {time} {camera}`. Empty tokens drop out; names are made safe for
//! Windows, macOS and Linux and kept short.

const MONTHS: [&str; 12] =
    ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"];

/// Longest folder or file name segment Roll generates (characters).
pub const MAX_SEGMENT: usize = 100;

#[derive(Debug, Clone, Default)]
pub struct Ctx {
    /// `YYYY-MM-DDTHH:MM:SS` local time.
    pub local: Option<String>,
    pub event: Option<String>,
    pub category: Option<String>,
    pub subcategory: Option<String>,
    pub source: Option<String>,
    pub camera: Option<String>,
    pub country: Option<String>,
    pub city: Option<String>,
}

fn token(name: &str, ctx: &Ctx) -> Option<String> {
    let l = ctx.local.as_deref().filter(|l| l.len() >= 19);
    Some(match name {
        "year" => l.map(|l| l[0..4].to_string()).unwrap_or_else(|| "Undated".into()),
        "month" => l?[5..7].to_string(),
        "month_name" => MONTHS.get(l?[5..7].parse::<usize>().ok()?.checked_sub(1)?)?.to_string(),
        "date" => l?[0..10].to_string(),
        "yyyy-mm" => l?[0..7].to_string(),
        "time" => l?[11..19].replace(':', "."),
        "event" => ctx.event.clone()?,
        "category" => ctx.category.clone()?,
        "subcategory" => ctx.subcategory.clone()?,
        "source" => ctx.source.clone()?,
        "camera" => ctx.camera.clone()?,
        "country" => ctx.country.clone()?,
        "city" => ctx.city.clone()?,
        _ => return None,
    })
}

/// Render a template. Segments that end up empty are dropped.
pub fn render(template: &str, ctx: &Ctx) -> String {
    template
        .split('/')
        .map(|seg| {
            let mut out = String::new();
            let mut rest = seg;
            while let Some(open) = rest.find('{') {
                out.push_str(&rest[..open]);
                let Some(close) = rest[open..].find('}') else {
                    out.push_str(&rest[open..]);
                    rest = "";
                    break;
                };
                let name = &rest[open + 1..open + close];
                out.push_str(&token(name, ctx).unwrap_or_default());
                rest = &rest[open + close + 1..];
            }
            out.push_str(rest);
            safe_segment(&out)
        })
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("/")
}

const RESERVED: &[&str] = &[
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8", "COM9", "LPT1", "LPT2", "LPT3", "LPT4",
    "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

/// A folder or file name that works everywhere: no reserved characters, no
/// trailing dots or spaces, no reserved device names, at most MAX_SEGMENT.
pub fn safe_segment(s: &str) -> String {
    let cleaned: String = s.chars().map(|c| if c.is_control() || "<>:\"/\\|?*".contains(c) { ' ' } else { c }).collect();
    let collapsed = cleaned.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut out = shorten(collapsed.trim_matches(|c: char| c == '.' || c == ' '), MAX_SEGMENT);
    let stem = out.split('.').next().unwrap_or("").to_ascii_uppercase();
    if RESERVED.contains(&stem.as_str()) {
        out.insert(0, '_');
    }
    out
}

/// Shorten to `max` characters, keeping an extension and marking the cut.
pub fn shorten(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let (stem, ext) = match s.rsplit_once('.') {
        Some((st, e)) if e.len() <= 6 && !st.is_empty() => (st, format!(".{e}")),
        _ => (s, String::new()),
    };
    let keep = max.saturating_sub(ext.chars().count() + 1);
    let cut: String = stem.chars().take(keep).collect();
    format!("{}…{ext}", cut.trim_end())
}

/// Insert " (n)" before the extension.
pub fn numbered(name: &str, n: u32) -> String {
    match name.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() => format!("{stem} ({n}).{ext}"),
        _ => format!("{name} ({n})"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx() -> Ctx {
        Ctx {
            local: Some("2019-08-12T14:03:22".into()),
            event: Some("2019-08 Lake Garda".into()),
            category: Some("Memes".into()),
            camera: Some("NIKON D750".into()),
            ..Default::default()
        }
    }

    #[test]
    fn renders_tokens() {
        assert_eq!(render("{year}/Photos", &ctx()), "2019/Photos");
        assert_eq!(render("{year}/{event}", &ctx()), "2019/2019-08 Lake Garda");
        assert_eq!(render("{year}/{month} {month_name}", &ctx()), "2019/08 August");
        assert_eq!(render("{date} {time} {camera}", &ctx()), "2019-08-12 14.03.22 NIKON D750");
        assert_eq!(render("{year}/{city}/{category}", &ctx()), "2019/Memes");
        assert_eq!(render("Wallpapers", &ctx()), "Wallpapers");
        assert_eq!(render("{year}/Photos", &Ctx::default()), "Undated/Photos");
    }

    #[test]
    fn safe_names() {
        assert_eq!(safe_segment("a:b*c?"), "a b c");
        assert_eq!(safe_segment("trailing. "), "trailing");
        assert_eq!(safe_segment("con"), "_con");
        assert_eq!(safe_segment("NUL.txt"), "_NUL.txt");
        let long = "x".repeat(300);
        assert_eq!(safe_segment(&long).chars().count(), MAX_SEGMENT);
        assert_eq!(shorten(&format!("{}.jpeg", "y".repeat(200)), 20), format!("{}….jpeg", "y".repeat(14)));
        assert_eq!(numbered("IMG_1.JPG", 2), "IMG_1 (2).JPG");
        assert_eq!(numbered("README", 3), "README (3)");
    }
}
