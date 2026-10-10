//! PhotoCraft desktop application — 100% sovereign Martensite runtime.

#![cfg_attr(all(target_os = "windows", not(debug_assertions)), windows_subsystem = "windows")]

mod app_dirs;
mod app_icon;
mod control_server;
mod crash_guard;
mod gpu_startup;
// Pure logic is tested on every platform; only Linux runs the check.
#[cfg(any(target_os = "linux", test))]
mod linux_libs;
mod logging;
mod monitor_profile;
mod services;
mod ui_state;

/// Windows and Linux: no OS title bar; the app's top bar is the title bar, with its own caption
/// buttons and edge resizing, as Photoshop does on Windows. macOS keeps its traffic lights over
/// the integrated title strip.
const CUSTOM_TITLEBAR: bool = !cfg!(target_os = "macos");

/// Whether this start draws its own title bar: Windows and Linux do, unless Preferences ›
/// Interface › System Title Bar asks for the system's (#1271, #1316). Read from the saved
/// preferences before the window opens; a missing or unreadable file keeps the default.
fn custom_titlebar(prefs_file: Option<&std::path::Path>) -> bool {
    let prefs: photocraft_engine::prefs::Preferences =
        prefs_file.and_then(|p| std::fs::read_to_string(p).ok()).and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
    CUSTOM_TITLEBAR && !prefs.interface.system_title_bar
}

/// Parse a `--control` / `PHOTOCRAFT_CONTROL_PORT` value. An unparseable port is an error that
/// names the value (issue #701): silently running with no control server leaves a launcher or
/// agent unable to tell a typo from a successful grant.
fn parse_control_port(value: &str, source: &str) -> Result<u16, String> {
    value.trim().parse().map_err(|_| format!("{source}: `{value}` is not a valid port (expected a number from 0 to 65535)"))
}

/// Process exit status for the collected `--control` / `PHOTOCRAFT_CONTROL_PORT` errors: `None`
/// when there are none, else 2 (a command-line usage error), never 0.
fn control_args_exit_code(errors: &[String]) -> Option<i32> {
    if errors.is_empty() { None } else { Some(2) }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // First, so the panic hook and every start-up warning are recorded (`logging`).
    let logger = logging::install();
    crash_guard::install_hook();
    let mut control_port: Option<u16> = None;
    let mut control_arg_errors: Vec<String> = Vec::new();
    if let Some(value) = std::env::var("PHOTOCRAFT_CONTROL_PORT").ok().filter(|v| !v.trim().is_empty()) {
        match parse_control_port(&value, "PHOTOCRAFT_CONTROL_PORT") {
            Ok(port) => control_port = Some(port),
            Err(error) => control_arg_errors.push(error),
        }
    }
    let mut control_token = None;
    let mut control_token_file = None;
    let mut automation_read_root = std::env::var_os("PHOTOCRAFT_AUTOMATION_READ_ROOT").map(std::path::PathBuf::from);
    let mut automation_write_root = std::env::var_os("PHOTOCRAFT_AUTOMATION_WRITE_ROOT").map(std::path::PathBuf::from);
    let mut files = Vec::new();
    // Arguments that are not valid Unicode (issue #1108): a file manager can hand over a Latin-1
    // file name on Linux, and `std::env::args()` would panic before any window opened. Paths are
    // reported in a notice once the app is up; a control token is a usage error like a bad port.
    let mut unreadable_paths: Vec<String> = Vec::new();
    let mut safe_gpu = false;
    let mut args = std::env::args_os().skip(1);
    while let Some(a) = args.next() {
        match a.to_str() {
            Some("--control") => match args.next() {
                Some(value) => match parse_control_port(&value.to_string_lossy(), "--control") {
                    Ok(port) => control_port = Some(port),
                    Err(error) => control_arg_errors.push(error),
                },
                None => control_arg_errors.push("--control: missing port value (expected `--control <port>`)".to_string()),
            },
            Some("--control-token") => match args.next().map(std::ffi::OsString::into_string) {
                Some(Ok(token)) => control_token = Some(token),
                Some(Err(raw)) => control_arg_errors.push(format!("--control-token: value is not valid Unicode (`{}`)", raw.to_string_lossy())),
                None => control_token = None,
            },
            Some("--control-token-file") => control_token_file = args.next().map(std::path::PathBuf::from),
            Some("--automation-read-root") => automation_read_root = args.next().map(std::path::PathBuf::from),
            Some("--automation-write-root") => automation_write_root = args.next().map(std::path::PathBuf::from),
            Some("--safe-gpu") => safe_gpu = true,
            Some("--in-window-menus") => {}
            Some("--version") => {
                println!("photocraft {}", photocraft_engine::build_info::long_version());
                return Ok(());
            }
            // Old macOS passes a process serial number when launched from Finder.
            Some(s) if s.starts_with("-psn_") => {}
            Some(s) => files.push(s.to_owned()),
            None => unreadable_paths.push(a.to_string_lossy().into_owned()),
        }
    }

    // A malformed control port must not silently drop the control server (issue #701): name the
    // bad value and fail the launch, like `photocraft-cli serve --port` does for the same typo.
    // Exit status 2 is the usual command-line usage error, so a launcher sees the failure.
    if let Some(code) = control_args_exit_code(&control_arg_errors) {
        for error in &control_arg_errors {
            eprintln!("photocraft: {error}");
        }
        std::process::exit(code);
    }

    // The log file lives under the settings directory; opened after the arguments, so `--version`
    // and usage errors leave no file behind. Records logged until now are written to it first.
    if let (Some(logger), Some(dir)) = (logger, services::config_dir()) {
        match logger.attach_dir(&dir.join("logs")) {
            Ok(path) => log::info!("PhotoCraft {}, log file {}", photocraft_engine::build_info::long_version(), path.display()),
            // Standard error only by now (`attach_dir` gave up on the file); unlike `eprintln!`, never panics.
            Err(e) => log::warn!("no log file: {e}"),
        }
    }

    // The display server to open the window on: Xwayland for a pen on Wayland, which gives this
    // app no pen input (#639).
    #[cfg(target_os = "linux")]
    let session = linux_libs::session_from_env(|k| std::env::var(k).ok());
    // Preferences › Performance › Linux display server = X11: Xwayland on a Wayland session too,
    // where native file drops work (winit 0.30 has none on Wayland, #386). Only when Xwayland can
    // run the window ($DISPLAY set, X11 libraries installed); otherwise the session's own.
    #[cfg(target_os = "linux")]
    let session = if session == linux_libs::DisplaySession::Wayland
        && gpu_startup::read_display_server(services::prefs_file().as_deref()) == photocraft_engine::prefs::LinuxDisplayServer::X11
        && std::env::var_os("DISPLAY").is_some_and(|d| !d.is_empty())
        && linux_libs::available(linux_libs::DisplaySession::X11)
    {
        eprintln!("photocraft: opening the window through Xwayland (Preferences › Performance › Linux display server)");
        linux_libs::DisplaySession::X11
    } else {
        session
    };

    // winit and wgpu dlopen the windowing and GPU libraries, and some of those crates panic when
    // one is missing (issue #201). Name the package to install and exit instead.
    #[cfg(target_os = "linux")]
    if let Err(message) = linux_libs::preflight(session) {
        eprint!("{message}");
        std::process::exit(1);
    }

    // The control channel's shared secret and its file-system sandbox: without a supplied token
    // one is generated and printed so the launcher can hand it to agents.
    let control = if let Some(port) = control_port {
        let (supplied, token_file) = photocraft_automation::security::token_inputs(control_token, control_token_file);
        let token = match photocraft_automation::security::server_token(supplied.as_deref(), token_file.as_deref()) {
            Ok(token) => token,
            Err(e) => {
                eprintln!("photocraft: cannot configure control authentication: {e}");
                return Ok(());
            }
        };
        if let Some(path) = token_file {
            eprintln!("photocraft: control token file: {}", path.display());
        } else if supplied.is_none() {
            eprintln!("photocraft: control token: {token}");
        } else {
            eprintln!("photocraft: using supplied control token");
        }
        Some((port, token))
    } else {
        None
    };
    let automation_workspace = photocraft_automation::AuthorizedWorkspace::new(automation_read_root.as_deref(), automation_write_root.as_deref())
        .map_err(|error| format!("photocraft: cannot configure automation workspace: {error}"))?;

    // Read the displays' ICC profiles while the window opens (colour-managed canvas).
    let monitor = monitor_profile::detect_async();
    // Brush presets load in the background; the app attaches them when they arrive.
    let presets = services::presets_dir().map(photocraft_engine::preset_store::open_dir_async);
    let custom_titlebar = custom_titlebar(services::prefs_file().as_deref());
    // The Martensite window keeps its own geometry; drop stale eframe persistence keys that would
    // mislead a future reader of the settings file.
    ui_state::sanitize(services::config_dir().map(|dir| dir.join("ui.ron")).as_deref());
    // Crash-safe GPU startup (#4): pick the backend (a marker left by a start that died in the
    // driver moves to a safer one), and lock this start's marker until the first frames render.
    let os = gpu_startup::Os::current();
    let (pref, mode) = gpu_startup::read_rendering_prefs(services::prefs_file().as_deref());
    let (previous, sentinel) = match services::config_dir() {
        Some(dir) => gpu_startup::Sentinel::begin(&dir),
        None => (gpu_startup::Previous::Clean, None),
    };
    let env_backend = std::env::var("WGPU_BACKEND").ok();
    let plan = gpu_startup::plan_with_mode(pref, mode, previous.crashed(), env_backend.as_deref(), safe_gpu, os);
    if let Some(m) = previous.crashed() {
        log::warn!("the previous start didn't finish (GPU backend {}, adapter {:?}); {}", m.backend, m.adapter, plan.reason.as_deref().unwrap_or(""));
    }
    let sentinel: gpu_startup::SharedSentinel = std::sync::Arc::new(std::sync::Mutex::new(sentinel));
    if let Some(s) = sentinel.lock().unwrap_or_else(std::sync::PoisonError::into_inner).as_mut() {
        let backend = match &plan.env {
            Some(v) => format!("env:{v}"),
            None => plan.backend.name().to_string(),
        };
        let marker = gpu_startup::Marker { backend, version: photocraft_engine::build_info::long_version().to_string(), ..Default::default() };
        if let Err(e) = s.write(marker) {
            log::warn!("GPU startup marker: {e}");
        }
    }
    // Retry in a fresh process on renderer-init failure only (`--safe-gpu` already is the CPU path).
    let prefer_cpu = !safe_gpu && plan.backend != photocraft_engine::prefs::GpuBackend::Cpu;

    // The control server runs on its own thread; requests queue for the UI thread. The runner
    // polls (`ControlFlow::Poll` + continuous redraws), so a no-op wake is enough — requests are
    // drained every frame.
    let control_rx = control.map(|(port, token)| control_server::start(port, token, std::sync::Arc::new(|| {})));

    // A remark the GPU setup raises (the Intel-on-Windows DX12 default, a missing software
    // adapter under `cpu`): the status bar shows it once the shell exists.
    let gpu_note = std::sync::Arc::new(std::sync::Mutex::new(None::<String>));

    let mut cfg = photocraft_ui_martensite::runner::RunnerConfig {
        title: "PhotoCraft".to_string(),
        decorations: !custom_titlebar,
        icon: app_icon::window_icon().map(|i| (i.width, i.height, i.rgba)),
        safe_gpu: plan.backend == photocraft_engine::prefs::GpuBackend::Cpu,
        files: files.clone(),
        unreadable_paths,
        control_rx,
        file_reader: automation_read_root.is_some().then(|| {
            std::sync::Arc::new(move |path: &std::path::Path| automation_workspace.read(&path.to_string_lossy()).map_err(|e| e.to_string()))
                as photocraft_ui_martensite::commands::FileReader
        }),
        command_authorize: Some(std::sync::Arc::new(|id: &str, params: &serde_json::Value| {
            photocraft_automation::workspace::authorize_desktop_engine_command(id, params).map_err(|e| e.to_string())
        })),
        gpu_factory: Some(Box::new({
            let sentinel = sentinel.clone();
            let note = gpu_note.clone();
            move |window| gpu_startup::create_gpu(window, &plan, os, sentinel, note)
        })),
        #[cfg(target_os = "linux")]
        prefer_x11: session == linux_libs::DisplaySession::X11,
        ..Default::default()
    };
    if let Some(rx) = presets {
        let rx = std::sync::Mutex::new(rx);
        cfg.drains.push(Box::new(move |app| {
            let opened = rx.lock().unwrap_or_else(std::sync::PoisonError::into_inner).try_recv();
            if let Ok(opened) = opened {
                let mut engine = app.engine.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
                for note in engine.attach_preset_store(opened) {
                    app.pending_notices.push(note);
                }
            }
        }));
    }
    if let Some(rx) = monitor {
        let rx = std::sync::Mutex::new(rx);
        cfg.drains.push(Box::new(move |_app| {
            let detection = rx.lock().unwrap_or_else(std::sync::PoisonError::into_inner).try_recv();
            if let Ok(detection) = detection {
                match detection {
                    Ok(displays) => log::info!("display profiles: {} display(s)", displays.len()),
                    Err(e) => log::warn!("display profiles: {e}"),
                }
            }
        }));
    }
    cfg.drains.push(Box::new({
        let note = gpu_note.clone();
        move |app| {
            if let Some(n) = note.lock().unwrap_or_else(std::sync::PoisonError::into_inner).take() {
                app.pending_notices.push(n);
            }
        }
    }));
    {
        let sentinel = sentinel.clone();
        cfg.on_started = Some(Box::new(move || {
            if let Some(s) = sentinel.lock().unwrap_or_else(std::sync::PoisonError::into_inner).take() {
                s.finish();
            }
        }));
    }
    if let Some(w) = &app_dirs::current().warning {
        cfg.notices.push(format!("Portable mode is off: {w}"));
    }

    let result = photocraft_ui_martensite::runner::run(cfg);
    let gpu_init_failed = result.as_ref().err().is_some_and(|e| e.gpu_init);
    // Closed or failed outside graphics initialization: not a driver crash. A renderer
    // error keeps the marker, so the next start tries a safer backend.
    if !gpu_startup::keep_marker_after_run(gpu_init_failed)
        && let Some(mut s) = sentinel.lock().unwrap_or_else(std::sync::PoisonError::into_inner).take()
    {
        if result.is_err()
            && let Some(marker) = previous.crashed()
        {
            // This failure supplies no new graphics-crash evidence: retain the previous
            // marker, rather than recording this attempt's fallback backend.
            if let Err(error) = s.write(marker.clone()) {
                log::warn!("couldn't restore previous GPU startup marker: {error}");
            }
        } else {
            s.finish();
        }
    }
    // Retry in a fresh process: winit event loops cannot be recreated reliably in-process.
    // Only renderer initialization failures qualify; never restart after editing has begun.
    if prefer_cpu && gpu_init_failed {
        let reason = result.as_ref().err().map(ToString::to_string).unwrap_or_default();
        if let Some(s) = sentinel.lock().unwrap_or_else(std::sync::PoisonError::into_inner).take() {
            s.finish();
        }
        if let Ok(exe) = std::env::current_exe() {
            let launched = std::process::Command::new(exe)
                .args(std::env::args_os().skip(1))
                .arg("--safe-gpu")
                .env_remove("WGPU_BACKEND")
                .env("PHOTOCRAFT_GPU_STARTUP_FAILURE", &reason)
                .spawn();
            match launched {
                Ok(_) => return Ok(()),
                Err(error) => log::error!("could not start CPU compatibility mode: {error}"),
            }
        }
    }
    result.map_err(|e| -> Box<dyn std::error::Error> { Box::new(e) })
}

#[cfg(test)]
mod control_port_tests {
    use super::{control_args_exit_code, parse_control_port};

    #[test]
    fn control_port_errors_exit_non_zero() {
        assert_eq!(control_args_exit_code(&[]), None);
        let errors = vec![parse_control_port("nope", "--control").unwrap_err()];
        assert_eq!(control_args_exit_code(&errors), Some(2));
    }

    #[test]
    fn control_port_accepts_valid_numbers() {
        assert_eq!(parse_control_port("50494", "--control"), Ok(50494));
        assert_eq!(parse_control_port(" 8080 ", "--control"), Ok(8080));
        assert_eq!(parse_control_port("0", "PHOTOCRAFT_CONTROL_PORT"), Ok(0));
    }

    #[test]
    fn control_port_rejects_unparseable_values_naming_them() {
        // Out of range, a following flag eaten as the value, and an empty value all fail,
        // naming the offending value in the message.
        let err = parse_control_port("78787", "--control").unwrap_err();
        assert!(err.contains("78787"), "{err}");
        assert!(parse_control_port("--safe-gpu", "--control").is_err());
        assert!(parse_control_port("", "--control").is_err());
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_system_title_bar_preference_keeps_the_window_decorations() {
        // #1271, #1316: Preferences › Interface › System Title Bar keeps the system
        // decorations on Windows and Linux; macOS always has them.
        let dir = std::env::temp_dir().join(format!("pc-titlebar-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("prefs.json");
        assert_eq!(super::custom_titlebar(None), super::CUSTOM_TITLEBAR, "no preferences file: the default");
        std::fs::write(&file, "not json").unwrap();
        assert_eq!(super::custom_titlebar(Some(&file)), super::CUSTOM_TITLEBAR, "an unreadable file: the default");
        std::fs::write(&file, r#"{"interface":{"systemTitleBar":true}}"#).unwrap();
        assert!(!super::custom_titlebar(Some(&file)));
        std::fs::write(&file, r#"{"interface":{"systemTitleBar":false}}"#).unwrap();
        assert_eq!(super::custom_titlebar(Some(&file)), super::CUSTOM_TITLEBAR);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
