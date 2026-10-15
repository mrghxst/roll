//! "Better ranking" (PROMPT 4.2): the decision model rates how well each of
//! the top search matches fits the words, from 0 to 10. One call per few
//! thumbnails (320 px, no metadata); only the words and the pictures are
//! sent.

use base64::Engine;
use serde_json::{Value, json};

pub const PROMPT_VERSION: &str = "rerank-1";

pub fn body(model: &str, words: &str, jpegs: &[Vec<u8>], openrouter: bool) -> Value {
    let system = "You rank results of a search in a personal photo library. \
For each picture, in order, rate how well it matches the search from 0 (not at all) to 10 (exactly what was asked for). \
Judge only what is visible. Reply with JSON only.";
    let mut content = vec![json!({"type": "text", "text": format!("Search: {words}\n{} pictures:", jpegs.len())})];
    for j in jpegs {
        let b64 = base64::engine::general_purpose::STANDARD.encode(j);
        content.push(json!({"type": "image_url", "image_url": {"url": format!("data:image/jpeg;base64,{b64}")}}));
    }
    let schema = json!({
        "type": "object",
        "additionalProperties": false,
        "required": ["scores"],
        "properties": {
            "scores": {"type": "array", "items": {"type": "integer", "minimum": 0, "maximum": 10}, "minItems": jpegs.len(), "maxItems": jpegs.len()}
        }
    });
    let mut b = json!({
        "model": model,
        "temperature": 0,
        "max_tokens": if openrouter { 100 } else { 2200 },
        "messages": [{"role": "system", "content": system}, {"role": "user", "content": content}],
        "response_format": {"type": "json_schema", "json_schema": {"name": "rerank", "strict": true, "schema": schema}}
    });
    if openrouter {
        b["provider"] = json!({"zdr": true, "data_collection": "deny", "require_parameters": true, "sort": "price"});
        b["reasoning"] = json!({"effort": "none", "exclude": true});
    }
    b
}

/// One score per picture, 0–10.
pub fn parse(content: &str, n: usize) -> Result<Vec<u8>, String> {
    let text = content.trim();
    let start = text.find('{').ok_or("no JSON object in the reply")?;
    let end = text.rfind('}').ok_or("no JSON object in the reply")?;
    let v: Value = serde_json::from_str(&text[start..=end]).map_err(|e| e.to_string())?;
    let scores: Vec<u8> = v["scores"]
        .as_array()
        .ok_or("no scores in the reply")?
        .iter()
        .map(|s| s.as_f64().map(|x| x.clamp(0.0, 10.0).round() as u8).ok_or("a score is not a number"))
        .collect::<Result<_, _>>()?;
    if scores.len() != n {
        return Err(format!("{} scores for {n} pictures", scores.len()));
    }
    Ok(scores)
}

/// The new order of `ids` by score, the old order breaking ties.
pub fn order(ids: &[i64], scores: &[Option<u8>]) -> Vec<(i64, Option<u8>)> {
    let mut out: Vec<(usize, i64, Option<u8>)> = ids.iter().zip(scores).enumerate().map(|(i, (id, s))| (i, *id, *s)).collect();
    // Unscored pictures (a failed batch) keep their place among the middling ones.
    out.sort_by_key(|(i, _, s)| (std::cmp::Reverse(s.unwrap_or(5)), *i));
    out.into_iter().map(|(_, id, s)| (id, s)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn body_parse_and_order() {
        let b = body("m", "red bike", &[vec![0xff, 0xd8], vec![0xff, 0xd8]], true);
        assert_eq!(b["messages"][1]["content"].as_array().unwrap().len(), 3);
        assert!(b["messages"][1]["content"][0]["text"].as_str().unwrap().contains("red bike"));
        assert_eq!(b["provider"]["zdr"], true);
        assert_eq!(parse("```json\n{\"scores\": [3, 11]}\n```", 2).unwrap(), vec![3, 10]);
        assert!(parse("{\"scores\": [3]}", 2).is_err());
        let o = order(&[1, 2, 3, 4], &[Some(2), Some(9), None, Some(9)]);
        assert_eq!(o.iter().map(|x| x.0).collect::<Vec<_>>(), vec![2, 4, 3, 1]);
    }
}
