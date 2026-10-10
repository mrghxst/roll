//! Hierarchical tags (`Event/2019-08 Lake Garda`) in the three forms photo
//! apps read: digiKam `TagsList` (`A/B`), Lightroom `HierarchicalSubject`
//! (`A|B`) and flat `Subject`/`Keywords` (leaf names).

/// Normalize one tag path: trim each level, drop empty levels.
pub fn normalize(tag: &str) -> Option<String> {
    let parts: Vec<&str> = tag.split(['/', '|']).map(str::trim).filter(|p| !p.is_empty()).collect();
    (!parts.is_empty()).then(|| parts.join("/"))
}

pub fn leaf(tag: &str) -> &str {
    tag.rsplit('/').next().unwrap_or(tag)
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TagForms {
    pub tags_list: Vec<String>,
    pub hierarchical: Vec<String>,
    pub subjects: Vec<String>,
}

pub fn serialize<S: AsRef<str>>(tags: &[S]) -> TagForms {
    let mut f = TagForms::default();
    for t in tags {
        let Some(t) = normalize(t.as_ref()) else { continue };
        if f.tags_list.contains(&t) {
            continue;
        }
        let leaf = leaf(&t).to_string();
        f.hierarchical.push(t.replace('/', "|"));
        f.tags_list.push(t);
        if !f.subjects.contains(&leaf) {
            f.subjects.push(leaf);
        }
    }
    f
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn three_forms() {
        let f = serialize(&["Category/Memes", " Event / 2019-08 Lake Garda ", "Favorite", "Category/Memes", "Source/Memes"]);
        assert_eq!(f.tags_list, vec!["Category/Memes", "Event/2019-08 Lake Garda", "Favorite", "Source/Memes"]);
        assert_eq!(f.hierarchical, vec!["Category|Memes", "Event|2019-08 Lake Garda", "Favorite", "Source|Memes"]);
        assert_eq!(f.subjects, vec!["Memes", "2019-08 Lake Garda", "Favorite"]);
    }

    #[test]
    fn normalizes() {
        assert_eq!(normalize("a||b/ /c").as_deref(), Some("a/b/c"));
        assert_eq!(normalize(" / "), None);
    }
}
