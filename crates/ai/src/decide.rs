//! The decision prompt (PROMPT 5.5): numbered thumbnails with short hints,
//! one category each, as JSON checked against a schema. The prompt is
//! versioned; results are stored with the version that produced them.

use base64::Engine;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

pub const PROMPT_VERSION: &str = "decide-1";

pub const SOURCES: [&str; 11] =
    ["instagram", "tiktok", "snapchat", "whatsapp", "telegram", "reddit", "x", "facebook", "pinterest", "youtube", "other"];

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CategoryDef {
    pub key: String,
    pub definition: String,
}

/// What the model may know about an item besides the picture. Never file
/// names, paths, places or dates.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Hints {
    pub camera: bool,
    pub screenshot_marker: bool,
    pub width: Option<i64>,
    pub height: Option<i64>,
    /// Seconds, for videos (the image is a strip of 3 frames).
    pub video_secs: Option<f64>,
}

impl Hints {
    pub fn line(&self, i: usize) -> String {
        let yn = |b: bool| if b { "yes" } else { "no" };
        let size = match (self.width, self.height) {
            (Some(w), Some(h)) => format!("{w}x{h}"),
            _ => "unknown".into(),
        };
        let video = match self.video_secs {
            Some(s) => format!("yes, {s:.0} s, 3 frames side by side"),
            None => "no".into(),
        };
        format!("Image {i}: camera={}, screenshot_marker={}, size={size}, video={video}", yn(self.camera), yn(self.screenshot_marker))
    }
}

pub struct Item {
    pub hints: Hints,
    /// JPEG, at most 512 px, without metadata.
    pub jpeg: Vec<u8>,
}

pub fn system_prompt(cats: &[CategoryDef]) -> String {
    let mut s = String::from(
        "You sort a personal photo library. For each numbered image choose exactly one \
category from the list, using the definitions and the hints. If none fits or \
you can't tell, answer \"unsure\". Be calibrated: confidence is the probability \
that your category is right. Reply with JSON only.\n\nCategories:\n",
    );
    for c in cats {
        s.push_str(&format!("- {}: {}\n", c.key, c.definition.trim()));
    }
    s.push_str(&format!(
        "Sources (optional): {}\n\nFor each image also give a caption of at most 12 words, whether it shows readable text (has_text), \
and whether it shows private documents such as IDs, bank cards or medical papers (private).",
        SOURCES.join(", ")
    ));
    s
}

pub fn schema(cats: &[CategoryDef]) -> Value {
    let mut keys: Vec<&str> = cats.iter().map(|c| c.key.as_str()).collect();
    keys.push("unsure");
    let mut sources: Vec<Value> = SOURCES.iter().map(|s| json!(s)).collect();
    sources.push(Value::Null);
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": ["items"],
        "properties": {
            "items": {
                "type": "array",
                "items": {
                    "type": "object",
                    "additionalProperties": false,
                    "required": ["i", "category", "confidence", "source", "caption", "has_text", "private"],
                    "properties": {
                        "i": {"type": "integer"},
                        "category": {"type": "string", "enum": keys},
                        "confidence": {"type": "number"},
                        "source": {"type": ["string", "null"], "enum": sources},
                        "caption": {"type": "string"},
                        "has_text": {"type": "boolean"},
                        "private": {"type": "boolean"}
                    }
                }
            }
        }
    })
}

/// Request settings that differ between OpenRouter and local servers.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Options {
    pub openrouter: bool,
    /// Owner allowed endpoints that keep data.
    pub allow_non_zdr: bool,
    /// "none" first; "minimal" for models that cannot turn reasoning off.
    pub reasoning: &'static str,
}

pub fn body(model: &str, cats: &[CategoryDef], items: &[Item], o: Options) -> Value {
    let mut content = Vec::with_capacity(items.len() * 2);
    for (n, it) in items.iter().enumerate() {
        content.push(json!({"type": "text", "text": it.hints.line(n + 1)}));
        let b64 = base64::engine::general_purpose::STANDARD.encode(&it.jpeg);
        content.push(json!({"type": "image_url", "image_url": {"url": format!("data:image/jpeg;base64,{b64}")}}));
    }
    let mut b = json!({
        "model": model,
        "temperature": 0,
        "max_tokens": crate::cost::max_tokens(items.len()),
        "messages": [
            {"role": "system", "content": system_prompt(cats)},
            {"role": "user", "content": content}
        ],
        "response_format": {"type": "json_schema", "json_schema": {"name": "verdicts", "strict": true, "schema": schema(cats)}}
    });
    if o.openrouter {
        let mut provider = json!({"data_collection": "deny", "require_parameters": true, "sort": "price"});
        if !o.allow_non_zdr {
            provider["zdr"] = json!(true);
        }
        b["provider"] = provider;
        b["reasoning"] = json!({"effort": o.reasoning, "exclude": true});
    }
    b
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Verdict {
    /// `None` for "unsure".
    pub category: Option<String>,
    pub confidence: f64,
    pub source: Option<String>,
    pub caption: Option<String>,
    pub has_text: bool,
    pub private: bool,
}

/// Check a reply against the schema. Returns one entry per item (`None`
/// where the reply has nothing usable for it), or an error when the reply
/// is not the JSON asked for at all.
pub fn parse(content: &str, n: usize, cats: &[CategoryDef]) -> Result<Vec<Option<Verdict>>, String> {
    let text = content.trim();
    let start = text.find('{').ok_or("no JSON object in the reply")?;
    let end = text.rfind('}').ok_or("no JSON object in the reply")?;
    let v: Value = serde_json::from_str(&text[start..=end]).map_err(|e| e.to_string())?;
    let items = v["items"].as_array().ok_or("no items in the reply")?;
    let mut out = vec![None; n];
    for it in items {
        let Some(i) = it["i"].as_u64().map(|i| i as usize).filter(|i| (1..=n).contains(i)) else { continue };
        let Some(cat) = it["category"].as_str() else { continue };
        let Some(conf) = it["confidence"].as_f64() else { continue };
        let category = (cat != "unsure").then(|| cat.to_string()).filter(|c| cats.iter().any(|d| &d.key == c));
        if category.is_none() && cat != "unsure" {
            continue;
        }
        let caption = it["caption"].as_str().map(|c| c.split_whitespace().take(12).collect::<Vec<_>>().join(" ")).filter(|c| !c.is_empty());
        out[i - 1] = Some(Verdict {
            category,
            confidence: conf.clamp(0.0, 1.0),
            source: it["source"].as_str().filter(|s| SOURCES.contains(s)).map(String::from),
            caption,
            has_text: it["has_text"].as_bool().unwrap_or(false),
            private: it["private"].as_bool().unwrap_or(false),
        });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cats() -> Vec<CategoryDef> {
        ["photos", "screenshots", "memes"].iter().map(|k| CategoryDef { key: k.to_string(), definition: format!("{k} def") }).collect()
    }

    #[test]
    fn body_carries_hints_images_and_privacy_settings() {
        let items = vec![
            Item { hints: Hints { camera: true, width: Some(4032), height: Some(3024), ..Default::default() }, jpeg: vec![0xff, 0xd8] },
            Item { hints: Hints { video_secs: Some(12.4), ..Default::default() }, jpeg: vec![0xff, 0xd8] },
        ];
        let b = body("g/gemma", &cats(), &items, Options { openrouter: true, allow_non_zdr: false, reasoning: "none" });
        let user = b["messages"][1]["content"].as_array().unwrap();
        assert_eq!(user.len(), 4);
        assert_eq!(user[0]["text"], "Image 1: camera=yes, screenshot_marker=no, size=4032x3024, video=no");
        assert!(user[2]["text"].as_str().unwrap().contains("video=yes, 12 s"));
        assert!(user[1]["image_url"]["url"].as_str().unwrap().starts_with("data:image/jpeg;base64,"));
        assert_eq!(b["provider"]["zdr"], true);
        assert_eq!(b["provider"]["data_collection"], "deny");
        assert_eq!(b["temperature"], 0);
        let enums = &b["response_format"]["json_schema"]["schema"]["properties"]["items"]["items"]["properties"]["category"]["enum"];
        assert_eq!(enums.as_array().unwrap().last().unwrap(), "unsure");
        let local = body("qwen3-vl:4b", &cats(), &items, Options { openrouter: false, allow_non_zdr: false, reasoning: "none" });
        assert!(local.get("provider").is_none() && local.get("reasoning").is_none());
    }

    #[test]
    fn replies_are_checked() {
        let reply = "```json\n{\"items\":[{\"i\":2,\"category\":\"memes\",\"confidence\":1.3,\"source\":\"reddit\",\
\"caption\":\"a cat that is surprised by a cucumber on the kitchen floor at night\",\"has_text\":true,\"private\":false},\
{\"i\":1,\"category\":\"unsure\",\"confidence\":0.2,\"source\":null,\"caption\":\"\",\"has_text\":false,\"private\":false},\
{\"i\":9,\"category\":\"memes\",\"confidence\":0.9},{\"i\":3,\"category\":\"cats\",\"confidence\":0.9}]}\n```";
        let v = parse(reply, 3, &cats()).unwrap();
        let m = v[1].as_ref().unwrap();
        assert_eq!((m.category.as_deref(), m.confidence, m.source.as_deref()), (Some("memes"), 1.0, Some("reddit")));
        assert_eq!(m.caption.as_deref().unwrap().split(' ').count(), 12);
        assert_eq!(v[0].as_ref().unwrap().category, None);
        assert!(v[2].is_none(), "unknown category");
        assert!(parse("Sorry, I can't help with that.", 3, &cats()).is_err());
    }
}
