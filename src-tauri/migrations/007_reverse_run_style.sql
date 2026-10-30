-- v0.3.1：精准反推（CONTRACT §12.7）——重画前自动用画风索引在两本词典全量里检索原图最像的一条，把它的画师串加进重画
-- （桌面端把 artist_mode="style" 翻译成对插件的 given + artist，插件不改）。
-- render_style_entry_id / render_style_title / render_style_score / render_style_decision：用到的那条命中（相似度不够被兜底时也记最像的那条，
-- 靠 render_style_fallback 区分）；render_style_fallback = 1 表示这次没用检索结果（无命中 / score < style_min_score），按 style_fallback 退到 pool / none
-- （0/1，NULL 读作 0）。非 style 模式的重画与老行五列全 NULL。
ALTER TABLE reverse_run ADD COLUMN render_style_entry_id TEXT;
ALTER TABLE reverse_run ADD COLUMN render_style_title TEXT;
ALTER TABLE reverse_run ADD COLUMN render_style_score REAL;
ALTER TABLE reverse_run ADD COLUMN render_style_decision TEXT;
ALTER TABLE reverse_run ADD COLUMN render_style_fallback INTEGER;
