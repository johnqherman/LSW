use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use serde::{Deserialize, Serialize};

use lsw_config::Dirs;

use crate::envops;
use crate::error::{Error, Result};

pub const PROTOCOL_VERSION: u32 = 1;

pub fn socket_path(dirs: &Dirs) -> PathBuf {
    match std::env::var_os("XDG_RUNTIME_DIR") {
        Some(rt) if !rt.is_empty() => PathBuf::from(rt).join("lsw/lswd.sock"),
        _ => dirs.cache.join("lswd.sock"),
    }
}

#[derive(Debug, Deserialize)]
struct Request {
    #[serde(default)]
    id: u64,
    method: String,
}

#[derive(Debug, Serialize)]
struct Response {
    id: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

pub fn serve(dirs: &Dirs) -> Result<()> {
    let path = socket_path(dirs);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| Error::io(parent.to_path_buf(), e))?;
    }
    let _ = std::fs::remove_file(&path);
    let listener = UnixListener::bind(&path).map_err(|e| Error::io(path.clone(), e))?;

    let running = Arc::new(AtomicBool::new(true));
    for stream in listener.incoming() {
        if !running.load(Ordering::SeqCst) {
            break;
        }
        match stream {
            Ok(stream) => handle_connection(stream, dirs, &running),
            Err(e) => {
                tracing::warn!("lswd accept error: {e}");
                break;
            }
        }
    }
    let _ = std::fs::remove_file(&path);
    Ok(())
}

fn handle_connection(stream: UnixStream, dirs: &Dirs, running: &Arc<AtomicBool>) {
    let mut writer = match stream.try_clone() {
        Ok(w) => w,
        Err(_) => return,
    };
    let reader = BufReader::new(stream);
    for line in reader.lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() {
            continue;
        }
        let response = dispatch(&line, dirs, running);
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

fn dispatch(line: &str, dirs: &Dirs, running: &Arc<AtomicBool>) -> Response {
    let request: Request = match serde_json::from_str(line) {
        Ok(r) => r,
        Err(e) => {
            return Response {
                id: 0,
                result: None,
                error: Some(format!("malformed request: {e}")),
            };
        }
    };
    let id = request.id;
    match request.method.as_str() {
        "version" => Response {
            id,
            result: Some(serde_json::json!({
                "protocol": PROTOCOL_VERSION,
                "version": env!("CARGO_PKG_VERSION"),
            })),
            error: None,
        },
        "ping" => Response {
            id,
            result: Some(serde_json::json!({ "pong": true })),
            error: None,
        },
        "env.list" => match envops::list(dirs) {
            Ok(envs) => {
                let names: Vec<String> = envs.into_iter().map(|e| e.name).collect();
                Response {
                    id,
                    result: Some(serde_json::json!({ "environments": names })),
                    error: None,
                }
            }
            Err(e) => Response {
                id,
                result: None,
                error: Some(e.to_string()),
            },
        },
        "shutdown" => {
            running.store(false, Ordering::SeqCst);
            Response {
                id,
                result: Some(serde_json::json!({ "stopping": true })),
                error: None,
            }
        }
        other => Response {
            id,
            result: None,
            error: Some(format!("unknown method '{other}'")),
        },
    }
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
        self.next_id += 1;
        let mut line = serde_json::to_string(&serde_json::json!({ "id": id, "method": method }))
            .expect("request serializes");
        line.push('\n');
        self.stream
            .write_all(line.as_bytes())
            .map_err(|e| Error::io(self.path.clone(), e))?;
        self.stream
            .flush()
            .map_err(|e| Error::io(self.path.clone(), e))?;

        let mut reader = BufReader::new(&self.stream);
        let mut response = String::new();
        reader
            .read_line(&mut response)
            .map_err(|e| Error::io(self.path.clone(), e))?;

        let value: serde_json::Value =
            serde_json::from_str(response.trim()).map_err(|e| Error::DaemonUnavailable {
                path: self.path.clone(),
                detail: format!("malformed response: {e}"),
            })?;
        if let Some(err) = value.get("error").and_then(|e| e.as_str()) {
            return Err(Error::DaemonUnavailable {
                path: self.path.clone(),
                detail: err.to_owned(),
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
    fn serve_answers_version_ping_envlist_then_shutdown() {
        let tmp = tempfile::tempdir().unwrap();
        let dirs = temp_dirs(tmp.path());
        let sock = dirs.cache.join("lswd.sock");
        std::fs::create_dir_all(&dirs.cache).unwrap();
        std::fs::create_dir_all(dirs.environments()).unwrap();

        let server_dirs = dirs.clone();
        let sock_for_server = sock.clone();
        let handle = std::thread::spawn(move || {
            serve_on(&sock_for_server, &server_dirs).unwrap();
        });

        for _ in 0..100 {
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
    }

    fn serve_on(sock: &std::path::Path, dirs: &Dirs) -> Result<()> {
        let _ = std::fs::remove_file(sock);
        let listener = UnixListener::bind(sock).map_err(|e| Error::io(sock.to_path_buf(), e))?;
        let running = Arc::new(AtomicBool::new(true));
        for stream in listener.incoming() {
            if !running.load(Ordering::SeqCst) {
                break;
            }
            match stream {
                Ok(stream) => handle_connection(stream, dirs, &running),
                Err(_) => break,
            }
            if !running.load(Ordering::SeqCst) {
                break;
            }
        }
        let _ = std::fs::remove_file(sock);
        Ok(())
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
