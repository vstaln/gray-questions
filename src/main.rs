//! `questions` sidecar: exposes the `request_user_input` tool over the gray
//! wire v1. Blocking std I/O with a reader thread (same shape as
//! `crates/gray-plugin/testdata/echo_plugin.rs`); no async runtime, no gray
//! deps beyond the in-repo `gray_questions` lib.
//!
//! Wire summary:
//! - host→sidecar `{"id":N,"method":"plugin/manifest"}` → `{"id":N,"result":{…}}`
//! - host→sidecar `{"id":N,"method":"tool/call","params":{"name","args",…}}`
//! - host→sidecar `{"id":N,"method":"prompt/context","params":{"cwd",…}}`
//! - host→sidecar `{"method":"event/notify",…}` (no id: ignored, no reply)
//! - host→sidecar `{"method":"plugin/shutdown"}` (no id: exit cleanly)
//! - sidecar→host `{"id":"<n>","method":"host/ask","params":{…}}` →
//!   `{"id":"<n>","result":{"answers":{…}}}` (blocking read with a TTL)

use gray_questions::{UserQuestion, answers_to_json, manifest, prompt_guidance};
use std::collections::HashMap;
use std::io::{BufRead, Write};
use std::sync::{Arc, Mutex, mpsc};
use std::time::Duration;

/// How long `tool/call` waits for a `host/ask` reply before failing loudly.
/// Must sit under the host's ask-gated outer deadline (330s) so the plugin
/// reports its own timeout instead of surfacing the host's generic one.
const ASK_TTL: Duration = Duration::from_secs(300);

type Pending = Arc<Mutex<HashMap<String, mpsc::Sender<serde_json::Value>>>>;

fn next_id(counter: &Arc<Mutex<u64>>) -> String {
    let mut n = counter.lock().expect("id counter");
    *n += 1;
    format!("q{n}")
}

fn send_host_ask(
    out: &Arc<Mutex<std::io::Stdout>>,
    id: &str,
    questions: &[UserQuestion],
    blocking: bool,
) {
    let req = serde_json::json!({
        "id": id,
        "method": gray_questions::HOST_ASK,
        "params": { "questions": questions, "blocking": blocking },
    });
    let mut o = out.lock().expect("stdout");
    let _ = writeln!(o, "{req}");
    let _ = o.flush();
}

fn tool_call(
    out: &Arc<Mutex<std::io::Stdout>>,
    pending: &Pending,
    counter: &Arc<Mutex<u64>>,
    params: &serde_json::Value,
) -> serde_json::Value {
    let args = params
        .get("args")
        .cloned()
        .unwrap_or(serde_json::Value::Null);
    let questions: Vec<UserQuestion> = match args
        .get("questions")
        .cloned()
        .map(serde_json::from_value)
        .transpose()
    {
        Ok(q) => q.unwrap_or_default(),
        Err(e) => {
            return serde_json::json!({
                "content": format!("request_user_input: bad args: {e}"),
                "is_error": true,
            });
        }
    };
    if let Err(e) = gray_questions::validate_tool_args(&questions) {
        return serde_json::json!({ "content": e, "is_error": true });
    }
    let normalized = match gray_questions::normalize_questions(questions) {
        Ok(q) => q,
        Err(e) => return serde_json::json!({ "content": e, "is_error": true }),
    };
    let blocking = args
        .get("blocking")
        .and_then(|b| b.as_bool())
        .unwrap_or(true);
    let id = next_id(counter);
    let (tx, rx) = mpsc::channel();
    pending.lock().expect("pending").insert(id.clone(), tx);
    send_host_ask(out, &id, &normalized, blocking);
    match rx.recv_timeout(ASK_TTL) {
        Ok(result) => {
            if let Some(err) = result.get("error").and_then(|e| e.as_str()) {
                return serde_json::json!({
                    "content": format!("request_user_input failed: {err}"),
                    "is_error": true,
                });
            }
            let answers = gray_questions::parse_ask_result(&result);
            serde_json::json!({ "content": answers_to_json(&answers).to_string() })
        }
        Err(_) => serde_json::json!({
            "content": "request_user_input: host did not answer in time",
            "is_error": true,
        }),
    }
}

fn main() {
    let stdout = Arc::new(Mutex::new(std::io::stdout()));
    let pending: Pending = Arc::new(Mutex::new(HashMap::new()));
    let counter = Arc::new(Mutex::new(0u64));

    // Reader thread: host→sidecar notifications route `host/ask` replies by
    // string id; numeric-id requests (`tool/call`…) queue to the main loop.
    let (work_tx, work_rx) = mpsc::channel::<serde_json::Value>();
    let reader_pending = pending.clone();
    let reader = std::thread::spawn(move || {
        let stdin = std::io::stdin();
        for line in stdin.lock().lines() {
            let Ok(line) = line else { break };
            let Ok(v) = serde_json::from_str::<serde_json::Value>(&line) else {
                continue;
            };
            // `plugin/shutdown` is a notification (no id): stop reading so
            // the work channel closes and the main loop exits. Anything
            // still queued drains first; an in-flight `host/ask` still
            // waits its TTL (the host kills us after its grace period).
            if v.get("method").and_then(|m| m.as_str()) == Some("plugin/shutdown") {
                break;
            }
            // `host/ask` reply: string id + result (no method).
            if let Some(id) = v.get("id").and_then(|i| i.as_str())
                && v.get("method").is_none()
                && let Some(tx) = reader_pending.lock().expect("pending").remove(id)
            {
                let _ = tx.send(v.get("result").cloned().unwrap_or(serde_json::Value::Null));
                continue;
            }
            // Notifications (no id) need no reply and no handling.
            let Some(id) = v.get("id").and_then(|i| i.as_u64()) else {
                continue;
            };
            let _ = id;
            if work_tx.send(v).is_err() {
                break;
            }
        }
    });

    for req in work_rx {
        let method = req.get("method").and_then(|m| m.as_str()).unwrap_or("");
        let id = req.get("id").cloned().unwrap_or(serde_json::json!(0));
        let params = req
            .get("params")
            .cloned()
            .unwrap_or(serde_json::Value::Null);
        let result = match method {
            "plugin/manifest" => manifest(),
            "prompt/context" => serde_json::json!({ "text": prompt_guidance() }),
            "tool/call" => {
                let name = params.get("name").and_then(|n| n.as_str()).unwrap_or("");
                if name == gray_questions::TOOL_NAME || name.is_empty() {
                    tool_call(&stdout, &pending, &counter, &params)
                } else {
                    serde_json::json!({
                        "content": format!("unknown tool: {name}"),
                        "is_error": true,
                    })
                }
            }
            _ => continue, // unknown methods/lines are ignored
        };
        let reply = serde_json::json!({ "id": id, "result": result });
        let mut o = stdout.lock().expect("stdout");
        let _ = writeln!(o, "{reply}");
        let _ = o.flush();
    }
    let _ = reader.join();
}
