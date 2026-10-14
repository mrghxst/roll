//! Database migrations. Append only: never edit a shipped migration.

pub const MIGRATIONS: &[&str] = &[M1, M2, M3, M4, M5, M6, M7, M8, M9, M10, M11, M12];

const M1: &str = r#"
CREATE TABLE settings (
  key TEXT PRIMARY KEY,
  value TEXT NOT NULL
);

CREATE TABLE roots (
  id INTEGER PRIMARY KEY,
  path TEXT NOT NULL UNIQUE,
  -- 'library' (the archive) or 'staging' (an import being checked)
  kind TEXT NOT NULL DEFAULT 'library',
  added_at INTEGER NOT NULL
);

CREATE TABLE categories (
  id INTEGER PRIMARY KEY,
  key TEXT NOT NULL UNIQUE,
  name TEXT NOT NULL,
  parent_id INTEGER REFERENCES categories(id) ON DELETE CASCADE,
  definition TEXT NOT NULL DEFAULT '',
  folder_template TEXT,
  -- Subcategories are tags only unless they get their own folder.
  own_folder INTEGER NOT NULL DEFAULT 1,
  immich_default INTEGER NOT NULL DEFAULT 0,
  auto_threshold REAL,
  sort INTEGER NOT NULL DEFAULT 0,
  builtin INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE groups (
  id INTEGER PRIMARY KEY,
  kind TEXT NOT NULL,
  key TEXT NOT NULL,
  UNIQUE(kind, key)
);

CREATE TABLE assets (
  id INTEGER PRIMARY KEY,
  root_id INTEGER NOT NULL REFERENCES roots(id) ON DELETE CASCADE,
  rel_path TEXT NOT NULL,
  dir TEXT NOT NULL,
  name TEXT NOT NULL,
  ext TEXT NOT NULL,
  size INTEGER NOT NULL,
  mtime INTEGER NOT NULL,
  quick_hash TEXT NOT NULL,
  blake3 TEXT,
  kind INTEGER NOT NULL,
  format TEXT NOT NULL,
  width INTEGER,
  height INTEGER,
  duration_ms INTEGER,
  fps REAL,
  has_audio INTEGER,
  codec TEXT,
  orientation INTEGER,
  rotation INTEGER,
  taken_utc INTEGER,
  taken_local TEXT,
  tz_offset INTEGER,
  date_source TEXT,
  date_conf REAL,
  lat REAL,
  lon REAL,
  alt REAL,
  loc_acc REAL,
  loc_source TEXT,
  make TEXT,
  model TEXT,
  serial TEXT,
  lens TEXT,
  software TEXT,
  screenshot INTEGER NOT NULL DEFAULT 0,
  content_id TEXT,
  burst_id TEXT,
  icc TEXT,
  qtable TEXT,
  signature TEXT,
  has_xmp INTEGER NOT NULL DEFAULT 0,
  has_makernotes INTEGER NOT NULL DEFAULT 0,
  group_id INTEGER REFERENCES groups(id) ON DELETE SET NULL,
  group_role TEXT,
  category_id INTEGER REFERENCES categories(id) ON DELETE SET NULL,
  category_source TEXT,
  category_conf REAL,
  -- 0 undecided, 1 proposed, 2 auto-accepted, 3 reviewed by the owner
  review INTEGER NOT NULL DEFAULT 0,
  rating INTEGER NOT NULL DEFAULT 0,
  favorite INTEGER NOT NULL DEFAULT 0,
  description TEXT,
  private INTEGER NOT NULL DEFAULT 0,
  -- 'default', 'send' or 'local'
  immich TEXT NOT NULL DEFAULT 'default',
  immich_id TEXT,
  immich_synced TEXT,
  -- Edits in the database not yet written into files and sidecars.
  meta_dirty INTEGER NOT NULL DEFAULT 0,
  missing INTEGER NOT NULL DEFAULT 0,
  trashed_at INTEGER,
  -- 0 pending, 1 done, 2 failed
  thumb INTEGER NOT NULL DEFAULT 0,
  thumb_v INTEGER NOT NULL DEFAULT 0,
  avg_color INTEGER,
  thumbhash BLOB,
  stats TEXT,
  phash INTEGER,
  problems INTEGER NOT NULL DEFAULT 0,
  -- 0 seen, 1 new, 2 not checked yet
  inbox INTEGER NOT NULL DEFAULT 0,
  added_at INTEGER NOT NULL,
  UNIQUE(root_id, rel_path)
);
CREATE INDEX assets_taken ON assets(taken_utc);
CREATE INDEX assets_local ON assets(taken_local);
CREATE INDEX assets_dir ON assets(root_id, dir);
CREATE INDEX assets_quick ON assets(size, quick_hash);
CREATE INDEX assets_blake3 ON assets(blake3);
CREATE INDEX assets_category ON assets(category_id);
CREATE INDEX assets_group ON assets(group_id);
CREATE INDEX assets_thumb ON assets(thumb) WHERE thumb = 0;

-- The values a file had when Roll first read it.
CREATE TABLE asset_orig (
  asset_id INTEGER PRIMARY KEY REFERENCES assets(id) ON DELETE CASCADE,
  json TEXT NOT NULL
);

CREATE TABLE sidecars (
  id INTEGER PRIMARY KEY,
  root_id INTEGER NOT NULL REFERENCES roots(id) ON DELETE CASCADE,
  rel_path TEXT NOT NULL,
  asset_id INTEGER REFERENCES assets(id) ON DELETE SET NULL,
  -- 'xmp', 'aae' or 'takeout'
  kind TEXT NOT NULL,
  size INTEGER NOT NULL,
  mtime INTEGER NOT NULL,
  UNIQUE(root_id, rel_path)
);
CREATE INDEX sidecars_asset ON sidecars(asset_id);

CREATE TABLE tags (
  id INTEGER PRIMARY KEY,
  path TEXT NOT NULL UNIQUE COLLATE NOCASE
);

CREATE TABLE asset_tags (
  asset_id INTEGER NOT NULL REFERENCES assets(id) ON DELETE CASCADE,
  tag_id INTEGER NOT NULL REFERENCES tags(id) ON DELETE CASCADE,
  source TEXT NOT NULL DEFAULT 'manual',
  conf REAL,
  PRIMARY KEY(asset_id, tag_id)
);
CREATE INDEX asset_tags_tag ON asset_tags(tag_id);

CREATE TABLE events (
  id INTEGER PRIMARY KEY,
  name TEXT NOT NULL,
  start_utc INTEGER,
  end_utc INTEGER,
  lat REAL,
  lon REAL,
  place TEXT,
  -- 'proposed', 'confirmed' or 'dismissed'
  state TEXT NOT NULL DEFAULT 'proposed',
  own_folder INTEGER NOT NULL DEFAULT 1,
  immich TEXT NOT NULL DEFAULT 'default',
  immich_album_id TEXT,
  created_at INTEGER NOT NULL
);

CREATE TABLE event_assets (
  event_id INTEGER NOT NULL REFERENCES events(id) ON DELETE CASCADE,
  asset_id INTEGER NOT NULL REFERENCES assets(id) ON DELETE CASCADE,
  PRIMARY KEY(event_id, asset_id)
);
CREATE INDEX event_assets_asset ON event_assets(asset_id);

-- Provenance and undo for every editable field.
CREATE TABLE field_history (
  id INTEGER PRIMARY KEY,
  asset_id INTEGER NOT NULL REFERENCES assets(id) ON DELETE CASCADE,
  field TEXT NOT NULL,
  old TEXT,
  new TEXT,
  source TEXT NOT NULL,
  source_ref TEXT,
  conf REAL,
  why TEXT,
  batch_id INTEGER,
  at INTEGER NOT NULL
);
CREATE INDEX field_history_asset ON field_history(asset_id, field);
CREATE INDEX field_history_batch ON field_history(batch_id);

-- The journal: a batch of file operations and their states.
CREATE TABLE batches (
  id INTEGER PRIMARY KEY,
  kind TEXT NOT NULL,
  label TEXT NOT NULL,
  -- 'planned', 'applying', 'applied', 'undoing', 'undone', 'failed'
  state TEXT NOT NULL,
  created_at INTEGER NOT NULL,
  applied_at INTEGER,
  undone_at INTEGER,
  summary TEXT
);

CREATE TABLE ops (
  id INTEGER PRIMARY KEY,
  batch_id INTEGER NOT NULL REFERENCES batches(id) ON DELETE CASCADE,
  seq INTEGER NOT NULL,
  -- 'mkdir', 'move', 'copy', 'write_meta', 'trash', 'rmdir'
  kind TEXT NOT NULL,
  asset_id INTEGER,
  src TEXT,
  dst TEXT,
  payload TEXT,
  -- 'pending', 'performing', 'done', 'undoing', 'undone', 'failed', 'skipped'
  state TEXT NOT NULL DEFAULT 'pending',
  error TEXT
);
CREATE INDEX ops_batch ON ops(batch_id, seq);

CREATE TABLE jobs (
  id INTEGER PRIMARY KEY,
  kind TEXT NOT NULL,
  label TEXT NOT NULL,
  -- 'queued', 'running', 'paused', 'done', 'failed', 'cancelled'
  state TEXT NOT NULL,
  priority INTEGER NOT NULL,
  done INTEGER NOT NULL DEFAULT 0,
  total INTEGER NOT NULL DEFAULT 0,
  cost_usd REAL NOT NULL DEFAULT 0,
  params TEXT,
  error TEXT,
  created_at INTEGER NOT NULL,
  started_at INTEGER,
  finished_at INTEGER
);

CREATE TABLE rules (
  id INTEGER PRIMARY KEY,
  key TEXT UNIQUE,
  name TEXT NOT NULL,
  enabled INTEGER NOT NULL DEFAULT 1,
  priority INTEGER NOT NULL DEFAULT 100,
  -- JSON: { "when": [conditions], "then": actions }
  def TEXT NOT NULL,
  builtin INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE saved_filters (
  id INTEGER PRIMARY KEY,
  name TEXT NOT NULL,
  query TEXT NOT NULL,
  sort INTEGER NOT NULL DEFAULT 0
);

INSERT INTO categories (key, name, definition, folder_template, immich_default, sort, builtin) VALUES
 ('photos', 'Photos', 'Real-life photos and videos taken with a camera: people, places, events, things, including ones received from friends. No added captions, app frames or screen interface.', '{year}/Photos', 1, 1, 1),
 ('screenshots', 'Screenshots', 'Captures of a whole phone or computer screen: apps, chats, settings, mail, web pages, maps, games, usually with a status bar or app interface. A chat or feed captured from the screen is a screenshot.', '{year}/Screenshots', 0, 2, 1),
 ('memes', 'Memes', 'Made to be funny or shared: an image with a caption added over or above it (even a real photo), reaction images, comics, joke formats, stickers, GIFs.', '{year}/Memes', 0, 3, 1),
 ('social_media', 'Social media', 'A single post, story or picture saved from a social app or website, often with the app''s frame (user name, likes, caption below): other people''s photos, outfits, recipes, products, news, quotes. Not memes, not whole-screen captures of chats or settings.', '{year}/Social media', 0, 4, 1),
 ('wallpapers', 'Wallpapers', 'Backgrounds, digital art, AI-generated images and design assets with no personal content.', 'Wallpapers', 0, 5, 1),
 ('documents', 'Documents', 'Photos or scans of paper, receipts, tickets, IDs, whiteboards, notes and labels, or a screen photographed for its text.', '{year}/Documents', 0, 6, 1),
 ('junk', 'Junk', 'Accidental, black or very blurry shots, pocket shots, test shots, broken files.', '{year}/Junk', 0, 7, 1),
 ('unsorted', 'Unsorted', 'Not decided yet.', NULL, 0, 8, 1);
"#;

/// M3: imported location history and dismissed suggestions.
const M2: &str = r#"
-- Points are packed: i64 UTC ms, f64 latitude, f64 longitude, f32 accuracy.
CREATE TABLE tracks (
  id INTEGER PRIMARY KEY,
  name TEXT NOT NULL,
  format TEXT NOT NULL,
  points BLOB NOT NULL,
  count INTEGER NOT NULL,
  start_utc INTEGER,
  end_utc INTEGER,
  imported_at INTEGER NOT NULL
);
-- Suggestions the owner rejected or sets they dismissed, by kind and key.
CREATE TABLE dismissed (
  kind TEXT NOT NULL,
  key TEXT NOT NULL,
  at INTEGER NOT NULL,
  PRIMARY KEY (kind, key)
);
CREATE INDEX assets_camera ON assets(make, model, serial);
"#;

/// M4: audits of auto-accepted decisions, for the precision estimate.
const M3: &str = r#"
CREATE TABLE audits (
  asset_id INTEGER PRIMARY KEY REFERENCES assets(id) ON DELETE CASCADE,
  -- The category Roll decided, and whether the owner agreed.
  category_key TEXT NOT NULL,
  ok INTEGER,
  at INTEGER NOT NULL
);
"#;

/// M4: full-text search over names, folders, tags, places and text.
const M4: &str = r#"
CREATE VIRTUAL TABLE fts USING fts5(
  name, folder, tags, place, body,
  tokenize = 'unicode61 remove_diacritics 2'
);
"#;

/// M4: image embeddings (f16, one row per item and model) and OCR text.
const M5: &str = r#"
CREATE TABLE embeddings (
  asset_id INTEGER NOT NULL REFERENCES assets(id) ON DELETE CASCADE,
  model TEXT NOT NULL,
  dim INTEGER NOT NULL,
  vec BLOB NOT NULL,
  at INTEGER NOT NULL,
  PRIMARY KEY (asset_id, model)
);
CREATE INDEX embeddings_model ON embeddings(model);
CREATE TABLE ocr (
  asset_id INTEGER PRIMARY KEY REFERENCES assets(id) ON DELETE CASCADE,
  model TEXT NOT NULL,
  text TEXT NOT NULL,
  -- Share of the image covered by text boxes, 0–1.
  area REAL NOT NULL DEFAULT 0,
  at INTEGER NOT NULL
);
"#;

/// M6: events keep their detection key and whether the owner named them;
/// event actions keep the rows they change, for undo.
const M6: &str = r#"
ALTER TABLE events ADD COLUMN key TEXT;
ALTER TABLE events ADD COLUMN custom_name INTEGER NOT NULL DEFAULT 0;
CREATE TABLE event_undo (
  batch_id INTEGER NOT NULL,
  event_id INTEGER NOT NULL,
  -- The event as JSON before the batch; NULL when the batch created it.
  row TEXT,
  assets TEXT NOT NULL,
  PRIMARY KEY (batch_id, event_id)
);
"#;

/// Immich users an event's album is shared with (JSON array of user ids).
/// Decision models (M5): measured prices and token counts per model, runs
/// with their budget, and one row per item and run so a run resumes
/// without sending an item twice. The model check's labelled sample.
/// Text embeddings of category descriptions and searches, so the large
/// text tower loads only for new text; taught categories.
const M12: &str = r#"
CREATE TABLE text_embeddings (
  model TEXT NOT NULL,
  text TEXT NOT NULL,
  vec BLOB NOT NULL,
  at INTEGER NOT NULL,
  PRIMARY KEY (model, text)
);
-- Categories the owner taught with examples; retrained on corrections.
ALTER TABLE categories ADD COLUMN taught INTEGER NOT NULL DEFAULT 0;
"#;

/// Sharper built-in definitions after the first real model run: memes
/// were taken for photos and chat screenshots for social media. Only
/// definitions the owner has not edited change.
const M11: &str = r#"
UPDATE categories SET definition = 'Real-life photos and videos taken with a camera: people, places, events, things, including ones received from friends. No added captions, app frames or screen interface.' WHERE key = 'photos' AND builtin = 1 AND definition = 'Real-life photos and videos: people, places, events, things. Includes personal photos received from friends.';
UPDATE categories SET definition = 'Captures of a whole phone or computer screen: apps, chats, settings, mail, web pages, maps, games, usually with a status bar or app interface. A chat or feed captured from the screen is a screenshot.' WHERE key = 'screenshots' AND builtin = 1 AND definition = 'Screen captures and screen recordings from any device.';
UPDATE categories SET definition = 'Made to be funny or shared: an image with a caption added over or above it (even a real photo), reaction images, comics, joke formats, stickers, GIFs.' WHERE key = 'memes' AND builtin = 1 AND definition = 'Made to be funny or shared: captions over images, reaction images, comics, joke formats, stickers, GIFs.';
UPDATE categories SET definition = 'A single post, story or picture saved from a social app or website, often with the app''s frame (user name, likes, caption below): other people''s photos, outfits, recipes, products, news, quotes. Not memes, not whole-screen captures of chats or settings.' WHERE key = 'social_media' AND builtin = 1 AND definition = 'Saved posts, stories, reels and pictures from apps and websites that are not memes: other people''s photos, outfits, recipes, products, news, quotes.';
"#;

const M10: &str = r#"
CREATE TABLE ai_models (
  -- 'openrouter' or a local server's base URL
  provider TEXT NOT NULL,
  model TEXT NOT NULL,
  -- JSON price, USD per token, image and request
  price TEXT,
  -- measured averages from earlier runs
  input_per_item REAL,
  output_per_item REAL,
  updated_at INTEGER NOT NULL,
  PRIMARY KEY (provider, model)
);
CREATE TABLE ai_runs (
  id INTEGER PRIMARY KEY,
  -- 'decide' (proposals for Review) or 'check' (the model check)
  kind TEXT NOT NULL,
  provider TEXT NOT NULL,
  model TEXT NOT NULL,
  prompt_version TEXT NOT NULL,
  batch_size INTEGER NOT NULL,
  items INTEGER NOT NULL,
  estimate_usd REAL NOT NULL,
  budget_usd REAL,
  spent_usd REAL NOT NULL DEFAULT 0,
  -- 'ready', 'running', 'stopped', 'capped', 'done', 'failed'
  state TEXT NOT NULL,
  note TEXT,
  seconds REAL NOT NULL DEFAULT 0,
  created_at INTEGER NOT NULL,
  finished_at INTEGER
);
CREATE TABLE ai_items (
  run_id INTEGER NOT NULL REFERENCES ai_runs(id) ON DELETE CASCADE,
  asset_id INTEGER NOT NULL REFERENCES assets(id) ON DELETE CASCADE,
  -- 0 pending, 1 sent, 2 done, 3 failed
  state INTEGER NOT NULL DEFAULT 0,
  cost REAL,
  tokens_in INTEGER,
  tokens_out INTEGER,
  -- the model's answer (JSON), NULL for unsure
  verdict TEXT,
  error TEXT,
  at INTEGER,
  PRIMARY KEY (run_id, asset_id)
);
CREATE INDEX ai_items_asset ON ai_items(asset_id);
CREATE TABLE ai_check (
  asset_id INTEGER PRIMARY KEY REFERENCES assets(id) ON DELETE CASCADE,
  -- the owner's category key; NULL until labelled
  truth TEXT,
  labeled_at INTEGER
);
ALTER TABLE assets ADD COLUMN caption TEXT;
"#;

const M9: &str = r#"
ALTER TABLE events ADD COLUMN immich_share TEXT NOT NULL DEFAULT '[]';
"#;

/// What kind of export an import is, and what Roll found in it.
const M8: &str = r#"
ALTER TABLE imports ADD COLUMN kind TEXT NOT NULL DEFAULT 'folder';
ALTER TABLE imports ADD COLUMN facts TEXT NOT NULL DEFAULT '[]';
"#;

/// Add photos: one row per import, tied to its staging root.
const M7: &str = r#"
CREATE TABLE imports (
  id INTEGER PRIMARY KEY,
  root_id INTEGER NOT NULL UNIQUE REFERENCES roots(id) ON DELETE CASCADE,
  -- 'checking', 'applied' or 'closed'
  state TEXT NOT NULL DEFAULT 'checking',
  -- the owner's event name; NULL proposes one
  name TEXT,
  dates_ok INTEGER NOT NULL DEFAULT 0,
  places_ok INTEGER NOT NULL DEFAULT 0,
  -- JSON array of asset ids the owner left out
  left_out TEXT NOT NULL DEFAULT '[]',
  batch_id INTEGER,
  created_at INTEGER NOT NULL
);
"#;
