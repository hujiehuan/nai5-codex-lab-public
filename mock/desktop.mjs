// Synthetic local library. No upstream data or user decisions.
import { jpegFor } from "./plugin/images.mjs";
const ids = ["artist_nai5_personal", "nai5_community_pack"];
const entries = ids.flatMap((codex_id) =>
  Array.from({ length: 42 }, (_, i) => ({
    entry_id: `fixture-${codex_id}-${i}`,
    codex_id,
    first_seen_release: "mock-release",
    book_order: i,
    title: `合成示例 ${i + 1}${i === 2 ? " · 超长标题用于版面检查".repeat(15) : ""}`,
    tags_first_line: "artist:synthetic_example, luminous colors, soft lighting",
    rating: "safe",
    batches: ["fixture/1"],
    path: ["合成分类", i % 2 ? "柔和色彩" : "清晰线条"],
    decision: ["undecided", "include", "exclude"][i % 3],
    thumb_path: "data:image/jpeg;base64," + jpegFor(i),
    image_path: "data:image/jpeg;base64," + jpegFor(i),
    image_w: 360,
    image_h: 520,
    image_count: i % 5 === 0 ? 2 : 1,
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
        artist_nai5_personal: "风格词典",
        nai5_community_pack: "精选图包",
      }[codex_id],
      type: codex_id === "nai5_community_pack" ? "pack" : "string",
      version: "fixture/1",
      author: "Synthetic only",
      entry_count: items.length,
      counts: { ...counts(items), total: items.length },
      update_filters: [],
      tree: [
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
      tags: (e.tags_first_line + ", ").repeat(100),
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
    // Style-dictionary entries carry a parsed recipe (like artist_parse rows); packs do not.
    parse:
      e.codex_id === "artist_nai5_personal"
        ? {
            artists: [{ name: "synthetic_example", weight: 1 }],
            auxiliary: [
              { value: "luminous colors", weight: 1, kind: "style" },
              { value: "soft lighting", weight: 1, kind: "style" },
            ],
            negative_tail: [],
            signature: "synthetic_example",
            warnings: [],
          }
        : null,
    siblings: [],
  };
}
