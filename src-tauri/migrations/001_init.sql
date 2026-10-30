-- nai5-codex-lab schema v1
CREATE TABLE IF NOT EXISTS schema_version (version INTEGER NOT NULL);
CREATE TABLE IF NOT EXISTS app_state (key TEXT PRIMARY KEY, value TEXT);

-- 上游镜像（只由同步写）
CREATE TABLE IF NOT EXISTS codex_release (
  release_id   TEXT PRIMARY KEY,
  published_at TEXT NOT NULL,
  content_hash TEXT NOT NULL,
  fetched_at   TEXT NOT NULL,
  raw_dir      TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS codex_book (
  codex_id     TEXT PRIMARY KEY,
  type         TEXT NOT NULL,
  title        TEXT NOT NULL,
  version      TEXT NOT NULL,
  author       TEXT,
  entry_count  INTEGER,
  tree_json    TEXT NOT NULL,
  update_filters_json TEXT,
  file_sha256  TEXT,
  release_id   TEXT REFERENCES codex_release(release_id)
);
CREATE TABLE IF NOT EXISTS codex_entry (
  entry_id      TEXT PRIMARY KEY,
  codex_id      TEXT NOT NULL REFERENCES codex_book(codex_id),
  book_order    INTEGER NOT NULL,
  entry_version INTEGER NOT NULL DEFAULT 1,
  title         TEXT NOT NULL,
  path_json     TEXT NOT NULL,
  tags          TEXT NOT NULL,
  negative      TEXT,
  character_prompts_json TEXT,
  note          TEXT,
  rating        TEXT NOT NULL,
  is_new        INTEGER NOT NULL DEFAULT 0,
  update_batches_json TEXT,
  asset_rev     TEXT,
  image_w       INTEGER,
  image_h       INTEGER,
  first_seen_release TEXT NOT NULL,
  last_seen_release  TEXT NOT NULL,
  removed_at    TEXT
);
CREATE INDEX IF NOT EXISTS codex_entry_book ON codex_entry(codex_id, book_order);
CREATE INDEX IF NOT EXISTS codex_entry_rating ON codex_entry(codex_id, rating);
CREATE TABLE IF NOT EXISTS codex_entry_history (
  entry_id TEXT NOT NULL,
  entry_version INTEGER NOT NULL,
  snapshot_json TEXT NOT NULL,
  replaced_at TEXT NOT NULL,
  PRIMARY KEY(entry_id, entry_version)
);
CREATE TABLE IF NOT EXISTS entry_image (
  entry_id     TEXT NOT NULL REFERENCES codex_entry(entry_id),
  ordinal      INTEGER NOT NULL,
  image_file   TEXT NOT NULL,
  original_file TEXT,
  raw_tag      TEXT,
  asset_rev    TEXT,
  etag         TEXT,
  local_path   TEXT,
  thumb_path   TEXT,
  original_local_path TEXT,
  bytes        INTEGER,
  downloaded_at TEXT,
  error        TEXT,
  PRIMARY KEY(entry_id, ordinal)
);

-- 本地决策层（同步永不触碰）
CREATE TABLE IF NOT EXISTS library_membership (
  entry_id    TEXT PRIMARY KEY REFERENCES codex_entry(entry_id),
  library     TEXT NOT NULL,
  decision    TEXT NOT NULL,
  note        TEXT,
  decided_at  TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS membership_audit (
  id INTEGER PRIMARY KEY,
  entry_id TEXT NOT NULL,
  from_decision TEXT,
  to_decision TEXT NOT NULL,
  at TEXT NOT NULL,
  via TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS membership_audit_entry ON membership_audit(entry_id, id);
CREATE TABLE IF NOT EXISTS artist_blacklist (
  name TEXT PRIMARY KEY,
  reason TEXT,
  added_at TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS artist_parse (
  entry_id     TEXT PRIMARY KEY REFERENCES codex_entry(entry_id),
  artists_json TEXT NOT NULL,
  auxiliary_json TEXT NOT NULL,
  negative_tail_json TEXT NOT NULL,
  signature    TEXT NOT NULL,
  parse_warnings TEXT
);
CREATE INDEX IF NOT EXISTS artist_parse_sig ON artist_parse(signature);

-- 日志
CREATE TABLE IF NOT EXISTS sync_log (
  id INTEGER PRIMARY KEY,
  started_at TEXT NOT NULL,
  finished_at TEXT,
  kind TEXT NOT NULL,
  release_id TEXT,
  added INTEGER NOT NULL DEFAULT 0,
  changed INTEGER NOT NULL DEFAULT 0,
  images_ok INTEGER NOT NULL DEFAULT 0,
  images_failed INTEGER NOT NULL DEFAULT 0,
  failures_json TEXT
);
CREATE TABLE IF NOT EXISTS app_event (
  id INTEGER PRIMARY KEY,
  at TEXT NOT NULL,
  level TEXT NOT NULL,
  area TEXT NOT NULL,
  message TEXT NOT NULL,
  data_json TEXT
);

-- 视图（用户备注起别名 member_note，避免与 codex_entry.note 冲突）
CREATE VIEW IF NOT EXISTS artist_library AS
  SELECT e.*, m.decision, m.note AS member_note, m.decided_at, p.artists_json, p.signature
  FROM codex_entry e
  JOIN library_membership m ON m.entry_id = e.entry_id
  LEFT JOIN artist_parse p ON p.entry_id = e.entry_id
  WHERE e.codex_id IN ('artist_nai5_personal','local_artist') AND m.decision='include' AND e.removed_at IS NULL;
CREATE VIEW IF NOT EXISTS pack_library AS
  SELECT e.*, m.decision, m.note AS member_note, m.decided_at
  FROM codex_entry e
  JOIN library_membership m ON m.entry_id = e.entry_id
  WHERE e.codex_id IN ('nai5_community_pack','local_pack') AND m.decision='include' AND e.removed_at IS NULL;

-- 反推实验室
CREATE TABLE IF NOT EXISTS reverse_template (
  template_id INTEGER PRIMARY KEY,
  name TEXT NOT NULL,
  version INTEGER NOT NULL,
  body TEXT NOT NULL,
  parent_id INTEGER,
  created_at TEXT NOT NULL,
  note TEXT,
  UNIQUE(name, version)
);
CREATE TABLE IF NOT EXISTS reverse_ruleset (
  ruleset_id INTEGER PRIMARY KEY,
  name TEXT NOT NULL,
  version INTEGER NOT NULL,
  banned_json TEXT NOT NULL,
  appearance_regex TEXT NOT NULL,
  accessory_ok_regex TEXT NOT NULL,
  refusal_regex TEXT NOT NULL,
  always_drop_json TEXT NOT NULL,
  max_words INTEGER NOT NULL DEFAULT 6,
  max_len INTEGER NOT NULL DEFAULT 40,
  created_at TEXT NOT NULL,
  note TEXT,
  UNIQUE(name, version)
);
CREATE TABLE IF NOT EXISTS reverse_provider (
  provider_id INTEGER PRIMARY KEY,
  name TEXT NOT NULL,
  base_url TEXT NOT NULL,
  model TEXT NOT NULL,
  max_tokens INTEGER NOT NULL DEFAULT 1100,
  enabled INTEGER NOT NULL DEFAULT 1,
  created_at TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS reverse_batch (
  batch_id TEXT PRIMARY KEY,
  created_at TEXT NOT NULL,
  finished_at TEXT,
  template_id INTEGER NOT NULL,
  ruleset_id INTEGER NOT NULL,
  provider_ids_json TEXT NOT NULL,
  question_count INTEGER NOT NULL,
  status TEXT NOT NULL,
  note TEXT
);
CREATE TABLE IF NOT EXISTS reverse_run (
  run_id INTEGER PRIMARY KEY,
  batch_id TEXT NOT NULL REFERENCES reverse_batch(batch_id),
  created_at TEXT NOT NULL,
  source_kind TEXT NOT NULL,
  source_ref TEXT NOT NULL,
  image_ordinal INTEGER,
  image_sha256 TEXT NOT NULL,
  truth_source TEXT,
  template_id INTEGER NOT NULL,
  ruleset_id INTEGER NOT NULL,
  provider_id INTEGER NOT NULL,
  request_json TEXT NOT NULL,
  raw_response TEXT,
  parsed_json TEXT,
  cleaned_tags_json TEXT,
  latency_ms INTEGER,
  prompt_tokens INTEGER,
  completion_tokens INTEGER,
  status TEXT NOT NULL,
  error TEXT
);
CREATE INDEX IF NOT EXISTS reverse_run_batch ON reverse_run(batch_id, run_id);
CREATE TABLE IF NOT EXISTS reverse_metric (
  run_id INTEGER PRIMARY KEY REFERENCES reverse_run(run_id),
  truth_json TEXT NOT NULL,
  truth_eval_json TEXT NOT NULL,
  expected_miss_json TEXT NOT NULL,
  hit INTEGER NOT NULL,
  missed INTEGER NOT NULL,
  extra INTEGER NOT NULL,
  expected_hit INTEGER NOT NULL,
  precision REAL,
  recall REAL,
  f1 REAL,
  appearance_leak INTEGER NOT NULL,
  banned_leak INTEGER NOT NULL,
  ruleset_drop INTEGER NOT NULL,
  contradiction INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS reverse_judgment (
  run_id INTEGER PRIMARY KEY REFERENCES reverse_run(run_id),
  score INTEGER,
  comment TEXT,
  at TEXT NOT NULL
);

INSERT INTO schema_version(version) SELECT 1 WHERE NOT EXISTS (SELECT 1 FROM schema_version);
