// 反推单题面板的 tag 云着色：命中绿、漏灰、多余橙、违规命中红；矛盾词红框。纯函数。
import type { RunDetail } from "../types";

export type TagClass = "hit" | "miss" | "extra" | "violation" | "expected";
export interface CloudTag { tag: string; cls: TagClass; contradiction: boolean }
export interface TagClouds { truth: CloudTag[]; pred: CloudTag[] }

export const TAG_CLASS_LABEL: Record<TagClass, string> = {
  hit: "命中",
  miss: "漏",
  extra: "多余",
  violation: "违规命中",
  expected: "不计分",
};

type Source = Pick<RunDetail, "truth" | "expected_miss" | "pred" | "hits" | "extras" | "expected_hits" | "contradictions">;

export function buildTagClouds(d: Source): TagClouds {
  const hits = new Set(d.hits);
  const expectedMiss = new Set(d.expected_miss);
  const expectedHits = new Set(d.expected_hits);
  const contradictions = new Set(d.contradictions);
  const truth: CloudTag[] = d.truth.map((tag) => ({
    tag,
    cls: hits.has(tag) ? "hit" : expectedHits.has(tag) ? "violation" : expectedMiss.has(tag) ? "expected" : "miss",
    contradiction: contradictions.has(tag),
  }));
  const pred: CloudTag[] = d.pred.map((tag) => ({
    tag,
    // 模型输出里既不命中也不违规的就是多余，不需要再查 d.extras
    cls: hits.has(tag) ? "hit" : expectedHits.has(tag) ? "violation" : "extra",
    contradiction: contradictions.has(tag),
  }));
  return { truth, pred };
}
