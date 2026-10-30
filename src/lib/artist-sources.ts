/** 风格词典页的书目：侧栏入口与路由高亮共用。 */
export const CODEX_ARTIST_ID = "artist_nai5_personal";

export const ARTIST_SOURCES = [{ id: CODEX_ARTIST_ID, label: "风格词典" }] as const;

export function isArtistBook(id: unknown): boolean {
  return ARTIST_SOURCES.some((source) => source.id === id);
}
