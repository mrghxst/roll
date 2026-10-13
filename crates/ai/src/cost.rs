//! Cost math for decision runs (PROMPT 5.3). Prices are USD per token (and
//! per image or request where a provider charges for those), as OpenRouter
//! lists them.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct Price {
    pub prompt: f64,
    pub completion: f64,
    #[serde(default)]
    pub image: f64,
    #[serde(default)]
    pub request: f64,
}

impl Price {
    pub fn is_free(&self) -> bool {
        self.prompt == 0.0 && self.completion == 0.0 && self.image == 0.0 && self.request == 0.0
    }

    /// From OpenRouter's `pricing` object (strings of USD per unit).
    pub fn from_json(v: &serde_json::Value) -> Option<Self> {
        let num = |k: &str| v.get(k).and_then(|x| x.as_str().and_then(|s| s.parse::<f64>().ok()).or_else(|| x.as_f64()));
        let p = Price {
            prompt: num("prompt")?,
            completion: num("completion")?,
            image: num("image").unwrap_or(0.0),
            request: num("request").unwrap_or(0.0),
        };
        // OpenRouter marks variable-price routers with -1.
        (p.prompt >= 0.0 && p.completion >= 0.0).then_some(p)
    }
}

/// Token counts per item. The model check measures `image_tokens` for a
/// model; the rest follow from the prompt.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Assumptions {
    pub image_tokens: f64,
    pub hint_tokens: f64,
    /// System prompt and category list, shared by a batch.
    pub prompt_tokens: f64,
    pub output_tokens: f64,
    pub batch: usize,
}

impl Default for Assumptions {
    fn default() -> Self {
        Self { image_tokens: 300.0, hint_tokens: 30.0, prompt_tokens: 400.0, output_tokens: 35.0, batch: 8 }
    }
}

impl Assumptions {
    pub fn input_per_item(&self) -> f64 {
        self.image_tokens + self.hint_tokens + self.prompt_tokens / self.batch.max(1) as f64
    }
}

/// Expected cost of one item.
pub fn per_item(p: &Price, a: &Assumptions) -> f64 {
    a.input_per_item() * p.prompt + a.output_tokens * p.completion + p.image + p.request / a.batch.max(1) as f64
}

/// The most one request for `n` items can cost: input with a margin for
/// larger images, and every output token up to `max_tokens`. Reserved
/// before sending so a run never passes its budget.
pub fn worst_case(p: &Price, a: &Assumptions, n: usize, max_tokens: u32) -> f64 {
    let input = a.prompt_tokens + n as f64 * (a.image_tokens * 2.0 + a.hint_tokens * 2.0);
    input * p.prompt + max_tokens as f64 * p.completion + n as f64 * p.image + p.request
}

/// The output cap for a batch: room for every item's JSON line.
pub fn max_tokens(n: usize) -> u32 {
    (60 + n * 70) as u32
}

pub fn per_100k(p: &Price, a: &Assumptions) -> f64 {
    per_item(p, a) * 100_000.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_the_spec_table() {
        // google/gemma-3-12b-it on DeepInfra: $0.05 in, $0.15 out per M.
        let p = Price { prompt: 0.05e-6, completion: 0.15e-6, ..Default::default() };
        let a = Assumptions::default();
        let c = per_100k(&p, &a);
        assert!((2.0..2.6).contains(&c), "{c}");
        assert!(worst_case(&p, &a, 8, max_tokens(8)) > per_item(&p, &a) * 8.0);
        let parsed = Price::from_json(&serde_json::json!({"prompt": "0.00000005", "completion": "0.00000015", "image": "0"})).unwrap();
        assert_eq!(parsed, p);
        assert!(Price::from_json(&serde_json::json!({"prompt": "-1", "completion": "-1"})).is_none());
    }
}
