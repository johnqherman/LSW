use std::io::{Read, Write};
use std::process::{Command, Stdio};
use std::sync::mpsc;

use base64::Engine as _;

use crate::error::{Error, Result};

const MAX_WINRM_BYTES: usize = 32 * 1024 * 1024;
const MAX_RECEIVE_OUTPUT: usize = 64 * 1024 * 1024;
const MAX_UPLOAD_BYTES: u64 = 512 * 1024 * 1024;

fn drain_capped(mut reader: impl Read + Send + 'static, cap: usize) -> mpsc::Receiver<Vec<u8>> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut buf = Vec::new();
        let mut chunk = [0u8; 8192];
        loop {
            match reader.read(&mut chunk) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    if buf.len() < cap {
                        let take = (cap - buf.len()).min(n);
                        buf.extend_from_slice(&chunk[..take]);
                    }
                }
            }
        }
        let _ = tx.send(buf);
    });
    rx
}
use crate::project::Project;
use crate::verifyops::{
    self, AgentResult, VerifyReport, VerifyStatus, default_remote_dir, validate_windows_dir,
    validate_windows_name,
};

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
        if verifyops_which("curl").is_none() {
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
            addr = self.addr,
            mid = self.message_id(),
        )
    }

    fn post(&self, envelope: &str) -> Result<String> {
        let mut child = Command::new("curl")
            .args([
                "-s",
                "-k",
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
            .map(|s| drain_capped(s, MAX_WINRM_BYTES));
        let err_rx = child
            .stderr
            .take()
            .map(|s| drain_capped(s, MAX_WINRM_BYTES));
        let write_res = child
            .stdin
            .take()
            .expect("piped stdin")
            .write_all(envelope.as_bytes());
        let status = child
            .wait()
            .map_err(|e| Error::io(std::path::PathBuf::from("curl"), e))?;
        write_res.map_err(|e| Error::io(std::path::PathBuf::from("curl"), e))?;
        let stdout = out_rx.and_then(|rx| rx.recv().ok()).unwrap_or_default();
        let stderr = err_rx.and_then(|rx| rx.recv().ok()).unwrap_or_default();
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
            .map(|s| xml_attr(&s))
            .ok_or_else(|| Error::ProbeFailed {
                host: self.addr.clone(),
                detail: format!("WinRM did not return a ShellId: {}", first_fault(&resp)),
            })
    }

    fn command(&self, shell: &str, program: &str, args: &[&str], skip_cmd: bool) -> Result<String> {
        let mut body = format!(
            "<rsp:CommandLine><rsp:Command>{}</rsp:Command>",
            xml(program)
        );
        for a in args {
            body.push_str(&format!("<rsp:Arguments>{}</rsp:Arguments>", xml(a)));
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
            .map(|s| xml_attr(&s))
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

fn verifyops_which(program: &str) -> Option<std::path::PathBuf> {
    crate::buildops::which(program)
}

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

    let status = if results.is_empty() {
        VerifyStatus::WindowsUnavailable
    } else if all_passed {
        VerifyStatus::WindowsVerified
    } else {
        VerifyStatus::WindowsFailed
    };
    let detail = match status {
        VerifyStatus::WindowsVerified => "all artifacts ran successfully on native Windows".into(),
        VerifyStatus::WindowsFailed => "one or more artifacts failed on native Windows".into(),
        VerifyStatus::WindowsUnavailable => "no runnable artifacts were produced".into(),
    };
    Ok(VerifyReport {
        status,
        host: Some(winrm.addr),
        results,
        detail,
    })
}

fn xml(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn xml_attr(s: &str) -> String {
    xml(s).replace('"', "&quot;").replace('\'', "&apos;")
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
