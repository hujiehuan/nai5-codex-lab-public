# Third-party notices

The application code in this repository, including the built-in prompting guide in `resources/prompting-guide/`, is
MIT-licensed (see `LICENSE`). The table below lists third-party code that the repository contains and material that the
app downloads at runtime. Rust crates and npm packages are resolved through `Cargo.lock` and `package-lock.json` and
keep their own licenses.

| Component | License | How it is used |
|---|---|---|
| [Langbai NovelAI Studio](https://github.com/2786886095/novelai-image-desktop) (`src/artist-recipe.ts`, `src/v5-artist-weight-repair.ts`) | MIT | **Ported.** `src/lib/artist-recipe.ts` and `src-tauri/src/artist/*.rs` port its prompt-recipe parser and serializer. Full license text below. |
| [OpenAI CLIP ViT-B/32](https://huggingface.co/openai/clip-vit-base-patch32) | MIT ([upstream repository](https://github.com/openai/CLIP)) | **Runtime download** of `config.json` and `pytorch_model.bin` at a pinned revision. |
| [Google SigLIP so400m](https://huggingface.co/google/siglip-so400m-patch14-384) | Apache-2.0 | **Runtime download** of `config.json` and `preprocessor_config.json` only, used to build the MegaStyle image tower. |
| [MegaStyle encoder](https://huggingface.co/Gaojunyao/MegaStyle) | See the upstream model card | **Runtime download** of `megastyle_encoder.pth` at a pinned revision. Preprocessing follows [Tencent/MegaStyle](https://github.com/Tencent/MegaStyle). |
| [CSD ViT-L](https://huggingface.co/tomg-group-umd/CSD-ViT-L) | Weights CC-BY-4.0; [model code](https://github.com/learn2phoenix/CSD) MIT | **Runtime download** of `pytorch_model.bin` at a pinned revision. The Rust encoder follows the published model code. |

Nothing in the runtime-download rows is committed to the repository or bundled in the `.app`. Each file is fetched
only when the user asks for it, from the pinned revision recorded in `src-tauri/src/style/catalog.rs`, and stays under
its own license. Weight files are checked against the size and SHA-256 recorded there before they count as downloaded.

## Upstream data (not bundled)

The catalogue data and images synced at runtime come from 法典图鉴 (https://novelai.quicktagcloud.com, frontend
https://github.com/AgIzT/NovelAI-Tag). Entry texts belong to their original compilers; the compiled structure is
© AgIzT and may not be redistributed.

This application only syncs that data to the user's own machine. The repository contains none of it; all test
fixtures are synthetic.

## Langbai NovelAI Studio license

```
MIT License

Copyright (c) 2026 浪白

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```
