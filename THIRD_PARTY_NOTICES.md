# Third-party notices

The application code in this repository, including the built-in prompting guide in `resources/prompting-guide/`, is
MIT-licensed (see `LICENSE`). The items below are third-party code the repository contains or material the app
downloads at runtime.

## Ported code

### Langbai NovelAI Studio (MIT)

`src/lib/artist-recipe.ts` and `src-tauri/src/artist/*.rs` are ports of the artist-string parser/serializer from
Langbai NovelAI Studio (https://github.com/2786886095/novelai-image-desktop, `src/artist-recipe.ts`,
`src/v5-artist-weight-repair.ts`), released under the MIT License, Copyright (c) 2026 浪白.

## Downloaded at runtime (not in this repository)

Nothing below is committed to the repository or bundled in the `.app`. Each item is fetched only when the user asks
for it, from a pinned revision, and stays under its own license; see the linked source for terms.

- **ONNX Runtime 1.22.0** (MIT): https://github.com/microsoft/onnxruntime, loaded dynamically for the ONNX classifiers.
- **Style encoders** (`src-tauri/src/style/catalog.rs`):
  - OpenAI CLIP ViT-B/32: https://huggingface.co/openai/clip-vit-base-patch32
  - Google SigLIP so400m: https://huggingface.co/google/siglip-so400m-patch14-384
  - MegaStyle encoder: https://huggingface.co/Gaojunyao/MegaStyle (preprocessing follows https://github.com/Tencent/MegaStyle)
  - CSD ViT-L: https://huggingface.co/tomg-group-umd/CSD-ViT-L (model code follows https://github.com/learn2phoenix/CSD)
- **ONNX classifiers** (`src-tauri/src/style/classifier.rs`):
  - Kaloscope: https://huggingface.co/DraconicDragon/Kaloscope-onnx
  - camie-tagger-v2: https://huggingface.co/Camais03/camie-tagger-v2
- **NPE skill text** (optional, user-triggered import): https://github.com/RefirmSaky/novelai-prompt-design-enhancer
  at revision `58f98acfbc9b7556258a294183ef46057e00afb1`.

## Upstream data (not bundled)

The catalogue data and images synced at runtime come from:

- 法典图鉴 (https://novelai.quicktagcloud.com, frontend https://github.com/AgIzT/NovelAI-Tag). Entry texts belong to
  their original compilers; the compiled structure is © AgIzT and may not be redistributed.
- NAX V5 artist gallery (https://nax.moe), maintained by Zelest.

This application only syncs that data to the user's own machine. The repository contains none of it; all test
fixtures are synthetic.
