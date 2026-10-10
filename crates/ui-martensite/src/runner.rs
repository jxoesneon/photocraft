//! The windowed runtime: winit `ApplicationHandler` + Martensite GPU pipeline
//! (`GpuContext` → `SurfaceWrapper` → `RenderOrchestrator` with TinySkia/Vello
//! recovery through `RecoveryMachine`), one `WidgetArena` rooted at
//! [`PhotoCraftShell`], and the loopback control channel drained on the UI
//! thread.
//!
//! Mirrors Martensite's canonical `morph_viewer` host: lazy surface creation in
//! `can_create_surfaces`, `ControlFlow::Poll` so control requests and the status
//! channel drain without a repaint storm.

use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use glam::Vec2;
use martensite::core::{HotNode, LayoutContext, NodeFlags, PaintList, Rect, WidgetId};
use martensite::focus::FocusManager;
use martensite::theme::ThemeDictionary;
use martensite::wgpu::{BackdropMode, GpuContext, OrchestratorConfig, PresentModePreference, RecoveryMachine, RenderOrchestrator, SurfaceWrapper, wgpu};
use martensite::window::event::MouseButton as MButton;
use martensite::window::{EventRouter, ModifierKeys, PointerEvent, PointerId, PointerKind};
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::{ButtonSource, ElementState, MouseButton, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::icon::{Icon, RgbaIcon};
use winit::window::{Window, WindowAttributes, WindowId};

use crate::PhotocraftApp;
use crate::commands::{CommandExecutor, EngineExecutor, FileReader};
use crate::control::ControlRequest;
use crate::shell::PhotoCraftShell;

/// GPU + surface bring-up for [`RunnerConfig::gpu_factory`]. Receives the created window; the
/// returned surface must come from the same instance the context's adapter was requested on.
pub type GpuFactory = Box<dyn FnOnce(Arc<dyn Window>) -> Result<(GpuContext, wgpu::Surface<'static>), String> + Send>;

/// Everything `main` resolved before the event loop exists.
pub struct RunnerConfig {
    /// Window title.
    pub title: String,
    /// Initial inner size (logical px).
    pub inner_size: (f64, f64),
    /// Minimum inner size.
    pub min_size: (f64, f64),
    /// `false` when Preferences › Interface › System Title Bar is off and the
    /// app draws its own chrome (the shell's menu strip doubles as it).
    pub decorations: bool,
    /// Decoded window icon (RGBA8), when the bundled PNG parsed.
    pub icon: Option<(u32, u32, Vec<u8>)>,
    /// `--safe-gpu`: force the CPU presentation path this launch.
    pub safe_gpu: bool,
    /// Files passed on the command line (`photocraft a.psd …`).
    pub files: Vec<String>,
    /// Non-Unicode paths the shell reports once the window is up.
    pub unreadable_paths: Vec<String>,
    /// Queued requests from the `--control` server, drained per frame.
    pub control_rx: Option<std::sync::mpsc::Receiver<ControlRequest>>,
    /// Workspace-sandboxed file read for control-channel `file.open` requests
    /// (`--automation-read-root`); `None` reads the filesystem directly.
    pub file_reader: Option<FileReader>,
    /// Gate for commands the control channel runs (`--automation-read-root`/
    /// `--automation-write-root` allowlisting); `None`: the channel is trusted.
    pub command_authorize: Option<crate::commands::CommandAuthorize>,
    /// Custom GPU+surface bring-up — PhotoCraft's crash-safe plan
    /// (`gpu_startup::create_gpu`). Receives the created window; the returned
    /// surface must come from the same instance the context's adapter was
    /// requested on. `None`: Martensite's default platform selection.
    pub gpu_factory: Option<GpuFactory>,
    /// Background channels drained each frame on the UI thread — the async
    /// preset-store load, display-profile detection, adapter-selection notes.
    /// (`Receiver` fields should be captured inside a `Mutex`: widgets are
    /// `Send + Sync`.)
    pub drains: Vec<crate::shell::AppDrain>,
    /// Called once the window/GPU stack is up (the GPU sentinel unlock).
    pub on_started: Option<Box<dyn FnOnce() + Send>>,
    /// Startup warnings shown on the status bar.
    pub notices: Vec<String>,
    /// Linux only: open the event loop on X11 (Xwayland) — the `performance.
    /// linuxDisplayServer` preference resolved it in `main`; ignored elsewhere.
    pub prefer_x11: bool,
}

impl Default for RunnerConfig {
    fn default() -> Self {
        Self {
            title: "PhotoCraft".to_string(),
            inner_size: (1440.0, 900.0),
            min_size: (760.0, 480.0),
            decorations: true,
            icon: None,
            safe_gpu: false,
            files: Vec::new(),
            unreadable_paths: Vec::new(),
            control_rx: None,
            file_reader: None,
            command_authorize: None,
            gpu_factory: None,
            drains: Vec::new(),
            on_started: None,
            notices: Vec::new(),
            prefer_x11: false,
        }
    }
}

/// The windowed launch failed before or during startup. `gpu_init` is true when the failure is
/// in GPU/wgpu bring-up (surface, adapter or device): the GPU sentinel keeps its marker so the
/// next launch retries a safer backend. Windowing errors report `gpu_init == false`.
#[derive(Debug)]
pub struct LaunchError {
    message: String,
    /// The failure happened inside GPU initialization.
    pub gpu_init: bool,
}

impl std::fmt::Display for LaunchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for LaunchError {}

/// The windowed app state. GPU/window-bound members are created lazily in
/// `can_create_surfaces` per the winit 0.31 lifecycle.
struct Runner {
    cfg: RunnerConfig,
    arena: Option<Arc<Mutex<martensite::core::WidgetArena>>>,
    root: Option<WidgetId>,
    router: EventRouter,
    focus: Arc<Mutex<FocusManager>>,
    mods: winit::keyboard::ModifiersState,
    window: Option<Arc<dyn Window>>,
    gpu: Option<GpuContext>,
    surface: Option<SurfaceWrapper<'static>>,
    orchestrator: Option<RenderOrchestrator>,
    recovery: RecoveryMachine,
    needs_layout: bool,
    last_frame: Instant,
    /// Filled once the first frame presents (GPU sentinel bookkeeping).
    started: bool,
    /// Window/GPU bring-up failure, readable after `run_app` returns
    /// (`(message, gpu_init)`).
    launch_error: Arc<Mutex<Option<(String, bool)>>>,
}

impl Runner {
    fn new(cfg: RunnerConfig) -> Self {
        Self {
            cfg,
            arena: None,
            root: None,
            router: EventRouter::new(),
            focus: Arc::new(Mutex::new(FocusManager::new())),
            mods: winit::keyboard::ModifiersState::empty(),
            window: None,
            gpu: None,
            surface: None,
            orchestrator: None,
            recovery: RecoveryMachine::new(),
            needs_layout: true,
            last_frame: Instant::now(),
            started: false,
            launch_error: Arc::new(Mutex::new(None)),
        }
    }

    /// Record a startup failure and leave it for [`run`] to report.
    fn fail(&mut self, message: String, gpu_init: bool) {
        log::error!("{message}");
        *self.launch_error.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = Some((message, gpu_init));
    }

    fn build_arena(&mut self, scale: f32) {
        let mut arena = martensite::core::WidgetArena::new();
        arena.set_theme(ThemeDictionary::new().dark_theme().clone());
        arena.set_scale_factor(scale);
        arena.set_text_painter(martensite::text_paint::shared_painter());
        martensite::window::prefs::apply_platform_preferences(&mut arena);

        let mut hot = HotNode::default();
        hot.flags |= NodeFlags::VISIBLE | NodeFlags::HIT_TEST_ENABLED;
        let mut shell = PhotoCraftShell::new(PhotocraftApp::new(photocraft_engine::Engine::new()));
        if let Some(rx) = self.cfg.control_rx.take() {
            shell.set_control(rx, self.cfg.file_reader.clone(), self.cfg.command_authorize.clone());
        }
        shell.set_drains(std::mem::take(&mut self.cfg.drains));
        for notice in self.cfg.notices.drain(..) {
            shell.app.pending_notices.push(notice);
        }
        for path in &self.cfg.unreadable_paths {
            shell.app.pending_notices.push(format!("{path}: the path is not valid Unicode; rename the file and open it again."));
        }
        // Documents named on the command line open before the first frame; a
        // failure lands on the status bar like an interactive open error.
        let exec = EngineExecutor::default();
        for file in &self.cfg.files {
            let mut engine = shell.app.engine.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            if let Err(e) = exec.open_document(&mut engine, Path::new(file)) {
                shell.app.pending_notices.push(format!("{file}: {e}"));
            }
        }
        let root = arena.insert_with_widget(hot, Box::new(shell));
        self.focus.lock().unwrap_or_else(std::sync::PoisonError::into_inner).set_focus_unchecked(root);
        self.arena = Some(Arc::new(Mutex::new(arena)));
        self.root = Some(root);
    }

    /// Lays the root widget across the full window.
    fn layout(&mut self, w: u32, h: u32) {
        let Some(arena) = &self.arena else { return };
        let Some(root) = self.root else { return };
        let mut arena = arena.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let scale = arena.scale_factor();
        let r = Rect::new(0.0, 0.0, w as f32, h as f32);
        arena.overlay_mut().set_viewport(r);
        if let Some((hot, cold)) = arena.get_both_mut(root) {
            hot.bounds = r;
            cold.widget.layout(&mut LayoutContext { hot, scale }, r);
        }
    }

    /// One frame: tick the arena, drain the control channel, rebuild the paint
    /// list, render. The first successful frame also fires `on_started`.
    fn frame(&mut self, dt: Duration) {
        let Some(arena) = self.arena.clone() else {
            return;
        };
        let Some(root) = self.root else { return };
        {
            let mut guard = arena.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            // `tick` reaches the shell, which drains the control channel and the
            // background channels (preset store, display profiles) itself.
            guard.tick(dt);
            let mut list = PaintList::new();
            guard.build_paint_list(root, &mut list);
            drop(guard);
            let Some(window) = &self.window else { return };
            let (Some(gpu), Some(surface), Some(orchestrator)) = (&self.gpu, &mut self.surface, &mut self.orchestrator) else {
                return;
            };
            orchestrator.render(&list, &self.recovery);
            if let Err(err) = orchestrator.render_to_surface(&gpu.device, &gpu.queue, surface) {
                self.recovery.handle_surface_error(err);
                let size = window.surface_size();
                if let Err(err) = surface.resize(&gpu.device, size.width.max(1), size.height.max(1)) {
                    log::error!("surface resize failed: {err}");
                }
            }
            window.request_redraw();
            if !self.started {
                self.started = true;
                if let Some(f) = self.cfg.on_started.take() {
                    f();
                }
            }
        }
    }

    /// winit pointer event → `EventRouter` → arena dispatch.
    fn dispatch_pointer(&mut self, position: winit::dpi::PhysicalPosition<f64>, state: martensite::window::PointerState, button: Option<MButton>) {
        let mut mods = ModifierKeys::empty();
        if self.mods.shift_key() {
            mods |= ModifierKeys::SHIFT;
        }
        if self.mods.control_key() {
            mods |= ModifierKeys::CONTROL;
        }
        if self.mods.alt_key() {
            mods |= ModifierKeys::ALT;
        }
        if self.mods.meta_key() {
            mods |= ModifierKeys::COMMAND;
        }
        let ev = PointerEvent {
            pointer_id: PointerId::PRIMARY,
            kind: PointerKind::Mouse,
            position: Vec2::new(position.x as f32, position.y as f32),
            state,
            button,
            modifiers: mods,
        };
        if let (Some(arena), Some(root), Some(window)) = (self.arena.as_ref(), self.root, self.window.as_ref()) {
            self.router.dispatch_pointer_event(&mut arena.lock().unwrap_or_else(std::sync::PoisonError::into_inner), root, window.id(), &ev);
        }
    }
}

impl ApplicationHandler for Runner {
    fn can_create_surfaces(&mut self, event_loop: &dyn ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let mut attrs = WindowAttributes::default()
            .with_title(self.cfg.title.clone())
            .with_surface_size(LogicalSize::new(self.cfg.inner_size.0, self.cfg.inner_size.1))
            .with_min_surface_size(LogicalSize::new(self.cfg.min_size.0, self.cfg.min_size.1))
            .with_decorations(self.cfg.decorations);
        if let Some((w, h, rgba)) = self.cfg.icon.take()
            && let Ok(icon) = RgbaIcon::new(rgba, w, h)
        {
            attrs = attrs.with_window_icon(Some(Icon(Arc::new(icon))));
        }
        let window: Arc<dyn Window> = match event_loop.create_window(attrs) {
            Ok(w) => w.into(),
            Err(e) => {
                self.fail(format!("could not create the window: {e}"), false);
                event_loop.exit();
                return;
            }
        };

        // GPU bring-up: the app's crash-safe plan when it provided a factory,
        // else Martensite's default instance + high-performance adapter.
        let (gpu, raw_surface) = if let Some(factory) = self.cfg.gpu_factory.take() {
            match factory(Arc::clone(&window)) {
                Ok(pair) => pair,
                Err(e) => {
                    self.fail(e, true);
                    event_loop.exit();
                    return;
                }
            }
        } else {
            let instance = GpuContext::create_instance();
            let raw_surface = match instance.create_surface(Arc::clone(&window)) {
                Ok(s) => s,
                Err(e) => {
                    self.fail(format!("could not create the GPU surface: {e}"), true);
                    event_loop.exit();
                    return;
                }
            };
            match pollster::block_on(GpuContext::for_surface(&instance, &raw_surface)) {
                Ok(g) => (g, raw_surface),
                Err(e) => {
                    self.fail(format!("no usable GPU adapter: {e}"), true);
                    event_loop.exit();
                    return;
                }
            }
        };
        gpu.device.on_uncaptured_error(std::sync::Arc::new(|err| {
            log::error!("wgpu uncaptured error: {err}");
        }));

        let size = window.surface_size();
        let mut surface = SurfaceWrapper::new(raw_surface);
        surface.set_pacing(PresentModePreference::LowLatency);
        if let Err(e) = surface.configure(&gpu.device, &gpu.adapter, size.width.max(1), size.height.max(1), BackdropMode::Opaque) {
            self.fail(format!("could not configure the surface: {e}"), true);
            event_loop.exit();
            return;
        }
        let prefer_cpu = self.cfg.safe_gpu || std::env::var("MARTENSITE_CPU").is_ok();
        let mut orchestrator = match RenderOrchestrator::new(size.width.max(1), size.height.max(1), OrchestratorConfig::new(true, prefer_cpu)) {
            Ok(o) => o,
            Err(e) => {
                self.fail(format!("could not start the renderer: {e}"), true);
                event_loop.exit();
                return;
            }
        };
        let notify_window = Arc::clone(&window);
        orchestrator.set_pre_present_notify(Some(Box::new(move || {
            notify_window.pre_present_notify();
        })));

        self.build_arena(window.scale_factor() as f32);
        self.needs_layout = true;

        self.window = Some(window);
        self.gpu = Some(gpu);
        self.surface = Some(surface);
        self.orchestrator = Some(orchestrator);

        if let Some(window) = &self.window {
            window.request_redraw();
        }
    }

    fn window_event(&mut self, event_loop: &dyn ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::SurfaceResized(size) => {
                if size.width > 0 && size.height > 0 {
                    if let (Some(surface), Some(gpu)) = (&mut self.surface, &self.gpu)
                        && let Err(err) = surface.resize(&gpu.device, size.width, size.height)
                    {
                        log::error!("surface resize failed: {err}");
                    }
                    if let Some(o) = &mut self.orchestrator {
                        o.set_frame_size(size.width, size.height);
                    }
                    self.needs_layout = true;
                }
            }
            WindowEvent::ScaleFactorChanged { .. } => {
                if let Some(window) = &self.window
                    && let Some(arena) = &self.arena
                {
                    arena.lock().unwrap_or_else(std::sync::PoisonError::into_inner).set_scale_factor(window.scale_factor() as f32);
                }
                self.needs_layout = true;
            }
            WindowEvent::ModifiersChanged(m) => {
                self.mods = m.state();
            }
            WindowEvent::PointerMoved { position, primary, .. } => {
                if primary {
                    self.dispatch_pointer(position, martensite::window::PointerState::Moved, None);
                }
            }
            WindowEvent::PointerButton { state, position, button, primary, .. } => {
                if !primary {
                    return;
                }
                let btn = match button {
                    ButtonSource::Mouse(MouseButton::Left) => Some(MButton::Left),
                    ButtonSource::Mouse(MouseButton::Right) => Some(MButton::Right),
                    ButtonSource::Mouse(MouseButton::Middle) => Some(MButton::Middle),
                    _ => None,
                };
                self.dispatch_pointer(
                    position,
                    if state == ElementState::Pressed { martensite::window::PointerState::Pressed } else { martensite::window::PointerState::Released },
                    btn,
                );
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let s = self.arena.as_ref().map(|a| a.lock().unwrap_or_else(std::sync::PoisonError::into_inner).scale_factor()).unwrap_or(1.0);
                let d = match delta {
                    winit::event::MouseScrollDelta::LineDelta(x, y) => Vec2::new(x * 48.0 * s, y * 48.0 * s),
                    winit::event::MouseScrollDelta::PixelDelta(p) => Vec2::new(p.x as f32, p.y as f32),
                    _ => return,
                };
                if let (Some(arena), Some(window)) = (self.arena.as_ref(), self.window.as_ref()) {
                    self.router.dispatch_scroll_event(&mut arena.lock().unwrap_or_else(std::sync::PoisonError::into_inner), window.id(), d);
                }
            }
            WindowEvent::KeyboardInput { event, .. } => {
                let pressed = event.state == ElementState::Pressed;
                let key_name = match &event.logical_key {
                    winit::keyboard::Key::Named(n) => format!("{n:?}"),
                    winit::keyboard::Key::Character(c) => c.to_string(),
                    _ => String::new(),
                };
                let key_name = match key_name.as_str() {
                    "Space" => " ".to_string(),
                    other => other.to_string(),
                };
                if !key_name.is_empty()
                    && let (Some(arena), Some(root)) = (&self.arena, self.root)
                {
                    let mut arena = arena.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
                    self.router.dispatch_keyboard_event(&mut arena, Some(root), &key_name, pressed, event.repeat);
                    if pressed
                        && !event.repeat
                        && let Some(text) = &event.text
                    {
                        let t = text.to_string();
                        if !t.is_empty() {
                            arena.dispatch_event(root, &martensite::core::WidgetEvent::ImeCommitted { text: t });
                        }
                    }
                }
            }
            WindowEvent::Ime(ime) => {
                if let Some(ev) = martensite::window::event::ime_event_for_winit(&ime)
                    && let (Some(arena), Some(root)) = (&self.arena, self.root)
                {
                    self.router.dispatch_ime_event(&mut arena.lock().unwrap_or_else(std::sync::PoisonError::into_inner), Some(root), &ev);
                }
            }
            WindowEvent::RedrawRequested => {
                let now = Instant::now();
                let dt = now - self.last_frame;
                self.last_frame = now;
                if self.needs_layout {
                    if let Some(window) = &self.window {
                        let size = window.surface_size();
                        self.layout(size.width, size.height);
                    }
                    self.needs_layout = false;
                }
                self.frame(dt);
            }
            _ => {}
        }
    }
}

/// Windowed entry — winit + Vello/TinySkia GPU rendering through
/// `RenderOrchestrator`, polling so the control channel and seams drain
/// continuously.
///
/// # Errors
///
/// [`LaunchError`] on window/GPU bring-up failures (with `gpu_init` set for the
/// GPU sentinel) and on event-loop failures.
pub fn run(cfg: RunnerConfig) -> Result<(), LaunchError> {
    // The PhotoCraft icon overlay is the ambient family for the UI thread —
    // install before the event loop starts and keep it for the app's life.
    std::mem::forget(martensite::icons::install_ambient_icons(crate::icons::ambient_set()));
    #[cfg(target_os = "linux")]
    let event_loop = {
        let mut builder = EventLoop::builder();
        if cfg.prefer_x11 {
            use winit::platform::x11::EventLoopBuilderExtX11 as _;
            builder.with_x11();
        }
        builder.build()
    };
    #[cfg(not(target_os = "linux"))]
    let event_loop = EventLoop::new();
    let event_loop = event_loop.map_err(|e| LaunchError { message: e.to_string(), gpu_init: false })?;
    event_loop.set_control_flow(ControlFlow::Poll);
    let runner = Runner::new(cfg);
    let launch_error = runner.launch_error.clone();
    if let Err(e) = event_loop.run_app(runner) {
        return Err(LaunchError { message: e.to_string(), gpu_init: false });
    }
    if let Some((message, gpu_init)) = launch_error.lock().unwrap_or_else(std::sync::PoisonError::into_inner).take() {
        return Err(LaunchError { message, gpu_init });
    }
    Ok(())
}
