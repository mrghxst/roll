//! Rules: deterministic, explainable sorting decisions stored as data.
//! A rule matches on file facts (name, camera, size, markers…) and proposes
//! a category, tags, or both. Only rules marked `certain` decide on their
//! own; the rest are proposals that Review and the models refine.

use regex::Regex;
use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "snake_case")]
pub enum Op {
    Eq,
    Ne,
    Lt,
    Gt,
    Regex,
    Exists,
    Missing,
    In,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Condition {
    /// name, ext, format, dir, make, model, software, kind, screenshot,
    /// has_camera, width, height, long_side, aspect, duration_ms, size,
    /// broken, junk.
    pub field: String,
    pub op: Op,
    #[ts(type = "string | number | boolean | string[] | null")]
    pub value: serde_json::Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Then {
    pub category: Option<String>,
    pub confidence: f32,
    /// Decides without review.
    pub certain: bool,
    pub tags: Vec<String>,
    /// Shown as the "why": "the iOS screenshot marker".
    pub because: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct RuleDef {
    pub when: Vec<Condition>,
    pub then: Then,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Rule {
    pub id: i32,
    pub key: Option<String>,
    pub name: String,
    pub enabled: bool,
    pub priority: i32,
    pub def: RuleDef,
    pub builtin: bool,
}

fn cond(field: &str, op: Op, value: serde_json::Value) -> Condition {
    Condition { field: field.into(), op, value }
}

fn then(category: Option<&str>, confidence: f32, certain: bool, tags: &[&str], because: &str) -> Then {
    Then {
        category: category.map(String::from),
        confidence,
        certain,
        tags: tags.iter().map(|t| t.to_string()).collect(),
        because: because.into(),
    }
}

use serde_json::json;

/// Makers of dedicated cameras. Their files are camera photos.
pub const CAMERA_BRANDS: &[&str] = &[
    "canon",
    "nikon",
    "sony",
    "fujifilm",
    "panasonic",
    "olympus",
    "om digital",
    "pentax",
    "ricoh",
    "leica",
    "hasselblad",
    "gopro",
    "dji",
    "sigma",
    "kodak",
    "minolta",
    "konica",
    "samsung techwin",
    "casio",
    "insta360",
];

/// The rules Roll starts with, as (key, name, priority, definition).
pub fn defaults() -> Vec<(&'static str, &'static str, i32, RuleDef)> {
    vec![
        (
            "ios_screenshot_marker",
            "iOS screenshot marker",
            10,
            RuleDef {
                when: vec![cond("screenshot", Op::Eq, json!(true))],
                then: then(Some("screenshots"), 1.0, true, &[], "the iOS screenshot marker"),
            },
        ),
        (
            "screenshot_name",
            "Screenshot file name",
            11,
            RuleDef {
                when: vec![cond("name", Op::Regex, json!(r"(?i)^(screenshot|screen shot|scrnshot)[ _-]"))],
                then: then(Some("screenshots"), 0.99, true, &[], "a screenshot file name"),
            },
        ),
        (
            "screen_recording_name",
            "Screen recording file name",
            12,
            RuleDef {
                when: vec![cond("name", Op::Regex, json!(r"(?i)^(rpreplay|screen[ _-]?recording|screenrecord)"))],
                then: then(Some("screenshots"), 0.99, true, &[], "a screen recording file name"),
            },
        ),
        (
            "whatsapp_name",
            "WhatsApp file name",
            20,
            RuleDef {
                when: vec![cond("name", Op::Regex, json!(r"^(IMG|VID|AUD|PTT|STK)-\d{8}-WA\d+"))],
                then: then(None, 1.0, true, &["Source/WhatsApp"], "a WhatsApp file name"),
            },
        ),
        (
            "telegram",
            "Telegram file name or folder",
            21,
            RuleDef {
                when: vec![cond(
                    "path",
                    Op::Regex,
                    json!(r"(?i)(^|/)telegram( (desktop|images|video))?(/|$)|(^|/)photo_\d{4}-\d{2}-\d{2}_\d{2}-\d{2}-\d{2}"),
                )],
                then: then(None, 0.95, true, &["Source/Telegram"], "a Telegram file name or folder"),
            },
        ),
        (
            "snapchat",
            "Snapchat file name or app",
            22,
            RuleDef {
                when: vec![cond("name_or_software", Op::Regex, json!(r"(?i)^snapchat"))],
                then: then(None, 0.95, true, &["Source/Snapchat"], "a Snapchat file name or app tag"),
            },
        ),
        (
            "facebook",
            "Facebook file name",
            23,
            RuleDef {
                when: vec![cond("name", Op::Regex, json!(r"^FB_IMG_\d+"))],
                then: then(None, 0.95, true, &["Source/Facebook"], "a Facebook file name"),
            },
        ),
        (
            "messenger",
            "Messenger file name",
            24,
            RuleDef {
                when: vec![cond("name", Op::Regex, json!(r"^received_\d+"))],
                then: then(None, 0.9, true, &["Source/Messenger"], "a Messenger file name"),
            },
        ),
        (
            "instagram_software",
            "Instagram app tag",
            25,
            RuleDef {
                when: vec![cond("software", Op::Regex, json!(r"(?i)instagram"))],
                then: then(None, 0.95, true, &["Source/Instagram"], "the Instagram app tag"),
            },
        ),
        (
            "canva_software",
            "Canva app tag",
            26,
            RuleDef {
                when: vec![cond("software", Op::Regex, json!(r"(?i)canva"))],
                then: then(None, 0.95, true, &["Source/Canva"], "the Canva app tag"),
            },
        ),
        (
            "dedicated_camera",
            "Dedicated camera",
            30,
            RuleDef {
                when: vec![cond("camera_brand", Op::Eq, json!(true))],
                then: then(Some("photos"), 0.98, true, &[], "a dedicated camera's make"),
            },
        ),
        (
            "phone_camera",
            "Phone camera",
            31,
            RuleDef {
                when: vec![cond("has_camera", Op::Eq, json!(true))],
                then: then(Some("photos"), 0.85, false, &[], "camera data in the file"),
            },
        ),
        (
            "broken",
            "Broken file",
            28,
            RuleDef { when: vec![cond("broken", Op::Eq, json!(true))], then: then(Some("junk"), 0.9, false, &[], "the file is broken") },
        ),
        (
            "junk_pixels",
            "Black, white, tiny or very blurry",
            29,
            RuleDef {
                when: vec![cond("junk", Op::Exists, json!(null))],
                then: then(Some("junk"), 0.8, false, &[], "the image looks like junk"),
            },
        ),
        (
            "gif",
            "Animated GIF",
            50,
            RuleDef { when: vec![cond("format", Op::Eq, json!("gif"))], then: then(Some("memes"), 0.7, false, &[], "it is a GIF") },
        ),
    ]
}

/// Insert the default rules once.
pub fn seed(c: &Connection) -> rusqlite::Result<()> {
    for (key, name, priority, def) in defaults() {
        c.execute(
            "INSERT INTO rules (key, name, enabled, priority, def, builtin) VALUES (?1, ?2, 1, ?3, ?4, 1) ON CONFLICT(key) DO NOTHING",
            params![key, name, priority, serde_json::to_string(&def).unwrap_or_default()],
        )?;
    }
    Ok(())
}

pub fn load(c: &Connection) -> rusqlite::Result<Vec<Rule>> {
    let mut st = c.prepare("SELECT id, key, name, enabled, priority, def, builtin FROM rules ORDER BY priority, id")?;
    let rows = st.query_map([], |r| {
        let def: String = r.get(5)?;
        Ok(Rule {
            id: r.get(0)?,
            key: r.get(1)?,
            name: r.get(2)?,
            enabled: r.get::<_, i64>(3)? != 0,
            priority: r.get(4)?,
            def: serde_json::from_str(&def)
                .map_err(|e| rusqlite::Error::FromSqlConversionFailure(5, rusqlite::types::Type::Text, Box::new(e)))?,
            builtin: r.get::<_, i64>(6)? != 0,
        })
    })?;
    rows.collect()
}

/// The facts a rule can look at.
#[derive(Debug, Clone, Default)]
pub struct Facts {
    pub name: String,
    pub path: String,
    pub ext: String,
    pub format: String,
    pub make: Option<String>,
    pub model: Option<String>,
    pub software: Option<String>,
    pub video: bool,
    pub screenshot: bool,
    pub width: Option<i64>,
    pub height: Option<i64>,
    pub duration_ms: Option<i64>,
    pub size: i64,
    pub broken: bool,
    pub junk: Option<String>,
}

impl Facts {
    fn camera_brand(&self) -> bool {
        let make = self.make.as_deref().unwrap_or("").to_lowercase();
        !make.is_empty() && CAMERA_BRANDS.iter().any(|b| make.starts_with(b))
    }

    fn value(&self, field: &str) -> serde_json::Value {
        match field {
            "name" => json!(self.name),
            "path" => json!(self.path),
            "ext" => json!(self.ext),
            "format" => json!(self.format),
            "make" => json!(self.make),
            "model" => json!(self.model),
            "software" => json!(self.software),
            "name_or_software" => json!(format!("{}\n{}", self.name, self.software.as_deref().unwrap_or(""))),
            "kind" => json!(if self.video { "video" } else { "photo" }),
            "screenshot" => json!(self.screenshot),
            "has_camera" => json!(
                self.make.as_deref().is_some_and(|m| !m.trim().is_empty()) || self.model.as_deref().is_some_and(|m| !m.trim().is_empty())
            ),
            "camera_brand" => json!(self.camera_brand()),
            "width" => json!(self.width),
            "height" => json!(self.height),
            "long_side" => json!(self.width.zip(self.height).map(|(w, h)| w.max(h))),
            "aspect" => json!(self.width.zip(self.height).filter(|(_, h)| *h > 0).map(|(w, h)| w as f64 / h as f64)),
            "duration_ms" => json!(self.duration_ms),
            "size" => json!(self.size),
            "broken" => json!(self.broken),
            "junk" => json!(self.junk),
            _ => serde_json::Value::Null,
        }
    }
}

fn num(v: &serde_json::Value) -> Option<f64> {
    v.as_f64()
}

pub fn matches(c: &Condition, f: &Facts, re_cache: &mut std::collections::HashMap<String, Option<Regex>>) -> bool {
    let v = f.value(&c.field);
    match c.op {
        Op::Exists => !v.is_null() && v != json!(false) && v != json!(""),
        Op::Missing => v.is_null() || v == json!(""),
        Op::Eq => {
            v == c.value || (v.is_string() && c.value.is_string() && v.as_str().unwrap().eq_ignore_ascii_case(c.value.as_str().unwrap()))
        }
        Op::Ne => v != c.value,
        Op::Lt => matches!((num(&v), num(&c.value)), (Some(a), Some(b)) if a < b),
        Op::Gt => matches!((num(&v), num(&c.value)), (Some(a), Some(b)) if a > b),
        Op::In => c.value.as_array().is_some_and(|list| {
            list.iter().any(|x| x == &v || (x.as_str().zip(v.as_str()).is_some_and(|(a, b)| a.eq_ignore_ascii_case(b))))
        }),
        Op::Regex => {
            let Some(pat) = c.value.as_str() else { return false };
            let Some(text) = v.as_str() else { return false };
            let re = re_cache.entry(pat.to_string()).or_insert_with(|| Regex::new(pat).ok());
            re.as_ref().is_some_and(|r| r.is_match(text))
        }
    }
}

/// The outcome of the rules for one asset.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Decision {
    /// (category key, confidence, certain, rule key or id, because)
    pub category: Option<(String, f32, bool, String, String)>,
    /// (tag, rule key, because)
    pub tags: Vec<(String, String, String)>,
}

/// Run every enabled rule. The first matching rule that sets a category
/// wins (rules are in priority order); tags from all matching rules apply.
pub fn evaluate(rules: &[Rule], f: &Facts) -> Decision {
    let mut d = Decision::default();
    let mut cache = std::collections::HashMap::new();
    for r in rules.iter().filter(|r| r.enabled) {
        if r.def.when.is_empty() || !r.def.when.iter().all(|c| matches(c, f, &mut cache)) {
            continue;
        }
        let key = r.key.clone().unwrap_or_else(|| format!("rule:{}", r.id));
        if let (Some(cat), None) = (&r.def.then.category, &d.category) {
            d.category = Some((cat.clone(), r.def.then.confidence, r.def.then.certain, key.clone(), r.def.then.because.clone()));
        }
        for t in &r.def.then.tags {
            if !d.tags.iter().any(|(x, _, _)| x == t) {
                d.tags.push((t.clone(), key.clone(), r.def.then.because.clone()));
            }
        }
    }
    d
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rules() -> Vec<Rule> {
        let mut v: Vec<Rule> = defaults()
            .into_iter()
            .enumerate()
            .map(|(i, (k, n, p, d))| Rule {
                id: i as i32,
                key: Some(k.into()),
                name: n.into(),
                enabled: true,
                priority: p,
                def: d,
                builtin: true,
            })
            .collect();
        v.sort_by_key(|r| r.priority);
        v
    }

    fn facts(name: &str) -> Facts {
        Facts { name: name.into(), path: format!("Dump/{name}"), format: "jpeg".into(), ..Default::default() }
    }

    #[test]
    fn screenshots_are_certain() {
        let d = evaluate(&rules(), &Facts { screenshot: true, ..facts("IMG_1.PNG") });
        assert_eq!(d.category.as_ref().map(|c| (c.0.as_str(), c.2)), Some(("screenshots", true)));
        let d = evaluate(&rules(), &facts("Screenshot_20200301-101500.png"));
        assert_eq!(d.category.unwrap().0, "screenshots");
        let d = evaluate(&rules(), &facts("RPReplay_Final1612345678.MP4"));
        assert_eq!(d.category.unwrap().0, "screenshots");
    }

    #[test]
    fn sources_and_cameras() {
        let d = evaluate(&rules(), &facts("IMG-20200102-WA0004.jpg"));
        assert!(d.category.is_none());
        assert_eq!(d.tags[0].0, "Source/WhatsApp");
        let d = evaluate(&rules(), &Facts { make: Some("NIKON CORPORATION".into()), ..facts("DSC_0001.JPG") });
        assert_eq!(d.category.as_ref().map(|c| (c.0.as_str(), c.2)), Some(("photos", true)));
        let d = evaluate(&rules(), &Facts { make: Some("Apple".into()), ..facts("IMG_1.HEIC") });
        assert_eq!(d.category.as_ref().map(|c| (c.0.as_str(), c.2)), Some(("photos", false)));
        let d = evaluate(&rules(), &Facts { software: Some("Instagram".into()), ..facts("IMG_7301.JPG") });
        assert_eq!(d.tags[0].0, "Source/Instagram");
        let d = evaluate(&rules(), &facts("photo_2021-03-04_10-11-12.jpg"));
        assert_eq!(d.tags[0].0, "Source/Telegram");
    }

    #[test]
    fn junk_and_gif() {
        // Junk outranks the phone camera prior.
        let d = evaluate(&rules(), &Facts { junk: Some("almost black".into()), make: Some("Apple".into()), ..facts("IMG_6101.JPG") });
        assert_eq!(d.category.as_ref().map(|c| (c.0.as_str(), c.2)), Some(("junk", false)));
        let d = evaluate(&rules(), &Facts { junk: Some("almost black".into()), ..facts("x.jpg") });
        assert_eq!(d.category.unwrap().0, "junk");
        let d = evaluate(&rules(), &Facts { format: "gif".into(), ..facts("a.gif") });
        assert_eq!(d.category.unwrap().0, "memes");
    }

    #[test]
    fn ops() {
        let f = Facts { width: Some(100), height: Some(50), ..facts("a.jpg") };
        let mut c = std::collections::HashMap::new();
        assert!(matches(&cond("long_side", Op::Lt, json!(200)), &f, &mut c));
        assert!(matches(&cond("aspect", Op::Gt, json!(1.5)), &f, &mut c));
        assert!(matches(&cond("format", Op::In, json!(["png", "JPEG"])), &f, &mut c));
        assert!(matches(&cond("make", Op::Missing, json!(null)), &f, &mut c));
        assert!(!matches(&cond("name", Op::Regex, json!("(")), &f, &mut c));
    }
}
