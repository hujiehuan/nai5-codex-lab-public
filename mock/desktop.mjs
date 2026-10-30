// Synthetic local library. No upstream data or user decisions.
import { jpegFor } from "./plugin/images.mjs";
const ids = [
  "artist_nai5_personal",
  "nax_v5_artist",
  "nai5_community_pack",
  "suozhang_nai5",
];
const CODEX_BOOKS = ["suozhang_nai5"];
// 所长法典里有少数条目没有配图（上游 image: null）
const imageless = (codex_id, i) => CODEX_BOOKS.includes(codex_id) && i % 7 === 6;
const entries = ids.flatMap((codex_id) =>
  Array.from({ length: 42 }, (_, i) => ({
    entry_id: `fixture-${codex_id}-${i}`,
    codex_id,
    first_seen_release:
      codex_id === "nax_v5_artist" ? "nax-v5-fixture" : "mock-release",
    book_order: i,
    title:
      codex_id === "nax_v5_artist"
        ? `synthetic_artist_${i + 1}`
        : `合成示例 ${i + 1}${i === 2 ? " · 超长标题用于版面检查".repeat(15) : ""}`,
    tags_first_line:
      codex_id === "nax_v5_artist"
        ? `artist:synthetic_artist_${i + 1}`
        : "artist:synthetic_example, luminous colors, soft lighting",
    rating: "safe",
    batches: ["fixture/1"],
    path:
      codex_id === "nax_v5_artist"
        ? ["NAX", "V5 Full"]
        : ["合成分类", i % 2 ? "柔和色彩" : "清晰线条"],
    decision:
      codex_id === "nax_v5_artist"
        ? "undecided"
        : ["undecided", "include", "exclude"][i % 3],
    thumb_path: imageless(codex_id, i)
      ? null
      : "data:image/jpeg;base64," + jpegFor(i),
    image_path: imageless(codex_id, i)
      ? null
      : "data:image/jpeg;base64," + jpegFor(i),
    image_w: imageless(codex_id, i) ? null : 360,
    image_h: imageless(codex_id, i) ? null : 520,
    image_count: imageless(codex_id, i)
      ? 0
      : codex_id === "nax_v5_artist"
        ? 1
        : i % 5 === 0
          ? 2
          : 1,
    dup_count: 1,
    is_new: i % 3 === 0,
  })),
);
const audit = [];
const notes = {};
const counts = (items) =>
  Object.fromEntries(
    ["include", "exclude", "undecided"].map((d) => [
      d,
      items.filter((e) => e.decision === d).length,
    ]),
  );
export function books() {
  return ids.map((codex_id) => {
    const items = entries.filter((e) => e.codex_id === codex_id);
    return {
      codex_id,
      title: {
        artist_nai5_personal: "画师词典",
        nax_v5_artist: "NAX · V5 画师测试库",
        nai5_community_pack: "精选图包",
        suozhang_nai5: "所长N5常规NovelAI个人法典（合成示例）",
      }[codex_id],
      type: CODEX_BOOKS.includes(codex_id)
        ? "codex"
        : codex_id === "nai5_community_pack"
          ? "pack"
          : "string",
      version: codex_id === "nax_v5_artist" ? "V5 Full" : "fixture/1",
      author: "Synthetic only",
      entry_count: items.length,
      counts: { ...counts(items), total: items.length },
      update_filters:
        codex_id === "nax_v5_artist"
          ? [{ id: "nax-v5-fixture", label: "NAX 当前同步", latest: true }]
          : [],
      tree:
        codex_id === "nax_v5_artist"
          ? [
              {
                name: "NAX",
                count: items.length,
                children: [
                  { name: "V5 Full", count: items.length, children: [] },
                ],
              },
            ]
          : [
              {
                name: "合成分类",
                count: 42,
                children: [
                  { name: "柔和色彩", count: 21, children: [] },
                  { name: "清晰线条", count: 21, children: [] },
                ],
              },
            ],
    };
  });
}
export function list(query) {
  let items = entries.filter(
    (e) =>
      e.codex_id === query.codex_id &&
      (!query.rating?.length || query.rating.includes(e.rating)) &&
      (!query.first_seen_release ||
        query.first_seen_release === e.first_seen_release) &&
      (!query.keyword ||
        `${e.title} ${e.tags_first_line}`.includes(query.keyword)) &&
      (!query.path_prefix?.length ||
        query.path_prefix.every((p, i) => e.path[i] === p)) &&
      (!query.multi_image_only || e.image_count > 1),
  );
  const resultCounts = counts(items);
  if (query.decision?.length)
    items = items.filter((e) => query.decision.includes(e.decision));
  if (query.sort_dir === "desc") items = [...items].reverse();
  return {
    items: items.slice(
      query.offset || 0,
      (query.offset || 0) + (query.limit || 100),
    ),
    total: items.length,
    counts: resultCounts,
  };
}
export function decide(entry_ids, decision) {
  let changed = 0;
  for (const id of entry_ids) {
    const entry = entries.find((e) => e.entry_id === id);
    if (entry && entry.decision !== decision) {
      audit.push({ entry_id: id, restored: entry.decision });
      entry.decision = decision;
      changed++;
    }
  }
  return { changed };
}
export function undo() {
  const item = audit.pop();
  if (item)
    entries.find((e) => e.entry_id === item.entry_id).decision = item.restored;
  return item || null;
}
export function note(id, text) {
  notes[id] = text;
}
export function detail(id) {
  const e = entries.find((e) => e.entry_id === id);
  if (!e) throw Error("Synthetic entry missing");
  return {
    entry: {
      ...e,
      entry_version: 1,
      tags:
        e.codex_id === "nax_v5_artist"
          ? e.tags_first_line
          : (e.tags_first_line + ", ").repeat(100),
      negative: null,
      character_prompts: [],
      note: null,
      is_new: e.is_new,
      update_batches: e.batches,
      asset_rev: "fixture",
      first_seen_release: e.first_seen_release,
      last_seen_release: "fixture/1",
      removed_at: null,
    },
    images: Array.from({ length: e.image_count }, (_, i) => ({
      ordinal: i,
      image_file: "fixture.jpg",
      original_file: null,
      raw_tag: null,
      local_path: e.image_path,
      thumb_path: e.thumb_path,
      original_local_path: null,
      bytes: 5000,
      error: null,
    })),
    membership: {
      decision: e.decision,
      member_note: notes[id] || null,
      decided_at: "2026-09-10",
    },
    audit: [],
    parse:
      e.codex_id === "nax_v5_artist"
        ? {
            artists: [{ name: e.title, weight: 1 }],
            auxiliary: [],
            negative_tail: [],
            signature: e.title,
            warnings: [],
          }
        : null,
    siblings: [],
  };
}
