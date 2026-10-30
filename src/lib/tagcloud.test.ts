import { describe, expect, it } from "vitest";
import { buildTagClouds } from "./tagcloud";

describe("tag cloud classification", () => {
  const clouds = buildTagClouds({
    truth: ["1girl", "smile", "red hair", "hat", "sword"],
    expected_miss: ["1girl", "red hair"],
    pred: ["smile", "red hair", "banana", "night"],
    hits: ["smile"],
    extras: ["banana", "night"],
    expected_hits: ["red hair"],
    contradictions: ["night"],
  });

  it("colours truth tags: hit / expected (unscored) / violation / miss", () => {
    expect(clouds.truth.map((t) => [t.tag, t.cls])).toEqual([
      ["1girl", "expected"],
      ["smile", "hit"],
      ["red hair", "violation"],
      ["hat", "miss"],
      ["sword", "miss"],
    ]);
  });

  it("colours predicted tags: hit / violation / extra", () => {
    expect(clouds.pred.map((t) => [t.tag, t.cls])).toEqual([
      ["smile", "hit"],
      ["red hair", "violation"],
      ["banana", "extra"],
      ["night", "extra"],
    ]);
  });

  it("flags contradictions on either side", () => {
    expect(clouds.pred.find((t) => t.tag === "night")?.contradiction).toBe(true);
    expect(clouds.pred.find((t) => t.tag === "smile")?.contradiction).toBe(false);
  });
});
