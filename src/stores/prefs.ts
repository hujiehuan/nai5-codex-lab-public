import { defineStore } from "pinia";
import { ref } from "vue";
import { isCardSize, loadCardSize, saveCardSize, stepCardSize, type CardSize } from "../lib/grid";

/**
 * 全局偏好（不随书、不进后端设置）。目前只有词典页卡片档位：
 * 存 localStorage `codexlab.cardSize`（"s" | "m" | "l" | "xl"），store 创建时读取（词典页首次进入即创建），
 * 非法值回默认 "m"。
 */
export const usePrefs = defineStore("prefs", () => {
  const cardSize = ref<CardSize>(loadCardSize());

  function setCardSize(size: CardSize) {
    if (!isCardSize(size) || size === cardSize.value) return;
    cardSize.value = size;
    saveCardSize(size);
  }
  /** 放大一档（⌘⇧=），到 xl 不动 */
  function biggerCard() {
    setCardSize(stepCardSize(cardSize.value, 1));
  }
  /** 缩小一档（⌘⇧-），到 s 不动 */
  function smallerCard() {
    setCardSize(stepCardSize(cardSize.value, -1));
  }

  return { cardSize, setCardSize, biggerCard, smallerCard };
});
