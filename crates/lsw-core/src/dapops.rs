use std::io::{BufRead, Write};

use serde::{Deserialize, Serialize};

use crate::envops::Environment;
use crate::error::{Error, Result};

const MAX_MESSAGE_BYTES: usize = 8 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProtocolMessage {
    pub seq: i64,
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub event: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_seq: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub success: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(default, skip_serializing_if = "serde_json::Value::is_null")]
    pub arguments: serde_json::Value,
    #[serde(default, skip_serializing_if = "serde_json::Value::is_null")]
    pub body: serde_json::Value,
}

pub fn read_message<R: BufRead>(reader: &mut R) -> Result<Option<ProtocolMessage>> {
    let mut content_length: Option<usize> = None;
    loop {
        let mut line = String::new();
        let n = reader.read_line(&mut line).map_err(|e| Error::Dap {
            detail: format!("header read failed: {e}"),
        })?;
        if n == 0 {
            return Ok(None);
        }
        let trimmed = line.trim_end_matches(['\r', '\n']);
        if trimmed.is_empty() {
            break;
        }
        if let Some(value) = trimmed.strip_prefix("Content-Length:") {
            content_length = value.trim().parse().ok();
        }
    }
    let len = content_length.ok_or_else(|| Error::Dap {
        detail: "message had no Content-Length header".into(),
    })?;
    if len > MAX_MESSAGE_BYTES {
        return Err(Error::Dap {
            detail: format!("message body {len} bytes exceeds the {MAX_MESSAGE_BYTES}-byte limit"),
        });
    }
    let mut buf = vec![0u8; len];
    reader.read_exact(&mut buf).map_err(|e| Error::Dap {
        detail: format!("body read failed: {e}"),
    })?;
    let msg = serde_json::from_slice(&buf).map_err(|e| Error::Dap {
        detail: format!("malformed DAP message: {e}"),
    })?;
    Ok(Some(msg))
}

pub fn write_message<W: Write>(writer: &mut W, msg: &ProtocolMessage) -> Result<()> {
    let body = serde_json::to_vec(msg).map_err(|e| Error::Dap {
        detail: format!("cannot serialize DAP message: {e}"),
    })?;
    write!(writer, "Content-Length: {}\r\n\r\n", body.len()).map_err(|e| Error::Dap {
        detail: format!("write failed: {e}"),
    })?;
    writer.write_all(&body).map_err(|e| Error::Dap {
        detail: format!("write failed: {e}"),
    })?;
    writer.flush().map_err(|e| Error::Dap {
        detail: format!("flush failed: {e}"),
    })?;
    Ok(())
}

pub struct Adapter<'a> {
    env: &'a Environment,
    seq: i64,
    backend: Option<std::process::Child>,
}

impl Drop for Adapter<'_> {
    fn drop(&mut self) {
        if let Some(mut child) = self.backend.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

impl<'a> Adapter<'a> {
    pub fn new(env: &'a Environment) -> Self {
        Self {
            env,
            seq: 0,
            backend: None,
        }
    }

    fn next_seq(&mut self) -> i64 {
        self.seq += 1;
        self.seq
    }

    fn success_response(
        &mut self,
        req: &ProtocolMessage,
        body: serde_json::Value,
    ) -> ProtocolMessage {
        ProtocolMessage {
            seq: self.next_seq(),
            kind: "response".into(),
            command: req.command.clone(),
            event: None,
            request_seq: Some(req.seq),
            success: Some(true),
            message: None,
            arguments: serde_json::Value::Null,
            body,
        }
    }

    fn event(&mut self, name: &str, body: serde_json::Value) -> ProtocolMessage {
        ProtocolMessage {
            seq: self.next_seq(),
            kind: "event".into(),
            command: None,
            event: Some(name.into()),
            request_seq: None,
            success: None,
            message: None,
            arguments: serde_json::Value::Null,
            body,
        }
    }

    fn error_response(&mut self, req: &ProtocolMessage, message: &str) -> ProtocolMessage {
        ProtocolMessage {
            seq: self.next_seq(),
            kind: "response".into(),
            command: req.command.clone(),
            event: None,
            request_seq: Some(req.seq),
            success: Some(false),
            message: Some(message.to_owned()),
            arguments: serde_json::Value::Null,
            body: serde_json::Value::Null,
        }
    }

    pub fn handle(&mut self, req: &ProtocolMessage) -> Result<Vec<ProtocolMessage>> {
        let command = req.command.as_deref().unwrap_or("");
        match command {
            "initialize" => {
                let caps = serde_json::json!({
                    "supportsConfigurationDoneRequest": true,
                    "supportsTerminateRequest": true,
                });
                let response = self.success_response(req, caps);
                let initialized = self.event("initialized", serde_json::Value::Null);
                Ok(vec![response, initialized])
            }
            "launch" => {
                let Some(program) = req.arguments.get("program").and_then(|v| v.as_str()) else {
                    return Ok(vec![
                        self.error_response(req, "launch request missing 'program'"),
                    ]);
                };
                match self.launch_backend(std::path::Path::new(program)) {
                    Ok(()) => {
                        let response = self.success_response(req, serde_json::Value::Null);
                        let running = self.event(
                            "process",
                            serde_json::json!({ "name": program, "startMethod": "launch" }),
                        );
                        Ok(vec![response, running])
                    }
                    Err(e) => Ok(vec![self.error_response(req, &e.to_string())]),
                }
            }
            "configurationDone" => Ok(vec![self.success_response(req, serde_json::Value::Null)]),
            "terminate" | "disconnect" => {
                if let Some(mut child) = self.backend.take() {
                    let _ = child.kill();
                    let _ = child.wait();
                }
                let response = self.success_response(req, serde_json::Value::Null);
                let terminated = self.event("terminated", serde_json::Value::Null);
                Ok(vec![response, terminated])
            }
            other => {
                Ok(vec![self.error_response(
                    req,
                    &format!("unsupported request '{other}'"),
                )])
            }
        }
    }

    fn launch_backend(&mut self, program: &std::path::Path) -> Result<()> {
        if !program.is_file() {
            return Err(Error::NotExecutable {
                program: program.to_path_buf(),
                detail: "file not found".into(),
            });
        }
        let program =
            std::path::absolute(program).map_err(|e| Error::io(program.to_path_buf(), e))?;
        let winedbg = find_winedbg().ok_or_else(|| Error::ToolMissing {
            tool: "winedbg".into(),
            fix: "install wine (winedbg ships with it)".into(),
        })?;
        let mut command = std::process::Command::new(winedbg);
        lsw_runtime::scrub_wine_env(&mut command);
        command
            .args(["--gdb", "--no-start"])
            .arg(&program)
            .env("WINEPREFIX", self.env.layout.prefix())
            .env("WINEDEBUG", "fixme-all");
        let child = command
            .spawn()
            .map_err(|e| Error::io(std::path::PathBuf::from("winedbg"), e))?;
        self.backend = Some(child);
        Ok(())
    }
}

fn find_winedbg() -> Option<std::path::PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|d| d.join("winedbg"))
        .find(|c| c.is_file())
}

pub fn serve<R: BufRead, W: Write>(
    env: &Environment,
    reader: &mut R,
    writer: &mut W,
) -> Result<()> {
    let mut adapter = Adapter::new(env);
    while let Some(req) = read_message(reader)? {
        if req.kind != "request" {
            continue;
        }
        let terminating = matches!(
            req.command.as_deref(),
            Some("terminate") | Some("disconnect")
        );
        let responses = adapter.handle(&req)?;
        for msg in &responses {
            write_message(writer, msg)?;
        }
        if terminating {
            break;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn request(seq: i64, command: &str, arguments: serde_json::Value) -> Vec<u8> {
        let msg = ProtocolMessage {
            seq,
            kind: "request".into(),
            command: Some(command.into()),
            event: None,
            request_seq: None,
            success: None,
            message: None,
            arguments,
            body: serde_json::Value::Null,
        };
        let body = serde_json::to_vec(&msg).unwrap();
        let mut out = format!("Content-Length: {}\r\n\r\n", body.len()).into_bytes();
        out.extend_from_slice(&body);
        out
    }

    #[test]
    fn framing_roundtrip() {
        let bytes = request(1, "initialize", serde_json::json!({"adapterID": "lsw"}));
        let mut reader = Cursor::new(bytes);
        let msg = read_message(&mut reader).unwrap().unwrap();
        assert_eq!(msg.command.as_deref(), Some("initialize"));
        assert_eq!(msg.seq, 1);
        assert!(read_message(&mut reader).unwrap().is_none());
    }

    #[test]
    fn oversized_content_length_is_rejected_before_allocating() {
        let bytes = b"Content-Length: 9999999999999999\r\n\r\n".to_vec();
        let mut reader = Cursor::new(bytes);
        let err = read_message(&mut reader).unwrap_err();
        assert!(err.to_string().contains("exceeds"));
    }

    #[test]
    fn unsupported_request_returns_failure_response_not_error() {
        let mut adapter = FakeAdapter::default();
        let req = ProtocolMessage {
            seq: 7,
            kind: "request".into(),
            command: Some("setBreakpoints".into()),
            event: None,
            request_seq: None,
            success: None,
            message: None,
            arguments: serde_json::Value::Null,
            body: serde_json::Value::Null,
        };
        let out = adapter.handle_unsupported(&req);
        assert_eq!(out[0].kind, "response");
        assert_eq!(out[0].success, Some(false));
        assert_eq!(out[0].request_seq, Some(7));
    }

    #[test]
    fn initialize_returns_capabilities_and_initialized_event() {
        let mut adapter = FakeAdapter::default();
        let req = ProtocolMessage {
            seq: 1,
            kind: "request".into(),
            command: Some("initialize".into()),
            event: None,
            request_seq: None,
            success: None,
            message: None,
            arguments: serde_json::Value::Null,
            body: serde_json::Value::Null,
        };
        let out = adapter.handle_initialize(&req);
        assert_eq!(out[0].kind, "response");
        assert_eq!(out[0].success, Some(true));
        assert_eq!(
            out[0].body["supportsTerminateRequest"],
            serde_json::json!(true)
        );
        assert_eq!(out[1].kind, "event");
        assert_eq!(out[1].event.as_deref(), Some("initialized"));
    }

    #[test]
    fn terminate_emits_terminated_event() {
        let mut adapter = FakeAdapter::default();
        let req = ProtocolMessage {
            seq: 5,
            kind: "request".into(),
            command: Some("terminate".into()),
            event: None,
            request_seq: None,
            success: None,
            message: None,
            arguments: serde_json::Value::Null,
            body: serde_json::Value::Null,
        };
        let out = adapter.handle_terminate(&req);
        assert_eq!(out[0].request_seq, Some(5));
        assert_eq!(out[1].event.as_deref(), Some("terminated"));
    }

    #[derive(Default)]
    struct FakeAdapter {
        seq: i64,
    }

    impl FakeAdapter {
        fn next_seq(&mut self) -> i64 {
            self.seq += 1;
            self.seq
        }
        fn handle_unsupported(&mut self, req: &ProtocolMessage) -> Vec<ProtocolMessage> {
            vec![ProtocolMessage {
                seq: self.next_seq(),
                kind: "response".into(),
                command: req.command.clone(),
                event: None,
                request_seq: Some(req.seq),
                success: Some(false),
                message: Some("unsupported".into()),
                arguments: serde_json::Value::Null,
                body: serde_json::Value::Null,
            }]
        }
        fn handle_initialize(&mut self, req: &ProtocolMessage) -> Vec<ProtocolMessage> {
            let caps = serde_json::json!({
                "supportsConfigurationDoneRequest": true,
                "supportsTerminateRequest": true,
            });
            vec![
                ProtocolMessage {
                    seq: self.next_seq(),
                    kind: "response".into(),
                    command: req.command.clone(),
                    event: None,
                    request_seq: Some(req.seq),
                    success: Some(true),
                    message: None,
                    arguments: serde_json::Value::Null,
                    body: caps,
                },
                ProtocolMessage {
                    seq: self.next_seq(),
                    kind: "event".into(),
                    command: None,
                    event: Some("initialized".into()),
                    request_seq: None,
                    success: None,
                    message: None,
                    arguments: serde_json::Value::Null,
                    body: serde_json::Value::Null,
                },
            ]
        }
        fn handle_terminate(&mut self, req: &ProtocolMessage) -> Vec<ProtocolMessage> {
            vec![
                ProtocolMessage {
                    seq: self.next_seq(),
                    kind: "response".into(),
                    command: req.command.clone(),
                    event: None,
                    request_seq: Some(req.seq),
                    success: Some(true),
                    message: None,
                    arguments: serde_json::Value::Null,
                    body: serde_json::Value::Null,
                },
                ProtocolMessage {
                    seq: self.next_seq(),
                    kind: "event".into(),
                    command: None,
                    event: Some("terminated".into()),
                    request_seq: None,
                    success: None,
                    message: None,
                    arguments: serde_json::Value::Null,
                    body: serde_json::Value::Null,
                },
            ]
        }
    }
}
