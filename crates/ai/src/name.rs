//! A name for a Discover group (PROMPT 4.3): one call to the decision model
//! with up to 9 thumbnails (512 px, no metadata). It suggests a short
//! category name and one line about what belongs, or says which existing
//! category fits.

use base64::Engine;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

pub const PROMPT_VERSION: &str = "name-1";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Suggestion {
    pub name: String,
    pub definition: String,
    /// The key of an existing category that already fits.
    pub existing: Option<String>,
}

pub fn body(model: &str, jpegs: &[Vec<u8>], categories: &[(String, String)], openrouter: bool) -> Value {
    let keys: Vec<Value> = categories.iter().map(|(k, _)| json!(k)).chain([Value::Null]).collect();
    let list = categories.iter().map(|(k, n)| format!("{k} ({n})")).collect::<Vec<_>>().join(", ");
    let system = format!(
        "These pictures are a group from a personal photo library that look like they belong together. \
Name the kind of picture they are as a library category: one to three words, sentence case, plural where it reads naturally \
(for example \"Pokémon GO\", \"Receipts\", \"Concert tickets\"), and one short line about what belongs. \
If one of these existing categories clearly fits, give its key in `existing`: {list}. Reply with JSON only."
    );
    let mut content = vec![json!({"type": "text", "text": format!("{} pictures:", jpegs.len())})];
    for j in jpegs {
        let b64 = base64::engine::general_purpose::STANDARD.encode(j);
        content.push(json!({"type": "image_url", "image_url": {"url": format!("data:image/jpeg;base64,{b64}")}}));
    }
    let schema = json!({
        "type": "object",
        "additionalProperties": false,
        "required": ["name", "definition", "existing"],
        "properties": {
            "name": {"type": "string"},
            "definition": {"type": "string"},
            "existing": {"type": ["string", "null"], "enum": keys}
        }
    });
    let mut b = json!({
        "model": model,
        "temperature": 0,
        "max_tokens": if openrouter { 150 } else { 2200 },
        "messages": [{"role": "system", "content": system}, {"role": "user", "content": content}],
        "response_format": {"type": "json_schema", "json_schema": {"name": "group_name", "strict": true, "schema": schema}}
    });
    if openrouter {
        b["provider"] = json!({"zdr": true, "data_collection": "deny", "require_parameters": true, "sort": "price"});
        b["reasoning"] = json!({"effort": "none", "exclude": true});
    }
    b
}

pub fn parse(content: &str, categories: &[(String, String)]) -> Result<Suggestion, String> {
    let text = content.trim();
    let start = text.find('{').ok_or("no JSON object in the reply")?;
    let end = text.rfind('}').ok_or("no JSON object in the reply")?;
    let v: Value = serde_json::from_str(&text[start..=end]).map_err(|e| e.to_string())?;
    let clean = |s: &str, max: usize| s.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(max).collect::<String>();
    let name = clean(v["name"].as_str().unwrap_or(""), 40);
    if name.is_empty() {
        return Err("no name in the reply".into());
    }
    let existing = v["existing"].as_str().filter(|k| categories.iter().any(|(c, _)| c == k)).map(String::from);
    Ok(Suggestion { name, definition: clean(v["definition"].as_str().unwrap_or(""), 160), existing })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn body_and_reply() {
        let cats = vec![("screenshots".to_string(), "Screenshots".to_string())];
        let b = body("m", &[vec![0xff, 0xd8], vec![0xff, 0xd8]], &cats, true);
        assert_eq!(b["messages"][1]["content"].as_array().unwrap().len(), 3);
        assert_eq!(b["provider"]["zdr"], true);
        let s = parse("```json\n{\"name\":\"  Pokémon   GO \",\"definition\":\"Screenshots of the game.\",\"existing\":null}\n```", &cats)
            .unwrap();
        assert_eq!(s, Suggestion { name: "Pokémon GO".into(), definition: "Screenshots of the game.".into(), existing: None });
        let s = parse("{\"name\":\"Screens\",\"definition\":\"\",\"existing\":\"screenshots\"}", &cats).unwrap();
        assert_eq!(s.existing.as_deref(), Some("screenshots"));
        assert!(parse("{\"name\":\"\",\"definition\":\"\",\"existing\":null}", &cats).is_err());
    }
}
