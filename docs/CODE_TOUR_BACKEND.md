# 后端代码导览：src-tauri/（写给熟悉 Java/Python、第一次读 Rust 的读者）

> 写于 v0.1.0，之后代码有较大扩展；`文件:行号` 以 v0.1.0 为准，仅作阅读路线参考。

这份文档不复述设计，只带你读代码。约定：`文件:行号` 都相对 `src-tauri/`；摘录尽量原文，≤10 行；每段先给代码再翻译成你熟悉的说法。建议开着编辑器对照着看，读完 §2 和 §3(a) 就能顺着调用链自己走剩下的。

---

## 1. 目录地图

| 文件 | 一句话 |
|---|---|
| `Cargo.toml` | 等价于 `pom.xml`/`pyproject.toml`：依赖、crate 类型（第 10-12 行同时产出 `rlib`+`cdylib`+`staticlib`）、release 优化（45-50 行：LTO、`panic = "abort"`） |
| `build.rs` | 编译前钩子，只调 `tauri_build::build()`，生成图标/权限清单 |
| `tauri.conf.json` | 窗口尺寸、CSP、`assetProtocol.scope = ["$APPDATA/**"]`（26-30 行）——前端能用 `asset://` 直接显示数据目录里的图 |
| `capabilities/main.json` | Tauri 2 的权限白名单：主窗口只开 `core:default`、`dialog:default`、`opener:allow-open-url`；自定义命令默认全开（注释里写了 v0.2 再收紧） |
| `migrations/001_init.sql` | 全部建表 DDL + 两个视图 `artist_library`/`pack_library`（128-138 行）；最后一行给 `schema_version` 塞 1 |
| `src/main.rs` | 6 行，只调 `nai5_codex_lab_lib::run()` |
| `src/lib.rs` | 模块声明 + `run()`：装插件、`setup` 里初始化 `AppState`、注册 38 个命令 |
| `src/error.rs` | 唯一错误类型 `AppError{code,message}` 与一堆 `From` 转换 |
| `src/models.rs` | 全部 DTO（与前端 `types.ts` 逐字段对应），纯数据，没逻辑 |
| `src/db.rs` | SQLite 连接封装 `Db`（一把互斥锁包一条连接）、迁移、`app_state` KV、事件日志、`VACUUM INTO` 备份 |
| `src/state.rs` | `AppState`（数据目录、Db、HTTP 客户端、节流器、同步控制块）、`Throttle`、`SyncController`、tracing 初始化、每日备份 |
| `src/secrets.rs` | 系统钥匙串（macOS Keychain）读写，account 白名单 |
| `src/commands/*.rs` | 命令层，薄：拿 `State`、校验参数、丢给模块函数；`mod.rs:23` 的 `blocking()` 是所有同步 DB 调用的入口 |
| `src/sync/mod.rs` | 同步编排：data-source → current → manifest → 两本书 → 图片队列 → `sync_log` |
| `src/sync/upstream.rs` | 上游 JSON 的形状、规范化、`Comparable`、sha256 校验；不碰网络和 DB |
| `src/sync/upsert.rs` | 条目入库：新增/比较/快照/版本号/消失打标/画师解析 |
| `src/sync/images.rs` | 图片队列构建、并发下载、`.part` 续传、ETag-MD5 校验、缩略图 |
| `src/artist/recipe.rs` | 画师串/提示词语法解析器（从 Langbai 的 TS 移植） |
| `src/artist/mod.rs` | 解析器之上的词典规则：剥 `<artist>` 壳、无前缀画师名补前缀、signature、序列化、图包清洗 |
| `src/library.rs` | 词典页数据层：书目、分页筛选、详情、三态决策 + 审计、撤销、黑名单、日志读取 |
| `src/export.rs` | 三个导出文件 + 推送到 AstrBot 插件（JWT 登录） |
| `src/reverse/mod.rs` | 反推实验室六个子模块的声明 |
| `src/reverse/builtin.rs` | 内置 `plugin v1` 模板/规则集常量（从 Python 插件原文搬来）、角色词表 |
| `src/reverse/clean.rs` | 规则集编译（正则）与 `clean_tags` |
| `src/reverse/client.rs` | 图片预处理、OpenAI 兼容 `chat/completions` 调用、从响应里抠 JSON |
| `src/reverse/metrics.rs` | 真值构建与 P/R/F1 等指标 |
| `src/reverse/store.rs` | 反推相关表的 CRUD、题库、批次汇总、比较、评分 |
| `src/reverse/batch.rs` | 批次后台任务：逐题串行、多端点并行、落库、推事件 |
| `tests/live_smoke.rs` | 联网冒烟测试，`LIVE_SMOKE=1` 才跑（19-24 行） |

---

## 2. 启动时发生了什么（按执行顺序）

1. **`main.rs:4-6`** 进程入口，只有一行 `nai5_codex_lab_lib::run()`。Rust 的 bin 与 lib 分开，是为了让 `tests/` 和 Tauri 的移动端入口都能复用同一个 lib。

2. **`lib.rs:31-39`** 建 Tauri 应用：

```rust
tauri::Builder::default()
    .plugin(tauri_plugin_dialog::init())
    .plugin(tauri_plugin_opener::init())
    .setup(|app| {
        let data_dir = app.path().app_data_dir().expect("app_data_dir");
        let state = state::AppState::init(app.handle().clone(), data_dir)?;
        app.manage(state);
        Ok(())
    })
```

`Builder` 是典型的建造者模式（和 Java 的 `OkHttpClient.Builder` 一样）。`setup` 里的闭包在窗口创建前跑一次；`app.manage(state)` 相当于把一个单例塞进 DI 容器，后面每个命令的 `State<'_, AppState>` 参数都是从这里取的。

3. **`state.rs:126-148` `AppState::init`** 依次做：建 7 个子目录（`SUBDIRS`，21 行）→ `init_tracing`（201-218 行，按天滚动的 JSON 日志 + stderr）→ `Db::open`（134 行）→ `ensure_builtin`（135 行，首次插入内置模板/规则集）→ `daily_backup`（136 行，今天没备份就 `VACUUM INTO` 一份）→ `build_http_client`（138 行，UA + 20 s 连接超时）→ 组装 `AppState`，其中 `Throttle::new(4, 100ms)`、`SyncController::new()`。

4. **`db.rs:28-48` `Db::open` → `init_conn`**：`busy_timeout(10s)`，然后一条 `execute_batch` 打开外键、WAL、`synchronous=NORMAL`，再 `migrate`。

5. **`db.rs:146-170` `migrate`**：查 `sqlite_master` 有没有 `schema_version` 表，没有则 `current=0`；遍历 `MIGRATIONS`（16 行，`include_str!` 把 SQL 文件在编译期嵌进二进制），版本大于当前的就 `execute_batch`。注意 159-166 行：v1 的版本号由 SQL 文件最后一行自己插入，v2 以后才走 `UPDATE`——写 `002_*.sql` 时别再 INSERT。

6. **`lib.rs:40-79`** `generate_handler![...]` 把 38 个 `#[tauri::command]` 函数注册成 IPC 端点；前端 `invoke("list_entries", {...})` 就是按函数名找到这里。

7. **`lib.rs:80`** `.run(tauri::generate_context!())` 阻塞主线程，进入事件循环。之后一切都是前端 `invoke` 驱动的：前端首屏通常先调 `app_info`、`settings_get`、`sync_status`、`list_books`。

---

## 3. 用户动作 → 代码路径

### (a) 点「同步」

1. **前端** `invoke("sync_start", { force, on_event: channel })`。
2. **`commands/sync.rs:46-69` `sync_start`**：

```rust
if !state.sync.try_start() {
    return Err(AppError::sync_running());
}
state.sync.update(|s| { s.running = true; s.phase = SyncPhase::Manifest; ... });
let ctx = match make_ctx(&state, on_event, true) { Ok(c) => c, Err(e) => { state.sync.finish(); return Err(e); } };
tauri::async_runtime::spawn(async move { sync::run_sync(ctx, force).await; });
Ok(())
```

`try_start`（`state.rs:75-77`）是 `AtomicBool::compare_exchange(false,true)`，就是 Java 的 `AtomicBoolean.compareAndSet`——同一时刻只允许一个同步。`spawn` 后立刻返回 `Ok(())`，前端不等；进度通过 `on_event` 这个 `Channel<SyncEvent>` 推回去（`make_ctx` 26-43 行把它包进闭包 `emit`）。

3. **`sync/mod.rs:137-195` `run_sync`**：外壳。调 `sync_inner`，不管成功失败都写一行 `sync_log`（158-175 行）和一条 `app_event`，发最后一个 `Done`/`Error` 事件，最后 `controller.finish()` 把 `running` 放回 false。这是 Java 里 `try { … } finally { … }` 的手写版。

4. **`sync/mod.rs:197-388` `sync_inner`**，按步骤：
   - 202-206 行：拉 `data-source.json`（这一枪不经节流）→ 拼出 `current.json` 的 URL 再拉。`upstream::parse_data_source`/`parse_current`（`upstream.rs:276-290`）用 serde 反序列化并检查 `schemaVersion == 1`。
   - 224-236 行：本地 `latest_release` 等于远端且没 `force` → 只补图片，跳到第 5 步。
   - 238-251 行：先 `backup_into(..., Some("presync"), 10)` 做一份同步前备份；拉 manifest，原文落盘到 `codex_raw/<release>/`。
   - 253-266 行 `fetch_verified` 闭包：拉文件 → `upstream::verify_file`（`upstream.rs:301-311`，先比 size 再比 sha256）→ 落盘 → 返回字节。
   - 282-333 行：对白名单两本书循环。先查本地 `codex_book.file_sha256`，没变就 `continue`（跳过解析）；变了就下载、`spawn_blocking` 里 `parse_book`（307 行，7 MB JSON 的解析放阻塞线程；非 safe 分级的条目在校验前就丢弃，`entryCount` 保留上游值），然后 **`upsert::upsert_book`**（315 行）。
   - 334 行：两本书都 upsert 完才 `app_state_set("latest_release")`——保证半途失败不会把 release 标成新的。
   - 343-361 行：`images::build_sync_queue` 算出要下的图，`images::run_queue` 并发下载，回调里更新 `Progress` 并 `emit`。
   - 363-388 行：算 `kind`（`full`/`partial`/`resume`/`noop`）和人话 message，返回 `Outcome`。

5. **`upstream.rs:199-248` `normalize_entry`**：把 `serde_json::Value` 手工挑字段进 `NormEntry`，缺 key 一律 `None`，缺 `id/tags/image` 直接 `Err(upstream)`。相当于 Python 里 `d.get("x")` 加类型检查，但每个字段都写死了类型。

### (b) 点图片切三态（加入/不加入/未定）

1. 前端 `invoke("set_decision", { entry_ids: [...], decision: "include", via: "click" })`。serde 把 `"include"` 反序列化成 `Decision::Include`（`models.rs:10-16`，`#[serde(rename_all = "lowercase")]`）。
2. **`commands/entries.rs:28-37`**：空数组直接返回 `changed: 0`；否则 `spawn_blocking(move || library::set_decision(&db, &entry_ids, decision, via))`。注意末尾的 `??`：外层 `?` 处理 `JoinError`（线程崩了），内层 `?` 处理业务 `AppError`。
3. **`library.rs:360-384` `set_decision`**：

```rust
let changed = db.with(|conn| {
    let tx = conn.transaction()?;
    let mut changed = 0i64;
    for id in entry_ids {
        let codex_id: Option<String> = tx.query_row("SELECT codex_id FROM codex_entry WHERE entry_id = ?1", params![id], |r| r.get(0)).optional()?;
        let Some(codex_id) = codex_id else { continue };
        let from = current_decision(&tx, id)?;
        if from == Some(decision) || (from.is_none() && decision == Decision::Undecided) { continue; }
        write_decision(&tx, id, &codex_id, from, decision, via, &now)?;
        changed += 1;
    }
    tx.commit()?;
```

一个事务里逐条：条目不存在跳过；决策没变跳过（不写审计）；否则 `write_decision`（348-358 行）做两件事——`library_membership` 的 upsert（`ON CONFLICT(entry_id) DO UPDATE`）和 `membership_audit` 插一行 `from → to, via`。`library_of(codex_id)`（`upstream.rs:22-28`）决定这条属于 `artist` 还是 `pack` 库。

4. 撤销走 **`library.rs:402-431` `undo_decision`**：读 `app_state('undone_audit_ids')` 拿已撤销过的 id 集合，从最近 200 条 `via != 'undo'` 的审计里找第一条没撤销过的，把它的 `from_decision` 写回去（再记一条 `via='undo'` 的审计），把 id 追加进集合（上限 500）。所以连按 ⌘Z 会一路往回退。

### (c) 「推送到插件」

1. `commands/export.rs:21-23` 直接 `export::push_to_plugin(&state).await`（无参数）——这是少数不 `spawn` 的长命令，前端要等它返回。
2. **`export.rs:291-305`**：读 settings 里的 `plugin_base_url`；`secrets::get("webui_password")` 没有就报 `secret`；`library_token` 可空。然后 `spawn_blocking` 跑 **`export_libraries`**（227-253 行），它调 `build_bundle`（200-210 行）生成三份东西并写到 `exports/<时间戳>/`：
   - `build_artist_pool`（43-111 行）：读视图 `artist_library`，同 signature 只留 `decided_at` 最新的一条（67-96 行的 `HashMap<签名, (decided_at, batch, artists)>`），过黑名单，`artist::compose` 拼成最终串。
   - `build_pack`（114-156 行）：读 `pack_library`，`clean_pack_tags` 清洗，拼 `{s, c, t, tags, r}`。
   - `build_library_export`（159-198 行）：两库全量。
3. **`login`（267-285 行）**：POST `/api/auth/login`，密码先过 `password_for_login`（257-265 行：已是 32 位 hex 就原样，否则 md5——AstrBot 存的是 md5）；从响应 `/data/token` 取 JWT。
4. **307-358 行**：`loop { POST /api/plug/astrbot_plugin_vivian/page/library/import, bearer_auth(token), X-Library-Token }`；401 且没重登过 → 重新 `login` 再来一次；非 2xx → `plugin_rejected`（带正文前 500 字）；成功则从响应 `data` 里挑字段填 `PushResult`。

### (d) 反推 batch

1. 前端 `invoke("reverse_run_batch", { req, on_event: channel })`。
2. **`commands/reverse.rs:86-105`**：`spawn_blocking(batch::prepare)` 校验并插 `reverse_batch` 行；造一个 `CancellationToken` 存进 `state.batch_cancel`；组 `BatchCtx`；`tauri::async_runtime::spawn(batch::run(ctx, prepared))`；立即返回 `batch_id`。
3. **`batch.rs:76-122` `prepare`**：查模板、规则集（并试编译正则）、每个 provider（禁用的报错；key 从钥匙串 `provider:<id>` 取）；解析 `question_id`（63-73 行：`local:<path>` 或 `<entry_id>#<ordinal>`），图没下载的报 `bad_request`。
4. **`batch.rs:264-343` `run`**：外层 `for q in &prepared.questions` 串行；每题先 `spawn_blocking(client::prepare_image)`（`client.rs:46-75`，>9 MB 才缩到 2048 边并按 JPEG 88/72/55 压），再对所有 provider 用 `join_all` 并行调 `call_provider`。
5. **`batch.rs:135-185` `call_provider`**：`tokio::select!` 在 `client::chat` 和取消令牌之间二选一；成功后 `client::extract_json`（`client.rs:174-197`）抠第一个 `{…}`，`tags_field` 取 tags，`compiled.refusal` 查拒答语，`compiled.clean_tags`（`clean.rs:56-84`）清洗；决定 `status ∈ {ok, refused, error}`。
6. **`batch.rs:187-262` `insert_run`**：写 `reverse_run`（请求日志用 sha256 代替 base64）；若是题库图，`metrics::build_truth` + `metrics::compute` 写 `reverse_metric`。
7. 每条落库后 `emit(run_done/run_failed)`；结束 `finish()` 更新 `reverse_batch.status`，再 `batch_done/batch_failed`。

---

## 4. 核心算法逐段讲

### 4.1 画师串解析器 `artist/recipe.rs`

输入类似 `{{artist:foo，bar}}, 0.4::impasto ::，-3::a, b::, c`。目标：切成 token，每个带 `value/weight/kind`。

**预处理（142-150 行）**：NFKC 归一（全角→半角），`，、；;` 全变成 `,`。Python 里就是 `unicodedata.normalize("NFKC", s).translate(...)`。

**状态机（93-100 行 `ParseState` + 159-187 行主循环）**：逐字符扫。`{`/`}` 增减 `curly`，`[`/`]` 增减 `square`，`,` 触发 `flush()`，其它字符追加进 `token`。`starting_level`（183-185 行）记的是**这个 token 第一个非空白字符时**的括号深度——这样 `{{a}}, b` 里 `a` 的深度是 2，`b` 是 0。

**`flush()`（102-139 行）** 是全部语义所在：

```rust
let numeric_start = re(&NUMERIC_START, r"(?s)^(-?\d+(?:\.\d+)?)\s*::\s*(.*)$");
if let Some(m) = numeric_start.captures(&raw) {
    if let Ok(w) = m[1].parse::<f64>() { self.numeric_weight = Some(w); raw = m[2].trim().to_string(); }
}
let closes_numeric = raw.ends_with("::");
if closes_numeric { raw = raw[..raw.len() - 2].trim().to_string(); }
if !raw.is_empty() {
    let weight = match self.numeric_weight {
        Some(w) => w,
        None => 1.05f64.powi(self.starting_level.unwrap_or(self.curly - self.square)),
    };
```

- 开头 `w::` → 记下 `numeric_weight`，这个权重会**一直沿用到遇到以 `::` 结尾的 token**（134-136 行才清掉）。这就是 `-3::a, b::` 里 `a` 和 `b` 都是 -3 的原因。
- 没数值权重就用 `1.05^深度`（花括号加、方括号减），和 NovelAI 的 `{}`/`[]` 语义一致。
- 129-131 行剥一层最外的 `(...)`；`weight < 0` 直接判 `Negative`，否则 `classify`（73-91 行按 artist→year→quality→negative→style 正则顺序判）。
- `round_weight`（57-60 行）故意用 `floor(x*100+0.5)/100` 而不是 Rust 的 `round()`，为了和 JS `Math.round` 对 `.5` 的行为一致。

**上层 `artist/mod.rs:103-143` `parse_entry_tags`**：先 `strip_wrappers`（25-28 行，`<artist>`/`<style>`/换行都替换成逗号），跑解析器，然后按 kind 分三堆：负权重进 `negative_tail`，`Artist` 剥掉 `artist:` 前缀进 `artists`，其余进 `auxiliary`。108 行 `bare_mode`：`path[0] == "单画师词典"` 且整串没有 `artist:` 时，把 `Other/Style` 也当画师名。最后 `signature`（77-87 行）= canonical 名 + 权重量化到 0.25，排序后 `+` 拼接——用来判「其实是同一个画师配方」。

### 4.2 条目比较与版本化 `sync/upsert.rs`

**`upsert_book`（194-271 行）** 一本书一个事务。先 `existing` = 本书未删除的 id 集合（199-203 行）。对每条上游条目：

```rust
match load_local(&tx, &e.id)? {
    None => { tx.execute("INSERT INTO codex_entry(... entry_version ...) VALUES (?1, ?2, ?3, 1, ...)")?; stats.added += 1; }
    Some(local) => {
        if local.comparable != e.comparable() {
            tx.execute("INSERT OR REPLACE INTO codex_entry_history(entry_id, entry_version, snapshot_json, replaced_at) VALUES (?1, ?2, ?3, ?4)", ...)?;
            tx.execute("UPDATE codex_entry SET entry_version = entry_version + 1, ... WHERE entry_id = ?1", ...)?;
            stats.changed += 1;
        } else {
            tx.execute("UPDATE codex_entry SET book_order = ?2, is_new = ?3, update_batches_json = ?4, last_seen_release = ?5, removed_at = NULL WHERE entry_id = ?1", ...)?;
```

关键是 **`Comparable`（`upstream.rs:112-124`）**：一个只含「算变化」字段的结构体，派生了 `PartialEq`，所以 `!=` 就是逐字段比较（Java 里要自己写 `equals`）。`isNew/updateBatches/book_order` 不在里面，所以它们变了不升版本、不留快照。`character_prompts` 被序列化成规范 JSON 字符串再比（`character_prompts_json`，142-148 行，空数组和 None 等价）。

**`load_local`（66-153 行）** 把旧行连同 `entry_image` 一起读出来，一半用来构造 `Comparable`，一半用 `json!` 拼成 `snapshot`——变更时整行进 `codex_entry_history`，所以任何时候都能回看某个版本。

**`upsert_images`（155-173 行）**：逐 ordinal upsert；`ON CONFLICT` 子句里用 `CASE WHEN excluded.image_file = entry_image.image_file THEN 保留 ELSE NULL` 保住已下载的 `local_path/etag/bytes`——文件名没变就不重下。171 行删掉多出来的 ordinal（上游删了图）。

**消失处理（257-268 行）**：`missing = existing − seen`；`missing*10 >= existing.len()`（≥10%）就只置 `removed_skipped` 让上层告警，否则逐条打 `removed_at`。重新出现时上面的 UPDATE 都带 `removed_at = NULL`，自动复活。

### 4.3 图片续传与校验 `sync/images.rs`

**队列（168-189 行 `build_sync_queue`）**：主图（`ordinal == 0`）或用户点过「下载全部」的条目才候选；`needs_display`（124-129 行）判断 `local_path` 为空、`asset_rev` 不等于条目当前 `asset_rev`、或文件在磁盘上不存在。

**并发（210-253 行 `run_queue`）**：`futures_util::stream::iter(jobs).map(|job| async move {...}).buffer_unordered(concurrency)`——把 Vec 变成异步流，同时最多跑 `concurrency` 个。计数用 `Arc<AtomicU64>`，失败清单用 `Arc<Mutex<Vec<_>>>`。

**单文件（351-426 行 `download_attempt`）**：

```rust
let existing = tokio::fs::metadata(part).await.map(|m| m.len()).unwrap_or(0);
let side_etag = tokio::fs::read_to_string(side).await.ok()...;
let resume = existing > 0 && side_etag.is_some();
let _permit = throttle.acquire().await;
let mut req = http.get(url).timeout(Duration::from_secs(60));
if resume {
    req = req.header(RANGE, format!("bytes={existing}-")).header(IF_RANGE, side_etag...);
}
```

- 有 `.part` 且旁边有 `.part.etag` → 发 `Range` + `If-Range`。服务端回 206 就 append；回 200（文件变了）就重头写并更新旁车 ETag；416 就删干净重来。
- 流式落盘（394-408 行）：`resp.bytes_stream()` 一块块 `write_all`，每块之间 `select!` 一下取消令牌。
- 校验（412-421 行）：ETag 若是 32 位 hex（`md5_from_etag`，288-295 行）就 `spawn_blocking` 算文件 MD5 比对；不符删掉重来。
- 423 行 `rename(part, dest)` 才算完成，之后 `process_job`（255-286 行）生成缩略图并写 DB——**文件先落地，DB 后更新**，崩溃只会漏记不会错记。

**重试（314-349 行 `download_file`）**：最多 4 次，间隔 `2^attempt` 秒（2/4/8），退避期间也能被取消。

**缩略图（429-447 行）**：最长边 ≤ 320 不放大，JPEG 80，也是先写 `.tmp` 再 rename。

### 4.4 指标 `reverse/metrics.rs`

三个集合概念：`truth`（答案全集）、`truth_eval`（真正计分的分母）、`expected_miss`（答案里有但模型按规则**不该**输出的，如角色名/发色）。

**`norm`（23-32 行）**：小写、`_`→空格、剥权重语法 `1.2::x::`、剥 `{}[]`、压空白。两边都过这一道再比，等价于 Python 里先 `lower().replace("_"," ")` 再 `re.sub`。

**`build_truth`（81-147 行）**：来源 = `raw_tag`（没有就用 `tags`）+ 每个 `characterPrompts[].prompt`，都过 4.1 的解析器。对每个 token：

```rust
let is_excluded = matches!(t.kind, TokenKind::Artist | TokenKind::Quality | TokenKind::Negative | TokenKind::Year)
    || t.weight < 0.0 || quality_extra || is_rating(&v);
push_unique(&mut sets.truth, v.clone());
if is_excluded { excluded.insert(v); continue; }
let mut miss = false;
if is_character_name(&v, aliases) || PEOPLE_WORDS.contains(&v.as_str()) || ruleset.is_banned(&v) || ruleset.is_appearance(&v) { miss = true; }
if idx > 0 && !first_char_done && !GENERIC_WORDS.contains(&v.as_str()) { first_char_done = true; miss = true; }
```

剔除类根本不进分母；`expected_miss` 进答案但不进分母；剩下的是 `truth_eval`。114-117 行：角色提示词里第一个非通用词视为角色名。

**`compute`（181-222 行）**：`hits = pred ∩ truth_eval`；`misses = truth_eval − pred`；`extras = pred − truth（全集）`（所以输出了 expected_miss 词不算 extra，而算 `expected_hit`）；`contradictions = pred ∩ negatives`。`precision = hit/|pred|`（pred 空则 0），`recall = hit/|truth_eval|`（分母空则 `None`），F1 随 recall 为 `None`。`appearance_leak/banned_leak` 用的是**清洗前**的 `pred_raw`，衡量模型本身；`ruleset_drop = |pred_raw| − |pred|` 衡量规则集干了多少活。

---

## 5. 语言/框架概念清单

**所有权与借用。** Java/Python 里对象随便传引用；Rust 每个值只有一个主人，传参要么「搬走」（move）要么「借」（`&` 只读、`&mut` 可写）。看 `commands/entries.rs:32-36`：`let db = state.db.clone();` 然后 `move || library::set_decision(&db, ...)`——闭包要在别的线程跑，必须拥有 `db`，所以先 `clone` 一个 `Arc` 再 `move` 进去；`state` 本身是借来的不能搬。再看 `library.rs:413` 的 `drop(stmt)`：`stmt` 借着 `tx`，不先放掉就不能 `tx.commit()`，编译器会拦。

**`Result` / `?` / `AppError`。** 没有异常，函数返回 `Result<T, E>`，`?` 等价于「是 Err 就 return 它，否则解包」——像 Java 的 checked exception 但写在类型里。`error.rs:33-53` 一串 `impl From<X> for AppError` 让 `?` 能把 `rusqlite::Error`、`reqwest::Error`、`io::Error` 自动转成 `AppError`；`error.rs:55` 的 `AppResult<T>` 是类型别名。命令函数返回 `AppResult<T>`，Tauri 把 `AppError` 用 `Serialize` 变成 `{code, message}` 给前端。

**`Option` 与 `match`。** `Option<T>` 是「可能为空」的显式类型（Java `Optional`，Python 的 `None` 但编译器强制你处理）。`secrets.rs:30-36`：`match entry(account).and_then(|e| e.get_password()) { Ok(v) => ..., Err(keyring::Error::NoEntry) => Ok(None), Err(e) => Err(...) }`——`match` 是穷举的 switch，漏一个分支不给编译。`let Some(x) = ... else { continue };`（`library.rs:370`）是 let-else，Python 里相当于 `if x is None: continue`。

**`Arc` / `Mutex` / `parking_lot`。** `Arc<T>` = 线程安全引用计数指针（多线程共享的 `shared_ptr`）；`Mutex<T>` 把数据锁在里面，`lock()` 返回的 guard 出作用域自动解锁（没有 `finally { unlock() }`）。`db.rs:22-25` `Db { conn: Mutex<Connection> }`，`with()`（55-58 行）就是「拿锁 → 执行闭包 → 自动放锁」。`parking_lot::Mutex` 是标准库 Mutex 的更快替代，`lock()` 不返回 `Result`（不用处理「毒化」）。`state.rs:28` 的 `Throttle.last` 用的是 `tokio::sync::Mutex`，那是能在 `.await` 中间持有的异步锁。

**`async/await`、`tokio::spawn`、`spawn_blocking`。** `async fn` 返回一个 Future，`.await` 才真正跑——和 Python `asyncio` 一模一样，tokio 就是事件循环。`tauri::async_runtime::spawn(async move {...})`（`commands/sync.rs:65`）= `asyncio.create_task`，火完就走。**`spawn_blocking`**（`commands/mod.rs:23-25`）是给同步阻塞代码（SQLite、文件 MD5、图片解码）用的线程池，避免卡住事件循环——对应 `loop.run_in_executor`。`tokio::select!`（`images.rs:371-374`）= `asyncio.wait(..., FIRST_COMPLETED)`，这里用来让取消令牌能打断网络请求。

**闭包与 `dyn Fn`。** `sync/mod.rs:30` `type Emitter = Arc<dyn Fn(SyncEvent) + Send + Sync>`：一个可跨线程共享的函数对象（Java `Consumer<SyncEvent>`）。`dyn` 表示运行期分发的 trait 对象，`Send + Sync` 是「可以跨线程」的标记。`commands/sync.rs:39-41` 把 `Channel` 捕获进闭包，模块层不用知道 Tauri 的存在。

**trait 与 `derive`。** trait ≈ Java 接口 + Python 协议。`#[derive(Serialize, Deserialize)]`（`models.rs` 到处都是）让编译器按字段生成 JSON 编解码；`#[derive(PartialEq)]`（`upstream.rs:113`）生成逐字段 `==`；`#[derive(Default)]` 生成 `Default::default()`；`Clone`/`Copy`/`Debug` 同理。手写实现看 `error.rs:26-31` 的 `impl Display for AppError`。`#[serde(rename_all = "lowercase")]`（`models.rs:11`）和 `#[serde(default)]` 是编解码的注解，等价于 Jackson 的 `@JsonProperty`。

**生命周期（`'a`）。** 引用不能比它指向的东西活得久，大多数时候编译器自动推断；显式写出来的地方：`metrics.rs:45-50` `TruthInput<'a>`——结构体里存了几个借来的 `&'a str`，标 `'a` 告诉编译器「这个结构体活多久，那几个引用就至少活多久」；`state.rs:42` `SemaphorePermit<'_>`——许可证借着 `Throttle`；`commands/*.rs` 里 `State<'_, AppState>`——Tauri 的状态句柄借着应用本体。`'static`（`sync/mod.rs:56` `&'static str`）是「活到进程结束」，字面量字符串都是。

**`OnceLock` / `static`。** `artist/recipe.rs:25-32` 一堆 `static X: OnceLock<Regex>` + 21-23 行的 `re()`：线程安全的懒初始化单例，Python 里对应 `functools.lru_cache` 包一个无参函数。正则编译一次全程复用。

**`rusqlite` 的 `params!` 与 `query_map`。** `params![a, b, c]` 造位置参数（`?1 ?2 ?3`），像 JDBC 的 `setString(1, …)`。`conn.query_row(sql, params, |r| ...)` 取一行；`stmt.query_map(params, |r| Ok(Struct{ f: r.get(0)?, ... }))` 返回一个迭代器，每个元素是 `Result<Struct>`，`.collect::<Result<Vec<_>, _>>()?` 把「迭代器里全是 Result」翻成「一个 Result 里装 Vec」（`library.rs:31-51`）。动态 WHERE 看 `library.rs:65-117` `build_filter`：拼 SQL 串同时往 `Vec<Box<dyn ToSql>>` 里推参数，最后 `params_from_iter`。`.optional()` 把「没查到行」从错误变成 `Ok(None)`。事务：`conn.transaction()?` 得到 `tx`，`tx` 能当 `Connection` 用（`Deref`），不 `commit` 就 drop 自动回滚。

**`#[tauri::command]` 与 `State`。** 加了这个宏的 `async fn` 就是 IPC 端点；参数从前端 JSON 反序列化，`State<'_, AppState>` 由 Tauri 注入（`app.manage` 塞进去的那个），`Channel<T>`（`commands/sync.rs:46`）是前端传进来的流式回调。`rename_all = "snake_case"` 让前端传 `entry_ids` 而非 `entryIds`。

**模块与可见性。** `mod x;` 声明子模块（文件），`pub` 才对外可见，`pub(crate)` 只对本 crate；`commands/mod.rs:11-15` 的 `pub use app::*;` 是 re-export，让 `lib.rs` 能写 `commands::app_info`。`crate::` 是绝对路径（Python 的绝对 import），`super::` 是父模块。

**`include_str!` / `concat!` / `env!`。** 编译期宏：`db.rs:16` 把 SQL 文件嵌进二进制，`builtin.rs:62` 嵌角色词表，`state.rs:19-20` 从 Cargo.toml 读版本号。

**`#[cfg(test)]` 与 `cargo test`。** 每个文件底部的 `mod tests` 只在测试构建时编译，`#[test]` 函数就是用例；`Db::open_memory()` + fixture JSON 让全部 41 个测试离线可跑。

---

## 6. 想改 X 去哪改

| 想改 | 去哪 |
|---|---|
| 加一个新命令 | `commands/<文件>.rs` 写 `#[tauri::command] async fn`；`lib.rs:40-79` 注册；`models.rs` 加 DTO；前端 `types.ts` 同步 |
| 加一张表 / 一列 | 新建 `migrations/002_*.sql`（只写 DDL，不要 INSERT schema_version）；`db.rs:16` 的 `MIGRATIONS` 加 `(2, include_str!(...))` |
| 改上游 URL / 白名单书 | `sync/mod.rs:28` `DATA_SOURCE_URL`；`upstream.rs:14-16` `BOOK_WHITELIST` 等 |
| 改「什么算条目变了」 | `upstream.rs:112-124` `Comparable` 加减字段，`upsert.rs:131-141` 的构造同步改 |
| 改消失阈值 10% | `upsert.rs:260` |
| 改并发 / 节流 / 超时 | 并发上限 `commands/sync.rs:36` 与 `images.rs:214` 的 `clamp(1,4)`；节流 `state.rs:144` `Throttle::new(4, 100ms)`；单文件超时 `images.rs:365`；重试次数 `images.rs:327` |
| 改缩略图尺寸/质量 | `images.rs:433` 与 `440` |
| 改画师串序列化格式 / 质量后缀 | `artist/mod.rs:15-16` `QUALITY_SUFFIX`，`41-47` `format_token`，`50-74` `core_of/compose` |
| 改分类正则（什么算 quality/style） | `artist/recipe.rs:34-54` |
| 改单画师（无前缀名）目录 | `artist/mod.rs:19` `SINGLE_ARTIST_DIR` |
| 加一个列表筛选条件 | `models.rs:249-274` `EntryQuery` 加字段；`library.rs:65-117` `build_filter` 加一段 |
| 改撤销深度 | `library.rs:408`（候选 200）与 `424`（记忆 500） |
| 改导出文件名 / 结构 | `export.rs:231-236`（文件名）、`147`（图包字段）、`189-193`（全量导出字段） |
| 改插件接口路径 / 登录 | `export.rs:268`（login）、`314`（import URL）、`257-265`（密码处理） |
| 改内置模板 / 规则 | `reverse/builtin.rs:19-60`；注意只在表为空时插入，改了常量对已有库无效，得用 `reverse_template_save` 存新版本 |
| 改清洗规则逻辑 | `reverse/clean.rs:56-84` |
| 改指标口径 | `reverse/metrics.rs:16-20` 常量，`81-147` `build_truth`，`181-222` `compute` |
| 改模型请求体 / 图片上限 | `reverse/client.rs:15-18` 常量，`86-94` `build_messages`，`147-171` `chat` |
| 改 batch 并行策略 | `reverse/batch.rs:283-334`（现在题串行、端点并行） |
| 改钥匙串 account 白名单 | `secrets.rs:9` |
| 改日志级别 / 文件 | `state.rs:213-214` `EnvFilter`（环境变量 `RUST_LOG` 可覆盖） |
| 改备份保留份数 | `state.rs:194`（每日 30）、`sync/mod.rs:239`（presync 10） |
| 改窗口 / CSP / 资源协议 | `tauri.conf.json` |
| 放开或收紧前端权限 | `capabilities/main.json` |

---

## 7. 读代码时发现的坑与可改进点

1. **单连接 + 互斥锁，所有 DB 操作串行。** `db.rs:22-25` 一把 `Mutex<Connection>`。`upsert_book` 持锁 1–2 s（`sync/mod.rs:315`），且是在 `async fn` 里直接 `ctx.db.with(...)`，没走 `spawn_blocking`——期间 tokio 的这个工作线程被阻塞。`sync_status`（`commands/sync.rs:21`）、`settings_get`（`app.rs:28`）也是在 async 线程上直接查库，和 `commands/mod.rs` 顶部「同步 DB 查询放 spawn_blocking」的约定不一致。量小时没事，但这是以后卡 UI 的第一嫌疑人。可以考虑读写分离成两条连接（WAL 模式下读不阻塞写）。

2. **`panic = "abort"`（`Cargo.toml:49`）让所有 `JoinError` 分支形同虚设。** `commands/mod.rs:25`、`batch.rs:315-320` 都认真处理了「阻塞任务 panic」的情况，但 release 构建里 panic 直接杀进程，根本不会走到那里。要么去掉 abort，要么接受这些分支只在 debug 有效。

3. **错误类型不统一。** `images.rs:314-426` 用 `Result<_, String>`，别处都是 `AppError`；`process_job` 又把 `AppError` 转回 `String`（284 行）。前端拿到的 `SyncFailure.error` 因此没有 `code`，无法区分「网络」「校验失败」「取消」。

4. **同步与按需下载可能同时下同一个文件。** `download_images`（`commands/sync.rs:84-104`）不占 `running` 标记、用独立取消令牌，只共用节流。若主同步正在下某条目的主图，用户又对同一条目点「下载全部」，`build_entry_queue` 会把 ordinal 0 也放进队列，两个任务对同一个 `.part` 文件 append/create。概率低，但没有互斥。

5. **`sync_cancel` 在 manifest 阶段不能立刻生效。** `check_cancel`（`sync/mod.rs:104-110`）只在阶段之间检查；`fetch_bytes`（91-102 行）没有 `select!` 取消令牌，一个卡住的请求要等满 60 s 超时才响应取消。图片阶段做对了（`images.rs:371`、`396`），manifest 阶段没做。

6. **迁移没有事务。** `db.rs:159-166` 对每个版本 `execute_batch` 后才更新版本号；如果 002 的 DDL 中途失败，表建了一半而版本号没动，下次启动会再跑一遍撞上 `CREATE TABLE` 冲突（001 用了 `IF NOT EXISTS` 才没事，之后的迁移也得记得这么写，或者包一层 `BEGIN/COMMIT`）。

7. **孤儿文件。** `upsert_images:171` 删掉多余 ordinal、`162-166` 行在文件名变化时把 `local_path` 置空，但磁盘上的旧图和缩略图都不删。长期同步会积累垃圾，没有任何清理命令。

8. **ETag 不是 MD5 时静默跳过校验**（`images.rs:412-421`）。对象存储的分片上传 ETag 形如 `"abc-2"`，此时文件内容不校验，只信 rename。目前上游是 32 位 hex，将来换 CDN 要留意。

9. **`push_to_plugin` 每次都完整导出一遍并写盘**（`export.rs:303`），`ExportResult` 直接丢弃（`_export`）。推送十次就多十个 `exports/<时间戳>/` 目录，没有清理。另外 `secrets::get`（297-298 行）是阻塞的钥匙串调用，直接在 async 线程上跑，与 `commands/app.rs:59` 用 `spawn_blocking` 的做法不一致。

10. **密码是否 md5 靠长度猜**（`export.rs:259`）。真实密码恰好是 32 位 hex 就会被当成已哈希原样发送而登录失败。更稳的做法是设置里显式标记，或永远 md5。

11. **`AppState.batch_cancel` 没有命令能触发**（`state.rs:122`、`commands/reverse.rs:91`），令牌造了存着但没有 `reverse_cancel_batch`。同理 `SyncController.running`（原子）和 `status.running`（锁内字段）是两份真相，`sync_status` 里手工用前者覆盖后者（`commands/sync.rs:19`）。

12. **`insert_run` 查了两次 `question_row`**（`batch.rs:192`、`225`），且每次落库都在锁内做 `build_truth`（解析 + 正则），批次大时每条 run 都在锁内跑几十毫秒。可以在 `prepare` 阶段算好真值缓存进 `Prepared`。

13. **`extract_text` 兜底读 `reasoning_content`**（`client.rs:143`）。推理模型只回 reasoning 没回 content 时，会把思考过程当答案去抠 JSON，得到的多半是拒答语或垃圾。应当至少标一个 warning。

14. **每日备份在 `setup` 里同步执行**（`state.rs:136`）。`VACUUM INTO` 对几百 MB 的库要几秒，窗口出现前用户看到的是空白。放到 `spawn_blocking` 延后几秒做更好。

15. **`Throttle.last` 用 `tokio::sync::Mutex`**（`state.rs:28`）但锁内没有 `.await`，用 `parking_lot::Mutex` 就够；不是 bug，只是多了一次异步调度开销，且读者容易误以为这里需要异步锁。

16. **`capabilities/main.json` 对自定义命令不设限**，前端任何 JS（包括被注入的）都能调 `secret_set`。桌面单机应用风险有限，注释里也承认了，但 v0.2 收紧时记得给 `secret_*` 和 `push_to_plugin` 单独配 permission。

17. 小处：`export.rs:82` `row.batches.iter().max()` 按字符串比较 `"2026.9.6" > "2026.10.1"` 会得到相反结果（`.9` 排在 `.1` 后面）；`list_entries` 的 `dup_count` 是相关子查询（`library.rs:161-163`），每行都要按 signature 数一遍，条目上万后这页会明显变慢——可以改成先 `GROUP BY signature` 再 JOIN。
