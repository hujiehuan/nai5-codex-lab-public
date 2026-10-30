# Changelog

## 0.5.16 — 2026-10-28

- 侧栏新增「所长法典」页（法典图鉴的 `suozhang_nai5`，所长法典 · N5 常规）。随图库一起同步条目和展示图，可预览、复制、按目录和批次筛选、三态标记。
- 法典书文件不带 `type`、少数条目没有配图、条目不写分级：按 `codex` 入库，无图条目显示「无配图」，分级按书定为 safe（非 safe 分级的条目在解析时丢弃）。其他书的校验不变。
- 法典是可选来源：单本下载或解析失败只记一条并报告部分完成，画师词典和精选图包照常更新；下次同步只重试失败那本。升级后第一次同步即使 release 没变也会补拉这本。
- 法典标记只留在本机：不进完整池推送、总实验室参考和画风索引，插件的 /画 参考池不受影响。无数据库迁移。
- 公开版只保留桌面端功能：插件桥收窄为只读的 `plugin_model_selection`（常用 LLM 名单，CONTRACT §14、§17），实验室点数查询命令一并删除。内置提示词资料换成本仓库原创的 `resources/prompting-guide/GUIDE.md`（skill id 仍为 `nai5-prompting`，版本 `builtin-1`）；反推评测的角色别名表换成合成示例 `character_aliases.txt`。

## 0.5.15 — 2026-10-03

- 画师词典新增「法典 / NAX V5」来源切换，NAX 的 V5 Full 单画师样例复用三态筛选、本地预览与画师串复制；两来源独立保留筛选与选择。
- 常规同步每次检查 NAX 全部编号页，独立于法典 release，增量入库并补齐公开 WebP 展示图；暂停后可继续补图。来源读取失败保留旧条目和决策、继续另一来源，明确报告部分完成。
- NAX 选中画师可随画师库导出；本地画风索引与检索纳入新来源。数据库升级仅扩展画师库视图，旧决策、备注、审计及实验记录保留。
- 详细说明与验证见 `docs/NAX_V5_SYNC.md`。

## 0.5.14 — 2026-09-22

- 画风复审的视觉模型改为批内排序：图 1 固定为基准图，每个模型一次查看全部 2–8 张图片，从其余候选中返回画风最接近的前三名、批内相似度和具体理由。
- 结果页直接展示前三名候选预览；无效或不完整结果不补造赢家，并保留模型原文。旧版两两判断历史仍可查看。
- CLIP、CSD、MegaStyle 本地编码器继续做全部图对与人工标签指标，不与视觉模型的批内自评分混排。

## 0.5.12 — 2026-09-21

- 总实验室默认资料改为「跟随插件所选 skill（自动同步）」：`/画` 使用插件的“出图用”选择，反推使用“反推用”选择。显示实际名称、版本、摘要和全文；插件仅下载但未启用的版本不会切换到 Mac。
- 每轮新实验开始前同步一次，为所有模型组合冻结同版资料；重试和已有方案保留旧快照。反推关闭 skill 时明确不附加资料，读取失败或旧插件明确提示，不回落到旧缓存。固定版本、本机资料和无 skill 对照继续可选。
- 旧默认组仅迁移一次，自定义组合和历史保持。无 SQLite、筛选或出图参数变更。需要插件提供新的 Studio skill 读取接口；插件更新后在 AstrBot 重载。

## 0.5.11 — 2026-09-21

- 总实验室 `/画` 新增清晰的「全部图文一起传」选择：每个合格图包的主参考图、原图标签及完整主/负面/角色提示词同时提供给构思模型。新工作区默认全量，已有抽样选择保留；抽样模式也提供对应图片。
- 同一轮对照先冻结图片和文本，图包文件后来变化不影响对比。逐图上传并复用插件缓存，最终构思仍为一次完整图文调用；缺图、损坏、旧插件或模型无法容纳时明确报错，不自动丢图、裁剪或降级为纯文字。
- 方案显示实际提供的文字条数和图片张数，旧方案标为纯文字；新构思需插件 v2.0.9 或兼容能力，更新后重载插件。旧已冻结方案仍能查看和出图。
- 不改变 SQLite、筛选、插件指令默认逻辑、出图尺寸/点数规则。用 1367 条合成图文、真实适配器组装方法及假模型验证。

## 0.5.10 — 2026-09-21

- 修复总实验室点击结果图后原图不可见或超出窗口的问题：弹窗与图片区域明确按窗口尺寸布局，兼容页面滚动及应用缩放，整张图片按比例显示。
- 原图加载中、读取失败和重试分别显示；保留关闭按钮、Escape 及焦点恢复。查看图片不触发重新构思或生图，不改图像文件、生成参数或筛选数据。
- 通过「设置 → 软件更新」检查本版本。验证命令：`npm run typecheck`、`npm run test`、`npm run build`、`npm run test:studio-image`。

## 0.5.8 — 2026-09-21

- 全量参考按所选范围完整发送，取消总实验室与旧创作入口的 2 MiB 参考、4 MiB 文本请求硬上限。插件需同步升级至 v2.0.7 并重载，模型上下文或基础设施的实际限制仍明确报错，不自动抽样。
- 移除本地私有实验快照的固定读取大小限制，避免全量材料已保存却无法恢复；不改 SQLite、筛选决策、图片尺寸或点数规则。
- 更新提示与合成回归，验证超过旧阈值的材料在构思、编辑、冻结和恢复后保持完整；抽样预算保持。

## 0.5.7 — 2026-09-19

- 总实验室的反推原图支持从 Finder 拖入，区域高亮并显示读取状态；保留点击选择或更换图片。
- 单次接受一张图片，校验预览后替换；多图、错误文件、读取失败均保留旧图。导入时锁定构建操作，实验执行时锁定原图，切页退订且忽略迟到结果。
- 复用现有本地副本导入与冻结输入，不改变原文件、生成参数或数据库。适配当前 Wry 的 macOS 逻辑坐标及应用缩放；原生 Finder/Retina 仍需 Mac 实机验收。
- 验证：`npm run typecheck`、`npm run test`、`npm run build`；`npm run test:studio-drop` 覆盖 9 组原生事件桥场景和 1280/390px × 浅深色 4 档布局，`npm run test:studio` 回归四类实验流程。基线没有拖放监听、拖入无效果；新增入口通过模拟事件导入并预览图片，测试不调用真实模型或出图。

## 0.5.5 — 2026-09-19

- 三个独立实验室合并为总实验室：反推、/画、画风复审三个模式，旧实验记录仍可查看。模型 × skill 组合共用冻结输入，构思与生成分开，失败、停止、重试与历史分别处理。
- 反推可编辑普通/分别反推模板、评分模型和画风复审模型。创作可配置角色、多人简报、硬要求、独立构思张数，以及已加入/全部决策的图包范围；实验设置不写回插件配置。
- 技能支持内置 NAI5 提示词写法（`nai5-prompting`）、从固定版本导入 NPE、导入本机 Markdown 目录及空组合对照；结果记录材料摘要。所有新生成限定三档免费尺寸和逐张免费资格检查，额外 LLM API 费用明确显示。
- 画风复审支持多图视觉模型比较与 CLIP ViT-B/32、CSD ViT-L/14、MegaStyle 本地全部图对比较，按人工分组评估；模型未安装、未知判断和评分失败不计为零分。JPEG 是可调质量的衍生副本，原 PNG 保留并显示实际字节数。
- 修复插件兼容层误删 characters 造成的创作能力错误；全量构思等待与插件对齐。schema 仍为7，没有筛选决策迁移。

## 0.5.0 — 2026-09-10

- Naive UI 基础、统一颜色令牌与 macOS 明暗主题，重做页面外壳及原有视图布局；宽表和长提示词局部滚动，统一按钮、键盘焦点及费用标识。
- 跨页面共享 JWT 并合并并发登录，401 重登一次、429 冷却；保留钥匙串进程缓存。反推详情优先展示已下载原图，展示图/缩略图回退明确标注。
- 读取失败与空数据分开；保留原有三态、批量、撤销及快捷键。schema 保持 7，历史库升级用合成 fixture 验证，提供 Mac 本机只读校验脚本。
- 插件仓库未修改。桌面候选复审缺少插件接口，未冒充接通。
- 验证：前端 311 通过 / 1 跳过，Rust 152 单元 + 2 接口样本，44 组版面检查通过；历史版本合成库逐级迁移保持 3191 条目和 502 加入。Mac 实机验收待用户进行。

## 0.4.1 — 2026-09-07

- 反推实验室运行区新增「停止运行」：停止后保留已完成结果，不再提交后续题目或重画；界面显示「停止中…」与「已停止」，保留实际进度。
- 修复取消批次被误报完成、浏览历史影响活动批次状态的问题；启动准备到结束只允许一个活动批次，停止按批次 ID 匹配，重复停止不会误停新批次。
- 已发出的远程模型 / 绘图请求仍可能在服务端完成；本版停止桌面端调度，不改变线上插件。MegaStyle 权重已校验并通过真实 CPU 推理，用户反馈它在本轮图片上最符合画风还原需求；插件端向量检索仍待接通。

- 验证：前端 304 passed / 1 skipped，Rust 145 单元测试 + 2 接口样例通过；类型检查、生产构建与模拟 IPC 的浏览器停止流程通过，没有调用真实模型服务或出图接口。

## 0.4.0 — 2026-09-07

- 完整保留画风词典原配方：不再只提取 artist。检索、复制、重画和画风池导出保留线条、配色、质量词与正负权重；仅对完整串去重。社区图包退出画风索引/检索，原有画风词典 CLIP 向量继续可用。
- 加入 MegaStyle-Encoder 与 CSD ViT-L/14，和 CLIP 分模型下载、选择、增量索引及同图对比，保留 Metal / CPU 选择、暂停续算与 ETA。模型 ID 在批次开场固定，切换不会混用索引或均值。
- 加入 Kaloscope 2.0、Camie Tagger v2 的画师标签识别对照；自动准备官方 ONNX Runtime，无需 Python。识别无需图库索引，CPU 执行，概率不作为画风权重。
- 固定模型文件版本并校验大权重 SHA256；顺序下载全部、取消续传，顺序推理并释放前一模型缓存。完整池新增 style_prompt，插件筛选池导入接受完整风格配方。插件端向量检索仍待接通。
- 真实权重已在 Linux 下载并验证；CSD/MegaStyle 与官方参考实现对拍通过。使用方法、限制与模型来源见 docs/STYLE_MODEL_COMPARISON.md；用户图片上的还原效果和 Mac GPU 仍待真机对比。

- 回归：前端 293 项通过 / 1 跳过，Rust 134 项与 2 项接口样例通过，插件 17 / 7 组通过；类型检查、生产构建和五模型模拟 UI 检查通过。

## 0.3.4 — 2026-09-07

- 设置页「画风模型」新增计算设备：自动（优先 Metal）、Metal GPU、CPU。macOS 构建默认包含 Metal，旧设置默认自动；界面显示实际加载的设备。自动模式在 GPU 初始化、模型加载或试算失败时回退 CPU 并显示原因；显式 Metal 失败会报错。
- 模型权重和图片张量一起放到选定设备，索引与查询图编码共用；切换设备后重新加载，已完成索引保留，继续只算缺失或换图的条目。模型、F32、预处理、512 维向量与 schema 7 保持不变。
- 为 Candle、CPU F32 矩阵乘法和图像预处理启用开发依赖优化，改善 `tauri dev` 下的计算速度；首次更新需重编这些依赖，启动前的编译会更久。索引 / 同步 / 下载运行时禁止切换设备。
- 验证：前端 283 项测试（另 1 项跳过）、类型检查及构建通过；后端检查、116 项单元测试与 2 项接口样例通过，实际加载官方权重完成 CPU 编码；浏览器检查设备保存、忙碌禁选、实际设备及回退显示通过。Mac Metal 编译、GPU 运行和 M5 Pro 提速幅度仍待真机验收，提供 CPU / Metal 同图向量对照测试。

## 0.3.3 — 2026-09-07

- 画风索引新增「已用时间」「预计剩余约…」「处理速度」：设置页手动建索引与同步页自动建索引共用；根据最近几批的实际速度估算，续算时只计本轮待处理条目，切换页面保留计时。
- 前两批采样完成前显示「正在估算…」，排除模型首次加载对速度的影响；长时间无进展时重新估算，收尾阶段不显示虚假的剩余 0 秒。完成、取消或失败时停止计时，保留已用时间；再次运行重新采样。
- 修正取消索引的提示：明确显示「已取消，已完成结果已保留」，避免把取消说成全部完成。

- 验证：前端类型检查、281 项测试（另 1 项跳过）、生产构建通过；Rust 检查与 110 项单元测试、2 项接口样例测试通过。使用真实 Vue 页面和模拟进度事件检查估算显示、切页保留、取消、重新启动与布局；Mac 真机 ETA 仍待用户验收。

## 0.3.2 — 2026-09-07

- 修复设置页「下载模型」HTTP 404：官方 `openai/clip-vit-base-patch32` 主分支只有 `pytorch_model.bin`，此前请求的 `model.safetensors` 不存在。默认下载改为官方 bin，并使用 candle 的纯 Rust PyTorch 权重读取器加载图像塔；不需要安装 Python/PyTorch。
- 保留已有 `model.safetensors` 的识别与加载，不重复下载权重；空配置会重新下载，`.part` 不算已完成。模型仍为同一个 CLIP ViT-B/32，512 维、预处理、检索算法和数据库 schema 不变。
- 下载回归桩现在只提供真实存在的文件路径，防止「任意 URL 都成功」漏掉文件名错误；覆盖官方 bin 下载、断点续传、取消、已有 safetensors 和空配置修复。

- 验证：Rust 检查与 110 项单元测试、2 项接口样例测试通过；已下载并加载官方真实权重，完成 512 维编码及同图/异图相似度检查。前端类型检查、261 项测试（另 1 项跳过）与生产构建通过；Mac 真机下载和索引仍待用户验收。

## 0.3.1 — 2026-09-07

精准反推（CONTRACT §12.7）：反推标签之外，**自动**用画风索引在两本词典全量里检索原图最像的一条，把它的画师串加进重画——一次同时反推"画什么"和"谁的画风"。它是 §12.6 画师来源的第四个取值 `style`，桌面端把它翻译成对插件的 `artist_mode="given"` + `artist=<命中串>`，**插件不改**。成对评委的"像原图"分在 `style` 模式与 `pool` 模式之间的差，就是画风检索对最终效果的贡献。前端 vitest 19 文件 262 例（+14）。

- **契约（§12.7）**：`RunBatchRequest` / `reverse_render_run` 新增 `style_top_k`（缺省 5，检索候选数，取过滤后第一条可用的）、`style_decisions`（空 = 全部；黑名单永不用）、`style_min_score`（相似度低于它按兜底处理；null = 不设门槛）、`style_fallback`（`pool` 缺省 / `none`）、`style_center`（缺省 true）；流程 = run 解析成功 → `style::search`（词典题 entry/ordinal，本地题 path）→ 过滤取第一条 → 画师串 = 画师条目 `core` / 图包 `pack_tags`（散文图包只取 `<artist>` 块解析出的 `core`，没有则跳过该命中）→ 无命中或 `score < style_min_score` → 兜底 → 调重画；迁移 007 给 `reverse_run` 加 `render_style_entry_id / render_style_title / render_style_score / render_style_decision / render_style_fallback`，`RunRow` 同名字段，`render_artist_source` 取值新增 `"style"`；每模型汇总加 `style_hit_n`（用到检索画风的张数）/ `style_score_avg`；索引未建 / 模型未下载 → 批次开始前 `bad_request`（不消耗点数）
- **前端**：`types.ts` / `api.ts` 按 §12.7 逐字段追加（`RenderArtistMode` 加 `"style"`、新 `StyleFallback`、`RunBatchRequest` 五键、`RunRow` 五键、`ProviderSummary` 两键；`reverseRenderRun` 第六个参数 `style` = 五个 `style_*` 键，不给就发契约缺省，两条路都**总是显式发**、后端只在 style 时看）。实验配置"画师来源"下拉新增"精准反推（画风检索）"，选中展开子表单：检索候选 top_k（1–20，缺省 5）、决策范围三个复选（一个都不勾 = 全部）、最低相似度（0–1，留空 = 不设门槛，非法红框并挡住运行）、兜底（画风池随机 / 不带画风）、去内容（缺省开）；索引未建 / 模型未下载时块内黄底提示"先在设置页下载模型并建索引"+ 状态一行 + 去设置页链接 + 刷新（只提示不挡：后端在批次开始前就报错、不消耗点数）；运行按钮后缀 `· 检索画风`。结果表每模型行新增"检索命中 / 检索相似度均值"两列（老后端缺键显示 —），逐题行"重画"一格前对 `render_artist_source="style"` 的 run 加"检索"徽标（title = `检索命中：标题 · 0.512 · 加入`，兜底了附说明）。单题面板重画图下方新增"命中画风"块：命中条目缩略图（`RunRow` 没有，经 `get_entry` 取展示图 `thumb_path`、按条目缓存，取不到只不放图）、标题（空退到 entry_id）、相似度三位小数、决策徽标、"已兜底"徽标 + 说明（命中但低于门槛 / 根本没命中）；画师来源徽标新增"检索命中"。单题面板"重画"沿用实验配置里当前的来源（style 时连子表单一起发），画风反推"用它重画"仍是 given。`lib/reverse-summary.ts` 新增 `StyleRenderForm` / `styleRequestFields` / `clampStyleTopK` / `parseMinScore` / `styleHitInfo` / `summaryStyleCells` 等纯函数 + 单测 12 例，`stores/reverse.test.ts` +2 例；表单会话内保留不写 localStorage（与画风反推面板一致，实验配置本来就没有持久化）

## 0.3.0 — 2026-09-07

画风索引与反推画风（CONTRACT §12）：反推画风 = **检索**而不是让视觉模型猜画师名——同步时给两本词典全部条目的展示图各算一个 CLIP 向量存库，反推时只给那张图算一个向量、减去库均值后比余弦相似度取前 K；命中可以一键复制画师串，也可以直接"用它重画"，让"反推标签 + 检索到的画风"重画出的图和原图对照，作为画风检索准不准的直接证据。同一版本里把实验室重画与插件 `/反推` 逐条对齐（§12.6），否则对照实验没有意义。前端 vitest 19 文件 248 例（+36）。

- **裁定（§12.0）**：编码器 v1 = CLIP ViT-B/32（openai/clip-vit-base-patch32，图像塔 + 投影，512 维，L2 归一），candle 纯 Rust CPU 推理，权重从 Hugging Face 下到数据目录 `models/clip-vit-base-patch32/`（`config.json` + `model.safetensors`，约 600 MB，一次性），模型名与维度随向量一起存、换模型 = 全量重算；索引覆盖**全量**（含未定 / 不加入 / 黑名单命中），不是筛选后的池；"看风格不看内容"的 v1 手段是查询与库向量都减去库内全部向量的均值再算余弦（示例图共用同一角色模板，均值里装的是内容），均值缓存 `app_state('style_mean:<model>')`、索引变化时重算；黑名单命中的**永远**不返回，其余按相似度降序、每条带 `decision`，"不加入"灰色标出但不藏；图包条目也进索引，命中图包给整条画师串
- **后端（§12.1–§12.4，Rust）**：迁移 006——`image_embedding(entry_id, ordinal, model, dim, vec BLOB f32 小端, image_sha256, computed_at)`，只对 `ordinal=0` 且已下载的展示图建索引、`removed_at` 非空跳过，`app_state('style_model')` 记当前模型名（缺省 `clip-vit-base-patch32`），`reverse_run` 加 `render_artist TEXT`。同步在图片阶段之后增加 `embedding` 阶段（`SyncPhase::Embedding`，`SyncEvent` 加 `embed_total / embed_done / embed_failed`）：对当前模型缺向量或 `image_sha256` 与 `entry_image.sha256` 不一致的展示图逐张算，单张失败记 `app_event` warn 并继续，`sync_cancel` 可取消；模型未下载整个阶段跳过、发一条 `message="画风模型未下载，跳过索引"`、不算错误。新命令 `style_model_status() -> StyleModelStatus { model, dir, present, size_bytes, indexed, total, mean_ready }`、`style_model_download(Channel<DownloadEvent>) -> StyleModelStatus`（从 `huggingface.co/openai/clip-vit-base-patch32/resolve/main/{config.json,model.safetensors}` 流式下到 `.part` 再改名，`DownloadEvent { file, done_bytes, total_bytes, finished, error }`，`style_model_download_cancel()` 取消）、`style_index_rebuild(force, Channel<SyncEvent>)`（不经同步单独建索引，`force=true` 删掉当前模型全部向量重算）、`style_search(req) -> StyleSearchResult { model, indexed, query_ms, hits }`（`StyleSearchRequest { source: entry | path, top_k 1..50, codex_ids 空 = 两本, decisions 空 = 全部, center }`，`StyleHit` 带 `decision / score / core / pack_tags / thumb_path / prose`，模型未下载 / 索引为空 → `bad_request`）。预处理：短边缩放到 224、中心裁 224×224、RGB、CLIP mean/std 归一，图像塔 pooled 输出经投影 512 维 L2 归一，16 张一批，`spawn_blocking` 里跑，不阻塞异步运行时；单测用随机权重（`VarBuilder::from_varmap`；契约写 `VarBuilder::zeros`，但全零权重经 L2 归一是 NaN、验不了单位向量）实例化图像塔 + 投影验证形状与流水线、不下载权重，设了 `CLIP_MODEL_DIR` 才跑真权重的最近邻健全性测试（同一张图与自身 > 0.99）。完整池导出补 §8.1 预留：索引存在时 `embedding = { model, dim: 512, centered: false, mean: base64(float16) }`，每条 `FullEntry` 追加 `vec: base64(float16 小端 × dim)` 的**原始**向量（未减均值；没有向量的条目 `vec: null`）。`RunBatchRequest` / `reverse_render_run` 增加 `artist: string|null`，`RunRow` 增加 `render_artist`
- **插件端（§12.5，astrbot_plugin_vivian）**：`POST /page/lab/reverse/render` 增加 `artist: string|null`——非空则作为 `Req.artist`，**仍不注入画风池**；`full_pool.json` 的 `embedding` / `vec` 照旧当不透明字段存储与显示
- **前端**：`types.ts` / `api.ts` 按 §12.2 逐字段追加（`SyncPhase` 加 `embedding`、`SyncEvent` 三个可选 `embed_*`、`StyleModelStatus` / `DownloadEvent` / `StyleSearchSource` / `StyleSearchRequest` / `StyleHit` / `StyleSearchResult`、`RunBatchRequest.artist`、`RunRow.render_artist`；`styleModelStatus / styleModelDownload / styleModelDownloadCancel / styleIndexRebuild / styleSearch` 五个封装，`reverseRenderRun` 第三个参数 `artist` 缺省 null）。设置页新增"画风模型"卡片（模型名、目录、是否已下载与大小、已索引 / 应索引与百分比、均值是否就绪；"下载模型 / 重新下载"、"重建索引"、"全部重算"、"取消下载 / 取消索引"，下载进度按文件、建索引进度按 `embed_done / embed_total`）；同步页进度条在 `embedding` 阶段改显示画风索引进度（轮询到的状态没有计数时跑马灯），完成横幅附画风索引计数。反推实验室底部改为两个互斥的可折叠面板：新"画风反推"（`components/reverse/StylePanel.vue`：来源 = 当前 run 的原图 / 题库勾选的题 / 本地图片；top_k、词典范围、决策范围、去内容；结果卡缩略图 + 标题与目录 + 决策 / 散文徽标 + 相似度条（宽度按余弦收进 0..1，数字原样三位小数）+ 画师串（画师条目 `core`、图包 `pack_tags`）一键复制 + "用它重画"；模型未下载 / 索引为空在面板内给"去设置页"链接）与原"对比两个批次"。"用它重画"打到 store 里的"当前 run"（最近在单题面板打开的那条，面板关掉仍记住、换批次清掉），没有则置灰给提示；单题面板重画图下显示 `render_artist`；实验配置的重画块新增"画师串（可空）"整批用。`lib/style.ts` 纯函数 + 单测 18 例，`stores/style.ts` + 单测 4 例，`stores/reverse.test.ts` +2 例
- **重画与插件 `/反推` 逐条对齐（§12.6，三端）**：此前实验室重画固定 `size_for_tags` + 单张 + 不抽画风池，与线上不一致。现在按 §12.6 对照表逐项对齐——尺寸按**原图宽高比**用线上同一函数（`reverse_draw._size_for_image` 拆出 `size_for_ratio(plugin, w, h)`，缺原图宽高才退到 `size_for_tags` 并回 `size_source="tags"`）；画师串按 `artist_mode`（`pool` 缺省 = 与线上同一函数同一概率抽画风池 / `given` 用 `artist` / `none` 纯标签对照）并回传 `artist` / `artist_source`；张数 `count`（null = 插件配置 `reverse_draw.candidates`，上限 4）全部出图、全部评分，顶层 `file / seed / quality_*` = 最高分那张（兼容旧字段），`images[]` + `best_idx` 给全部候选，成对评委只对最优那张跑；framing 不在五选一 → `cowboy shot`；提示词仍用 Mac 传来的 `tags`（规则集是实验对象），但路由再跑一遍线上 `_clean_tags` 回传 `plugin_dropped`（线上会多剔的 token）与 `plugin_would_reject`（线上口径 < 5 条，实验室不报错只标）；seed 可固定，`count>1` 每张 seed+i；模型仍 `defaults.model`（不走配额，差异保留）。
  - 插件端（astrbot_plugin_vivian）：`POST /page/lab/reverse/render` 请求体 +`original_width` / `original_height` / `artist_mode` / `count`（十四键），响应 +`size_source` / `artist` / `artist_source` / `images` / `best_idx` / `plugin_dropped` / `plugin_would_reject`；`pool` 时忽略 `artist`，`given` 但串空 → 不注入、`artist_source="none"` 不报错；张与张之间最多 2 并发，240 s 预算到点出了 ≥1 张就返回已有的；`count>1` 文件名 `lab_rerender_<ts>_<i+1>.png`；插件指令行为零变化
  - 后端（Rust）：`client::prepare_image` 另回**缩放前**原图宽高；`RunBatchRequest` 加 `render_artist_mode`（缺省 `pool`）/ `render_count`；`reverse_render_run(run_id, judge_provider_id, artist, render_artist_mode, render_count)`（三个都以本次参数为准，非法 → `bad_request`）；请求体十四键；`RunRow` 加 `render_artist_source / render_size / render_size_source / plugin_dropped / plugin_would_reject / render_candidates`（迁移 006 一并加六列），`render_artist` 改记插件实际注入的串；全部候选另拉到 `labruns/reverse/<run_id>_<idx>.jpg` 回写 `local_path`；老插件缺键照收（六键空 / false）。cargo test lib 101（+3）
  - 前端：`types.ts` / `api.ts` 逐字段追加（`RenderArtistMode`、`RenderCandidate`、`RunBatchRequest` 两键、`RunRow` 六键、`reverseRenderRun` 的 `artist_mode` / `count`）。实验配置重画块改为"画师来源"（画风池随机 / 不带画风；"指定画师"只在画风反推面板用，§12.5 的批次级"画师串（可空）"文本域随之去掉）+ "张数"（插件默认 / 1–4）两个下拉，运行按钮后缀 `· 重画×N`；单题面板显示尺寸与来源（按原图比例 / 按标签）、画师来源徽标 + 串、候选条（缩略图 + `seed · 分数`，绿框 = 最优，点看大图）、"线上 /反推 还会剔掉"chips 与线上会拒的红底提示；单题面板的"重画"沿用实验配置里当前的画师来源 / 张数，画风反推"用它重画" = `artist_mode="given"` + 命中串。`lib/reverse-summary.ts` +11 例、`stores/reverse.test.ts` +1 例

## 0.2.5 — 2026-09-07

- 修复：插件以 2xx 返回 `{status:"error"}`（典型：插件未重载时的"未找到该路由"）被报成"响应里没有 xxx 字段"，现在统一显示插件原话并附"请重载插件"提示。
- 反推题库的本地图片（选择或拖入）先拷进数据目录 `local_images/`，运行详情能显示原图；同一文件重复导入得到同一路径。

## 0.2.4 — 2026-09-07

散文提示词与反推重画对照（CONTRACT §11）：社区图包里 XML 结构 / 代码围栏 / 整段自然语言的"散文"条目由三端同一判定、导出时整块保留、不进随机池；反推实验室可以把每条解析出的标签交给插件按同一角色重画一张并打两个分（重画质量、像原图），散文题与本地图片题不计 P/R/F1 但仍看泄漏与重画对照。vitest 17 文件 212 例（+34）。

- **散文判定（§11.1，三端）**：`strip_artist_block` 剥掉 `<artist>…</artist>` 后，含 `<artwork>`、含 "```"、有独立成行的 XML 标签 `^\s*<[a-z_]+>\s*$`（`<style>` 除外）、或按 `[,\n]` 切分后非空 token 平均词数 > 5，任一即散文（`<style>` 块只去标签、内容照常切分）。Rust `artist::is_prose_prompt`（新文件 `artist/prose.rs`）、插件 `lab_sync.is_prose_prompt`、前端 `lib/prose.ts` 三份实现在上游 1898 条样本上得到**同一份 159 条** id（`<artwork>` 8 / 围栏 74 / XML 行 44 / 长句 33）；契约写的「232 + 29」按 §11.1 字面复现不出（把独立成行的 `<style>` 也算 XML → 238，不剥 `<artist>` → 239，两者都做 → 251），三端一致按字面实现
- **导出与完整池（§11.2，后端）**：新增 `pack.prose` / `FullEntry.prose` / `pack_pool.entries[].prose`（仅 true 时写）。散文条目的 `pack.tags` = 剥画师块后的原文（`<style>` 块与 XML 块原样，非空的 `character_prompts[].prompt` 原文空一行依次接在后面；不切分、不清洗、不剔角色），`t` = title + " · 散文"，永不计 skipped，`artists` / `core` 仍按 `<artist>` 块解析；§9.6 的主标签词汇表只从非散文条目建
- **插件端（§11.3，astrbot_plugin_vivian）**：`lab_run` 随机候选排除散文，`entry_id` 点名散文时 `tags_used` = 原文、`dropped=[]`、`LabPlan.prose=true`（负面与画师掷骰照旧）。新路由 `POST /page/lab/reverse/render`（token 同 §8.3；body `{ tags, framing, character, seed, negative_extra, judge, original_base64, original_mime, judge_provider_id }`）：prompt = `character, person_tag, [framing], tags, defaults.prompt`，**不注入画师**，`size_for_tags`，`seed` 缺省随机并回传，单张同步生成（`sem.at(PRIO_NORMAL)` + `lease_token`）到 `lab_rerender_<ts>.png`，240 s 总预算（出图超时 504，评委各用剩余预算）；`judge=true` 时 `quality_score / quality_notes` = `ag._score_image`（未配评委 / 全失败给 null 而不是保底 5.0），给了原图再跑成对评委（provider = `judge_provider_id` → `reverse_draw.score_provider_ids[0]` → `auto_gallery.score_provider_ids[0]`；A 原图 B 重画，忽略角色外貌差异，只比构图与镜头 / 姿势与手势 / 服装与配饰 / 道具 / 场景与背景 / 光线与氛围，输出 `{"score": 0–10, "reason"}`），评委失败对应字段 null、不影响出图；校验顺序 tags → 角色 → seed → 评委 id → 原图解码，写错不白耗点数
- **后端（§11.4，Rust）**：迁移 `005_reverse_run_render.sql`——`reverse_run` 加 `truth_kind`（`tags | prose | none`）、`render_file / render_local / render_seed / render_prompt`、`quality_score / quality_notes / similarity_score / similarity_reason / render_error`，另追加 `leak_appearance / leak_banned` 两列让没有 `reverse_metric` 行的散文 / 本地题也有泄漏数（老本地图片题回填 `none`，老图包题保留 `tags` 与已算指标），`SCHEMA_VERSION = 5`；`metrics::truth_kind_of`（真值 `COALESCE(raw_tag, tags)` 是散文 → `prose`、metric 为 null）与 `metrics::leaks`（按 `pred_raw`，任何题都算）；`RunBatchRequest` 加 `render / render_character / render_seed / judge_provider_id`（`#[serde(default)]`），`status='ok'` 且 `cleaned_tags` 非空的 run 才重画——新模块 `reverse/render.rs`：调 `/lab/reverse/render`（原图 = 题目图片经 `client::prepare_image` 的同一份字节）→ 写 render_* 列 → 经 `GET /page/thumb?name=&full=1` 把图拉到 `labruns/reverse/<run_id>.jpg`，任一步失败只写 `render_error`、已落库的分数保留，重画结束再发一次 `run_done`；`request_json` 多 `render: { character, seed, judge_provider_id }`；新命令 `reverse_render_run(run_id, judge_provider_id) -> RunRow`（单条重画覆盖旧结果，角色与固定种子沿用批次当时的，失败不抛）；`RunRow` 十一键（`render_path` 是绝对路径）、`ProviderSummary` 加 `tags_n / render_n / quality_avg / similarity_avg`，P/R/F1 只聚合 `truth_kind='tags'` 且有指标的 run
- 前端：`types.ts` / `api.ts` 按 §11.4 逐字段追加（`TruthKind`、`RunRow` 十一键、`ProviderSummary` 四键、`RunBatchRequest` 四键、`reverseRenderRun`；另两个可选键 `LabPlan.prose?` 与 `Question.prose?`，后者是向契约的追加，缺键时 `lib/prose.ts` 从 `truth_preview` 退化判定）。题库：散文条目带"散文"徽标（title 说明命中的规则）、新增"只看标签题"（纯前端过滤、切换不重拉、全选只勾可见的、计数行给散文数）；实验配置新增"重画对照"块——复选"反推后重画并评分（消耗点数）"、角色（缺省 薇拉）、固定种子（非法红框并挡住运行）、成对评委下拉（`plugin_providers()`，空 = 插件默认，不可达栏内灰字不 toast）；结果表汇总行加 题数 / 重画 / 重画质量 / 像原图 四列，逐题行加 散文 / 本地 徽标、泄漏改读顶层 `leak_*`（metric 为 null 时 P/R/F1 画横线仍显示泄漏）、加"重画"一列（`质量 x · 像 y` / 失败 / —，title 给评委理由或错误）；单题面板原图与重画图并排、seed、两个分数与评语 / 理由、`render_error`、"重画"按钮（单条 `reverse_render_run`，返回的行就地换进结果表与详情再静默重拉汇总）、散文题提示"散文题：不计 P/R/F1，只看重画对照与泄漏"、"重画 prompt"折叠；画图实验室结果卡对 `LabPlan.prose` 标"散文"。`lib/reverse-summary.ts` 纯函数 + 单测 12 例，`lib/prose.test.ts` 15 例（样本存在时与 Rust / 插件的 id 列表对拍），`stores/reverse.test.ts` 新增 7 例

## 0.2.3 — 2026-09-07

反推实验室用插件里的模型 + 拖图进题库（CONTRACT §10）：连上插件后，反推模型可以直接选 AstrBot 里配置好的 provider，不必在 Mac 上再填 base_url / key；题库支持把图片文件直接拖进窗口。vitest 15 文件 178 例（+33）。

- **插件里的模型（§10.1）**：`reverse_provider` 表加 `kind`（`openai` | `plugin`，默认 `openai`）与 `plugin_provider_id`（迁移 `004_reverse_provider_kind.sql`）；`ReverseProvider` / `ReverseProviderInput` 追加同名两键（`#[serde(default)]`，老客户端不传即 `openai`；`plugin` 时 `base_url` / `model` 存空串、`has_key` 恒 true）。新命令 `plugin_providers()`：GET `{PLUGIN_PAGE_PATH}/providers`（JWT，无需 token）返回 provider id 列表，不可达 → `network`。批次运行时 `kind='plugin'` 的模型走插件 `POST /page/lab/reverse`（body `{ provider_id, prompt, image_base64, mime, max_tokens }` → `{ text, latency_ms, provider_id }`），图片同样经 `client::prepare_image` 压到 ≤ 9 MB，解析 / 清洗 / 指标仍在 Mac 端；`request_json` 里 `model` 记 `plugin:<provider_id>`、`base_url` 记插件地址，取消令牌同样生效
- 插件端（astrbot_plugin_vivian）：新路由 `POST /page/lab/reverse`（JWT + `X-Library-Token`）——`provider_id` 须在 `context.get_all_providers()` 里否则 400，`image_base64` 解码后 ≤ 12 MB 否则 413；图片落 `lab/tmp/<uuid>.<ext>`，调 `prov.text_chat(prompt, image_urls=[path])`，120 s 超时，完成或异常都删临时文件；返回 `completion_text` 原文，不判拒答、不解析；一行 `logger.info`，不入 runs 记录
- 前端：模型管理对话框分上下两栏——"插件里的模型"（打开时 `plugin_providers()`，可刷新；每个 provider 一键"启用为反推模型"，已加入的显示启用开关与删除；AstrBot 里已删掉但本地仍有记录的标"插件里已没有这个 provider"，仍可停用 / 删除；插件不可达时栏内灰字给原因，不弹 toast）与"本地 OpenAI 兼容模型"（原表单，恒 `kind=openai`）；实验配置的模型列表带 插件 / 本地 徽标。`lib/provider.ts` 纯函数 + 单测 12 例
- **拖图进题库（§10.3）**：`QuestionBank.vue` 订阅 Tauri webview 的 drag-drop 事件（`getCurrentWebview().onDragDropEvent`，题库挂载时订阅、卸载时退订，其它页面丢文件不做任何事）：拖到窗口上题库面板高亮并显示"松开：加为本地图片题"，松开后 png / jpg / jpeg / webp / gif（大小写不敏感）逐个走"添加本地图片"同一条路（`addLocalImagePath`，重复路径只勾选不重复加），其余忽略并提示一次"只接受图片文件"。`lib/dnd.ts` 纯函数 + 单测 13 例，`stores/reverse.ts` 新增单测 8 例

## 0.2.2 — 2026-09-07

- 修复：开发模式下每次访问钥匙串都弹授权框。钥匙串读取加进程内缓存，每个条目每次启动最多读一次；保存/删除同步更新缓存。根因是未签名的开发版二进制每次重编译都被 macOS 当成新程序，"始终允许"记不住。

## 0.2.1 — 2026-09-07

- 修复：AstrBot 4.27 起登录改为服务器端哈希，桌面端此前永远发 md5 导致"用户名或密码错误"。现在先发明文、401 再退回 md5，两代都能登（CONTRACT §6 裁定更新）。

## 0.2.0 — 2026-09-07

画图实验室（CONTRACT §9）：出图、提示词组装、清洗、评分全在插件里跑（与插件指令出图同一套函数），Mac 端只负责选图包、下参数、看结果、打分；运行记录留在插件数据目录，桌面端镜像一份。vitest 12 文件 145 例（+34）；cargo test 62（+8）。

- **事实修正（§9.0）**：插件 `/画` 指令**没有图包路径**——它是 LLM 主题系统（构图 / 服装 / 场景骰子）。画图室的"按图包画"因此是一条新路径；带自然语言描述且未选图包时才走 `/画` 现有 LLM 路径。这条裁定不改动插件指令的行为。画图室只用 safe 图包，标签恒经清洗
- 插件端（astrbot_plugin_vivian）：新模块 `src/lab_run.py`——`compose`（§9.2 规则 1–8：插件 `/画` 指令同款角色切分与 `风格=`、图包来源 `lab_sync.full_entries(... decision="include")` + `scope` 前缀随机 / `entry_id` 点名、`entry.pack.tags` + `scrub_entry` 清洗、`desc` 经 `_build_prompt`、画风池按 `artist_ratio` 掷骰、`character, person_tag, tags_used[, extra_tags], defaults.prompt` 拼装）与 `start_run`（逐张 `sem.at(PRIO_NORMAL)` + `lease_token()` + `wrapped_generate(owner_id="vva_lab")`，最多 2 张并发，单张失败只标记该张，`score=true` 跑三评委；同一时刻只允许一个实验室任务 → 409；后台任务登记进 `plugin._background_tasks`）；记录 `lab/runs/<run_id>.json` + `index.jsonl`；六条路由 `POST /page/lab/run/preview`、`POST /page/lab/run/start`、`GET /page/lab/run/status`、`GET /page/lab/run/list`、`GET /page/lab/run/image`、`POST /page/lab/run/rate`（JWT 之后核对 `X-Library-Token`，GET 也接受 `?token=`）；管理页"桌面端对接"下加"实验室最近运行"只读列表（最近 10 条）
- 后端（Rust）：`models.rs` 按 §9.1 逐字段加 `LabRunRequest / LabPlan / LabImage`（flatten，JSON 扁平）`/ LabRun / LabRunSummary / LabRunStart`；插件传输层从 `export.rs` 抽到 `plugin.rs`；新模块 `lab.rs` + 六个命令 `lab_run_preview / lab_run_start / lab_run_status / lab_run_list / lab_run_image / lab_run_rate`（不可达 → `network`；`count` 1–8、`artist_ratio` 0–1 联网前校验）；本地表 `lab_run(run_id PK, mode, created_at, status, json)`（迁移 `003_lab_run.sql`，`lab_run_status` 拿到 `done|error` 时整份落盘，`lab_run_list` 本地 ∪ 插件、本地优先、`created_at` 倒序）；图片缓存 `labruns/<run_id>/<idx>[.thumb].png`（首次从插件拉 `data_base64` 落盘，之后直接本地，返回相对数据目录的路径）；评分先写插件、成功后改本地镜像
- 前端：新视图 `/lab/draw` 画图实验室（侧栏排在反推实验室之后）三栏——左栏图包树（本地 `nai5_community_pack`、已加入、只含 safe；勾目录 = `scope` 最小前缀、点条目 = `entry_id`、顶部"随机"、关键词筛）；中栏角色（文本框 + 最近用过的 datalist，默认 薇拉）+ 描述 + `风格=` 拼成 `text`、画风池命中概率滑块（默认 50%）、张数 1–8（默认 1）、评分开关（默认开）、固定种子、预览 / 出图；右栏结果卡（缩略图点开全图、最终 prompt 可复制、画师及来源徽标、seed、图包标题 / 路径、`tags_raw` vs `tags_used` 的清洗 diff、评分与评语、收藏 / 点踩乐观更新失败回滚）+ 历史运行（`lab_run_list(30)` 按模式过滤，点开回看，未到终态继续轮询）。`lab_run_status` 每 2 s 轮询到 `done|error`；表单按模式存 `codexlab.lab.<mode>`，回到页面自动回看上次的 run。`types.ts` / `api.ts` 按 §9.1 / §9.4 追加，`lib/lab.ts` 纯函数（树 / scope / diff / text / 默认请求 / 表单持久化）+ 单测 34 例

## 0.1.4 — 2026-09-07

桌面端 ⇄ 插件"对接系统"第一版（CONTRACT §8）：完整池导出与推送、设置页插件持有状态面板；插件端新增 `lab_sync` 模块承接。vitest 111。

- 完整池 `full_pool.json`（`schema = nai5-codex-lab/full/1`）：两本词典全部条目（含未加入 / 未决策 / 黑名单命中），每条带 `decision / decided_at / member_note / blacklisted / artists / core / pack / images`；codex 顺序画师词典在前、图包在后，条目按 `book_order`；rating 规则同筛选池（只出 safe）；黑名单只标记不剔除；`embedding` 预留为 null
- 新命令 `export_full_pool`（写 `exports/<时间戳>/full_pool.json`）与 `push_full_to_plugin`（内存构建直推 `/page/library/import_full`，登录与 401 重登同筛选池，返回按 codex 的 total / include / exclude / undecided / blacklisted 与备份名）、`plugin_library_status`（GET `/page/library/status`；连不上 / 超时 / 未配密码或 token 返回 `reachable=false` + 原因，不抛错；401/403 才 `plugin_rejected`）
- 设置页："导出与推送"分成筛选池（原两个按钮）与完整池（"导出完整池" / "推送完整池"）两行；完整池推送结果按 codex 列出五项计数与合计。新增"插件当前持有"面板：进入设置页自动查一次、可刷新、任一推送成功后自动刷新；筛选池 / 完整池各一张卡（导入时间 / release / 导出时间 / 桌面端版本 / 计数 / 文件大小 / 备份数），不可达时面板内灰字给原因、不弹错误
- 插件端（astrbot_plugin_vivian）：新模块 `src/lab_sync.py`，`web_page.py` 只做注册；`POST /page/library/import_full`（dashboard JWT + `X-Library-Token`；校验 schema 前缀 / `codexes` 数组 / `entry_id` `title` 非空 / `tags` 字符串 / `decision` 三值，最多 20 条错误合并 400；原子写、旧文件 `.bak-<ts>`、只留最近 5 份）、`GET /page/library/status`；原 `import` 成功后写 `filtered.meta.json`；文件存插件数据目录 `lab/`（不入库），`library_export.json` 迁到同处；读取接口 `full_pool()` / `full_entries()` / `status()` 给未来功能；管理页新增"桌面端对接"板块。/画 /反推 行为不变
- 前端：`types.ts` 新增 `FullCounts` / `PushFullResult` / `PoolStatus` / `PluginLibraryStatus`（§8.2 逐字段），`api.ts` 三个封装，`lib/pool.ts` 纯函数（codex 中文名与顺序、计数求和与摘要行）+ 单测 8 例

## 0.1.3 — 2026-09-06

看图体验三件：整页缩放、卡片尺寸档位、按住看大图。

- 整页缩放：`⌘=`/`⌘+` 放大、`⌘-` 缩小、`⌘0` 复位，每档 10%，70%–200%，记在本地；侧栏显示当前比例（`src/lib/zoom.ts`，CSS zoom）
- 卡片尺寸：小 / 中 / 大 / 特大（200 / 240 / 320 / 400 px），`⌘⇧=`/`⌘⇧-` 切档，大档改用展示图而非缩略图，换档保持焦点条目可见；全局偏好 `codexlab.cardSize`（`stores/prefs.ts`）
- 按住看大图：卡片上按住左键 250 ms 全窗口覆盖显示展示图（半透明压暗），松开或 Esc 回到网格，长按后不触发三态切换；移动超 6 px 或带修饰键不触发（`src/lib/peek.ts` 状态机 + `components/PeekOverlay.vue`）

## 0.1.2 — 2026-09-06

用户 Mac 端验收通过后的第一批需求：显式翻页 + 可选排序，能回到上次筛选到的位置。cargo test 50，vitest 74。

- 词典页：滚到底加载改为分页（每页 50/100/200、上一页/下一页、页码跳转、`⌘←`/`⌘→`）；页码、每页条数、排序、方向与筛选一起持久化，重开回到上次那页；筛选变化页码归 1，换每页条数按当前位置换算页码，越界回最后一页
- 排序：书序 / 标题 / 决策时间 / 同步时间（主图落地时间，未下载排最后）/ 首次出现（release 发布时间），可切升降序（CONTRACT §7）
- 后端：`list_entries` 支持 `sort` 五键与 `sort_dir`，NULL 恒排最后，`book_order` 稳定键，未知值 bad_request

## 0.1.1 — 2026-09-06

代码导览审读（docs/CODE_TOUR_*.md §7）后的修复版。cargo test 46，vitest 58，均通过；联网冒烟通过。

- 前端：快速切书不再停在旧书（请求序号）；侧栏/树/筛选栏三处计数在决策与同步后一致刷新；同步或反推的结束事件丢失时轮询兜底，按钮不再永久禁用；「本次新增」筛选不再粘住旧 release；批量按钮后焦点回网格；失败清单显示错误分类；删除未生效的 `@/` 别名；tagcloud 死分支；乐观更新回滚改为按条反向计数；新增 stores/codex 单测 8 例
- 后端：下载路径统一 AppError 并给 `SyncFailure.code`（network/upstream/io/checksum/cancelled）；同一图片的同步与按需下载互斥（在飞集合）；manifest 阶段取消立即生效（`select!`）；登录密码永远 md5；推送不再每次落盘 exports/；钥匙串调用改 `spawn_blocking`；`reasoning_content` 兜底时记 warn 并标注；批次版本号按数字段比较；`dup_count` 改为 GROUP BY 派生表；迁移按版本包事务；每日备份延后 5 s 异步执行
- 文档：CODE_TOUR_BACKEND.md / CODE_TOUR_FRONTEND.md；CONTRACT §6 密码条目更新

## 0.1.0 — 2026-09-06

首个可构建版本。`cargo check`/`cargo test`（41）与 `vue-tsc`/`vitest`（50）/`vite build` 全过；Mac 端未实机运行。

- 同步：法典图鉴 release 链（sha256 校验、按书跳过、条目版本化、图片 ETag 续传与校验、缩略图、断点补缺）
- 词典页：画师词典 / 精选图包，虚拟网格，三态决策（加入微绿 / 不加入微黑），筛选（目录树、rating、批次、本次新增、决策、关键词、多图、重复组），详情抽屉，键盘操作，撤销
- 导出：`nai5_artist_pool.json`、`pack_pool.json`、`library_export.json`；推送到插件（WebUI JWT + 共享密钥）
- 反推实验室：模板 / 规则集 / 模型端点版本化，题库（条目 × 图片），批次运行，P/R/F1 与泄漏指标，批次对比，人工打分
- 联网冒烟 `LIVE_SMOKE=1 cargo test --test live_smoke`：对真实 release（r-79767c5a…）跑完 data-source → manifest → 两本书 sha256 校验 → upsert 3216 条 / 4401 图 → 队列 3216 张，实下 4 张含缩略图与 ETag，全部通过
- 日志：tracing 按天文件 + `app_event`、`sync_log`、`membership_audit`；每日与同步前 `VACUUM INTO` 备份
