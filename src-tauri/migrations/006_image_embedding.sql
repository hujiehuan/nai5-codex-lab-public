-- v0.3.0：画风索引（CONTRACT §12.1）、重画画师（§12.5）与重画对齐线上 /反推 后多回的字段（§12.6）。
-- image_embedding：每条展示图（ordinal=0）在某个模型下的向量；vec 是 f32 小端 dim 个；image_sha256 是算向量时的图片哈希，
-- 与 entry_image.sha256 不一致（图片换了）就重算；换模型 = 新的 model 行，老模型的行留着无害（同步只看当前模型）。
-- 不加外键：条目下架（removed_at）后向量留着，查询时 JOIN codex_entry 过滤即可，删条目也不用先删向量。
CREATE TABLE IF NOT EXISTS image_embedding (
  entry_id TEXT NOT NULL, ordinal INTEGER NOT NULL, model TEXT NOT NULL, dim INTEGER NOT NULL,
  vec BLOB NOT NULL,            -- f32 小端，dim 个
  image_sha256 TEXT,            -- 算向量时的图片哈希；图片换了要重算
  computed_at TEXT NOT NULL,
  PRIMARY KEY (entry_id, ordinal, model)
);
CREATE INDEX IF NOT EXISTS image_embedding_model ON image_embedding(model, ordinal);
-- §12.5：重画时注入的画师串（检索命中的 core / pack_tags），null = 没注入。
ALTER TABLE reverse_run ADD COLUMN render_artist TEXT;
-- §12.6：重画与 插件 /反推 逐条对齐后插件多回的东西。render_artist_source = 画师来源（pool / given / none）；render_size = 实际尺寸 WxH；
-- render_size_source = 尺寸来源（按原图宽高比选的，或缺原图宽高时退到 tags）；plugin_dropped = 线上 _clean_tags 口径会多剔的 token（JSON 数组）；
-- plugin_would_reject = 线上口径下清洗后 < 5 条（0/1，NULL 读作 0）；render_candidates = 全部候选的 JSON 数组
-- [{file, seed, quality_score, quality_notes, local_path}]，本地文件 labruns/reverse/<run_id>_<idx>.jpg。老行与老插件的重画一律 NULL。
ALTER TABLE reverse_run ADD COLUMN render_artist_source TEXT;
ALTER TABLE reverse_run ADD COLUMN render_size TEXT;
ALTER TABLE reverse_run ADD COLUMN render_size_source TEXT;
ALTER TABLE reverse_run ADD COLUMN plugin_dropped TEXT;
ALTER TABLE reverse_run ADD COLUMN plugin_would_reject INTEGER;
ALTER TABLE reverse_run ADD COLUMN render_candidates TEXT;
