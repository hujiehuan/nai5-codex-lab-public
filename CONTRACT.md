# 前后端契约 v0.1（权威文件，前后端都以此为准）

## 27. 总实验室显式纯文字参考（2026-09-21，用户授权）

- 用户在保留图文能力的同时要求增加「全部文一起传」「抽样文参考」。本节覆盖 §24 中新版 draw 必须传图的要求：界面提供全部图文、抽样图文、全部文、抽样文四种选择。`reference_mode:all|sample` 仍只决定条目范围；新增 `reference_content:image_text|text` 决定是否发送图像。旧工作区缺省为 image_text，保留已有 all/sample 选择，不擅自改默认或自动降级。reverse 的原图与反推流程不受此选项影响。
- text 模式保留所选图包全部主、负面、角色提示词及来源元数据，明确不读取、校验、冻结、编码、上传或发送参考图片；缺少本地图像不影响纯文字构思。all 仍无应用层参考总字节预算，sample 沿用原数量、选取和预算；筛选范围、分级、黑名单、角色、skill、生成与费用规则不变。纯文字减少图像输入，不承诺总 token 必然很小或消除模型上下文/超时限制。
- capabilities 新增 `reference_text_supported:true`；用户显式选择 text 时两端须校验此能力，不支持则提示更新插件。图文仍以 `reference_images_supported` 门禁并校验完整图片清单；对仅支持旧图文协议的插件可以省略新增字段，不能把图文请求改成 text。显式未知内容模式、text 携带 reference_images、image_text 缺失/空/损坏图片均须拒绝。插件旧请求未带 reference_content 时保持原兼容：有 reference_images 按图文处理，无该字段按旧纯文处理；不能把这些历史记录冒充新图文。
- `studio_prepare` 返回并冻结 reference_content；text 的 reference_image_count 为 0，不产生图片快照。`studio_plan` 使用冻结选择，不以表单当前值覆盖；text 请求发送 reference_content=text，不发送 reference_images。插件响应 settings 明确回显 reference_content=text、reference_image_count=0、reference_images=[]；桌面严格核对。图文回显的 mode 与图片顺序/ID/哈希仍须一致，旧图文插件缺 content 回显只可按完整图片清单判定。既有方案、历史、同请求重试保持原材料；旧缺图片快照的 input 不因新增模式而自动复活。
- 插件冻结的实验输入摘要包含显式 reference_content，防同 ID 换材料模式；未带字段的旧请求仍沿用旧摘要。Mac 私有输入同时冻结是否在 wire 中声明该字段，旧图文 input 不因插件升级而补加字段改变摘要；原来显式协商的图文 input 遇到插件降级应报错，不能省略字段冒充同一输入。text 走完整文字保护而非图片包装；不裁剪或自动切换模型。UI 根据模式展示实际文字与图片数、抽样条数及说明，不要求纯文字模型有视觉能力。所有异步错误保持明确，未知数量不记作零。
- 不涉及 SQLite schema、用户筛选数据、插件配置或自动重载。回归使用合成数据验证四种方式、无图纯文字、缺图图文拒绝、能力不支持、冻结重试与旧历史；1280/390px 深浅模式量化版面。真实模型与反推对照实验独立执行，须先收到用户提供的原图和选定模型；不得把 mock 图片交付为实际生成。

## 25. 总实验室跟随插件所选 skill（2026-09-21，用户授权）

- 用户要求插件更新并选中新 skill 后，Mac 默认资料随之更新。本节覆盖 §16 默认使用内置固定版的约定；下载但未选中的版本不影响当前选择。新增稳定选择项 `plugin-active`：draw 跟随插件 `skills.draw_skill_id`，reverse 跟随 `skills.reverse_skill_id`；反推未启用资料时明确提供零份 skill，绝不补入内置版本。内置固定版、本地导入、固定网络快照以及无 skill 对照仍可显式选择。
- 新增 GET `page/lab/studio/skills?features=studio_v1&kind=draw|reverse`，沿用 Studio 的 AstrBot JWT 与 X-Library-Token 鉴权、status/data 封装。data 为 `{version:1,kind,enabled,selected_id,skill}`；skill 为完整 `{id,name,version,sha256,text,source,builtin,bytes}`。draw 默认返回有效 builtin；reverse 关闭为 enabled=false/selected_id=""/skill=null。指定版本缺失、损坏、哈希/字节数不符必须报错，不静默回旧版或冒充关闭；资料仅读取，不改插件选择或访问 GitHub。
- Mac IPC `studio_skills_sync(kind)` 校验后返回相同形状，其中 `skill.id` 改为本机不可变快照 ID，`selected_id` 保留插件 ID。正文、版本、来源和 SHA256 存本机私有 `studio/plugin_skills/`，普通资料列表不堆积历代快照、普通删除不影响它们。读取失败显式报错，不用旧缓存伪报同步成功；保留既有 skill 单份/组合容量与完整性校验，不涉及法典参考总量。
- 新工作区默认资料选择 `plugin-active`；旧工作区仅首次把未改名的 default/默认资料、且仅包含 nai5-prompting 的默认组迁移为跟随，记 `skill_follow_version:1`。自定义组合、显式空组、已有行/方案/历史均保留；用户之后显式选择固定版不会被重新迁移。未解析的 plugin-active 不得直接进入构思后端。
- 进入页面、切换实验模式、窗口重新聚焦和显式同步可读取当前选中版本；新一轮含跟随项的对照在准备输入前必须再同步一次，将该轮所有组合解析为同一份本机快照 ID。构思、失败重试及已冻结方案再出图不重新取插件当前版本；新一轮才跟随新选择。界面显示实际名称、版本/哈希与关闭/加载/失败状态，不把“同步成功”当作模型调用证据。重复跟随项不得重复发送材料。
- 无 SQLite schema/筛选决策变更，不改模型、图片/文本参考、生成尺寸/费用、插件指令运行逻辑或真实配置，不自动重载机器人。用合成资料覆盖版本切换、关闭/缺失/篡改、老插件、跨模式并发、同轮一致与旧实验重试；真实 Mac 原生运行另列验收。

## 24. 总实验室全部图文参考（2026-09-21，用户选择 A）

- 新版总实验室 `/画` 的每条构思参考改为「主参考图 + 完整图包文本」，覆盖 §15.3 在本入口“不附带图片”的旧约定。全量模式同时提供当前范围内每个合格图包，不先让模型挑图、不分批归纳或拼接缩略图、不静默退回文字或抽样。抽样入口仍保留，明确只发送其选中条目的图文；新工作区默认全量，已有用户选择保留。用户要求、角色、skill、筛选/分级、画风与免费出图参数不变；本次不改插件指令默认流程或旧 creative 协议。
- 每个图包固定取 `entry_image.ordinal=0` 的主参考图：优先已下载的展示图 `local_path`，缺文件才使用同一主图的 `original_local_path`，绝不拿 `thumb_path` 或另一条图包代替。附带该图 `raw_tag`（允许 null），以及图包全部主/负面/角色提示词；通过 entry_id 和图片顺序关联。缺图、损坏或不支持的图片必须定位条目并在调用模型前报错，提示补齐图片；不从参考中剔除缺图条目，不自动联网下载。图片保持完整像素和字节，不为增加条数缩放/重编码；沿用单图 12 MiB、32M 像素及 PNG/JPEG/WebP 格式校验，不新增图文总量或图片张数预算。
- `studio_prepare` 将全部图文和图片 SHA256 冻结；图片副本放本机应用私有 `studio/reference_images/<input_id>/`，工作区不保存 Base64。返回新增 `reference_image_count`；原图/图库后来改变不得影响同一轮模型/skill 对照或重试。旧方案仍可查看和出图；旧纯文本 input 不能冒充新图文输入，重新构思须重建输入。
- Studio capabilities 新增 `reference_images_supported:true`，旧响应缺省不支持；新版客户端在 draw 新任务前检查，明确要求更新重载插件，不纯文本降级。新增同鉴权、同 `features=studio_v1` 的 POST `page/lab/studio/reference_images_status`：`{images:[{sha256,mime}]}` → `{missing:[sha256]}`；POST `reference_image_upload`：`{sha256,mime,data_base64}` → `{sha256,mime,bytes}`，逐张校验真实格式/像素/哈希后幂等缓存到插件私有 `lab/studio/reference_images/`。哈希不得作为任意路径，坏缓存不得视作命中。逐图传输避开 AstrBot 单次 128 MiB 上传限制，不改宿主全局配置；缓存没有材料内容日志，不进入 Git。
- POST plan 新增 `reference_images:[{entry_id,ordinal:0,mime,sha256,raw_tag:string|null}]`，按 references 相同顺序一条对一张、无遗漏/重复/越界，插件从已校验缓存取得真实图片。新字段缺失仅保留旧客户端的纯文本兼容，显式空/损坏不能降级。reverse 入口不能借此添加无关材料。返回 `settings.reference_image_count` 与 `settings.reference_images` 同一清单，桌面严格比对后保存，历史显示实际提供的文字和图片数量；图片数量不等于模型实际认真阅读或训练的证据。
- 构思调用必须同时包含所有完整参考文字、顺序/ID/每图 raw_tag 标识和所有真实图片，支持看图的模型才能处理。独立 provider 副本阻止上下文剪裁和删除图片后重试，检查适配器是否丢图，保留正确 MIME，不修改共享 provider/AstrBot 核心；不兼容适配器、模型上下文/图数/请求容量拒绝与超时明确报错，不换模型、不自动拆批或少喂。图文均为不可信参考材料，不能覆盖用户要求或程序输出协议。模型 API 费用独立提示，上传/冻结不调用模型。
- 仅以合成图片、假模型、fixture 数据库验证：1367 条图文一一送达、顺序/哈希/MIME、缺图/缓存坏/旧插件/无视觉/适配器删图拒绝、冻结后文件改变仍用原快照、重新出图零构思调用。无 SQLite schema/筛选决策变更，不提交或复制真实图包、用户图像/密钥，不自动重启线上机器人。

## 23. 全量参考的单图包材料完整性（2026-09-21，用户授权）

- 修正 §21 仅移除总预算后遗留的单图包限制：`reference_mode=all` 不再限制参考图包的角色段数，也不再限制参考主提示词、负面词、角色提示词及角色负面词的字符长度。参考材料不是最终生成请求，不应以单张生成的角色数量约束它；不换更大常量、不截断或丢弃整条图包。
- 仍验证 JSON 数组/对象/字符串类型、来源 ID、元数据、重复条目、筛选范围、分级和黑名单。可选负面词缺失或 null 仍按空串处理，其他错误类型须报错；角色字段错误指出来源 entry_id、从 1 起的角色段序号和字段，不输出参考正文。
- Studio 与旧 creative 的全量构思、冻结快照及执行前复核共用此规则；抽样模式既有数量/长度预算、来源挑选策略、模型最终输出格式及免费生成条件不变。实际模型上下文或服务容量拒绝仍如实显示，不自动删材料或换模型重试。
- 本次修改插件参考校验，Mac 请求 DTO 不变，无需为此更新桌面二进制；不迁移数据库或改写筛选数据。用合成多角色/长正文和 1367 条参考验证原文送达假模型、冻结及恢复，不调用真实计费模型。

## 22. Mac 内置源码更新（2026-09-21，用户授权）

- 设置页新增「软件更新」，侧栏版本可直达。当前仓库无预构建/签名更新源，本轮采用本机既有 Git 与 Node/Rust/Xcode 工具链；首次选本仓库源码目录，之后在应用内检查、独立构建、安装重启。不要求输入新 token 或复制钥匙串密钥。首次仍需手动安装带此入口的版本。
- 只检查指定仓库 `hujiehuan/nai5-codex-lab-public` 的稳定 `vX.Y.Z` 标签；冻结版本、tag 与解析后的 commit，构建前复核 tag 未改变。检出到应用私有更新目录的 detached worktree，运行锁文件安装与 Tauri app 构建；不切换/清理用户原工作树、不丢未提交修改，不自动安装系统依赖。Git 鉴权失败、缺工具、构建失败、取消和读取失败独立显示，不伪报最新版；不静默使用其他仓库或来源。
- 进度以实际阶段与日志摘要显示，不伪造下载/编译百分比；检查只访问 Git，不编译；准备更新只有明确点击才执行；安装只有「安装并重启」明确触发。离开设置页不丢进行中状态；同时只允许一个更新操作，取消终止自身进程树。启动时把未完成任务标记中断，不自动继续执行命令或重启。
- 原生 DTO `AppUpdateStatus`：`supported:boolean, phase:idle|checking|up_to_date|available|preparing|ready|installing|error|cancelled, current_version:string, current_revision:string|null, source_dir:string|null, candidate:{version:string,revision:string,tag:string}|null, stage:string|null, message:string|null, log_tail:string[], started_at:string|null, can_install:boolean`。IPC：`app_update_status()`、`app_update_choose_source()`、`app_update_check()`、`app_update_prepare()`、`app_update_cancel()`、`app_update_install()`；长检查/构建后台运行并通过 status 轮询，取消选目录保持原选择。
- 安装前阻止新工作并确认当前本地任务/请求结束，刷新并保存工作区状态，使用既有 SQLite 一致性备份完成升级备份；失败则保留旧软件继续使用。校验生成 `.app` 的 bundle ID、版本、可执行文件与当前架构；开发窗口、DMG/只读/不可写安装位置不能伪称已自动安装，应明确给出原因。
- 安装助手等待当前进程退出，在同目录准备新 `.app`、保留旧 `.app` 后切换，复制/切换/打开失败尽力恢复旧应用并持久化结果供下次读取；不自动回滚数据库覆盖新增决策。仅替换应用本体，identifier `moe.vivian.codexlab`、数据目录、钥匙串 service 与进程读缓存保持。日志不记录凭据、完整远程认证 URL 或用户法典内容。更新脚本的参数以 argv 传入，不拼接用户路径执行 shell。
- 不修改插件、出图/筛选语义或 SQLite schema。测试使用临时 Git 仓库、假工具链/安装目录和浏览器 mock，验证失败/取消/并发/重启恢复及数据未被替换；Linux 可验证状态机和文件安装逻辑，Finder 权限与 Mac 真机应用替换必须明确为实机验收项。

## 21. 全量参考取消应用字节预算（2026-09-19，用户授权）

- 用户明确要求全量参考不因材料大小被实验室挡下。本节覆盖 §15.3、§16 的旧 2 MiB 参考 / 4 MiB 整体构思预算：Mac 总实验室、旧创作入口与插件共用创作链的 `reference_mode=all` 不设应用层参考总字节数或整体文本请求字节预算，也不换一个更大的常量继续拦截。
- 当前选定范围内的有效条目完整送给提示词模型，主/负面/角色提示词、来源与冻结快照保持一致。构思、编辑、保存/恢复、再次出图的复核不得重新施加旧预算；不得静默截断、减少条数、改成抽样或改写筛选范围。移除本地私有实验快照及插件已接收 `full_pool.json` 的固定读取大小限制，避免已保存/成功导入的材料无法恢复或复核；全库仍按既有结构、筛选和分级验证，宿主上传限制保持。
- 只取消全量文本的总字节预算：抽样的既有数量/预算、字段类型与内容格式、筛选/分级/黑名单、skill 与图片校验、工作区限制、生成尺寸与免费条件均不变；不改用户配置、SQLite schema 或决策数据。插件指令共用的全量路径一起适配，默认开关与参考范围不变。
- 全量构思仍可能增加模型 token、费用与等待时间。模型上下文、模型服务/HTTP 基础设施的真实容量限制和超时应明确报错，不能承诺无限上下文或自动降级；AstrBot 全局上传限制不在本次修改范围。验证用超过旧 2/4 MiB 阈值的合成正文贯穿构思、编辑和快照恢复，核对全文/条数不丢，不调用真实计费模型。

## 20. 总实验室反推原图拖放（2026-09-19，用户授权）

- 「反推实验 → 共同输入」支持将一张本地图片拖入原图区域，同时保留点击选图。原生拖放订阅只在该组件存在时启用，区域外不接收；离开页面退订，迟到的导入结果不得写入其他模式。
- 复用 `reverse_import_local_image` 拷贝到应用数据目录，不修改原文件；点击选图复用已经完成拷贝的 `reverse_pick_local_image` 返回值，不重复导入。支持扩展名与原选图入口相同，导入后先验证图片可预览才替换旧原图；多文件、非图片或读取失败明确提示并保留旧原图。
- 导入期间禁止构建方案与启动新实验，已有实验执行期间禁止替换原图；拖放只准备本地输入，不调用模型。冻结输入、生成参数、SQLite schema、历史数据与鉴权不变。
- 当前锁定 Wry 0.55.1 的 macOS 拖放位置来自 AppKit 逻辑坐标；命中测试不重复除 Retina 比例或 CSS 缩放。其他平台按 SDK 物理坐标转换。Mac 原生 Finder 拖放仍需实机验收，浏览器测试模拟其原生事件协议。

## 19. 反推识图只保留候补链（2026-09-19，用户授权）

- 插件删除旧的单选识图模型键 `reverse_draw.provider_id`，运行时不再读取、不迁移真实配置。识图只按候补链 `reverse_draw.provider_ids` 顺序尝试，拒答/错误才换下一项、成功即停止；候补链为空时保留原有剩余回退：`auto_gallery.reverse_provider_id` → `auto_gallery.prompt_provider_id` → `llm.advanced_arg_generation_provider`。
- Mac Studio 的 `defaults.reverse_provider_id` 是现存响应字段名，仍保留，值取候补链首项或上述回退。桌面二进制无需升级；本次不改 SQLite、数据导出和历史实验。

## 17. 插件与桌面共用常用 LLM 名单（2026-09-19，用户授权）

- 插件总览管理 `model_selection: {enabled: false, provider_ids: []}`。默认未启用时可选全部 AstrBot LLM；启用时只展示所选且仍注册的模型，按用户保存顺序排列。启用时不可保存空名单；已选模型后来全部移除则有效名单为空，明确提示，不回退全部。
- `GET/POST page/model-selection` 沿用管理页 WebUI JWT 鉴权；POST 为 `{enabled:boolean,selected:string[]}`，GET/POST 的 data 为 `{enabled,selected,available,effective,missing}`。available 是注册全量，selected 是保存名单，effective 是实际选项，missing 是已移除的所选 ID。严格检查形状、重复和新增未知 ID；允许保留或删除旧 missing。保存失败回滚内存，不伪报成功；读取失败不返回空成功。
- 桌面端只读这份名单：无参命令 `plugin_model_selection` 只 GET `page/model-selection`，原样返回插件 `data`（前端 `api.pluginModelSelection()`）；名单在插件管理页编辑，桌面不 POST。
- `page/providers` 保持 string[] 结构；插件内各处候选列表与 HTTP creative/studio capabilities 的候选列表同步过滤。插件与 Mac 的识图、反推、构思、评分、画风复审等 LLM 选择框只接受这份名单，不手输或把旧值自动补成选项。切回页面/窗口及手动刷新重新读取，保存名单后共享列表立即刷新。CLIP/CSD/MegaStyle 等本地编码器不属于 LLM 名单。
- 这是模型**选择范围**，不改 AstrBot 其他插件的模型，不自动改写插件已保存默认值、候补顺序或已冻结方案。名单外旧值单独显示提示，可保持或移除；更换/新增模型及新实验显式模型参数必须在有效名单内。无关配置保存不要求先清掉旧值；插件已有默认配置与在途/冻结任务继续按原模型运行，注册模型本身被删除仍报原有不可用错误。
- 不迁移 SQLite、不改画质、提示词、抽签、点数及计费规则。验证仅使用合成模型清单和本地 mock，不实际调用模型或改线上配置。

## 16. v0.5.5 总实验室（2026-09-19，用户授权）

本节只替代桌面的新实验入口；导航合并为「总实验室」，包含反推、/画、画风复审三种模式。旧路径重定向到对应模式，旧批次和图片继续可读，不删库、不改 schema 7、不覆盖插件默认配置。实验参数逐请求冻结，禁止临时修改全局配置或共享 provider。旧 §9/§12/§15 协议保持兼容；新协议独立使用 `page/lab/studio/*`，统一 JWT + X-Library-Token，版本 `1`。

- **对照单位**：一个试验 = 一个提示词模型 × 一个有序 skill 组合 × 相同输入快照。多模型不走候补链，失败单独显示。组合可为空（无 skill 对照），可选单个或多个；记录版本、全文 SHA256、实际发送文本。技能材料仅作为模型参考，不执行其中命令，也不能覆盖用户硬要求、输出协议、免费约束。内置 `nai5-prompting`（本仓库原创的简短写法说明 `resources/prompting-guide/GUIDE.md`，MIT，版本 `builtin-1`）随应用固定，RefirmSaky/novelai-prompt-design-enhancer 从明确固定 revision 导入，外部导入须预览内容及大小，绝不静默追踪 main。导入、历史写应用私有 JSON 文件，不碰决策库。
- **创作参考范围**：/画只使用 safe 图包。决策范围可选 include 或 all（include/exclude/undecided），这是用户明确授权的实验范围扩展，不改变「加入」语义或线上池。未移除、未拉黑仍必需；画师词典不作为构图图包。全量模式发送当前范围所有合格图包完整正文；按 §21 不设应用字节预算，不够上下文报错，禁止暗中抽样。抽样保留明确数量/来源记录。冻结方案出图前复查所选范围资格；all 模式不能误以 include 规则否决 exclude/undecided。
- **创作与反推**：draw 的 count=1..8 个不同构思；同一硬要求和角色贯穿各图，重复方案或数量错误须报错。角色留空从插件有效角色池选择并冻结，多角色为显式有序列表。创作简报/多人简报、评分模型均可实验覆盖。reverse 提供普通与分别反推模板、原图、评分模型列表、画风复审模型和候选张数；沿用现有识图/清洗/画风检索算法，实际参数与插件 `/反推` 指令的差异逐项展示。反推 count=同一反推方案的候选图，不伪称多个独立构思。独立画风复审实验与反推中的候选重排是两种用途。
- **生成与费用**：新总实验室全部使用 §15 的三档免费尺寸、V5、最多 28 steps、逐张订阅/点数/余额检查，不进入加强或付费降级路径。这是新反推实验相对旧 §12.6 可配置尺寸的明确差异，旧历史与旧重画口径保留。构思、识图、评分、视觉复审可能产生额外模型 API 费用；本地向量推理不调用模型 API。费用未知保持 null。构思与出图分开操作，start 只使用冻结方案、具有 request_id 幂等性；网络不明时重试相同 request_id，不重复构思/出图。停止只阻止未提交工作，不承诺撤回远端在途请求。
- **JPEG**：可选择原图 PNG 或 JPEG 衍生副本，JPEG 默认质量 88、4:4:4，质量可调；保留原始 PNG，显示实际两份字节数。它是有损存储/导出选项，不改变尺寸/提示词/模型，不宣称绝对最佳画质或一定更小。不要把“是否转 JPEG”伪装为 NAI 请求参数。
- **插件协议**：GET capabilities 返回 `{version,providers,characters,free_sizes,model,steps,available,defaults,templates}`；defaults 提供 draw/reverse 的模型和评分/复审默认值，templates 提供 reverse/reverse_separate/draw_brief/multi_brief。POST plan 接受 `{mode,provider_id,character,characters,requirements,count,orientation,reference_mode,reference_decisions,references,style_name,artist_ratio,skill_bundles,brief,multi_brief,reverse,score_provider_ids,jpeg}`；references 为本地原文快照，skill_bundles 为 `{id,version,sha256,text}`；reverse 为原图、两种模板、是否分别、复审模型/候选参数。返回批次方案 `{plan_id,created_at,mode,plans,settings,warnings}`。POST start `{plan_id,request_id,seed}`；GET status/list/image、POST cancel 与旧创作同类语义，记录在 `lab/studio` 下，禁止混入旧严格 DTO。POST compare 接受 2–8 张 `{provider_id,prompt,images:[{id,mime,data_base64}]}`，第一张固定为基准图，视觉模型必须一次查看全部候选并按画风相似度返回最多三名：`{provider_id,text,latency_ms,reference_id,ranking:[{rank,image_id,similarity,reason}]}`。名次连续、候选不重复，similarity 为 0–100 的本批解释分，不能跨模型比较；解析失败保留原文并返回空 ranking，不编造赢家。旧 `pairs` 结果只供历史与交错升级读取。图片只用于此次请求，不对外发送。
- **桌面桥**：`studio_capabilities`、`studio_references(req)`、`studio_plan(req)`、`studio_start(req)`、`studio_status(run_id)`、`studio_list(limit)`、`studio_cancel(run_id)`、`studio_image(run_id,idx,format,quality)`；`studio_skills_list/import/delete` 管理资料；`studio_compare(req)` 将本地图片副本送选定视觉模型。Rust 负责本地 reference/skill 快照与图片边界，前端不直接传文件系统代理对象。矩阵以确定组合逐个构思/运行，各行独立错误与状态；活动任务与历史选择分离，保存 request_id，不自动重试计费构思。状态列表读取失败不回报空成功。
- **本地画风比较**：`style_compare(req:{images:[{id,path}],models:string[]})` 支持 2..32 张、上述三个固定注册编码器。所有无序图对分别计算原始 L2 向量余弦，不减本批均值、不写索引、不改当前模型；缺权重逐模型报错，不自动下载。返回输入 hash/尺寸与逐模型 `{model,label,model_version,preprocessing,device,fallback_reason,load_ms,encode_ms,total_ms,pairs:[{left_id,right_id,cosine}],error}`，顶层 `centered:false`。注册 revision 仅说明预期权重，不冒充用户文件已核验。前端每个模型独立阈值且注明尚未校准；有人工作品组标签时才算 pair accuracy/precision/recall，未标注不当阴性。不能用相似度断言原始画师串相同；其他模型仅有研究资料时明确未接入，不做虚假可运行按钮。
- **兼容修正与验收**：旧 creative.capabilities 的 characters 不属于需剥离的多人方案字段；修正响应裁剪，真实分发测试覆盖。全量等待与插件 480 秒对齐，桌面留足网络余量。仅合成图包/图片/模型桩、fixture 数据库、离线接口及浏览器验证；不调用真实付费模型、不重启线上机器人、不提交上游法典、用户图片或密钥。

### 16.1 实现接口细化

- `studio_prepare(req)` 一次冻结输入，返回 `{input_id,character,characters,reference_count,reference_ids,source_image?}`；矩阵所有行沿用同一 input_id。`studio_plan` 在固定输入上只覆盖模型/skill/简报/评分与复审参数，插件收到 `experiment_id=input_id`，保证同一轮画风抽签一致。`studio_plan_get(plan_id)` 可离线读冻结方案；`studio_workspace_get/save` 保留本轮配置/行 ID，最多16 MiB，不重复保存原图 Base64。
- `studio_skills_fetch(id="npe")` 由用户点击后从 `RefirmSaky/novelai-prompt-design-enhancer@58f98acfbc9b7556258a294183ef46057e00afb1` 下载主 SKILL 与设计参考到本机私有目录，不把无许可证的上游全文再分发进本仓。明确这是完整材料一次输入模式，不冒充原仓 Agent 的动态阅读；不附带可选画师库。单份材料最多768 KiB、组合最多1 MiB，实际摘要随方案冻结。
- 本地比较每图最多32 MiB、合计128 MiB、最多32M像素、长边16384、长宽比64；超限报错而非静默缩图。`style_compare_cancel()` 在模型/批次边界停止，已经开始的本地编码批次完成后释放共享推理锁。视觉 LLM 每次最多8张，使用与旧反推相同的输入预处理，源图本身不改。
- `studio_image` 返回 `{path,original_path,format,original_bytes,encoded_bytes,quality}`；原始 PNG 与 JPEG 衍生均缓存为本地绝对路径。PNG 格式 quality=null。实验图片未准备好是独立 pending 状态，不误判接口未安装。

## 15. 精选图包创作实验（2026-09-10，用户批准）

本节替代桌面 `/lab/draw` 的新任务流程；旧画图历史保留可读，反推不变。2026-09-10 用户确认插件已完成并授权对接：以插件 `65245ff` 已合入的创作接口为准，不再次应用旧扩展补丁。

### 15.2 桌面与共用创作协议

- **参考来源**：Rust 从本机 SQLite 读取 `nai5_community_pack` / `local_pack` 中 include、safe、未移除且不命中黑名单的条目，保留主提示词、负面词和角色子提示词。选择的 ID/目录只能缩小这个范围；空池明确报错，不回退其他决策或分级。散文可作为语义参考，不能当作旧随机池配方直接出图；这是对 §11.3 仅用于新创作模式的补充。
- **构思**：原始要求原样保存；指定角色保持其身份。2026-09-11 用户纠正：角色留空（含纯空白）时，桌面 Rust 在每次新构思前读取插件最新 `capabilities.characters`，从有效角色中随机选一个，作为明确角色传入插件；不得让 LLM 自选角色或从参考图包继承人物。空池、无有效角色或读取失败时停止，不调用构思模型。沿用插件角色池范围，不另造角色名单或擅自扩池。LLM 同时读参考快照、固定版本的通用 skill 和用户要求，在一次结构化调用中提炼构图手法、设计新事件并写实际 NAI 提示词。参考文本是数据，不能覆盖任务指令。硬要求优先于参考、skill 与随机构思。不声称训练了模型，也不承诺扩散模型每张都满足全部要求。
- **skill**：内置 skill id `nai5-prompting`（「NAI5 提示词写法」），正文为本仓库原创的简短写法说明 `resources/prompting-guide/GUIDE.md`（MIT，随应用代码），版本 `SKILL_VERSION = "builtin-1"`，随方案记录版本与摘要。不在运行时联网拉取；不假设第三方前端注入了预设。它不替代本应用的角色解析、完整画风配方或生成参数约束。
- **方案冻结**：`creative_plan` 返回不可变 `plan_id`；记录原始输入、参考 ID/哈希/快照、skill 与模型版本、构思、提示词/角色字段、完整画风、生成参数、LLM 原文及独立的要求检查。角色留空时同时保留原始空输入与发送给插件的实际选角，返回的 `plan.character` 必须与选角一致，并在界面显示。已冻结方案、编辑、执行和同构思再抽都保持这一角色；只有新构思才重新选角，旧历史不重写。`prompt` 已包含实际画风前缀，执行时不得再拼一次。画风在构思时抽一次，执行时不重抽。编辑通过 `creative_edit` 复核后生成新 `plan_id`，原方案不覆盖；复核有额外 LLM 调用。
- **执行**：`creative_start({plan_id,request_id,count,seed,score})` 只执行服务端已保存方案，拒收任意 prompt/size/付费参数。`request_id` 为客户端 UUID，重试同一请求返回同一 run，不重复出图；同 ID 不同内容拒绝。计数 1–8，逐张请求 `n_samples=1`。同一构思再抽一张仅变 seed；换构思才重新调用提示词模型。开始前重新检查当前本地决策，已删除/排除/拉黑的引用不能绕过筛选；原文正常更新不替换旧方案快照。
- **免费出图**：仅允许 `832x1216`、`1216x832`、`1024x1024`，沿用插件配置的 NovelAI V5 模型与 1–28 steps；配置不合格就显示原因，不静默改模型/步数。禁止加强/高清/付费重试路径。逐张在同一出图通道租约内读取新鲜订阅，必须 active Opus、有效使用量、`isNegative=false` 且点数至少 5%、距到期超过 10 分钟（保守余量）；读取失败或配额不足立即停止。不依赖旧缓存，不因免费出图不可用而转 Anlas。额外的构思/编辑/评分 LLM API 调用需单独标识。
- **费用事实**：官方公开图像请求没有已核实的服务器原子“最多扣 0 Anlas”字段；不能虚构这种保证。免费参数和逐张预检是应用保护，无法锁住同一订阅的其他客户端。执行前后读取余额，`anlas_spent` 未核实为 null，不能预填 0；异常变化/读取失败停止余下任务并记录原因。界面显示免费模式规则与实际读取结果，不把费用未知当作零。
- **停止和恢复**：cancel 幂等停止后续张数，正在远端执行的一张可能完成并记录；不承诺撤回已发送请求。排队、执行、部分结果、失败、取消分开保存；查看历史不改变活动任务的取消目标。记录原子落盘到应用独立目录，不改 schema 7、不写筛选决策；所有真实提示词/图片/快照排除在 Git 外。
- **接口**：新协议版本 1，桌面 `creative_{capabilities,references,plan,edit,plans_list,plan_references,start,status,list,cancel,image}`。插件新增 `page/lab/creative/{capabilities,plan,edit,start,status,list,cancel,image}`，统一 AstrBot JWT 后校验 `X-Library-Token`。返回仍为 `{status,message,data}`；计划与运行 DTO 见 `src/types-creative.ts`，字段 snake_case。capabilities 分 `available/reason`（构思）与 `render_available/render_reason`（生成）；旧插件 404 提示更新并重载支持创作接口的插件，不能误用旧 preview/start 接口。图片传回 base64，Rust 校验后缓存本地绝对路径。列表严格校验 Array，错误/未读/空记录分开。
- **图片准备状态（2026-09-11）**：插件运行状态中的图片 `done` 可能早于取图记录落盘。取图接口明确返回业务 404“这张图片尚未生成”时，桌面映射为 `creative_image_pending`，显示“图片准备中”并仅重试取图；不得误报接口未安装。每张图片请求去重，约 2 秒重试；正在跟踪的任务仍处于生成或评分阶段时持续显示准备状态，任务结束后至多再等待 60 秒，未跟踪的历史记录也使用 60 秒窗口，超时后给出重试取图入口。读取图片不阻塞运行状态刷新或取消。记录/文件确实不存在、鉴权、网络和响应损坏等错误独立显示，不能一律当作加载中。离开界面清理重试计时器；不重发构思或生图请求，不改插件、生成参数或筛选数据库。
- **并发边界**：创作自身逐张检查和余额变化即停继续生效，但不能承诺同插件进程的其他画图请求都被同一出图租约排除；桌面不改变插件的并发策略，也不声称可对并发造成的余额变化准确归因。
- **验证**：仅合成图包、假 LLM 与 renderer，不提交上游法典，不调用真实计费服务。需证明冻结后执行零新增构思调用、参数不重组、取消保留已完成、错误不伪装为空、三尺寸/单张/步数/订阅 guard 生效，并在宽窄屏深浅色测布局。

### 15.3 全量参考图包（2026-09-11，用户批准）

- 桌面画图实验室增加“全量参考当前范围图包”开关，默认关闭；桌面每次请求明确发送自己的模式，不隐式修改插件配置。
- 创作请求新增 `reference_mode: "sample" | "all"`，省略时为 `sample`。关闭沿用当前抽样规则（桌面原有重点参考/补充规则），开启后取消抽样条数限制：桌面使用当前 `scope` 内全部合格图包。合格仍为 include、safe、未移除、未拉黑的图包，单画师词典不参与。桌面关键词只过滤浏览列表、重点勾选不是全量模式的范围限制，界面明确说明范围由目录选择决定。
- 所有参考保留完整主提示词、负面词、角色子提示词、来源 ID/哈希；不附带图片或检索向量，不截断、不静默减条数。按 §21，全量参考及整个文本构思请求不再设应用层总字节预算。模型上下文拒绝或超时也不得暗中退回抽样。全量构思/编辑的应用模型等待上限 300 秒、桌面 HTTP 等待 360 秒，具体 provider 自身的更短超时仍可能生效；默认抽样路径保持原行为。
- `capabilities.full_reference_supported` 为可选布尔值，旧响应缺省 false；桌面全量模式在能力不支持时拒绝开始。新版客户端对所有创作 HTTP 请求附加 `features=full_references`；未协商时不返回新增字段以兼容旧严格 DTO，旧客户端访问全量方案明确提示更新，编辑/开始等须在副作用之前拒绝，不把全量历史伪装成抽样。冻结方案新增 `reference_mode`（旧历史缺省 sample）与 `used_reference_ids`（旧历史缺省未知）。既有 `reference_ids` 始终记录全部提供给模型的材料，`used_reference_ids` 仅记录模型声明借鉴的有效子集，不声称模型逐条实际阅读或训练过。界面分别显示这两种数量。编辑须重新声明借鉴条目，继续保留全部原始快照；再次出图继续复核全部引用资格。
- 全量调用在独立的 provider 浅副本上阻止 `pop_record` 自动裁剪；共享 provider 与 AstrBot 核心不改。上下文超限须保留材料并报错，不允许删空用户输入后重试。构思与编辑的大段重合校验仍覆盖全部参考，在线程中执行，避免大量材料阻塞 AstrBot 事件循环。
- 新开关只扩大构思材料，模型、角色、提示词原要求、三档免费尺寸、免费校验与配额保持既有契约。界面提示全量增加 LLM token 和等待时间，不能把模型 API 费用标成免费。仅合成池与假模型验证，不迁移 schema、不改真实筛选数据、不调用线上计费模型。

## 14. 桌面端 v2 对接与界面重构（2026-09-10，分阶段）

- 不更改 §3/§8 导出、三态、评测清洗、画质、张数或模型默认值；数据库 schema 继续为 7，真实决策不迁移重写。所有新增状态是旁路查询。
- 插件传输层按地址、用户名和密码身份在进程内复用 JWT；并发首次登录合并，401 只刷新一次；登录失败短暂冷却防止页面切换触发登录风暴。密码变化使用新身份；JWT/密码不落库。保留明文先试、仅 401 回退旧版 MD5 的兼容顺序；保留钥匙串进程读缓存。
- 原图展示：新增 `reverse_source_image(run_id) -> {path:string|null, kind:"original"|"display"|"thumbnail"|"missing"}`。只读本地文件：词典题优先已有 original_local_path，再 local_path，再 thumb_path；本地题取拷入应用的图片。缩略图回退必须标明，缺文件明确占位；不自动下载，不改变模型输入。
- 插件桥：无参命令 `plugin_model_selection() -> any`，只 GET 插件 `page/model-selection`（§17 常用 LLM 名单）并原样返回插件 `data`；前端 `api.pluginModelSelection()`。JWT 鉴权；该接口不要求 X-Library-Token，库/实验室原有双重鉴权保留。非 2xx 与 `{status:"error"}` 均抛出 AppError。早期的通用控制台转发命令已删除，桌面端不再调用其余控制台路由；不得成为任意 URL 代理。
- 所有 IPC 的数据参数在边界 JSON 快照化（保留 Channel 的 Tauri 序列化协议），避免传递 Vue Proxy；mock 仅独立离线开发模式启用，不使用真实配置或密钥。
- 视觉：Naive UI + 单一 CSS 令牌来源，跟随系统外观；金色 Anlas、青绿点数，同时区分不消耗出图配额与额外模型 API 调用。未知费用不标免费。数字等宽，宽内容局部滚动，加载/空/错误各自独立。

后端 Rust（`src-tauri/`）与前端 Vue（`src/`）由不同的人并行实现，只靠这份文件对齐。改契约先改这里，再改两边。

## 0. 通用约定

- 命令名 `snake_case`。**参数名也是 `snake_case`**：Rust 侧每个命令都写 `#[tauri::command(rename_all = "snake_case")]`，前端 `invoke("list_entries", { query })`。
- DTO 字段全部 `snake_case`，Rust `serde` 不做 rename。TS 类型在 `src/types.ts`，Rust 在 `src-tauri/src/models.rs`，**两边逐字段一致**，`types.ts` 是形状的权威。
- 时间一律 ISO 8601 UTC 字符串，如 `2026-09-06T12:00:00Z`（Rust：`Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true)`）。
- 命令返回 `Result<T, AppError>`。`AppError` 序列化为 `{ "code": string, "message": string }`；前端 `invoke` 抛出的就是这个对象。常见 code：`sync_running`、`batch_running`、`not_found`、`bad_request`、`upstream`、`io`、`db`、`secret`、`network`、`plugin_rejected`。
- 图片：命令只返回本地**绝对路径**；前端用 `convertFileSrc(path)` 显示。没下载的图 `local_path` 为 `null`，前端显示占位。
- 长任务用 `tauri::ipc::Channel<T>` 推进度，命令本身立即返回（`sync_start` 返回时后台任务已启动）。前端：`const ch = new Channel<SyncEvent>(); ch.onmessage = e => …; await invoke("sync_start", { force, on_event: ch })`。
- 数据目录 = `app.path().app_data_dir()`（macOS：`~/Library/Application Support/moe.vivian.codexlab/`）。子目录：`codex_raw/<release>/`、`images/<codex_id>/`、`originals/<codex_id>/`、`thumbs/<codex_id>/`、`backups/`、`exports/`、`logs/`。数据库 `library.db` 在根目录。
- 缩略图：320 px 最长边 JPEG（质量 80），文件名 `<image_file>.jpg` 放 `thumbs/<codex_id>/`。（设计稿写 WebP，实现改 JPEG，避免原生 libwebp 依赖。）
- 上游请求：UA `nai5-codex-lab/<version>`；对 `assets.quicktagcloud.com` 全局并发 4、间隔 ≥ 100 ms；对 `novelai.quicktagcloud.com` 每次同步 1 次。
- SQLite：`PRAGMA foreign_keys=ON; journal_mode=WAL; synchronous=NORMAL;`。DDL 在 `src-tauri/migrations/001_init.sql`，`schema_version` 表记录已应用版本。
- 日志：`tracing` JSON 行写 `logs/nai5-codex-lab.YYYY-MM-DD.log`（按天滚动），同时写 `app_event` 表（同步起止、导出、推送、批量决策、实验 batch 起止、错误）。

## 1. 命令清单

| 命令 | 参数 | 返回 | 说明 |
|---|---|---|---|
| `app_info` | — | `AppInfo` | 版本、数据目录、`latest_release` |
| `settings_get` | — | `Settings` | 存 `app_state('settings')` JSON |
| `settings_set` | `patch: SettingsPatch` | `Settings` | 合并写回，未知键拒绝（`bad_request`） |
| `secret_set` | `account: string, value: string` | — | 钥匙串 service `nai5-codex-lab`；account 取值：`webui_password`、`library_token`、`provider:<provider_id>` |
| `secret_has` | `account` | `boolean` | |
| `secret_delete` | `account` | — | |
| `sync_status` | — | `SyncStatus` | |
| `sync_start` | `force: boolean, on_event: Channel<SyncEvent>` | — | 已在跑则 `sync_running` |
| `sync_cancel` | — | — | 取消后台任务，已落地的不回滚 |
| `sync_logs` | `limit: number` | `SyncLog[]` | 最新在前 |
| `list_books` | — | `Book[]` | 含每本的决策计数 |
| `list_entries` | `query: EntryQuery` | `EntryPage` | 分页；`counts` 是**当前筛选**下的三态计数 |
| `get_entry` | `entry_id: string` | `EntryDetail` | |
| `set_decision` | `entry_ids: string[], decision: Decision, via: DecisionVia` | `{ changed: number }` | 每条写 `membership_audit` |
| `set_member_note` | `entry_id: string, note: string` | — | |
| `undo_decision` | — | `UndoResult \| null` | 撤销最近一次非 undo 的审计记录 |
| `download_images` | `entry_id: string, kind: "display_all" \| "originals", on_event: Channel<SyncEvent>` | — | 按需下载；与同步共用队列与节流 |
| `blacklist_list` | — | `BlacklistItem[]` | |
| `blacklist_add` | `name: string, reason: string` | — | name 先 canonical（小写、空格→`_`） |
| `blacklist_remove` | `name: string` | — | |
| `export_libraries` | `options: ExportOptions` | `ExportResult` | 写 `exports/<时间戳>/`（或 `out_dir`）三个文件，规则见 §3；`ExportOptions = { out_dir?: string }` |
| `push_to_plugin` | — | `PushResult` | 先导出再推送；JWT 登录见 §3.4 |
| `reverse_templates_list` | — | `ReverseTemplate[]` | |
| `reverse_template_save` | `input: ReverseTemplateInput` | `ReverseTemplate` | 同名则 version+1，永不覆盖 |
| `reverse_rulesets_list` | — | `ReverseRuleset[]` | |
| `reverse_ruleset_save` | `input: ReverseRulesetInput` | `ReverseRuleset` | 同上 |
| `reverse_providers_list` | — | `ReverseProvider[]` | 不含 key |
| `reverse_provider_save` | `input: ReverseProviderInput` | `ReverseProvider` | key 走 `secret_set("provider:<id>")` |
| `reverse_provider_delete` | `provider_id: number` | — | |
| `reverse_questions` | `query: QuestionQuery` | `Question[]` | 条目 × 图片 ordinal，带真值 |
| `reverse_pick_local_image` | — | `string \| null` | Rust 侧打开文件对话框 |
| `reverse_run_batch` | `req: RunBatchRequest, on_event: Channel<RunEvent>` | `{ batch_id: string }` | 立即返回，后台跑 |
| `reverse_cancel_batch` | `batch_id: string` | — | 停止指定的活动批次；已结束或不匹配时幂等返回，不影响其他批次；见 §12.9 |
| `reverse_batches_list` | — | `BatchSummary[]` | |
| `reverse_batch_results` | `batch_id: string` | `BatchResults` | |
| `reverse_run_get` | `run_id: number` | `RunDetail` | 含 raw_response 全文 |
| `reverse_compare` | `batch_a: string, batch_b: string` | `CompareResult` | 按 question 对齐 |
| `reverse_judge` | `run_id: number, score: number, comment: string` | — | |
| `events_recent` | `limit: number` | `AppEvent[]` | |
| `plugin_model_selection` | — | `any`（插件 `data`） | 只 GET 插件 `page/model-selection`（常用 LLM 名单），见 §14、§17 |

后续版本新增的命令写在各自章节（§8.2、§9.4、§10.1、§11.4、§12、§14–§16、§22 等）。早期的通用插件控制台转发命令与实验室点数查询命令已删除，由上表的 `plugin_model_selection` 取代需要的部分。

内置数据：首次启动时若 `reverse_template` / `reverse_ruleset` 为空，后端插入 `plugin v1`（模板与规则原文在 `src-tauri/src/reverse/builtin.rs`，来自插件 `reverse_draw.py` 当前版本）。

## 2. 事件形状

`SyncEvent`（同步与按需下载共用）：`{ phase, message, release, added, changed, images_total, images_done, images_failed }`，`phase ∈ manifest | entries | images | done | error`。`done` 与 `error` 各只发一次，之后不再发。

`RunEvent`：`{ batch_id, run_id, status, done, total, message }`，`status ∈ started | run_done | run_failed | batch_done | batch_failed | batch_cancelled`。

## 3. 导出与推送规则（后端实现，前端只展示结果）

### 3.1 `nai5_artist_pool.json` ← 视图 `artist_library`

1. 解析前剥 `<artist>`、`</artist>`、`<style>`、`</style>` 与换行；`path[0]='单画师词典'` 且 tags 是不带前缀的名字（不含 `artist:`）的按画师处理并补前缀。
2. `artist_parse.artists` 为空的条目跳过并记 `app_event`。
3. 同 `signature` 只出一条：优先 `decided_at` 最新的 include，其次 `update_batches` 最新。
4. 过 `artist_blacklist`（name canonical 比较）。
5. 序列化：`core = artists 按原顺序，权重 |w−1|<0.001 省略写 artist:name，否则 w::artist:name::（w 最多两位小数去尾零，:: 前无空格）`，用 `, ` 连接；输出 `core + ", amazing quality, very aesthetic, absurdres, masterpiece, amazing, best quality, -2::artist collaboration::"`。文件是 `list[string]`。

### 3.2 `pack_pool.json` ← 视图 `pack_library`（只含 safe 图包条目，插件端负责与旧池合并）

`{ "source": "法典图鉴 N5社区图包 v<version>", "note": "由 nai5-codex-lab 导出；未过插件清洗", "entries": [ { "s", "c", "t", "tags", "r" } ] }`

- `s` = path[0] 去掉「·」后的作者段；`c` = `"图包/" + path[1] + "/" + path[2]`（缺的段跳过）；`t` = `title + " · " + 前 3 个非画师非质量 token 的 value`；`r` = rating（恒为 `safe`）。
- `tags` 清洗：`parseArtistRecipe` 后剔除 `kind ∈ {artist, quality, negative, year}` 与 value 匹配 `^rating:` 的 token；剩余 token 用 `formatToken`（权重 1 写原值，否则 `w::value::`）按原顺序 `, ` 连接。
- 只含 `rating='safe'`（非 safe 分级的条目在解析时丢弃，不入库）。

### 3.3 `library_export.json`

`{ "schema": "nai5-codex-lab/1", "exported_at", "release_id", "entries": [ { entry_id, library, title, path, tags, negative, character_prompts, rating, member_note, images: [ { ordinal, raw_tag, file } ] } ] }`，`file` 是相对数据目录的路径。

### 3.4 推送

1. `POST {plugin_base_url}/api/auth/login`，body `{ "username": settings.webui_username, "password": 钥匙串 webui_password }`；响应 `data.token` 为 JWT（AstrBot 返回形状 `{ status, message, data: { token, username, change_pwd_hint } }`）。
2. `POST {plugin_base_url}/api/plug/astrbot_plugin_vivian/page/library/import`，头 `Authorization: Bearer <jwt>`、`X-Library-Token: <钥匙串 library_token>`、`Content-Type: application/json`，body `{ "nai5_artist_pool": [...], "pack_pool": {...}, "library_export": {...}, "exported_at", "release_id" }`。
3. 401 一次自动重登；插件返回非 2xx → `plugin_rejected`，message 带响应正文前 500 字。响应 JSON 原样放进 `PushResult.plugin_response`；`PushResult` 的计数字段为 `nai5` / `pack` / `dropped_blacklist`。

## 4. 反推评测口径（后端 `reverse/metrics.rs` 实现，前端只展示）

- 归一 `norm(t)`：小写、`_`→空格、去首尾空白与权重语法。
- `truth` = `COALESCE(entry_image.raw_tag, codex_entry.tags)` ∪ 所有 `character_prompts[].prompt`，各自 `parseArtistRecipe` 后取 `norm(value)`。
- 剔除类（不在答案里）：`kind ∈ {artist, quality, negative, year}`；quality 扩展表 `absurdres, highres, best illustration, high complexity, highly complexity, ultra complexity, no text`；`^rating:`。
- `expected_miss`（保留在答案但不计分母）：角色名（`\(.+\)$` 括号型、`character_aliases` 角色别名表（合成示例）命中、每个 character_prompts label 的首个非通用 token）；外貌词（规则集 `appearance_regex` 命中且不命中 `accessory_ok_regex`）；人数词 `1girl|1boy|solo|2girls|2boys|multiple girls|multiple boys`；规则集 `banned` 命中。
- `pred_raw` = `parsed.tags` 归一集合；`pred` = `cleaned_tags ∪ {parsed.framing}`。
- `truth_eval = truth − 剔除类 − expected_miss`；`hit = |pred ∩ truth_eval|`；`precision = hit/|pred|`（`|pred|=0` 时 0）；`recall = hit/|truth_eval|`（分母 0 时 null）；`f1` 常规；`extra = |pred − truth|`；`expected_hit = |pred ∩ expected_miss|`；`appearance_leak`、`banned_leak` 在 `pred_raw` 上算；`ruleset_drop = |pred_raw| − |pred|`；`contradiction = |pred ∩ norm(negative ∪ character_prompts[].negative)|`。
- 规则集清洗（对应插件 `_clean_tags`）：按逗号/换行切分 → trim 与去引号 → 去重（含 `always_drop` 初始集合、角色名、framing）→ 丢弃长度 > `max_len` 或词数 > `max_words` 或命中 `refusal_regex` 的 → 丢弃 `banned` 子串命中的 → 丢弃 `appearance_regex` 命中且不命中 `accessory_ok_regex` 的。
- 送模型前预处理：文件 > 9 MB 才缩到最长边 2048、JPEG 88→72→55 直到 ≤ 9 MB；请求体 OpenAI 兼容 `chat/completions`，`messages=[{role:"system", content: template.body}, {role:"user", content:[{type:"image_url", image_url:{url:"data:<mime>;base64,…", detail:"high"}}, {type:"text", text: "Generate the prompt for this image."}]}]`，`temperature=0`，`max_tokens=provider.max_tokens`，超时 180 s。模板里的 `{extra}` 占位替换为空串。
- 解析：从响应文本里取第一个 `{…}` JSON；失败则 `status='error'`；`suitable=false` 或含拒答语按 `status='refused'`。

## 6. v0.1.0 实现后的补充裁定（两边已按此对齐）

- `EntryQuery.rating` / `decision`（以及 `QuestionQuery`）：**显式空数组 `[]` → 0 条**；键缺失 → 无此筛选。`path_prefix` 按 `path_json` 前 N 个元素逐个相等匹配。
- `EntryPage.counts` 应用除 `decision` 之外的全部筛选（这样筛"只看未定"时仍能看到加入/不加入各多少）；`total` 与 `items` 应用全部筛选。
- `set_decision`：决策未变化的条目不写审计、不计入 `changed`；未知 id 静默跳过。
- `undo_decision`：已撤销的审计 id 记在 `app_state('undone_audit_ids')`，连续撤销逐条回退；备注不动。
- `download_images` 有自己的取消令牌，`sync_cancel` 不影响它；"下载全部"的标记持久化在 `app_state('download_all')`，之后每次同步都会补齐这些条目。
- 本地图片题：`RunBatchRequest.question_ids` 用 `local:<绝对路径>`；`RunRow.question_id` 原样返回该串（不是 null），`metric` 为 null。
- `reverse_provider_save` 先返回 `provider_id`，前端再 `secret_set("provider:<id>")`；`has_key` 由钥匙串是否存在推导；钥匙串"无此条目"视为未设置而不是错误。
- 推送登录（v0.2.1 改）：AstrBot ≥4.27 的服务器端自己做哈希（md5 或 pbkdf2），客户端必须发明文；≤4.26 的登录页在浏览器里预先 md5。后端先发明文，收到 401 再发 `md5(明文)`，两次都 401 才报 `plugin_rejected`（v0.1.1–v0.2.0 曾永远发 md5，在 4.27 上登录必败，已废止）。
- `format_token` 输出 `w::value::`（`::` 前无空格）。`precision` 在 `|pred|=0` 时为 0；`ruleset_drop` 不小于 0。
- `pack_pool.json` 跳过清洗后 tags 为空的条目，计入 `ExportResult.skipped`；`library_export.json` 的 `images[].file` 固定为 `images/<codex_id>/<file>`。
- 上游消失 id ≥ 10%：不打 `removed_at`、写 warn 级 `app_event`、同步继续（不做交互式暂停）。
- `codex_raw/<release>/` 额外保留 `current.json` 与 `manifest.json`。

## 7. v0.1.2：分页与排序（用户验收后的第一批需求）

- `EntryQuery.sort` 扩为 `book_order | title | decided_at | synced_at | first_seen`，新增 `sort_dir?: "asc" | "desc"`。缺省方向：`book_order`/`title`/`synced_at` 升序，`decided_at`/`first_seen` 降序。次序键一律再按 `book_order` 升序稳定。
  - `synced_at` = 该条目 `entry_image` 中 `ordinal=0` 的 `downloaded_at`；为 NULL（未下载）的条目无论方向都排最后。
  - `first_seen` = `codex_release.published_at`（按 `first_seen_release` JOIN）；`local` 等无 release 的条目排最后。
  - `decided_at` 只对有 `library_membership` 的条目有值，其余排最后。
- 前端从"滚到底自动加载"改为**显式分页**：每页 50 / 100 / 200 可选，上一页 / 下一页 / 跳页，`Cmd+←` `Cmd+→` 翻页；`page`、`page_size`、`sort`、`sort_dir` 与其余筛选一起持久化在 `codexlab.filter.<codex_id>`，重开应用回到上次那一页。筛选条件变化时 `page` 归 1；`total` 变小导致当前页越界时回到最后一页。
- 后端 `list_entries` 不变形状，`offset = (page-1) * page_size`。

## 8. v0.1.4：完整池导出与推送（桌面端 ⇄ 插件"对接系统"）

### 8.0 两种池

- **筛选池**：§3 的三个文件（`nai5_artist_pool.json` / `pack_pool.json` / `library_export.json`），只含"已加入"且过黑名单的条目；插件的 /画 /反推 随机抽画师串用它。原有命令与路由不变。
- **完整池**：两本词典的全部条目（含未加入、未决策、黑名单命中的），每条带决策标记。给插件端将来做画风检索等"要看全量"的功能。本版插件只负责接收、落盘、展示状态、提供读取接口；**不改变** /画 /反推 的行为。

### 8.1 `full_pool.json`（后端 `export::build_full_pool`）

```
{ "schema": "nai5-codex-lab/full/1", "exported_at": ISO8601, "release_id": string|null, "app_version": string,
  "counts": { "<codex_id>": { "total", "include", "exclude", "undecided", "blacklisted" } },
  "codexes": [ { "codex_id", "title", "library": "artist"|"pack", "entries": [ FullEntry ] } ],
  "embedding": null }

FullEntry = { "entry_id", "title", "path": string[], "book_order": int, "tags", "negative": string|null,
  "character_prompts": [ { label, prompt, negative? } ], "rating": "safe",
  "decision": "include"|"exclude"|"undecided", "decided_at": ISO|null, "member_note": string|null, "blacklisted": bool,
  "artists": [ { "name", "weight" } ]|null, "core": string|null, "pack": { s, c, t, tags, r }|null,
  "images": [ { "ordinal", "raw_tag": string|null, "sha256": string|null, "file": string|null } ] }
```

- codex 顺序固定：`artist_nai5_personal` 在前、`nai5_community_pack` 在后；`entries` 按 `book_order` 升序。
- rating 规则与 §3.2 一致，两本都适用：只有 `safe`。
- `decision`：`library_membership` 有记录取其值，否则 `"undecided"`。`removed_at` 非空的条目不导出。
- `blacklisted`：`artist_parse.artists` 任一 name canonical 命中 `artist_blacklist` 为 true。完整池**不剔除**黑名单条目，只标记。
- `artists` / `core`：仅 `library='artist'` 且 `artist_parse` 有结果时填；`core` = §3.1 第 5 步的序列化，**不带**质量后缀；解析失败为 null（条目仍导出）。
- `pack`：仅 `library='pack'`：按 §3.2 清洗；清洗后 tags 为空 → null（条目仍导出）。
- `images`：该条目 `entry_image` 全部 ordinal；`file` = 已下载则 `images/<codex_id>/<file>`，否则 null；`sha256` = `entry_image.sha256`（未下载 null）。
- `embedding` 预留：本版恒为 null。将来为 `{ "model": string, "dim": int }` 时每条 FullEntry 追加 `"vec": base64(float16[dim])`。插件端遇到非 null 也必须能存能显示（当作不透明字段）。

### 8.2 新命令

| 命令 | 参数 | 返回 | 说明 |
|---|---|---|---|
| `export_full_pool` | `options: ExportOptions` | `ExportResult` | 写 `exports/<时间戳>/full_pool.json`；`files` 一项 `name="full_pool.json"`，`count`=条目总数；`skipped=0` |
| `push_full_to_plugin` | — | `PushFullResult` | 内存构建后推送，不落盘；登录与 401 重登同 §3.4，路由 `/page/library/import_full` |
| `plugin_library_status` | — | `PluginLibraryStatus` | GET `/page/library/status`；登录同 §3.4 |

```
PushFullResult = { ok: bool, message: string, total: int, counts: Record<codex_id, { total, include, exclude, undecided, blacklisted }>, bak: string[], plugin_response: any }
PluginLibraryStatus = { reachable: bool, message: string, plugin_version: string|null, filtered: PoolStatus|null, full: PoolStatus|null }
PoolStatus = { imported_at: ISO, release_id: string|null, exported_at: string|null, app_version: string|null, counts: Record<string, number>, file: string, size_bytes: int, backups: string[] }
```

- `filtered.counts` = `{ nai5, pack, kept_old, dropped_blacklist }`；`full.counts` = 按 codex_id 的 total（与 8.1 `counts[codex].total` 一致）。
- 网络错误（连不上/超时/未配置密码或 token）→ `plugin_library_status` 返回 `reachable=false` + `message`，**不抛错**；401/403 → `plugin_rejected`。

### 8.3 插件端路由（新模块 `src/lab_sync.py`；`web_page.py` 只做注册）

所有路由都在 dashboard JWT 之后再核对 `X-Library-Token`（同 §3.4）。

- `POST /page/library/import` — 行为不变，但成功后写 `filtered.meta.json`。
- `POST /page/library/import_full` — body = 8.1。校验：`schema` 以 `nai5-codex-lab/full/` 开头、`codexes` 为数组、每条 `entry_id`/`title` 为非空字符串、`tags` 为字符串、`decision` ∈ 三值；最多 20 条错误合并成 400。成功：原子写文件，旧文件改名 `.bak-<ts>`，只保留最近 5 个备份。返回 `{ ok, total, counts, bak, release_id, exported_at }`（`counts` 原样回传 8.1 的 `counts`）。
- `GET /page/library/status` — 返回 `{ plugin_version, filtered: PoolStatus|null, full: PoolStatus|null }`。
- 存储目录：`StarTools.get_data_dir(PLUGIN_NAME)/lab/`（在插件 git 仓库之外，永不入库）：`full_pool.json`、`full_pool.meta.json`、`filtered.meta.json`、`full_pool.json.bak-*`。`library_export.json` 从 `src/prompts/` 迁到该目录（此前无消费者）。
- 读取接口（给未来功能用）：`lab_sync.full_pool()` 按 mtime 热载返回 dict；`lab_sync.full_entries(codex_id=None, decision=None, rating=None)` 迭代 FullEntry；`lab_sync.status()` 与 GET 路由同形。
- 管理页：新增"桌面端对接"板块：`lab.library_token` 配置项、两种池的状态卡（导入时间 / release / 条目数 / 备份数）、刷新按钮。

### 8.4 前端

设置页"导出与推送"分成两行：筛选池（现有两个按钮）与完整池（"导出完整池" / "推送完整池"）。下方"插件当前持有"面板：进入设置页自动拉一次 `plugin_library_status`，有刷新按钮；不可达时显示原因文本，不弹错误 toast。

## 9. v0.2.0：画图实验室（出图在插件跑，Mac 只做界面与记录）

### 9.0 裁定（用户 2026-09-07 拍板）

- 出图、提示词组装、清洗、评分全部在插件里跑，走和插件指令相同的函数；Mac 端只负责选图包、下参数、看结果、打分。日志留在插件数据目录，Mac 端镜像一份。
- 实验室只使用 safe 图包，图包标签恒经清洗。
- 画图实验室有"画风池命中概率"（0–100%，画风池 = 已推送的筛选池 `nai5_artist_pool.json`）和"一次出几张"（1–8）。
- **事实修正**：插件 `/画` 指令目前没有图包路径，它是 LLM 主题系统（构图/服装/场景骰子）。因此画图室的"按图包画"是一条新路径；带自然语言描述且未选图包时才走 `/画` 现有 LLM 路径。这条裁定不改动插件指令的行为。

### 9.1 请求与计划（两端共用的 DTO，snake_case）

```
LabRunRequest = {
  mode: "draw",
  text: string,                 // 用户敲在 /画 后面的那串：首 token 角色（或 随机），其余为自然语言描述；可含 风格=xxx
  entry_id: string | null,      // 选中的图包（完整池里 nai5_community_pack 的 entry_id）；null = 随机
  scope: string[][],            // 随机时的候选范围：path 前缀列表（与 EntryQuery.path_prefix 同语义）；空数组 = 默认范围
  artist_ratio: number,         // 0..1，命中则注入画风池串
  count: number,                // 1..8
  score: boolean,               // 是否跑三评委
  seed: number | null           // 固定种子（每张 +i）；null = 随机
}
LabPlan = { idx, character, pack_entry_id: string|null, pack_title: string|null, pack_path: string[]|null,
            tags_raw: string, tags_used: string, dropped: string[], extra_tags: string|null,
            prompt: string, negative: string, artist: string, artist_source: "style"|"pool"|"none",
            size: string, seed: number|null, model: string }
LabImage = LabPlan & { status: "pending"|"done"|"error", file: string|null, error: string|null,
                       score: number|null, notes: string[], rating: "fav"|"dis"|null }
LabRun = { run_id, mode, request: LabRunRequest, status: "queued"|"running"|"done"|"error",
           created_at, finished_at: string|null, stamina_before: number|null, stamina_after: number|null,
           images: LabImage[], error: string|null, plugin_version }
LabRunSummary = { run_id, mode, status, created_at, count, done, text, pack_title: string|null, best_score: number|null }
```

### 9.2 插件端组装规则（新模块 `src/lab_run.py`；复用 `auto_gallery.py` / `reverse_draw.py` 等现有函数，禁止复制粘贴逻辑）

1. **角色**：`text` 按插件 `/画` 指令同款切分（`风格=` → `_parse_style`；首 token `随机` 或 `_resolve_character`；其余为 `desc`）。角色未命中且像名字 → 400 带角色列表。
2. **图包来源**：`lab_sync.full_entries(lab_dir, codex_id="nai5_community_pack", decision="include")`。`entry_id` 指定则取它（不在完整池 → 400 "完整池里没有该条目，请先推送完整池"）；否则在 `scope` 内随机，`scope=[]` 时默认范围为 `rating='safe'`。候选为空 → 400。
3. **图包标签**：用 `entry.pack.tags`（§3.2 已剥画师/质量/负面/年份/rating），再过插件的 `scrub_entry`（`_ENTRY_BAN`）；`dropped` = 两步剔掉的 token 列表。
4. **自然语言 `desc`**：
   - 未选图包 → 走 `/画` 现有路径 `ag._build_prompt(1, mc, character, extra=desc, n5_kind="draw")`（每张独立骰子）；选了图包 → 图包组装 + `desc` 经 `_scrub_freestyle` 成 `extra_tags`。
   - `_llm_compose` 里 `event.unified_msg_origin` 的兜底改成 `event is not None` 守卫（与 `scene.py:117` 同款），插件指令行为不变。
5. **多张不同**：`count>1` 时每张独立随机图包 / 独立画风池掷骰 / 独立 seed；`entry_id` 指定时图包固定但画风与 seed 仍独立。
6. **负面**：`defaults.negative_prompt`。
7. **画师**：`风格=` → `style.tags`；否则 `random() < artist_ratio` → `artist_pool.pool()` 随机一条（`artist_source="pool"`）；未命中为 `"none"`。
8. **拼装**：`prompt = character, person_tag, tags_used[, extra_tags], defaults.prompt`；`size = size_for_tags(plugin, prompt)`；`model = defaults.model`（实验室不走配额/V5 门）；`steps/scale/sampler/noise_schedule` 同插件出图默认值。
9. **出图**：逐张 `Req` → `sem.at(PRIO_NORMAL)` + `lease_token()` + `wrapped_generate(..., owner_id="vva_lab")`，最多 2 张并发，失败只标记该张 `status="error"`。文件写 `ag.out_dir / f"lab_{mode}_{ts}_{idx+1}.png"`。`score=true` → `ag._score_image(fp)`。
10. **记录**：`lab_dir/runs/<run_id>.json` 全量 `LabRun`；`lab_dir/runs/index.jsonl` 每次追加一行 `LabRunSummary`；`logger.info` 一行摘要。评分（fav/dis）写回 run json。同一时刻只允许一个实验室任务（409）。后台任务登记进 `plugin._background_tasks`。

### 9.3 插件端路由（JWT 之后再核对 `X-Library-Token`，与 §8.3 相同，GET 也接受 `?token=`）

| 路由 | 说明 |
|---|---|
| `POST /page/lab/run/preview` | body `LabRunRequest` → `{ plans: LabPlan[] }`，只组装不出图（LLM 路径会调 LLM） |
| `POST /page/lab/run/start` | body `LabRunRequest` → `{ run_id, plans }`；运行中 → 409 |
| `GET /page/lab/run/status?run_id=` | `LabRun` |
| `GET /page/lab/run/list?limit=` | `LabRunSummary[]`，新的在前（读 index.jsonl 尾部） |
| `GET /page/lab/run/image?run_id=&idx=&full=0|1` | `{ name, mime, data_base64 }`，复用 `page_thumb` 的缩略/全图逻辑 |
| `POST /page/lab/run/rate` | `{ run_id, idx, rating: "fav"|"dis"|null }` → `{ ok }` |

管理页"桌面端对接"板块下方加"实验室最近运行"只读列表（最近 10 条 `LabRunSummary`）。

### 9.4 桌面端

- 新命令：`lab_run_preview(req)`、`lab_run_start(req)`、`lab_run_status(run_id)`、`lab_run_list(limit)`、`lab_run_image(run_id, idx, full)`（首次从插件拉取并落盘 `labruns/<run_id>/<idx>[.thumb].png`，返回相对数据目录路径；之后直接返回本地）、`lab_run_rate(run_id, idx, rating)`（先写插件，成功后写本地）。网络语义同 §8.2（不可达 → `AppError network`）。早期的实验室点数查询命令与 `LabStamina` / `LabStaminaAccount` 类型已删除，插件 `GET /page/lab/stamina` 桌面端不再使用。
- 本地表 `lab_run(run_id PK, mode, created_at, status, json)`：每次 `lab_run_status` 返回 `done|error` 时落盘一份，`lab_run_list` 合并本地与插件（本地优先，按 created_at 倒序）。
- 视图：`/lab/draw` 画图实验室（v0.5.5 起旧路径重定向到 §16 总实验室，旧页面已删除；以下为历史描述）。左栏：图包树（本地 `codex_entry` 中 `nai5_community_pack`、`decision=include`、只含 safe），可勾选目录作为 `scope`，或点单条作为 `entry_id`，顶部"随机"；中栏：角色下拉（来自插件 `char_pool`，取不到就文本框，默认 薇拉）+ 描述文本框（拼成 `text`）、画风池概率滑块（默认 50%）、张数（默认 1）、评分开关、固定种子；按钮 预览 / 出图。右栏：结果卡片（图、最终 prompt、画师及来源、seed、图包标题、清洗前后 diff、评分与评语、收藏/点踩），下方历史运行列表（点开回看）。`lab_run_status` 每 2 s 轮询直到 `done|error`。

### 9.5 加强画图（v0.2.1，本轮 v0.2.0 不实现；用户 2026-09-07 提出）

- `LabRunRequest.enhance: "off" | "on" | "ab"`，仅 `mode="draw"` 生效，默认 `"off"`。`"ab"` = 对照：每个计划生成两张，同图包同 seed，一张 `off` 一张 `on`，`count` 上限因此为 4。
- 插件端 `lab_run.enhance(plugin, plan) -> plan`：在 `tags_used`（已清洗）基础上调一次 LLM（provider 同 `_llm_pick` 的解析顺序），提示词要求：保留构图/姿势/服装/场景的核心 token；补充与该构图相符的高质量细节（光线、材质、镜头、氛围、景深），最多 8 个；删除累赘、重复或互相冲突的 token，最多删原 token 数的 30%；禁止输出外貌、人数、角色名、画师名、质量词。输出 JSON `{ "keep": [], "add": [], "drop": [], "reason": "<20 字>" }`。
- 结果再过 `_scrub_freestyle` 与 `/画` 的外貌过滤（`auto_gallery._build_prompt` 的同款后置过滤），保证加的词不与角色设定冲突；LLM 失败或超时 → 原样出图并在计划里记 `enhance_error`。
- `LabPlan` 追加 `enhance: { mode: "off"|"on", added: string[], dropped: string[], reason: string|null, error: string|null } | null`；`tags_used` 为加强后的串，`tags_raw` 不变；界面三色显示 保留 / 新增 / 删除。
- 评分与收藏按 `enhance.mode` 聚合，给出"加强 vs 不加强"的均分差。本条不动插件指令。

### 9.6 v0.2.0 联调后的裁定

- **图包标签的来源（同时修正 §3.2 与 §9.2-3）**：社区图包里 1168/1903 条把姿势与服装写在 `character_prompts[].prompt` 而不是 `tags`。因此图包标签 = `tags` 与全部 `character_prompts[].prompt` 依次拼接（主提示词在前，各角色提示词按顺序在后，用 `, ` 连接），再做清洗：实验室与导出 → §3.2 的清洗 + 剔角色 token。角色 token 的判定（两端一致）：任何位置的 `\(.+\)$` 括号型与角色别名表（`character_aliases`，合成示例）命中；label 非占位（非 char1/角色1 这类）时其首个非通用 token；每个角色提示词的首个非通用 token **仅当**它不在「主标签词汇表」里时才算角色名（词汇表 = 同词典全部条目主 `tags` 的归一 token 集合，任意决策；括号型不入表）。这样以 cowboy shot 开头的无名角色提示词不会被误删，而 takamatsu tomori 这类不带括号后缀的角色名会被剔掉。`pack_pool.json` 与 `full_pool.json` 的 `pack.tags` 都按此生成（v0.2.0 起）。
- `lab_run_status` 插件不可达时不做本地回退（历史列表用本地行即可）；`lab_run_image` 缓存文件后缀按 mime 决定（插件返回 JPEG 则 `.jpg`）。

## 10. v0.2.3：反推实验室用插件里的模型 + 拖图进题库

### 10.0 裁定（用户 2026-09-07 提出）

- 已经连上插件，反推实验室的"模型"应当能直接选 AstrBot 里配置好的 provider，不必在 Mac 上再填 base_url / key。本地 OpenAI 兼容模型仍保留。
- 题库支持把图片文件直接拖进窗口，等价于"添加本地图片"。

### 10.1 模型的两种来源

- `reverse_provider` 表加列（migration 004）：`kind TEXT NOT NULL DEFAULT 'openai'`（`openai` | `plugin`）、`plugin_provider_id TEXT NULL`。`kind='plugin'` 时 `base_url`/`model` 存空串，`has_key` 恒 true（不需要 key），`name` 默认为 provider id。
- DTO：`ReverseProvider` 与 `ReverseProviderInput` 追加 `kind: "openai" | "plugin"`、`plugin_provider_id: string | null`（`#[serde(default)]`，老客户端不传即 `openai`）。
- 新命令 `plugin_providers() -> string[]`：GET `{PLUGIN_PAGE_PATH}/providers`（JWT，无需 token）返回 provider id 列表；不可达 → `AppError network`。
- 批次运行（`reverse/batch.rs`）：`kind='plugin'` 的 provider 走 `POST {PLUGIN_PAGE_PATH}/lab/reverse`，body `{ "provider_id": string, "prompt": string, "image_base64": string, "mime": string, "max_tokens": int }`，头同 §8.3（JWT + `X-Library-Token`）。响应 `{ "text": string, "latency_ms": int, "provider_id": string }`；`text` 为模型原文，解析、清洗、指标全在 Mac 端照旧。图片走 `client::prepare_image` 同一套缩放（≤ 9 MB）。`request_json` 记录里 `model` 填 `plugin:<provider_id>`，`base_url` 填插件地址。取消令牌同样生效（丢弃响应）。
- 模型管理对话框分两栏："插件里的模型"（列出 `plugin_providers()`，每个一键"启用为反推模型"，已加入的显示启用开关）和"本地 OpenAI 兼容模型"（现有表单）。插件不可达时上半栏显示原因文本，不弹 toast。

### 10.2 插件端路由 `POST /page/lab/reverse`

- 校验 token（同 §8.3）；`provider_id` 必须在 `plugin.context.get_all_providers()` 里，否则 400；`image_base64` 解码后 ≤ 12 MB，否则 413。
- 图片写到 `lab_dir/tmp/<uuid>.<ext>`，调 `prov.text_chat(prompt=prompt, image_urls=[path])`，超时 120 s，完成后删除临时文件（异常也删）。返回 `completion_text` 原文；不做拒答判定、不做解析（那是 Mac 端的事）。
- 记一行 `logger.info`（provider、耗时、文本长度）；不入 runs 记录。

### 10.3 拖图进题库

- `QuestionBank.vue` 监听 Tauri webview 的 drag-drop 事件（`getCurrentWebview().onDragDropEvent`）：`over` 时题库面板高亮，`drop` 时把 `paths` 里扩展名为 png/jpg/jpeg/webp/gif 的文件逐个走现有 `addLocalImage(path)` 逻辑（重复路径忽略），其余忽略并 toast 提示"只接受图片文件"。`leave`/`cancel` 取消高亮。
- 拖放只在反推实验室页面生效；其它页面丢文件不做任何事。

## 11. v0.2.4：散文提示词、反推重画对照（用户 2026-09-07 拍板三件一起做）

### 11.1 散文判定（两端同一语义；Rust `artist::is_prose_prompt` / 插件 `lab_sync.is_prose_prompt`）

- `strip_artist_block(t)`：删除 `<artist>…</artist>`（大小写不敏感、跨行）。
- `is_prose_prompt(t)`：对 strip 后文本，满足任一即 true：含 `<artwork>`；含 "```"；存在独立成行的 XML 标签 `^\s*<[a-z_]+>\s*$`（`<style>`/`</style>` 除外）；按 `[,\n]` 切分后非空 token 的平均词数 > 5。
- `<style>…</style>` 的内容按标签处理（去掉标签本身后参与切分）。上游实测（v0.2.4 三端一致）：1898 条里 159 条散文（126 条 XML 式、33 条长句），1739 条标签；另有 79 条是「`<style>` 块 + 逗号标签」，按标签处理。此前写的 232/29 是把独立成行的 `<style>` 误算成 XML 的结果，作废。

### 11.2 导出与完整池

- 新增 `pack.prose: bool`、`FullEntry.prose: bool`、`pack_pool.entries[].prose`（仅 true 时写）。
- prose 条目：`pack.tags` = strip_artist_block 后原文（保留 `<style>` 块与 XML 块，去首尾空白；不切分、不清洗、不剔角色）；`t` = title + " · 散文"；不计 skipped；`artists`/`core` 仍按 `<artist>` 块解析。
- §9.6 的主标签词汇表只从非 prose 条目建。

### 11.3 插件端

- `lab_run`：随机候选排除 prose；`entry_id` 指定 prose 条目时 `tags_used`=原文、`dropped=[]`、`LabPlan.prose=true`；仍加角色与质量后缀。
- 新路由 `POST /page/lab/reverse/render`（token 同 §8.3）：

```
body = { tags: string, framing: string|null, character: string (缺省 "薇拉"), seed: int|null,
         negative_extra: string|null, judge: bool, original_base64: string|null, original_mime: string|null,
         judge_provider_id: string|null }
resp = { file, seed, prompt, negative, size, latency_ms,
         quality_score: number|null, quality_notes: string[], similarity_score: number|null, similarity_reason: string|null }
```

  - prompt = `character, person_tag(character), [framing], tags, defaults.prompt`；negative = `defaults.negative_prompt`（+ `negative_extra`）；**不注入画师**；`size_for_tags`；`seed` 缺省随机并回传；单张同步生成（`sem.at(PRIO_NORMAL)` + `lease_token`），文件 `ag.out_dir/lab_rerender_<ts>.png`；总超时 240 s。
  - `judge=true`：`quality_score/notes` = `ag._score_image(fp)`；给了 `original_base64` 再跑成对评委：provider = `judge_provider_id` → `reverse_draw.score_provider_ids[0]` → `auto_gallery.score_provider_ids[0]`，`text_chat(prompt, image_urls=[原图临时文件, 重画图])`，输出 `{"score": 0-10, "reason": "<20 字"}`。成对提示词要求：A 为原图、B 为按反推标签重画；**忽略角色外貌差异**（发色瞳色体型脸），只比较构图与镜头、姿势与手势、服装与配饰、道具、场景与背景、光线与氛围；给出 0–10 与一句理由；只输出 JSON。评委失败 → 对应字段 null，不影响出图结果。
- 桌面端取图用已有 `GET /page/thumb?name=&full=1`。

### 11.4 桌面端

- migration 005：`reverse_run` 加列 `truth_kind TEXT NOT NULL DEFAULT 'tags'`（`tags|prose|none`）、`render_file TEXT`、`render_local TEXT`、`render_seed INTEGER`、`render_prompt TEXT`、`quality_score REAL`、`quality_notes TEXT`（JSON 数组）、`similarity_score REAL`、`similarity_reason TEXT`、`render_error TEXT`。
- 指标：题目 truth 来源为 prose → `truth_kind='prose'`、`metric=null`，但 `appearance_leak`/`banned_leak` 仍按 `pred_raw` 计算并放进 `RunRow`（新增 `leak_appearance`/`leak_banned` 顶层字段，任何 truth_kind 都填）；本地图片题 `truth_kind='none'`。
- `RunBatchRequest` 加 `render: boolean`（缺省 false）、`render_character: string|null`、`render_seed: number|null`、`judge_provider_id: string|null`。流程：每个 run 解析成功后若 `render=true` → 调 11.3 路由（`tags` = cleaned 用 ", " 连接；`framing` = parsed.framing；原图 = 题目图片经 `client::prepare_image` 的 base64）→ 结果写入 run，图片经 `/page/thumb` 拉到 `labruns/reverse/<run_id>.jpg`；失败只写 `render_error`。
- 新命令 `reverse_render_run(run_id, judge_provider_id: string|null) -> RunRow`（单条重画，覆盖旧结果）。`RunRow` 新增 `truth_kind, render_path, render_seed, render_prompt, quality_score, quality_notes, similarity_score, similarity_reason, render_error, leak_appearance, leak_banned`。
- `BatchSummary` 的每模型行：P/R/F1 只聚合 `truth_kind='tags'` 且 metric 非空的 run，新增 `tags_n`（参与 F1 的题数）、`render_n`、`quality_avg`、`similarity_avg`。
- `Question` DTO 新增 `prose: bool`（后端按 truth 来源判定，与 `truth_kind` 同一规则）；migration 005 同时加 `leak_appearance INTEGER`、`leak_banned INTEGER`；`RunRow.render_path` 为绝对路径；prose 条目的 `pack.tags` = strip_artist_block(主 tags) 后接空行再接各 `character_prompts[].prompt` 原文（非空时）。
- 题库：prose 条目卡片标"散文"，新增筛选"只看标签题"；运行详情：原图与重画图并排、两个分数与理由、"重画"按钮；RunConfig：复选"反推后重画并评分（消耗点数）"、角色文本（缺省 薇拉）、固定种子、成对评委 provider 下拉（`plugin_providers()`，可空=插件默认）。

## 12. v0.3.0：画风索引与反推画风（用户 2026-09-07 提出）

### 12.0 裁定

#### v0.4.0 用户修订（覆盖本节此前的全库 / artist-only 串规则）

- 画风向量、均值、检索候选仅包含画风词典 `artist_nai5_personal`，覆盖加入/不加入/未定，黑名单继续用于剔除检索结果；社区图包不参与画风比较。旧图包向量保留但不使用，旧全库均值失效后按画风词典重算。图包原本的浏览/导出用途保留。
- 画风配方使用条目原始 `tags`，只去首尾空白：保留所有 artist、质量词、线条/配色词、年份、负权重、顺序与内部格式，不拼接固定质量后缀。`StyleHit` 新增必需的 `style_prompt: string`；复制、手动「用它重画」、精准反推全部使用此字段，`core` / `pack_tags` 仅为旧字段兼容。筛选画风池导出同样保留完整原串，去重按完整串而非仅按画师 signature，避免误并配色/线条不同的配方。完整池画师条目追加 `style_prompt`，社区条目该字段为 null。
- 模型目录与索引分别隔离：已有 `clip-vit-base-patch32` 保留，新接 `megastyle-siglip-so400m` / `csd-vit-large-patch14`；更换模型不清除其他模型权重或向量。模型 ID 必须来自后端注册表，拒绝任意路径/URL。模型各用原训练架构及预处理，输出 L2 向量，维度由注册表确定。
- `style_model_status(model?: string)`、`style_model_download(model?: string, on_event)`、`style_index_rebuild(model?: string, force, on_event)` 可指定模型，缺省为当前模型；后台任务开始时固定模型。新增 `style_models_list() -> StyleModelStatus[]` 和 `style_model_select(model: string) -> StyleModelStatus`，选择持久化 `app_state('style_model')`。下载/同步/索引期间拒绝切换；同一时刻一个模型下载 / 一个索引任务。
- `StyleModelStatus` 追加 `label: string`、`description: string`、`dim: number`、`download_size_bytes: number`、`default_center: boolean`。`StyleSearchRequest` 新增可选 `model: string`（缺省当前）；`StyleSearchResult.model` 标明实际模型。对比按相同原图和筛选条件依次查询已下载且已索引的模型，各组独立显示结果/错误/耗时，分数不做跨模型高低排名。
- 精准反推配置新增 `style_model: string|null`（null 采用启动批次时的当前模型，随后固定并记录），手动指定完整串重画仍走 `given`。原来的 `style_center` 仅表示「减库均值」，前端不再称作保证「去内容」。

- 反推画风 = **检索**，不是让视觉模型猜画师名：同步时给两本词典（画师词典 + 社区图包）**全部**条目的展示图各算一个向量，存库；反推时只给用户那张图算一个向量，和库里的比余弦相似度，取前 K。索引覆盖全量（含未定 / 不加入 / 黑名单命中），不是筛选后的池。
- 编码器 v1：CLIP ViT-B/32（openai/clip-vit-base-patch32，图像塔 + 投影，512 维，L2 归一）。用 candle（纯 Rust，v0.3.4 起支持 CPU / macOS Metal）推理，权重从 Hugging Face 下载到数据目录 `models/clip-vit-base-patch32/`（`config.json`、`pytorch_model.bin`，约 600 MB，一次性；v0.3.2 修正，官方 main 没有 `model.safetensors`）。模型名与维度随向量一起存，换模型 = 全量重算。
- "看风格不看内容"的 v1 手段：查询与库向量都减去**库内全部向量的均值**再算余弦（所有示例图共用同一角色模板，均值里装的是内容）。均值缓存在 `app_state('style_mean:<model>')`，索引变化时重算。
- 命中排序规则（与 §9.0 的画风池讨论一致）：黑名单命中的**永远**不返回；其余按相似度降序，每条带 `decision`，界面上"不加入"用灰色标出但不藏。图包条目也进索引，命中图包时给整条画师串（`pack` 的原文里含画师块）。

### 12.1 存储（migration 006）

```
CREATE TABLE image_embedding (
  entry_id TEXT NOT NULL, ordinal INTEGER NOT NULL, model TEXT NOT NULL, dim INTEGER NOT NULL,
  vec BLOB NOT NULL,            -- f32 小端，dim 个
  image_sha256 TEXT,            -- 算向量时的图片哈希；图片换了要重算
  computed_at TEXT NOT NULL,
  PRIMARY KEY (entry_id, ordinal, model)
);
```

- 只对 `ordinal = 0` 的展示图建索引（`entry_image.local_path` 非空者）。`removed_at` 非空的条目跳过。
- `app_state('style_model')` = 当前模型名（缺省 `clip-vit-base-patch32`）。

### 12.2 建索引

- 同步流程在图片阶段之后增加 `embedding` 阶段（`SyncPhase` 新增 `Embedding`；`SyncEvent` 新增 `embed_total` / `embed_done` / `embed_failed`）：对当前模型缺向量或 `image_sha256` 与 `entry_image.sha256` 不一致的展示图逐张计算；单张失败记 `app_event` warn 并继续；可用 `sync_cancel` 取消。模型未下载 → 整个阶段跳过，发一条 `message="画风模型未下载，跳过索引"` 的事件，不算错误。
- 新命令：
  - `style_model_status() -> StyleModelStatus { model, dir, present: bool, size_bytes, indexed: int, total: int, mean_ready: bool }`
  - `style_model_download(on_event: Channel<DownloadEvent>) -> StyleModelStatus`：从 `https://huggingface.co/openai/clip-vit-base-patch32/resolve/main/{config.json,pytorch_model.bin}` 流式下载到 `.part` 再改名；`DownloadEvent { file, done_bytes, total_bytes, finished: bool, error: string|null }`。可用 `style_model_download_cancel()` 取消。
  - v0.3.2 权重格式兼容：默认下载并通过 candle `VarBuilder::from_pth` 读取官方 `pytorch_model.bin`，无需 Python/PyTorch。已有非空 `model.safetensors` 仍能直接加载且不重复下载权重；两者都有时优先 `.bin`。状态要求非空 `config.json` 和至少一种非空权重文件，`.part` 不算完成。两种格式均读取同一套 `vision_model.*` / `visual_projection.weight`，模型名、512 维向量和数据库 schema 不变。
  - `style_index_rebuild(force: bool, on_event: Channel<SyncEvent>)`：不经同步单独建索引；`force=true` 删掉当前模型的全部向量重算。
  - `style_search(req: StyleSearchRequest) -> StyleSearchResult`：

```
StyleSearchRequest = { source: { kind: "entry", entry_id, ordinal } | { kind: "path", path }, top_k: number (1..50，缺省 10),
                       codex_ids: string[] (空 = 两本), decisions: ("include"|"exclude"|"undecided")[] (空 = 全部), center: boolean (缺省 true) }
StyleHit = { entry_id, codex_id, title, path: string[], decision, blacklisted: false, score: number (-1..1), core: string|null, pack_tags: string|null, thumb_path: string|null, prose: boolean }
StyleSearchResult = { model, indexed: int, query_ms: int, hits: StyleHit[] }
```

  - `core` = 画师条目按 §3.1 第 5 步的串（不带质量后缀）；图包条目 `pack_tags` = 导出用的 `pack.tags`（散文原文或清洗后标签）。模型未下载 / 索引为空 → `AppError bad_request`，message 说明去设置页下载或重建。

#### v0.3.3 索引时间估算（前端，无 DTO / 后端变更）

- 设置页手动建索引，以及同步页自动进入画风索引阶段，显示「已用时间」「预计剩余约…」「…张/分钟」。只计算本轮 `embed_total` 的工作量，续算不把库里已完成的条目重复计入；已处理数 = `embed_done + embed_failed`。
- 每轮单独计时：手动索引从点击启动开始，同步从首次进入 `embedding` 阶段开始，下载和前面的同步阶段不计入。首个完成批次作为速度采样基准，收到第二个有进展的批次后再显示 ETA，避免把模型初次加载耗时放大成整个任务的耗时。速度用最近最多 6 个批间区间的实际进展与耗时估计；ETA 随批次更新，已用时间每秒更新。
- 没有足够样本时显示「正在估算…」；长时间无进展（至少 60 秒且超过最近平均批时的 3 倍）隐藏过期 ETA，显示「等待下一批，正在重新估算…」。计数完成但终态未到时显示「正在收尾…」，避免显示虚假的剩余 0 秒。完成、取消、错误后停止计时并隐藏 ETA；下一次任务重新估算。计时状态随 store 在本次应用会话内保留，切换页面不重置。

#### v0.3.4 推理设备

- `Settings.style_device: "auto" | "metal" | "cpu"`，缺省 `auto`（旧设置自动补齐）。`settings_set` 可单独保存该项；同步 / 索引 / 模型下载正在运行时拒绝切换，返回 `bad_request`，其他设置仍可保存。非 macOS 构建拒绝显式 `metal`。
- macOS 构建启用 Candle Metal（本版 GPU 路径需 macOS 15+；旧系统或无 GPU 提前返回普通错误供自动回退）；`auto` 优先尝试 Metal，初始化或模型预热失败则退回 CPU 并显示原因；显式 `metal` 失败直接报错。`cpu` 始终使用 CPU。每轮索引固定持有同一编码器，模型缓存同时匹配目录和设备偏好；切换时释放缓存，下次加载生效。
- `StyleModelStatus` 追加 `device_preference: "auto" | "metal" | "cpu"`、`active_device: "cpu" | "metal" | null`（当前偏好尚未加载时为 null）、`metal_available: bool`（当前构建是否含 Metal 支持，不保证驱动初始化成功）、`device_message: string | null`（自动回退原因或平台不支持的说明）。前端显示实际加载设备，并在首批计算后刷新状态。
- 设备选择同时用于建索引与查询图编码。保持同一模型、F32、预处理与向量格式；CPU / Metal 仅有浮点误差，切换不清空既有索引，重建继续缺失 / 变更图片。GPU 加速的是本地编码，模型下载与服务器出图不受此选项影响。

### 12.3 预处理与推理

- 图片解码后短边缩放到 224、中心裁 224×224、RGB、CLIP 的 mean/std 归一；取图像塔 pooled 输出经投影得 512 维，L2 归一。批量 16 张一批。在 `tokio::task::spawn_blocking` 里跑，不阻塞异步运行时。
- 单元测试用随机权重（`VarBuilder::zeros`）实例化模型验证形状与流水线，不下载权重；设了 `CLIP_MODEL_DIR` 环境变量时额外跑真权重的最近邻健全性测试（同一张图与自身相似度 > 0.99）。

### 12.4 完整池导出（补 §8.1 预留）

- 索引存在时 `embedding = { "model": string, "dim": 512, "centered": false }`，每条 `FullEntry` 追加 `vec`：`base64(float16 小端 × dim)` 的**原始**向量（未减均值；均值另给 `embedding.mean`，同样 base64 float16）；没有向量的条目 `vec: null`。插件端照旧不透明存储。

### 12.5 反推实验室

- 新面板"画风反推"（`components/reverse/StylePanel.vue`）：来源 = 题库当前选中的一道题，或题库里的本地图片；参数 top_k、词典范围、决策范围、是否去内容；按钮"反推画风"。结果列表：缩略图、标题与目录、相似度条、决策徽章、画师串（一键复制）。
- 与重画打通：每条命中有"用它重画"按钮 → 调 `reverse_render_run` 的扩展：`RunBatchRequest`/`reverse_render_run` 增加 `artist: string|null`，插件 `/page/lab/reverse/render` 增加 `artist: string|null`（非空则作为 `Req.artist`，仍不注入画风池）；运行详情记录 `render_artist`（migration 006 给 `reverse_run` 加 `render_artist TEXT`）。这样"反推标签 + 检索到的画风"重画出的图和原图对照，就是画风检索准不准的直接证据。
- 设置页"画风模型"卡片：模型名、目录、是否已下载、大小、已索引 / 应索引条数、"下载模型" / "重建索引" / "取消" 按钮与进度条。

### 12.6 重画必须与插件 `/反推` 逐条对齐（用户 2026-09-07 指出：实验室重画固定按标签选尺寸，与线上不一致，实验就失去意义）

对照表（线上 = `reverse_draw.handle_reverse`）：

| 项 | 线上 `/反推` | 重画路由（v0.3.0 起） |
|---|---|---|
| 尺寸 | `_size_for_image(原图)`：宽高比 ≥1.15 横 `landscape_size`、≤0.87 竖 `portrait_size`、否则方 `square_size` | **同一函数**：`_size_for_image` 拆出 `size_for_ratio(plugin, w, h)`，路由按 `original_width/original_height` 调用；两者缺失才退到 `size_for_tags` 并在响应 `size_source="tags"` 标明 |
| 画师串 | `风格=` 指定，否则按 `pool.n5_ratio_reverse` 概率从画风池 `_n5.pick(plugin,"reverse")` 抽，未命中为空 | `artist_mode: "pool" \| "given" \| "none"`，缺省 `"pool"`（同一函数同一概率）；`"given"` 用 `artist`；`"none"` 纯标签对照。响应回传 `artist`、`artist_source` |
| 张数与择优 | `reverse_draw.candidates`（默认 2，上限 4）张，三评委各打分，只发最高分那张 | `count: int|null`（null = 插件配置同一默认，上限 4）；全部出图、全部评分，响应 `images: [{file, seed, quality_score, quality_notes}]` + `best_idx`；顶层 `file/seed/quality_*` = 最优那张（兼容旧字段）；成对评委只对最优那张跑 |
| framing | 不在五选一里 → `cowboy shot` | 同样回退 |
| 标签清洗 | `_clean_tags(raw, character, framing)`，清洗后 < 5 条报错 | 提示词用 Mac 传来的 `tags`（规则集是实验对象，不能被线上规则覆盖），但路由再跑一遍 `_clean_tags` 并回传 `plugin_dropped: string[]`（线上还会多剔的 token）；线上口径下 < 5 条时**不报错**，回传 `plugin_would_reject: true` |
| 拼装 | `character, person_tag, framing, tags…, quality` | 相同 |
| 负面 / steps / scale / sampler / noise_schedule | `defaults.*` | 相同（`negative_extra` 仍可追加） |
| 模型 | `quota.v5_gate` 决定 | `defaults.model`（实验室不走配额；差异保留并注明） |
| seed | 随机 | 可固定（实验需要）；`count>1` 时每张 seed+i |

- 桌面端：`client::prepare_image` 额外返回**缩放前**的原图宽高；重画请求体新增 `original_width`、`original_height`、`artist_mode`、`count`；`RunBatchRequest` 新增 `render_artist_mode`（缺省 `"pool"`）与 `render_count`（null = 插件默认）；`RunRow` 新增 `render_artist_source`、`render_size`、`render_size_source`、`plugin_dropped`（JSON 数组）、`plugin_would_reject`、`render_candidates`（JSON：全部候选的 file/seed/score）；migration 006 一并加列。运行详情显示尺寸来源、画师来源、线上会多剔的词、候选缩略图与分数。
- 画风反推面板的"用它重画" = `artist_mode="given"` + `artist=命中串`。

### 12.7 精准反推（v0.3.1，用户 2026-09-07 提出）

- 定义：反推标签之外，**自动**用画风索引在两本词典全量里检索原图最像的一条，把它的画师串加进重画，于是一次同时反推"画什么"和"谁的画风"。它是 §12.6 `artist_mode` 的第四个取值 `"style"`，桌面端把它翻译成对插件的 `artist_mode="given"` + `artist=<命中串>`；插件不改。
- `RunBatchRequest` / `reverse_render_run` 新增：`style_top_k: number`（缺省 5，检索候选数，取第一条可用的）、`style_decisions: ("include"|"exclude"|"undecided")[]`（空 = 全部；黑名单永不用）、`style_min_score: number|null`（相似度低于它时按 `style_fallback` 处理）、`style_fallback: "pool"|"none"`（缺省 `"pool"`）、`style_center: boolean`（缺省 true）。
- 流程（`reverse/render.rs`）：run 解析成功 → `style::search`（source = 题目图片：词典题用 entry/ordinal，本地题用 path）→ 过滤后取第一条 → 画师串 = 画师条目 `core`，图包条目 `pack_tags`（散文图包只取其 `<artist>` 块解析出的 `core`，没有则跳过该命中）→ 若无命中或 `score < style_min_score` → fallback → 调重画。
- 记录：migration 007 给 `reverse_run` 加 `render_style_entry_id TEXT`、`render_style_title TEXT`、`render_style_score REAL`、`render_style_decision TEXT`、`render_style_fallback INTEGER`；`RunRow` 同名字段；`render_artist_source` 取值新增 `"style"`。索引未建或模型未下载 → 批次开始前就报 `bad_request`（不消耗点数）。
- 界面：运行配置"画师来源"下拉新增"精准反推（画风检索）"，展开时显示 top_k、决策范围、最低相似度、兜底方式；结果表每模型行新增 `style_hit_n`（用到检索画风的张数）与 `style_score_avg`；运行详情显示命中条目（缩略图、标题、相似度、决策）。
- 意义：成对评委的"像原图"分在 `style` 模式与 `pool` 模式之间的差，就是画风检索对最终效果的贡献。

## 5. 视图与库的定义（SQL 见 migrations）

- `artist_library`：`codex_id IN ('artist_nai5_personal','local_artist') AND decision='include' AND removed_at IS NULL`
- `pack_library`：`codex_id IN ('nai5_community_pack','local_pack') AND decision='include' AND removed_at IS NULL`
- 三态循环：`undecided → include → exclude → undecided`（前端实现，调用 `set_decision`）。


### 12.8 画师标签识别对照（v0.4.0）

独立于图片向量检索，Kaloscope 2.0 (`kaloscope-2.0`) 与 Camie Tagger v2 (`camie-tagger-v2`) 不建立图库索引。`style_classifiers_list()` 返回 `{ model, label, description, download_size_bytes, present, dir, runtime_ready }[]`；`style_classifier_download(model, on_event)` 顺序准备模型、标签映射与 ONNX Runtime，复用下载取消命令。`style_classify(req: { source: StyleSearchSource, model, top_k })` 返回 `{ model, label, query_ms, hits: [{ artist, score }], device? }`，top_k 限制为 1..50。得分仅为该分类器输出的类别概率，不能换算为 NovelAI 权重，也不作为原画作者认定。

分类推理在本机 CPU / ONNX Runtime 执行，自动下载对应平台的运行库，用户不需要配置 Python。三个向量编码器仍按 Auto / Metal / CPU 设置运行。模型之间顺序计算，图片、决策过滤条件冻结为本次请求；不同模型分开显示，不按原始分数混排。模型文件固定上游版本，大权重校验 SHA256；校验通过前不作为已下载文件。

筛选池插件导入 `/page/library/import` 的 `nai5_artist_pool` 接受非空字符串数组（含完整风格配方，可无 `artist:` 标签），不修改内部词条或权重；其余鉴权、黑名单与原子落盘规则沿用。

每次重画实际参数另记 `reverse_run.request_json.last_render_request`（包括本次冻结的 `style_model`，失败尝试也记录），原 `request_json.render` 保留批次开场参数。重画完成事件包含 `style_model`，不再用开场模型解释后来换模型的结果。

### 12.9 反推批次停止运行（v0.4.1）

- `reverse_cancel_batch(batch_id: string) -> ()` 请求停止当前正在执行且 ID 相同的反推批次；重复请求、已结束或其他 ID 均幂等返回，不取消其他批次。`reverse_run_batch` 从启动准备开始只允许一个活动批次，重叠启动返回 `batch_running`；失败或结束后释放活动状态。
- 停止后取消客户端待执行 / 等待中的反推与重画工作，不再继续提交后续题目或重画请求；已落库的结果保留。数据库批次终态为 `cancelled`，推送 `batch_cancelled` 事件，`done/total` 保留实际进度。计数表示已处理的反推结果，不表示所有关联重画均已完成；最后一题的重画期间停止也可能是 N/N。中断不能伪装成全部成功或全部失败，也不为尚未完成的模型请求制造失败结果。
- 已经发送到插件 / 模型服务的请求可能仍在远端完成并产生消耗；本次不新增插件端强制中断协议。停止按钮负责终止桌面端批次调度，不能承诺撤回远端已接收的工作。
- 反推实验室运行区常驻「停止运行」按钮，启动未取得 ID 时显示启动状态，请求停止后显示「停止中…」并防重复操作，终态到达后才允许新批次。前端活动批次 ID 与当前查看的历史批次 ID 分开；查看历史不改变停止目标，通道事件或轮询结束均恢复按钮并显示真实的已停止进度。

## 28. NAX V5 画师来源（2026-10-03，用户授权）

- 画师词典增加法典与 NAX 来源切换，NAX 使用独立书目 `nax_v5_artist`，复用现有 IPC、筛选、三态决策、预览和复制。各来源分别持久化筛选；切换来源立即清空旧卡片，读取失败不得对旧来源误操作。
- 每次 `sync_start` 都检查 NAX `danbooru-artist-tags-2-v5` 的完整公开编号页；法典 `latest_release` 未变也不能跳过。验证 V5 Full 设置、HTML 完整性、画师标签和 CDN 单段文件名，抓取结束重查首页。分页边界可去重同标签同图片的重复项；冲突、截断和失败不发布新目录。
- NAX ID 为 book ID 前缀加精确画师标签 SHA256，目录哈希不受投票影响；快照 `nax-v5-<hash前16位>` 存入既有 `codex_release`，不改法典 `app_info.latest_release`。`Book.update_filters` 增加本来源 `{id,label,latest:true}`，本次新增依 `first_seen_release` 查询；当前快照标记写入条目 `update_batches`。同步不改决策、备注或审计；NAX 缺失项一律保留，避免非原子分页误下架。
- 任一来源失败保留已存数据并继续另一来源和图片补缺；结束为 partial，复用 `SyncFailure` 保存来源 URL、原因和 code，页面显示实际部分完成原因，不能将它冒称全部最新。取消仍中止整轮任务。
- 图片复用既有续传/限速/取消/校验队列，NAX 特定 URL 分支指向真实 gallery slug；展示图与缩略图本地文件名用 CDN 文件名 SHA256，防文件名超长。按需下载 NAX 不依赖法典 media.json；只下载公开 WebP，不猜原图。来源署名、URL、生成设置保留在 note，tags 只存画师串。
- 迁移 008 仅重建 artist_library 视图，纳入 NAX；所有旧用户数据保留。NAX 参与画师解析、已选画师导出、完整池独立来源计数和本地画风索引/检索；均值缓存改 `artist-sources-v2`，既有向量复用。`library_export.entries` 增加 `codex_id` 标明来源。图包范围不纳入 NAX。
- 没有新增收费模型调用、真实出图或用户库上传；验证使用合成库/HTML、浏览器 mock 和公开来源目录。使用与限制见 docs/NAX_V5_SYNC.md。
