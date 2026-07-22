use std::io::{BufRead, Write};

const PROTOCOL: u32 = 1;

fn main() {
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() {
            continue;
        }
        let request: serde_json::Value = match serde_json::from_str(&line) {
            Ok(value) => value,
            Err(_) => continue,
        };
        let id = request.get("id").cloned().unwrap_or(serde_json::json!(0));
        let method = request
            .get("method")
            .and_then(|m| m.as_str())
            .unwrap_or_default();

        let response = match method {
            "handshake" => serde_json::json!({
                "id": id,
                "result": {
                    "protocol": PROTOCOL,
                    "provider": "example",
                    "providerVersion": env!("CARGO_PKG_VERSION"),
                    "kind": "runtime"
                }
            }),
            "version" => serde_json::json!({
                "id": id,
                "result": { "provider": "example", "version": env!("CARGO_PKG_VERSION") }
            }),
            "resolve" => serde_json::json!({
                "id": id,
                "result": { "available": true, "detail": "reference provider; runs nothing" }
            }),
            "shutdown" => {
                let bye = serde_json::json!({ "id": id, "result": null });
                let _ = writeln!(out, "{bye}");
                let _ = out.flush();
                break;
            }
            other => serde_json::json!({
                "id": id,
                "error": { "code": -32601, "message": format!("unknown method: {other}") }
            }),
        };
        if writeln!(out, "{response}").is_err() {
            break;
        }
        let _ = out.flush();
    }
}
