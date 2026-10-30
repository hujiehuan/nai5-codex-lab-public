# 架构说明

这里记的是我怎么分层、几个关键机制怎么做、测试放在哪。接口细节以 [`CONTRACT.md`](CONTRACT.md) 为准，数据库迁移见 [`docs/MIGRATIONS.md`](docs/MIGRATIONS.md)，编码器的验收口径见 [`docs/STYLE_BENCHMARK.md`](docs/STYLE_BENCHMARK.md)。

## 模块地图

```
Vue 3 + Pinia + Naive UI（src/）
  views/      SyncView 同步 · CodexView 风格词典 / 图包 · StudioView 总实验室 · EvalView 反推评测 · SettingsView
  stores/ lib/  状态与纯函数（配方解析、分页查询、指标汇总……），各自旁边放 *.test.ts
  ipc.ts      唯一的 invoke 出口；参数在边界做 JSON 快照，mock 模式下改走 /__mock__/invoke
        │  invoke() + Channel<T> 进度事件
        ▼
Tauri 2 命令层（src-tauri/src/commands/）
  只做参数校验、取 State，然后调用领域模块；同步的数据库操作放进 spawn_blocking，长任务后台启动后立即返回
        ▼
领域模块（src-tauri/src/）
  sync/       上游 release 链 → sha256 校验 → 条目 upsert → 图片队列（续传）→ 缩略图 → 向量索引
  db.rs       SQLite 连接、逐级迁移、VACUUM INTO 快照
  library.rs  分页查询、三态决策、撤销、排除名单
  export.rs   本地导出（风格池 / 图包池 / 完整池），筛选池推送
  plugin.rs   插件 HTTP 传输层：JWT 登录、401 重登、X-Library-Token
  reverse/    反推评测：模板 / 规则集 / 端点、批次调度、清洗、指标、重画对照
  style/      candle 推理（CLIP / CSD / MegaStyle）、向量存储、减均值余弦检索、权重下载
  studio/ creative.rs  总实验室：冻结输入、方案、运行记录、skill 快照
  secrets.rs  macOS 钥匙串
  updater.rs update_installer.rs  从固定仓库的 vX.Y.Z tag 构建并替换 .app
```

依赖只朝下走：Vue 不知道文件系统，命令层不写业务逻辑，领域模块之间直接调用普通函数，不经过 IPC。`AppState`（`state.rs`）持有数据库连接、HTTP 客户端、节流器和各类后台任务的取消令牌。

## 契约优先

前后端是不同的会话并行写的，所以我先写 `CONTRACT.md`，再写两边的代码。约定不多，两边都严格遵守：

- 命令名、参数名、DTO 字段一律 `snake_case`。Rust 侧每个命令都写 `#[tauri::command(rename_all = "snake_case")]`，serde 不做 rename。
- `src-tauri/src/models.rs` 和 `src/types.ts`（以及 `types-creative.ts`、`types-studio.ts`）逐字段对应。要改数据形状，顺序固定：先改契约，再改两边。
- 错误统一为 `AppError { code, message }`。前端按 `code` 分支（`sync_running`、`bad_request`、`network`、`plugin_rejected`……），不解析中文文案。
- 长任务用 `tauri::ipc::Channel<T>` 推进度，`done` 和 `error` 各只发一次。

`mock/` 按同一份契约实现了一个假的 IPC 后端。`npm run dev:mock` 不启动 Rust，也能在浏览器里跑整个界面；e2e 测试也跑在它上面。

## 存储：SQLite WAL、逐级迁移、快照

数据只有一个文件 `library.db`，开 `foreign_keys=ON`、`journal_mode=WAL`、`synchronous=NORMAL`。连接放在互斥锁里，只短时持有。

迁移是 `src-tauri/migrations/` 下 8 个按版本号编号的 SQL 文件。启动时按 `schema_version` 逐版执行，每一版一个事务，失败整版回滚。已发布的迁移不再修改。

人工决策单独存在 `library_membership` 和 `membership_audit`，和上游条目分表，重新同步不会覆盖。

同步拉取新 release 前、每天一次、安装更新前，各用 `VACUUM INTO` 写一份一致的快照：先写 `.tmp` 再改名，按份数轮换。升级前后用 `scripts/check-library.py` 做对比，它以只读方式打开数据库，生成决策 / 备注 / 审计的摘要。

## 断点续传同步

上游是一个按 release 发布的静态 JSON 站点。一次同步分几个阶段，进度通过 `SyncEvent` 推给前端：

1. release 链：`data-source → current → manifest`。每个 JSON 按 manifest 里的 size 和 sha256 校验后，原文落盘到 `codex_raw/<release>/`；书的 `file_sha256` 没变就跳过解析。release 没变且不强制时，只做图片补缺。
2. 条目 upsert：条目按 `entry_id` 版本化，变化写历史表。上游消失的条目只打 `removed_at`；消失比例过高时不下架，只记警告。全部写完才更新 `latest_release`。
3. 图片队列：先写 `<file>.part`，并把第一次拿到的 ETag 存进旁车文件 `.part.etag`。中断后续传时带 `Range: bytes=<已有长度>-` 和 `If-Range: <那个 ETag>`：服务器回 206 就追加，回 200（说明文件已变）就从头重写。下完后，如果 ETag 是 MD5，就和文件内容核对，通过才 `rename` 成正式文件，再把哈希、字节数和路径写进 `entry_image`。
4. 向量索引：图片阶段之后，给缺向量或图片哈希已变的展示图补算向量。模型没下载就跳过这一阶段，不算同步失败。

请求上游资源主机时，全局并发 ≤ 4，相邻请求间隔 ≥ 100 ms；单文件 60 秒超时，失败按 2 / 4 / 8 秒退避重试 3 次。同一张图同一时刻只允许一个任务写（在飞集合），所以主同步和按需下载不会写同一个 `.part`。

## 反推评测流水线

反推评测页（`/eval`）跑的是可复现的批次实验：题目 × 模型端点。

题目有两种：图鉴里的条目图片，真值是该图的原始标签加角色提示词；或者拖进来的本地图片，没有真值，只看泄漏和重画。

Rust 直接请求 OpenAI 兼容的 `chat/completions` 端点，`temperature=0`，API key 存在钥匙串的 `provider:<id>` 下。每个端点内逐题串行，多个端点之间并行。原始响应全文、请求参数和延迟都落库。超过 9 MB 的图先缩放再发送。

解析时从响应里取第一个 JSON 对象，取不到记为 error，模型拒答记为 refused。清洗规则集（长度、词数、禁用词、外貌词正则等）带版本号；和提示词模板一样，同名保存会生成新版本，不覆盖旧版本，方便对照。

指标在 `reverse/metrics.rs`。标签归一后，真值先剔除画师 / 质量 / 负面 / 年份 / `rating:*`，再把角色名、外貌词、人数词等「预期不该答出」的词移出分母。`precision = hit/|pred|`，`recall = hit/|truth_eval|`（分母为 0 时为 null），F1 照常计算。另外记 extra、外貌 / 禁用词泄漏、与负面提示词的矛盾数。

汇总表每个模型一行：P / R / F1（只聚合有标签真值的题）、平均延迟、拒答和错误数。两个批次可以按题对齐，比较 F1 差值。

重画和评分是可选的：把清洗后的标签交给插件，按相同的角色、尺寸规则、种子和张数重画；评分模型给出质量分，并把原图和重画图成对比较，给出 0–10 的「像原图」分。单条结果也可以人工打分。

同一时刻只有一个活动批次。停止后不再提交新请求，已落库的结果保留，批次终态记为 `cancelled`。

## 画风索引与检索

三个编码器我都用 candle 重新实现了（`style/model.rs`、`csd.rs`、`megastyle.rs`），沿用各自原训练时的预处理，输出 L2 归一化的向量（512 / 768 / 1152 维）。Apple Silicon 上默认用 Metal，失败自动退回 CPU 并显示原因。CSD 和 MegaStyle 有与官方参考实现逐值对拍的测试。

权重不进仓库。`style/catalog.rs` 里登记了 Hugging Face 固定 revision 的下载地址、字节数和 SHA256；下载同样走 `.part` 续传，校验通过才算完成。模型 ID 只能从这张表里选，不接受任意路径或 URL。

索引覆盖风格词典里每条配方的展示图，按模型 ID 分开存在 `image_embedding` 表里（f32 小端 BLOB，附图片哈希，图片变了就重算）。

检索时，查询图经 CLIP / CSD / MegaStyle 编码，减库均值后按余弦取 top-k，返回最相近的风格配方。之后用相同的反推标签、种子和张数重画，由评审模型成对打分，以此量化检索对最终效果的贡献。库均值按模型缓存在 `app_state`，索引变化时重算。排除名单里的配方永远不会返回。

总实验室的「画风复审」走另一条不写索引的路径（`style/compare.rs`）：2–32 张本地图片先冻结字节和哈希，再计算全部无序图对的原始余弦，按人工分组标签算准确率 / 精确率 / 召回率。

## 密钥与插件

WebUI 密码、库令牌和各模型端点的 API key 只存在 macOS 钥匙串（service `nai5-codex-lab`），设置文件、数据库和日志里都没有。account 名有白名单。未签名的开发版每次重编译都会让钥匙串重新弹授权框，所以我加了进程内缓存，每个条目每次启动最多读一次。

插件名可配置：`Settings.plugin_name`，缺省 `astrbot_plugin_codexlab`，校验规则 `^[A-Za-z0-9_]{1,64}$`。所有插件页面路由都由它拼成 `/api/plug/{plugin_name}/page/...`，代码里不硬编码插件名，测试里的路由期望也由测试常量拼出。

传输层（`plugin.rs`）在进程内复用 JWT：并发的首次登录会合并，401 只重登一次，登录失败后短暂冷却。插件不可达时，状态类查询返回 `reachable=false`，不当作空数据。

同步、素材库、本地导出、画风索引和反推评测都不依赖插件。出图、重画评分和推送筛选池需要一个实现了契约里插件路由的插件，插件本身不在这个仓库里。

## 测试布局

| 位置 | 运行 | 内容 |
|---|---|---|
| `src-tauri/src/**` 内的 `#[cfg(test)]` | `cd src-tauri && cargo test` | 单元与集成测试：迁移、续传、清洗与指标、检索、实验冻结、更新器状态机等。需要真实权重的对拍测试默认 ignored |
| `src-tauri/tests/` | 同上 | 插件输出形状的夹具回归；`live_smoke` 访问真实上游，只在设置 `LIVE_SMOKE=1` 时运行 |
| `src/**/*.test.ts` | `npm test`（vitest） | store、纯函数和 IPC 层，与源文件放在一起 |
| `tests/e2e/` | `npm run test:browser` / `npm run test:e2e` | puppeteer-core 驱动无头 Chrome，跑在 `mock/` 假后端上的界面流程与版面检查；`test:browser` 是日常的三项冒烟，`test:e2e` 跑全部 |
| `scripts/test-library-check.py` | `python3 scripts/test-library-check.py` | 只读校验脚本不会修改数据库 |

所有测试夹具都是合成数据；测试不调用真实的计费模型，也不发起真实出图请求。
