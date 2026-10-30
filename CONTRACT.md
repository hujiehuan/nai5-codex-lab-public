# 前后端契约

本文件是前后端共用的接口契约：Tauri 命令、参数、DTO、事件，以及桌面端调用的插件路由，都以此为准。前端类型在 `src/types.ts`、`src/types-creative.ts`、`src/types-studio.ts`，后端在 `src-tauri/src/models.rs`（实验相关的 DTO 在 `creative.rs`、`studio/`），两边逐字段一致。前后端并行开发，只靠这份文件对齐：改形状时先改这里，再改两边。

> 章节编号沿用开发期编号：每个功能加入时取下一个编号，所以中间有空号；后加的章节可能修订前面章节的约定，文中写明「覆盖 §N」。下文按编号排列。

## 0. 通用约定

- 命令名 `snake_case`。**参数名也是 `snake_case`**：Rust 侧每个命令都写 `#[tauri::command(rename_all = "snake_case")]`，前端 `invoke("list_entries", { query })`。
- DTO 字段全部 `snake_case`，Rust `serde` 不做 rename。`types.ts` 是形状的权威。
- 时间一律 ISO 8601 UTC 字符串，如 `2026-09-06T12:00:00Z`（Rust：`Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true)`）。
- 命令返回 `Result<T, AppError>`。`AppError` 序列化为 `{ "code": string, "message": string }`；前端 `invoke` 抛出的就是这个对象。常见 code：`sync_running`、`batch_running`、`not_found`、`bad_request`、`upstream`、`io`、`db`、`secret`、`network`、`plugin_rejected`。
- 图片：命令只返回本地**绝对路径**；前端用 `convertFileSrc(path)` 显示。没下载的图 `local_path` 为 `null`，前端显示占位。
- 长任务用 `tauri::ipc::Channel<T>` 推进度，命令本身立即返回（`sync_start` 返回时后台任务已启动）。前端：`const ch = new Channel<SyncEvent>(); ch.onmessage = e => …; await invoke("sync_start", { force, on_event: ch })`。
- 所有 IPC 的数据参数在边界做 JSON 快照（保留 Channel 的序列化协议），避免把 Vue Proxy 传给 Tauri。`mock/` 只在独立的离线开发模式（`vite --mode mock`）启用，不读真实配置或密钥。
- 数据目录 = 应用数据目录（Tauri `app_data_dir`）。子目录：`codex_raw/<release>/`、`images/<codex_id>/`、`originals/<codex_id>/`、`thumbs/<codex_id>/`、`models/<model_id>/`、`backups/`、`exports/`、`logs/`。数据库 `library.db` 在根目录。
- 缩略图：最长边 320 px 的 JPEG（质量 80），文件名 `<image_file>.jpg`，放 `thumbs/<codex_id>/`。用 JPEG 而不是 WebP，避免引入原生 libwebp 依赖。
- 上游请求：UA `nai5-codex-lab/<version>`；对 `assets.quicktagcloud.com` 全局并发 4、相邻请求间隔 ≥ 100 ms；对 `novelai.quicktagcloud.com` 每次同步 1 次。
- SQLite：`PRAGMA foreign_keys=ON; journal_mode=WAL; synchronous=NORMAL;`。DDL 在 `src-tauri/migrations/`，`schema_version` 表记录已应用版本，当前为 8。迁移与快照规则见 `docs/MIGRATIONS.md`。
- 日志：`tracing` JSON 行写 `logs/nai5-codex-lab.YYYY-MM-DD.log`（按天滚动），同时写 `app_event` 表（同步起止、导出、推送、批量决策、实验批次起止、错误）。
- 插件路由：桌面端调用的插件页面路由统一为 `/api/plug/{plugin_name}/page/...`，`plugin_name` 取自 `Settings.plugin_name`（§1）。下文写作 `page/...` 或 `/page/...` 的路由都相对这个前缀。所有插件页面路由先过 AstrBot WebUI 的 JWT；库与实验路由再核对 `X-Library-Token`。AstrBot 把插件返回包在 `{ status, message, data }` 里，桌面端取 `data`；2xx 但 `status="error"` 同样按错误处理。

## 1. 命令清单

`Settings`（存 `app_state('settings')` JSON；缺键补默认，已退役的键忽略）：

| 字段 | 类型 | 说明 |
|---|---|---|
| `plugin_base_url` | string | AstrBot WebUI 地址，如 `http://127.0.0.1:6185`，不带尾斜杠 |
| `plugin_name` | string | 插件目录名，缺省 `astrbot_plugin_codexlab`；必须匹配 `^[A-Za-z0-9_]{1,64}$`，否则 `settings_set` 返回 `bad_request`。插件页面前缀 = `/api/plug/{plugin_name}/page` |
| `webui_username` | string | WebUI 用户名；密码存钥匙串 `webui_password` |
| `download_concurrency` | number | 图片下载并发，1–4 |
| `style_device` | `"auto" \| "metal" \| "cpu"` | 画风编码器的推理设备，见 §12.2 |

| 命令 | 参数 | 返回 | 说明 |
|---|---|---|---|
| `app_info` | — | `AppInfo` | 版本、数据目录、`latest_release` |
| `settings_get` | — | `Settings` | |
| `settings_set` | `patch: SettingsPatch` | `Settings` | 合并写回，逐键校验，未知键拒绝（`bad_request`） |
| `secret_set` | `account: string, value: string` | — | 钥匙串 service `nai5-codex-lab`；account 取值：`webui_password`、`library_token`、`provider:<provider_id>` |
| `secret_has` | `account` | `boolean` | |
| `secret_delete` | `account` | — | |
| `sync_status` | — | `SyncStatus` | |
| `sync_start` | `force: boolean, on_event: Channel<SyncEvent>` | — | 已在跑则 `sync_running` |
| `sync_cancel` | — | — | 取消后台任务，已落地的不回滚 |
| `sync_logs` | `limit: number` | `SyncLog[]` | 最新在前 |
| `list_books` | — | `Book[]` | 含每本的决策计数 |
| `list_entries` | `query: EntryQuery` | `EntryPage` | 分页；`counts` 是**当前筛选**下的三态计数 |
| `get_entry` | `entry_id: string` | `EntryDetail` | |
| `set_decision` | `entry_ids: string[], decision: Decision, via: DecisionVia` | `{ changed: number }` | 每条写 `membership_audit` |
| `set_member_note` | `entry_id: string, note: string` | — | |
| `undo_decision` | — | `UndoResult \| null` | 撤销最近一次非 undo 的审计记录 |
| `download_images` | `entry_id: string, kind: "display_all" \| "originals", on_event: Channel<SyncEvent>` | — | 按需下载；与同步共用队列与节流 |
| `blacklist_list` | — | `BlacklistItem[]` | 排除名单 |
| `blacklist_add` | `name: string, reason: string` | — | name 先 canonical（小写、空格→`_`） |
| `blacklist_remove` | `name: string` | — | |
| `export_libraries` | `options: ExportOptions` | `ExportResult` | 写 `exports/<时间戳>/`（或 `out_dir`）三个文件，规则见 §3；`ExportOptions = { out_dir?: string }` |
| `push_to_plugin` | — | `PushResult` | 内存构建筛选池后推送，见 §3.4 |
| `reverse_templates_list` | — | `ReverseTemplate[]` | |
| `reverse_template_save` | `input: ReverseTemplateInput` | `ReverseTemplate` | 同名则 version+1，永不覆盖 |
| `reverse_rulesets_list` | — | `ReverseRuleset[]` | |
| `reverse_ruleset_save` | `input: ReverseRulesetInput` | `ReverseRuleset` | 同上 |
| `reverse_providers_list` | — | `ReverseProvider[]` | 不含 key |
| `reverse_provider_save` | `input: ReverseProviderInput` | `ReverseProvider` | key 走 `secret_set("provider:<id>")` |
| `reverse_provider_delete` | `provider_id: number` | — | |
| `reverse_questions` | `query: QuestionQuery` | `Question[]` | 条目 × 图片 ordinal，带真值 |
| `reverse_pick_local_image` | — | `string \| null` | Rust 侧打开文件对话框，并把图片拷入数据目录 |
| `reverse_run_batch` | `req: RunBatchRequest, on_event: Channel<RunEvent>` | `{ batch_id: string }` | 立即返回，后台跑 |
| `reverse_cancel_batch` | `batch_id: string` | — | 停止指定的活动批次；见 §12.9 |
| `reverse_batches_list` | — | `BatchSummary[]` | |
| `reverse_batch_results` | `batch_id: string` | `BatchResults` | |
| `reverse_run_get` | `run_id: number` | `RunDetail` | 含 raw_response 全文 |
| `reverse_compare` | `batch_a: string, batch_b: string` | `CompareResult` | 按 question 对齐 |
| `reverse_judge` | `run_id: number, score: number, comment: string` | — | 人工打分 |
| `events_recent` | `limit: number` | `AppEvent[]` | |
| `plugin_model_selection` | — | `any`（插件 `data`） | 只 GET 插件 `page/model-selection`（常用 LLM 名单），见 §14、§17 |

其余命令按章节定义：

| 章节 | 命令 |
|---|---|
| §8.2 完整池 | `export_full_pool`、`plugin_library_status` |
| §9.4 旧版出图实验 | `lab_run_preview`、`lab_run_start`、`lab_run_status`、`lab_run_list`、`lab_run_image`、`lab_run_rate` |
| §10.1 插件端模型 | `plugin_providers` |
| §11.4 重画对照 | `reverse_render_run` |
| §12 画风索引 | `style_models_list`、`style_model_select`、`style_model_status`、`style_model_download`、`style_model_download_cancel`、`style_index_rebuild`、`style_search` |
| §14 原图展示 | `reverse_source_image` |
| §15 创作实验 | `creative_capabilities`、`creative_references`、`creative_plan`、`creative_edit`、`creative_plans_list`、`creative_plan_references`、`creative_start`、`creative_status`、`creative_list`、`creative_cancel`、`creative_image` |
| §16 总实验室 | `studio_capabilities`、`studio_references`、`studio_prepare`、`studio_plan`、`studio_plan_get`、`studio_start`、`studio_status`、`studio_list`、`studio_cancel`、`studio_image`、`studio_compare`、`studio_skills_list`、`studio_skills_import`、`studio_skills_delete`、`studio_workspace_get`、`studio_workspace_save`、`style_compare`、`style_compare_cancel` |
| §20 原图拖放 | `reverse_import_local_image` |
| §22 软件更新 | `app_update_status`、`app_update_choose_source`、`app_update_check`、`app_update_prepare`、`app_update_cancel`、`app_update_install` |
| §25 跟随插件 skill | `studio_skills_sync` |

已删除的命令：`style_classifiers_list`、`style_classifier_download`、`style_classify`、`push_full_to_plugin`、`studio_skills_fetch`，以及早期的插件控制台转发命令和实验室预算查询命令（需要的部分由 `plugin_model_selection` 取代）。

内置数据：首次启动时若 `reverse_template` / `reverse_ruleset` 为空，后端插入 `plugin v1`（模板与规则原文在 `src-tauri/src/reverse/builtin.rs`，与插件反推指令所用的版本一致）。

## 2. 事件形状

`SyncEvent`（同步与按需下载共用）：`{ phase, message, release, added, changed, images_total, images_done, images_failed, embed_total?, embed_done?, embed_failed? }`，`phase ∈ manifest | entries | images | embedding | done | error`（`embedding` 见 §12.2）。`done` 与 `error` 各只发一次，之后不再发。

`RunEvent`：`{ batch_id, run_id, status, done, total, message }`，`status ∈ started | run_done | run_failed | batch_done | batch_failed | batch_cancelled`。

## 3. 导出与推送规则（后端实现，前端只展示结果）

### 3.1 `nai5_artist_pool.json`（风格池）← 视图 `artist_library`

1. 按 `decided_at` 降序、`book_order` 升序遍历已加入的风格词典条目（§5）。
2. 每条输出条目原始 `tags` 去首尾空白后的完整风格配方：保留所有 artist、质量词、线条 / 配色词、年份、负权重、顺序与内部格式，不拼接固定质量后缀（§12.0）。
3. 跳过：配方为空；配方里解析出的任一画师名（canonical 比较）命中排除名单 `artist_blacklist`；与前面已输出的配方完全相同（按完整串去重）。跳过数计入 `ExportResult.skipped`。
4. 文件是 `list[string]`。

画师名解析（用于排除名单比对、`artist_parse` 缓存与 `core`）：剥 `<artist>`、`</artist>`、`<style>`、`</style>` 与换行后用 `parseArtistRecipe` 解析；`path[0]` 是上游的单画师分类、且 tags 是不带前缀的名字（不含 `artist:`）时，按画师处理并补前缀。

`core` 序列化（§8.1、§12.2 引用）：artists 按原顺序，权重 `|w−1|<0.001` 省略写 `artist:name`，否则写 `w::artist:name::`（w 最多两位小数、去尾零，`::` 前无空格），用 `, ` 连接，不带质量后缀。

### 3.2 `pack_pool.json` ← 视图 `pack_library`（只含 safe 图包条目，插件端负责与旧池合并）

`{ "source": "法典图鉴 N5社区图包 v<version>", "note": "由 nai5-codex-lab 导出；未过插件清洗", "entries": [ { "s", "c", "t", "tags", "r" } ] }`

- `s` = path[0] 去掉「·」后的作者段；`c` = `"图包/" + path[1] + "/" + path[2]`（缺的段跳过）；`t` = `title + " · " + 前 3 个非画师非质量 token 的 value`；`r` = rating（恒为 `safe`）。
- `tags` 来源与清洗见 §9.6：`parseArtistRecipe` 后剔除 `kind ∈ {artist, quality, negative, year}` 与 value 匹配 `^rating:` 的 token，再剔角色 token；剩余 token 用 `formatToken`（权重 1 写原值，否则 `w::value::`）按原顺序 `, ` 连接。散文条目按 §11.2 原文保留。
- 只含 `rating='safe'`（非 safe 分级的条目在解析时丢弃，不入库）。

### 3.3 `library_export.json`

`{ "schema": "nai5-codex-lab/1", "exported_at", "release_id", "entries": [ { entry_id, codex_id, library, title, path, tags, negative, character_prompts, rating, member_note, images: [ { ordinal, raw_tag, file } ] } ] }`，`file` 是相对数据目录的路径，固定为 `images/<codex_id>/<file>`。

### 3.4 推送筛选池

1. `POST {plugin_base_url}/api/auth/login`，body `{ "username": settings.webui_username, "password": 钥匙串 webui_password }`；响应 `data.token` 为 JWT（AstrBot 返回形状 `{ status, message, data: { token, username, change_pwd_hint } }`）。登录顺序见 §6。
2. `POST {plugin_base_url}/api/plug/{plugin_name}/page/library/import`，头 `Authorization: Bearer <jwt>`、`X-Library-Token: <钥匙串 library_token>`、`Content-Type: application/json`，body `{ "nai5_artist_pool": [...], "pack_pool": {...}, "library_export": {...}, "exported_at", "release_id" }`。三个 JSON 只在内存里构建，推送不落盘。
3. 401 自动重登一次；插件返回非 2xx → `plugin_rejected`，message 带响应正文前 500 字。响应 JSON 原样放进 `PushResult.plugin_response`；`PushResult` 的计数字段为 `nai5` / `pack` / `dropped_blacklist`。

## 4. 反推评测口径（后端 `reverse/metrics.rs` 实现，前端只展示）

- 归一 `norm(t)`：小写、`_`→空格、去首尾空白与权重语法。
- `truth` = `COALESCE(entry_image.raw_tag, codex_entry.tags)` ∪ 所有 `character_prompts[].prompt`，各自 `parseArtistRecipe` 后取 `norm(value)`。
- 剔除类（不在答案里）：`kind ∈ {artist, quality, negative, year}`；quality 扩展表 `absurdres, highres, best illustration, high complexity, highly complexity, ultra complexity, no text`；`^rating:`。
- `expected_miss`（保留在答案但不计分母）：角色名（`\(.+\)$` 括号型、`character_aliases` 角色别名表（合成示例）命中、每个 character_prompts label 的首个非通用 token）；外貌词（规则集 `appearance_regex` 命中且不命中 `accessory_ok_regex`）；人数词 `1girl|1boy|solo|2girls|2boys|multiple girls|multiple boys`；规则集 `banned` 命中。
- `pred_raw` = `parsed.tags` 归一集合；`pred` = `cleaned_tags ∪ {parsed.framing}`。
- `truth_eval = truth − 剔除类 − expected_miss`；`hit = |pred ∩ truth_eval|`；`precision = hit/|pred|`（`|pred|=0` 时 0）；`recall = hit/|truth_eval|`（分母 0 时 null）；`f1` 常规；`extra = |pred − truth|`；`expected_hit = |pred ∩ expected_miss|`；`appearance_leak`、`banned_leak` 在 `pred_raw` 上算；`ruleset_drop = |pred_raw| − |pred|`（不小于 0）；`contradiction = |pred ∩ norm(negative ∪ character_prompts[].negative)|`。
- 规则集清洗（与插件反推指令的清洗同一规则）：按逗号 / 换行切分 → trim 与去引号 → 去重（含 `always_drop` 初始集合、角色名、framing）→ 丢弃长度 > `max_len` 或词数 > `max_words` 或命中 `refusal_regex` 的 → 丢弃 `banned` 子串命中的 → 丢弃 `appearance_regex` 命中且不命中 `accessory_ok_regex` 的。
- 送模型前预处理：文件 > 9 MB 才缩到最长边 2048、JPEG 88→72→55 直到 ≤ 9 MB；请求体 OpenAI 兼容 `chat/completions`，`messages=[{role:"system", content: template.body}, {role:"user", content:[{type:"image_url", image_url:{url:"data:<mime>;base64,…", detail:"high"}}, {type:"text", text: "Generate the prompt for this image."}]}]`，`temperature=0`，`max_tokens=provider.max_tokens`，超时 180 s。模板里的 `{extra}` 占位替换为空串。
- 解析：从响应文本里取第一个 `{…}` JSON；失败则 `status='error'`；`suitable=false` 或含拒答语按 `status='refused'`。响应只有 `reasoning_content` 没有 `content` 时照常解析，但记一条 warn 并在 `error` 里标注来源。
- 延迟：每次调用记录 `latency_ms`；批次汇总给出每个端点的平均延迟、拒答数与错误数。

## 5. 视图与库的定义（SQL 见 migrations）

- `artist_library`：风格词典来源（`artist_nai5_personal`、`local_artist`；迁移 008 扩展了来源列表）中 `decision='include' AND removed_at IS NULL` 的条目。
- `pack_library`：`codex_id IN ('nai5_community_pack','local_pack') AND decision='include' AND removed_at IS NULL`。
- 三态循环：`undecided → include → exclude → undecided`（前端实现，调用 `set_decision`）。

## 6. 补充约定

- `EntryQuery.rating` / `decision`（以及 `QuestionQuery`）：**显式空数组 `[]` → 0 条**；键缺失 → 无此筛选。`path_prefix` 按 `path_json` 前 N 个元素逐个相等匹配。
- `EntryPage.counts` 应用除 `decision` 之外的全部筛选（筛「只看未定」时仍能看到加入 / 不加入各多少）；`total` 与 `items` 应用全部筛选。
- `set_decision`：决策未变化的条目不写审计、不计入 `changed`；未知 id 静默跳过。
- `undo_decision`：已撤销的审计 id 记在 `app_state('undone_audit_ids')`，连续撤销逐条回退；备注不动。
- `download_images` 有自己的取消令牌，`sync_cancel` 不影响它；「下载全部」的标记持久化在 `app_state('download_all')`，之后每次同步都会补齐这些条目。同一张图同一时刻只允许一个任务写（在飞集合）。
- 本地图片题：`RunBatchRequest.question_ids` 用 `local:<绝对路径>`；`RunRow.question_id` 原样返回该串（不是 null），`metric` 为 null。
- `reverse_provider_save` 先返回 `provider_id`，前端再 `secret_set("provider:<id>")`；`has_key` 由钥匙串是否存在推导；钥匙串「无此条目」视为未设置而不是错误。
- 插件登录：AstrBot ≥ 4.27 由服务器端做哈希（md5 或 pbkdf2），客户端必须发明文；≤ 4.26 的登录页在浏览器里预先 md5。后端先发明文，收到 401 再发 `md5(明文)`，两次都 401 才报 `plugin_rejected`。
- `format_token` 输出 `w::value::`（`::` 前无空格）。
- `pack_pool.json` 跳过清洗后 tags 为空的条目，计入 `ExportResult.skipped`。
- 上游消失 id ≥ 10%：不打 `removed_at`、写 warn 级 `app_event`、同步继续（不做交互式暂停）。
- `codex_raw/<release>/` 额外保留 `current.json` 与 `manifest.json`。

## 7. 分页与排序（v0.1.2）

- `EntryQuery.sort` 为 `book_order | title | decided_at | synced_at | first_seen`，`sort_dir?: "asc" | "desc"`。缺省方向：`book_order` / `title` / `synced_at` 升序，`decided_at` / `first_seen` 降序。次序键一律再按 `book_order` 升序稳定；未知值 `bad_request`。
  - `synced_at` = 该条目 `entry_image` 中 `ordinal=0` 的 `downloaded_at`；为 NULL（未下载）的条目无论方向都排最后。
  - `first_seen` = `codex_release.published_at`（按 `first_seen_release` JOIN）；`local` 等无 release 的条目排最后。
  - `decided_at` 只对有 `library_membership` 的条目有值，其余排最后。
- 前端显式分页：每页 50 / 100 / 200，上一页 / 下一页 / 跳页，`Cmd+←` `Cmd+→` 翻页；`page`、`page_size`、`sort`、`sort_dir` 与其余筛选一起持久化在 `codexlab.filter.<codex_id>`，重开应用回到上次那一页。筛选条件变化时 `page` 归 1；`total` 变小导致当前页越界时回到最后一页。
- 后端 `list_entries` 不变形状，`offset = (page-1) * page_size`。

## 8. 完整池（v0.1.4）

### 8.0 两种池

- **筛选池**：§3 的三个文件（`nai5_artist_pool.json` / `pack_pool.json` / `library_export.json`），只含「已加入」且未命中排除名单的条目；可以本地导出（`export_libraries`），也可以推送到插件（`push_to_plugin`），插件的 `/画`、`/反推` 随机取风格配方时用它。
- **完整池**：两本词典的全部条目（含未加入、未决策、命中排除名单的），每条带决策标记，只做本地导出（`export_full_pool`），供离线分析或外部工具读取。桌面端不推送完整池。

### 8.1 `full_pool.json`（后端 `export::build_full_pool`）

```
{ "schema": "nai5-codex-lab/full/1", "exported_at": ISO8601, "release_id": string|null, "app_version": string,
  "counts": { "<codex_id>": { "total", "include", "exclude", "undecided", "blacklisted" } },
  "codexes": [ { "codex_id", "title", "library": "artist"|"pack", "entries": [ FullEntry ] } ],
  "embedding": null | { "model", "dim", "centered": false, "mean" } }

FullEntry = { "entry_id", "title", "path": string[], "book_order": int, "tags", "negative": string|null,
  "character_prompts": [ { label, prompt, negative? } ], "rating": "safe",
  "decision": "include"|"exclude"|"undecided", "decided_at": ISO|null, "member_note": string|null, "blacklisted": bool,
  "artists": [ { "name", "weight" } ]|null, "core": string|null, "style_prompt": string|null,
  "pack": { s, c, t, tags, r, prose? }|null, "prose": bool, "vec": string|null,
  "images": [ { "ordinal", "raw_tag": string|null, "sha256": string|null, "file": string|null } ] }
```

- codex 顺序固定：`artist_nai5_personal` 在前、`nai5_community_pack` 在后；`entries` 按 `book_order` 升序。
- rating 规则与 §3.2 一致，两本都适用：只有 `safe`。
- `decision`：`library_membership` 有记录取其值，否则 `"undecided"`。`removed_at` 非空的条目不导出。
- `blacklisted`：`artists` 任一 name canonical 命中 `artist_blacklist` 为 true。完整池**不剔除**这些条目，只标记。
- `artists` / `core`：仅 `library='artist'` 且解析出画师时填；`core` 按 §3.1 序列化；解析失败为 null（条目仍导出）。`style_prompt`：风格词典条目为完整原始配方（§12.0），图包条目为 null。
- `pack`：仅 `library='pack'`：按 §3.2 清洗；清洗后 tags 为空 → null（条目仍导出）。`prose` 见 §11.2。
- `images`：该条目 `entry_image` 全部 ordinal；`file` = 已下载则 `images/<codex_id>/<file>`，否则 null；`sha256` = `entry_image.sha256`（未下载 null）。
- `embedding` / `vec`：见 §12.4；没有索引时 `embedding` 为 null。

### 8.2 命令

| 命令 | 参数 | 返回 | 说明 |
|---|---|---|---|
| `export_full_pool` | `options: ExportOptions` | `ExportResult` | 写 `exports/<时间戳>/full_pool.json`；`files` 一项 `name="full_pool.json"`，`count`=条目总数；`skipped=0` |
| `plugin_library_status` | — | `PluginLibraryStatus` | GET `page/library/status`；登录同 §3.4 |

```
PluginLibraryStatus = { reachable: bool, message: string, plugin_version: string|null, filtered: PoolStatus|null, full: PoolStatus|null }
PoolStatus = { imported_at: ISO, release_id: string|null, exported_at: string|null, app_version: string|null, counts: Record<string, number>, file: string, size_bytes: int, backups: string[] }
```

- `filtered.counts` = `{ nai5, pack, kept_old, dropped_blacklist }`；`full` 是插件端已有的完整池记录（按 codex_id 的 total），没有则为 null。
- 网络错误（连不上 / 超时 / 未配置密码或 token）→ `plugin_library_status` 返回 `reachable=false` + `message`，**不抛错**；401 / 403 → `plugin_rejected`。

### 8.3 插件端路由

所有路由都在 WebUI JWT 之后再核对 `X-Library-Token`（同 §3.4）。

- `POST page/library/import`：接收筛选池（§3.4），原子写入插件数据目录，成功后写一份导入元数据（时间、release、计数）；`nai5_artist_pool` 的校验见 §12.8。
- `GET page/library/status`：返回 `{ plugin_version, filtered: PoolStatus|null, full: PoolStatus|null }`。
- 插件把收到的文件存放在自己的数据目录，和插件代码目录分开，不进版本库。

### 8.4 前端

设置页「导出与推送」分两行：筛选池（导出 / 推送）与完整池（导出）。下方「插件当前持有」面板：进入设置页自动拉一次 `plugin_library_status`，有刷新按钮；不可达时显示原因文本，不弹错误提示。

## 9. 旧版出图实验协议（v0.2.0）

这一节是总实验室（§16）之前的出图实验协议。页面已并入总实验室，`/lab/draw` 重定向到 `/studio?mode=draw`；命令和本地记录保留，用于读取历史运行。出图、提示词组装、清洗和评分都在插件里执行，与插件指令走同一套逻辑；桌面端只负责选图包、下参数、看结果和打分。

### 9.0 约定

- 只使用 safe 图包，图包标签恒经清洗。
- 参数包括风格池命中概率 `artist_ratio`（0–1；风格池 = 已推送的筛选池 `nai5_artist_pool.json`）和张数（1–8）。
- 插件 `/画` 指令本身没有图包路径（它按构图 / 服装 / 场景随机组合主题），按图包出图是新增路径；只带自然语言描述、没选图包时才走 `/画` 现有的 LLM 路径。插件指令的行为不变。

### 9.1 请求与计划（两端共用的 DTO，snake_case）

```
LabRunRequest = {
  mode: "draw",
  text: string,                 // 首 token 为角色（或 随机），其余为自然语言描述；可含 风格=xxx
  entry_id: string | null,      // 选中的图包 entry_id；null = 随机
  scope: string[][],            // 随机时的候选范围：path 前缀列表（与 EntryQuery.path_prefix 同语义）；空数组 = 默认范围
  artist_ratio: number,         // 0..1，命中则注入风格池里的配方
  count: number,                // 1..8
  score: boolean,               // 是否评分
  seed: number | null           // 固定种子（每张 +i）；null = 随机
}
LabPlan = { idx, character, pack_entry_id: string|null, pack_title: string|null, pack_path: string[]|null,
            tags_raw: string, tags_used: string, dropped: string[], extra_tags: string|null,
            prompt: string, negative: string, artist: string, artist_source: "style"|"pool"|"none",
            size: string, seed: number|null, model: string, prose?: boolean }
LabImage = LabPlan & { status: "pending"|"done"|"error", file: string|null, error: string|null,
                       score: number|null, notes: string[], rating: "fav"|"dis"|null }
LabRun = { run_id, mode, request: LabRunRequest, status: "queued"|"running"|"done"|"error",
           created_at, finished_at: string|null, stamina_before: number|null, stamina_after: number|null,
           images: LabImage[], error: string|null, plugin_version }
LabRunSummary = { run_id, mode, status, created_at, count, done, text, pack_title: string|null, best_score: number|null }
```

`stamina_before` / `stamina_after` 是插件回传的生成预算读数，桌面端只展示。

### 9.2 插件端组装规则

1. **角色**：`text` 按 `/画` 指令同样的方式切分（`风格=xxx` 指定风格配方；首 token 为角色名或 `随机`；其余为描述）。角色无法解析且像名字时返回 400 并附可选角色列表。
2. **图包来源**：插件已导入的图包数据中 `decision=include` 的 `nai5_community_pack` 条目。指定 `entry_id` 则取它（不存在 → 400）；否则在 `scope` 内随机，`scope=[]` 时默认范围为 safe 图包。候选为空 → 400。
3. **图包标签**：用 §3.2 / §9.6 清洗后的 `pack.tags`，再过插件自身的条目过滤；`dropped` = 两步剔除的 token。
4. **自然语言描述**：未选图包时走 `/画` 现有路径（每张独立随机）；选了图包时，描述经插件的自由文本过滤后作为 `extra_tags`。
5. **多张不同**：`count>1` 时每张独立随机图包、独立掷风格池、独立 seed；指定 `entry_id` 时图包固定，风格配方与 seed 仍独立。
6. **负面**：插件默认负面提示词。
7. **风格配方**：`风格=` 指定时用它；否则以 `artist_ratio` 的概率从风格池随机取一条（`artist_source="pool"`），未命中为 `"none"`。
8. **拼装**：`prompt = 角色, 人数词, tags_used[, extra_tags], 默认质量词`；尺寸按标签选择；模型、steps、scale、sampler、noise_schedule 用插件出图默认值。
9. **出图**：逐张提交插件的出图队列，最多 2 张并发；单张失败只把该张标为 `status="error"`。`score=true` 时由插件的评分模型打分。
10. **记录**：插件端每次运行保存完整 `LabRun` 和一行 `LabRunSummary` 索引；评分（fav/dis）写回运行记录。同一时刻只允许一个实验任务（409）。

### 9.3 插件端路由（JWT 之后再核对 `X-Library-Token`，GET 也接受 `?token=`）

| 路由 | 说明 |
|---|---|
| `POST page/lab/run/preview` | body `LabRunRequest` → `{ plans: LabPlan[] }`，只组装不出图（LLM 路径会调 LLM） |
| `POST page/lab/run/start` | body `LabRunRequest` → `{ run_id, plans }`；运行中 → 409 |
| `GET page/lab/run/status?run_id=` | `LabRun` |
| `GET page/lab/run/list?limit=` | `LabRunSummary[]`，新的在前 |
| `GET page/lab/run/image?run_id=&idx=&full=0\|1` | `{ name, mime, data_base64 }` |
| `POST page/lab/run/rate` | `{ run_id, idx, rating: "fav"\|"dis"\|null }` → `{ ok }` |

### 9.4 桌面端

- 命令：`lab_run_preview(req)`、`lab_run_start(req)`、`lab_run_status(run_id)`、`lab_run_list(limit)`、`lab_run_image(run_id, idx, full)`（首次从插件拉取并落盘 `labruns/<run_id>/<idx>[.thumb].<ext>`，后缀按 mime 决定，之后直接返回本地）、`lab_run_rate(run_id, idx, rating)`（先写插件，成功后写本地）。插件不可达 → `AppError network`。
- 本地表 `lab_run(run_id PK, mode, created_at, status, json)`：`lab_run_status` 返回 `done|error` 时落盘一份；`lab_run_list` 合并本地与插件（本地优先，按 created_at 倒序）。插件不可达时 `lab_run_status` 不做本地回退。

### 9.6 图包标签的来源与角色 token

- 社区图包里有相当一部分条目把姿势与服装写在 `character_prompts[].prompt` 而不是 `tags`（开发时的上游数据中约 1168/1903 条）。因此图包标签 = `tags` 与全部 `character_prompts[].prompt` 依次拼接（主提示词在前，各角色提示词按顺序在后，用 `, ` 连接），再做 §3.2 的清洗并剔除角色 token。`pack_pool.json` 与 `full_pool.json` 的 `pack.tags` 都按此生成。
- 角色 token 的判定（两端一致）：任何位置的 `\(.+\)$` 括号型与角色别名表（`character_aliases`，合成示例）命中；label 非占位（非 `char1` / `角色1` 这类）时其首个非通用 token；每个角色提示词的首个非通用 token **仅当**它不在「主标签词汇表」里时才算角色名（词汇表 = 同词典全部条目主 `tags` 的归一 token 集合，任意决策；括号型与散文条目不入表）。这样以 `cowboy shot` 开头的无名角色提示词不会被误删，而不带括号后缀的角色名会被剔掉。

## 10. 反推模型的两种来源与拖图进题库（v0.2.3）

### 10.1 模型的两种来源

- `reverse_provider` 表加列（migration 004）：`kind TEXT NOT NULL DEFAULT 'openai'`（`openai` | `plugin`）、`plugin_provider_id TEXT NULL`。`kind='plugin'` 时 `base_url` / `model` 存空串，`has_key` 恒 true（不需要 key），`name` 默认为 provider id。
- DTO：`ReverseProvider` 与 `ReverseProviderInput` 追加 `kind: "openai" | "plugin"`、`plugin_provider_id: string | null`（`#[serde(default)]`，不传即 `openai`）。
- 命令 `plugin_providers() -> string[]`：GET `page/providers`（JWT，无需 token）返回插件已注册的 provider id 列表；不可达 → `AppError network`。
- 批次运行（`reverse/batch.rs`）：`kind='plugin'` 的端点走 `POST page/lab/reverse`，body `{ "provider_id": string, "prompt": string, "image_base64": string, "mime": string, "max_tokens": int }`，头同 §8.3。响应 `{ "text": string, "latency_ms": int, "provider_id": string }`；`text` 为模型原文，解析、清洗、指标仍在桌面端做。图片走同一套缩放（≤ 9 MB）。`request_json` 里 `model` 填 `plugin:<provider_id>`。整个批次复用一次登录；取消令牌同样生效。
- 界面：评测页的模型管理只暴露 OpenAI 兼容直连端点（`base_url`、`model`、钥匙串 key）；`kind='plugin'` 分支在后端保留，界面不再提供入口。

### 10.2 插件端路由 `POST page/lab/reverse`

- 校验 token（同 §8.3）；`provider_id` 必须是插件已注册的 provider，否则 400；`image_base64` 解码后 ≤ 12 MB，否则 413。
- 图片写到插件临时目录，调用该 provider 的图文对话，超时 120 s，完成后删除临时文件（异常也删）。返回模型原文；不做拒答判定、不做解析（由桌面端完成）。

### 10.3 拖图进题库

- `QuestionBank.vue` 监听 Tauri webview 的 drag-drop 事件（`getCurrentWebview().onDragDropEvent`，挂载时订阅、卸载时退订）：`over` 时题库面板高亮，`drop` 时把 `paths` 里扩展名为 png / jpg / jpeg / webp / gif（大小写不敏感）的文件逐个走「添加本地图片」（重复路径忽略），其余忽略并提示「只接受图片文件」。`leave` / `cancel` 取消高亮。
- 本地图片先拷进数据目录 `local_images/`，同一文件重复导入得到同一路径。拖放只在反推评测页生效；其它页面丢文件不做任何事。

## 11. 散文提示词与反推重画对照（v0.2.4）

### 11.1 散文判定（两端同一语义；Rust `artist::is_prose_prompt`，插件端同名函数）

- `strip_artist_block(t)`：删除 `<artist>…</artist>`（大小写不敏感、跨行）。
- `is_prose_prompt(t)`：对 strip 后文本，满足任一即 true：含 `<artwork>`；含 "```"；存在独立成行的 XML 标签 `^\s*<[a-z_]+>\s*$`（`<style>` / `</style>` 除外）；按 `[,\n]` 切分后非空 token 的平均词数 > 5。
- `<style>…</style>` 的内容按标签处理（去掉标签本身后参与切分）。在当时的上游数据上实测：1898 条里 159 条散文（126 条 XML 式、33 条长句），1739 条标签；另有 79 条是「`<style>` 块 + 逗号标签」，按标签处理。

### 11.2 导出与完整池

- 新增 `pack.prose: bool`、`FullEntry.prose: bool`、`pack_pool.entries[].prose`（仅 true 时写）。
- 散文条目：`pack.tags` = `strip_artist_block(主 tags)` 后的原文（保留 `<style>` 块与 XML 块，去首尾空白），非空时空一行再接各 `character_prompts[].prompt` 原文；不切分、不清洗、不剔角色；`t` = title + " · 散文"；不计 skipped；`artists` / `core` 仍按 `<artist>` 块解析。
- §9.6 的主标签词汇表只从非散文条目建。

### 11.3 插件端

- 旧版出图实验（§9）：随机候选排除散文；`entry_id` 指定散文条目时 `tags_used`=原文、`dropped=[]`、`LabPlan.prose=true`；仍加角色与质量后缀。
- 重画路由 `POST page/lab/reverse/render`（token 同 §8.3）：

```
body = { tags: string, framing: string|null, character: string (缺省为插件默认角色), seed: int|null,
         negative_extra: string|null, judge: bool, original_base64: string|null, original_mime: string|null,
         judge_provider_id: string|null }
resp = { file, seed, prompt, negative, size, latency_ms,
         quality_score: number|null, quality_notes: string[], similarity_score: number|null, similarity_reason: string|null }
```

  - prompt = `character, 人数词, [framing], tags, 默认质量词`；negative = 插件默认负面词（+ `negative_extra`）；**不注入风格配方**（§12.5 起由 `artist` / `artist_mode` 控制）；`seed` 缺省随机并回传；单张同步生成；总超时 240 s。
  - `judge=true`：`quality_score` / `quality_notes` 由插件的评分模型给出；给了 `original_base64` 再跑成对评审：provider = `judge_provider_id`，为空时用插件配置的默认评分模型；同时输入原图与重画图，输出 `{"score": 0-10, "reason": "<20 字"}`。成对提示词要求：A 为原图、B 为按反推标签重画；**忽略角色外貌差异**（发色、瞳色、体型、脸），只比较构图与镜头、姿势与手势、服装与配饰、道具、场景与背景、光线与氛围；只输出 JSON。评审失败 → 对应字段 null，不影响出图结果。
- 桌面端取图用 `GET page/thumb?name=&full=1`。

### 11.4 桌面端

- migration 005：`reverse_run` 加列 `truth_kind TEXT NOT NULL DEFAULT 'tags'`（`tags|prose|none`）、`render_file TEXT`、`render_local TEXT`、`render_seed INTEGER`、`render_prompt TEXT`、`quality_score REAL`、`quality_notes TEXT`（JSON 数组）、`similarity_score REAL`、`similarity_reason TEXT`、`render_error TEXT`、`leak_appearance INTEGER`、`leak_banned INTEGER`。
- 指标：题目 truth 来源为散文 → `truth_kind='prose'`、`metric=null`，但 `appearance_leak` / `banned_leak` 仍按 `pred_raw` 计算并放进 `RunRow` 顶层的 `leak_appearance` / `leak_banned`（任何 truth_kind 都填）；本地图片题 `truth_kind='none'`。
- `RunBatchRequest` 加 `render: boolean`（缺省 false）、`render_character: string|null`、`render_seed: number|null`、`judge_provider_id: string|null`。每个 run 解析成功后若 `render=true` → 调 §11.3 路由（`tags` = cleaned 用 ", " 连接；`framing` = parsed.framing；原图 = 题目图片缩放后的 base64）→ 结果写入 run，图片拉到 `labruns/reverse/<run_id>.jpg`；失败只写 `render_error`。
- 命令 `reverse_render_run(run_id, judge_provider_id: string|null, …) -> RunRow`（单条重画，覆盖旧结果；后续参数见 §12.5–§12.7）。`RunRow` 新增 `truth_kind, render_path, render_seed, render_prompt, quality_score, quality_notes, similarity_score, similarity_reason, render_error, leak_appearance, leak_banned`；`render_path` 为绝对路径。
- `BatchSummary` 的每模型行：P/R/F1 只聚合 `truth_kind='tags'` 且 metric 非空的 run，新增 `tags_n`（参与 F1 的题数）、`render_n`、`quality_avg`、`similarity_avg`。
- `Question` DTO 新增 `prose: bool`（后端按 truth 来源判定，与 `truth_kind` 同一规则）。
- 界面：散文条目卡片标「散文」，可筛「只看标签题」；运行详情原图与重画图并排、两个分数与理由、「重画」按钮；运行配置可勾选「反推后重画并评分（消耗生成预算）」，填角色、固定种子、选择成对评审模型（`plugin_providers()`，可空 = 插件默认）。

## 12. 画风索引与检索（v0.3.0 起）

### 12.0 约定（含 v0.4.0 修订）

- **检索而不是猜测**：给查询图找风格配方 = 检索，不让视觉模型猜画师名。为风格词典（`artist_nai5_personal`）全部条目的展示图各算一个向量存库，覆盖加入 / 不加入 / 未定；查询时只给查询图算一个向量，与库里的比余弦相似度取前 K。排除名单命中的**永远**不返回；其余按相似度降序，每条带 `decision`，界面把「不加入」标灰但不隐藏。社区图包不参与画风索引与检索（早期版本的图包向量留在表里但不使用）。
- **完整配方**：风格配方使用条目原始 `tags`，只去首尾空白，保留所有 artist、质量词、线条 / 配色词、年份、负权重、顺序与内部格式，不拼接固定质量后缀。`StyleHit.style_prompt: string` 必需；复制、指定配方重画、精准反推、筛选风格池导出都使用它，`core` / `pack_tags` 只为兼容保留。导出去重按完整串，不按画师 signature 合并，避免误并配色 / 线条不同的配方。
- **模型注册表**（`style/catalog.rs`）：`clip-vit-base-patch32`（512 维）、`megastyle-siglip-so400m`（1152 维）、`csd-vit-large-patch14`（768 维）。权重目录、索引与均值按模型 ID 隔离，更换模型不清除其他模型的数据。模型 ID 必须来自注册表，拒绝任意路径 / URL。各模型使用原训练架构及预处理，输出 L2 归一化向量。
- **模型选择**：`style_model_status(model?)`、`style_model_download(model?, on_event)`、`style_index_rebuild(model?, force, on_event)` 可指定模型，缺省为当前模型；后台任务开始时固定模型。`style_models_list() -> StyleModelStatus[]`、`style_model_select(model) -> StyleModelStatus`，选择持久化在 `app_state('style_model')`（缺省 `clip-vit-base-patch32`）。下载 / 同步 / 索引期间拒绝切换；同一时刻一个模型下载、一个索引任务。
- `StyleModelStatus` 含 `label`、`description`、`dim`、`download_size_bytes`、`default_center`。`StyleSearchRequest` 可选 `model`（缺省当前）；`StyleSearchResult.model` 标明实际模型。同图对比按相同原图和筛选条件依次查询已下载且已索引的模型，各组独立显示结果 / 错误 / 耗时，分数不做跨模型排名。
- **减库均值**：示例图共用相近的角色与构图，库均值里主要是内容成分；`center=true` 时查询与库向量都先减去该模型的库均值再算余弦。均值缓存在 `app_state`（按模型），索引变化时重算。它只是检索选项，不保证去除内容；每个模型有自己的 `default_center`（CLIP 缺省开，CSD / MegaStyle 缺省关）。

### 12.1 存储（migration 006）

```
CREATE TABLE image_embedding (
  entry_id TEXT NOT NULL, ordinal INTEGER NOT NULL, model TEXT NOT NULL, dim INTEGER NOT NULL,
  vec BLOB NOT NULL,            -- f32 小端，dim 个
  image_sha256 TEXT,            -- 算向量时的图片哈希；图片换了要重算
  computed_at TEXT NOT NULL,
  PRIMARY KEY (entry_id, ordinal, model)
);
```

- 只对 `ordinal = 0` 的展示图建索引（`entry_image.local_path` 非空者）。`removed_at` 非空的条目跳过。

### 12.2 建索引与检索

- 同步流程在图片阶段之后增加 `embedding` 阶段（`SyncPhase::Embedding`；`SyncEvent` 带 `embed_total` / `embed_done` / `embed_failed`）：对当前模型缺向量或 `image_sha256` 与 `entry_image.sha256` 不一致的展示图分批计算；单张失败记 warn `app_event` 并继续；可用 `sync_cancel` 取消。模型未下载 → 整个阶段跳过，发一条「画风模型未下载，跳过索引」事件，不算错误。
- `style_model_status(model?) -> StyleModelStatus { model, dir, present, size_bytes, indexed, total, mean_ready, … }`。
- `style_model_download(model?, on_event: Channel<DownloadEvent>) -> StyleModelStatus`：按注册表逐个下载固定 revision 的文件，例如 `https://huggingface.co/openai/clip-vit-base-patch32/resolve/3d74acf9a28c67741b2f4f2ea7635f0aaf6f0268/pytorch_model.bin`，完整清单与字节数、SHA-256 见 `style/catalog.rs`。文件先下到 `<file>.download.part`（`Range` 续传；上游回 200 就从头重写），长度与 SHA-256 校验通过才改名为正式文件。`DownloadEvent { file, done_bytes, total_bytes, finished, error }`。`style_model_download_cancel()` 取消并保留 `.part` 供下次续传。
- CLIP 权重格式：默认下载官方 `pytorch_model.bin`，用 candle `VarBuilder::from_pth` 读取，无需 Python / PyTorch；已有非空 `model.safetensors` 也能直接加载且不重复下载，两者都有时优先 `.bin`。状态要求非空 `config.json` 和至少一种非空权重文件，`.part` 不算完成。
- `style_index_rebuild(model?, force, on_event: Channel<SyncEvent>)`：不经同步单独建索引；`force=true` 删掉该模型的全部向量重算。
- `style_search(req: StyleSearchRequest) -> StyleSearchResult`：

```
StyleSearchRequest = { source: { kind: "entry", entry_id, ordinal } | { kind: "path", path }, top_k: number (1..50，缺省 10),
                       codex_ids: string[] (空 = 全部), decisions: ("include"|"exclude"|"undecided")[] (空 = 全部),
                       center: boolean, model?: string }
StyleHit = { entry_id, codex_id, title, path: string[], decision, blacklisted: false, score: number (-1..1),
             style_prompt: string, core: string|null, pack_tags: string|null, thumb_path: string|null, prose: boolean }
StyleSearchResult = { model, indexed: int, query_ms: int, hits: StyleHit[] }
```

  - `core` 按 §3.1 序列化（不带质量后缀）。模型未下载 / 索引为空 → `AppError bad_request`，message 说明去设置页下载或重建。

#### 索引时间估算（v0.3.3，前端，无 DTO 变更）

- 设置页手动建索引和同步页自动进入索引阶段时，显示「已用时间」「预计剩余约…」「…张/分钟」。只计算本轮 `embed_total` 的工作量；已处理数 = `embed_done + embed_failed`。
- 首个完成批次作为速度采样基准，收到第二个有进展的批次后再显示 ETA，避免把模型首次加载时间算进速度；速度用最近最多 6 个批间区间估计。样本不足显示「正在估算…」；长时间无进展（≥ 60 秒且超过最近平均批时的 3 倍）隐藏过期 ETA；计数完成但终态未到时显示「正在收尾…」。完成、取消、错误后停止计时；计时状态在本次会话内跨页面保留。

#### 推理设备（v0.3.4）

- `Settings.style_device: "auto" | "metal" | "cpu"`，缺省 `auto`。同步 / 索引 / 模型下载运行时拒绝切换（`bad_request`），其他设置仍可保存。非 macOS 构建拒绝显式 `metal`。
- macOS 构建启用 candle Metal；`auto` 优先尝试 Metal，初始化或模型预热失败则退回 CPU 并显示原因；显式 `metal` 失败直接报错；`cpu` 始终使用 CPU。模型缓存同时匹配目录和设备偏好，切换时释放缓存，下次加载生效。
- `StyleModelStatus` 含 `device_preference`、`active_device: "cpu" | "metal" | null`（尚未加载时为 null）、`metal_available`（当前构建是否含 Metal 支持，不保证驱动初始化成功）、`device_message`（回退原因或平台不支持的说明）。
- 设备选择同时用于建索引与查询图编码，模型、F32、预处理与向量格式不变；CPU / Metal 只有浮点误差，切换不清空既有索引。

### 12.3 预处理与推理

- CLIP：短边缩放到 224、中心裁 224×224、RGB、CLIP mean/std 归一；取图像塔 pooled 输出经投影得 512 维，L2 归一。CSD、MegaStyle 的预处理见 `docs/STYLE_BENCHMARK.md`。批大小由注册表决定（CLIP 16、CSD 4、MegaStyle 2）。推理在 `spawn_blocking` 里跑，不阻塞异步运行时。
- 单元测试用随机权重实例化模型验证形状与流水线，不下载权重；设了 `CLIP_MODEL_DIR` 时额外跑真权重的健全性测试。CSD / MegaStyle 与官方参考实现的对拍测试需要 `CSD_MODEL_DIR` / `MEGASTYLE_MODEL_DIR`，默认 ignored。

### 12.4 完整池里的向量

- 索引存在时 `embedding = { "model": string, "dim": int, "centered": false, "mean": base64(float16) }`，每条 `FullEntry` 追加 `vec`：`base64(float16 小端 × dim)` 的**原始**向量（未减均值）；没有向量的条目 `vec: null`。

### 12.5 反推评测页的画风面板

- 画风面板（`components/reverse/StylePanel.vue`）：来源 = 题库当前选中的题，或本地图片；参数 top_k、词典范围、决策范围、是否减库均值、模型。结果列表：缩略图、标题与目录、相似度条、决策徽章、完整风格配方（一键复制）。
- 与重画打通：每条命中可以「按此配方重画」= `artist_mode="given"` + `artist=<style_prompt>`。`RunBatchRequest` / `reverse_render_run` 增加 `artist: string|null`，插件重画路由增加 `artist: string|null`；运行详情记录 `render_artist`（migration 006）。「反推标签 + 检索到的风格配方」重画出的图和原图对照，就是检索准不准的直接证据。
- 设置页「画风模型」卡片：模型名、目录、是否已下载、大小、已索引 / 应索引条数、下载 / 重建索引 / 取消按钮与进度条。

### 12.6 重画与插件 `/反推` 逐条对齐

实验室重画如果和插件 `/反推` 指令的参数不一致，对照就失去意义。对照表（线上 = 插件 `/反推` 指令）：

| 项 | 线上 `/反推` | 重画路由 |
|---|---|---|
| 尺寸 | 按原图宽高比：≥ 1.15 横版、≤ 0.87 竖版、否则方形 | **同一函数**，按请求里的 `original_width` / `original_height` 选择；两者缺失才按标签选尺寸，并在响应 `size_source="tags"` 标明 |
| 风格配方 | `风格=` 指定，否则按插件配置的概率从风格池抽一条，未命中为空 | `artist_mode: "pool" \| "given" \| "none"`，缺省 `"pool"`（同一函数同一概率）；`"given"` 用 `artist`；`"none"` 是不带风格配方的纯标签对照。响应回传 `artist`、`artist_source` |
| 张数与择优 | 按插件配置出若干张（默认 2，上限 4），评分后只发最高分那张 | `count: int\|null`（null = 插件配置的同一默认，上限 4）；全部出图、全部评分，响应 `images: [{file, seed, quality_score, quality_notes}]` + `best_idx`；顶层 `file/seed/quality_*` = 最优那张；成对评审只对最优那张跑 |
| framing | 不在五个取值里 → `cowboy shot` | 同样回退 |
| 标签清洗 | 插件清洗后不足 5 条报错 | 提示词用桌面端传来的 `tags`（规则集是实验对象，不能被线上规则覆盖），但路由再跑一遍插件清洗并回传 `plugin_dropped: string[]`；线上口径下不足 5 条时**不报错**，回传 `plugin_would_reject: true` |
| 拼装 | `角色, 人数词, framing, tags…, 质量词` | 相同 |
| 负面 / steps / scale / sampler / noise_schedule | 插件默认值 | 相同（`negative_extra` 仍可追加） |
| 模型 | 由插件的生成后端决定 | 插件默认模型（差异保留并注明） |
| seed | 随机 | 可固定（实验需要）；`count>1` 时每张 seed+i |

- 桌面端：图片预处理额外返回**缩放前**的原图宽高；重画请求体新增 `original_width`、`original_height`、`artist_mode`、`count`；`RunBatchRequest` 新增 `render_artist_mode`（缺省 `"pool"`）与 `render_count`（null = 插件默认）；`RunRow` 新增 `render_artist_source`、`render_size`、`render_size_source`、`plugin_dropped`（JSON 数组）、`plugin_would_reject`、`render_candidates`（JSON：全部候选的 file / seed / score）；migration 006 一并加列。候选图另拉到 `labruns/reverse/<run_id>_<idx>.jpg`。运行详情显示尺寸来源、风格配方来源、线上会多剔的词、候选缩略图与分数。

### 12.7 精准反推（v0.3.1）

- 定义：反推标签之外，**自动**用画风索引检索原图最相近的一条风格配方，把它加进重画。它是 §12.6 `artist_mode` 的第四个取值 `"style"`，桌面端把它翻译成对插件的 `artist_mode="given"` + `artist=<命中配方>`；插件不需要改动。
- `RunBatchRequest` / `reverse_render_run` 新增：`style_top_k: number`（缺省 5，检索候选数，取过滤后第一条可用的）、`style_decisions: ("include"|"exclude"|"undecided")[]`（空 = 全部；排除名单永不使用）、`style_min_score: number|null`（相似度低于它时按 `style_fallback` 处理）、`style_fallback: "pool"|"none"`（缺省 `"pool"`）、`style_center: boolean`、`style_model: string|null`（null 采用启动批次时的当前模型，随后固定并记录）。
- 流程（`reverse/render.rs`）：run 解析成功 → `style::search`（词典题用 entry/ordinal，本地题用 path）→ 过滤后取第一条的完整 `style_prompt` → 无命中或 `score < style_min_score` → 按兜底处理 → 调重画。检索出错只写 `render_error`，不调用插件。
- 记录：migration 007 给 `reverse_run` 加 `render_style_entry_id TEXT`、`render_style_title TEXT`、`render_style_score REAL`、`render_style_decision TEXT`、`render_style_fallback INTEGER`；`RunRow` 同名字段；`render_artist_source` 取值新增 `"style"`。模型未下载或索引为空 → 批次开始前就报 `bad_request`，不插批次行，不消耗生成预算。
- 界面：运行配置的「风格配方来源」下拉新增「精准反推（画风检索）」，展开时显示 top_k、决策范围、最低相似度、兜底方式；结果表每模型行新增 `style_hit_n`（用到检索配方的张数）与 `style_score_avg`；运行详情显示命中条目（缩略图、标题、相似度、决策）。
- 意义：成对评审的「像原图」分在 `style` 模式与 `pool` 模式之间的差，就是检索对最终效果的贡献。

### 12.8 筛选池导入与重画参数记录（v0.4.0）

插件导入 `page/library/import` 的 `nai5_artist_pool` 接受非空字符串数组（每项是完整风格配方，可以不含 `artist:` 标签），不修改内部词条或权重；其余鉴权、排除名单与原子落盘规则沿用。

每次重画的实际参数另记在 `reverse_run.request_json.last_render_request`（包括本次固定的 `style_model`，失败的尝试也记录），原 `request_json.render` 保留批次开始时的参数。重画完成事件包含 `style_model`，不用开始时的模型去解释后来换模型的结果。

### 12.9 反推批次停止运行（v0.4.1）

- `reverse_cancel_batch(batch_id: string) -> ()` 请求停止当前正在执行且 ID 相同的反推批次；重复请求、已结束或其他 ID 均幂等返回，不取消其他批次。`reverse_run_batch` 从启动准备开始只允许一个活动批次，重叠启动返回 `batch_running`；失败或结束后释放活动状态。
- 停止后取消客户端待执行 / 等待中的反推与重画工作，不再提交后续题目或重画请求；已落库的结果保留。数据库批次终态为 `cancelled`，推送 `batch_cancelled` 事件，`done/total` 保留实际进度。计数表示已处理的反推结果，不表示所有关联重画都已完成。中断不伪装成全部成功或全部失败，也不为尚未完成的模型请求制造失败结果。
- 已经发送到插件 / 模型服务的请求可能仍在远端完成并产生费用；停止只终止桌面端的批次调度，不承诺撤回远端已接收的工作。
- 反推评测页运行区常驻「停止运行」按钮：启动尚未拿到 ID 时显示启动状态，请求停止后显示「停止中…」并防重复操作，终态到达后才允许新批次。活动批次 ID 与当前查看的历史批次 ID 分开；查看历史不改变停止目标。

## 14. 桌面端 v2 对接与界面重构（v0.5.0）

- 不改变 §3 / §8 的导出、三态、评测清洗、张数或模型默认值，不引入 schema 变更；新增状态都是旁路查询。
- 插件传输层按地址、用户名和密码在进程内复用 JWT；并发的首次登录合并，401 只刷新一次；登录失败后短暂冷却，防止页面切换触发连续登录。密码变化使用新身份；JWT / 密码不落库。保留 §6 的登录顺序和钥匙串进程内读缓存。
- 原图展示：`reverse_source_image(run_id) -> {path:string|null, kind:"original"|"display"|"thumbnail"|"missing"}`。只读本地文件：词典题依次取 `original_local_path`、`local_path`、`thumb_path`；本地题取拷入应用的图片。缩略图回退必须标明，缺文件明确占位；不自动下载，不改变模型输入。
- 插件桥：无参命令 `plugin_model_selection() -> any`，只 GET 插件 `page/model-selection`（§17 常用 LLM 名单）并原样返回插件 `data`；前端 `api.pluginModelSelection()`。该接口只需 JWT，不要求 `X-Library-Token`。非 2xx 与 `{status:"error"}` 均抛出 AppError。它不是任意 URL 代理。
- 视觉：Naive UI + 单一 CSS 令牌来源，跟随系统外观。费用标识分为计费路径、生成预算、不额外计费、本机计算不计费和额外模型 API 调用；未知费用不标为不计费。数字等宽，宽内容局部滚动，加载 / 空 / 错误各自独立显示。

## 15. 精选图包创作实验（v0.5，2026-09-10）

本节定义创作实验的新任务流程，接口为插件的 `page/lab/creative/*`；旧版出图历史（§9）保留可读，反推不变。

### 15.2 桌面与共用创作协议

- **参考来源**：Rust 从本机 SQLite 读取 `nai5_community_pack` / `local_pack` 中 include、safe、未移除且不命中排除名单的条目，保留主提示词、负面词和角色子提示词。选择的 ID / 目录只能缩小这个范围；空池明确报错，不回退其他决策或分级。散文可作为语义参考，不能当作随机池配方直接出图。
- **构思**：原始要求原样保存；指定角色保持其身份。角色留空（含纯空白）时，桌面端在每次新构思前读取插件最新 `capabilities.characters`，从有效角色中随机选一个作为明确角色传给插件；不让 LLM 自选角色，也不从参考图包继承人物。空池、无有效角色或读取失败时停止，不调用构思模型。LLM 同时读参考快照、固定版本的 skill 和用户要求，在一次结构化调用中提炼构图手法、设计新事件并写出 NAI 提示词。参考文本是数据，不能覆盖任务指令；硬要求优先于参考、skill 与随机构思。不承诺扩散模型每张都满足全部要求。
- **skill**：内置 skill id `nai5-prompting`，展示名「内置写法指南」，正文为本仓库原创的 `resources/prompting-guide/GUIDE.md`（MIT，随应用代码），版本 `SKILL_VERSION = "builtin-1"`，随方案记录版本与摘要。运行时不联网拉取。它不替代本应用的角色解析、完整风格配方或生成参数约束。
- **方案冻结**：`creative_plan` 返回不可变 `plan_id`，记录原始输入、参考 ID / 哈希 / 快照、skill 与模型版本、构思、提示词 / 角色字段、完整风格配方、生成参数、LLM 原文及独立的要求检查。角色留空时同时保留原始空输入与实际选角，`plan.character` 必须与选角一致并在界面显示。已冻结方案的编辑、执行和同构思再抽都保持这一角色；只有新构思才重新选角。`prompt` 已包含实际风格前缀，执行时不再拼一次；风格配方在构思时抽一次，执行时不重抽。编辑通过 `creative_edit` 复核后生成新 `plan_id`，原方案不覆盖；复核有额外 LLM 调用。
- **执行**：`creative_start({plan_id,request_id,count,seed,score})` 只执行服务端已保存的方案，拒收任意 prompt / 尺寸 / 计费参数。`request_id` 为客户端 UUID，重试同一请求返回同一 run，不重复出图；同 ID 不同内容拒绝。张数 1–8，逐张请求 `n_samples=1`。同一构思再抽一张只变 seed；换构思才重新调用提示词模型。开始前重新检查当前本地决策，已删除 / 排除 / 命中排除名单的引用不能绕过筛选；原文正常更新不替换旧方案快照。
- **生成参数与预算闸门**：只允许 `832x1216`、`1216x832`、`1024x1024` 三个尺寸，沿用插件配置的 NovelAI V5 模型与 1–28 steps，即无额外计费的参数档位（3 个尺寸、≤ 28 步）；配置不合格就显示原因，不静默改模型或步数。不走加强、高清或计费重试路径，不自动升级到计费路径。生成后端由外部服务提供，本应用只做预算闸门：逐张预算检查，每张执行前后各读一次余额，差值非 0 或读取失败就停止余下任务并记录原因；不依赖旧缓存。构思、编辑、评分的 LLM API 调用单独标识。
- **费用事实**：生成服务没有已核实的「本次请求一定不计费」的原子保证，应用不虚构这种保证。参数档位限制和逐张预算检查是应用侧保护，约束不了使用同一生成后端的其他客户端或同一插件进程里的其他出图请求，也不对这些并发造成的余额变化做归因。实际花费字段未核实时为 null，不能预填 0；界面显示规则与实际读取结果，不把未知费用当作零。
- **停止和恢复**：cancel 幂等停止后续张数，正在远端执行的一张可能完成并记录；不承诺撤回已发送请求。排队、执行、部分结果、失败、取消分开保存；查看历史不改变活动任务的取消目标。记录原子落盘到应用独立目录，不涉及 schema 变更、不写筛选决策。
- **接口**：协议版本 1，桌面 `creative_{capabilities,references,plan,edit,plans_list,plan_references,start,status,list,cancel,image}`。插件 `page/lab/creative/{capabilities,plan,edit,start,status,list,cancel,image}`，JWT 后校验 `X-Library-Token`。返回 `{status,message,data}`；计划与运行 DTO 见 `src/types-creative.ts`，字段 snake_case。capabilities 分 `available/reason`（构思）与 `render_available/render_reason`（生成）；旧插件返回 404 时提示更新并重载插件。图片以 base64 传回，Rust 校验后缓存为本地绝对路径。列表严格校验 Array，错误 / 未读 / 空记录分开。
- **图片准备状态**：插件运行状态里的图片 `done` 可能早于取图记录落盘。取图接口返回业务 404「这张图片尚未生成」时，桌面映射为 `creative_image_pending`，显示「图片准备中」并只重试取图（约 2 秒一次，每张去重）；跟踪中的任务仍在生成或评分时持续显示准备状态，结束后至多再等 60 秒，未跟踪的历史记录同样 60 秒，超时后给出重试入口。读取图片不阻塞状态刷新或取消。记录 / 文件确实不存在、鉴权、网络和响应损坏等错误独立显示。离开界面清理计时器；不重发构思或出图请求。
- **验证**：只用合成图包、假 LLM 与假出图，不调用真实计费服务。覆盖冻结后执行零新增构思调用、参数不重组、取消保留已完成、错误不伪装为空、三尺寸 / 单张 / 步数 / 预算闸门生效。

### 15.3 全量参考图包

- 创作请求 `reference_mode: "sample" | "all"`，省略时为 `sample`；桌面每次请求明确发送自己的模式，不隐式修改插件配置。`sample` 沿用抽样规则；`all` 取消抽样条数限制，使用当前 `scope` 内全部合格图包（include、safe、未移除、未命中排除名单；风格词典不参与）。关键词只过滤浏览列表，重点勾选不是全量模式的范围限制，界面说明范围由目录选择决定。
- 所有参考保留完整主提示词、负面词、角色子提示词、来源 ID / 哈希，不截断、不静默减条数；按 §21 不设应用层总字节预算。模型上下文拒绝或超时不暗中退回抽样。全量构思 / 编辑的应用模型等待上限 300 秒、桌面 HTTP 等待 360 秒，provider 自身更短的超时仍可能生效。
- `capabilities.full_reference_supported` 为可选布尔值，缺省 false；能力不支持时桌面拒绝开始全量模式。新版客户端对所有创作 HTTP 请求附加 `features=full_references`；未协商时插件不返回新增字段，以兼容旧的严格 DTO；旧客户端访问全量方案时提示更新，编辑 / 开始等在副作用之前拒绝。冻结方案新增 `reference_mode`（旧历史缺省 sample）与 `used_reference_ids`（旧历史缺省未知）。`reference_ids` 记录全部提供给模型的材料，`used_reference_ids` 只记录模型声明借鉴的有效子集，不声称模型逐条阅读过；界面分别显示两种数量。
- 全量调用在独立的 provider 浅副本上阻止自动裁剪上下文；共享 provider 与 AstrBot 核心不改。上下文超限须保留材料并报错，不删空用户输入后重试。
- 全量只扩大构思材料，模型、角色、硬要求、生成参数档位与预算检查不变。界面提示全量会增加 LLM token 和等待时间，不把模型 API 费用标为不额外计费。

## 16. 总实验室（v0.5.5，2026-09-19）

导航合并为「总实验室」，包含反推、/画、画风复审三种模式；旧路径重定向到对应模式，旧批次和图片继续可读，不改 schema、不覆盖插件默认配置。实验参数逐请求冻结，禁止临时修改全局配置或共享 provider。§9 / §12 / §15 的协议保持兼容；新协议独立使用 `page/lab/studio/*`，统一 JWT + `X-Library-Token`，版本 `1`。反推评测页（`/eval`）是独立页面，沿用 §4 / §10–§12 的批次协议。

- **对照单位**：一个试验 = 一个提示词模型 × 一个有序 skill 组合 × 相同输入快照。多模型不走候补链，失败单独显示。组合可为空（无 skill 对照），可选单个或多个；记录版本、全文 SHA-256 和实际发送文本。skill 材料只作为模型参考，不执行其中的命令，也不能覆盖硬要求、输出协议或生成参数约束。来源：内置 `nai5-prompting`（「内置写法指南」，§15.2）随应用固定；导入本机 Markdown 目录（导入前预览内容与大小）；跟随插件当前所选版本（§25）。导入与历史写应用私有 JSON 文件，不碰决策库。
- **创作参考范围**：/画只使用 safe 图包。决策范围可选 include 或 all（include / exclude / undecided），不改变「加入」语义或插件端的池。未移除、未命中排除名单仍是必需条件；风格词典不作为构图图包。全量模式发送当前范围所有合格图包的完整正文，按 §21 不设应用字节预算，上下文不够时报错，不暗中抽样；抽样保留明确的数量和来源记录。冻结方案出图前复查所选范围资格；all 模式不能误用 include 规则否决 exclude / undecided。
- **创作与反推**：draw 的 count=1..8 个不同构思；同一硬要求和角色贯穿各图，重复方案或数量错误须报错。角色留空时从插件有效角色池选择并冻结，多角色为显式有序列表。创作简报 / 多人简报、评分模型均可按实验覆盖。reverse 提供普通与分别反推模板、原图、评分模型列表、画风复审模型和候选张数；沿用现有识图 / 清洗 / 画风检索算法，实际参数与插件 `/反推` 指令的差异逐项展示。反推的 count 是同一反推方案的候选图，不伪称多个独立构思。独立的画风复审实验与反推中的候选重排是两种用途。
- **生成与费用**：新总实验室全部使用 §15 的无额外计费参数档位（3 个尺寸、≤ 28 步）、V5 与逐张预算检查，不进入加强或计费路径；这是相对旧 §12.6 可配置尺寸的明确差异，旧历史保留。构思、识图、评分、视觉复审可能产生额外模型 API 费用；本地向量推理本机计算，不计费。费用未知保持 null。构思与出图分开操作，start 只使用冻结方案并具有 `request_id` 幂等性；网络状态不明时重试相同 `request_id`，不重复构思 / 出图。停止只阻止未提交的工作，不承诺撤回远端在途请求。
- **JPEG**：可选择原图 PNG 或 JPEG 衍生副本，JPEG 默认质量 88、4:4:4，质量可调；保留原始 PNG，显示两份实际字节数。它是有损存储 / 导出选项，不改变尺寸 / 提示词 / 模型，也不是 NAI 请求参数。
- **插件协议**：GET capabilities 返回 `{version,providers,characters,free_sizes,model,steps,available,defaults,templates}`；defaults 提供 draw / reverse 的模型和评分 / 复审默认值，templates 提供 reverse / reverse_separate / draw_brief / multi_brief。POST plan 接受 `{mode,provider_id,character,characters,requirements,count,orientation,reference_mode,reference_decisions,references,style_name,artist_ratio,skill_bundles,brief,multi_brief,reverse,score_provider_ids,jpeg}`；references 为本地原文快照，skill_bundles 为 `{id,version,sha256,text}`；reverse 为原图、两种模板、是否分别反推、复审模型 / 候选参数。返回批次方案 `{plan_id,created_at,mode,plans,settings,warnings}`。POST start `{plan_id,request_id,seed}`；GET status / list / image、POST cancel 与 §15 同类语义，记录在插件 `lab/studio` 下，不混入旧严格 DTO。POST compare 接受 2–8 张 `{provider_id,prompt,images:[{id,mime,data_base64}]}`，第一张固定为基准图，视觉模型一次查看全部候选并按画风相似度返回最多三名：`{provider_id,text,latency_ms,reference_id,ranking:[{rank,image_id,similarity,reason}]}`。名次连续、候选不重复，similarity 是 0–100 的批内解释分，不能跨模型比较；解析失败保留原文并返回空 ranking，不编造赢家。旧 `pairs` 结果只供读取历史。图片只用于本次请求。
- **桌面桥**：`studio_capabilities`、`studio_references(req)`、`studio_plan(req)`、`studio_start(req)`、`studio_status(run_id)`、`studio_list(limit)`、`studio_cancel(run_id)`、`studio_image(run_id,idx,format,quality)`；`studio_skills_list/import/delete` 管理本机资料，`studio_skills_sync` 见 §25；`studio_compare(req)` 把本地图片副本送给选定的视觉模型。Rust 负责本地 reference / skill 快照与图片边界，前端不直接传文件系统代理对象。矩阵按确定组合逐个构思 / 运行，各行独立显示错误与状态；活动任务与历史选择分离，保存 `request_id`，不自动重试计费构思。状态列表读取失败不回报空成功。
- **本地画风比较**：`style_compare(req:{images:[{id,path}],models:string[]})` 支持 2..32 张图、§12.0 的三个注册编码器。所有无序图对分别计算原始 L2 向量的余弦，不减本批均值、不写索引、不改当前模型；缺权重逐模型报错，不自动下载。返回输入哈希 / 尺寸与逐模型 `{model,label,model_version,preprocessing,device,fallback_reason,load_ms,encode_ms,total_ms,pairs:[{left_id,right_id,cosine}],error}`，顶层 `centered:false`。注册 revision 只说明预期权重，不冒充本地文件已核验。前端每个模型独立设阈值并注明尚未校准；有人工分组标签时才算 pair accuracy / precision / recall，未标注不当阴性。不能用相似度断言两张图的风格配方相同；只有研究资料、尚未接入的模型不提供可运行按钮。验收口径见 `docs/STYLE_BENCHMARK.md`。
- **兼容与验收**：旧 creative capabilities 的 characters 不属于需剥离的多人方案字段，响应裁剪按此处理并有分发测试覆盖。全量等待与插件 480 秒对齐，桌面留足网络余量。只用合成图包 / 图片 / 模型桩、fixture 数据库和离线接口验证。

### 16.1 实现接口细化

- `studio_prepare(req)` 一次冻结输入，返回 `{input_id,character,characters,reference_count,reference_ids,source_image?}`；矩阵所有行沿用同一 `input_id`。`studio_plan` 在固定输入上只覆盖模型 / skill / 简报 / 评分与复审参数，插件收到 `experiment_id=input_id`，保证同一轮的风格抽取一致。`studio_plan_get(plan_id)` 可离线读冻结方案；`studio_workspace_get/save` 保存本轮配置和行 ID，最多 16 MiB，不重复保存原图 Base64。
- skill 材料单份最多 768 KiB、组合最多 1 MiB，实际摘要随方案冻结。
- 本地比较每图最多 32 MiB、合计 128 MiB、最多 32M 像素、长边 16384、长宽比 64；超限报错，不静默缩图。`style_compare_cancel()` 在模型 / 批次边界停止，已经开始的本地编码批次完成后释放共享推理锁。视觉 LLM 每次最多 8 张，输入预处理与 §4 相同，源图本身不改。
- `studio_image` 返回 `{path,original_path,format,original_bytes,encoded_bytes,quality}`；原始 PNG 与 JPEG 衍生都缓存为本地绝对路径，PNG 格式 `quality=null`。实验图片未准备好是独立的 pending 状态，不误判为接口未安装。

## 17. 插件与桌面共用常用 LLM 名单

- 插件管理 `model_selection: {enabled: false, provider_ids: []}`。未启用时可选全部 AstrBot LLM；启用时只展示所选且仍注册的模型，按保存顺序排列。启用时不可保存空名单；已选模型后来全部移除则有效名单为空并明确提示，不回退全部。
- `GET/POST page/model-selection` 沿用 WebUI JWT 鉴权；POST 为 `{enabled:boolean,selected:string[]}`，GET / POST 的 data 为 `{enabled,selected,available,effective,missing}`。available 是注册全量，selected 是保存名单，effective 是实际选项，missing 是已移除的所选 ID。严格检查形状、重复和新增的未知 ID；允许保留或删除旧 missing。保存失败回滚，不伪报成功；读取失败不返回空成功。
- 桌面端只读这份名单：`plugin_model_selection` 只 GET，原样返回插件 `data`；名单在插件管理页编辑，桌面不 POST。
- `page/providers` 保持 `string[]` 结构；插件内各处候选列表与 creative / studio capabilities 的候选列表同步过滤。识图、反推、构思、评分、画风复审等 LLM 选择框只接受这份名单，不手输，也不把旧值自动补成选项。切回页面 / 窗口及手动刷新时重新读取。CLIP / CSD / MegaStyle 等本地编码器不属于 LLM 名单。
- 这是模型**选择范围**：不改 AstrBot 其他插件的模型，不自动改写插件已保存的默认值、候补顺序或已冻结方案。名单外的旧值单独提示，可保持或移除；新增模型和新实验的显式模型参数必须在有效名单内。插件已有默认配置与在途 / 冻结任务继续按原模型运行。
- 不迁移 SQLite，不改画质、提示词、风格抽取、生成预算与计费规则。

## 19. 反推识图只保留候补链

- 插件识图只按候补链 `provider_ids` 的顺序尝试，拒答 / 错误才换下一项，成功即停止；候补链为空时按插件既有的回退顺序取识图模型。旧的单选识图配置键不再读取。
- 总实验室的 `defaults.reverse_provider_id` 是现存的响应字段名，仍保留，值取候补链首项或上述回退。桌面端无需升级；不改 SQLite、数据导出和历史实验。

## 20. 总实验室反推原图拖放

- 「反推 → 共同输入」支持把一张本地图片拖入原图区域，同时保留点击选图。原生拖放订阅只在该组件存在时启用，区域外不接收；离开页面退订，迟到的导入结果不得写入其他模式。
- 拖放走 `reverse_import_local_image(path)`，拷贝到应用数据目录、不修改原文件；点击选图复用 `reverse_pick_local_image` 已完成拷贝的返回值，不重复导入。支持的扩展名与选图入口相同，导入后先验证图片可预览才替换旧原图；多文件、非图片或读取失败明确提示并保留旧原图。
- 导入期间禁止构建方案与启动新实验，实验执行期间禁止替换原图；拖放只准备本地输入，不调用模型。冻结输入、生成参数、SQLite schema、历史数据与鉴权不变。
- 当前锁定的 Wry 0.55.1 在 macOS 上的拖放位置是 AppKit 逻辑坐标，命中测试不再除以 Retina 比例或 CSS 缩放；其他平台按 SDK 物理坐标转换。浏览器测试模拟其原生事件协议。

## 21. 全量参考取消应用字节预算

- 本节覆盖 §15.3、§16 早期的 2 MiB 参考 / 4 MiB 整体构思预算：总实验室、旧创作入口与插件共用创作链的 `reference_mode=all` 不设应用层参考总字节数或整体文本请求字节预算，也不换一个更大的常量继续拦截。
- 当前范围内的有效条目完整送给提示词模型，主 / 负面 / 角色提示词、来源与冻结快照保持一致。构思、编辑、保存 / 恢复、再次出图的复核都不重新施加旧预算；不静默截断、减少条数、改成抽样或改写筛选范围。本地私有实验快照取消固定读取大小限制，避免已保存的材料无法恢复或复核。
- 只取消全量文本的总字节预算：抽样的数量 / 预算、字段类型与内容格式、筛选 / 分级 / 排除名单、skill 与图片校验、工作区限制、生成参数档位与预算检查都不变；不改配置、SQLite schema 或决策数据。
- 全量构思仍可能增加模型 token、费用与等待时间。模型上下文、模型服务 / HTTP 的真实容量限制和超时明确报错，不承诺无限上下文，也不自动降级；AstrBot 的全局上传限制不在本节范围。

## 22. 内置源码更新

- 设置页「软件更新」，侧栏版本号可直达。没有预构建 / 签名的更新源，使用本机已有的 Git 与 Node / Rust / Xcode 工具链：首次选择本仓库的源码目录，之后在应用内检查、独立构建、安装重启。不需要输入新 token 或复制钥匙串密钥。
- 只检查固定仓库 `hujiehuan/nai5-codex-lab-public` 的稳定 `vX.Y.Z` 标签；冻结版本、tag 与解析后的 commit，构建前复核 tag 未改变。检出到应用私有更新目录的 detached worktree，运行锁文件安装与 Tauri app 构建；不切换 / 清理原工作树、不丢未提交修改，不自动安装系统依赖。Git 鉴权失败、缺工具、构建失败、取消和读取失败分别显示，不伪报最新版，不静默使用其他仓库或来源。
- 进度显示实际阶段与日志摘要，不伪造下载 / 编译百分比；检查只访问 Git、不编译；准备更新只在明确点击后执行；安装只由「安装并重启」触发。离开设置页不丢进行中的状态；同一时刻只允许一个更新操作，取消会终止自身进程树。启动时把未完成任务标记为中断，不自动继续执行命令或重启。
- 原生 DTO `AppUpdateStatus`：`supported:boolean, phase:idle|checking|up_to_date|available|preparing|ready|installing|error|cancelled, current_version:string, current_revision:string|null, source_dir:string|null, candidate:{version:string,revision:string,tag:string}|null, stage:string|null, message:string|null, log_tail:string[], started_at:string|null, can_install:boolean`。IPC：`app_update_status()`、`app_update_choose_source()`、`app_update_check()`、`app_update_prepare()`、`app_update_cancel()`、`app_update_install()`；长检查 / 构建后台运行并通过 status 轮询，取消选目录时保持原选择。
- 安装前阻止新工作并确认当前本地任务 / 请求结束，保存工作区状态，用 `VACUUM INTO` 写一份 `preupdate` 快照；快照失败则停止安装，旧软件继续可用。校验生成的 `.app` 的 bundle ID、版本、可执行文件与当前架构；开发窗口、DMG / 只读 / 不可写的安装位置不伪称已自动安装，并给出原因。
- 安装助手等待当前进程退出，在同目录准备新 `.app`、保留旧 `.app` 后切换；复制 / 切换 / 打开失败时尽力恢复旧应用，并持久化结果供下次读取；不自动回滚数据库。只替换应用本体，identifier、应用数据目录、钥匙串 service 与进程读缓存保持不变。日志不记录凭据、完整远程认证 URL 或图鉴内容。更新脚本的参数以 argv 传入，不拼接路径执行 shell。
- 测试使用临时 Git 仓库、假工具链 / 安装目录和浏览器 mock，覆盖失败 / 取消 / 并发 / 重启恢复及数据不被替换；Finder 权限与真实的应用替换只能在 Mac 上验收。

## 23. 全量参考的单图包材料完整性

- `reference_mode=all` 不限制参考图包的角色段数，也不限制参考主提示词、负面词、角色提示词及角色负面词的字符长度。参考材料不是最终生成请求，不按单张生成的角色数量约束它；不换更大常量、不截断或丢弃整条图包。
- 仍验证 JSON 数组 / 对象 / 字符串类型、来源 ID、元数据、重复条目、筛选范围、分级和排除名单。可选负面词缺失或 null 按空串处理，其他错误类型报错；角色字段错误指出来源 entry_id、从 1 起的角色段序号和字段，不输出参考正文。
- 总实验室与旧创作入口的全量构思、冻结快照及执行前复核共用此规则；抽样模式的数量 / 长度预算、来源挑选策略、模型输出格式及生成参数档位不变。实际模型上下文或服务容量拒绝如实显示，不自动删材料或换模型重试。
- 只改插件端的参考校验，桌面请求 DTO 不变；不迁移数据库，不改写筛选数据。

## 24. 总实验室全部图文参考

- 总实验室 `/画` 的每条构思参考为「主参考图 + 完整图包文本」，覆盖 §15.3 在本入口「不附带图片」的约定。全量模式同时提供当前范围内每个合格图包，不先让模型挑图、不分批归纳或拼接缩略图、不静默退回文字或抽样。抽样入口只发送其选中条目的图文；新工作区默认全量，已有选择保留。用户要求、角色、skill、筛选 / 分级、风格配方与生成参数不变；不改插件指令默认流程或旧 creative 协议。
- 每个图包固定取 `entry_image.ordinal=0` 的主参考图：优先已下载的展示图 `local_path`，缺文件才用同一主图的 `original_local_path`，绝不拿 `thumb_path` 或另一条图包代替。附带该图 `raw_tag`（允许 null）以及图包全部主 / 负面 / 角色提示词，通过 entry_id 和图片顺序关联。缺图、损坏或不支持的图片在调用模型前定位条目并报错，提示补齐图片；不从参考中剔除缺图条目，不自动联网下载。图片保持完整像素和字节，不为增加条数缩放 / 重编码；单图上限 12 MiB、32M 像素，格式为 PNG / JPEG / WebP，不另设图文总量或图片张数预算。
- `studio_prepare` 冻结全部图文和图片 SHA-256；图片副本放本机应用私有的 `studio/reference_images/<input_id>/`，工作区不保存 Base64。返回 `reference_image_count`；原图 / 图库后来改变不影响同一轮的模型 / skill 对照或重试。旧方案仍可查看和出图；旧纯文本 input 不能冒充新图文输入，重新构思须重建输入。
- Studio capabilities 的 `reference_images_supported:true` 表示支持图文参考，缺省为不支持；新版客户端在 draw 新任务前检查，不支持时要求更新并重载插件，不降级为纯文本。同鉴权、同 `features=studio_v1` 的 `POST page/lab/studio/reference_images_status`：`{images:[{sha256,mime}]}` → `{missing:[sha256]}`；`POST page/lab/studio/reference_image_upload`：`{sha256,mime,data_base64}` → `{sha256,mime,bytes}`，逐张校验真实格式 / 像素 / 哈希后幂等缓存到插件私有目录。哈希不能作为任意路径，坏缓存不视作命中。逐图传输避开 AstrBot 单次 128 MiB 的上传限制，不改宿主全局配置。
- POST plan 的 `reference_images:[{entry_id,ordinal:0,mime,sha256,raw_tag:string|null}]` 与 references 同序一一对应，无遗漏 / 重复 / 越界，插件从已校验的缓存取真实图片。字段缺失只为旧客户端保留纯文本兼容，显式空 / 损坏不能降级。reverse 入口不能借此添加无关材料。返回 `settings.reference_image_count` 与 `settings.reference_images` 同一清单，桌面严格比对后保存；历史显示实际提供的文字和图片数量，图片数量不等于模型认真阅读过的证据。
- 构思调用同时包含所有完整参考文字、顺序 / ID / 每图 raw_tag 标识和所有真实图片，只有支持看图的模型才能处理。独立 provider 副本阻止上下文裁剪和删图后重试，检查适配器是否丢图，保留正确的 MIME，不修改共享 provider / AstrBot 核心；不兼容的适配器、模型上下文 / 图数 / 请求容量拒绝与超时明确报错，不换模型、不自动拆批或少给材料。图文都是不可信的参考材料，不能覆盖用户要求或程序输出协议。上传 / 冻结不调用模型。
- 只用合成图片、假模型和 fixture 数据库验证：1367 条图文一一送达，顺序 / 哈希 / MIME 正确，缺图 / 缓存损坏 / 旧插件 / 无视觉能力 / 适配器删图都被拒绝，冻结后文件改变仍用原快照，重新出图零构思调用。无 SQLite schema 或筛选决策变更。

## 25. 总实验室跟随插件所选 skill

- 插件更新并选中新 skill 后，桌面默认资料随之更新；本节覆盖 §16「默认使用内置固定版」的约定。插件里下载但未选中的版本不影响当前选择。稳定选择项 `plugin-active`：draw 跟随插件 `skills.draw_skill_id`，reverse 跟随 `skills.reverse_skill_id`；反推未启用资料时明确提供零份 skill，不补入内置版本。内置固定版、本机导入以及无 skill 对照仍可显式选择。
- 插件 `GET page/lab/studio/skills?features=studio_v1&kind=draw|reverse`，沿用 Studio 的 JWT 与 `X-Library-Token` 鉴权和 status / data 封装。data 为 `{version:1,kind,enabled,selected_id,skill}`；skill 为完整的 `{id,name,version,sha256,text,source,builtin,bytes}`。draw 默认返回有效的 builtin；reverse 关闭时为 `enabled=false` / `selected_id=""` / `skill=null`。指定版本缺失、损坏、哈希 / 字节数不符必须报错，不静默回到旧版或冒充关闭；该接口只读，不改插件的选择。
- 桌面 `studio_skills_sync(kind)` 校验后返回相同形状，其中 `skill.id` 改为本机不可变快照 ID，`selected_id` 保留插件 ID。正文、版本、来源和 SHA-256 存在本机私有的 `studio/plugin_skills/`；普通资料列表不堆积历代快照，普通删除不影响它们。读取失败显式报错，不用旧缓存冒充同步成功；保留单份 / 组合容量与完整性校验（§16.1）。
- 新工作区默认资料选择 `plugin-active`；旧工作区只在首次把未改名的默认组（仅包含 `nai5-prompting`）迁移为跟随，记 `skill_follow_version:1`。自定义组合、显式空组、已有的行 / 方案 / 历史都保留；之后显式选择固定版不会被再次迁移。未解析的 `plugin-active` 不能直接进入构思。
- 进入页面、切换实验模式、窗口重新聚焦和显式同步时读取当前选中的版本；新一轮含跟随项的对照在准备输入前必须再同步一次，把该轮所有组合解析为同一份本机快照 ID。构思、失败重试及已冻结方案再出图不重新读取插件的当前版本，新一轮才跟随新选择。界面显示实际名称、版本 / 哈希与关闭 / 加载 / 失败状态，不把「同步成功」当作模型调用的证据。重复的跟随项不重复发送材料。
- 无 SQLite schema / 筛选决策变更，不改模型、图片 / 文本参考、生成参数或插件指令逻辑。用合成资料覆盖版本切换、关闭 / 缺失 / 篡改、旧插件、跨模式并发、同轮一致与旧实验重试。

## 27. 总实验室显式纯文字参考

- 在保留图文参考的同时，提供「全部文」「抽样文」两种纯文字参考；本节覆盖 §24 中 draw 必须传图的要求，界面提供全部图文、抽样图文、全部文、抽样文四种选择。`reference_mode:all|sample` 仍只决定条目范围；`reference_content:image_text|text` 决定是否发送图像。旧工作区缺省为 image_text，保留已有的 all / sample 选择，不擅自改默认或自动降级。reverse 的原图与反推流程不受此选项影响。
- text 模式保留所选图包全部主 / 负面 / 角色提示词及来源元数据，不读取、校验、冻结、编码、上传或发送参考图片；缺少本地图像不影响纯文字构思。all 仍不设应用层参考总字节预算，sample 沿用原有的数量、选取和预算；筛选范围、分级、排除名单、角色、skill、生成与费用规则不变。纯文字减少图像输入，但不承诺总 token 一定很小或消除模型上下文 / 超时限制。
- capabilities 的 `reference_text_supported:true` 表示支持纯文字参考；显式选择 text 时两端都校验此能力，不支持则提示更新插件。图文仍由 `reference_images_supported` 控制并校验完整图片清单；对只支持旧图文协议的插件可以省略新字段，但不能把图文请求改成 text。显式的未知内容模式、text 携带 reference_images、image_text 缺失 / 空 / 损坏图片都拒绝。旧请求未带 reference_content 时保持原兼容：有 reference_images 按图文处理，没有该字段按旧纯文本处理；这些历史记录不能冒充新图文。
- `studio_prepare` 返回并冻结 reference_content；text 的 `reference_image_count` 为 0，不产生图片快照。`studio_plan` 使用冻结的选择，不以表单当前值覆盖；text 请求发送 `reference_content=text`，不发送 reference_images。插件响应的 settings 回显 `reference_content=text`、`reference_image_count=0`、`reference_images=[]`，桌面严格核对。图文回显的 mode 与图片顺序 / ID / 哈希仍须一致；旧图文插件缺 content 回显时只按完整图片清单判定。既有方案、历史、同请求重试保持原材料。
- 插件冻结的实验输入摘要包含显式的 reference_content，防止同一 ID 换材料模式；未带字段的旧请求仍沿用旧摘要。桌面私有输入同时记录是否在请求中声明了该字段，旧图文 input 不因插件升级而补字段改变摘要；原来显式协商的图文 input 遇到插件降级时报错，不能省略字段冒充同一输入。text 走完整的文字保护而非图片包装；不裁剪或自动切换模型。界面按模式展示实际的文字与图片数、抽样条数及说明，不要求纯文字模型有视觉能力。所有异步错误明确显示，未知数量不记作零。
- 不涉及 SQLite schema、筛选数据、插件配置或自动重载。回归用合成数据验证四种方式、无图纯文字、缺图图文拒绝、能力不支持、冻结重试与旧历史，并在 1280 / 390 px 宽度和深浅两种外观下检查版面。
