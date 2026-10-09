//! End-to-end shell tests: the real `PhotoCraftShell` mounted in a real
//! `WidgetArena` and driven through `EventRouter` — the same dispatch path
//! the windowed runner uses — with assertions on live engine state.
//!
//! View-state assertions (active tool, zoom, status text) live in the
//! shell's unit tests where the widget's fields are reachable; this suite
//! proves the routed surface: menu activation, shortcuts, palette and
//! canvas input, file drops.

use glam::Vec2;
use martensite::core::{ColdNode, DropPayload, HotNode, LayoutContext, NodeFlags, Rect, WidgetArena};
use martensite::window::WindowId;
use martensite::window::event::{DropAction, DropEvent, EventRouter, ModifierKeys, MouseButton, PointerEvent, PointerId, PointerKind, PointerState};
use photocraft_engine::Engine;
use photocraft_ui_martensite::PhotocraftApp;
use photocraft_ui_martensite::shell::PhotoCraftShell;

const WIN_W: f32 = 1280.0;
const WIN_H: f32 = 800.0;

struct Ui {
    arena: WidgetArena,
    router: EventRouter,
    root: martensite::core::WidgetId,
    win: WindowId,
    engine: std::sync::Arc<std::sync::Mutex<Engine>>,
}

fn mount() -> Ui {
    // The PhotoCraft overlay pack — `photo.*` glyph names resolve here just
    // like in the windowed runner (thread-local ambient install).
    std::mem::forget(martensite::icons::install_ambient_icons(photocraft_ui_martensite::icons::ambient_set()));
    let app = PhotocraftApp::new(Engine::new());
    let engine = app.engine.clone();
    let shell = PhotoCraftShell::new(app);
    let mut arena = WidgetArena::new();
    // The windowed runner installs a shared text painter; without it the
    // menu buttons measure zero width and clicks never land.
    arena.set_text_painter(martensite::text_paint::shared_painter());
    let root = arena.insert(
        HotNode { bounds: Rect::new(0.0, 0.0, WIN_W, WIN_H), flags: NodeFlags::VISIBLE | NodeFlags::HIT_TEST_ENABLED, ..HotNode::default() },
        ColdNode::new(Box::new(shell)),
    );
    let r = Rect::new(0.0, 0.0, WIN_W, WIN_H);
    arena.overlay_mut().set_viewport(r);
    if let Some((hot, cold)) = arena.get_both_mut(root) {
        hot.bounds = r;
        cold.widget.layout(&mut LayoutContext { hot, scale: 1.0 }, r);
    }
    Ui { arena, router: EventRouter::new(), root, win: WindowId::from_raw(1), engine }
}

impl Ui {
    fn pointer(&self, x: f32, y: f32, state: PointerState) -> PointerEvent {
        PointerEvent {
            pointer_id: PointerId::PRIMARY,
            kind: PointerKind::Mouse,
            position: Vec2::new(x, y),
            state,
            button: if matches!(state, PointerState::Moved) { None } else { Some(MouseButton::Left) },
            modifiers: ModifierKeys::empty(),
        }
    }

    fn click(&mut self, x: f32, y: f32) {
        let press = self.pointer(x, y, PointerState::Pressed);
        self.router.dispatch_pointer_event(&mut self.arena, self.root, self.win, &press);
        let release = self.pointer(x, y, PointerState::Released);
        self.router.dispatch_pointer_event(&mut self.arena, self.root, self.win, &release);
    }

    fn key(&mut self, key: &str, down: bool) {
        self.router.dispatch_keyboard_event(&mut self.arena, Some(self.root), key, down, false);
    }

    fn frame(&mut self) {
        self.arena.sync_overlays();
        self.arena.tick(std::time::Duration::from_millis(16));
    }

    fn docs(&self) -> usize {
        self.engine.lock().unwrap_or_else(std::sync::PoisonError::into_inner).documents().len()
    }

    /// Opens the menu whose button contains `menu_x` and clicks row `item`,
    /// then runs a frame so the activation drains into command dispatch.
    fn menu_click(&mut self, menu_x: f32, item: usize) {
        self.click(menu_x, 13.0); // the menu-button strip (menubar is 26px tall)
        self.arena.sync_overlays();
        let entry = self.arena.overlay().entries().next().and_then(|e| self.arena.overlay().entry_bounds(e.id())).expect("menu popup open");
        // Row i centre: popup pad (4pt) + 26pt rows.
        let row_y = entry.min_y() + 4.0 + item as f32 * 26.0 + 13.0;
        self.click(entry.min_x() + entry.width() / 2.0, row_y);
        self.frame();
    }
}

/// The regression test for "couldn't start a new project": a real click on
/// File, then a real click on the first row, must run `file.new`.
#[test]
fn file_new_from_the_menu_creates_a_document() {
    let mut ui = mount();
    assert_eq!(ui.docs(), 0);
    ui.menu_click(20.0, 0); // File ▸ New…
    assert_eq!(ui.docs(), 1, "File ▸ New must create a document end-to-end");
}

#[test]
fn shortcut_new_document() {
    let mut ui = mount();
    ui.key("Control", true);
    ui.key("n", true);
    ui.key("n", false);
    ui.key("Control", false);
    ui.frame();
    assert_eq!(ui.docs(), 1, "Ctrl+N must create a document");
}

#[test]
fn palette_and_canvas_routes_do_not_dead_end() {
    let mut ui = mount();
    // Every palette cell — click, frame, no panic; then a canvas gesture per
    // modal tool class (drag for Brush, click for Zoom, drag for Hand).
    for i in 0..12 {
        ui.click(22.0, 48.0 + i as f32 * 40.0);
        ui.frame();
    }
    // Brush stroke across the canvas.
    let press = ui.pointer(600.0, 400.0, PointerState::Pressed);
    ui.router.dispatch_pointer_event(&mut ui.arena, ui.root, ui.win, &press);
    let mv = ui.pointer(660.0, 440.0, PointerState::Moved);
    ui.router.dispatch_pointer_event(&mut ui.arena, ui.root, ui.win, &mv);
    let release = ui.pointer(660.0, 440.0, PointerState::Released);
    ui.router.dispatch_pointer_event(&mut ui.arena, ui.root, ui.win, &release);
    // Scroll-zoom: scroll routes to the hovered widget — hover the canvas
    // first, then deliver the delta.
    let hover = ui.pointer(600.0, 400.0, PointerState::Moved);
    ui.router.dispatch_pointer_event(&mut ui.arena, ui.root, ui.win, &hover);
    ui.router.dispatch_scroll_event(&mut ui.arena, ui.win, Vec2::new(0.0, 1.0));
    ui.frame();
}

#[test]
fn drop_file_opens_a_document() {
    let mut ui = mount();
    ui.engine.lock().unwrap_or_else(std::sync::PoisonError::into_inner).execute("file.new", serde_json::json!({"width": 16, "height": 16})).expect("file.new");
    assert_eq!(ui.docs(), 1);
    let png = {
        let engine = ui.engine.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let doc = engine.active().expect("active document");
        photocraft_io::export(&doc.doc, "png", &photocraft_io::ExportOptions::default()).expect("png export").bytes
    };
    let path = std::env::temp_dir().join(format!("photocraft-e2e-drop-{}.png", std::process::id()));
    std::fs::write(&path, &png).expect("write temp png");

    ui.router.dispatch_drop_event(&mut ui.arena, ui.root, ui.win, &DropEvent::Entered { position: Some(Vec2::new(700.0, 400.0)), action: DropAction::Copy });
    ui.router.dispatch_drop_event(&mut ui.arena, ui.root, ui.win, &DropEvent::Dropped { action: DropAction::Copy });
    ui.router.dispatch_drop_payload(&mut ui.arena, ui.root, ui.win, DropPayload::Files(vec![path.clone()]));
    ui.frame();

    assert_eq!(ui.docs(), 2, "dropping a PNG must open it as a second document");
    let _ = std::fs::remove_file(&path);
}

#[test]
fn every_tool_glyph_resolves_an_icon() {
    // The palette must paint icons, not fallback text — resolve every glyph
    // through the ambient set the app installs (overlay + builtin).
    let set = photocraft_ui_martensite::icons::ambient_set();
    for name in [
        "arrow.move",
        "photo.marquee",
        "photo.lasso",
        "edit.wand",
        "edit.crop",
        "edit.paintbrush",
        "edit.eraser",
        "photo.gradient",
        "misc.type",
        "edit.pen",
        "edit.grab",
        "nav.search",
    ] {
        assert!(set.resolve(name).is_some(), "icon {name} must resolve");
    }
}
