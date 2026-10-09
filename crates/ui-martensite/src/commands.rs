//! Command dispatch surface shared by the menu bar, shortcuts and the control channel.
//!
//! [`CommandExecutor`] abstracts how a command id plus JSON params reach the engine, so the
//! control plane (`crate::control`) can be exercised in tests without a session. The
//! production implementation is [`EngineExecutor`], which forwards to `Session::execute` —
//! the same dispatch the CLI and the old egui shell used.

use std::path::Path;
use std::sync::{Arc, Mutex};

use photocraft_engine::Engine;
use serde_json::Value;

use crate::PhotocraftApp;

/// Runs a command (or opens a document) against the session.
pub trait CommandExecutor {
    /// Executes `command` with `params`; returns whether app-visible state changed.
    fn execute(&self, app: &mut PhotocraftApp, engine: &Arc<Mutex<Engine>>, command: &str, params: &Value) -> photocraft_engine::Result<bool>;

    /// Imports `path` into the session and returns the opened document's title.
    fn open_document(&self, engine: &mut Engine, path: &Path) -> photocraft_engine::Result<String>;
}

/// Reads a document's bytes for [`CommandExecutor::open_document`]. The runner installs the
/// automation-workspace-sandboxed read (`--automation-read-root`) for control-channel requests.
pub type FileReader = Arc<dyn Fn(&Path) -> Result<Vec<u8>, String> + Send + Sync>;

/// Authorization gate for commands reached through the control channel — the
/// automation workspace's path allowlist (`authorize_desktop_engine_command`).
/// `None`: the channel is trusted and every engine command runs.
pub type CommandAuthorize = Arc<dyn Fn(&str, &Value) -> Result<(), String> + Send + Sync>;

/// The production executor: commands go to `Session::execute`, documents through `photocraft-io`.
#[derive(Default)]
pub struct EngineExecutor {
    /// Sandboxed read for `open_document` (`None`: a direct filesystem read).
    pub reader: Option<FileReader>,
    /// Authorization gate applied before `Session::execute`.
    pub authorize: Option<CommandAuthorize>,
}

impl CommandExecutor for EngineExecutor {
    fn execute(&self, app: &mut PhotocraftApp, engine: &Arc<Mutex<Engine>>, command: &str, params: &Value) -> photocraft_engine::Result<bool> {
        if let Some(authorize) = &self.authorize {
            authorize(command, params).map_err(|e| photocraft_engine::EngineError::BadParams { cmd: command.into(), msg: e })?;
        }
        let mut session = engine.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let before = session.documents().len();
        session.execute(command, params.clone())?;
        let changed = session.documents().len() != before || session.active().is_some();
        app.is_dirty = true;
        Ok(changed)
    }

    fn open_document(&self, engine: &mut Engine, path: &Path) -> photocraft_engine::Result<String> {
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "document".to_string());
        let bytes = match &self.reader {
            Some(read) => read(path),
            None => std::fs::read(path).map_err(|e| e.to_string()),
        }
        .map_err(|e| photocraft_engine::EngineError::BadParams { cmd: "file.open".into(), msg: format!("cannot read {}: {e}", path.display()) })?;
        // Last-resort guard (never-crash standard): a decoder panic on a hostile file becomes an
        // error reply instead of taking the app down. The panic hook installed at startup logs it.
        let import = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| photocraft_io::import(&name, &bytes)))
            .map_err(|_| format!("importing {name} failed with an internal error (logged); the document was not opened"))
            .and_then(|r| r.map_err(|e| e.to_string()))
            .map_err(|msg| photocraft_engine::EngineError::BadParams { cmd: "file.open".into(), msg })?;
        let title = import.document.name.clone();
        let path_string = path.to_string_lossy().into_owned();
        engine.open_document(import.document, Some(path_string.clone()));
        // File › Open Recent bookkeeping (Preferences › File Handling): de-duplicated, newest
        // first, capped at "Recent File List Contains".
        let cap = engine.prefs.get().file_handling.recent_file_count.min(100) as usize;
        let mut recents = engine.prefs.get().file_handling.recent_files.clone();
        recents.retain(|p| p != &path_string);
        recents.insert(0, path_string);
        recents.truncate(cap);
        engine.prefs.edit(|p| p.file_handling.recent_files = recents);
        Ok(title)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn engine_executor_runs_file_new() {
        let engine = Arc::new(Mutex::new(Engine::new()));
        let mut app = PhotocraftApp::new(Engine::new());
        // The app's own engine and the shared engine are the same session in production;
        // here the executor is exercised against the shared handle.
        let changed = EngineExecutor::default().execute(&mut app, &engine, "file.new", &serde_json::json!({"width": 32, "height": 32})).unwrap();
        assert!(changed);
        assert_eq!(engine.lock().unwrap().documents().len(), 1);
    }

    #[test]
    fn open_document_reports_read_errors() {
        let mut engine = Engine::new();
        let err = EngineExecutor::default().open_document(&mut engine, Path::new("/nonexistent/never.png")).unwrap_err();
        assert!(err.to_string().contains("cannot read"));
    }
}
