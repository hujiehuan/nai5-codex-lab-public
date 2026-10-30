/**
 * 插件 HTTP 接口的合成数据（离线开发用），只保留桌面端仍会读取的两个接口：
 *   page/providers      → 插件已配置的 LLM 名单（plugin_providers，也是模型名单的候选来源）；
 *   page/library/status → 插件 lab_sync.status() / _pool_status()（plugin_library_status）。
 *
 * 形状照插件处理函数的返回值整理；插件改了返回值，这里要同步更新，
 * 否则你会对着一份真实后端永远不会返回的数据调 UI。
 * 数据刻意带一条超长 provider id 和五位数计数，用来检查布局会不会被撑破。
 */

export const PROVIDERS = [
  "gemini-2.5-pro-视觉",
  "gemini-2.5-flash-中转",
  "gpt-4o-识图",
  "claude-sonnet-4-识图",
  "qwen2.5-vl-72b-本地部署",
  "glm-4v-plus",
  "doubao-vision-pro-32k",
  "deepseek-v3-文本评委",
  "moonshot-v1-128k-vision",
  // 故意超长的 provider id：下拉框、标签、表格列都得能扛
  "自建中转-新加坡节点-gemini-2.5-pro-exp-0827-高并发-备用链路-仅供反推评分使用",
]

const LAB_DIR = "data/plugin_data/astrbot_plugin_vivian/lab"

export function libraryStatus() {
  return {
    plugin_version: "2.0.0",
    filtered: {
      imported_at: "2026-09-08T22:14:07+08:00",
      release_id: "rel_20260908_2213_9f3c1a",
      exported_at: "2026-09-08T22:13:51+08:00",
      app_version: "nai5-codex-lab 0.4.2",
      counts: { nai5: 95, pack: 2158, kept_old: 431, dropped_blacklist: 33 },
      file: `${LAB_DIR}/library_export.json`,
      size_bytes: 4823914,
      backups: ["library_export.json.bak-20260908-221407", "library_export.json.bak-20260901-100233"],
    },
    full: {
      imported_at: "2026-09-09T09:02:41+08:00",
      release_id: "rel_20260909_0902_71bd44",
      exported_at: "2026-09-09T09:02:18+08:00",
      app_version: "nai5-codex-lab 0.4.2",
      counts: { artist_nai5_personal: 9421, nai5_community_pack: 2158 },
      file: `${LAB_DIR}/full_pool.json`,
      size_bytes: 41287336,
      backups: [
        "full_pool.json.bak-20260909-090241",
        "full_pool.json.bak-20260906-183012",
        "full_pool.json.bak-20260903-114455",
        "full_pool.json.bak-20260830-201907",
        "full_pool.json.bak-20260825-093311",
      ],
    },
  }
}
