"""Synthetic fixtures only; prove the local checker never changes the file."""
import hashlib
import json
from pathlib import Path
import sqlite3
import subprocess
import sys
import tempfile
import unittest

ROOT = Path(__file__).resolve().parent.parent

class CheckerTests(unittest.TestCase):
    def test_read_only_and_same_count_mutation_detection(self):
        with tempfile.TemporaryDirectory() as folder:
            db = Path(folder) / 'synthetic.db'
            report = Path(folder) / 'before.json'
            with sqlite3.connect(db) as c:
                for i, path in enumerate(sorted((ROOT / 'src-tauri/migrations').glob('*.sql')), 1):
                    c.executescript(path.read_text())
                    c.execute('INSERT INTO schema_version VALUES (?)', (i,))
                c.execute("INSERT INTO codex_book(codex_id,type,title,version,tree_json) VALUES('fixture','string','fixture','1','[]')")
                c.execute("INSERT INTO codex_entry(entry_id,codex_id,book_order,title,path_json,tags,rating,first_seen_release,last_seen_release) VALUES('secret-id','fixture',0,'fixture','[]','DO NOT EXPORT','safe','1','1')")
                c.execute("INSERT INTO library_membership VALUES('secret-id','artist','include','PRIVATE NOTE','now')")
            digest = hashlib.sha256(db.read_bytes()).hexdigest()
            command = [sys.executable, str(ROOT / 'scripts/check-library.py'), str(db)]
            first = subprocess.run(command, capture_output=True, text=True, check=True)
            self.assertEqual(hashlib.sha256(db.read_bytes()).hexdigest(), digest)
            self.assertNotIn('PRIVATE NOTE', first.stdout)
            self.assertNotIn('secret-id', first.stdout)
            self.assertNotIn('DO NOT EXPORT', first.stdout)
            report.write_text(first.stdout)
            same = subprocess.run(command + ['--compare', str(report)], capture_output=True, text=True)
            self.assertEqual(same.returncode, 0)
            with sqlite3.connect(db) as c:
                c.execute("UPDATE library_membership SET note='changed without changing any count'")
            changed = subprocess.run(command + ['--compare', str(report)], capture_output=True, text=True)
            self.assertNotEqual(changed.returncode, 0)
            self.assertEqual(json.loads(changed.stdout)['distribution'], json.loads(first.stdout)['distribution'])

if __name__ == '__main__':
    unittest.main()
