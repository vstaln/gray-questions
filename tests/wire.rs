//! Wire-level tests: drive the built sidecar binary over stdio like the
//! gray host does (numeric host→sidecar ids, string sidecar→host ids).

use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::time::Duration;

struct Harness {
    child: Child,
    stdin: ChildStdin,
    lines: std::sync::mpsc::Receiver<String>,
    _reader: std::thread::JoinHandle<()>,
}

impl Harness {
    fn spawn() -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_questions"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn questions sidecar");
        let stdin = child.stdin.take().expect("stdin");
        let stdout: ChildStdout = child.stdout.take().expect("stdout");
        let (tx, rx) = std::sync::mpsc::channel();
        let reader = std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                if tx.send(line).is_err() {
                    break;
                }
            }
        });
        Self {
            child,
            stdin,
            lines: rx,
            _reader: reader,
        }
    }

    fn send(&mut self, v: &serde_json::Value) {
        writeln!(self.stdin, "{v}").expect("write");
        self.stdin.flush().expect("flush");
    }

    fn next(&self) -> serde_json::Value {
        let line = self
            .lines
            .recv_timeout(Duration::from_secs(10))
            .expect("reply within 10s");
        serde_json::from_str(&line).expect("valid json reply")
    }

    /// Next line that is NOT a sidecar→host request (string id + method).
    fn next_host_reply(&self) -> serde_json::Value {
        loop {
            let v = self.next();
            let is_sidecar_req = v.get("id").and_then(|i| i.as_str()).is_some()
                && v.get("method").and_then(|m| m.as_str()).is_some();
            if !is_sidecar_req {
                return v;
            }
            // stow nothing: callers that expect host/ask use next_ask()
            panic!("expected host reply, got sidecar request: {v}");
        }
    }

    fn next_ask(&self) -> (String, serde_json::Value) {
        loop {
            let v = self.next();
            if let (Some(id), Some(method)) = (
                v.get("id").and_then(|i| i.as_str()),
                v.get("method").and_then(|m| m.as_str()),
            ) && method == "host/ask"
            {
                return (id.to_string(), v);
            }
        }
    }
}

impl Drop for Harness {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn one_question() -> serde_json::Value {
    serde_json::json!({
        "questions": [{
            "id": "color",
            "header": "Color",
            "question": "Which color?",
            "options": [
                {"label": "Red", "description": "warm"},
                {"label": "Blue (Recommended)", "description": "cool"}
            ]
        }]
    })
}

#[test]
fn manifest_and_guidance() {
    let mut h = Harness::spawn();
    h.send(&serde_json::json!({"id": 1, "method": "plugin/manifest"}));
    let m = h.next_host_reply();
    assert_eq!(m["id"], 1);
    assert_eq!(m["result"]["name"], "questions");
    assert_eq!(m["result"]["protocol"], "1.1");
    assert_eq!(m["result"]["tools"][0]["name"], "request_user_input");
    h.send(&serde_json::json!({"id": 2, "method": "prompt/context", "params": {"cwd": "/tmp"}}));
    let g = h.next_host_reply();
    assert!(
        g["result"]["text"]
            .as_str()
            .unwrap()
            .contains("request_user_input")
    );
    h.send(&serde_json::json!({"method": "event/notify", "params": {"type": "turn_end"}}));
    h.send(&serde_json::json!({"method": "plugin/shutdown"}));
    let status = h.child.wait_timeout();
    assert!(status, "sidecar exits on plugin/shutdown");
}

#[test]
fn tool_call_round_trips_host_ask() {
    let mut h = Harness::spawn();
    h.send(&serde_json::json!({"id": 1, "method": "plugin/manifest"}));
    let _ = h.next_host_reply();
    h.send(&serde_json::json!({
        "id": 2, "method": "tool/call",
        "params": {"name": "request_user_input", "args": one_question()}
    }));
    let (ask_id, ask) = h.next_ask();
    assert_eq!(ask["params"]["blocking"], true);
    assert_eq!(ask["params"]["questions"][0]["id"], "color");
    // host answers; plugin must normalize (is_other forced) silently
    h.send(&serde_json::json!({
        "id": ask_id,
        "result": {"answers": {"color": {"answers": ["Blue (Recommended)"]}}}
    }));
    let reply = h.next_host_reply();
    assert_eq!(reply["id"], 2);
    assert_eq!(reply["result"]["is_error"], serde_json::Value::Null);
    let content: serde_json::Value =
        serde_json::from_str(reply["result"]["content"].as_str().unwrap()).unwrap();
    assert_eq!(
        content["answers"]["color"]["answers"][0],
        "Blue (Recommended)"
    );
}

#[test]
fn tool_call_rejects_bad_args_without_asking() {
    let mut h = Harness::spawn();
    h.send(&serde_json::json!({"id": 1, "method": "plugin/manifest"}));
    let _ = h.next_host_reply();
    for (id, args) in [
        (2, serde_json::json!({"questions": []})),
        (
            3,
            serde_json::json!({"questions": [
                {"id":"a","header":"A","question":"q?","options":[{"label":"x","description":"y"}]},
                {"id":"b","header":"B","question":"q?","options":[{"label":"x","description":"y"}]},
                {"id":"c","header":"C","question":"q?","options":[{"label":"x","description":"y"}]},
                {"id":"d","header":"D","question":"q?","options":[{"label":"x","description":"y"}]}
            ]}),
        ),
        (
            4,
            serde_json::json!({"questions": [
                {"id":"a","header":"A","question":"q?","options":[]}
            ]}),
        ),
    ] {
        h.send(&serde_json::json!({
            "id": id, "method": "tool/call",
            "params": {"name": "request_user_input", "args": args}
        }));
        let reply = h.next_host_reply();
        assert_eq!(reply["id"], id);
        assert_eq!(reply["result"]["is_error"], true, "args {id}");
    }
}

trait WaitTimeout {
    fn wait_timeout(&mut self) -> bool;
}

impl WaitTimeout for Child {
    fn wait_timeout(&mut self) -> bool {
        let start = std::time::Instant::now();
        while start.elapsed() < Duration::from_secs(5) {
            match self.try_wait() {
                Ok(Some(_)) => return true,
                Ok(None) => std::thread::sleep(Duration::from_millis(50)),
                Err(_) => return false,
            }
        }
        false
    }
}
