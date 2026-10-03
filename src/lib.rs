//! `gray_questions` shared core: the `request_user_input` tool schema and
//! normalization, plus the `host/ask` wire payloads. The sidecar binary
//! (`src/main.rs`) and the host-side asker (`gray` crate) must agree on
//! these shapes; this lib is the single definition.
//!
//! History: ported from gray's deleted builtins
//! (`crates/gray-core/src/questions.rs` +
//! `crates/gray-tools/src/request_user_input.rs` at `d81c1a7`), reshaped as
//! a sidecar: the plugin owns schema + validation, the host owns user I/O
//! via `host/ask`.

use serde::{Deserialize, Serialize};

/// One selectable option shown to the user.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UserOption {
    pub label: String,
    pub description: String,
}

/// One question in a request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UserQuestion {
    pub id: String,
    pub header: String,
    pub question: String,
    #[serde(default)]
    pub options: Vec<UserOption>,
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

/// Codex normalization: every question needs options, and an "Other" option
/// is always added client-side.
pub fn normalize_questions(mut args: Vec<UserQuestion>) -> Result<Vec<UserQuestion>, String> {
    if args.iter().any(|q| q.options.is_empty()) {
        return Err("request_user_input requires non-empty options for every question".to_string());
    }
    for q in &mut args {
        q.is_other = true;
    }
    Ok(args)
}

pub fn validate_tool_args(questions: &[UserQuestion]) -> Result<(), String> {
    if questions.is_empty() || questions.len() > 3 {
        return Err("request_user_input requires 1-3 questions".to_string());
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
        "description": "Request user input for one to three short questions and wait for the response.",
        "parameters": {
            "type": "object",
            "properties": {
                "questions": {
                    "type": "array",
                    "description": "Questions to show the user. Prefer 1 and do not exceed 3",
                    "items": {
                        "type": "object",
                        "properties": {
                            "id": {"type": "string", "description": "Stable identifier for mapping answers (snake_case)."},
                            "header": {"type": "string", "description": "Short header label shown in the UI (12 or fewer chars)."},
                            "question": {"type": "string", "description": "Single-sentence prompt shown to the user."},
                            "options": {
                                "type": "array",
                                "description": "Provide 2-3 mutually exclusive choices. Put the recommended option first and suffix its label with \"(Recommended)\". Do not include an \"Other\" option in this list; the client will add a free-form \"Other\" option automatically.",
                                "items": {
                                    "type": "object",
                                    "properties": {
                                        "label": {"type": "string", "description": "User-facing label (1-5 words)."},
                                        "description": {"type": "string", "description": "One short sentence explaining impact/tradeoff if selected."}
                                    },
                                    "required": ["label", "description"],
                                    "additionalProperties": false
                                }
                            }
                        },
                        "required": ["id", "header", "question", "options"],
                        "additionalProperties": false
                    }
                },
                "blocking": {
                    "type": "boolean",
                    "description": "Wait for answers before continuing (default true). When false the agent keeps working; answers arrive as a follow-up message or auto-resolve after 2 minutes."
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
    })
}

/// The `prompt/context` guidance text (usage guidelines for the model).
pub fn prompt_guidance() -> &'static str {
    "request_user_input — ask the user 1-3 multiple-choice questions when a decision blocks progress. \
     Use it when a concrete decision blocks progress and the choices are few; act on sensible defaults otherwise. \
     Ask 1 question when possible (max 3); give each 2-3 mutually exclusive options, recommended first with a \"(Recommended)\" suffix."
}

#[cfg(test)]
mod tests {
    use super::*;

    fn q(options: usize) -> UserQuestion {
        UserQuestion {
            id: "x".into(),
            header: "H".into(),
            question: "q?".into(),
            options: (0..options)
                .map(|i| UserOption {
                    label: format!("o{i}"),
                    description: "d".into(),
                })
                .collect(),
            is_other: false,
        }
    }

    #[test]
    fn normalize_requires_options() {
        assert!(normalize_questions(vec![q(0), q(2)]).is_err());
        assert!(normalize_questions(vec![q(2), q(3)]).is_ok());
    }

    #[test]
    fn normalize_forces_is_other() {
        let out = normalize_questions(vec![q(2)]).unwrap();
        assert!(out[0].is_other);
    }

    #[test]
    fn validate_rejects_count() {
        assert!(validate_tool_args(&[]).is_err());
        assert!(validate_tool_args(&[q(1), q(1), q(1), q(1)]).is_err());
        assert!(validate_tool_args(&[q(1)]).is_ok());
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
