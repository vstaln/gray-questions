<p align="center">
  <img src="assets/gray-logo.svg" alt="gray" width="96">
</p>
<h1 align="center">gray-questions</h1>
<p align="center">Structured clarifying questions for interactive agent sessions.</p>
<p align="center">
  <a href="https://github.com/vstaln/gray-questions/blob/main/LICENSE"><img alt="MIT License" src="https://img.shields.io/badge/license-MIT-blue.svg"></a>
  <img alt="gray plugin" src="https://img.shields.io/badge/gray-plugin-7aa2f7.svg">
  <img alt="rust" src="https://img.shields.io/badge/built%20with-rust-orange.svg">
</p>

A self-contained Rust sidecar that gives the agent a structured
`request_user_input` tool for asking you 1–3 multiple-choice questions.

## What it does

- Exposes `request_user_input` with question text, selectable options, and
  free-form notes.
- Validates input in the plugin: 1–3 questions, non-empty options, and an
  automatic "None of the above" path.
- Delegates asking to the host with `host/ask` and waits up to 300 seconds
  for the `{"answers": …}` reply. Missing host support returns a loud
  error instead of hanging.
- Claims `prompt/context` to inject short usage guidelines into the system
  prompt.

## Where questions show up

The plugin never renders UI itself; it asks the host, and the host decides
how the question is presented.

- **Terminal (TUI):** an inline modal above the input box.
- **Piped stdin:** one prompt per question; no stdin resolves empty.
- **`gray -p --json` with `GRAY_JSON_ASK=1`:** the question goes out as an
  `ask` progress row and the answer comes back on stdin, so the program
  driving gray can render its own UI.

## Install

Requires a gray build with the `host/ask` bridge:

```sh
cargo build --release
gray plugin install <release-tarball-url>
gray plugin install questions
```

## Development

```sh
cargo test
cargo fmt --check
```

## Wire

- `plugin/manifest` → `{name: "questions", tools: [request_user_input], hooks: ["prompt/context"]}`
- `tool/call` → delegates to `host/ask`, returns `{"content": "{"answers": …}"}`
- `prompt/context` → usage guidelines text
- `event/notify`, unknown lines → ignored; `plugin/shutdown` → clean exit

---
Part of the [gray](https://github.com/vstaln/gray) plugin ecosystem —
the open-source AI agent harness. <https://gray.alignment.id>
