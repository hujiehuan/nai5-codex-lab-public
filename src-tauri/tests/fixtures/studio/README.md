These are synthetic responses produced by the plugin's `scripts/test_studio_lab.py`
through the actual studio planner, run executor, image and comparison endpoints.
The tests use fake providers, synthetic references and an 8×8 generated PNG;
they never read a real library or call a model or NovelAI service.

The template text and repeated `llm_raw` strings were replaced with short synthetic
text. Protocol keys, nesting, per-image plan IDs, request IDs, empty/null fields,
reference IDs, and generation parameters are preserved.

The Rust test `studio::tests::actual_plugin_wire_fixtures` reads these by default.
To validate freshly generated responses against both implementations:

```sh
# in the plugin checkout
python3 scripts/test_studio_lab.py
# in this repository
cd src-tauri
STUDIO_FIXTURE_DIR=/tmp/studio-wire cargo test --offline --locked --lib studio::tests::actual_plugin_wire_fixtures
```

Do not replace these with live account data, user images or exported codex material.
