//! What Roll knows about vision models before measuring them: a quality
//! tier from published benchmarks, and for local models the download size
//! and the graphics memory they need.
//!
//! Tiers are coarse on purpose. Vendors report different benchmarks
//! (MMMU, MMMU-Pro, with or without thinking), so scores from different
//! cards do not compare one to one. The model check measures quality on
//! the owner's own items and always wins over a tier.

use serde::{Deserialize, Serialize};

/// 1 is the best.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Tier(pub u8);

impl Tier {
    pub fn label(self) -> &'static str {
        match self.0 {
            1 => "Best",
            2 => "Very good",
            3 => "Good",
            _ => "Basic",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Known {
    pub tier: Tier,
    /// Where the tier comes from, for the UI.
    pub basis: &'static str,
}

/// OpenRouter model ids (prefix match, most specific first).
const CLOUD: &[(&str, u8, &str)] = &[
    ("google/gemini-3", 1, "Gemini 3 family; frontier multimodal scores in Google's model card"),
    ("google/gemini-2.5-pro", 1, "Gemini 2.5 Pro; frontier multimodal scores in Google's model card"),
    ("google/gemini-2.5-flash-lite", 2, "Gemini 2.5 Flash-Lite; Google's smallest 2.5 model"),
    ("google/gemini-2.5-flash", 1, "Gemini 2.5 Flash; strong multimodal scores in Google's model card"),
    ("openai/gpt-5-nano", 2, "GPT-5 nano; OpenAI's smallest GPT-5"),
    ("openai/gpt-5", 1, "GPT-5 family; frontier multimodal scores"),
    ("anthropic/claude", 1, "Claude; frontier multimodal scores"),
    ("qwen/qwen3-vl-235b", 1, "Qwen3-VL 235B; the largest Qwen3-VL"),
    ("qwen/qwen3-vl-32b", 1, "Qwen3-VL 32B"),
    ("qwen/qwen3-vl-30b", 1, "Qwen3-VL 30B-A3B"),
    ("qwen/qwen3-vl-8b", 2, "Qwen3-VL 8B; MMMU 69.6 as reported from the model card"),
    ("qwen/qwen3-vl-4b", 3, "Qwen3-VL 4B; MMMU 67.4 as reported from the model card"),
    ("google/gemma-4-31b", 1, "Gemma 4 31B; MMMU-Pro 76.9 (Gemma 4 model card)"),
    ("google/gemma-4-26b", 1, "Gemma 4 26B A4B; MMMU-Pro 73.8 (Gemma 4 model card)"),
    ("google/gemma-3-27b", 2, "Gemma 3 27B; MMMU 64.9 (Gemma 3 model card)"),
    ("google/gemma-3-12b", 3, "Gemma 3 12B; MMMU 59.6 (Gemma 3 model card)"),
    ("google/gemma-3-4b", 4, "Gemma 3 4B; MMMU 48.8 (Gemma 3 model card)"),
    ("meta-llama/llama-4-maverick", 2, "Llama 4 Maverick"),
    ("meta-llama/llama-4-scout", 3, "Llama 4 Scout"),
    ("mistralai/mistral-small-3", 3, "Mistral Small 3.x"),
];

/// A local model: Ollama tag, quality, download size and memory to run it
/// fully on the graphics card (Ollama's default 4-bit builds).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LocalModel {
    pub tag: &'static str,
    pub tier: Tier,
    pub basis: &'static str,
    pub download_gb: f64,
    pub memory_gb: f64,
}

/// Sizes from ollama.com/library (2026-10-13). Qwen3-VL's plain tags
/// (`qwen3-vl:8b`) are its thinking variants, which reason before every
/// answer; the `-instruct` tags answer directly and are the ones to use.
pub const LOCAL: &[LocalModel] = &[
    LocalModel { tag: "gemma4:31b", tier: Tier(1), basis: "MMMU-Pro 76.9 (Gemma 4 model card)", download_gb: 19.0, memory_gb: 22.0 },
    LocalModel { tag: "gemma4:26b", tier: Tier(1), basis: "MMMU-Pro 73.8 (Gemma 4 model card)", download_gb: 16.0, memory_gb: 19.0 },
    LocalModel { tag: "qwen3-vl:32b-instruct", tier: Tier(1), basis: "Qwen3-VL 32B", download_gb: 21.0, memory_gb: 24.0 },
    LocalModel { tag: "qwen3-vl:30b-a3b-instruct", tier: Tier(1), basis: "Qwen3-VL 30B-A3B", download_gb: 20.0, memory_gb: 23.0 },
    LocalModel {
        tag: "qwen3-vl:8b-instruct",
        tier: Tier(2),
        basis: "MMMU 69.6 as reported from the model card",
        download_gb: 6.1,
        memory_gb: 8.0,
    },
    LocalModel { tag: "gemma3:12b", tier: Tier(2), basis: "MMMU 59.6 (Gemma 3 model card)", download_gb: 8.2, memory_gb: 10.0 },
    LocalModel { tag: "gemma4:e4b", tier: Tier(2), basis: "MMMU-Pro 52.6 (Gemma 4 model card)", download_gb: 6.6, memory_gb: 8.0 },
    LocalModel {
        tag: "qwen3-vl:4b-instruct",
        tier: Tier(3),
        basis: "MMMU 67.4 as reported from the model card",
        download_gb: 3.3,
        memory_gb: 5.0,
    },
    LocalModel { tag: "gemma3:4b", tier: Tier(4), basis: "MMMU 48.8 (Gemma 3 model card)", download_gb: 3.4, memory_gb: 5.0 },
    LocalModel { tag: "qwen3-vl:2b-instruct", tier: Tier(4), basis: "Qwen3-VL 2B, the smallest", download_gb: 1.9, memory_gb: 3.0 },
];

/// The tier of an OpenRouter model id, if Roll knows it.
pub fn cloud(id: &str) -> Option<Known> {
    let id = id.to_lowercase();
    CLOUD.iter().find(|(p, _, _)| id.starts_with(p)).map(|(_, t, b)| Known { tier: Tier(*t), basis: b })
}

/// What Roll knows about a local model name ("qwen3-vl:8b-instruct",
/// "qwen3-vl:8b-instruct-q8_0", "Qwen3-VL-8B-Instruct-GGUF"). The second
/// value is true for a thinking variant of a known model.
pub fn local(name: &str) -> Option<(&'static LocalModel, bool)> {
    let n = name.to_lowercase().replace(['-', '_'], "");
    let thinking = n.contains("thinking")
        || n.ends_with("qwen3vl:2b")
        || n.ends_with("qwen3vl:4b")
        || n.ends_with("qwen3vl:8b")
        || n.ends_with("qwen3vl:latest");
    let n = n.replace("instruct", "").replace("thinking", "").replace("latest", "8b").replace("a3b", "");
    LOCAL.iter().find_map(|m| {
        let tag = m.tag.replace("-instruct", "").replace("-a3b", "");
        let (family, size) = tag.split_once(':').unwrap();
        let family = family.replace('-', "");
        // "qwen3vl:8b…" or "qwen3vl8b…"
        (n.starts_with(&format!("{family}:{size}")) || n.contains(&format!("{family}{size}"))).then_some((m, thinking))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_models() {
        assert_eq!(cloud("google/gemini-2.5-flash-lite").unwrap().tier, Tier(2));
        assert_eq!(cloud("google/gemini-2.5-flash").unwrap().tier, Tier(1));
        assert_eq!(cloud("google/gemma-3-12b-it").unwrap().tier.label(), "Good");
        assert!(cloud("inclusionai/ling-3.0-flash-vl").is_none());
        assert_eq!(local("qwen3-vl:8b-instruct").unwrap(), (&LOCAL[4], false));
        assert_eq!(local("qwen3-vl:8b-instruct-q8_0").unwrap().0.tag, "qwen3-vl:8b-instruct");
        assert_eq!(local("Qwen3-VL-4B-Instruct-GGUF").unwrap(), (&LOCAL[7], false));
        // The plain tag is the thinking variant.
        assert_eq!(local("qwen3-vl:8b").unwrap(), (&LOCAL[4], true));
        assert!(local("qwen3-vl:32b-thinking-q4_K_M").unwrap().1);
        assert_eq!(local("qwen3-vl:30b-a3b-instruct").unwrap().0.tag, "qwen3-vl:30b-a3b-instruct");
        assert_eq!(local("gemma4:e4b").unwrap().0.tier, Tier(2));
        assert!(local("llava:7b").is_none());
        // Best first.
        assert!(LOCAL.windows(2).all(|w| w[0].tier <= w[1].tier));
    }
}
