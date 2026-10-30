# 本地画风比较：模型选择与验收口径

核查日期：2026-09-19。实现契约见 `CONTRACT.md` §16。

这里分开记三类东西：已经接入的实现、上游作者报告的结果、待验证的候选。论文结果不能当成 NAI5 图片上的准确率。

## 视觉模型排序

总实验室里，视觉 LLM 做的是批内检索：图 1 是基准图，模型一次看最多 7 张候选图，返回画风最接近的前三名、0–100 的批内解释分和理由。这个分数只用来说明同一模型在这一轮为什么这样排，不是校准过的概率，也不能跨模型或跨批次比较。指令里排除了题材、角色、姿势、构图和背景内容，但还是要用“同风格不同内容”和“不同风格相同内容”的样本，检查模型有没有受内容干扰。

CLIP、CSD、MegaStyle 等本地编码器则计算全部无序图对，用人工组标签评估阈值、准确率、精确率和召回率。两类结果回答的问题不同，所以界面不会把视觉 LLM 的前三名自评分混进本地编码指标。

## 已接入的三个模型

| 模型 | 本应用用途 | 原生输入处理 | 本应用输出 |
| --- | --- | --- | --- |
| CLIP ViT-B/32 | 通用图像向量基线，检验内容相似是否干扰画风判断 | 短边双线性缩放、中心裁剪 224×224 | 512 维、L2 后余弦 |
| CSD ViT-L/14 | 专门训练的画风描述向量 | 短边 bicubic 缩放、中心裁剪 224×224 | style projection 的 768 维、L2 后余弦 |
| MegaStyle-Encoder | 面向画风相似度的 SigLIP 编码器 | 整图 bicubic 缩放 384×384 | attention pooling 的 1152 维、L2 后余弦 |

表里的预处理是现有 Rust 实现的做法，沿用 `style/model.rs`、`style/csd.rs`、`style/megastyle.rs`；做比较功能时没有改旧的索引算法。传进去的是本地原文件，不是 UI 缩略图。但“读原图”之后仍会按模型要求缩放/裁剪，不是每个模型都保留了边缘信息。

CLIP 官方实现同时有图像和文本编码器，训练目标是图文匹配。这里只复用图像塔当通用基线，所以相似的人物、服装、场景也可能拉高分数。这是选它做基线的理由，不是对某张图片的判断。[CLIP 官方仓库](https://github.com/openai/CLIP)

CSD 作者的目标就是图像画风描述与检索。上游仓库至今仍注明发布的权重和论文数字有差异，所以论文里的准确率不能照搬。仓库代码是 MIT，作者链接的权重卡是 CC-BY-4.0，代码和权重的许可要分别保留。[作者仓库](https://github.com/learn2phoenix/CSD)、[作者权重卡](https://huggingface.co/tomg-group-umd/CSD-ViT-L)

MegaStyle 官方有两条路径：独立的 Encoder 画风评分，和另一个用于风格迁移的 FLUX。本应用只用 Encoder。官方在 2026-09-02 更新了 MegaStyle++ 的论文与数据集公告；本应用没有因此切换新权重，这条公告也不是自动替换现有模型的理由。[MegaStyle 官方仓库](https://github.com/Tencent/MegaStyle)

下载版本和文件大小固定在 `src-tauri/src/style/catalog.rs`，以那里为准：

| ID | 权重固定 revision | 注册权重字节数 |
| --- | --- | ---: |
| `clip-vit-base-patch32` | `3d74acf9a28c67741b2f4f2ea7635f0aaf6f0268` | 605,247,071 |
| `csd-vit-large-patch14` | `5bc26a6fb0487f3f00a2a7313135103a005b1b67` | 2,438,228,893 |
| `megastyle-siglip-so400m` | `8ac053fb78602495e3a932789ccc6f5edbed943e` | 856,600,406 |

这些字节数不含配置文件，也不等于运行内存。`model_version` 里的 `catalog:` 表示预期的注册版本；`local-unverified` 表示这次比较没有重新扫描大权重算 SHA256。手动放进来的 CLIP safetensors 单独标为本地未验证，注册 revision 不能当作本地文件的身份。

## 实现一致性（parity）

三个编码器都是按原训练架构和预处理在 candle 里重写的，所以要单独验证「Rust 实现与官方参考实现给出同一向量」。这一节只讲实现一致性和能不能跑，不回答哪个模型在 NAI5 图片上效果最好。

| 模型 | 参考实现 | 维度 | 预处理要点 |
| --- | --- | ---: | --- |
| CSD ViT-L/14 | 作者官方 PyTorch 图像塔 + `last_layer_style` 投影 | 768 | 短边 bicubic 224、中心裁剪、CLIP mean/std |
| MegaStyle-Encoder | HF Transformers `SiglipVisionModel.pooler_output`（MegaStyle 微调权重） | 1152 | 整图 bicubic 384×384、归一到 [-1, 1] |
| CLIP ViT-B/32 | `openai/clip-vit-base-patch32` 图像塔 + `visual_projection` | 512 | 短边 224、中心裁剪；随机权重测形状，真权重测自相似 |

2026-09-07 在 Linux CPU 上按上表的固定 revision 下载了全部权重并校验 SHA256，用同一张合成渐变图对拍：CSD 与 MegaStyle 的余弦都约为 1.0，最大逐维差分别是 4.48e-7 和 2.00e-7。

回归测试 `style::csd::tests::csd_matches_official_pytorch_reference` 和 `style::megastyle::tests::megastyle_matches_hf_reference` 把官方参考输出的前八维写进代码（容差 2e-5），同时要求同一张图两次编码的余弦 > 0.9999，用来防止投影层、张量布局或预处理回退。这两个测试需要真实权重，默认 `#[ignore]`；没有权重时显示为 ignored，不会静默通过。运行方式：

```sh
cd src-tauri
CSD_MODEL_DIR=/path/to/csd-vit-large-patch14 MEGASTYLE_MODEL_DIR=/path/to/megastyle-siglip-so400m \
  cargo test --lib -- --ignored matches_
```

模型目录、向量和均值按模型 ID 分开存放，切换模型不会混用索引或均值。每个模型都要单独补齐一次自己的索引，CLIP 向量不能拿给 CSD 或 MegaStyle 用。

CPU 和 Metal 用同一模型、F32 和同样的预处理，差异只在浮点误差级别；上面的数值以 CPU 结果为准。

## 待验证的其他候选

下表只是研究记录。这些模型没有接入下载器或比较命令，界面上不把它们列为可运行选项。

| 候选 | 一手资料确认的机制 | 对本项目的判断与下一步 |
| --- | --- | --- |
| IntroStyle | 利用扩散模型中间特征的通道均值/方差，以 2-Wasserstein 距离比较画风；无需为画风重新训练 | 和现有的余弦编码器不是一类方法，值得单独实验。它的距离不能直接塞进 `cosine` 字段。下一步先固定扩散模型、层、时间步与随机性，测 Mac 上的运行成本。 |
| Anime Images Style Embedder v4 | 作者用 DINOv3 的 CLS 特征加 MLP 生成动漫画风向量，模型卡注明需要获取 DINOv3 权限 | 和动漫插画的目标接近，优先做小规模配对测试。先核查完整权重、许可和离线推理；输出维度小不代表更准确。 |
| Anima Style Embedding v2 | 作者报告面向动漫/插画、1–8 张参考、1024 维向量；组合 VAE、DINOv3 与 Anima 描述符 | 可能适合多参考研究，但依赖链较长，模型卡是非商业许可。先确认外部特征是否齐全，再考虑本地接入。 |
| anime-style-classifier-v5 | EfficientNet-B2，四类动画制作风格，提供 1408 维倒数第二层特征 | 可以当粗粒度的风格对照；四分类结果证明不了它能分辨 NAI5 风格配方之间的细小差异。 |

来源：[IntroStyle 作者项目页](https://anandk27.github.io/IntroStyle/)、[作者代码](https://github.com/AnandK27/introstyle)、[Anime Images Style Embedder 作者模型卡](https://huggingface.co/Fgdfgfthgr/Anime_Images_Style_Embedder)、[Anima 作者研究报告](https://huggingface.co/Baragi-AI/Anima-Style-Embedding/blob/main/RESEARCH_REPORT.md)、[anime-style-classifier-v5 作者模型卡](https://huggingface.co/Mitchins/anime-style-classifier-v5)。这些作者报告的效果还没有在本应用里复现。

WD Tagger 这类动漫标签模型适合当内容识别的对照。它的官方输出是分级、角色和通用标签，标签分类准确率不能直接当成画风相似度准确率。[WD EVA02-Large 官方模型卡](https://huggingface.co/SmilingWolf/wd-eva02-large-tagger-v3)

另一个方向是不急着换编码器，先研究 CSD 的分数怎么读。2026 年有一篇预印本专门报告了把原始 CSD 余弦当作绝对画风分数时的失效情况，并研究了 CSLS 等读法。它提醒我要在自己的语料上做校准，但不是已经验证过的二次元方案。这个比较功能仍按契约用原始余弦，没有加 CSLS、位置插值或均值处理。[论文 v2](https://arxiv.org/abs/2605.09030v2)

## 这个实验回答什么

它回答的是“这两张图在人工定义的画风上是否接近”。它证明不了两张图用了同一个 NovelAI 风格配方，也识别不了真实作者。同一配方换了内容，外观可能不同；不同配方也可能画出相近的外观。如果按生成配方分组，标签就记成“同配方”，不要叫“同画风真值”。

测试素材放在本机、不进仓库，至少准备四种配对：

1. 同风格、不同内容：换角色、动作、场景，检查内容变化会不会导致漏判。
2. 不同风格、相同内容：固定角色和构图，换线条、上色与材质，检查内容会不会导致误判。
3. 容易判断的正例和负例：用来检查路径、顺序和分数方向。
4. 含糊的边界例：保留“未标注”，不要硬塞进阴性；单独人工看。

重复文件或只做了轻微 JPEG 压缩的版本适合做数值健全性检查，但不要大量拿来当画风正例，否则高分可能只说明模型认出了同一幅画。角色、裁剪范围、清晰度、背景在画面中的占比都要记下来，方便解释模型之间的分歧。

## 阈值、指标与数据划分

默认阈值 `0.8` 只是界面上的起点，还没有校准；`cosine >= threshold` 才算这次实验的阳性预测。`0.8` 不等于“80% 同画风概率”。阈值每个模型单独设，不同模型的原始分数不直接拿来排名。

两端的人工组标签都非空时，同组算正例、异组算负例；任一端未标注就不参与准确率。组标签隐含了组内风格一致的假设，表达不了的含糊图应该留空。

每一对只计一次，不算自配对。记录 TP、FP、TN、FN 和有效配对数；accuracy = `(TP+TN)/N`，precision = `TP/(TP+FP)`，recall = `TP/(TP+FN)`。分母为零时显示未知，不显示 0% 或 100%。

缺权重、坏图、推理失败都不能当阴性或零分参加统计。当前界面按每个模型各自的有效判断分别计算指标，并显示覆盖量，不会自动选出赢家。人工比较模型时只取共同成功的配对，免得失败多的模型因为只剩简单题而显得更好。

一批 N 张图最多得到 `N*(N-1)/2` 个配对，但这些配对共享图片，不能当成同样数量的独立样本。调阈值和最后报告效果要用不同的图片/风格组；不要把同一张图片参与的配对随机拆到训练和验证两边。

如果阈值是在当前这批数据上调的，显示的值就叫“当前样本结果”；在留出的一批新图上复验之后，才能讨论泛化表现。样本太少时，混淆矩阵和错例比一个漂亮的总分更有用。

这些是本项目自己的实验设计，不是引用模型卡里的性能承诺。实际做法可以先选一个模型当主要候选，再用同一批图和另外两个模型做盲评，把模型判断不一致的具体图片对留下来人工复看。

## 本地运行与复现

`style_compare` 接收 2–32 张本地图片和 1–3 个固定模型 ID。路径必须是绝对路径，规范化后互不相同；指向同一文件的符号链接也算重复。每张最多 32 MiB，合计 128 MiB；每张最多 33,554,432 像素，长边 16,384，长宽比最多 64:1；解码器另有 256 MiB 内存预算。超限直接报错，不会悄悄缩图。整个请求先冻结图片字节、尺寸和 SHA256，之后所有模型都复用这份快照。

推理沿用设置里的 Auto / Metal / CPU 偏好，结果里记下实际设备和回退原因。模型逐个加载，每批同时受注册的 batch_size 和像素预算约束。比较过程不读写向量索引，不改当前画风模型，也不碰筛选决策。缺权重的模型单独报错，要先去设置页下载。

结果里的计时字段：

- `load_ms`：包含取得/加载模型的时间，缓存命中时很短。
- `encode_ms`：包含冻结图片的重新解码、编码和配对计算。
- `total_ms`：该模型这次的处理时间。
- 顶层 `elapsed_ms` 另外还包括输入冻结和等待共享推理锁的时间。

比较性能时，冷启动和热缓存分开记，下载时间不算进编码速度。

`style_compare_cancel()` 请求停止，可以重复调用（幂等）。取消之后，本次比较仍占着比较槽，直到当前这次阻塞推理结束；不会去强杀 Candle 工作线程。比较槽被占用时再发起新的比较，返回 `batch_running`。取消不影响模型下载任务，也不会撤销其他实验室的任务。

离线测试（不下载真实权重、不调用任何模型 API）：

```sh
cd src-tauri
cargo test --offline --locked --lib style::compare::tests -- --test-threads=1
cargo test --offline --locked --lib commands::style_compare::tests -- --test-threads=1
```

测试用合成 PNG、假向量和小型随机 CLIP，覆盖输入/路径边界、字节冻结、所有无序配对、非法向量、取消、缺权重及混合成功/失败。它们只验证管线是否正确，证明不了真实模型的画风准确率；Linux 上的 CPU 测试也代替不了 Mac 上的 Metal 实测。
