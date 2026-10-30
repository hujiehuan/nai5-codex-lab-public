-- v0.2.3：反推实验室的模型端点分两种来源（CONTRACT §10.1）。
-- kind = 'openai'：本地 OpenAI 兼容端点，base_url / model / 钥匙串 key 照旧；
-- kind = 'plugin'：AstrBot 里配置好的 provider，base_url / model 存空串、不需要 key，plugin_provider_id 是插件里的 provider id。
-- 老行按默认值成 'openai'，plugin_provider_id 为 NULL。
ALTER TABLE reverse_provider ADD COLUMN kind TEXT NOT NULL DEFAULT 'openai';
ALTER TABLE reverse_provider ADD COLUMN plugin_provider_id TEXT;
