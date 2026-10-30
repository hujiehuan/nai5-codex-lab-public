/** 画师页共用入口；每个来源仍保留独立的筛选状态与条目决策。 */
export const CODEX_ARTIST_ID = "artist_nai5_personal";
export const NAX_ARTIST_ID = "nax_v5_artist";
export const NAX_ARTIST_URL =
  "https://nax.moe/?gallery=danbooru-artist-tags-2-v5";

export const ARTIST_SOURCES = [
  { id: CODEX_ARTIST_ID, label: "法典画师" },
  { id: NAX_ARTIST_ID, label: "NAX · V5 画师" },
] as const;

export function isArtistBook(id: unknown): boolean {
  return ARTIST_SOURCES.some((source) => source.id === id);
}
