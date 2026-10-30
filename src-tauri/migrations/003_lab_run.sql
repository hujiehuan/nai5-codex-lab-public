-- v0.2.0：画图实验室运行记录的本地镜像（CONTRACT §9.4）。
-- json 是插件返回的整份 LabRun；只在 lab_run_status 拿到 done|error（终态）时落盘，评分（fav/dis）写插件成功后同步改这份 json。
-- lab_run_list 把本表与插件 /page/lab/run/list 按 run_id 合并（本地优先，created_at 倒序）。
CREATE TABLE IF NOT EXISTS lab_run (
  run_id     TEXT PRIMARY KEY,
  mode       TEXT NOT NULL,
  created_at TEXT NOT NULL,
  status     TEXT NOT NULL,
  json       TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS lab_run_created ON lab_run(created_at);
