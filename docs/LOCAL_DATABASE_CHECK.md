# 在 Mac 本机备份与校验决策

真实 `library.db` 留在 Mac，不上传服务器、聊天、Git 或临时共享目录。本次保持 schema 7，升级测试使用合成数据：1313 条画风、1878 条图包、502 条加入决策，覆盖 schema 1–7。

## 升级前先备份

1. 完全退出 NAI5 Codex Lab，包括正在运行的 `npm run tauri dev`。
2. Finder 中按 ⇧⌘G，打开 `~/Library/Application Support/moe.vivian.codexlab`。
3. 将整个目录复制到自己 Mac 的备份位置，确认包含 `library.db`；若存在 `library.db-wal` 和 `library.db-shm`，也必须一起保留。不要在程序写入时只拷贝单个数据库。
4. 在桌面端仓库目录的终端执行（`python3` 可用即可）：

```sh
python3 scripts/check-library.py "$HOME/Library/Application Support/moe.vivian.codexlab/library.db" > "$HOME/Desktop/nai5-before.json"
```

脚本用 SQLite 只读连接及 `query_only`，不会迁移或写入数据库。报告只有完整性、schema、决策分布、条数和内容摘要，不包含条目 ID、提示词、备注原文或密钥。

## 升级后对比

打开新版应用，待启动完成后退出。此时先不要同步、筛选或改备注，避免正常编辑导致摘要变化。执行：

```sh
python3 scripts/check-library.py "$HOME/Library/Application Support/moe.vivian.codexlab/library.db" --compare "$HOME/Desktop/nai5-before.json" > "$HOME/Desktop/nai5-after.json"
```

退出码 0 表示决策、备注、审计、黑名单、撤销标记及隐式未定条目一致。退出码非 0 时保留备份与两份报告，停止进一步编辑；不要覆盖旧备份。完整性报错也应停止升级。

报告不证明图片缓存或索引内容一致，重点保护不可再生的人工决策。整个应用目录备份仍然必要。

## 开发者合成测试

```sh
python3 scripts/test-library-check.py
cd src-tauri
cargo test --lib migration_integrity_tests
```

同一迁移 fixture 也已在隔离的 v0.4.1 基线上运行，迁移 SQL 本次没有变化。测试包含显式/隐式未定、已下架条目、空备注、多行 Unicode 备注、黑名单和撤销审计。
