use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

pub const PROTOCOL_VERSION: u32 = 1;

const PREFIX: &str = "lsw-provider-";

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
    #[allow(dead_code)]
    id: u64,
    #[serde(default)]
    result: Option<serde_json::Value>,
    #[serde(default)]
    error: Option<serde_json::Value>,
}

pub struct Plugin {
    name: String,
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    next_id: u64,
    pub handshake: Handshake,
}

impl Plugin {
    pub fn connect(name: &str, path: &std::path::Path) -> Result<Self> {
        let mut child = Command::new(path)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .map_err(|e| Error::io(path.to_path_buf(), e))?;

        let stdin = child.stdin.take().expect("piped stdin");
        let stdout = BufReader::new(child.stdout.take().expect("piped stdout"));

        let mut plugin = Plugin {
            name: name.to_owned(),
            child,
            stdin,
            stdout,
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
        self.stdin
            .write_all(line.as_bytes())
            .map_err(|e| self.protocol_err(format!("write failed: {e}")))?;
        self.stdin
            .flush()
            .map_err(|e| self.protocol_err(format!("flush failed: {e}")))?;

        let mut response_line = String::new();
        let read = self
            .stdout
            .read_line(&mut response_line)
            .map_err(|e| self.protocol_err(format!("read failed: {e}")))?;
        if read == 0 {
            return Err(self.protocol_err("plugin closed the connection".into()));
        }

        let response: Response = serde_json::from_str(response_line.trim())
            .map_err(|e| self.protocol_err(format!("malformed response: {e}")))?;
        if let Some(error) = response.error {
            return Err(self.protocol_err(format!("plugin returned error: {error}")));
        }
        response
            .result
            .ok_or_else(|| self.protocol_err("response has neither result nor error".into()))
    }

    fn protocol_err(&self, detail: String) -> Error {
        Error::PluginProtocol {
            name: self.name.clone(),
            detail,
        }
    }

    pub fn shutdown(mut self) {
        let _ = self.call("shutdown", serde_json::Value::Null);
        drop(self.stdin);
        let _ = self.child.wait();
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct DiscoveredPlugin {
    pub name: String,
    pub path: PathBuf,
}

pub fn discover() -> Vec<DiscoveredPlugin> {
    let Some(path_var) = std::env::var_os("PATH") else {
        return Vec::new();
    };
    let mut seen = std::collections::BTreeMap::new();
    for dir in std::env::split_paths(&path_var) {
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

        let prev = std::env::var_os("PATH");
        unsafe {
            std::env::set_var("PATH", tmp.path());
        }
        let found = discover();
        unsafe {
            match prev {
                Some(p) => std::env::set_var("PATH", p),
                None => std::env::remove_var("PATH"),
            }
        }

        let names: Vec<_> = found.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, vec!["alpha", "beta"]);
    }
}
