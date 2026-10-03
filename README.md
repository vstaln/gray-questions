# gray-questions

Sidecar plugin for the [gray](https://github.com/vstaln/gray) agent harness:
the AI asking **you** clarifying questions (`request_user_input`).

Rust, std-only I/O over the gray wire v1 (NDJSON over stdio). No async
runtime, no gray dependencies — the binary is self-contained.

## What it does

- Exposes one tool, `request_user_input`: 1–3 multiple-choice questions,
  options + free-form notes — the codex shape, verbatim schema.
- Validation is the plugin's: non-empty options per question, max 3,
  `is_other` forced (the client adds "None of the above" automatically).
- Asking is the host's: `tool/call` sends `host/ask` and waits up to 300s
  for the `{"answers": …}` reply. No host handler → loud error, never a hang.
- Claims `prompt/context` to inject the 2-line usage guidelines into the
  system prompt.

## Where questions show up

The plugin never talks to a person itself; it asks the host (`host/ask`), and
whatever hosts gray decides how the question is shown. gray has no question
tool of its own, so nothing asks unless this plugin is installed.

- **Terminal (TUI):** an inline modal above the input box.
- **Piped stdin:** one prompt per question; no stdin at all resolves empty.
- **`gray -p --json` with `GRAY_JSON_ASK=1`:** the question goes out as an
  `ask` progress row and the answer comes back on stdin, so the program
  driving gray shows it in its own UI. The Discord bridge
  (gray-discord-plugin) uses this to post a question card with buttons, a
  menu and a free-text form.

## Install

Requires a gray build with the `host/ask` bridge (gray ≥ 0.1.0 with
`feat/plugin-ask`):

```sh
cargo build --release
gray plugin install <release-tarball-url>   # day one
gray plugin install questions               # after index publish
```

## Development

```sh
cargo test
cargo fmt --check
```

## Wire

- `plugin/manifest` → `{name: "questions", tools: [request_user_input], hooks: ["prompt/context"]}`
- `tool/call` → delegates to `host/ask`, returns `{"content": "{\"answers\": …}"}`
- `prompt/context` → usage guidelines text
- `event/notify`, unknown lines → ignored; `plugin/shutdown` → clean exit
