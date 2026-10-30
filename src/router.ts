import { createRouter, createWebHashHistory } from "vue-router";

export const router = createRouter({
  history: createWebHashHistory(),
  routes: [
    { path: "/", redirect: "/sync" },
    { path: "/sync", name: "sync", component: () => import("./views/SyncView.vue") },
    { path: "/codex/:codexId?", name: "codex", component: () => import("./views/CodexView.vue"), props: true },
    { path: "/studio", name: "studio", component: () => import("./views/StudioView.vue") },
    { path: "/reverse", name: "reverse", redirect: { path: "/studio", query: { mode: "reverse" } } },
    { path: "/lab/draw", name: "lab-draw", redirect: { path: "/studio", query: { mode: "draw" } } },
    { path: "/settings", name: "settings", component: () => import("./views/SettingsView.vue") },
  ],
});
