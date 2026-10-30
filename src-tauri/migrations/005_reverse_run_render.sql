-- v0.2.4：反推重画对照与散文题（CONTRACT §11.4）。
-- truth_kind：tags（标签题，有指标）| prose（真值是散文，metric 为空）| none（本地图片题）。
-- render_*：插件 /page/lab/reverse/render 的出图结果；render_file 是插件端文件名，render_local 是拉到本地的绝对路径
-- （labruns/reverse/<run_id>.jpg）；quality_notes 是 JSON 数组；失败只写 render_error。
ALTER TABLE reverse_run ADD COLUMN truth_kind TEXT NOT NULL DEFAULT 'tags';
ALTER TABLE reverse_run ADD COLUMN render_file TEXT;
ALTER TABLE reverse_run ADD COLUMN render_local TEXT;
ALTER TABLE reverse_run ADD COLUMN render_seed INTEGER;
ALTER TABLE reverse_run ADD COLUMN render_prompt TEXT;
ALTER TABLE reverse_run ADD COLUMN quality_score REAL;
ALTER TABLE reverse_run ADD COLUMN quality_notes TEXT;
ALTER TABLE reverse_run ADD COLUMN similarity_score REAL;
ALTER TABLE reverse_run ADD COLUMN similarity_reason TEXT;
ALTER TABLE reverse_run ADD COLUMN render_error TEXT;
-- 契约 §11.4 之外的两列（记在 docs/CONTRACT_CHANGES.md「后端 v0.2.4」）：prose / none 题没有 reverse_metric 行，
-- 但 RunRow.leak_appearance / leak_banned 任何 truth_kind 都要填，泄漏数得有地方放；tags 题两边都写、读时 COALESCE。
ALTER TABLE reverse_run ADD COLUMN leak_appearance INTEGER NOT NULL DEFAULT 0;
ALTER TABLE reverse_run ADD COLUMN leak_banned INTEGER NOT NULL DEFAULT 0;
-- 老行：本地图片题按 §11.4 归为 none；有指标的老行把泄漏数抄过来（老的图包题一律保留 tags 与已算好的指标，不回头重判散文）
UPDATE reverse_run SET truth_kind = 'none' WHERE source_kind = 'local_file';
UPDATE reverse_run SET
  leak_appearance = COALESCE((SELECT m.appearance_leak FROM reverse_metric m WHERE m.run_id = reverse_run.run_id), 0),
  leak_banned = COALESCE((SELECT m.banned_leak FROM reverse_metric m WHERE m.run_id = reverse_run.run_id), 0);
