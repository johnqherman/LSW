use std::fmt::Write as _;
use std::io::Write;
use std::process::{Command, Stdio};

use base64::Engine as _;

use crate::error::{Error, Result};

const MAX_WINRM_BYTES: usize = 32 * 1024 * 1024;
const MAX_RECEIVE_OUTPUT: usize = 64 * 1024 * 1024;
const MAX_UPLOAD_BYTES: u64 = 512 * 1024 * 1024;

use crate::project::Project;
use crate::verifyops::{
    self, AgentResult, VerifyReport, VerifyStatus, default_remote_dir, validate_windows_dir,
    validate_windows_name,
};
use lsw_toolchain::drain_capped;

const NS: &str = "xmlns:s=\"http://www.w3.org/2003/05/soap-envelope\" \
     xmlns:a=\"http://schemas.xmlsoap.org/ws/2004/08/addressing\" \
     xmlns:w=\"http://schemas.dmtf.org/wbem/wsman/1/wsman.xsd\" \
     xmlns:rsp=\"http://schemas.microsoft.com/wbem/wsman/1/windows/shell\"";
const SHELL_URI: &str = "http://schemas.microsoft.com/wbem/wsman/1/windows/shell/cmd";
const ANON: &str = "http://schemas.xmlsoap.org/ws/2004/08/addressing/role/anonymous";
const B64: base64::engine::GeneralPurpose = base64::engine::general_purpose::STANDARD;

struct Winrm {
    addr: String,
    user: String,
    password: String,
    counter: std::cell::Cell<u64>,
}

impl Winrm {
    fn from_project(project: &Project) -> Result<Option<Winrm>> {
        let cfg = &project.manifest.verify;
        let Some(host) = cfg.host.clone() else {
            return Ok(None);
        };
        if crate::buildops::which("curl").is_none() {
            return Err(Error::ToolMissing {
                tool: "curl".into(),
                fix: "install curl to reach the Windows verification host over WinRM".into(),
            });
        }
        let force_https = cfg.transport.as_deref() == Some("https");
        let (user, hostport) = match host.split_once('@') {
            Some((u, h)) => (u.to_owned(), h.to_owned()),
            None => ("Administrator".to_owned(), host.clone()),
        };
        let default_port = if force_https { "5986" } else { "5985" };
        let (hostname, port) = match hostport.rsplit_once(':') {
            Some((h, p)) if p.chars().all(|c| c.is_ascii_digit()) => (h.to_owned(), p.to_owned()),
            _ => (hostport.clone(), default_port.to_owned()),
        };
        let scheme = if force_https || port == "5986" {
            "https"
        } else {
            "http"
        };
        let password = std::env::var("LSW_WINRM_PASSWORD").map_err(|_| Error::ProbeFailed {
            host: host.clone(),
            detail: "set LSW_WINRM_PASSWORD in the environment for WinRM auth".into(),
        })?;
        Ok(Some(Winrm {
            addr: format!("{scheme}://{hostname}:{port}/wsman"),
            user,
            password,
            counter: std::cell::Cell::new(0),
        }))
    }

    fn message_id(&self) -> String {
        let n = self.counter.get() + 1;
        self.counter.set(n);
        format!("uuid:00000000-0000-0000-0000-{n:012x}")
    }

    fn header(&self, action: &str) -> String {
        format!(
            "<a:To>{addr}</a:To>\
             <w:ResourceURI s:mustUnderstand=\"true\">{SHELL_URI}</w:ResourceURI>\
             <a:ReplyTo><a:Address s:mustUnderstand=\"true\">{ANON}</a:Address></a:ReplyTo>\
             <a:Action s:mustUnderstand=\"true\">{action}</a:Action>\
             <w:MaxEnvelopeSize s:mustUnderstand=\"true\">512000</w:MaxEnvelopeSize>\
             <a:MessageID>{mid}</a:MessageID>\
             <w:Locale xml:lang=\"en-US\" s:mustUnderstand=\"false\"/>\
             <w:OperationTimeout>PT120S</w:OperationTimeout>",
            addr = crate::xml_escape(&self.addr),
            mid = self.message_id(),
        )
    }

    fn post(&self, envelope: &str) -> Result<String> {
        let mut child = Command::new("curl")
            .args([
                "-s",
                "--max-time",
                "180",
                "--max-filesize",
                "33554432",
                "-u",
                &format!("{}:{}", self.user, self.password),
                "-X",
                "POST",
                &self.addr,
                "-H",
                "Content-Type: application/soap+xml;charset=UTF-8",
                "--data-binary",
                "@-",
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| Error::io(std::path::PathBuf::from("curl"), e))?;
        let out_rx = child
            .stdout
            .take()
            .map(|s| drain_capped(s, MAX_WINRM_BYTES as u64));
        let err_rx = child
            .stderr
            .take()
            .map(|s| drain_capped(s, MAX_WINRM_BYTES as u64));
        let write_res = child
            .stdin
            .take()
            .expect("piped stdin")
            .write_all(envelope.as_bytes());
        let status = child
            .wait()
            .map_err(|e| Error::io(std::path::PathBuf::from("curl"), e))?;
        write_res.map_err(|e| Error::io(std::path::PathBuf::from("curl"), e))?;
        let stdout = out_rx
            .map(lsw_toolchain::Drain::wait_eof)
            .unwrap_or_default();
        let stderr = err_rx
            .map(lsw_toolchain::Drain::wait_eof)
            .unwrap_or_default();
        let body = String::from_utf8_lossy(&stdout).into_owned();
        if !status.success() && body.is_empty() {
            return Err(Error::ProbeFailed {
                host: self.addr.clone(),
                detail: String::from_utf8_lossy(&stderr).trim().to_owned(),
            });
        }
        Ok(body)
    }

    fn selector(shell: &str) -> String {
        format!("<w:SelectorSet><w:Selector Name=\"ShellId\">{shell}</w:Selector></w:SelectorSet>")
    }

    fn create_shell(&self) -> Result<String> {
        let env = format!(
            "<s:Envelope {NS}><s:Header>{hdr}\
             <w:OptionSet><w:Option Name=\"WINRS_CODEPAGE\">65001</w:Option></w:OptionSet>\
             </s:Header><s:Body><rsp:Shell><rsp:InputStreams>stdin</rsp:InputStreams>\
             <rsp:OutputStreams>stdout stderr</rsp:OutputStreams></rsp:Shell></s:Body></s:Envelope>",
            hdr = self.header("http://schemas.xmlsoap.org/ws/2004/09/transfer/Create"),
        );
        let resp = self.post(&env)?;
        extract(&resp, "<rsp:ShellId>", "<")
            .map(|s| crate::xml_escape(&s))
            .ok_or_else(|| Error::ProbeFailed {
                host: self.addr.clone(),
                detail: format!("WinRM did not return a ShellId: {}", first_fault(&resp)),
            })
    }

    fn command(&self, shell: &str, program: &str, args: &[&str], skip_cmd: bool) -> Result<String> {
        let mut body = format!(
            "<rsp:CommandLine><rsp:Command>{}</rsp:Command>",
            crate::xml_escape(program)
        );
        for a in args {
            let _ = write!(
                body,
                "<rsp:Arguments>{}</rsp:Arguments>",
                crate::xml_escape(a)
            );
        }
        body.push_str("</rsp:CommandLine>");
        let opt = format!(
            "<w:OptionSet><w:Option Name=\"WINRS_CONSOLEMODE_STDIN\">TRUE</w:Option>\
             <w:Option Name=\"WINRS_SKIP_CMD_SHELL\">{}</w:Option></w:OptionSet>",
            if skip_cmd { "TRUE" } else { "FALSE" }
        );
        let env = format!(
            "<s:Envelope {NS}><s:Header>{hdr}{sel}{opt}</s:Header><s:Body>{body}</s:Body></s:Envelope>",
            hdr = self.header("http://schemas.microsoft.com/wbem/wsman/1/windows/shell/Command"),
            sel = Self::selector(shell),
        );
        let resp = self.post(&env)?;
        extract(&resp, "<rsp:CommandId>", "<")
            .map(|s| crate::xml_escape(&s))
            .ok_or_else(|| Error::ProbeFailed {
                host: self.addr.clone(),
                detail: format!("WinRM did not return a CommandId: {}", first_fault(&resp)),
            })
    }

    fn send_stdin(&self, shell: &str, command: &str, bytes: &[u8]) -> Result<()> {
        for chunk in bytes.chunks(96 * 1024) {
            let env = format!(
                "<s:Envelope {NS}><s:Header>{hdr}{sel}</s:Header><s:Body><rsp:Send>\
                 <rsp:Stream Name=\"stdin\" CommandId=\"{command}\">{data}</rsp:Stream>\
                 </rsp:Send></s:Body></s:Envelope>",
                hdr = self.header("http://schemas.microsoft.com/wbem/wsman/1/windows/shell/Send"),
                sel = Self::selector(shell),
                data = B64.encode(chunk),
            );
            self.post(&env)?;
        }
        let end = format!(
            "<s:Envelope {NS}><s:Header>{hdr}{sel}</s:Header><s:Body><rsp:Send>\
             <rsp:Stream Name=\"stdin\" CommandId=\"{command}\" End=\"true\"></rsp:Stream>\
             </rsp:Send></s:Body></s:Envelope>",
            hdr = self.header("http://schemas.microsoft.com/wbem/wsman/1/windows/shell/Send"),
            sel = Self::selector(shell),
        );
        self.post(&end)?;
        Ok(())
    }

    fn receive(&self, shell: &str, command: &str) -> Result<(String, String, Option<i32>)> {
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        for _ in 0..600 {
            let env = format!(
                "<s:Envelope {NS}><s:Header>{hdr}{sel}</s:Header><s:Body><rsp:Receive>\
                 <rsp:DesiredStream CommandId=\"{command}\">stdout stderr</rsp:DesiredStream>\
                 </rsp:Receive></s:Body></s:Envelope>",
                hdr =
                    self.header("http://schemas.microsoft.com/wbem/wsman/1/windows/shell/Receive"),
                sel = Self::selector(shell),
            );
            let resp = self.post(&env)?;
            collect_streams(&resp, "stdout", &mut stdout);
            collect_streams(&resp, "stderr", &mut stderr);
            if stdout.len().saturating_add(stderr.len()) > MAX_RECEIVE_OUTPUT {
                return Ok((
                    String::from_utf8_lossy(&stdout).into_owned(),
                    String::from_utf8_lossy(&stderr).into_owned(),
                    None,
                ));
            }
            if resp.contains("CommandState/Done") {
                let exit =
                    extract(&resp, "<rsp:ExitCode>", "<").and_then(|c| c.trim().parse().ok());
                return Ok((
                    String::from_utf8_lossy(&stdout).into_owned(),
                    String::from_utf8_lossy(&stderr).into_owned(),
                    exit,
                ));
            }
        }
        Err(Error::ProbeFailed {
            host: self.addr.clone(),
            detail: "WinRM command did not finish within the timeout".into(),
        })
    }

    fn delete_shell(&self, shell: &str) {
        let env = format!(
            "<s:Envelope {NS}><s:Header>{hdr}{sel}</s:Header><s:Body></s:Body></s:Envelope>",
            hdr = self.header("http://schemas.xmlsoap.org/ws/2004/09/transfer/Delete"),
            sel = Self::selector(shell),
        );
        let _ = self.post(&env);
    }

    fn exec(
        &self,
        shell: &str,
        program: &str,
        args: &[&str],
        skip_cmd: bool,
    ) -> Result<(String, String, Option<i32>)> {
        let command = self.command(shell, program, args, skip_cmd)?;
        self.receive(shell, &command)
    }

    fn upload(&self, shell: &str, remote: &str, bytes: &[u8]) -> Result<()> {
        let script = format!(
            "$i=[Console]::OpenStandardInput();$o=[IO.File]::Create('{remote}');$i.CopyTo($o);$o.Close()"
        );
        let command = self.command(
            shell,
            "powershell",
            &["-NoProfile", "-Command", &script],
            true,
        )?;
        self.send_stdin(shell, &command, bytes)?;
        let (_, stderr, exit) = self.receive(shell, &command)?;
        if exit != Some(0) {
            return Err(Error::ProbeFailed {
                host: self.addr.clone(),
                detail: format!("upload of {remote} failed: {}", stderr.trim()),
            });
        }
        Ok(())
    }
}

/// Run on host.
pub fn run_on_host(
    project: &Project,
    artifacts: &[std::path::PathBuf],
    args: &[String],
) -> Result<VerifyReport> {
    let Some(winrm) = Winrm::from_project(project)? else {
        return Ok(VerifyReport {
            status: VerifyStatus::WindowsUnavailable,
            host: None,
            results: Vec::new(),
            detail: "no [verify] host configured in lsw.toml".into(),
        });
    };
    let cfg = &project.manifest.verify;
    let remote_dir = cfg
        .remote_dir
        .clone()
        .unwrap_or_else(|| default_remote_dir(project));
    validate_windows_dir(&remote_dir)?;
    let plan = verifyops::plan(project, artifacts, &remote_dir);
    for (_, name) in &plan.uploads {
        validate_windows_name(name)?;
    }

    let shell = winrm.create_shell()?;
    let result: Result<(Vec<AgentResult>, bool)> = (|| {
        let mkdir = format!("New-Item -ItemType Directory -Force -Path '{remote_dir}' | Out-Null");
        winrm.exec(
            &shell,
            "powershell",
            &["-NoProfile", "-Command", &mkdir],
            true,
        )?;
        for (local, name) in &plan.uploads {
            use std::io::Read;
            let file = std::fs::File::open(local).map_err(|e| Error::io(local.clone(), e))?;
            let mut bytes = Vec::new();
            file.take(MAX_UPLOAD_BYTES + 1)
                .read_to_end(&mut bytes)
                .map_err(|e| Error::io(local.clone(), e))?;
            if bytes.len() as u64 > MAX_UPLOAD_BYTES {
                return Err(Error::io(
                    local.clone(),
                    std::io::Error::other(format!(
                        "artifact exceeds upload limit of {MAX_UPLOAD_BYTES} bytes"
                    )),
                ));
            }
            winrm.upload(&shell, &format!("{remote_dir}\\{name}"), &bytes)?;
        }
        let mut results = Vec::new();
        let mut all_passed = true;
        let arg_refs: Vec<&str> = args.iter().map(String::as_str).collect();
        for program in &plan.run {
            let (stdout, stderr, exit) =
                winrm.exec(&shell, &format!("{remote_dir}\\{program}"), &arg_refs, true)?;
            if exit != Some(0) {
                all_passed = false;
            }
            results.push(AgentResult {
                artifact: program.clone(),
                exit_code: exit,
                stdout,
                stderr,
                dump: None,
            });
        }
        Ok((results, all_passed))
    })();
    winrm.delete_shell(&shell);
    let (results, all_passed) = result?;

    Ok(verifyops::finish_report(winrm.addr, results, all_passed))
}

fn extract(haystack: &str, start: &str, end: &str) -> Option<String> {
    let i = haystack.find(start)? + start.len();
    let rest = &haystack[i..];
    let j = rest.find(end)?;
    Some(rest[..j].to_owned())
}

fn first_fault(resp: &str) -> String {
    extract(resp, "<f:Message>", "</f:Message>")
        .or_else(|| extract(resp, "<s:Text", "</s:Text>").map(|t| t.trim_start_matches('>').into()))
        .unwrap_or_else(|| "no fault detail".into())
}

fn collect_streams(resp: &str, name: &str, out: &mut Vec<u8>) {
    let marker = format!("Name=\"{name}\"");
    let mut cursor = 0;
    while let Some(rel) = resp[cursor..].find("<rsp:Stream ") {
        let start = cursor + rel;
        let Some(close_rel) = resp[start..].find('>') else {
            break;
        };
        let open_end = start + close_rel + 1;
        let Some(end_rel) = resp[open_end..].find("</rsp:Stream>") else {
            break;
        };
        let content_end = open_end + end_rel;
        let tag = &resp[start..open_end];
        let content = &resp[open_end..content_end];
        if tag.contains(&marker)
            && !content.is_empty()
            && let Ok(bytes) = B64.decode(content.trim())
        {
            out.extend_from_slice(&bytes);
        }
        cursor = content_end + "</rsp:Stream>".len();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_finds_value_between_markers() {
        let xml = "<root><rsp:ShellId>abc-123</rsp:ShellId></root>";
        assert_eq!(extract(xml, "<rsp:ShellId>", "<"), Some("abc-123".into()));
    }

    #[test]
    fn extract_returns_none_when_start_missing() {
        assert_eq!(extract("<a>b</a>", "<c>", "<"), None);
    }

    #[test]
    fn extract_returns_none_when_end_missing() {
        assert_eq!(extract("<a>b", "<a>", "<z>"), None);
    }

    #[test]
    fn extract_empty_value() {
        assert_eq!(extract("<a><b></b>", "<b>", "<"), Some(String::new()));
    }

    #[test]
    fn first_fault_prefers_message() {
        let resp = "<f:Message>access denied</f:Message><s:Text>other</s:Text>";
        assert_eq!(first_fault(resp), "access denied");
    }

    #[test]
    fn first_fault_falls_back_to_text() {
        let resp = "<s:Text>connection refused</s:Text>";
        assert_eq!(first_fault(resp), "connection refused");
    }

    #[test]
    fn first_fault_no_detail() {
        assert_eq!(first_fault("<ok/>"), "no fault detail");
    }

    #[test]
    fn collect_streams_decodes_stdout() {
        let encoded = B64.encode(b"hello world");
        let resp = format!(
            "<rsp:Stream Name=\"stdout\" CommandId=\"c1\">{encoded}</rsp:Stream>"
        );
        let mut out = Vec::new();
        collect_streams(&resp, "stdout", &mut out);
        assert_eq!(out, b"hello world");
    }

    #[test]
    fn collect_streams_ignores_other_name() {
        let encoded = B64.encode(b"err data");
        let resp = format!(
            "<rsp:Stream Name=\"stderr\" CommandId=\"c1\">{encoded}</rsp:Stream>"
        );
        let mut out = Vec::new();
        collect_streams(&resp, "stdout", &mut out);
        assert!(out.is_empty());
    }

    #[test]
    fn collect_streams_multiple_chunks() {
        let e1 = B64.encode(b"aaa");
        let e2 = B64.encode(b"bbb");
        let resp = format!(
            "<rsp:Stream Name=\"stdout\" CommandId=\"c1\">{e1}</rsp:Stream>\
             <rsp:Stream Name=\"stdout\" CommandId=\"c1\">{e2}</rsp:Stream>"
        );
        let mut out = Vec::new();
        collect_streams(&resp, "stdout", &mut out);
        assert_eq!(out, b"aaabbb");
    }

    #[test]
    fn collect_streams_skips_empty_content() {
        let resp = "<rsp:Stream Name=\"stdout\" CommandId=\"c1\"></rsp:Stream>";
        let mut out = Vec::new();
        collect_streams(resp, "stdout", &mut out);
        assert!(out.is_empty());
    }

    #[test]
    fn selector_formats_shell_id() {
        let sel = Winrm::selector("shell-42");
        assert!(sel.contains("ShellId"));
        assert!(sel.contains("shell-42"));
    }

    #[test]
    fn message_id_increments() {
        let w = Winrm {
            addr: "http://host:5985/wsman".into(),
            user: "admin".into(),
            password: "pass".into(),
            counter: std::cell::Cell::new(0),
        };
        let id1 = w.message_id();
        let id2 = w.message_id();
        assert_ne!(id1, id2);
        assert!(id1.contains("000000000001"));
        assert!(id2.contains("000000000002"));
    }

    #[test]
    fn header_contains_action_and_address() {
        let w = Winrm {
            addr: "http://host:5985/wsman".into(),
            user: "admin".into(),
            password: "pass".into(),
            counter: std::cell::Cell::new(0),
        };
        let hdr = w.header("http://example.com/Action");
        assert!(hdr.contains("http://host:5985/wsman"));
        assert!(hdr.contains("http://example.com/Action"));
        assert!(hdr.contains("PT120S"));
    }

    #[test]
    fn header_escapes_address() {
        let w = Winrm {
            addr: "http://host&co:5985/wsman".into(),
            user: "admin".into(),
            password: "pass".into(),
            counter: std::cell::Cell::new(0),
        };
        let hdr = w.header("test");
        assert!(hdr.contains("host&amp;co"));
    }
}
