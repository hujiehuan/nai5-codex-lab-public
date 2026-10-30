import type { Decision, DecisionCounts, DecisionVia } from "../types";

/** 三态循环：未定 → 加入 → 不加入 → 未定 */
export function cycleDecision(current: Decision): Decision {
  if (current === "undecided") return "include";
  if (current === "include") return "exclude";
  return "undecided";
}

/** 键盘：A 加入、X 不加入、U 未定 */
export function keyToDecision(key: string): Decision | null {
  switch (key) {
    case "a": case "A": return "include";
    case "x": case "X": return "exclude";
    case "u": case "U": return "undecided";
    default: return null;
  }
}

export const DECISION_LABEL: Record<Decision, string> = {
  include: "加入",
  exclude: "不加入",
  undecided: "未定",
};

export const VIA_LABEL: Record<DecisionVia, string> = {
  click: "点击",
  key: "键盘",
  batch: "批量",
  import: "导入",
  undo: "撤销",
  rule: "规则",
};

/** 一条从 from 变为 to 后计数如何变化（纯函数，返回新对象）。 */
export function shiftCounts(counts: DecisionCounts, from: Decision, to: Decision): DecisionCounts {
  if (from === to) return { ...counts };
  return {
    ...counts,
    [from]: Math.max(0, counts[from] - 1),
    [to]: counts[to] + 1,
  };
}
