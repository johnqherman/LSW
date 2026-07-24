use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use lsw_config::Dirs;

use crate::envops;
use crate::error::{Error, Result};

pub const PROTOCOL_VERSION: u32 = 1;

const CLIENT_IDLE_TIMEOUT: Duration = Duration::from_secs(30);
const MAX_RESPONSE_BYTES: u64 = 8 * 1024 * 1024;

const ACCEPT_POLL: Duration = Duration::from_millis(100);
const MAX_CONNECTIONS: usize = 64;
const MAX_ENV_LIST: usize = 100_000;

struct ActiveGuard(Arc<std::sync::atomic::AtomicUsize>);

impl Drop for ActiveGuard {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

pub fn socket_path(dirs: &Dirs) -> PathBuf {
    match std::env::var_os("XDG_RUNTIME_DIR") {
        Some(rt) if !rt.is_empty() => PathBuf::from(rt).join("lsw/lswd.sock"),
        _ => dirs.cache.join("lswd.sock"),
    }
}

const JSONRPC_VERSION: &str = "2.0";

#[derive(Debug, Deserialize)]
struct Request {
    #[serde(default)]
    jsonrpc: Option<String>,
    #[serde(default)]
    id: Option<u64>,
    method: String,
    #[serde(default)]
    #[allow(dead_code)]
    params: Option<serde_json::Value>,
}

#[derive(Debug, Serialize)]
struct RpcError {
    code: i32,
    message: String,
}

#[derive(Debug, Serialize)]
struct Response {
    jsonrpc: &'static str,
    id: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<RpcError>,
}

impl Response {
    fn ok(id: u64, result: serde_json::Value) -> Self {
        Self {
            jsonrpc: JSONRPC_VERSION,
            id,
            result: Some(result),
            error: None,
        }
    }

    fn err(id: u64, code: i32, message: impl Into<String>) -> Self {
        Self {
            jsonrpc: JSONRPC_VERSION,
            id,
            result: None,
            error: Some(RpcError {
                code,
                message: message.into(),
            }),
        }
    }
}

pub fn serve(dirs: &Dirs) -> Result<()> {
    let path = socket_path(dirs);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| Error::io(parent.to_path_buf(), e))?;
    }
    let listener = bind_socket(&path)?;
    run_accept_loop(listener, &path, dirs)
}

fn run_accept_loop(listener: UnixListener, path: &Path, dirs: &Dirs) -> Result<()> {
    listener
        .set_nonblocking(true)
        .map_err(|e| Error::io(path.to_path_buf(), e))?;

    let running = Arc::new(AtomicBool::new(true));
    let active = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    while running.load(Ordering::SeqCst) {
        match listener.accept() {
            Ok((stream, _)) => {
                if active.load(Ordering::SeqCst) >= MAX_CONNECTIONS
                    || stream.set_read_timeout(Some(CLIENT_IDLE_TIMEOUT)).is_err()
                {
                    drop(stream);
                    continue;
                }
                active.fetch_add(1, Ordering::SeqCst);
                let dirs = dirs.clone();
                let running = Arc::clone(&running);
                let guard = ActiveGuard(Arc::clone(&active));
                let spawned = std::thread::Builder::new().spawn(move || {
                    let _guard = guard;
                    handle_connection(stream, &dirs, &running);
                });
                if spawned.is_err() {
                    tracing::warn!("lswd could not spawn handler thread; dropping connection");
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(ACCEPT_POLL);
            }
            Err(e) => {
                tracing::warn!("lswd accept error: {e}");
                let _ = std::fs::remove_file(path);
                return Err(Error::io(path.to_path_buf(), e));
            }
        }
    }
    let _ = std::fs::remove_file(path);
    Ok(())
}

fn bind_socket(path: &Path) -> Result<UnixListener> {
    let listener = match UnixListener::bind(path) {
        Ok(listener) => listener,
        Err(e) if e.kind() == std::io::ErrorKind::AddrInUse => {
            if UnixStream::connect(path).is_ok() {
                return Err(Error::DaemonUnavailable {
                    path: path.to_path_buf(),
                    detail: "another lswd is already running on this socket".into(),
                });
            }
            let _ = std::fs::remove_file(path);
            UnixListener::bind(path).map_err(|e| Error::io(path.to_path_buf(), e))?
        }
        Err(e) => return Err(Error::io(path.to_path_buf(), e)),
    };
    use std::os::unix::fs::PermissionsExt;
    let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
    Ok(listener)
}

fn handle_connection(stream: UnixStream, dirs: &Dirs, running: &Arc<AtomicBool>) {
    let mut writer = match stream.try_clone() {
        Ok(w) => w,
        Err(_) => return,
    };
    if writer.set_write_timeout(Some(CLIENT_IDLE_TIMEOUT)).is_err() {
        return;
    }
    const MAX_FRAME: u64 = 1 << 20;
    let mut reader = BufReader::new(stream);
    loop {
        let mut buf = Vec::new();
        let read = std::io::Read::take(&mut reader, MAX_FRAME + 1).read_until(b'\n', &mut buf);
        match read {
            Ok(0) => break,
            Ok(_) if buf.len() as u64 > MAX_FRAME => break,
            Ok(_) => {}
            Err(_) => break,
        }
        let line = String::from_utf8_lossy(&buf);
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Some(response) = dispatch(line, dirs, running) else {
            continue;
        };
        let mut out = serde_json::to_string(&response).expect("response serializes");
        out.push('\n');
        if writer.write_all(out.as_bytes()).is_err() {
            break;
        }
        let _ = writer.flush();
        if !running.load(Ordering::SeqCst) {
            break;
        }
    }
}

fn dispatch(line: &str, dirs: &Dirs, running: &Arc<AtomicBool>) -> Option<Response> {
    let request: Request = match serde_json::from_str(line) {
        Ok(r) => r,
        Err(e) => {
            return Some(Response::err(0, -32700, format!("parse error: {e}")));
        }
    };
    let id = request.id?;
    if request.jsonrpc.as_deref() != Some(JSONRPC_VERSION) {
        return Some(Response::err(
            id,
            -32600,
            "invalid request: jsonrpc must be \"2.0\"".to_owned(),
        ));
    }
    Some(match request.method.as_str() {
        "version" => Response::ok(
            id,
            serde_json::json!({
                "protocol": PROTOCOL_VERSION,
                "version": env!("CARGO_PKG_VERSION"),
            }),
        ),
        "ping" => Response::ok(id, serde_json::json!({ "pong": true })),
        "env.list" => match envops::list(dirs) {
            Ok(envs) => {
                let names: Vec<String> = envs
                    .into_iter()
                    .map(|e| e.name)
                    .take(MAX_ENV_LIST)
                    .collect();
                Response::ok(id, serde_json::json!({ "environments": names }))
            }
            Err(e) => Response::err(id, -32603, e.to_string()),
        },
        "shutdown" => {
            running.store(false, Ordering::SeqCst);
            Response::ok(id, serde_json::json!({ "stopping": true }))
        }
        other => Response::err(id, -32601, format!("unknown method '{other}'")),
    })
}

pub struct DaemonClient {
    stream: UnixStream,
    path: PathBuf,
    next_id: u64,
}

impl DaemonClient {
    pub fn connect(dirs: &Dirs) -> Result<Self> {
        let path = socket_path(dirs);
        let stream = UnixStream::connect(&path).map_err(|e| Error::DaemonUnavailable {
            path: path.clone(),
            detail: e.to_string(),
        })?;
        Ok(Self {
            stream,
            path,
            next_id: 0,
        })
    }

    pub fn call(&mut self, method: &str) -> Result<serde_json::Value> {
        let id = self.next_id;
        let Some(next) = self.next_id.checked_add(1) else {
            return Err(Error::io(
                self.path.clone(),
                std::io::Error::other("request id space exhausted"),
            ));
        };
        self.next_id = next;
        let mut line = serde_json::to_string(
            &serde_json::json!({ "jsonrpc": JSONRPC_VERSION, "id": id, "method": method }),
        )
        .expect("request serializes");
        line.push('\n');
        self.stream
            .set_write_timeout(Some(CLIENT_IDLE_TIMEOUT))
            .map_err(|e| Error::io(self.path.clone(), e))?;
        self.stream
            .set_read_timeout(Some(CLIENT_IDLE_TIMEOUT))
            .map_err(|e| Error::io(self.path.clone(), e))?;
        self.stream
            .write_all(line.as_bytes())
            .map_err(|e| Error::io(self.path.clone(), e))?;
        self.stream
            .flush()
            .map_err(|e| Error::io(self.path.clone(), e))?;

        let mut reader = BufReader::new(&self.stream);
        let mut response = String::new();
        (&mut reader)
            .take(MAX_RESPONSE_BYTES + 1)
            .read_line(&mut response)
            .map_err(|e| Error::io(self.path.clone(), e))?;
        if !response.ends_with('\n') {
            return Err(Error::DaemonUnavailable {
                path: self.path.clone(),
                detail: "daemon response was not newline-terminated within the size limit".into(),
            });
        }

        let value: serde_json::Value =
            serde_json::from_str(response.trim()).map_err(|e| Error::DaemonUnavailable {
                path: self.path.clone(),
                detail: format!("malformed response: {e}"),
            })?;
        if value.get("jsonrpc").and_then(|v| v.as_str()) != Some(JSONRPC_VERSION) {
            return Err(Error::DaemonUnavailable {
                path: self.path.clone(),
                detail: "daemon response is not jsonrpc 2.0".to_owned(),
            });
        }
        if value.get("id").and_then(|v| v.as_u64()) != Some(id) {
            return Err(Error::DaemonUnavailable {
                path: self.path.clone(),
                detail: format!("daemon response id mismatch (expected {id})"),
            });
        }
        if value.get("result").is_some() == value.get("error").is_some() {
            return Err(Error::DaemonUnavailable {
                path: self.path.clone(),
                detail: "daemon response must contain exactly one of result or error".to_owned(),
            });
        }
        if let Some(err) = value.get("error") {
            let message = err
                .get("message")
                .and_then(|m| m.as_str())
                .unwrap_or("daemon error");
            return Err(Error::DaemonUnavailable {
                path: self.path.clone(),
                detail: message.to_owned(),
            });
        }
        Ok(value
            .get("result")
            .cloned()
            .unwrap_or(serde_json::Value::Null))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dirs(base: &std::path::Path) -> Dirs {
        Dirs {
            data: base.join("data"),
            config: base.join("cfg"),
            cache: base.join("cache"),
        }
    }

    #[test]
    fn socket_path_prefers_xdg_runtime_dir() {
        let dirs = temp_dirs(std::path::Path::new("/base"));
        let p = socket_path(&dirs);
        assert!(p.ends_with("lswd.sock"));
    }

    #[test]
    fn dispatch_emits_jsonrpc_2_0_envelope_and_structured_errors() {
        let tmp = tempfile::tempdir().unwrap();
        let dirs = temp_dirs(tmp.path());
        std::fs::create_dir_all(dirs.environments()).unwrap();
        let running = Arc::new(AtomicBool::new(true));

        let ok = dispatch(
            r#"{"jsonrpc":"2.0","id":7,"method":"ping"}"#,
            &dirs,
            &running,
        );
        let v = serde_json::to_value(&ok).unwrap();
        assert_eq!(v["jsonrpc"], "2.0");
        assert_eq!(v["id"], 7);
        assert_eq!(v["result"]["pong"], true);

        let unknown = dispatch(
            r#"{"jsonrpc":"2.0","id":1,"method":"nope"}"#,
            &dirs,
            &running,
        );
        let e = serde_json::to_value(&unknown).unwrap();
        assert_eq!(e["error"]["code"], -32601);
        assert!(e["error"]["message"].as_str().unwrap().contains("nope"));

        let missing_version = dispatch(r#"{"id":2,"method":"ping"}"#, &dirs, &running);
        assert_eq!(
            serde_json::to_value(&missing_version).unwrap()["error"]["code"],
            -32600
        );

        let bad = dispatch("not json", &dirs, &running);
        assert_eq!(serde_json::to_value(&bad).unwrap()["error"]["code"], -32700);
    }

    #[test]
    fn serve_answers_version_ping_envlist_then_shutdown() {
        let tmp = tempfile::tempdir().unwrap();
        let dirs = temp_dirs(tmp.path());
        let sock = dirs.cache.join("lswd.sock");
        std::fs::create_dir_all(&dirs.cache).unwrap();
        std::fs::create_dir_all(dirs.environments()).unwrap();

        let server_dirs = dirs.clone();
        let sock_for_server = sock.clone();
        let handle = std::thread::spawn(move || {
            let listener = bind_socket(&sock_for_server).unwrap();
            run_accept_loop(listener, &sock_for_server, &server_dirs).unwrap();
        });

        for _ in 0..200 {
            if sock.exists() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }

        let mut client = connect_to(&sock).unwrap();
        let version = client.call("version").unwrap();
        assert_eq!(version["protocol"], serde_json::json!(PROTOCOL_VERSION));
        assert_eq!(
            client.call("ping").unwrap()["pong"],
            serde_json::json!(true)
        );
        let envs = client.call("env.list").unwrap();
        assert!(envs["environments"].is_array());
        client.call("shutdown").unwrap();

        handle.join().unwrap();
        assert!(!sock.exists(), "socket left behind after shutdown");
    }

    #[test]
    fn bind_refuses_to_clobber_a_live_daemon() {
        let tmp = tempfile::tempdir().unwrap();
        let dirs = temp_dirs(tmp.path());
        std::fs::create_dir_all(&dirs.cache).unwrap();
        std::fs::create_dir_all(dirs.environments()).unwrap();
        let sock = dirs.cache.join("lswd.sock");

        let server_dirs = dirs.clone();
        let sock_for_server = sock.clone();
        let handle = std::thread::spawn(move || {
            let listener = bind_socket(&sock_for_server).unwrap();
            run_accept_loop(listener, &sock_for_server, &server_dirs).unwrap();
        });
        for _ in 0..200 {
            if sock.exists() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }

        let err = bind_socket(&sock).unwrap_err();
        assert!(err.to_string().contains("already running"));

        connect_to(&sock).unwrap().call("shutdown").unwrap();
        handle.join().unwrap();

        std::fs::write(&sock, b"").ok();
        let _ = std::fs::remove_file(&sock);
        assert!(bind_socket(&sock).is_ok());
    }

    fn connect_to(sock: &std::path::Path) -> Result<DaemonClient> {
        let stream = UnixStream::connect(sock).map_err(|e| Error::DaemonUnavailable {
            path: sock.to_path_buf(),
            detail: e.to_string(),
        })?;
        Ok(DaemonClient {
            stream,
            path: sock.to_path_buf(),
            next_id: 0,
        })
    }
}
