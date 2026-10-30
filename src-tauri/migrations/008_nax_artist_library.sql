-- NAX V5 与原画师词典共用本地画师库，保留各自的 codex_id / entry_id。
-- 只更新读取视图，不改用户的三态、备注、审计记录或原始向量。
DROP VIEW IF EXISTS artist_library;
CREATE VIEW artist_library AS
  SELECT e.*, m.decision, m.note AS member_note, m.decided_at, p.artists_json, p.signature
  FROM codex_entry e
  JOIN library_membership m ON m.entry_id = e.entry_id
  LEFT JOIN artist_parse p ON p.entry_id = e.entry_id
  WHERE e.codex_id IN ('artist_nai5_personal','nax_v5_artist','local_artist') AND m.decision='include' AND e.removed_at IS NULL;
