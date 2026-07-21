use std::io::{BufRead, Read, Write};

use serde::{Deserialize, Serialize};

use crate::debugops::{self, DebugOptions};
use crate::envops::Environment;
use crate::error::{Error, Result};

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
}

impl<'a> Adapter<'a> {
    pub fn new(env: &'a Environment) -> Self {
        Self { env, seq: 0 }
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
                let program = req
                    .arguments
                    .get("program")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| Error::Dap {
                        detail: "launch request missing 'program'".into(),
                    })?;
                self.launch_backend(std::path::Path::new(program))?;
                let response = self.success_response(req, serde_json::Value::Null);
                let running = self.event(
                    "process",
                    serde_json::json!({ "name": program, "startMethod": "launch" }),
                );
                Ok(vec![response, running])
            }
            "configurationDone" => Ok(vec![self.success_response(req, serde_json::Value::Null)]),
            "terminate" | "disconnect" => {
                let response = self.success_response(req, serde_json::Value::Null);
                let terminated = self.event("terminated", serde_json::Value::Null);
                Ok(vec![response, terminated])
            }
            other => Err(Error::Dap {
                detail: format!("unsupported DAP request '{other}'"),
            }),
        }
    }

    fn launch_backend(&self, program: &std::path::Path) -> Result<()> {
        debugops::debug(
            self.env,
            None,
            program,
            &[],
            &DebugOptions {
                gdb: true,
                no_start: true,
            },
        )
        .map(|_status| ())
    }
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
