# 前端代码导览：带着读 `src/`

> 面向：熟悉 Java / Python、第一次读 Vue / TypeScript 代码的读者。
> 范围：`src/` 全部 `.ts/.vue`（含 `lib/*.test.ts`）+ `index.html`、`vite.config.ts`、`tsconfig.json`、`package.json`。
> 版本：写于 v0.1.0，之后代码有较大扩展；行号以 v0.1.0 为准，仅作阅读路线参考。
> 我实际跑过：`npx vitest run` 6 文件 50 用例全绿；`npx vue-tsc --noEmit` 0 错误。

读法建议：先看 §1 地图和 §2 启动，然后挑 §3 里任意一条走读，**边读边开对应文件**。§5 概念清单可以当字典随时翻。

---

## 1. 目录地图（每个文件一句话）

**壳与配置**

| 文件 | 一句话 |
|---|---|
| `index.html` | 唯一的 HTML，只有一个 `<div id="app">` 和一行 `<script type="module" src="/src/main.ts">`，相当于 Java 的 `main` 类入口声明。 |
| `package.json` | 依赖与脚本：`dev`（vite 开发服务器）、`build`（先 `vue-tsc` 类型检查再打包）、`test`（vitest）、`typecheck`。 |
| `vite.config.ts` | 打包器配置；`server.port 5173` 给 Tauri 用，`test.include` 告诉 vitest 只跑 `src/**/*.test.ts`。 |
| `tsconfig.json` | TS 编译选项：`strict`、`noUnusedLocals/Parameters` 全开，等于 `-Werror` 级别的严格度。 |
| `src/main.ts` | 一行启动：创建 Vue 应用 → 装 Pinia → 装路由 → 挂到 `#app`。 |
| `src/App.vue` | 根组件：左侧导航 + `<RouterView>` + 全局 `<Toast>`；挂载时拉 `app_info`。 |
| `src/router.ts` | 4 条路由，全部懒加载；`/` 重定向到 `/sync`。 |
| `src/styles.css` | 全局 CSS 变量（`--accent`、`--green` 等）与暗色模式；组件内的 `<style scoped>` 都引用这些变量。 |
| `src/types.ts` | 前后端 DTO 的**唯一权威形状**（全 snake_case），对应 Rust 的 `models.rs`。 |
| `src/api.ts` | 后端命令的薄封装：每个 Tauri 命令一个函数，`invoke<T>("cmd", {args})`。 |

**状态（`src/stores/`，Pinia）**

| 文件 | 一句话 |
|---|---|
| `toast.ts` | 右下角提示队列，最多 5 条，`error(e, prefix)` 是全项目统一的错误出口。 |
| `settings.ts` | 设置对象 + 钥匙串"是否已设"标志；`ensureLoaded()` 做去重加载。 |
| `sync.ts` | 同步状态、日志、进度事件；`start()` 持有 `Channel<SyncEvent>`。 |
| `codex.ts` | 词典页的一切：书目、筛选、分页条目、三态计数、选择/焦点/抽屉；乐观更新在这里。 |
| `reverse.ts` | 反推实验室的一切：模板/规则集/模型、题库、勾选、批次、单题详情、对比。 |

**页面（`src/views/`）与组件（`src/components/`）**

| 文件 | 一句话 |
|---|---|
| `views/SyncView.vue` | 同步页：按钮、进度卡、完成横幅、最近 20 次日志表。 |
| `views/CodexView.vue` | 词典页骨架：左树 + 筛选栏 + 网格 + 抽屉；**所有键盘/鼠标手势的分发中心**。 |
| `views/ReverseLabView.vue` | 三栏布局（题库/配置/结果）+ 底部对比面板 + 单题抽屉。（v0.5.5 起并入总实验室 `views/StudioView.vue`，该文件已删除。） |
| `views/SettingsView.vue` | 设置表单、钥匙串、导出/推送、黑名单、事件日志；自己直接调 `api`，不经 store。 |
| `components/CodexTree.vue` | 递归目录树（组件自己引用自己）。 |
| `components/FilterBar.vue` | 筛选栏：所有控件都是 store 的"视图"，无本地状态（关键词除外，做了防抖）。 |
| `components/EntryGrid.vue` | 虚拟网格：只渲染可视行 ±2，滚到底发 `loadMore`。 |
| `components/EntryCard.vue` | 单张卡片：三态样式 + 角标；不知道自己在第几个，只会发事件。 |
| `components/EntryDrawer.vue` | 右侧详情抽屉：轮播、tags、画师解析、备注、审计历史、下载。 |
| `components/ProgressBar.vue` | 进度条；`value=null` 表示不确定（跑马灯）。 |
| `components/Toast.vue` | 把 `toast.items` 渲染出来，点一下消失。 |
| `components/reverse/QuestionBank.vue` | 题库列表 + 筛选 + 本地图片。 |
| `components/reverse/RunConfig.vue` | 模板/规则集选择与"新建版本"编辑器、模型勾选、运行按钮。 |
| `components/reverse/ProviderManager.vue` | 模型管理弹窗（左列表右表单）。 |
| `components/reverse/BatchResults.vue` | 批次选择 + 按模型汇总表 + 逐 run 表。 |
| `components/reverse/RunDetailPanel.vue` | 单题抽屉：指标、两朵 tag 云、原始输出、人工打分。 |
| `components/reverse/TagCloud.vue` | 一朵 tag 云；纯展示。 |
| `components/reverse/ComparePanel.vue` | 两个批次逐题 F1 对比，只列有变化的题。 |

**纯函数（`src/lib/`，全部有单测）**

| 文件 | 一句话 |
|---|---|
| `grid.ts` | 虚拟网格的所有算术：列数、可视行、单元坐标、焦点移动、滚动定位。 |
| `query.ts` | 词典筛选的形状、默认值、localStorage 读写校验、转成后端 `EntryQuery`。 |
| `artist-recipe.ts` | 画师串解析器（移植自 langbai）+ 本项目的画师池序列化。 |
| `tagcloud.ts` | `RunDetail` → 两组带颜色分类的 tag。 |
| `decision.ts` | 三态循环、A/X/U 键映射、计数增减。 |
| `format.ts` | 百分比/毫秒/字节/时间格式化。 |
| `debounce.ts` | 带 `cancel` 的防抖。 |
| `clipboard.ts` | 复制文本，带旧 API 回退。 |

---

## 2. 启动时发生了什么（按执行顺序）

**① 浏览器（Tauri 的 WebView）加载 `index.html`**，看到：

```html
<!-- index.html:9-10 -->
<div id="app"></div>
<script type="module" src="/src/main.ts"></script>
```

`type="module"` 表示这是 ES 模块，可以 `import`。Vite 在开发时会把 `.ts/.vue` 即时编译成浏览器能跑的 JS。

**② `main.ts` 执行**：

```ts
// src/main.ts:7
createApp(App).use(createPinia()).use(router).mount("#app");
```

类比 Spring Boot 的 `SpringApplication.run()`：`createApp(App)` 建容器，`.use(createPinia())` 注册状态管理插件（相当于注册一个全局的 bean 工厂），`.use(router)` 注册路由，`.mount("#app")` 把根组件渲染进那个 `div`。注意 `import "./styles.css"`（`main.ts:5`）——在 Vite 里 CSS 也是可 import 的模块，这行只是为了让它被打包进去。

**③ 根组件 `App.vue` 挂载**。`<script setup>` 里的顶层代码在组件创建时执行一次（类似构造函数）：

```ts
// src/App.vue:8-16
const info = ref<AppInfo | null>(null);
const bootError = ref<string | null>(null);
onMounted(async () => {
  try {
    info.value = await api.appInfo();
  } catch (e) {
    bootError.value = errMessage(e);
  }
});
```

`onMounted` 注册一个"DOM 已经渲染完"的钩子，类似 JavaFX 的 `initialize()` 或 Android 的 `onCreate` 之后。这里第一次调用后端：`api.appInfo()`。

**④ `api.appInfo()` 是什么**：

```ts
// src/api.ts:16
appInfo: () => invoke<T.AppInfo>("app_info"),
```

`invoke` 来自 `@tauri-apps/api/core`，是前端到 Rust 的 RPC：字符串 `"app_info"` 是 Rust 侧 `#[tauri::command] fn app_info` 的名字，返回 `Promise<AppInfo>`。泛型参数 `<T.AppInfo>` 只是告诉 TS "返回值长这样"，运行时没有校验（这点和 Java 的泛型擦除一个味道）。

**⑤ 模板渲染**：`App.vue:29` 的 `<div class="meta" v-if="info">v{{ info.version }} · {{ info.latest_release ?? "未同步" }}</div>`——`info` 从 `null` 变成对象时，这一行会自动出现。这就是响应式：你改 `info.value`，视图自己更新，不用手动 `setText`。

**⑥ 路由决定第一个页面**。`<RouterView />`（`App.vue:33`）是一个占位符，路由匹配到谁就把谁渲染在这。当前 URL 是 `#/`：

```ts
// src/router.ts:4-7
history: createWebHashHistory(),
routes: [
  { path: "/", redirect: "/sync" },
  { path: "/sync", name: "sync", component: () => import("./views/SyncView.vue") },
```

`createWebHashHistory` 用 `#/sync` 这种 hash 路由，因为 Tauri 是本地文件协议，没有服务器可以处理 `/sync` 路径。`component: () => import(...)` 是**懒加载**：返回一个函数，路由第一次匹配到时才去下载/执行这个模块（打包后是独立 chunk）。

**⑦ `SyncView.vue` 挂载**，`onMounted` 里 `await sync.refresh()`（`SyncView.vue:12-13`）→ `sync.ts:30-38` 并行调 `sync_status` 和 `sync_logs`。至此首屏完成。

**注意一个隐含顺序**：`useCodex()` 这类 store 只在第一次被某个组件调用时才创建（`codex.ts:11` 的 `defineStore` 只是注册工厂）。首屏是同步页，所以 codex store 此时还不存在——直到你点"画师词典"。

---

## 3. 端到端走读

### 3a. 启动 → 第一个页面拿到 app_info

上面 §2 已经走完。补一句错误路径：`api.appInfo()` 抛出的 `e` 是 `unknown` 类型（TS 的 catch 变量默认如此），`errMessage`（`api.ts:10-13`）用 `isAppError` 类型守卫判断它是不是后端约定的 `{code, message}`，是就拼成 `"message（code）"`，否则退回 `Error.message`。这个函数是全项目错误显示的唯一入口，`toast.error` 也调它（`toast.ts:25-27`）。

### 3b. 词典页：进入 → 筛选 → 网格 → 点击 → 乐观更新 → 回滚

**Step 1：点击侧栏"画师词典"** → `<RouterLink to="/codex/artist_nai5_personal">`（`App.vue:24`）→ 路由匹配 `router.ts:8`：

```ts
{ path: "/codex/:codexId?", name: "codex", component: () => import("./views/CodexView.vue"), props: true },
```

`:codexId?` 是路径参数（`?` 表示可选），`props: true` 让它自动变成组件的 prop，于是 `CodexView.vue:13` 的 `defineProps<{ codexId?: string }>()` 能直接拿到 `"artist_nai5_personal"`。

**Step 2：`CodexView` 挂载，调 `enter()`**（`CodexView.vue:39`）→ `enterInner()`：

```ts
// src/views/CodexView.vue:31-37
if (!props.codexId) {
  await router.replace(`/codex/${DEFAULT_ID}`);
  return;
}
const firstSeen = typeof route.query.first_seen === "string" ? route.query.first_seen : null;
await codex.open(props.codexId, { firstSeen });
if (firstSeen) await router.replace({ path: route.path });
```

`first_seen` 是同步页横幅带过来的 URL 查询参数，用完即从 URL 上抹掉（`router.replace`），但会写进筛选并持久化。

**Step 3：`codex.open()` 恢复筛选并拉第一页**（`stores/codex.ts:65-81`）：

```ts
await settingsStore.ensureLoaded();
await Promise.all([books.value.length ? Promise.resolve() : loadBooks(), loadAppInfo()]);
if (codexId.value !== id) {
  codexId.value = id;
  filter.value = loadFilter(id, defaults.value);
  selected.value = new Set(); anchor.value = null; focus.value = null; drawerEntryId.value = null;
}
```

先确保设置已加载，再并行拉书目和 app_info。切书时从 localStorage 恢复该书的筛选（`lib/query.ts:64-72`，见 §4.3），清空选择。最后 `await reload()`。

**Step 4：`reload()` 构造查询并调后端**（`codex.ts:96-116`）：

```ts
seq += 1;
const my = seq;
loading.value = true;
try {
  const page = await api.listEntries(buildEntryQuery(id, filter.value, 0, PAGE_SIZE));
  if (my !== seq) return;
  items.value = page.items;
```

`seq`/`my` 是一个**手写的请求序号**：如果用户在请求返回前又改了筛选，`seq` 已经变了，旧响应直接丢弃。Java 里你可能用 `AtomicInteger` + 比较做同样的事。`buildEntryQuery` 把 UI 侧的 `CodexFilter` 翻译成后端 `EntryQuery`（`query.ts:86-99`），空值不发送。`api.listEntries`（`api.ts:29`）→ Rust `list_entries` → 返回 `{ total, counts, items }`（`types.ts:108`）。

**Step 5：`EntryGrid` 只渲染可视行**。`CodexView.vue:142-155` 把 `codex.items`、`codex.selected`、`codex.focus` 作为 props 传给 `<EntryGrid>`。Grid 内部（`EntryGrid.vue:32-47`）：

```ts
const columns = computed(() => computeColumns(width.value));
const rows = computed(() => rowCount(props.items.length, columns.value));
const range = computed(() => visibleRowRange(scrollTop.value, height.value, ROW_HEIGHT, rows.value));
const totalHeight = computed(() => (rows.value ? GRID_PADDING * 2 + rows.value * ROW_HEIGHT - GRID_GAP : 0));
const visible = computed(() => {
  const out = [];
  const startIdx = range.value.start * cols;
  const endIdx = Math.min(props.items.length, range.value.end * cols);
  for (let i = startIdx; i < endIdx; i += 1) { const pos = cellPosition(i, cols); out.push({ index: i, entry: props.items[i], top: pos.top, left: pos.left }); }
```

四个 `computed` 串成一条依赖链：容器宽度变 → 列数变 → 行数变 → 可视区间变 → `visible` 变。模板里（`EntryGrid.vue:106-117`）外层一个 `spacer` div 撑出总高度让滚动条正确，里面只 `v-for` `visible` 这几十张卡片，每张用 `position:absolute; top/left` 定位。滚动时 `onScroll`（`:49-51`）只更新一个数字 `scrollTop`，其余全靠 computed 自动重算。

宽高从哪来？`onMounted` 里 `measure()` 读 `clientWidth/clientHeight`，再用 `ResizeObserver` 监听变化（`:60-66`）。

分页触发在 `:69-75` 的 `watch`：当 `range`/`rows`/`hasMore`/`loading` 任一变化，且 `shouldLoadMore(range, rows)`（离底不到 3 行）为真，就 `emit("loadMore")` → `CodexView.vue:153` `@load-more="codex.loadMore()"` → `codex.ts:118-135` 用 `items.value.length` 做 offset 拉下一页并追加。

**Step 6：`EntryCard` 三态样式**。卡片只收三个 props（`EntryCard.vue:8-12`：`entry/selected/focused`），样式全靠 class：

```vue
<!-- src/components/EntryCard.vue:28-31 -->
<div class="card"
  :class="[entry.decision, { selected, focused }]"
  :style="{ width: CARD_WIDTH + 'px', height: CARD_HEIGHT + 'px' }"
```

`:class` 的数组语法：第一项是字符串 `entry.decision`（`"include"/"exclude"/"undecided"` 之一直接当 class 名），第二项是对象 `{selected, focused}`，值为 true 时加对应 class。对应 CSS 在 `:65-68`：`.card.include` 绿色 2px 边框、`.card.exclude` 灰边 + 图片 `grayscale(0.6)` + 黑色蒙层、`.card.selected` 紫色 outline、`.card.focused` 紫色光晕。所以"改决策"在视觉上只是换一个字符串。

**Step 7：点击图片**。`EntryCard.vue:36` 的 `<div class="thumb" @click.stop="emit('imageClick', $event)">`——`.stop` 阻止冒泡到外层卡片的 `cardClick`。事件一路向上：`EntryCard` 发 `imageClick(ev)` → `EntryGrid.vue:114` 补上 index 再发 `imageClick(v.index, ev)` → `CodexView.vue:44-53`：

```ts
function onImageClick(index: number, ev: MouseEvent) {
  const entry = codex.items[index];
  codex.setFocus(index);
  if (ev.shiftKey) { codex.selectRange(index); return; }
  if (ev.metaKey || ev.ctrlKey) { codex.toggleSelect(index); return; }
  codex.setAnchor(index);
  if (ev.altKey) { void codex.setDecision([entry.entry_id], "exclude", "click"); return; }
  void codex.cycle(entry.entry_id, "click");
}
```

组件层层只做"转发 + 补 index"，**修饰键的解释集中在 View**，这样 Card 不用知道 store 存在。

**Step 8：乐观更新**。`codex.cycle`（`codex.ts:174-178`）查当前决策、用 `cycleDecision` 算下一态（`lib/decision.ts:4-8`：未定→加入→不加入→未定），然后 `setDecision`（`codex.ts:150-172`）：

```ts
const prev = new Map<string, Decision>();
let nextCounts = { ...counts.value };
const prevCounts = counts.value;
items.value = items.value.map((it) => {
  if (!idSet.has(it.entry_id) || it.decision === decision) return it;
  prev.set(it.entry_id, it.decision);
  nextCounts = shiftCounts(nextCounts, it.decision, decision);
  return { ...it, decision };
});
counts.value = nextCounts;
try { await api.setDecision(ids, decision, via); return true; }
```

**先改本地再发请求**：`items.value` 被替换成一个新数组（没变的元素复用原对象，变了的用 `{...it, decision}` 拷贝一份），卡片瞬间变绿；同时用 `prev` Map 记住每条的旧值。注意这里不是 `it.decision = decision` 原地改——这是 React 风格的不可变更新，Vue 其实允许原地改，但不可变更新让回滚逻辑很简单。

**Step 9：失败回滚**（`codex.ts:167-171`）：

```ts
} catch (e) {
  items.value = items.value.map((it) => (prev.has(it.entry_id) ? { ...it, decision: prev.get(it.entry_id)! } : it));
  counts.value = prevCounts;
  toast.error(e, "写入决策失败，已回滚");
  return false;
}
```

按 `prev` 逐条恢复，计数直接换回快照。`prev.get(...)!` 的 `!` 是非空断言——"我保证这里有值"，因为上面 `has` 已经查过。

### 3c. 同步页：点同步 → Channel → store → 进度条

**Step 1**：`SyncView.vue:58` `<button :disabled="sync.running || sync.starting" @click="sync.start(false)">同步</button>`。

**Step 2**：`sync.start(force)`（`stores/sync.ts:40-72`）：

```ts
const ch = new Channel<SyncEvent>();
ch.onmessage = (e) => {
  progress.value = e;
  if (e.phase === "done" || e.phase === "error") {
    channelActive.value = false;
    if (e.phase === "done") { lastDone.value = e; toast.success(`同步完成：新增 ${e.added} 条，变更 ${e.changed} 条`); }
    else toast.error(e.message, "同步失败");
    void refresh();
  }
};
channelActive.value = true;
try { await api.syncStart(force, ch); await refresh(); }
```

`Channel` 是 Tauri 提供的**后端→前端的单向流**：前端 new 一个、把它当参数传给命令，Rust 侧拿到 `Channel<SyncEvent>` 后可以 `.send()` 任意多次，每次触发这边的 `onmessage`。类比 Java 里传一个 `Consumer<SyncEvent>` 回调进去，或者 Python 的 generator 消费端。`api.syncStart` 的 `await` 返回只表示"后端已接受任务"，真正的完成靠 `phase === "done"` 的事件。

`channelActive` 是"本会话是否还在收事件"。`running`（`:28`）= `channelActive || status.running`：前者来自事件流，后者来自轮询，两者取或。为什么要轮询？`SyncView.vue:14-17` 每 2 秒查一次状态，但**只在 `status.running && !channelActive` 时**——覆盖"同步是上次会话发起的、这次没 Channel"的情况。

**Step 3：store 变化 → 进度条**。`SyncView.vue:23-33` 三个 computed 把两种来源合并：

```ts
const live = computed(() => (sync.channelActive ? sync.progress : sync.status));
const imageProgress = computed(() => {
  const s = live.value;
  if (!s || !s.images_total) return null;
  return s.images_done / s.images_total;
});
```

然后 `:70-74` `<ProgressBar :value="sync.running ? imageProgress : ..." :danger="live?.phase === 'error'" :label="..."/>`。`ProgressBar.vue:11` 把 0–1 变成百分比，`null` 时走 CSS 跑马灯（`:28`）。每来一条事件 → `progress.value = e` → `live` 重算 → `imageProgress` 重算 → 进度条宽度变。

**Step 4：完成横幅**。`lastDone` 非空时 `SyncView.vue:80-91` 显示横幅，链接带 `query: { first_seen: release }` 跳去词典页——这就接上了 3b Step 2 的 `first_seen`。

### 3d. 反推实验室：勾题 → 运行 → 结果表 → 四色 tag 云

**Step 1：勾选题目**。`QuestionBank.vue:81-82`：

```vue
<li v-for="q in rev.questions" :key="q.question_id" :class="{ on: rev.selectedQuestionIds.has(q.question_id) }" @click="rev.toggleQuestion(q.question_id)">
  <input type="checkbox" :checked="rev.selectedQuestionIds.has(q.question_id)" @click.stop="rev.toggleQuestion(q.question_id)" />
```

`toggleQuestion`（`reverse.ts:132-136`）每次都 `new Set(old)` 再增删再赋回——因为 `ref<Set>` 对 Set 内部方法的追踪不如"整个换掉"来得稳妥且易读；全项目对 Set 都是这个套路（`codex.ts:204-206`、`CodexTree.vue:26-28`）。

**Step 2：运行**。`RunConfig.vue:154` `<button :disabled="!rev.canRun" @click="rev.runBatch(note)">`。`canRun`（`reverse.ts:84-85`）是五个条件的 computed。`runBatch`（`:220-248`）和同步页同构：组装 `RunBatchRequest`（`types.ts:213-219`），`new Channel<RunEvent>()`，`onmessage` 里更新 `runProgress`，收到 `batch_done/batch_failed` 时置 `running=false` 并 `loadBatches().then(() => selectBatch(e.batch_id))` 自动切到新批次。进度条在 `RunConfig.vue:157-162`，`progress` computed = `done/total`。

**Step 3：结果表**。`selectBatch`（`reverse.ts:250-263`）调 `reverse_batch_results` 拿 `{ batch, runs, summary }`；`BatchResults.vue` 渲染两张表。批次下拉用可写 computed 桥接 store（`:10-13`）：

```ts
const batchId = computed({
  get: () => rev.currentBatchId ?? "",
  set: (v: string) => { void rev.selectBatch(v || null); },
});
```

`<select v-model="batchId">` 读时走 get，选择时走 set → 调 store 动作。这是本仓库把 `v-model` 接到 store 上的标准写法（`FilterBar.vue:30-41`、`QuestionBank.vue:21-28`、`RunConfig.vue:10-13` 同款）。

**Step 4：点一行 → 单题抽屉**。`BatchResults.vue:56` `@click="rev.openRun(r.run_id)"` → `reverse.ts:265-274` 拉 `RunDetail` 存到 `runDetail`。`RunDetailPanel.vue` 没有 props，直接读 store：

```ts
// src/components/reverse/RunDetailPanel.vue:10-11
const detail = computed(() => rev.runDetail);
const clouds = computed(() => (detail.value ? buildTagClouds(detail.value) : null));
```

**Step 5：四色 tag 云**。`lib/tagcloud.ts:18-35`：

```ts
const hits = new Set(d.hits); const expectedMiss = new Set(d.expected_miss);
const expectedHits = new Set(d.expected_hits); const extras = new Set(d.extras);
const truth: CloudTag[] = d.truth.map((tag) => ({
  tag,
  cls: hits.has(tag) ? "hit" : expectedHits.has(tag) ? "violation" : expectedMiss.has(tag) ? "expected" : "miss",
  contradiction: contradictions.has(tag),
}));
```

后端已经算好了 hits/extras 等集合，前端只是把每个 tag 归到一个 `TagClass`（`"hit" | "miss" | "extra" | "violation" | "expected"`，`:4`）。标准答案侧优先级：命中 > 违规命中 > 不计分 > 漏；模型输出侧：命中 > 违规命中 > 多余。`TagCloud.vue:12-18` 把 `t.cls` 直接当 class 名，颜色在 `:31-36`：绿/灰/橙/红/虚线灰，矛盾词加红色 outline。测试 `tagcloud.test.ts:5-13` 用一组 5+4 个词把每种颜色都覆盖了一遍，读一下就懂规则。

---

## 4. 核心算法逐段讲

### 4.1 `lib/grid.ts`：虚拟网格

设计原则写在第 1 行注释里："无 DOM 依赖，方便单测"。所有函数只吃数字、吐数字，`EntryGrid.vue` 负责从 DOM 读数字喂进去。

**列数**（`:15-24`）：

```ts
const inner = containerWidth - padding * 2;
if (!Number.isFinite(inner) || inner <= 0) return 1;
return Math.max(1, Math.floor((inner + gap) / (cardWidth + gap)));
```

n 列需要 `n*card + (n-1)*gap` 宽，移项得 `n <= (inner + gap) / (card + gap)`。`+gap` 是因为最后一列后面没有间距。测试 `grid.test.ts:20-31` 给了边界：272 恰好 1 列，523→1、524→2。`Number.isFinite` 防 `NaN`（容器还没量到时 `clientWidth` 可能是 0，但 NaN 也防一手）。

**可视行**（`:32-45`）：

```ts
const top = Math.max(0, scrollTop - padding);
const start = Math.max(0, Math.floor(top / rowHeight) - overscan);
const end = Math.min(totalRows, Math.ceil((top + Math.max(0, viewportHeight)) / rowHeight) + overscan);
return { start, end: Math.max(start, end) };
```

`floor` 取视口顶部所在行，`ceil` 取底部所在行（部分露出也算），再各扩 `overscan=2` 行，滚动时不会看到白边。`end` 不含。`Math.max(start, end)` 保证区间不反。

**是否该翻页**（`:48-50`）：`range.end >= totalRows - threshold`，阈值 3 行。

**单元坐标**（`:53-64`）：`row = floor(i / cols)`，`col = i % cols`，再乘尺寸加 padding——就是二维数组下标到像素的映射。

**焦点移动**（`:81-95`）：

```ts
if (index === null || index < 0 || index >= total) return 0;
let next = index;
if (dir === "left") next = index - 1; else if (dir === "right") next = index + 1;
else if (dir === "up") next = index - cols; else if (dir === "down") next = index + cols;
if (dir === "up" && next < 0) return index;
if (dir === "down" && next >= total) {
  return Math.floor(index / cols) === Math.floor((total - 1) / cols) ? index : total - 1;
}
return Math.max(0, Math.min(total - 1, next));
```

左右是 ±1（行尾自然换行，因为 index 是线性的），上下是 ±cols。两条特殊规则：向上越界留在原地（不是跳到 0）；向下越界时，如果当前不在最后一行（最后一行不满），落到最后一项，否则不动。测试 `grid.test.ts:90-105` 用 10 项 4 列画了 `[0..3][4..7][8..9]`，`6 down → 9`、`8 down → 8`。

**滚动定位**（`:110-122`）：目标行的 `top/bottom` 与视口比较，只在不可见时返回新的 `scrollTop`，否则 `null`——`EntryGrid.vue:77-86` 的 `watch(() => props.focus)` 拿到非 null 才赋给 `el.scrollTop`，避免每次按键都抖动。

### 4.2 `lib/artist-recipe.ts`：画师串解析器

输入是 NovelAI 风格的提示词串，例如 `{{artist:foo，bar}}, 0.4::impasto ::，-2::no halo ::`，要拆成带权重的 token。

**归一**（`:53`）：`normalize("NFKC")` 把全角字符折成半角，`，、；;` 统一成 `,`。

**主循环是一个手写的字符级状态机**（`:84-93`）：

```ts
for (const char of source) {
  if (char === "{") { curly += 1; continue; }
  if (char === "}") { curly = Math.max(0, curly - 1); continue; }
  if (char === "[") { square += 1; continue; }
  if (char === "]") { square = Math.max(0, square - 1); continue; }
  if (char === ",") { flush(); continue; }
  if (startingLevel === null && !/\s/.test(char)) startingLevel = curly - square;
  token += char;
}
flush();
```

`curly - square` 是当前嵌套层级：`{` 加强、`[` 减弱。`startingLevel` 记住 token **第一个非空白字符**时的层级——因为 `{{a}}, b` 里逗号出现在 `}}` 之后，flush 时层级已经归零，所以要在进入 token 时就把层级钉住。逗号触发 `flush`。

**`flush` 处理数值权重**（`:61-82`）：

```ts
const numericStart = raw.match(/^(-?\d+(?:\.\d+)?)\s*::\s*([\s\S]*)$/);
if (numericStart) { numericWeight = Number(numericStart[1]); raw = numericStart[2].trim(); }
const closesNumeric = raw.endsWith("::");
if (closesNumeric) raw = raw.slice(0, -2).trim();
if (raw) {
  const weight = numericWeight ?? Math.pow(1.05, startingLevel ?? (curly - square));
  const value = raw.replace(/^\((.*)\)$/s, "$1").trim();
  output.push({ raw, value, weight: roundWeight(weight), kind: weight < 0 ? "negative" : classify(value) });
}
if (closesNumeric) numericWeight = null;
```

`numericWeight` 是跨 token 的状态：`-3::a, b, c::` 里 `a` 开启 -3，`b` 继承，`c` 关闭。测试 `artist-recipe.test.ts:26-42` 就是验这两条。没有数值权重时用 `1.05^层级`（NovelAI 的旧式强调定义）。`flush` 是闭包——它读写外层的 `token/curly/numericWeight`，Java 里你得把这些做成类字段，Python 里得 `nonlocal`。

**分类**（`:38-46`）用一串正则按优先级匹配：`artist:` 前缀 > 年份 > 质量词 > 否定词 > 风格词 > other。

**本项目补充的序列化**（`:114-152`）：`extractArtists` 剥掉 `<artist>` 标记后解析，只留正权重、`kind==="artist"`（或 `bareNames` 时 other 也算，给单画师词典用），`canonicalArtistTagName` 归一名字（小写、空格→下划线、别名表），去重。`formatArtistPoolLine` 输出 `artist:foo, 1.25::artist:bar_baz::, ...` + 固定质量后缀，抽屉里"复制画师串"按钮用它（`EntryDrawer.vue:56`）。

### 4.3 `lib/query.ts`：筛选状态与持久化

三层：**UI 形状 `CodexFilter`**（`:9-19`）→ **localStorage**（JSON）→ **后端 `EntryQuery`**（`types.ts:76-89`）。

**默认值**（`:25-38`）：决策为空（= 全部）、按书序、第 1 页。词典页没有分级筛选——入库的条目都是 safe。

**从 localStorage 读回来要校验**（`:45-62`），因为存的东西可能是旧版本写的、或者被手改坏：

```ts
const out: CodexFilter = { ...defaults, path_prefix: [...defaults.path_prefix], decision: [...defaults.decision] };
if (typeof raw !== "object" || raw === null) return out;
const r = raw as Record<string, unknown>;
if (isStringArray(r.decision)) out.decision = r.decision.filter((x): x is Decision => ALL_DECISIONS.includes(x as Decision));
if (typeof r.keyword === "string") out.keyword = r.keyword;
if (r.sort === "book_order" || r.sort === "title" || r.sort === "decided_at") out.sort = r.sort;
```

逐字段 `typeof` 检查、枚举值过滤，类型不对就沿用默认。第一行显式拷贝数组是为了不和 `defaults` 共享引用（测试 `query.test.ts:49-53` 专门验这个——否则后面 `filter.decision.push` 会污染默认值）。这相当于 Java 里手写一个宽容的 `fromJson`，Python 里的 pydantic `model_validate` 但不抛异常。

`loadFilter/saveFilter`（`:64-80`）把 `JSON.parse/localStorage` 的异常全吞掉——存不了就算了，功能不受影响。key 是 `codexlab.filter.<codex_id>`，每本书一份。

**转后端查询**（`:86-99`）：空值不发送；前端不发 `rating`（后端 `EntryQuery.rating` 仍是可选参数）。**持久化时机**在 store：`codex.ts:83-87` 的 `setFilter` 每次合并 patch → `persist()` → `reload()`，所以任何一个复选框的变化都会立刻落盘。

---

## 5. 语言 / 框架概念清单

每条：它是什么 → 你在 Java/Python 里的对应物 → 本仓库出处。

**单文件组件（SFC）与 `<script setup>`**。一个 `.vue` 文件 = `<script>` + `<template>` + `<style scoped>`，相当于把一个类、它的 FXML/HTML 模板和私有 CSS 放进一个文件。`<script setup lang="ts">` 是语法糖：顶层声明的变量和函数**自动暴露给模板**，不用 `return`。`scoped` 让 CSS 只作用于本组件（编译器给每个元素加 `data-v-xxx` 属性）。看 `EntryCard.vue` 全文，一屏能看完。

**`ref` / `reactive` / `computed` / `watch`（响应式）**。`ref(x)` 把值装进一个盒子，脚本里用 `.value` 读写，模板里自动解包；改 `.value` 会通知所有依赖它的 computed 和模板重新计算。Java 里最接近的是 JavaFX 的 `SimpleObjectProperty` + binding；Python 里像 `traitlets`。`reactive({...})` 是对象版，不用 `.value`（`SettingsView.vue:12-18` 的 `form`）。`computed(fn)` 是带缓存的派生值，依赖不变不重算，且依赖是**自动收集**的（`EntryGrid.vue:32-35` 那条链）。`watch(source, cb)` 是"某个值变了就执行副作用"，`{ immediate: true }` 让它一开始就跑一次（`EntryDrawer.vue:50`）。为什么全项目对 Set/数组都"整个替换"而不是原地 `add/push`？原地改 Vue 也能追踪，但替换让"变了"这件事对 `watch` 更清晰，且回滚容易。

**props / emits**。父 → 子传数据用 props，子 → 父通知用 emit。`defineProps<{ entry: EntryCardT; selected: boolean; focused: boolean }>()`（`EntryCard.vue:8-12`）是类型即声明；`defineEmits<{ imageClick: [ev: MouseEvent] }>()`（`:13-17`）声明事件名和参数元组。类比：props 是构造函数参数（只读），emit 是回调接口/事件监听器。父组件模板里 `@image-click="..."`（kebab-case）监听 `imageClick`。**props 是只读的**——子组件想改就 emit，让父改，数据单向流。

**`v-model`**。表单元素双向绑定的语法糖：`<input v-model="keyword">` = `:value="keyword" @input="keyword = $event.target.value"`。`v-model.number` 自动转数字（`SettingsView.vue:163`）。本仓库的招牌用法是**可写 computed**：`computed({ get, set })` 让 `v-model` 直接对接 store 动作（`FilterBar.vue:30-41`）。

**Pinia store 与 `storeToRefs`**。`defineStore("codex", () => { ...; return {...} })`（`codex.ts:11`）是"setup 风格"：函数体里用 `ref/computed` 声明状态，返回的对象就是 store 的公开 API。`useCodex()` 在任何组件里拿到**同一个单例**（首次调用时创建）——像 Spring 的单例 bean 或 Python 的模块级全局。store 可以互相 `use`（`codex.ts:12-13` 用了 toast 和 settings）。**`storeToRefs` 本仓库没有用**：它解决的是"解构 store 会丢响应式"的问题（`const { items } = codex` 拿到的是死值），而这里的组件全部写 `codex.items`、`rev.questions`，永远通过 store 对象访问，所以不需要。以后你若想写 `const { items, total } = storeToRefs(codex)`，再引入它。

**vue-router 与懒加载**。`router.ts` 的 `routes` 数组把 URL 映射到组件；`useRoute()` 读当前路由（参数、query），`useRouter()` 做跳转（`CodexView.vue:14-15`）。`component: () => import(...)` 是动态 import，返回 Promise，打包器据此切 chunk——Java 里类似按需加载的 `ServiceLoader`，Python 里像函数内 `import`。`<RouterLink>` 是会加 `router-link-active` class 的 `<a>`（`styles.css` 里 `.nav.router-link-active`）。

**递归组件**。`CodexTree.vue:45-53` 在自己的模板里用 `<CodexTree>`——SFC 可以按文件名引用自己，不用 import。每层是独立实例，所以 `collapsed`（`:13`）是每层各一份。

**属性透传（fallthrough attrs）**。`EntryGrid.vue:113` 给 `<EntryCard :style="...">` 传了 `style`，但 `EntryCard` 没声明 `style` prop——未声明的属性会自动合并到子组件根元素上。所以 `position:absolute; top; left` 和 `EntryCard.vue:31` 自己的 `width/height` 会合并。

**`defineExpose` 与模板引用**。子组件默认对父封闭；`EntryGrid.vue:101` `defineExpose({ getColumns, focusContainer })` 开放两个方法，父用 `ref="grid"` + `grid.value?.getColumns()`（`CodexView.vue:17, 72`）调用。类似 Java 里只暴露接口方法。

**`<Transition>`**。`EntryDrawer.vue:132` 包一层，元素进出时自动加 `drawer-enter-from` 等 class，CSS 在 `:266-269` 定义动画。

**TypeScript：interface / 联合类型 / 泛型**。`interface EntryCard {...}`（`types.ts:90-107`）像 Java 的 record/POJO，但纯编译期。`type Decision = "include" | "exclude" | "undecided"`（`:3`）是**字面量联合**：变量只能是这三个字符串之一，比 Java enum 更轻，`switch` 漏 case 会被 `noFallthroughCasesInSwitch` 和类型收窄抓到。`Record<Decision, string>`（`decision.ts:20`）= `Map<Decision,String>` 但键必须齐全。`Partial<Settings>`（`types.ts:25`）= 所有字段可选，用作 patch。`invoke<T.AppInfo>("app_info")` 的 `<T>` 是泛型实参，告诉编译器返回类型；**运行时没有校验**。`(x): x is Rating => ...`（`query.ts:50`）是类型守卫，返回 true 时编译器把 `x` 收窄为 `Rating`。`e: unknown` 的 catch 变量必须先判类型再用（`api.ts:7-13`）。`debounce<A extends unknown[]>`（`debounce.ts:1`）是泛型函数，`A` 捕获参数元组类型，返回的函数签名和原函数一致。

**async / await 与 Promise**。`async function` 返回 `Promise<T>`，`await` 让出执行权直到 resolve——语义和 Python 的 `asyncio` 完全一样，和 Java 的 `CompletableFuture.thenApply` 链等价但写起来像同步代码。`Promise.all([...])`（`codex.ts:67`、`sync.ts:32`）= `asyncio.gather`。`void someAsync()`（`codex.ts:87`）显式表示"我知道这是 Promise 但不等它"，是给 lint 看的。`settings.ts:30-34` 的 `ensureLoaded` 用一个模块级 `loadPromise` 把并发调用合并成一次请求，是常用的去重模式。JS 是单线程事件循环，`seq` 这种计数器不需要加锁。

**模块与 import**。每个文件是一个模块，`export` 的才可见；`import type` 只引类型、打包时被擦掉（`api.ts:3`）。相对路径 `../lib/grid` 不带扩展名。`export { Channel, convertFileSrc }`（`api.ts:5`）是再导出，让组件只依赖 `api.ts` 而不直接依赖 Tauri 包。

**vitest 怎么写**。`describe/it/expect`（`grid.test.ts:19-31`）和 JUnit 的 `@Nested/@Test/assertEquals`、pytest 的函数+`assert` 一一对应。`toEqual` 深比较，`toBe` 是 `===`，`toMatchObject` 部分匹配（`artist-recipe.test.ts:22`）。`vite.config.ts:22-25` 指定 `environment: "node"`、只收 `src/**/*.test.ts`。测试文件和被测文件同目录同名加 `.test`。目前只测 `lib/`，因为它们不碰 DOM。

**闭包**。`debounce.ts:1-15` 的 `timer` 被返回的函数捕获；`artist-recipe.ts:61` 的 `flush` 读写外层变量。Python 的 `nonlocal`、Java 的匿名内部类捕获（但 JS 可以改被捕获的变量）。

**展开与解构**。`{ ...filter.value, ...patch }`（`codex.ts:84`）= 浅拷贝后覆盖，Python 的 `{**a, **b}`。`const [s, l] = await Promise.all(...)`（`sync.ts:32`）= 元组解包。`?.` 可选链、`??` 空值合并（`App.vue:29`）分别对应 Kotlin 的 `?.`/`?:`、Python 的 `x if x is not None else y`。

---

## 6. "想改 X 去哪改"速查表

| 想改 | 去哪 |
|---|---|
| 卡片尺寸、间距、预渲染行数 | `lib/grid.ts:3-9` 常量；`EntryCard.vue:69-78` 的 `.thumb` 高度要跟着 `CARD_HEIGHT` 一起改 |
| 三态颜色/蒙层 | `EntryCard.vue:65-84`；颜色变量在 `styles.css:2-12` |
| 键盘快捷键（方向/HJKL/A/X/U/空格/Esc） | 键→动作映射在 `lib/grid.ts:66-74`、`lib/decision.ts:11-18`；分发在 `CodexView.vue:69-106` |
| 鼠标修饰键含义（⇧/⌘/⌥/右键） | `CodexView.vue:44-67` |
| 三态循环顺序 | `lib/decision.ts:4-8` |
| 每页条数、翻页阈值 | `lib/query.ts:7`（200）、`lib/grid.ts:48`（离底 3 行） |
| 新增一个筛选条件 | 四处：`types.ts:76-89` 的 `EntryQuery`、`lib/query.ts` 的 `CodexFilter`+`defaultFilter`+`normalizeFilter`+`buildEntryQuery`、`FilterBar.vue` 加控件、Rust 侧 |
| 筛选默认值 / localStorage key | `lib/query.ts:21-38` |
| 关键词防抖时长 | `FilterBar.vue:14`（200ms）、`QuestionBank.vue:18`（300ms） |
| 新增一个后端命令 | `types.ts` 加 DTO → `api.ts:15-63` 加一行 → store 里加 action → 组件调 |
| 错误提示的文案格式 | `api.ts:10-13` `errMessage`；toast 时长 `toast.ts:16-32` |
| 侧栏导航项 / 默认打开哪本书 | `App.vue:23-27`；`CodexView.vue:18` `DEFAULT_ID`；`router.ts:6` 首页重定向 |
| 同步页轮询间隔 | `SyncView.vue:15-17`（2s） |
| 抽屉里的区块（加/减一节） | `EntryDrawer.vue:146-251`，每节一个 `<section class="sec">` |
| 画师串输出格式 / 质量后缀 | `lib/artist-recipe.ts:135-152`；别名表 `:20-23` |
| tag 云颜色与分类优先级 | 分类 `lib/tagcloud.ts:24-33`；颜色 `TagCloud.vue:31-36`；图例 `RunDetailPanel.vue:65-67` |
| 题库默认筛选（默认哪本书、几题） | `stores/reverse.ts:51-59` |
| 模板/规则集下拉排序规则 | `stores/reverse.ts:37-39` `sortVersioned` |
| 暗色模式配色 | `styles.css:18-23` |
| 加一个页面 | `views/` 新建 → `router.ts` 加路由 → `App.vue` 加 `RouterLink` |
| 加一个纯函数并测试 | `lib/foo.ts` + `lib/foo.test.ts`，`npm test` 自动收 |

---

## 7. 读代码时发现的坑与可改进点

按我认为的重要性排序，都是读源码时看到的，不是猜的。

1. **快速切书可能停在旧书上**。`CodexView.vue:20-29` 的 `enter()` 用 `entering` 标志防重入，但重入时是**直接丢弃**（`if (entering) return;`），不是排队。场景：从"画师词典"点到"精选图包"，在 `codex.open` 还没返回时又点回"画师词典"——第二次 `enter` 被丢，URL 是 A 但页面显示 B。修法：记录"最新请求的 id"，`enterInner` 完成后若发现 `props.codexId` 已变则再跑一次。

2. **左侧书目计数不会刷新**。`codex.ts:67` 只在 `books` 为空时 `loadBooks()`，之后决策再多次、同步再多次，`codex.book.counts`（`CodexView.vue:131` 显示的"加入/不加入/未定"）和树上的数字都停在第一次的值，直到重启应用。筛选栏的计数用的是 `page.counts`（每次查询带回），所以两处数字会对不上。建议在 `setDecision` 成功后或 `sync` 完成后重拉书目，或者让树头也用 `counts`。

3. **`tagcloud.ts:31` 有一处死代码**：`extras.has(tag) ? "extra" : "extra"`——两个分支一样。逻辑上没错（模型输出里不命中、不违规的就是多余），但读起来像漏写了一个分支。直接改成 `: "extra"` 更诚实。

4. **Channel 的终止事件一旦丢失，按钮就永远灰**。`sync.ts:62` 把 `channelActive` 置 true，只有收到 `done/error` 才置 false；`reverse.ts:239` 的 `running` 同理。如果后端 panic、或取消路径没发终止事件，前端"同步"/"运行 batch"按钮就一直 disabled，轮询兜底也不会救（`SyncView.vue:16` 的条件要求 `!channelActive`）。建议：`refresh()` 拿到 `status.running === false` 时顺手把 `channelActive` 清掉。

5. **`first_seen_release` 会"粘"在 localStorage 里**。从横幅点进来时它被写进筛选并持久化（`codex.ts:76-79`）。下次同步出了新 release，`latestRelease` 变了，但存的筛选还是旧 release，"本次新增"复选框显示勾选（`FilterBar.vue:39` 只看非空），实际筛的是上一次的。可以在 `open` 时比较 `filter.first_seen_release !== latestRelease` 就清掉。

6. **`tsconfig.json:17-18` 声明了 `@/*` 路径别名，但 `vite.config.ts` 没有对应的 `resolve.alias`**，全仓库也没人用 `@/`。你若照着 tsconfig 写 `import x from "@/lib/grid"`，类型检查会过、Vite 运行时会报找不到模块。要么删掉 tsconfig 那两行，要么在 vite 里补 alias。

7. **乐观更新的计数回滚用的是整体快照**（`codex.ts:155, 168`）。两次 `setDecision` 并发且第一次失败时，`counts` 会被恢复成第二次之前的值，把第二次的增减也抹掉。条目本身是按 id 回滚的所以没事。低概率，但如果以后加"批量 + 键盘连按"会碰到。用 `shiftCounts` 反向减回去比恢复快照更稳。

8. **`app_info` 被调了两次**：`App.vue:12` 和每次 `codex.open` 里的 `loadAppInfo()`（`codex.ts:53-58, 67`）。后者只为拿 `latest_release`。不算 bug，但可以让 App 把 info 放进一个 store 共享。

9. **测试只覆盖 `lib/`**。stores 里最有逻辑的 `setDecision` 回滚、`reload` 的 `seq` 竞态、`ensureLoaded` 的去重都没测。这些其实不依赖 DOM，可以用 `createPinia()` + `setActivePinia()` 直接在 node 环境测，只需 mock `../api`（vitest 的 `vi.mock`）。这是性价比最高的下一步。

10. **键盘快捷键依赖网格持有焦点**（`EntryGrid.vue:105` 的 `tabindex="0"`）。点了底部"全部加入"按钮后焦点在按钮上，再按 A/X 无反应，得点回网格。`onDrawerClose` 已经处理了抽屉关闭后回焦（`CodexView.vue:117-121`），批量栏的按钮可以照抄。

11. **`EntryGrid.vue:88-93` 的"回到顶部"是启发式**：靠"第一项 id 变了或数量变少"猜测是不是换了筛选。翻页追加不会误判，但"筛选变化后第一条恰好相同且结果更多"时不会回顶。更明确的做法是让 store 暴露一个 `reloadSeq` 之类的信号。

12. **`EntryDrawer.vue:74-79` 改决策后 `await load()` 整条重拉**，只为刷新审计历史。网格那边已经乐观更新过了，这里多一次往返；可以只在本地 push 一条 audit 行。小事。

13. **`FilterBar.vue:17` 卸载时 `pushKeyword.cancel()`**：在关键词输入后 200ms 内切走页面，最后几个字符不会落盘。符合"取消防抖"的常规做法，但知道一下。

14. **不可变更新的代价**：`setDecision` 每次 `items.value.map(...)` 生成一个新数组，一页 200 条无所谓；如果以后把 `PAGE_SIZE` 调到几千并连续翻页，`items` 变大后每次点击都是 O(n)。真到那一步再改成原地赋值（Vue 支持）。

总体印象：这一半代码的分层很干净——`lib/` 纯函数、`stores/` 状态与后端调用、组件只管渲染和转发事件——这也是为什么它适合拿来学 Vue。上面的坑里 1、2、4 是会被用户感知的，其余是代码整洁度问题。
