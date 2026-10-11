//! Stored image embeddings and hybrid search. Text search ranks with FTS5
//! (BM25) over names, folders, tags, places, descriptions and OCR text;
//! when a text embedding of the query is at hand, image similarity ranks
//! too, and the two lists are fused with Reciprocal Rank Fusion. Filters
//! from the rest of the query apply to both.

use std::collections::HashSet;

use roll_ai::vectors::{self, Index};
use rusqlite::{Connection, params, params_from_iter};

use crate::db::now_ms;
use crate::model::Query;
use crate::query;

/// How many results each list contributes before fusion.
const DEPTH: usize = 500;

pub fn store(c: &Connection, asset: i64, model: &str, v: &[f32]) -> rusqlite::Result<()> {
    c.execute(
        "INSERT OR REPLACE INTO embeddings (asset_id, model, dim, vec, at) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![asset, model, v.len() as i64, vectors::to_blob(v), now_ms()],
    )?;
    Ok(())
}

/// Every embedding of `model` (of items still in the library).
pub fn load(c: &Connection, model: &str) -> rusqlite::Result<Index> {
    let mut st = c.prepare(
        "SELECT e.asset_id, e.dim, e.vec FROM embeddings e JOIN assets a ON a.id = e.asset_id
         WHERE e.model = ?1 AND a.missing = 0 AND a.trashed_at IS NULL",
    )?;
    let mut ix: Option<Index> = None;
    let rows = st.query_map([model], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?, r.get::<_, Vec<u8>>(2)?)))?;
    for row in rows {
        let (id, dim, blob) = row?;
        let ix = ix.get_or_insert_with(|| Index::new(dim as usize));
        ix.insert(id, &vectors::from_blob(&blob));
    }
    Ok(ix.unwrap_or_default())
}

/// Photos and videos that still need an embedding from `model`, newest first.
pub fn missing(c: &Connection, model: &str, limit: usize) -> rusqlite::Result<Vec<i64>> {
    let mut st = c.prepare(
        "SELECT a.id FROM assets a
         WHERE a.missing = 0 AND a.trashed_at IS NULL AND (a.group_role IS NULL OR a.group_role = 'primary')
           AND NOT EXISTS (SELECT 1 FROM embeddings e WHERE e.asset_id = a.id AND e.model = ?1)
         ORDER BY a.taken_local DESC, a.id DESC LIMIT ?2",
    )?;
    st.query_map(params![model, limit as i64], |r| r.get(0))?.collect()
}

/// Items matching `text` in the full-text index, best first.
pub fn text_ranked(c: &Connection, text: &str, limit: usize) -> rusqlite::Result<Vec<i64>> {
    let Some(m) = crate::search::fts_match(text) else { return Ok(Vec::new()) };
    let mut st = c.prepare("SELECT rowid FROM fts WHERE fts MATCH ?1 ORDER BY bm25(fts, 4.0, 2.0, 3.0, 2.0, 1.0) LIMIT ?2")?;
    st.query_map(params![m, limit as i64], |r| r.get(0))?.collect()
}

/// Ranked results for `q`: its filters apply, its text ranks. `text_vec`
/// is the query's text embedding, in the space of `index`.
pub fn hybrid(c: &Connection, q: &Query, text_vec: Option<&[f32]>, index: Option<&Index>, limit: usize) -> rusqlite::Result<Vec<i64>> {
    let text = q.text.as_deref().unwrap_or("").trim();
    let filters = Query { text: None, ..q.clone() };
    let allowed: HashSet<i64> = {
        let sql = query::where_clause(&filters);
        let mut st = c.prepare(&format!("SELECT a.id FROM assets a WHERE {}", sql.clause))?;
        st.query_map(params_from_iter(sql.params.iter()), |r| r.get::<_, i64>(0))?.collect::<Result<_, _>>()?
    };
    let by_text: Vec<i64> = text_ranked(c, text, DEPTH * 4)?.into_iter().filter(|id| allowed.contains(id)).take(DEPTH).collect();
    let by_image: Vec<i64> = match (text_vec, index) {
        (Some(v), Some(ix)) if !text.is_empty() => ix.search(v, DEPTH, Some(&allowed)).into_iter().map(|(id, _)| id).collect(),
        _ => Vec::new(),
    };
    let fused = vectors::rrf(&[&by_text, &by_image], 60.0);
    Ok(fused.into_iter().take(limit).map(|(id, _)| id).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Db;
    use crate::testutil::insert_asset;

    #[test]
    fn stores_loads_and_fuses() {
        let db = Db::memory().unwrap();
        let c = db.write();
        let beach = insert_asset(&c, "2019/beach.jpg");
        let receipt = insert_asset(&c, "2019/IMG_2.jpg");
        let dog = insert_asset(&c, "2020/IMG_3.jpg");
        let gone = insert_asset(&c, "2020/IMG_4.jpg");
        c.execute("UPDATE assets SET kind = 1 WHERE id = ?1", [dog]).unwrap(); // a video
        c.execute("UPDATE assets SET trashed_at = 1 WHERE id = ?1", [gone]).unwrap();
        c.execute("INSERT INTO ocr (asset_id, model, text, at) VALUES (?1, 'ocr', 'Migros beach towel 12.90', 0)", [receipt]).unwrap();
        crate::search::reindex(&c, None, &|_, _| None).unwrap();

        // Pretend embeddings: dimension 0 is "beach", 1 is "dog".
        store(&c, beach, "m", &[1.0, 0.0, 0.1]).unwrap();
        store(&c, receipt, "m", &[0.2, 0.0, 1.0]).unwrap();
        store(&c, dog, "m", &[0.0, 1.0, 0.0]).unwrap();
        store(&c, gone, "m", &[1.0, 0.0, 0.0]).unwrap();
        let ix = load(&c, "m").unwrap();
        assert_eq!((ix.len(), ix.dim()), (3, 3), "trashed items are left out");
        assert!(load(&c, "other").unwrap().is_empty());
        assert_eq!(missing(&c, "m", 10).unwrap(), Vec::<i64>::new());
        assert_eq!(missing(&c, "other", 10).unwrap().len(), 3);

        let q = |text: &str| Query { text: Some(text.into()), ..Default::default() };
        // Text only: the receipt's OCR text and the file name both match.
        let t = hybrid(&c, &q("beach"), None, None, 10).unwrap();
        assert_eq!(t.len(), 2);
        assert!(t.contains(&beach) && t.contains(&receipt));
        // With the image list, the photo that looks like a beach and is
        // named so wins over the receipt.
        let h = hybrid(&c, &q("beach"), Some(&[1.0, 0.0, 0.0]), Some(&ix), 10).unwrap();
        assert_eq!(h[0], beach);
        // Image similarity finds what no text mentions.
        let d = hybrid(&c, &q("dog"), Some(&[0.0, 1.0, 0.0]), Some(&ix), 1).unwrap();
        assert_eq!(d, vec![dog]);
        // Filters apply to both lists.
        let photos = Query { media: vec!["photo".into()], ..q("dog") };
        let p = hybrid(&c, &photos, Some(&[0.0, 1.0, 0.0]), Some(&ix), 10).unwrap();
        assert!(!p.contains(&dog));
    }
}
