These are synthetic responses produced by the plugin's studio-lab test script
through the actual studio planner, run executor, image and comparison endpoints.
The tests use fake providers, synthetic references and an 8×8 generated PNG;
they never read a real library or call a model or NovelAI service.

The template text and repeated `llm_raw` strings were replaced with short synthetic
text. Protocol keys, nesting, per-image plan IDs, request IDs, empty/null fields,
reference IDs, and generation parameters are preserved, except one per-image
key that the studio validator ignores, which was dropped.

The Rust test `studio::tests::actual_plugin_wire_fixtures` reads these by default.
To validate freshly generated responses against both implementations:

```sh
# in the plugin checkout: run its studio-lab test script, writing to /tmp/studio-wire
# in this repository
cd src-tauri
STUDIO_FIXTURE_DIR=/tmp/studio-wire cargo test --offline --locked --lib studio::tests::actual_plugin_wire_fixtures
```

Do not replace these with live data, user images or exported codex material.
