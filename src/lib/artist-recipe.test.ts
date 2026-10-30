import { describe, expect, it } from "vitest";
import {
  ARTIST_POOL_SUFFIX,
  canonicalArtistTagName,
  classify,
  extractArtists,
  formatArtistPoolCore,
  formatArtistPoolLine,
  formatPoolWeight,
  formatToken,
  parseArtistRecipe,
  roundWeight,
  stripRecipeMarkup,
} from "./artist-recipe";

describe("artist recipe grammar", () => {
  it("parses full-width punctuation, bracket emphasis, and numerical emphasis", () => {
    const tokens = parseArtistRecipe("{{artist:foo，bar}}, 0.4::impasto ::，-2::no halo ::");
    expect(tokens.map((token) => token.value)).toEqual(["artist:foo", "bar", "impasto", "no halo"]);
    expect(tokens[0].weight).toBeGreaterThan(1);
    expect(tokens[0].kind).toBe("artist");
    expect(tokens[2]).toMatchObject({ weight: 0.4, kind: "style" });
    expect(tokens[3]).toMatchObject({ weight: -2, kind: "negative" });
  });

  it("protects every token inside a numerical negative-emphasis segment", () => {
    const tokens = parseArtistRecipe("-3::artist collaboration, text, variations::");
    expect(tokens.map((token) => [token.value, token.weight, token.kind])).toEqual([
      ["artist collaboration", -3, "negative"],
      ["text", -3, "negative"],
      ["variations", -3, "negative"],
    ]);
  });

  it("closes a numerical segment at the trailing :: and resets weight afterwards", () => {
    const tokens = parseArtistRecipe("1.2::artist:a, artist:b::, artist:c");
    expect(tokens.map((token) => [token.value, token.weight])).toEqual([
      ["artist:a", 1.2],
      ["artist:b", 1.2],
      ["artist:c", 1],
    ]);
  });

  it("uses bracket nesting depth as 1.05^level and strips wrapping parentheses", () => {
    const tokens = parseArtistRecipe("[[soft]], (plain), masterpiece");
    expect(tokens[0]).toMatchObject({ value: "soft", weight: roundWeight(Math.pow(1.05, -2)) });
    expect(tokens[1]).toMatchObject({ value: "plain", weight: 1 });
    expect(tokens[2]).toMatchObject({ value: "masterpiece", kind: "quality" });
  });

  it("ignores empty tokens and stray separators", () => {
    expect(parseArtistRecipe("")).toEqual([]);
    expect(parseArtistRecipe(",, ::, ，")).toEqual([]);
    expect(parseArtistRecipe("a,,b").map((token) => token.value)).toEqual(["a", "b"]);
  });

  it("classifies year, quality, negative, style and other", () => {
    expect(classify("year 2025")).toBe("year");
    expect(classify("year_2024")).toBe("year");
    expect(classify("best quality")).toBe("quality");
    expect(classify("no halo")).toBe("negative");
    expect(classify("cinematic lighting")).toBe("style");
    expect(classify("1girl")).toBe("other");
    expect(classify("Artist: foo")).toBe("artist");
  });

  it("rounds and clamps weights", () => {
    expect(roundWeight(1.006)).toBe(1.01);
    expect(roundWeight(1.2345)).toBe(1.23);
    expect(roundWeight(42)).toBe(10);
    expect(roundWeight(-42)).toBe(-10);
  });

  it("normalizes qualified Danbooru names and known aliases", () => {
    expect(canonicalArtistTagName("gochisousama (tanin050)")).toBe("gochisousama_(tanin050)");
    expect(canonicalArtistTagName(" Channel_(_Caststation) ")).toBe("channel_(caststation)");
    expect(canonicalArtistTagName("machi_(7769)")).toBe("machi_(machi0910)");
    expect(canonicalArtistTagName("ＦＯＯ　bar")).toBe("foo_bar");
  });

  it("formats tokens with the original spacing rule", () => {
    expect(formatToken({ value: "masterpiece", weight: 1 })).toBe("masterpiece");
    expect(formatToken({ value: "masterpiece", weight: 1.0004 })).toBe("masterpiece");
    expect(formatToken({ value: "impasto", weight: 0.6 })).toBe("0.6::impasto ::");
  });
});

describe("artist pool serialization (CONTRACT §3.1)", () => {
  it("strips artist/style markup and newlines before parsing", () => {
    expect(stripRecipeMarkup("<artist>artist:a</artist>\nartist:b")).toBe(" artist:a , artist:b");
  });

  it("extracts artists in order, canonical, deduplicated, ignoring negatives", () => {
    const artists = extractArtists("1.2::artist:Foo Bar ::, artist:baz, artist:foo_bar, -1::artist:evil::, masterpiece");
    expect(artists).toEqual([
      { name: "foo_bar", weight: 1.2 },
      { name: "baz", weight: 1 },
    ]);
  });

  it("treats bare names as artists only when asked (单画师词典)", () => {
    expect(extractArtists("inkwell")).toEqual([]);
    expect(extractArtists("inkwell", { bareNames: true })).toEqual([{ name: "inkwell", weight: 1 }]);
    expect(extractArtists("masterpiece", { bareNames: true })).toEqual([]);
  });

  it("formats weights with at most two decimals and no trailing zeros", () => {
    expect(formatPoolWeight(1.5)).toBe("1.5");
    expect(formatPoolWeight(2)).toBe("2");
    expect(formatPoolWeight(0.333)).toBe("0.33");
  });

  it("omits weight 1 and writes w::artist:name:: otherwise, no space before ::", () => {
    const core = formatArtistPoolCore([
      { name: "foo", weight: 1 },
      { name: "Bar Baz", weight: 1.25 },
      { name: "qux", weight: 0.999999 },
    ]);
    expect(core).toBe("artist:foo, 1.25::artist:bar_baz::, artist:qux");
  });

  it("appends the fixed quality suffix", () => {
    expect(formatArtistPoolLine([{ name: "foo", weight: 1 }])).toBe(`artist:foo, ${ARTIST_POOL_SUFFIX}`);
    expect(formatArtistPoolLine([])).toBe("");
  });
});
