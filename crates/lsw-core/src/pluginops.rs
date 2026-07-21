use std::io::{BufRead, BufReader, Read, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

pub const PROTOCOL_VERSION: u32 = 1;

const PREFIX: &str = "lsw-provider-";

const MAX_LINE_BYTES: u64 = 1024 * 1024;

const CALL_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Handshake {
    pub protocol: u32,
    pub provider: String,
    #[serde(rename = "providerVersion")]
    pub provider_version: String,
    #[serde(default)]
    pub kind: String,
}

#[derive(Serialize)]
struct Request<'a> {
    id: u64,
    method: &'a str,
    params: serde_json::Value,
}

#[derive(Deserialize)]
struct Response {
    #[serde(default)]
    id: u64,
    #[serde(default)]
    result: Option<serde_json::Value>,
    #[serde(default)]
    error: Option<serde_json::Value>,
}

pub struct Plugin {
    name: String,
    child: Child,
    stdin: Option<ChildStdin>,
    rx: std::sync::mpsc::Receiver<std::io::Result<Vec<u8>>>,
    reader: Option<std::thread::JoinHandle<()>>,
    next_id: u64,
    pub handshake: Handshake,
}

impl Plugin {
    pub fn connect(name: &str, path: &std::path::Path) -> Result<Self> {
        let mut child = spawn_plugin(path)?;

        let stdin = child.stdin.take().expect("piped stdin");
        let stdout = child.stdout.take().expect("piped stdout");

        let (tx, rx) = std::sync::mpsc::channel();
        let reader = std::thread::spawn(move || {
            let mut buf = BufReader::new(stdout);
            loop {
                match read_bounded_line(&mut buf, MAX_LINE_BYTES) {
                    Ok(Some(line)) => {
                        if tx.send(Ok(line)).is_err() {
                            break;
                        }
                    }
                    Ok(None) => break,
                    Err(e) => {
                        let _ = tx.send(Err(e));
                        break;
                    }
                }
            }
        });

        let mut plugin = Plugin {
            name: name.to_owned(),
            child,
            stdin: Some(stdin),
            rx,
            reader: Some(reader),
            next_id: 0,
            handshake: Handshake {
                protocol: 0,
                provider: String::new(),
                provider_version: String::new(),
                kind: String::new(),
            },
        };

        let value = plugin.call(
            "handshake",
            serde_json::json!({ "protocol": PROTOCOL_VERSION }),
        )?;
        let handshake: Handshake =
            serde_json::from_value(value).map_err(|e| Error::PluginProtocol {
                name: name.to_owned(),
                detail: format!("malformed handshake: {e}"),
            })?;
        if handshake.protocol != PROTOCOL_VERSION {
            return Err(Error::PluginProtocol {
                name: name.to_owned(),
                detail: format!(
                    "plugin speaks protocol {} but LSW speaks {PROTOCOL_VERSION}",
                    handshake.protocol
                ),
            });
        }
        plugin.handshake = handshake;
        Ok(plugin)
    }

    pub fn call(&mut self, method: &str, params: serde_json::Value) -> Result<serde_json::Value> {
        let id = self.next_id;
        self.next_id += 1;

        let request = Request { id, method, params };
        let mut line = serde_json::to_string(&request).expect("request serializes");
        line.push('\n');
        {
            let stdin = self
                .stdin
                .as_mut()
                .ok_or_else(|| plugin_err(&self.name, "plugin stdin closed".into()))?;
            stdin
                .write_all(line.as_bytes())
                .map_err(|e| plugin_err(&self.name, format!("write failed: {e}")))?;
            stdin
                .flush()
                .map_err(|e| plugin_err(&self.name, format!("flush failed: {e}")))?;
        }

        let raw = match self.rx.recv_timeout(CALL_TIMEOUT) {
            Ok(Ok(bytes)) => bytes,
            Ok(Err(e)) => return Err(plugin_err(&self.name, format!("read failed: {e}"))),
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                let _ = self.child.kill();
                return Err(plugin_err(
                    &self.name,
                    format!(
                        "no response within {}s; plugin killed",
                        CALL_TIMEOUT.as_secs()
                    ),
                ));
            }
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                return Err(plugin_err(
                    &self.name,
                    "plugin closed the connection".into(),
                ));
            }
        };

        let response: Response = serde_json::from_slice(&raw)
            .map_err(|e| plugin_err(&self.name, format!("malformed response: {e}")))?;
        if response.id != id {
            return Err(plugin_err(
                &self.name,
                format!("response id {} does not match request id {id}", response.id),
            ));
        }
        if let Some(error) = response.error {
            return Err(plugin_err(
                &self.name,
                format!("plugin returned error: {error}"),
            ));
        }
        response
            .result
            .ok_or_else(|| plugin_err(&self.name, "response has neither result nor error".into()))
    }

    pub fn shutdown(mut self) {
        let _ = self.call("shutdown", serde_json::Value::Null);
    }
}

impl Drop for Plugin {
    fn drop(&mut self) {
        drop(self.stdin.take());
        for _ in 0..20 {
            match self.child.try_wait() {
                Ok(Some(_)) => break,
                Ok(None) => std::thread::sleep(Duration::from_millis(25)),
                Err(_) => break,
            }
        }
        if matches!(self.child.try_wait(), Ok(None)) {
            let _ = self.child.kill();
        }
        let _ = self.child.wait();
        if let Some(reader) = self.reader.take() {
            let _ = reader.join();
        }
    }
}

fn plugin_err(name: &str, detail: String) -> Error {
    Error::PluginProtocol {
        name: name.to_owned(),
        detail,
    }
}

fn spawn_plugin(path: &std::path::Path) -> Result<Child> {
    let mut attempt = 0;
    loop {
        match Command::new(path)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
        {
            Ok(child) => return Ok(child),
            Err(e) if e.raw_os_error() == Some(26) && attempt < 25 => {
                attempt += 1;
                std::thread::sleep(Duration::from_millis(20));
            }
            Err(e) => return Err(Error::io(path.to_path_buf(), e)),
        }
    }
}

fn read_bounded_line<R: BufRead>(reader: &mut R, max: u64) -> std::io::Result<Option<Vec<u8>>> {
    let mut limited = reader.take(max + 1);
    let mut buf = Vec::new();
    let mut byte = [0u8; 1];
    loop {
        let n = limited.read(&mut byte)?;
        if n == 0 {
            return Ok(if buf.is_empty() { None } else { Some(buf) });
        }
        if byte[0] == b'\n' {
            return Ok(Some(buf));
        }
        buf.push(byte[0]);
        if buf.len() as u64 > max {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "plugin response line exceeded size limit",
            ));
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct DiscoveredPlugin {
    pub name: String,
    pub path: PathBuf,
}

pub fn discover() -> Vec<DiscoveredPlugin> {
    match std::env::var_os("PATH") {
        Some(path_var) => discover_in(std::env::split_paths(&path_var)),
        None => Vec::new(),
    }
}

fn discover_in(dirs: impl IntoIterator<Item = PathBuf>) -> Vec<DiscoveredPlugin> {
    let mut seen = std::collections::BTreeMap::new();
    for dir in dirs {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let file_name = entry.file_name();
            let Some(file) = file_name.to_str() else {
                continue;
            };
            let Some(name) = file.strip_prefix(PREFIX) else {
                continue;
            };
            if name.is_empty() || !is_executable(&entry.path()) {
                continue;
            }
            seen.entry(name.to_owned()).or_insert_with(|| entry.path());
        }
    }
    seen.into_iter()
        .map(|(name, path)| DiscoveredPlugin { name, path })
        .collect()
}

fn is_executable(path: &std::path::Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    path.metadata()
        .map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounded_line_reads_and_caps() {
        let data = b"hello\nworld\n";
        let mut reader = BufReader::new(&data[..]);
        assert_eq!(
            read_bounded_line(&mut reader, 1024).unwrap().unwrap(),
            b"hello"
        );
        assert_eq!(
            read_bounded_line(&mut reader, 1024).unwrap().unwrap(),
            b"world"
        );
        assert!(read_bounded_line(&mut reader, 1024).unwrap().is_none());

        let huge = [b'a'; 100];
        let mut reader = BufReader::new(&huge[..]);
        assert!(read_bounded_line(&mut reader, 10).is_err());
    }

    #[test]
    fn mismatched_response_id_is_rejected() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join(format!("{PREFIX}liar"));
        let script = "#!/bin/sh\n\
             n=0\n\
             while IFS= read -r line; do\n\
             \x20 case \"$line\" in\n\
             \x20   *handshake*) printf '{\"id\":0,\"result\":{\"protocol\":1,\"provider\":\"liar\",\"providerVersion\":\"1\",\"kind\":\"runtime\"}}\\n' ;;\n\
             \x20   *) printf '{\"id\":999,\"result\":{}}\\n' ;;\n\
             \x20 esac\n\
             done\n";
        std::fs::write(&path, script).unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();

        let mut plugin = Plugin::connect("liar", &path).unwrap();
        let err = plugin.call("ping", serde_json::Value::Null);
        assert!(err.is_err_and(|e| e.to_string().contains("does not match request id")));
    }

    fn write_mock_plugin(dir: &std::path::Path, name: &str, protocol: u32) -> PathBuf {
        let path = dir.join(format!("{PREFIX}{name}"));
        let script = format!(
            "#!/bin/sh\n\
             while IFS= read -r line; do\n\
             \x20 case \"$line\" in\n\
             \x20   *handshake*) printf '{{\"id\":0,\"result\":{{\"protocol\":{protocol},\"provider\":\"{name}\",\"providerVersion\":\"1.2\",\"kind\":\"runtime\"}}}}\\n' ;;\n\
             \x20   *shutdown*)  printf '{{\"id\":9,\"result\":null}}\\n'; exit 0 ;;\n\
             \x20   *ping*)      printf '{{\"id\":1,\"result\":{{\"pong\":true}}}}\\n' ;;\n\
             \x20   *)           printf '{{\"id\":1,\"error\":{{\"message\":\"unknown\"}}}}\\n' ;;\n\
             \x20 esac\n\
             done\n"
        );
        std::fs::write(&path, script).unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        path
    }

    #[test]
    fn connect_handshake_and_call() {
        let tmp = tempfile::tempdir().unwrap();
        let path = write_mock_plugin(tmp.path(), "mock", PROTOCOL_VERSION);

        let mut plugin = Plugin::connect("mock", &path).unwrap();
        assert_eq!(plugin.handshake.provider, "mock");
        assert_eq!(plugin.handshake.provider_version, "1.2");
        assert_eq!(plugin.handshake.kind, "runtime");

        let result = plugin.call("ping", serde_json::Value::Null).unwrap();
        assert_eq!(result["pong"], serde_json::json!(true));
        plugin.shutdown();
    }

    #[test]
    fn protocol_version_mismatch_is_rejected() {
        let tmp = tempfile::tempdir().unwrap();
        let path = write_mock_plugin(tmp.path(), "old", PROTOCOL_VERSION + 1);
        match Plugin::connect("old", &path) {
            Ok(_) => panic!("mismatched protocol must be rejected"),
            Err(e) => assert!(e.to_string().contains("LSW2022")),
        }
    }

    #[test]
    fn discover_finds_prefixed_executables() {
        let tmp = tempfile::tempdir().unwrap();
        write_mock_plugin(tmp.path(), "alpha", PROTOCOL_VERSION);
        write_mock_plugin(tmp.path(), "beta", PROTOCOL_VERSION);
        std::fs::write(tmp.path().join("not-a-plugin"), b"x").unwrap();

        let found = discover_in([tmp.path().to_path_buf()]);
        let names: Vec<_> = found.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, vec!["alpha", "beta"]);
    }
}
