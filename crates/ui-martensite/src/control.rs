//! Control-plane request handling: the queue consumed by the windowed runner.
//!
//! The loopback JSON-lines control server (`apps/photocraft`) parses wire requests into
//! [`ControlRequest`]s; [`drain_requests`] executes them on the event-loop thread where
//! [`crate::PhotocraftApp`] and `Engine` live. Requests either map onto the command surface
//! ([`crate::commands`], same path the menu bar and shortcuts take) or read-only getters —
//! matching the transport-neutral contract `photocraft-automation` uses for desktop sessions.

use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};

use photocraft_engine::Engine;
use serde_json::{Value, json};

use crate::PhotocraftApp;
use crate::commands::CommandExecutor;

/// A parsed control request and the oneshot the server thread blocks on for its reply.
pub struct ControlRequest {
    /// The wire `id` the reply is stamped back with by the server.
    pub id: Value,
    /// e.g. `"engine.execute"`.
    pub method: String,
    /// Parameters object as received.
    pub params: Value,
    /// Reply channel back to the blocking server thread.
    pub reply: Sender<Value>,
}

impl ControlRequest {
    /// Allocate a request + its reply receiver.
    pub fn new(id: Value, method: String, params: Value) -> (Self, Receiver<Value>) {
        let (tx, rx) = channel();
        (Self { id, method, params, reply: tx }, rx)
    }
}

/// Drain every queued control request, dispatching each on the UI thread.
///
/// The loopback control channel is the desktop's trusted transport — the client proved the
/// token — so requests run `authorized`; the executor's workspace gate
/// (`authorize_desktop_engine_command`) still constrains path arguments. Untrusted channels
/// (an unauthenticated session) go through [`handle`] with `authorized: false`, which keeps
/// only [`authorized_methods`], matching `photocraft-automation`'s `Session::authorize` gate.
pub fn drain_requests(rx: &Receiver<ControlRequest>, app: &mut PhotocraftApp, engine: &Arc<Mutex<Engine>>, executor: &dyn CommandExecutor) {
    while let Ok(req) = rx.try_recv() {
        let reply = handle(&req, app, engine, executor, /* authorized */ true);
        let _ = req.reply.send(reply);
    }
}

/// The method surface an *untrusted* control channel may reach — mirrors
/// `photocraft_automation::desktop_session::authorized_methods`: engine stepping, document-open
/// requests the UI layer arbitrates, and file-open routing.
fn authorized_methods() -> &'static [&'static str] {
    // Read-only introspection is safe for any caller; mutation methods require the
    // token-authenticated (trusted) channel.
    &["engine.step", "document.open", "file.open", "app.state", "windows.list"]
}

fn handle(req: &ControlRequest, app: &mut PhotocraftApp, engine: &Arc<Mutex<Engine>>, executor: &dyn CommandExecutor, authorized: bool) -> Value {
    let method = req.method.as_str();
    if !authorized && !authorized_methods().contains(&method) {
        return json!({"ok": false, "error": format!("method `{method}` requires authorization")});
    }
    match method {
        "engine.execute" => {
            let Some(command) = req.params.get("command").and_then(Value::as_str) else {
                return json!({"ok": false, "error": "params.command must be a string"});
            };
            let params = req.params.get("params").cloned().unwrap_or(json!({}));
            match executor.execute(app, engine, command, &params) {
                Ok(changed) => json!({"ok": true, "result": {"changed": changed}}),
                Err(e) => json!({"ok": false, "error": e.to_string()}),
            }
        }
        "app.state" => json!({
            "ok": true,
            "result": {
                "version": env!("CARGO_PKG_VERSION"),
                "tool": format!("{:?}", app.active_tool),
                "zoom": app.zoom_level,
                "dirty": app.is_dirty,
            },
        }),
        "windows.list" => json!({"ok": true, "result": [{"title": "PhotoCraft"}]}),
        "engine.step" => json!({"ok": true, "result": null}),
        "document.open" | "file.open" => {
            let Some(path) = req.params.get("path").and_then(Value::as_str) else {
                return json!({"ok": false, "error": "params.path must be a string"});
            };
            let mut engine = match engine.lock() {
                Ok(guard) => guard,
                Err(e) => e.into_inner(),
            };
            match executor.open_document(&mut engine, std::path::Path::new(path)) {
                Ok(title) => json!({"ok": true, "result": title}),
                Err(e) => json!({"ok": false, "error": e.to_string()}),
            }
        }
        other => json!({"ok": false, "error": format!("unknown method `{other}`")}),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct NoopExec;
    impl CommandExecutor for NoopExec {
        fn execute(&self, _app: &mut PhotocraftApp, _engine: &Arc<Mutex<Engine>>, _command: &str, _params: &Value) -> photocraft_engine::Result<bool> {
            Ok(false)
        }
        fn open_document(&self, _engine: &mut Engine, _path: &std::path::Path) -> photocraft_engine::Result<String> {
            Ok("doc".into())
        }
    }

    fn app() -> PhotocraftApp {
        PhotocraftApp::new(Engine::new())
    }

    #[test]
    fn untrusted_channel_rejects_engine_execute() {
        let engine = Arc::new(Mutex::new(Engine::new()));
        let mut app = app();
        let (req, _rx) = ControlRequest::new(json!(1), "engine.execute".into(), json!({"command": "file.new"}));
        let reply = handle(&req, &mut app, &engine, &NoopExec, false);
        assert_eq!(reply["ok"], false);
    }

    #[test]
    fn authorized_engine_execute_runs_the_command() {
        let engine = Arc::new(Mutex::new(Engine::new()));
        let mut app = app();
        let (req, _rx) = ControlRequest::new(json!(1), "engine.execute".into(), json!({"command": "file.new"}));
        let reply = handle(&req, &mut app, &engine, &NoopExec, true);
        assert_eq!(reply["ok"], true);
    }

    #[test]
    fn app_state_reports_app_fields() {
        let engine = Arc::new(Mutex::new(Engine::new()));
        let mut app = app();
        let (req, _rx) = ControlRequest::new(json!(9), "app.state".into(), json!({}));
        let reply = handle(&req, &mut app, &engine, &NoopExec, false);
        assert_eq!(reply["ok"], true);
        assert!(reply["result"]["version"].is_string());
    }
}
