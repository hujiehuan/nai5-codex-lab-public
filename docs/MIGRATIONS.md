# 数据库迁移、快照与只读校验

本地数据只有一个 SQLite 文件 `library.db`，放在应用数据目录（Tauri `app_data_dir`）的根目录。当前 schema 版本是 8（`src-tauri/src/db.rs` 的 `SCHEMA_VERSION`）。

## 连接与 WAL

每次打开连接都设置 `PRAGMA foreign_keys=ON; journal_mode=WAL; synchronous=NORMAL; temp_store=MEMORY`，并设 10 秒 busy timeout。连接放在一把互斥锁里，调用方只短时持锁，网络 IO 不在锁内做；命令层把同步的数据库操作放到 `spawn_blocking` 里，不阻塞 Tauri 的异步运行时。

WAL 模式下，还没合并的写入可能留在 `library.db-wal` 里。手动备份时要么先完全退出应用，要么把 `library.db`、`library.db-wal`、`library.db-shm` 三个文件一起复制；应用运行时只复制一个 `library.db`，可能拿到不完整的副本。

## 逐级迁移

迁移文件在 `src-tauri/migrations/`，按三位版本号命名，编译时用 `include_str!` 嵌入二进制，清单是 `db.rs` 里的 `MIGRATIONS`。打开数据库时：

1. 读 `schema_version` 表里的最大版本号（表不存在则视为 0）。
2. 对每个比当前版本大的迁移，开一个事务，执行整份 DDL 并写入新版本号，然后提交。
3. 任一语句失败，整版回滚，版本号保持不变，后面的版本也不会执行；下次启动从同一个版本重试。

已发布的迁移文件不再修改；需要新列或新表时追加下一个版本号的文件。

| 版本 | 内容 |
|---|---|
| 1 | 初始 schema：上游 release / 书目 / 条目及历史、图片、三态决策与审计、排除名单、配方解析缓存、同步日志、应用事件、反推评测的模板 / 规则集 / 模型端点 / 批次 / 运行 / 指标 / 人工评分；视图 `artist_library`、`pack_library` |
| 2 | `entry_image.sha256`：展示图落地后计算，旧行在导出完整池时补算 |
| 3 | `lab_run`：插件端实验运行记录的本地镜像 |
| 4 | `reverse_provider.kind`、`plugin_provider_id`：模型端点区分 OpenAI 兼容直连与插件端 provider |
| 5 | `reverse_run` 增加真值类型、重画结果、质量分 / 相似度分、两个泄漏计数 |
| 6 | `image_embedding` 表（每张展示图在每个模型下一行 f32 向量和图片哈希）；`reverse_run` 增加重画参数列 |
| 7 | `reverse_run` 增加检索命中记录（条目、标题、相似度、决策、是否走了兜底） |
| 8 | 扩展参考库视图以纳入多个来源，不改用户决策与向量 |

## 快照（`VACUUM INTO`）

`Db::backup_into` 先执行 `VACUUM INTO backups/library-YYYYMMDD-HHMMSS[-后缀].db.tmp`，成功后再改名去掉 `.tmp`，然后按后缀轮换旧文件。`VACUUM INTO` 在一个读事务里生成完整的单文件数据库，不需要附带 `-wal` 文件，也不用停掉应用。

| 时机 | 后缀 | 保留份数 |
|---|---|---|
| 同步发现上游有新 release（或强制同步、缺书）时，写入任何条目之前 | `presync` | 10 |
| 每天一次：启动 5 秒后在后台线程检查，当天已有就跳过 | 无 | 30 |
| 内置更新安装新版本之前 | `preupdate-<uuid>` | 全部保留 |

上游 release 没变时，同步只补缺失的图片，不做快照。快照失败只记一条 `app_event` 警告，不阻止同步；安装更新前的快照失败则中止安装，旧版本继续可用。

从快照恢复：完全退出应用，把当前的 `library.db`（以及 `-wal`、`-shm`）移到别处留存，再把选中的快照复制为 `library.db`。下次启动时，迁移会把它补到当前版本。

## 只读校验脚本

人工数据没法重新生成，所以升级前后各用 `scripts/check-library.py` 跑一次做对比。它以 `mode=ro` 打开数据库，并设置 `PRAGMA query_only=ON`，不会迁移或写入。报告只有完整性检查结果、schema 版本、各表行数和内容摘要（SHA-256）、决策分布和隐式未定条目数，不含条目 ID、提示词、备注原文或密钥。

```sh
DB="<应用数据目录>/library.db"   # macOS：~/Library/Application Support/<identifier>/，identifier 见 src-tauri/tauri.conf.json

# 升级前：先退出应用
python3 scripts/check-library.py "$DB" > before.json

# 升级后：启动新版、等它完成迁移后退出；这期间不要同步、筛选或改备注
python3 scripts/check-library.py "$DB" --compare before.json > after.json
```

退出码 0 表示决策、备注、审计记录、排除名单、撤销标记和隐式未定条目都和升级前一致。非 0 时保留备份和两份报告，先不要继续编辑。`PRAGMA integrity_check` 不是 `ok` 时，脚本直接报错退出。

脚本不检查图片缓存和向量索引，这两样都可以重新下载或重算；整个应用数据目录的备份仍然要做。

## 测试

```sh
python3 scripts/test-library-check.py                 # 校验脚本不改文件、能发现「条数不变但内容变了」
cd src-tauri
cargo test --lib db::tests                            # 每个版本只执行一次、失败整版回滚、快照轮换
cargo test --lib migration_integrity_tests            # 从任意历史版本升级到 8，人工数据逐行不变
```

`migration_integrity_tests` 用的是合成数据：3191 条条目（1313 条风格配方、1878 条图包）、502 条「加入」决策，外加显式 / 隐式未定、已下架条目、空备注、多行 Unicode 备注、排除名单和撤销记录。测试从版本 1–8 的每一个起点出发，逐级升到最新。每一步都比对决策、备注、审计和撤销标记的快照，并检查 `integrity_check` 与 `foreign_key_check`；最后再跑一次迁移，确认幂等。单个版本的升级测试（如 `migration_004_upgrades_v3_rows_to_openai`）在 `db.rs` 里。
