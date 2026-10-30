#!/usr/bin/env python3
"""Read-only local decision fingerprint. Does not export tags, IDs, notes or secrets.

Quit the app and back up its full data directory before upgrading. Run this before
and after on the Mac; keep reports there. It never migrates or modifies a database.
"""
import argparse
import hashlib
import json
import sqlite3
from pathlib import Path

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("database", type=Path)
parser.add_argument("--compare", type=Path, help="Previous report (local JSON)")
args = parser.parse_args()
path = args.database.expanduser().resolve(strict=True)
with sqlite3.connect(path.as_uri() + "?mode=ro", uri=True) as conn:
    conn.execute("PRAGMA query_only=ON")
    conn.execute("BEGIN")
    integrity = conn.execute("PRAGMA integrity_check").fetchone()[0]
    if integrity != "ok":
        raise SystemExit("数据库完整性检查失败，请停止升级并保留备份")
    tables = {}
    for table, order in [("library_membership", "entry_id"), ("membership_audit", "id"), ("artist_blacklist", "name")]:
        digest = hashlib.sha256()
        count = 0
        for row in conn.execute(f'SELECT * FROM "{table}" ORDER BY "{order}"'):
            digest.update(json.dumps(row, ensure_ascii=False, separators=(",", ":")).encode())
            digest.update(b"\n")
            count += 1
        tables[table] = {"rows": count, "sha256": digest.hexdigest()}
    undo = conn.execute("SELECT value FROM app_state WHERE key='undone_audit_ids'").fetchone()
    tables["undo"] = {"sha256": hashlib.sha256(json.dumps(undo).encode()).hexdigest()}
    distribution = [list(r) for r in conn.execute("SELECT library,decision,COUNT(*) FROM library_membership GROUP BY library,decision ORDER BY library,decision")]
    # Separate implicit undecided entries from explicitly recorded undecided decisions.
    implicit = conn.execute("SELECT COUNT(*) FROM codex_entry e LEFT JOIN library_membership m USING(entry_id) WHERE m.entry_id IS NULL").fetchone()[0]
    report = {"format": "nai5-decision-check/1", "schema": conn.execute("SELECT MAX(version) FROM schema_version").fetchone()[0], "integrity": integrity, "tables": tables, "distribution": distribution, "implicit_undecided": implicit}
print(json.dumps(report, ensure_ascii=False, indent=2))
if args.compare:
    previous = json.loads(args.compare.read_text())
    same = all(previous.get(k) == report[k] for k in ["tables", "distribution", "implicit_undecided"])
    if not same:
        raise SystemExit("校验不一致：决策/备注/审计/撤销标记或未定条目发生变化，请保留两份报告与备份")
