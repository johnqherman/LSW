use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};

use serde::{Deserialize, Serialize};

use crate::dbgproxy::{RspConn, Stop, amd64};
use crate::dwarfline::DebugInfo;
use crate::envops::Environment;
use crate::error::{Error, Result};

const REGISTERS_REF: i64 = 1;
const REGISTER_NAMES: [&str; 17] = [
    "rax", "rbx", "rcx", "rdx", "rsi", "rdi", "rbp", "rsp", "r8", "r9", "r10", "r11", "r12", "r13",
    "r14", "r15", "rip",
];

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

struct Breakpoint {
    addr: u64,
    source: String,
    verified: bool,
}

pub struct Adapter<'a> {
    env: &'a Environment,
    seq: i64,
    backend: Option<Child>,
    conn: Option<RspConn>,
    info: Option<DebugInfo>,
    slide: u64,
    program: Option<PathBuf>,
    breakpoints: Vec<Breakpoint>,
    next_bp_id: i64,
    stop_on_entry: bool,
    started: bool,
    exited: bool,
}

impl Drop for Adapter<'_> {
    fn drop(&mut self) {
        self.shutdown();
    }
}

impl<'a> Adapter<'a> {
    pub fn new(env: &'a Environment) -> Self {
        Self {
            env,
            seq: 0,
            backend: None,
            conn: None,
            info: None,
            slide: 0,
            program: None,
            breakpoints: Vec::new(),
            next_bp_id: 1,
            stop_on_entry: false,
            started: false,
            exited: false,
        }
    }

    fn shutdown(&mut self) {
        if let Some(conn) = self.conn.as_mut() {
            conn.kill();
        }
        self.conn = None;
        if let Some(mut child) = self.backend.take() {
            let _ = child.kill();
            let _ = child.wait();
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
                    "supportsEvaluateForHovers": true,
                    "supportsStepBack": false,
                });
                let response = self.success_response(req, caps);
                let initialized = self.event("initialized", serde_json::Value::Null);
                Ok(vec![response, initialized])
            }
            "launch" => self.handle_launch(req),
            "setBreakpoints" => self.handle_set_breakpoints(req),
            "configurationDone" => self.handle_configuration_done(req),
            "threads" => self.handle_threads(req),
            "stackTrace" => self.handle_stack_trace(req),
            "scopes" => self.handle_scopes(req),
            "variables" => self.handle_variables(req),
            "continue" => self.handle_execution(req, ExecKind::Continue),
            "next" => self.handle_execution(req, ExecKind::Next),
            "stepIn" => self.handle_execution(req, ExecKind::StepIn),
            "stepOut" => self.handle_execution(req, ExecKind::StepOut),
            "pause" => self.handle_pause(req),
            "evaluate" => self.handle_evaluate(req),
            "terminate" | "disconnect" => {
                self.shutdown();
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

    fn handle_launch(&mut self, req: &ProtocolMessage) -> Result<Vec<ProtocolMessage>> {
        let Some(program) = req.arguments.get("program").and_then(|v| v.as_str()) else {
            return Ok(vec![
                self.error_response(req, "launch request missing 'program'"),
            ]);
        };
        self.stop_on_entry = req
            .arguments
            .get("stopOnEntry")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        match self.launch_backend(Path::new(program)) {
            Ok(()) => {
                let response = self.success_response(req, serde_json::Value::Null);
                let process = self.event(
                    "process",
                    serde_json::json!({ "name": program, "startMethod": "launch" }),
                );
                Ok(vec![response, process])
            }
            Err(e) => Ok(vec![self.error_response(req, &e.to_string())]),
        }
    }

    fn handle_set_breakpoints(&mut self, req: &ProtocolMessage) -> Result<Vec<ProtocolMessage>> {
        let source = req
            .arguments
            .get("source")
            .and_then(|s| s.get("path").or_else(|| s.get("name")))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_owned();
        let requested: Vec<u32> = req
            .arguments
            .get("breakpoints")
            .and_then(|v| v.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|b| b.get("line").and_then(|l| l.as_u64()).map(|l| l as u32))
                    .collect()
            })
            .unwrap_or_default();

        self.clear_breakpoints_for(&source);

        let mut verified = Vec::new();
        for line in requested {
            let id = self.next_bp_id;
            self.next_bp_id += 1;
            let addr = self
                .info
                .as_ref()
                .and_then(|i| i.line_to_addr(&source, line))
                .map(|a| a + self.slide);
            let mut ok = false;
            let mut resolved_line = line;
            if let (Some(addr), Some(conn)) = (addr, self.conn.as_mut()) {
                ok = conn.set_breakpoint(addr).is_ok();
                if ok
                    && let Some((_, l)) = self
                        .info
                        .as_ref()
                        .and_then(|i| i.addr_to_line(addr - self.slide))
                {
                    resolved_line = l;
                }
            }
            self.breakpoints.push(Breakpoint {
                addr: addr.unwrap_or(0),
                source: source.clone(),
                verified: ok,
            });
            verified.push(serde_json::json!({
                "id": id,
                "verified": ok,
                "line": resolved_line,
            }));
        }
        Ok(vec![self.success_response(
            req,
            serde_json::json!({ "breakpoints": verified }),
        )])
    }

    fn clear_breakpoints_for(&mut self, source: &str) {
        let norm_path = |s: &str| s.replace('\\', "/").to_lowercase();
        let target = norm_path(source);
        let (removed, keep): (Vec<Breakpoint>, Vec<Breakpoint>) =
            std::mem::take(&mut self.breakpoints)
                .into_iter()
                .partition(|bp| norm_path(&bp.source) == target);
        for bp in removed {
            if bp.verified
                && bp.addr != 0
                && !keep.iter().any(|k| k.verified && k.addr == bp.addr)
                && let Some(conn) = self.conn.as_mut()
            {
                let _ = conn.remove_breakpoint(bp.addr);
            }
        }
        self.breakpoints = keep;
    }

    fn handle_configuration_done(&mut self, req: &ProtocolMessage) -> Result<Vec<ProtocolMessage>> {
        let response = self.success_response(req, serde_json::Value::Null);
        self.started = true;
        if self.conn.is_none() {
            return Ok(vec![response]);
        }
        if self.stop_on_entry {
            let stopped = self.event(
                "stopped",
                serde_json::json!({
                    "reason": "entry",
                    "threadId": 1,
                    "allThreadsStopped": true,
                }),
            );
            return Ok(vec![response, stopped]);
        }
        let mut out = vec![response];
        self.run_and_report("c", &mut out)?;
        Ok(out)
    }

    fn handle_threads(&mut self, req: &ProtocolMessage) -> Result<Vec<ProtocolMessage>> {
        let ids = match self.conn.as_mut() {
            Some(conn) => conn.thread_ids().unwrap_or_else(|_| vec![1]),
            None => vec![1],
        };
        let threads: Vec<_> = ids
            .iter()
            .map(|id| serde_json::json!({ "id": id, "name": format!("thread {id}") }))
            .collect();
        Ok(vec![self.success_response(
            req,
            serde_json::json!({ "threads": threads }),
        )])
    }

    fn handle_stack_trace(&mut self, req: &ProtocolMessage) -> Result<Vec<ProtocolMessage>> {
        let frames = self.build_frames().unwrap_or_default();
        let total = frames.len();
        Ok(vec![self.success_response(
            req,
            serde_json::json!({ "stackFrames": frames, "totalFrames": total }),
        )])
    }

    fn build_frames(&mut self) -> Result<Vec<serde_json::Value>> {
        let Some(conn) = self.conn.as_mut() else {
            return Ok(Vec::new());
        };
        let regs = conn.read_registers()?;
        let mut rip = amd64::reg(&regs, amd64::RIP).unwrap_or(0);
        let mut rbp = amd64::reg(&regs, amd64::RBP).unwrap_or(0);
        let mut frames = Vec::new();
        for depth in 0..64 {
            let file_addr = rip.wrapping_sub(self.slide);
            let name = self
                .info
                .as_ref()
                .and_then(|i| i.addr_to_func(file_addr))
                .unwrap_or_else(|| format!("{rip:#x}"));
            let mut frame = serde_json::json!({
                "id": depth + 1,
                "name": name,
                "line": 0,
                "column": 0,
            });
            if let Some((file, line)) = self.info.as_ref().and_then(|i| i.addr_to_line(file_addr)) {
                frame["line"] = serde_json::json!(line);
                frame["source"] = serde_json::json!({
                    "name": Path::new(&file).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| file.clone()),
                    "path": file,
                });
            }
            frames.push(frame);
            if rbp == 0 {
                break;
            }
            let Ok(saved) = conn.read_memory(rbp, 16) else {
                break;
            };
            if saved.len() < 16 {
                break;
            }
            let next_rbp = u64::from_le_bytes(saved[0..8].try_into().unwrap());
            let ret = u64::from_le_bytes(saved[8..16].try_into().unwrap());
            if ret == 0 || next_rbp <= rbp {
                break;
            }
            rip = ret;
            rbp = next_rbp;
            let _ = depth;
        }
        Ok(frames)
    }

    fn handle_scopes(&mut self, req: &ProtocolMessage) -> Result<Vec<ProtocolMessage>> {
        let scopes = serde_json::json!({
            "scopes": [{
                "name": "Registers",
                "variablesReference": REGISTERS_REF,
                "expensive": false,
            }]
        });
        Ok(vec![self.success_response(req, scopes)])
    }

    fn handle_variables(&mut self, req: &ProtocolMessage) -> Result<Vec<ProtocolMessage>> {
        let reference = req
            .arguments
            .get("variablesReference")
            .and_then(|v| v.as_i64())
            .unwrap_or(0);
        let mut variables = Vec::new();
        if reference == REGISTERS_REF
            && let Some(conn) = self.conn.as_mut()
            && let Ok(regs) = conn.read_registers()
        {
            for (idx, name) in REGISTER_NAMES.iter().enumerate() {
                if let Some(value) = amd64::reg(&regs, idx * 8) {
                    variables.push(serde_json::json!({
                        "name": name,
                        "value": format!("{value:#018x}"),
                        "variablesReference": 0,
                    }));
                }
            }
        }
        Ok(vec![self.success_response(
            req,
            serde_json::json!({ "variables": variables }),
        )])
    }

    fn handle_execution(
        &mut self,
        req: &ProtocolMessage,
        kind: ExecKind,
    ) -> Result<Vec<ProtocolMessage>> {
        if self.conn.is_none() || self.exited {
            return Ok(vec![
                self.error_response(req, "no live debuggee to control"),
            ]);
        }
        let body = match kind {
            ExecKind::Continue => serde_json::json!({ "allThreadsContinued": true }),
            _ => serde_json::Value::Null,
        };
        let mut out = vec![self.success_response(req, body)];
        match kind {
            ExecKind::Continue => self.run_and_report("c", &mut out)?,
            ExecKind::StepIn => self.source_step_and_report(false, &mut out)?,
            ExecKind::Next => self.source_step_and_report(true, &mut out)?,
            ExecKind::StepOut => self.step_out_and_report(&mut out)?,
        }
        Ok(out)
    }

    fn handle_pause(&mut self, req: &ProtocolMessage) -> Result<Vec<ProtocolMessage>> {
        let response = self.success_response(req, serde_json::Value::Null);
        if self.conn.is_some() && !self.exited {
            let stopped = self.event(
                "stopped",
                serde_json::json!({
                    "reason": "pause",
                    "threadId": 1,
                    "allThreadsStopped": true,
                }),
            );
            Ok(vec![response, stopped])
        } else {
            Ok(vec![response])
        }
    }

    fn handle_evaluate(&mut self, req: &ProtocolMessage) -> Result<Vec<ProtocolMessage>> {
        let expr = req
            .arguments
            .get("expression")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim()
            .to_lowercase();
        let value = REGISTER_NAMES
            .iter()
            .position(|n| *n == expr)
            .and_then(|idx| {
                let conn = self.conn.as_mut()?;
                let regs = conn.read_registers().ok()?;
                amd64::reg(&regs, idx * 8)
            });
        match value {
            Some(v) => Ok(vec![self.success_response(
                req,
                serde_json::json!({ "result": format!("{v:#018x}"), "variablesReference": 0 }),
            )]),
            None => {
                Ok(vec![self.error_response(
                    req,
                    "only register names can be evaluated",
                )])
            }
        }
    }

    fn run_and_report(&mut self, cmd: &str, out: &mut Vec<ProtocolMessage>) -> Result<()> {
        let (stop, output) = match self.conn.as_mut() {
            Some(conn) => conn.resume(cmd)?,
            None => return Ok(()),
        };
        self.emit_output(&output, out);
        self.report_stop(stop, "breakpoint", out);
        Ok(())
    }

    fn source_step_and_report(
        &mut self,
        step_over: bool,
        out: &mut Vec<ProtocolMessage>,
    ) -> Result<()> {
        let stop = self.source_step(step_over, out)?;
        let reason = if matches!(stop, Stop::Signal { signal: 5 }) && self.at_user_breakpoint() {
            "breakpoint"
        } else {
            "step"
        };
        self.report_stop(stop, reason, out);
        Ok(())
    }

    fn source_step(&mut self, step_over: bool, out: &mut Vec<ProtocolMessage>) -> Result<Stop> {
        let start = self.current_line();
        let start_sp = self.current_sp().unwrap_or(0);
        let start_range = self.current_func_range();
        let mut prev_in_func = true;
        for _ in 0..500_000 {
            let (stop, output) = match self.conn.as_mut() {
                Some(conn) => conn.resume("s")?,
                None => return Ok(Stop::Signal { signal: 5 }),
            };
            self.emit_output(&output, out);
            if !matches!(stop, Stop::Signal { signal: 5 }) {
                return Ok(stop);
            }
            if self.at_user_breakpoint() {
                return Ok(Stop::Signal { signal: 5 });
            }
            let sp = self.current_sp().unwrap_or(start_sp);
            let in_func = self.rip_in_range(start_range);
            if step_over && !in_func && sp < start_sp {
                if prev_in_func
                    && let Some(ret) = self.read_stack_u64(sp)
                    && let Some(stop) = self.finish_call(ret, start_sp, out)?
                {
                    return Ok(stop);
                }
                prev_in_func = self.rip_in_range(start_range);
                continue;
            }
            prev_in_func = in_func;
            let now = self.current_line();
            if now.is_some() && now != start {
                return Ok(Stop::Signal { signal: 5 });
            }
            if now.is_none() && sp > start_sp {
                return Ok(Stop::Signal { signal: 5 });
            }
        }
        Err(Error::Dap {
            detail: "single-step limit reached without completing the source step".into(),
        })
    }

    fn current_func_range(&mut self) -> Option<(u64, u64)> {
        let rip = self.current_rip()?;
        self.info.as_ref()?.func_range(rip.wrapping_sub(self.slide))
    }

    fn rip_in_range(&mut self, range: Option<(u64, u64)>) -> bool {
        let Some((low, high)) = range else {
            return false;
        };
        let Some(rip) = self.current_rip() else {
            return false;
        };
        let file_addr = rip.wrapping_sub(self.slide);
        file_addr >= low && file_addr < high
    }

    fn finish_call(
        &mut self,
        ret: u64,
        start_sp: u64,
        out: &mut Vec<ProtocolMessage>,
    ) -> Result<Option<Stop>> {
        let already = self.breakpoints.iter().any(|b| b.verified && b.addr == ret);
        let temp = match self.conn.as_mut() {
            Some(conn) if !already => conn.set_breakpoint(ret).is_ok().then_some(ret),
            _ => None,
        };
        if temp.is_none() && !already {
            return Ok(None);
        }
        let mut result = Ok(None);
        while let Some(conn) = self.conn.as_mut() {
            let (stop, output) = match conn.resume("c") {
                Ok(v) => v,
                Err(e) => {
                    result = Err(e);
                    break;
                }
            };
            self.emit_output(&output, out);
            if !matches!(stop, Stop::Signal { signal: 5 }) || self.at_user_breakpoint() {
                result = Ok(Some(stop));
                break;
            }
            if self.current_sp().unwrap_or(start_sp) >= start_sp {
                break;
            }
        }
        if let (Some(addr), Some(conn)) = (temp, self.conn.as_mut()) {
            let _ = conn.remove_breakpoint(addr);
        }
        result
    }

    fn at_user_breakpoint(&mut self) -> bool {
        let Some(rip) = self.current_rip() else {
            return false;
        };
        self.breakpoints.iter().any(|b| b.verified && b.addr == rip)
    }

    fn read_stack_u64(&mut self, addr: u64) -> Option<u64> {
        let conn = self.conn.as_mut()?;
        let mem = conn.read_memory(addr, 8).ok()?;
        Some(u64::from_le_bytes(mem.try_into().ok()?))
    }

    fn step_out_and_report(&mut self, out: &mut Vec<ProtocolMessage>) -> Result<()> {
        let ret = self.return_address();
        let temp = match (ret, self.conn.as_mut()) {
            (Some(ret), Some(conn)) if !self.breakpoints.iter().any(|b| b.addr == ret) => {
                conn.set_breakpoint(ret).is_ok().then_some(ret)
            }
            _ => None,
        };
        let (stop, output) = {
            let Some(conn) = self.conn.as_mut() else {
                self.report_stop(Stop::Signal { signal: 5 }, "step", out);
                return Ok(());
            };
            let result = conn.resume("c");
            if let Some(addr) = temp {
                let _ = conn.remove_breakpoint(addr);
            }
            result?
        };
        self.emit_output(&output, out);
        self.report_stop(stop, "step", out);
        Ok(())
    }

    fn current_line(&mut self) -> Option<(String, u32)> {
        let conn = self.conn.as_mut()?;
        let regs = conn.read_registers().ok()?;
        let rip = amd64::reg(&regs, amd64::RIP)?;
        self.info
            .as_ref()?
            .addr_to_line(rip.wrapping_sub(self.slide))
    }

    fn current_sp(&mut self) -> Option<u64> {
        let conn = self.conn.as_mut()?;
        let regs = conn.read_registers().ok()?;
        amd64::reg(&regs, amd64::RSP)
    }

    fn current_rip(&mut self) -> Option<u64> {
        let conn = self.conn.as_mut()?;
        let regs = conn.read_registers().ok()?;
        amd64::reg(&regs, amd64::RIP)
    }

    fn return_address(&mut self) -> Option<u64> {
        let conn = self.conn.as_mut()?;
        let regs = conn.read_registers().ok()?;
        let rbp = amd64::reg(&regs, amd64::RBP)?;
        if rbp == 0 {
            return None;
        }
        let mem = conn.read_memory(rbp + 8, 8).ok()?;
        Some(u64::from_le_bytes(mem.try_into().ok()?))
    }

    fn emit_output(&mut self, text: &str, out: &mut Vec<ProtocolMessage>) {
        if text.is_empty() {
            return;
        }
        let ev = self.event(
            "output",
            serde_json::json!({ "category": "stdout", "output": text }),
        );
        out.push(ev);
    }

    fn report_stop(&mut self, stop: Stop, reason: &str, out: &mut Vec<ProtocolMessage>) {
        match stop {
            Stop::Signal { signal: 5 } => {
                let ev = self.event(
                    "stopped",
                    serde_json::json!({
                        "reason": reason,
                        "threadId": 1,
                        "allThreadsStopped": true,
                    }),
                );
                out.push(ev);
            }
            Stop::Signal { signal } => {
                let ev = self.event(
                    "stopped",
                    serde_json::json!({
                        "reason": "exception",
                        "description": format!("signal {signal}"),
                        "threadId": 1,
                        "allThreadsStopped": true,
                    }),
                );
                out.push(ev);
            }
            Stop::Exited { code } => {
                self.exited = true;
                let exited = self.event("exited", serde_json::json!({ "exitCode": code }));
                let terminated = self.event("terminated", serde_json::Value::Null);
                out.push(exited);
                out.push(terminated);
            }
            Stop::Terminated { signal } => {
                self.exited = true;
                let exited = self.event(
                    "exited",
                    serde_json::json!({ "exitCode": 128 + signal as i32 }),
                );
                let terminated = self.event("terminated", serde_json::Value::Null);
                out.push(exited);
                out.push(terminated);
            }
        }
    }

    fn launch_backend(&mut self, program: &Path) -> Result<()> {
        self.shutdown();
        self.info = None;
        self.slide = 0;
        self.exited = false;
        self.breakpoints.clear();
        let result = self.spawn_backend(program);
        if result.is_err() {
            self.shutdown();
        }
        result
    }

    fn spawn_backend(&mut self, program: &Path) -> Result<()> {
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
        self.info = DebugInfo::load(&program).ok();
        let win_path = z_drive_path(&program);
        let mut command = Command::new(winedbg);
        lsw_runtime::scrub_wine_env(&mut command);
        command
            .args(["--gdb", "--no-start", &win_path])
            .env("WINEPREFIX", self.env.layout.prefix())
            .env("WINEDEBUG", "fixme-all")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped());
        let mut child = command
            .spawn()
            .map_err(|e| Error::io(PathBuf::from("winedbg"), e))?;
        let stderr = child.stderr.take().ok_or_else(|| Error::Dap {
            detail: "winedbg produced no stderr".into(),
        })?;
        self.backend = Some(child);
        let port = read_gdb_port(stderr)?;
        let mut conn = RspConn::connect(port)?;
        self.slide = compute_slide(&mut conn, self.info.as_ref());
        self.program = Some(program);
        self.conn = Some(conn);
        Ok(())
    }
}

enum ExecKind {
    Continue,
    Next,
    StepIn,
    StepOut,
}

fn z_drive_path(path: &Path) -> String {
    format!("Z:{}", path.to_string_lossy().replace('/', "\\"))
}

fn read_gdb_port<R: Read + Send + 'static>(stream: R) -> Result<u16> {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut reader = BufReader::new(stream);
        let mut forward = true;
        loop {
            let mut line = String::new();
            match reader.read_line(&mut line) {
                Ok(0) | Err(_) => break,
                Ok(_) => {
                    if forward && tx.send(line).is_err() {
                        forward = false;
                    }
                }
            }
        }
    });
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    loop {
        let remaining = deadline.saturating_duration_since(std::time::Instant::now());
        if remaining.is_zero() {
            break;
        }
        let Ok(line) = rx.recv_timeout(remaining) else {
            break;
        };
        if let Some(idx) = line.find("localhost:") {
            let digits: String = line[idx + "localhost:".len()..]
                .chars()
                .take_while(|c| c.is_ascii_digit())
                .collect();
            if let Ok(port) = digits.parse::<u16>() {
                return Ok(port);
            }
        }
    }
    Err(Error::Dap {
        detail: "winedbg did not report a gdb port (expected 'target remote localhost:<port>')"
            .into(),
    })
}

fn compute_slide(conn: &mut RspConn, info: Option<&DebugInfo>) -> u64 {
    let Some(info) = info else { return 0 };
    if let Ok(head) = conn.read_memory(info.image_base, 2)
        && head == b"MZ"
    {
        return 0;
    }
    0
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
