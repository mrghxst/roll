//! Search text to filters through a text model, for the words the offline
//! parser could not place ("pictures of the kids at the beach last
//! Christmas"). Only the leftover words, today's date and the category
//! keys are sent.

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

pub const PROMPT_VERSION: &str = "query-1";

/// What the model understood. Dates are local, `to` exclusive.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct TextQuery {
    pub categories: Vec<String>,
    /// "photo" and/or "video".
    pub media: Vec<String>,
    pub from: Option<String>,
    pub to: Option<String>,
    /// A town, region or country, as written.
    pub place: Option<String>,
    pub favorite: Option<bool>,
    /// Words to look for in names, tags, captions and text in images.
    pub words: String,
}

pub fn body(model: &str, text: &str, today: &str, categories: &[(String, String)], openrouter: bool) -> Value {
    let keys: Vec<&str> = categories.iter().map(|(k, _)| k.as_str()).collect();
    let list = categories.iter().map(|(k, n)| format!("{k} ({n})")).collect::<Vec<_>>().join(", ");
    let system = format!(
        "You turn a search in a personal photo library into filters. Today is {today}. \
Categories: {list}. Use a category only when the search clearly asks for that kind of item. \
Dates are YYYY-MM-DD, `to` is the day after the range. `place` is a town, region or country named in the search. \
`words` keeps what the filters do not cover (subjects, people, objects), in the search's language, without filler words. \
Reply with JSON only."
    );
    let schema = json!({
        "type": "object",
        "additionalProperties": false,
        "required": ["categories", "media", "from", "to", "place", "favorite", "words"],
        "properties": {
            "categories": {"type": "array", "items": {"type": "string", "enum": keys}},
            "media": {"type": "array", "items": {"type": "string", "enum": ["photo", "video"]}},
            "from": {"type": ["string", "null"]},
            "to": {"type": ["string", "null"]},
            "place": {"type": ["string", "null"]},
            "favorite": {"type": ["boolean", "null"]},
            "words": {"type": "string"}
        }
    });
    let mut b = json!({
        "model": model,
        "temperature": 0,
        "max_tokens": 200,
        "messages": [{"role": "system", "content": system}, {"role": "user", "content": text}],
        "response_format": {"type": "json_schema", "json_schema": {"name": "filters", "strict": true, "schema": schema}}
    });
    if openrouter {
        b["provider"] = json!({"zdr": true, "data_collection": "deny", "require_parameters": true, "sort": "price"});
        b["reasoning"] = json!({"effort": "none", "exclude": true});
    }
    b
}

fn date(v: &Value) -> Option<String> {
    let s = v.as_str()?;
    chrono_like(s).then(|| s.to_string())
}

/// "YYYY-MM-DD" with plausible numbers.
fn chrono_like(s: &str) -> bool {
    let p: Vec<&str> = s.split('-').collect();
    p.len() == 3
        && p[0].len() == 4
        && p[0].parse::<u32>().is_ok_and(|y| (1900..2200).contains(&y))
        && p[1].parse::<u32>().is_ok_and(|m| (1..=12).contains(&m))
        && p[2].parse::<u32>().is_ok_and(|d| (1..=31).contains(&d))
}

/// Check a reply; unknown categories and malformed dates are dropped.
pub fn parse(content: &str, categories: &[(String, String)]) -> Result<TextQuery, String> {
    let t = content.trim();
    let (s, e) = (t.find('{').ok_or("no JSON object in the reply")?, t.rfind('}').ok_or("no JSON object in the reply")?);
    let v: Value = serde_json::from_str(&t[s..=e]).map_err(|e| e.to_string())?;
    let strings = |k: &str| -> Vec<String> { v[k].as_array().into_iter().flatten().filter_map(|x| x.as_str().map(String::from)).collect() };
    Ok(TextQuery {
        categories: strings("categories").into_iter().filter(|c| categories.iter().any(|(k, _)| k == c)).collect(),
        media: strings("media").into_iter().filter(|m| m == "photo" || m == "video").collect(),
        from: date(&v["from"]),
        to: date(&v["to"]),
        place: v["place"].as_str().map(str::trim).filter(|p| !p.is_empty()).map(String::from),
        favorite: v["favorite"].as_bool(),
        words: v["words"].as_str().unwrap_or("").trim().to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replies_become_filters() {
        let cats = vec![("photos".to_string(), "Photos".to_string()), ("memes".to_string(), "Memes".to_string())];
        let b = body("m", "kids at the beach last christmas", "2026-10-13", &cats, true);
        assert!(b["messages"][0]["content"].as_str().unwrap().contains("Today is 2026-10-13"));
        assert_eq!(b["provider"]["zdr"], true);
        let q = parse(
            r#"{"categories":["photos","pets"],"media":["photo"],"from":"2025-12-24","to":"2025-12-27","place":" ","favorite":null,"words":"kids beach"}"#,
            &cats,
        )
        .unwrap();
        assert_eq!(q.categories, ["photos"]);
        assert_eq!((q.from.as_deref(), q.to.as_deref()), (Some("2025-12-24"), Some("2025-12-27")));
        assert_eq!((q.place, q.words.as_str()), (None, "kids beach"));
        assert_eq!(parse(r#"{"from":"last week","words":""}"#, &cats).unwrap().from, None);
        assert!(parse("no", &cats).is_err());
    }
}
