//! OpenRouter's catalogue: image-capable models with live prices, and which
//! of them have zero-data-retention (ZDR) endpoints (PROMPT 5.2).

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::chat::{ChatError, Endpoint};
use crate::cost::{self, Assumptions, Price};

/// One model as the picker shows it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Offer {
    pub id: String,
    pub name: String,
    /// The cheapest ZDR provider, when there is one.
    pub provider: Option<String>,
    pub price: Price,
    pub zdr: bool,
    /// Takes a JSON schema as `response_format`.
    pub structured: bool,
    pub context_length: Option<u64>,
    /// Estimated USD per 100,000 items with the default assumptions.
    pub per_100k: f64,
}

/// Image-capable models, cheapest first. `models` is the body of
/// `GET /models`, `zdr` the body of `GET /endpoints/zdr`.
pub fn offers(models: &Value, zdr: &Value, a: &Assumptions) -> Vec<Offer> {
    // Cheapest ZDR endpoint per model id (or per display name, for lists
    // that only carry names).
    let mut best: HashMap<String, (Option<Price>, String)> = HashMap::new();
    for e in zdr["data"].as_array().into_iter().flatten() {
        let key = e["model_id"].as_str().or(e["model_name"].as_str()).unwrap_or("").to_string();
        if key.is_empty() {
            continue;
        }
        let provider = e["provider_name"].as_str().unwrap_or("").to_string();
        let price = Price::from_json(&e["pricing"]);
        let entry = best.entry(key).or_insert((None, provider.clone()));
        if let Some(p) = price
            && entry.0.is_none_or(|q| cost::per_item(&p, a) < cost::per_item(&q, a))
        {
            *entry = (Some(p), provider);
        }
    }
    let mut out: Vec<Offer> = models["data"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|m| m["architecture"]["input_modalities"].as_array().is_some_and(|a| a.iter().any(|x| x == "image")))
        .filter_map(|m| {
            let id = m["id"].as_str()?.to_string();
            let name = m["name"].as_str().unwrap_or(&id).to_string();
            let listed = Price::from_json(&m["pricing"])?;
            let z = best.get(&id).or_else(|| best.get(&name));
            let price = z.and_then(|(p, _)| *p).unwrap_or(listed);
            let params = m["supported_parameters"].as_array();
            let structured = params.is_some_and(|p| p.iter().any(|x| x == "response_format" || x == "structured_outputs"));
            Some(Offer {
                per_100k: cost::per_100k(&price, a),
                id,
                name,
                provider: z.map(|(_, p)| p.clone()).filter(|p| !p.is_empty()),
                price,
                zdr: z.is_some(),
                structured,
                context_length: m["context_length"].as_u64(),
            })
        })
        .collect();
    out.sort_by(|a, b| a.per_100k.total_cmp(&b.per_100k).then(a.id.cmp(&b.id)));
    out
}

/// Fetch both lists and build the offers. Needs no key.
pub fn fetch(ep: &Endpoint, a: &Assumptions) -> Result<Vec<Offer>, ChatError> {
    let models = ep.get("/models")?;
    // The ZDR list is a newer endpoint; without it nothing counts as ZDR.
    let zdr = ep.get("/endpoints/zdr").unwrap_or(Value::Null);
    Ok(offers(&models, &zdr, a))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn image_models_cheapest_first_with_zdr() {
        let models = json!({"data": [
            {"id": "a/text", "name": "Text", "pricing": {"prompt": "0", "completion": "0"}, "architecture": {"input_modalities": ["text"]}},
            {"id": "g/gemma", "name": "Gemma", "pricing": {"prompt": "0.0000001", "completion": "0.0000002"},
             "architecture": {"input_modalities": ["text", "image"]}, "supported_parameters": ["response_format", "temperature"], "context_length": 131072},
            {"id": "l/ling", "name": "Ling", "pricing": {"prompt": "0.00000002", "completion": "0.00000006"},
             "architecture": {"input_modalities": ["image", "text"]}, "supported_parameters": ["max_tokens"]},
        ]});
        let zdr = json!({"data": [
            {"model_id": "g/gemma", "provider_name": "DeepInfra", "pricing": {"prompt": "0.00000005", "completion": "0.00000015"}},
            {"model_id": "g/gemma", "provider_name": "Pricey", "pricing": {"prompt": "0.000001", "completion": "0.000001"}},
        ]});
        let o = offers(&models, &zdr, &Assumptions::default());
        assert_eq!(o.iter().map(|o| o.id.as_str()).collect::<Vec<_>>(), ["l/ling", "g/gemma"]);
        let g = &o[1];
        assert!(g.zdr && g.structured);
        assert_eq!(g.provider.as_deref(), Some("DeepInfra"));
        assert_eq!(g.price.prompt, 0.00000005);
        assert!(!o[0].zdr && !o[0].structured);
    }
}
