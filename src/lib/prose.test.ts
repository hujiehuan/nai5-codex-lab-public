// prose.ts：CONTRACT §11.1 散文判定的 TS 移植（与 Rust artist::is_prose_prompt / 插件 lab_sync.is_prose_prompt 同一语义），
// 以及题库条目的散文判定（后端 prose 键优先，否则从 truth_preview 退化判定）。
import { existsSync, readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";
import {
  PROSE_AVG_WORDS, PROSE_KIND_LABEL, avgWords, isProsePrompt, previewAsLines, proseKind, questionIsProse, stripArtistBlock, stripStyleTags,
} from "./prose";

/** 社区图包里的 XML 散文条目（画师块 + <artwork> 块）。 */
const XML_PROMPT = `<artist>
artist:omone_hokoma_agm, artist:jima
</artist>

<artwork>
  <scene bbox="[0,0,1,1]">
    A girl stands by the window in soft morning light, looking out at the rain.
  </scene>
  <pose_and_biomechanics>
    leaning forward, hands on the sill
  </pose_and_biomechanics>
</artwork>`;
/** 常规逗号标签串（含画师 / 年份 / 质量词）。 */
const TAG_LIST = "artist:jima, 1girl, solo, looking at viewer, cowboy shot, smile, white dress, outdoors, cherry blossoms, year 2025, masterpiece, best quality";

describe("stripArtistBlock / stripStyleTags", () => {
  it("删 <artist>…</artist>：大小写不敏感、跨行、多块都删；没闭合的原样保留", () => {
    expect(stripArtistBlock("<artist>\nartist:a, artist:b\n</artist>\n\n1girl, smile")).toBe("\n\n1girl, smile");
    expect(stripArtistBlock("<ARTIST>x</Artist>, solo")).toBe(", solo");
    expect(stripArtistBlock("<artist>a</artist>, x, <artist>b</artist>")).toBe(", x, ");
    expect(stripArtistBlock("<artist>a, b, 1girl")).toBe("<artist>a, b, 1girl");
    expect(stripArtistBlock("no block")).toBe("no block");
  });
  it("只去掉 <style> / </style> 标签本身，内容保留", () => {
    expect(stripStyleTags("<STYLE>a</style>")).toBe("a");
    expect(stripStyleTags("<style>\nyear 2025, cute\n</style>\n1girl")).toBe("\nyear 2025, cute\n\n1girl");
  });
});

describe("avgWords", () => {
  it("按 [,\\n] 切分后非空 token 的平均词数；没有 token 为 0", () => {
    expect(avgWords("")).toBe(0);
    expect(avgWords(" , ,\n")).toBe(0);
    expect(avgWords("a b c, d\ne f")).toBeCloseTo(2, 9);
    expect(avgWords("1girl, solo, looking at viewer")).toBeCloseTo(5 / 3, 9);
    expect(PROSE_AVG_WORDS).toBe(5);
  });
});

describe("proseKind / isProsePrompt（§11.1 四条规则按顺序）", () => {
  it("XML 散文条目（<artwork> 块）是散文；常规逗号标签串不是", () => {
    expect(proseKind(XML_PROMPT)).toBe("artwork");
    expect(isProsePrompt(XML_PROMPT)).toBe(true);
    expect(proseKind(TAG_LIST)).toBeNull();
    expect(isProsePrompt(TAG_LIST)).toBe(false);
  });
  it("代码围栏、独立成行的 XML 标签（前后空白也算）", () => {
    expect(proseKind("1girl, smile\n```\na girl\n```")).toBe("fence");
    expect(proseKind("1girl, smile\n<scene>\nA girl by the window.\n</scene>")).toBe("xml_line");
    expect(proseKind("  <pose_and_biomechanics>  \nstanding\n")).toBe("xml_line");
    expect(proseKind("<artists>\nartist:a\n</artists>\n1girl, smile")).toBe("xml_line");
  });
  it("平均词数 > 5 才算长句：(7+1)/2=4 → 标签；(12+1)/2=6.5 → 散文；整段英文 → 散文", () => {
    expect(proseKind("a girl standing under a big tree, smile")).toBeNull();
    expect(isProsePrompt("a girl standing under a big old tree by the river bank, smile")).toBe(true);
    expect(proseKind("An anime-style illustration of a young woman standing in a field of flowers, she wears a long white dress")).toBe("long_tokens");
  });
  it("<style> 块是标签题：独立成行的 <style> 不算 XML 标签，内容去掉标签后照常切分", () => {
    const t = "<artist>\nartist:a\n</artist>\n\n<style>\nyear 2025, fine fabric emphasis, 2::cute::\n</style>\n\n1girl, solo, smile";
    expect(proseKind(t)).toBeNull();
    expect(proseKind("<style> year 2025, round face </style>1girl, loli, smile")).toBeNull();
  });
  it("大写标签名、带属性的标签不匹配 XML 行规则（契约原文 [a-z_]+）；<artwork> 另有整块判定", () => {
    expect(proseKind("<Scene>\nx\n</Scene>")).toBeNull();
    expect(proseKind("<scene bbox=\"[0,0,1,1]\">\nx\n</scene>")).toBeNull();
    expect(proseKind("<artwork>\n<scene bbox=\"[0,0,1,1]\">\nx\n</scene>\n</artwork>")).toBe("artwork");
    expect(proseKind("<ARTWORK>\nx\n</ARTWORK>")).toBeNull();
  });
  it("画师块整块剥掉：块里夹着 <scene> 行也不算散文；没闭合的画师块留下就会命中 XML 行", () => {
    expect(isProsePrompt("<artist>\n<scene>\nartist:a\n</scene>\n</artist>\n1girl, smile")).toBe(false);
    expect(proseKind("<artist>\nartist:a, artist:b\n</artists>\n1girl, smile")).toBe("xml_line");
  });
  it("空串、只有标签、中文散文（没有空格，平均词数规则不命中，与 Rust 一致）都不是散文", () => {
    expect(proseKind("")).toBeNull();
    expect(proseKind("1girl, solo, looking at viewer, cowboy shot")).toBeNull();
    expect(proseKind("1girl, solo, \n飞行在天空之中, 下面是城市夜景, 背景放着烟花")).toBeNull();
  });
  it("每种 kind 都有徽标 title 文字", () => {
    for (const k of ["artwork", "fence", "xml_line", "long_tokens"] as const) expect(PROSE_KIND_LABEL[k]).toBeTruthy();
  });
});

describe("previewAsLines / questionIsProse（题库条目）", () => {
  it("truth_preview 的 token 还原成行：去空白、丢空 token", () => {
    expect(previewAsLines("a, b ,, c")).toBe("a\nb\nc");
    expect(previewAsLines("")).toBe("");
  });
  it("后端给了 prose 就用它，不看 truth_preview", () => {
    expect(questionIsProse({ prose: true, truth_preview: "1girl, smile" })).toBe(true);
    expect(questionIsProse({ prose: false, truth_preview: "<artwork>, <scene>, a girl stands by the window in soft morning light" })).toBe(false);
  });
  it("没给 prose：从 truth_preview 退化判定——XML token 独立成 token 即命中；常规标签不命中", () => {
    expect(questionIsProse({ truth_preview: "artist:jima, <artwork>, <scene>, a girl stands by the window in soft morning light" })).toBe(true);
    expect(questionIsProse({ truth_preview: "artist:a, <scene>, a girl by the window., </scene>" })).toBe(true);
    expect(questionIsProse({ truth_preview: "artist:jima, 1girl, solo, looking at viewer, cowboy shot, smile, white dress" })).toBe(false);
    expect(questionIsProse({ truth_preview: "" })).toBe(false);
  });
});

/**
 * 上游社区图包全量样本（1898 条，不在仓库里）：路径来自 `NAI5_PACK_FIXTURE`，缺省是调度脚本的 scratchpad；文件不存在静默跳过。
 * 按 §11.1 原文实现得到 159 条散文（与 Rust / 插件端一致；契约里写的 232 + 29 复现不出来，见 docs/CONTRACT_CHANGES.md）。
 * 同目录若有插件端 / Rust 端产出的 `prose_ids_plugin.json` / `prose_ids_rust.json`，要求 id 集合完全一致。
 */
describe("上游样本（存在时）", () => {
  const SCRATCH = "";
  const path = process.env.NAI5_PACK_FIXTURE?.trim() || SCRATCH;
  const present = existsSync(path);
  it.skipIf(!present)("散文数在 150..175 之间，且与插件端 / Rust 端的 id 列表一致", () => {
    const entries = JSON.parse(readFileSync(path, "utf8")) as { id: string; tags?: string }[];
    const ids = entries.filter((e) => isProsePrompt(e.tags ?? "")).map((e) => e.id);
    expect(entries.length).toBeGreaterThan(0);
    expect(ids.length).toBeGreaterThanOrEqual(150);
    expect(ids.length).toBeLessThanOrEqual(175);
    const dir = path.slice(0, path.lastIndexOf("/") + 1);
    for (const name of ["prose_ids_plugin.json", "prose_ids_rust.json"]) {
      const f = `${dir}${name}`;
      if (!existsSync(f)) continue;
      const other = JSON.parse(readFileSync(f, "utf8")) as string[];
      expect([...ids].sort(), name).toEqual([...other].sort());
    }
  });
});
