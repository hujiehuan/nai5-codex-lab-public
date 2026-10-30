# NAI5 Codex Lab

macOS 桌面应用：在本地整理 NovelAI V5 的提示词参考库（风格配方与场景图包），并做可复现的提示词对比实验。

[English](#english)

我从 2026 年 9 月开始写这个工具，已经每天在用两个月。自用的本地库里大约有 3.9 万条图鉴条目和 4.1 万个画风向量。后端是 Rust（Tauri 2），前端是 Vue 3，数据存在本地 SQLite。

这是从私有仓库整理出来的公开版，git 历史是新的，不含私人配置和真实的上游数据。

## 能做什么

- 从公开图鉴站点同步风格词典和精选图包，图片一起存到本地。每条记录可以标成「加入 / 不加入 / 未定」，支持键盘操作、批量选择和撤销；决策单独存一张表，重新同步不会丢。筛选结果（风格池、图包池、完整池）可以导出成 JSON。
- 反推评测（`/eval`）：同一批图片交给多个视觉 LLM 反推提示词，按标签算精确率、召回率和 F1，同时记录延迟和拒答。两个批次可以逐题对比。反推结果可以拿去重画，再由评分模型打质量分和「像原图」分，也可以人工打分。
- 画风检索：查询图用 CLIP / CSD / MegaStyle 编码，减去库均值后按余弦取 top-k 个最相近的风格配方。再用同样的反推标签、种子和张数重画，让评审模型成对打分，就能看出检索对最终效果有没有帮助。编码器在本机跑，Apple Silicon 上用 Metal，不行就退回 CPU。CSD 和 MegaStyle 的 Rust 实现和官方实现做过逐值对拍。
- 总实验室：一轮实验里冻结输入，用来对照不同的提示词模型和 skill 组合。有三种模式：/画（参考精选图包先出构思再出图）、反推（同一张原图换模型和模板反推，再统一重画评分）、画风复审（对比视觉 LLM 的批内排序和本地编码器给出的相似度）。出图只用不额外计费的参数档位（3 个尺寸、最多 28 步），每张图都做预算检查。
- 软件更新：从本仓库的稳定 tag 拉源码在本地构建，换掉旧应用前先备份数据库。

## 实现上的几个决定

前后端的接口写在 [`CONTRACT.md`](CONTRACT.md) 里：每个命令的参数、DTO 和错误码。Rust 的 `models.rs` 和 TypeScript 的 `types.ts` 逐字段对应。`mock/` 按同一份契约实现了一个假后端，不启动 Rust 也能开发和测试界面。

SQLite 开了 WAL。8 个迁移按版本号依次执行，每个版本一个事务，失败就整版回滚。拉取新的上游版本前、每天一次、安装更新前，各用 `VACUUM INTO` 留一份快照，因为库里的决策都是一条条手工标出来的，丢了没法重来。细节见 [`docs/MIGRATIONS.md`](docs/MIGRATIONS.md)。

图片多，一次同步可能下不完。所以图片先写到 `.part` 文件，中断后用 `Range` 加第一次拿到的 ETag（`If-Range`）接着下，校验通过才改名入库。

密码和 API key 只存在 macOS 钥匙串里。模型权重不放进仓库，第一次用到时从 HuggingFace 的固定 revision 下载并校验 SHA-256（`src-tauri/src/style/catalog.rs`）。编码器是怎么选的见 [`docs/STYLE_BENCHMARK.md`](docs/STYLE_BENCHMARK.md)，模块划分和数据流见 [`ARCHITECTURE.md`](ARCHITECTURE.md)。

## 规模

| 项目 | 数量 |
|---|---|
| Tauri IPC 命令（`#[tauri::command]`） | 95 |
| SQLite schema 迁移 | 8 |
| Rust 测试（`cargo test`） | 241 通过，6 个默认忽略（4 个需要插件生成的协议夹具，2 个是需要本地模型权重的 CSD / MegaStyle 对拍） |
| 前端测试（vitest） | 389 通过，1 个跳过，共 32 个测试文件 |

## 构建和测试

需要 macOS、Xcode Command Line Tools、Rust stable 和 Node.js 22 以上。浏览器端测试还要装 Chrome（或者用 `CHROME_BIN` 指定路径）。

```bash
npm ci
npm run typecheck          # vue-tsc
npm test                   # vitest
npm run test:browser       # 无头 Chrome 跑 mock 界面的冒烟测试
npm run test:e2e           # tests/e2e/ 下的全部浏览器测试
cd src-tauri && cargo test
```

```bash
npm run dev:mock           # 浏览器里用离线假数据跑，不发真实请求
npm run tauri dev          # 原生开发窗口
npm run tauri build        # 打包，.app 在 src-tauri/target/release/bundle/macos/
```

`tauri build` 最后打 DMG 那一步在有些机器上会失败，不影响，`.app` 那时已经生成了。

同步、素材库、导出、画风检索和反推评测都不依赖插件。评测用的模型端点（OpenAI 兼容地址、模型名、API key）在评测页里填，key 存进钥匙串。构思、出图和重画评分要接一个实现了 `CONTRACT.md` 里插件路由的 AstrBot 插件，插件不在这个仓库里。在「设置」里填 WebUI 地址（例如 `http://127.0.0.1:6185`）、用户名和插件名（`plugin_name`，默认 `astrbot_plugin_codexlab`），密码和库令牌存进钥匙串。

数据都放在应用数据目录（Tauri `app_data_dir`）：`library.db`、图片、缩略图、模型权重、导出文件、备份和日志。仓库里没有上游图鉴数据和图片，测试夹具都是合成的。

## 关于 AI 辅助

代码是用 Codex 和 Claude Code 分模块写的，方案设计、代码审查和测试验收由我负责。

## 许可

应用代码用 MIT 许可证，见 [`LICENSE`](LICENSE)。移植的第三方代码和运行时下载的模型见 [`THIRD_PARTY_NOTICES.md`](THIRD_PARTY_NOTICES.md)。

---

## English

A macOS desktop app for curating a local NovelAI V5 prompt reference library (style recipes and scene packs) and running reproducible prompt-comparison experiments.

I started it in September 2026 and have used it daily for two months; my own library holds about 39k catalogue entries and 41k style vectors. The backend is Rust (Tauri 2), the frontend Vue 3, and data lives in a local SQLite database. This public version was extracted from a private repository with fresh history and contains no private configuration or real upstream data.

What it does:

- Syncs a style dictionary and curated scene packs, images included, from a public catalogue site. Entries are triaged as include / exclude / undecided (keyboard shortcuts, batch selection, undo); decisions live in their own table, so re-syncs keep them. The curated pools can be exported as JSON.
- Reverse-prompt evaluation (`/eval`): the same images go to several vision LLMs, the returned prompts are scored tag by tag (precision / recall / F1), and latency and refusals are recorded. Two batches can be compared question by question. Results can be redrawn and rated by a judge model or by hand.
- Style retrieval: the query image is encoded with CLIP / CSD / MegaStyle, the library mean is subtracted, and the cosine top-k gives the closest style recipes. Redrawing with the same tags, seed and image count and having a judge model compare the pairs shows whether retrieval helped. Encoders run on-device (Metal on Apple Silicon, CPU fallback); the Rust ports of CSD and MegaStyle are checked value by value against the official implementations.
- Studio: freezes the inputs of each round to compare prompt models and skill combinations, in three modes (draw, reverse, style review). Rendering only uses parameter tiers with no extra charge (three sizes, at most 28 steps), with a budget check for every image.
- Self-update: builds a stable tag of this repository locally and backs up the database before swapping the app.

Implementation notes: the IPC contract is in [`CONTRACT.md`](CONTRACT.md), with `models.rs` and `types.ts` matching field by field and `mock/` implementing the same contract so the UI runs without the backend. SQLite runs in WAL mode; eight numbered migrations run one transaction per version, and `VACUUM INTO` snapshots are taken before applying a new upstream release, daily, and before installing an update ([`docs/MIGRATIONS.md`](docs/MIGRATIONS.md)). Image downloads go to `.part` files and resume with `Range` plus the first ETag (`If-Range`). Passwords and API keys stay in the macOS Keychain; model weights are downloaded on demand from pinned HuggingFace revisions and checked by SHA-256. [`ARCHITECTURE.md`](ARCHITECTURE.md) (Chinese) has the module map.

Size: 95 Tauri IPC commands, 8 SQLite schema migrations, 241 passing Rust tests (6 ignored by default: 4 need plugin-generated wire fixtures, 2 are CSD / MegaStyle parity checks that need local weights), 389 passing frontend tests (1 skipped, 32 files).

Build and test (macOS, Xcode Command Line Tools, stable Rust, Node.js 22+; Chrome for browser tests):

```bash
npm ci
npm run typecheck
npm test                   # vitest
npm run test:browser       # headless Chrome smoke tests against the mock backend
npm run test:e2e           # every browser test in tests/e2e/
cd src-tauri && cargo test
npm run dev:mock           # UI with offline fake data
npm run tauri build        # .app under src-tauri/target/release/bundle/macos/
```

The DMG step at the end of `tauri build` fails on some machines; the `.app` is already built by then.

Sync, curation, export, style retrieval and evaluation work without the plugin; evaluation endpoints are configured on the evaluation page. Planning, rendering and redraw scoring need an AstrBot plugin implementing the plugin routes in `CONTRACT.md` (not part of this repository); set its WebUI address, user name and `plugin_name` (default `astrbot_plugin_codexlab`) in Settings.

The code was written module by module with Codex and Claude Code; I did the design, code review and test acceptance.

App code is MIT ([`LICENSE`](LICENSE)); see [`THIRD_PARTY_NOTICES.md`](THIRD_PARTY_NOTICES.md) for ported code and downloaded models.
