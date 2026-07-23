use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Duration;

use crate::error::{Error, Result};

const COMMAND_TIMEOUT: Duration = Duration::from_secs(15);
const MAX_RESUME_OUTPUT: usize = 1 << 20;
const MAX_PACKET_BYTES: usize = 16 << 20;

fn dap(detail: impl Into<String>) -> Error {
    Error::Dap {
        detail: detail.into(),
    }
}

fn output_packet(reply: &[u8]) -> Option<Vec<u8>> {
    let rest = reply.strip_prefix(b"O")?;
    if rest.is_empty() || !rest.iter().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    hex_to_bytes(std::str::from_utf8(rest).ok()?)
}

pub(crate) fn checksum(payload: &[u8]) -> u8 {
    payload.iter().fold(0u8, |acc, b| acc.wrapping_add(*b))
}

pub(crate) fn encode_packet(payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(payload.len() + 4);
    out.push(b'$');
    out.extend_from_slice(payload);
    out.push(b'#');
    out.extend_from_slice(format!("{:02x}", checksum(payload)).as_bytes());
    out
}

pub(crate) fn hex_to_bytes(s: &str) -> Option<Vec<u8>> {
    let bytes = s.as_bytes();
    if !bytes.len().is_multiple_of(2) {
        return None;
    }
    let mut out = Vec::with_capacity(bytes.len() / 2);
    for pair in bytes.chunks(2) {
        let hi = (pair[0] as char).to_digit(16)?;
        let lo = (pair[1] as char).to_digit(16)?;
        out.push((hi * 16 + lo) as u8);
    }
    Some(out)
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Stop {
    Signal { signal: u8 },
    Exited { code: i32 },
    Terminated { signal: u8 },
}

fn signal_byte(payload: &[u8]) -> Option<u8> {
    let hex = payload.get(1..3)?;
    hex_to_bytes(std::str::from_utf8(hex).ok()?)?
        .first()
        .copied()
}

pub(crate) fn parse_stop(payload: &[u8]) -> Option<Stop> {
    match payload.first()? {
        b'S' | b'T' => signal_byte(payload).map(|signal| Stop::Signal { signal }),
        b'W' => {
            let code = std::str::from_utf8(payload.get(1..)?).ok()?;
            let code = code.split(';').next().unwrap_or("0");
            i64::from_str_radix(code, 16)
                .ok()
                .map(|c| Stop::Exited { code: c as i32 })
        }
        b'X' => signal_byte(payload).map(|signal| Stop::Terminated { signal }),
        _ => None,
    }
}

pub(crate) struct RspConn {
    stream: TcpStream,
    no_ack: bool,
}

impl RspConn {
    pub(crate) fn connect(port: u16) -> Result<Self> {
        let stream = TcpStream::connect(("127.0.0.1", port)).map_err(|e| {
            dap(format!(
                "cannot connect to the wine gdb stub on port {port}: {e}"
            ))
        })?;
        Ok(Self {
            stream,
            no_ack: false,
        })
    }

    fn set_timeout(&self, d: Option<Duration>) {
        let _ = self.stream.set_read_timeout(d);
    }

    fn read_byte(&mut self) -> Result<u8> {
        let mut b = [0u8; 1];
        self.stream
            .read_exact(&mut b)
            .map_err(|e| dap(format!("gdb stub read failed: {e}")))?;
        Ok(b[0])
    }

    fn send(&mut self, payload: &[u8]) -> Result<()> {
        let pkt = encode_packet(payload);
        self.stream
            .write_all(&pkt)
            .map_err(|e| dap(format!("gdb stub write failed: {e}")))?;
        self.stream
            .flush()
            .map_err(|e| dap(format!("gdb stub flush failed: {e}")))?;
        if !self.no_ack {
            let ack = self.read_byte()?;
            if ack == b'-' {
                return Err(dap("gdb stub rejected the packet (nak)"));
            }
        }
        Ok(())
    }

    fn recv(&mut self) -> Result<Vec<u8>> {
        loop {
            let mut b = self.read_byte()?;
            while b != b'$' {
                b = self.read_byte()?;
            }
            let mut raw = Vec::new();
            let mut payload = Vec::new();
            loop {
                let c = self.read_byte()?;
                if c == b'#' {
                    break;
                }
                if raw.len() >= MAX_PACKET_BYTES {
                    return Err(dap("gdb stub sent an oversized packet"));
                }
                raw.push(c);
                if c == b'}' {
                    let esc = self.read_byte()?;
                    raw.push(esc);
                    payload.push(esc ^ 0x20);
                } else {
                    payload.push(c);
                }
            }
            let hi = self.read_byte()?;
            let lo = self.read_byte()?;
            let want = format!("{hi}{lo}", hi = hi as char, lo = lo as char);
            let got = format!("{:02x}", checksum(&raw));
            let valid = want.eq_ignore_ascii_case(&got);
            if !self.no_ack {
                self.stream.write_all(if valid { b"+" } else { b"-" }).ok();
                self.stream.flush().ok();
            }
            if valid {
                return Ok(payload);
            }
        }
    }

    pub(crate) fn command(&mut self, cmd: &str) -> Result<Vec<u8>> {
        self.set_timeout(Some(COMMAND_TIMEOUT));
        self.send(cmd.as_bytes())?;
        loop {
            let reply = self.recv()?;
            if output_packet(&reply).is_some() {
                continue;
            }
            return Ok(reply);
        }
    }

    pub(crate) fn resume(&mut self, cmd: &str) -> Result<(Stop, String)> {
        self.set_timeout(Some(COMMAND_TIMEOUT));
        self.send(cmd.as_bytes())?;
        self.set_timeout(None);
        let mut output = String::new();
        loop {
            let reply = self.recv()?;
            if let Some(bytes) = output_packet(&reply) {
                if output.len() < MAX_RESUME_OUTPUT {
                    output.push_str(&String::from_utf8_lossy(&bytes));
                }
                continue;
            }
            let stop = parse_stop(&reply).ok_or_else(|| {
                dap(format!(
                    "unexpected stop reply: {}",
                    String::from_utf8_lossy(&reply)
                ))
            })?;
            return Ok((stop, output));
        }
    }

    pub(crate) fn read_registers(&mut self) -> Result<Vec<u8>> {
        let reply = self.command("g")?;
        hex_to_bytes(std::str::from_utf8(&reply).unwrap_or(""))
            .ok_or_else(|| dap("gdb stub returned malformed register data"))
    }

    pub(crate) fn read_memory(&mut self, addr: u64, len: usize) -> Result<Vec<u8>> {
        let reply = self.command(&format!("m{addr:x},{len:x}"))?;
        if reply.first() == Some(&b'E') {
            return Err(dap(format!(
                "cannot read {len} bytes at {addr:#x}: {}",
                String::from_utf8_lossy(&reply)
            )));
        }
        hex_to_bytes(std::str::from_utf8(&reply).unwrap_or(""))
            .ok_or_else(|| dap("gdb stub returned malformed memory data"))
    }

    pub(crate) fn set_breakpoint(&mut self, addr: u64) -> Result<()> {
        self.expect_ok(&format!("Z1,{addr:x},1"))
    }

    pub(crate) fn remove_breakpoint(&mut self, addr: u64) -> Result<()> {
        self.expect_ok(&format!("z1,{addr:x},1"))
    }

    fn expect_ok(&mut self, cmd: &str) -> Result<()> {
        let reply = self.command(cmd)?;
        if reply == b"OK" {
            Ok(())
        } else {
            Err(dap(format!(
                "gdb stub rejected '{cmd}': {}",
                String::from_utf8_lossy(&reply)
            )))
        }
    }

    pub(crate) fn thread_ids(&mut self) -> Result<Vec<i64>> {
        let mut out = Vec::new();
        let mut reply = self.command("qfThreadInfo")?;
        while let Some(b'm') = reply.first() {
            for part in std::str::from_utf8(&reply[1..]).unwrap_or("").split(',') {
                if let Some(id) = parse_thread_id(part) {
                    out.push(id);
                }
            }
            reply = self.command("qsThreadInfo")?;
        }
        if out.is_empty() {
            out.push(1);
        }
        Ok(out)
    }

    pub(crate) fn select_thread(&mut self, id: i64) {
        let _ = self.command(&format!("Hg{id:x}"));
    }

    pub(crate) fn current_thread(&mut self) -> Option<i64> {
        let reply = self.command("qC").ok()?;
        let text = std::str::from_utf8(&reply).ok()?;
        parse_thread_id(text.strip_prefix("QC").unwrap_or(text))
    }

    pub(crate) fn kill(&mut self) {
        let pkt = encode_packet(b"k");
        let _ = self.stream.write_all(&pkt);
        let _ = self.stream.flush();
    }
}

fn parse_thread_id(s: &str) -> Option<i64> {
    let s = s.trim();
    let s = s.rsplit('.').next().unwrap_or(s);
    let s = s.strip_prefix('p').unwrap_or(s);
    if s.is_empty() || s == "-1" {
        return None;
    }
    i64::from_str_radix(s, 16).ok()
}

pub(crate) mod amd64 {
    pub const RBP: usize = 6 * 8;
    pub const RSP: usize = 7 * 8;
    pub const RIP: usize = 16 * 8;

    pub fn reg(regs: &[u8], offset: usize) -> Option<u64> {
        let slice = regs.get(offset..offset + 8)?;
        Some(u64::from_le_bytes(slice.try_into().ok()?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checksum_and_packet_framing() {
        assert_eq!(checksum(b"OK"), (b'O' as u32 + b'K' as u32) as u8);
        let pkt = encode_packet(b"g");
        assert_eq!(pkt, b"$g#67");
    }

    #[test]
    fn hex_roundtrip() {
        assert_eq!(hex_to_bytes("48656c6c6f").unwrap(), b"Hello");
        assert!(hex_to_bytes("abc").is_none());
    }

    #[test]
    fn parse_stop_replies() {
        assert_eq!(parse_stop(b"S05"), Some(Stop::Signal { signal: 5 }));
        assert_eq!(
            parse_stop(b"T05thread:01;rip:0011223344556677;"),
            Some(Stop::Signal { signal: 5 })
        );
        assert_eq!(parse_stop(b"W00"), Some(Stop::Exited { code: 0 }));
        assert_eq!(parse_stop(b"W2a"), Some(Stop::Exited { code: 42 }));
        assert_eq!(parse_stop(b"X0b"), Some(Stop::Terminated { signal: 11 }));
    }

    #[test]
    fn parse_stop_truncated_does_not_panic() {
        for p in [&b"S"[..], b"S0", b"T", b"T0", b"X", b"X0", b"W", b""] {
            assert_eq!(parse_stop(p), None);
        }
    }

    #[test]
    fn output_packet_distinguishes_ok() {
        assert_eq!(output_packet(b"OK"), None);
        assert_eq!(output_packet(b"O4869"), Some(b"Hi".to_vec()));
        assert_eq!(output_packet(b"O"), None);
    }

    #[test]
    fn amd64_register_extraction() {
        let mut regs = vec![0u8; 20 * 8];
        regs[amd64::RIP..amd64::RIP + 8].copy_from_slice(&0x1400013f4u64.to_le_bytes());
        regs[amd64::RSP..amd64::RSP + 8].copy_from_slice(&0x33fd80u64.to_le_bytes());
        assert_eq!(amd64::reg(&regs, amd64::RIP), Some(0x1400013f4));
        assert_eq!(amd64::reg(&regs, amd64::RSP), Some(0x33fd80));
    }

    #[test]
    fn thread_id_parsing() {
        assert_eq!(parse_thread_id("p1.1f"), Some(0x1f));
        assert_eq!(parse_thread_id("2a"), Some(0x2a));
        assert_eq!(parse_thread_id("-1"), None);
    }
}
