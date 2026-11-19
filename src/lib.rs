//! `gray_questions` shared core: the `request_user_input` tool schema and
//! normalization, plus the `host/ask` wire payloads. The sidecar binary
//! (`src/main.rs`) and the host-side asker (`gray` crate) must agree on
//! these shapes; this lib is the single definition.
//!
//! History: reshaped from gray's former builtins
//! (`crates/gray-core/src/questions.rs` +
//! `crates/gray-tools/src/request_user_input.rs`) as a sidecar: the plugin
//! owns schema + validation, the host owns user I/O via `host/ask`.

use serde::{Deserialize, Serialize};

/// One selectable option shown to the user.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UserOption {
    pub label: String,
    #[serde(default)]
    pub description: String,
    /// Picking this option opens a note editor instead of answering with the
    /// label ("Deny with a reason"); the host ships the text as `user_note:`.
    #[serde(default)]
    pub freeform: bool,
}

/// One question in a request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UserQuestion {
    pub id: String,
    pub header: String,
    pub question: String,
    /// Empty means a pure free-text question: the host shows one editor.
    #[serde(default)]
    pub options: Vec<UserOption>,
    /// The user may pick several options instead of one.
    #[serde(default)]
    pub multi: bool,
    /// Frontend adds an extra free-form "Other" option (codex forces this true).
    #[serde(default)]
    pub is_other: bool,
}

/// Answers for one question: selected option label plus optional notes
/// (`"user_note: …"` entries).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UserAnswer {
    pub id: String,
    pub answers: Vec<String>,
}

/// Tool name the model calls.
pub const TOOL_NAME: &str = "request_user_input";
/// Display name hosts show for the tool (manifest `label`); never sent to
/// the model.
pub const TOOL_LABEL: &str = "Request User Input";
/// Protocol version claimed in `plugin/manifest`.
pub const PROTOCOL: &str = "1.1";
/// Plugin name claimed in `plugin/manifest`.
pub const PLUGIN_NAME: &str = "questions";
/// `host/ask` method name on the sidecar→host wire.
pub const HOST_ASK: &str = "host/ask";

pub const OTHER_OPTION_LABEL: &str = "None of the above";
/// Surface-neutral on purpose: the host decides how notes are typed (a TUI
/// modal, a stdin prompt, a chat form), so the text names none of them.
pub const OTHER_OPTION_DESCRIPTION: &str = "Optionally, add details in your own words.";

/// Tool result for a `blocking:false` call that resolves without user
/// input. The wire has no late-answer channel — the host auto-resolves
/// non-blocking asks (`{"answers": {}}`) and `host/say` never reaches the
/// model — so the honest answer is "not yet".
pub const NO_ANSWER_YET: &str = "no answer yet — keep working; ask again later if still needed";

pub const MAX_QUESTIONS: usize = 12;
pub const MAX_OPTIONS: usize = 20;

/// Normalization: an "Other" row is always offered unless the question is
/// already free text, where the editor is the other row.
pub fn normalize_questions(mut args: Vec<UserQuestion>) -> Result<Vec<UserQuestion>, String> {
    if args.iter().any(|q| q.options.len() > MAX_OPTIONS) {
        return Err(format!(
            "request_user_input accepts at most {MAX_OPTIONS} options per question;              a longer list is a menu, not a question"
        ));
    }
    for q in &mut args {
        q.is_other = true;
    }
    Ok(args)
}

pub fn validate_tool_args(questions: &[UserQuestion]) -> Result<(), String> {
    if questions.is_empty() || questions.len() > MAX_QUESTIONS {
        return Err(format!(
            "request_user_input requires 1-{MAX_QUESTIONS} questions"
        ));
    }
    let mut ids = std::collections::HashSet::new();
    for q in questions {
        if q.id.trim().is_empty() {
            return Err("request_user_input requires an id for every question".to_string());
        }
        if !ids.insert(q.id.as_str()) {
            return Err(format!("request_user_input got a duplicate id: {}", q.id));
        }
        if q.question.trim().is_empty() {
            return Err(format!(
                "request_user_input got an empty question: {}",
                q.id
            ));
        }
    }
    normalize_questions(questions.to_vec()).map(|_| ())
}

/// `{"answers": {id: {"answers": [...]}}}` — the tool result shape.
pub fn answers_to_json(answers: &[UserAnswer]) -> serde_json::Value {
    let map: serde_json::Map<String, serde_json::Value> = answers
        .iter()
        .map(|a| (a.id.clone(), serde_json::json!({ "answers": a.answers })))
        .collect();
    serde_json::json!({ "answers": map })
}

/// Parse a `host/ask` result into answers. Lenient: missing/invalid shapes
/// yield empty answers (the caller decides; approvals fail closed).
pub fn parse_ask_result(v: &serde_json::Value) -> Vec<UserAnswer> {
    let Some(map) = v.get("answers").and_then(|a| a.as_object()) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for (id, entry) in map {
        let answers = entry
            .get("answers")
            .and_then(|a| a.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|e| e.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default();
        out.push(UserAnswer {
            id: id.clone(),
            answers,
        });
    }
    out
}

/// The `request_user_input` tool definition (model-facing schema).
pub fn tool_definition() -> serde_json::Value {
    serde_json::json!({
        "name": TOOL_NAME,
        "label": TOOL_LABEL,
        "description": "Ask the user one or more questions, each with any mix of multiple-choice options and free text, and wait for the answers.",
        "parameters": {
            "type": "object",
            "properties": {
                "questions": {
                    "type": "array",
                    "description": "Questions to ask. Prefer 1; ask several only when you genuinely need several answered at once.",
                    "items": {
                        "type": "object",
                        "properties": {
                            "id": {"type": "string", "description": "Stable identifier for mapping answers (snake_case), unique in the request."},
                            "header": {"type": "string", "description": "Short header label shown in the UI (12 or fewer chars)."},
                            "question": {"type": "string", "description": "The question, in one sentence. Further lines become the subject shown under it."},
                            "multi": {"type": "boolean", "description": "Let the user pick several options instead of one."},
                            "options": {
                                "type": "array",
                                "description": "Choices offered. Omit or leave empty for a free-text question. Put the recommended option first and suffix its label with \"(Recommended)\". Do not add an \"Other\" option; the client appends a free-form row automatically unless the option list is empty.",
                                "items": {
                                    "type": "object",
                                    "properties": {
                                        "label": {"type": "string", "description": "User-facing label (1-5 words)."},
                                        "description": {"type": "string", "description": "One short sentence explaining impact/tradeoff if selected."},
                                        "freeform": {"type": "boolean", "description": "Picking this opens a note editor and sends the typed text instead of the label, e.g. a \"Deny with a reason\" row."}
                                    },
                                    "required": ["label"],
                                    "additionalProperties": false
                                }
                            }
                        },
                        "required": ["id", "header", "question"],
                        "additionalProperties": false
                    }
                },
                "blocking": {
                    "type": "boolean",
                    "description": "Wait for answers before continuing (default true). When false the call returns at once without waiting for the user, reporting \"no answer yet\"; call again with blocking true once the answer is needed."
                }
            },
            "required": ["questions"],
            "additionalProperties": false
        }
    })
}

/// The `plugin/manifest` result value.
pub fn manifest() -> serde_json::Value {
    serde_json::json!({
        "name": PLUGIN_NAME,
        "version": env!("CARGO_PKG_VERSION"),
        "protocol": PROTOCOL,
        "tools": [tool_definition()],
        "commands": [],
        "hooks": ["prompt/context"],
        // `host/ask` is a gated host surface: undeclared, gray refuses it.
        "capabilities": ["host.ask"],
    })
}

/// The `prompt/context` guidance text (usage guidelines for the model).
pub fn prompt_guidance() -> &'static str {
    "request_user_input — ask the user when a decision blocks progress and only they can make it. \
     Prefer acting on a sensible default; ask when being wrong is expensive or the choice is genuinely theirs. \
     Ask one question when one is enough; batch several only when they belong together. \
     Offer a few mutually exclusive options, recommended first with a \"(Recommended)\" suffix, and set \"multi\": true when several can apply. \
     Leave options empty to ask for free text, and use a freeform option to invite a reason alongside a decision."
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifest_declares_the_ask_capability() {
        assert_eq!(manifest()["capabilities"], serde_json::json!(["host.ask"]));
    }

    fn q(options: usize) -> UserQuestion {
        UserQuestion {
            id: "x".into(),
            header: "H".into(),
            question: "q?".into(),
            options: (0..options)
                .map(|i| UserOption {
                    label: format!("o{i}"),
                    description: "d".into(),
                    freeform: false,
                })
                .collect(),
            multi: false,
            is_other: false,
        }
    }

    #[test]
    fn normalize_allows_free_text_and_caps_options() {
        assert!(normalize_questions(vec![q(0), q(2)]).is_ok());
        assert!(normalize_questions(vec![q(2), q(MAX_OPTIONS + 1)]).is_err());
    }

    #[test]
    fn normalize_forces_is_other() {
        let out = normalize_questions(vec![q(2)]).unwrap();
        assert!(out[0].is_other);
    }

    #[test]
    fn validate_rejects_count_and_duplicate_ids() {
        assert!(validate_tool_args(&[]).is_err());
        let many = |n: usize| {
            (0..n)
                .map(|i| UserQuestion {
                    id: format!("q{i}"),
                    ..q(1)
                })
                .collect::<Vec<_>>()
        };
        assert!(validate_tool_args(&many(MAX_QUESTIONS)).is_ok());
        assert!(validate_tool_args(&many(MAX_QUESTIONS + 1)).is_err());
        assert!(validate_tool_args(&many(2)).is_ok());
        assert!(validate_tool_args(&[q(1), q(1)]).is_err());
    }

    #[test]
    fn answers_json_shape() {
        let out = answers_to_json(&[UserAnswer {
            id: "mode".into(),
            answers: vec!["fast".into(), "user_note: hurry".into()],
        }]);
        assert_eq!(out["answers"]["mode"]["answers"][0], "fast");
        assert_eq!(out["answers"]["mode"]["answers"][1], "user_note: hurry");
    }

    #[test]
    fn parse_ask_result_lenient() {
        assert!(parse_ask_result(&serde_json::json!({})).is_empty());
        assert!(parse_ask_result(&serde_json::json!({"answers": null})).is_empty());
        let v = serde_json::json!({"answers": {"q": {"answers": ["a"]}}});
        let out = parse_ask_result(&v);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].id, "q");
        assert_eq!(out[0].answers, vec!["a".to_string()]);
    }

    #[test]
    fn other_option_names_no_particular_surface() {
        assert!(!OTHER_OPTION_DESCRIPTION.contains("tab"));
    }

    #[test]
    fn manifest_shape() {
        let m = manifest();
        assert_eq!(m["name"], "questions");
        assert_eq!(m["protocol"], "1.1");
        assert_eq!(m["tools"][0]["name"], "request_user_input");
        assert_eq!(m["tools"][0]["label"], "Request User Input");
    }
}
