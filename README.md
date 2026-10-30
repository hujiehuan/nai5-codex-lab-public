# NAI5 Codex Lab

[中文](#中文) · [English](#english)

---

## 中文

macOS 桌面应用，用来管理 NovelAI V5 图像生成的参考素材（画师串、图包），并做可复现的提示词对比实验。
Rust 后端通过 Tauri IPC 给 Vue 3 前端提供服务，数据存在本地 SQLite。我从 2026-09 开始写，已经日常自用两个月。
本地库目前大约有 3.9 万条图鉴条目、4.3 万张图片和 4.1 万个画风向量。

> 这是从私有仓库整理出来的公开版本，使用全新的 git 历史，不含私人配置和真实上游数据。

### 功能

- **图鉴同步**：从公开图鉴站点拉取 V5 画师词典、社区图包和 NAX 画师画廊，连同图片存进本地库。支持断点续传，每次同步前自动做数据库快照。
- **素材库**：每条记录可以标成「加入 / 不加入 / 未定」三种状态，支持键盘操作、批量选择和撤销。决策单独存表，重新同步不会丢。
- **导出 / 推送**：把筛选后的画师池、图包池和完整池导出成 JSON，或者通过 AstrBot WebUI API 推送给配套的插件。
- **总实验室**：有三种模式，都在同一个工作区里，每一轮会冻结输入，方便对照提示词模型和 skill 组合。
  - **/画**：参考精选图包，先生成构思方案，再出图。
  - **反推评测**：同一张原图交给多个视觉 LLM 识图，按标签算精确率 / 召回率 / F1 并记录延迟；可选重画，由评分模型给重画结果打分。
  - **画风复审**：同一组图片，比较视觉模型和本地编码器的判断。
- **本地画风向量**：用 CLIP / CSD / MegaStyle 编码器在本机算向量，Apple Silicon 上走 Metal，也能退回 CPU。检索时先减去库均值，再按余弦相似度取 Top-K。CSD 有和官方实现逐值对拍的测试。
- **软件更新**：从本仓库的稳定 tag 拉源码，本地构建后替换应用，替换前会先备份数据库。

### 架构

```
Vue 3 + Pinia + Naive UI (src/)
        │  invoke() / 事件            ← 契约：CONTRACT.md（命令 / DTO / 事件，前后端以它为准）
        ▼
Tauri 2 命令层 (src-tauri/src/commands/)   薄层：只做参数校验，然后转给领域模块
        ▼
领域模块 (src-tauri/src/)
  sync/      上游 release 链 → sha256 校验 → 条目 upsert → 图片队列（Range + If-Range 续传）
  db.rs      SQLite：WAL、foreign_keys；按 schema_version 逐级执行 migrations/；VACUUM INTO 快照
  library.rs 分页、三态决策、撤销、黑名单
  export.rs  画师池 / 图包池 / 完整池；推送走 plugin.rs（JWT 登录、401 自动重登）
  studio/ creative.rs reverse/   实验工作区：冻结输入、方案、运行记录、评分
  style/     candle 推理（CLIP / CSD / MegaStyle），向量存 SQLite，减均值余弦 Top-K 检索
  secrets.rs WebUI 密码与 token 存 macOS 钥匙串（service `nai5-codex-lab`），不落盘
  updater.rs 只认指定仓库的 vX.Y.Z tag，在独立 worktree 里构建
```

- **接口契约**：[`CONTRACT.md`](CONTRACT.md) 定义每个命令的参数、返回的 DTO 和错误码。Rust 的 `models.rs` 和 TypeScript 的 `types.ts` 逐字段对齐。前端还有一份 `mock/`，不启动后端也能跑。
- **SQLite WAL 与逐级迁移**：`src-tauri/migrations/` 里有 8 个按版本号编号的 DDL，启动时按 `schema_version` 依次补上缺的版本。测试里有迁移完整性检查。
- **断点续传同步**：图片先写到 `.part` 文件，中断后用 `Range` 加首次拿到的 ETag 接着下，下完校验通过才改名并入库。上游 release 没变时只补缺失的图片。
- **同步前快照**：每次同步前，以及每天一次，用 `VACUUM INTO` 生成一份一致的数据库副本，按份数轮换。
- **密钥放钥匙串**：密码和 token 只存在 macOS 钥匙串里，设置文件和日志里都没有。
- **本地画风向量**：模型权重不进仓库。在应用里点下载时，会从 HuggingFace 固定 revision 拉取（地址见 `src-tauri/src/style/catalog.rs`），存到数据目录的 `models/` 下。

### 规模

| 项目 | 数量 |
|---|---|
| Tauri IPC 命令（`#[tauri::command]`） | 101 |
| SQLite schema 迁移 | 8 |
| Rust 测试（`cargo test`） | 256 通过，4 个默认忽略（需要联网或外部夹具） |
| 前端测试（vitest） | 404 通过，1 个跳过，共 32 个测试文件 |

### 构建与测试

需要 macOS，并装好 Xcode Command Line Tools、Rust stable 和 Node.js 22 或更新版本。

```bash
npm ci
npm run typecheck          # vue-tsc
npm test                   # vitest
cd src-tauri && cargo test # Rust 单元测试和集成测试
```

```bash
npm run dev:mock           # 浏览器里跑离线假数据：不发真实请求，不产生费用
npm run tauri dev          # 原生开发窗口
npm run tauri build        # 打包 .app，产物在 src-tauri/target/release/bundle/macos/
```

`tauri build` 最后打 DMG 的那一步在部分机器上会失败，这时 `.app` 已经生成，可以直接使用。

首次运行时，在「设置」里填 AstrBot WebUI 地址（例如 `http://127.0.0.1:6185`）和用户名；密码、token 会写进钥匙串。
同步、素材库、画风向量这些功能不需要插件；构思、出图和推送需要一个实现了 `CONTRACT.md` 里插件路由的 AstrBot 插件，插件本身不在这个仓库里。

### 数据

所有数据都在 `~/Library/Application Support/moe.vivian.codexlab/`：`library.db`、图片、缩略图、导出文件、备份和日志。仓库里不包含任何上游图鉴数据或图片，测试夹具都是合成数据。

### 关于实现方式

代码由 Codex / Claude Code 辅助分模块实现；方案设计、代码审查和测试验收由我负责。

### 许可

应用代码使用 MIT 许可证，见 [`LICENSE`](LICENSE)。第三方代码和运行时下载的模型见 [`THIRD_PARTY_NOTICES.md`](THIRD_PARTY_NOTICES.md)。

---

## English

A macOS desktop app for curating reference material for NovelAI V5 image generation (artist strings and prompt packs) and running reproducible prompt-comparison experiments.
A Rust backend serves a Vue 3 frontend over Tauri IPC, and data lives in a local SQLite database. I started it in September 2026 and have used it daily for two months.
My local library currently holds about 39k catalogue entries, 43k images and 41k style vectors.

> This public version was extracted from a private repository with fresh git history. It contains no private configuration or real upstream data.

### Features

- **Catalogue sync**: pulls V5 artist dictionaries, community prompt packs and the NAX artist gallery, images included, into the local library. Downloads resume after interruption, and the database is snapshotted before every sync.
- **Curation library**: every entry can be marked include / exclude / undecided, with keyboard shortcuts, batch selection and undo. Decisions live in their own table, so re-syncs never lose them.
- **Export / push**: exports the curated artist pool, pack pool and full pool as JSON, or pushes them to the companion plugin through the AstrBot WebUI API.
- **Studio**: three modes share one workspace. Each round freezes its inputs so prompt models and skill combinations can be compared fairly.
  - **Draw**: uses curated packs as references, plans first, then renders.
  - **Reverse evaluation**: sends one source image to several vision LLMs, scores the captions tag by tag (precision / recall / F1) and records latency; optional redraws are rated by a judge model.
  - **Style review**: takes one set of images and compares vision models with local encoders.
- **Local style vectors**: CLIP / CSD / MegaStyle encoders run on-device (Metal on Apple Silicon, with a CPU fallback). Search subtracts the library mean and returns the cosine Top-K. The CSD encoder has a value-by-value parity test against the official implementation.
- **Self-update**: fetches a stable tag of this repository, builds it locally and swaps in the new app after backing up the database.

### Architecture

The module map in the Chinese section shows the layers: Vue → a thin Tauri command layer → domain modules (`sync`, `db`, `library`, `export`/`plugin`, `studio`/`creative`/`reverse`, `style`, `secrets`, `updater`).

- **Contract first**: [`CONTRACT.md`](CONTRACT.md) defines every IPC command, DTO and error code. `models.rs` (Rust) and `types.ts` (TypeScript) match field by field, and `mock/` lets the frontend run without the backend.
- **SQLite WAL + stepwise migrations**: eight numbered DDL files in `src-tauri/migrations/`, applied in order by `schema_version` at startup. Migration integrity is covered by tests.
- **Resumable sync**: images are written to `.part` files and resumed with `Range` plus the first ETag. They are renamed and recorded only after verification. If the upstream release hasn't changed, a sync only fills in missing images.
- **Pre-sync snapshots**: before each sync, and once a day, `VACUUM INTO` writes a consistent copy of the database, rotated by count.
- **Secrets in the macOS Keychain**: the WebUI password and tokens never touch settings files or logs.
- **Local style vectors**: inference uses candle (CLIP / CSD / MegaStyle), and vectors are stored in SQLite. Weights are not in the repository: the app downloads them on demand from pinned HuggingFace revisions (see `src-tauri/src/style/catalog.rs`).

### Size

| Item | Count |
|---|---|
| Tauri IPC commands (`#[tauri::command]`) | 101 |
| SQLite schema migrations | 8 |
| Rust tests (`cargo test`) | 256 passing, 4 ignored by default (need network or external fixtures) |
| Frontend tests (vitest) | 404 passing, 1 skipped, in 32 files |

### Build and test

Requires macOS with the Xcode Command Line Tools, stable Rust and Node.js 22 or later.

```bash
npm ci
npm run typecheck
npm test
cd src-tauri && cargo test
```

```bash
npm run dev:mock      # offline fake data in the browser; no real requests, no cost
npm run tauri dev     # native dev window
npm run tauri build   # .app under src-tauri/target/release/bundle/macos/
```

On some machines the final DMG step of `tauri build` fails; by then the `.app` is already built and usable.

Sync, curation and style vectors work without the plugin. Planning, rendering and push need an AstrBot plugin that implements the plugin routes in `CONTRACT.md`; that plugin is not part of this repository.

### How it was built

Implemented module by module with Codex / Claude Code assistance; I was responsible for the design, code review and test acceptance.

### License

App code is MIT ([`LICENSE`](LICENSE)). Third-party code and runtime-downloaded models are listed in [`THIRD_PARTY_NOTICES.md`](THIRD_PARTY_NOTICES.md).
