//! The PhotoCraft shell: a real Martensite widget tree — menu bar, tool palette,
//! document canvas and status bar — dispatched through the engine's command
//! surface (`Session::execute`) and the Photoshop keyboard ergonomics in
//! [`crate::shortcuts`].

use glam::Vec2;
use martensite::core::{
    DropPayload, EventContext, EventResponse, HotNode, LayoutConstraints, LayoutContext, PaintContext, Rect, TokenKey, Widget, WidgetEvent,
};
use martensite::widgets::menu::MenuItem;
use martensite::widgets::menu_bar::MenuBar;
use martensite::widgets::status_bar::StatusBar;
use martensite::widgets::tool_palette::{ToolItem, ToolPalette};
use photocraft_engine::{Engine, Tool};

use crate::PhotocraftApp;
use crate::commands::{CommandExecutor, EngineExecutor};

/// `PaintList` rect primitives take `kurbo` rects; node bounds are
/// `martensite::core::Rect` — convert at the call site.
fn kr(r: Rect) -> kurbo::Rect {
    kurbo::Rect::new(f64::from(r.min_x()), f64::from(r.min_y()), f64::from(r.max_x()), f64::from(r.max_y()))
}

/// Height of the menu strip, in logical px at scale 1.
const MENUBAR_H: f32 = 26.0;
/// Width of the left tool column, in logical px at scale 1.
const PALETTE_W: f32 = 46.0;
/// Height of the status strip, in logical px at scale 1.
const STATUS_H: f32 = 24.0;

/// The tool set in declaration order — the palette row index is the index here.
const TOOLS: &[Tool] = &[
    Tool::Move,
    Tool::Marquee,
    Tool::Lasso,
    Tool::QuickSelection,
    Tool::Crop,
    Tool::Brush,
    Tool::Eraser,
    Tool::Gradient,
    Tool::Type,
    Tool::Pen,
    Tool::Hand,
    Tool::Zoom,
];

/// Palette glyphs are namespaced icon names resolved through the ambient
/// icon family (`crate::icons` overlay over the builtin pack).
const TOOL_GLYPHS: &[&str] = &[
    "arrow.move",      // Move
    "photo.marquee",   // Marquee
    "photo.lasso",     // Lasso
    "edit.wand",       // Quick Selection
    "edit.crop",       // Crop
    "edit.paintbrush", // Brush
    "edit.eraser",     // Eraser
    "photo.gradient",  // Gradient
    "misc.type",       // Type
    "edit.pen",        // Pen
    "edit.grab",       // Hand
    "nav.search",      // Zoom
];
const TOOL_NAMES: &[&str] = &["Move", "Marquee", "Lasso", "Quick Selection", "Crop", "Brush", "Eraser", "Gradient", "Type", "Pen", "Hand", "Zoom"];

fn tool_index(tool: Tool) -> usize {
    TOOLS.iter().position(|&t| t == tool).unwrap_or(0)
}

/// One menu slot: a leaf command or a submenu's command list.
enum MenuCmd {
    Cmd(&'static str),
    Sub(Vec<&'static str>),
}

impl MenuCmd {
    /// Resolves a `MenuPath` tail (`path[1..]`) against this slot.
    fn get(&self, rest: &[usize]) -> Option<&'static str> {
        match (self, rest) {
            (MenuCmd::Cmd(id), []) => Some(*id),
            (MenuCmd::Sub(ids), [i, ..]) => ids.get(*i).copied(),
            _ => None,
        }
    }
}

/// `(label, items)` per top-level menu.
type MenuModel = Vec<(&'static str, Vec<MenuItem>)>;

/// Builds the menu model from the engine's real command surface:
/// `(label, items)` per top-level menu plus `menu_commands[menu][item]`
/// in the same declaration order the bar's `MenuPath` ([menu, item, …])
/// reports.
fn menu_model(engine: &Engine) -> (MenuModel, Vec<Vec<MenuCmd>>) {
    // First-level menus, in the order the engine declares them (deduped).
    let mut menu_names: Vec<&'static str> = Vec::new();
    for spec in photocraft_engine::command_specs() {
        if let Some(&top) = spec.menu.first()
            && !menu_names.contains(&top)
        {
            menu_names.push(top);
        }
    }
    let mut menus: Vec<(&'static str, Vec<MenuItem>)> = Vec::new();
    let mut commands: Vec<Vec<MenuCmd>> = Vec::new();
    for name in menu_names {
        let mut items: Vec<MenuItem> = Vec::new();
        let mut slots: Vec<MenuCmd> = Vec::new();
        for spec in photocraft_engine::command_specs() {
            if spec.menu.first() != Some(&name) {
                continue;
            }
            let enabled = engine.is_enabled(spec.id);
            match spec.menu.len() {
                1 => {
                    items.push(MenuItem::action(spec.label).enabled(enabled));
                    slots.push(MenuCmd::Cmd(spec.id));
                }
                _ => {
                    // Two-level menus (e.g. Image › Adjustments › Invert) —
                    // consecutive specs sharing the second path element fold
                    // into one submenu.
                    let sub_name = spec.menu[1];
                    match items.last_mut() {
                        Some(MenuItem::Submenu { label, items: sub, .. }) if label == sub_name => {
                            sub.push(MenuItem::action(spec.label).enabled(enabled));
                            if let Some(MenuCmd::Sub(ids)) = slots.last_mut() {
                                ids.push(spec.id);
                            }
                        }
                        _ => {
                            items.push(MenuItem::submenu(sub_name, vec![MenuItem::action(spec.label).enabled(enabled)]));
                            slots.push(MenuCmd::Sub(vec![spec.id]));
                        }
                    }
                }
            }
        }
        if !items.is_empty() {
            menus.push((name, items));
            commands.push(slots);
        }
    }
    (menus, commands)
}

/// Builds the menu bar from the engine's real command surface.
/// Returns the bar plus `menu_commands[menu][item]` in the same declaration
/// order the bar's `MenuPath` ([menu, item, …]) reports.
fn build_menubar(engine: &Engine) -> (MenuBar, Vec<Vec<MenuCmd>>) {
    let (menus, commands) = menu_model(engine);
    let mut bar = MenuBar::new();
    for (label, items) in menus {
        bar = bar.menu(label, items);
    }
    (bar, commands)
}

/// The document canvas: checkerboard, the active document's bounds, brush
/// marks while the Brush tool is down, and pan/zoom. Tool gestures map to the
/// Photoshop model — Hand drags, Zoom click magnifies around the clicked
/// point, wheel zooms at the cursor.
struct CanvasArea {
    tool: Tool,
    zoom: f32,
    pan: [f32; 2],
    doc: Option<(u32, u32)>,
    /// Set when a document arrives before the canvas is laid out — the first
    /// `layout`/`paint` with real bounds fits it like Photoshop's Fit on Screen.
    pending_fit: bool,
    marks: Vec<(Vec2, Vec2, [u8; 4])>,
    stroke_open: bool,
    last: Option<Vec2>,
    fg: [u8; 4],
    bounds: Rect,
    scale: f32,
}

impl CanvasArea {
    fn new() -> Self {
        Self {
            tool: Tool::Move,
            zoom: 1.0,
            pan: [0.0, 0.0],
            doc: None,
            pending_fit: false,
            marks: Vec::new(),
            stroke_open: false,
            last: None,
            fg: [12, 12, 14, 255],
            bounds: Rect::new(0.0, 0.0, 0.0, 0.0),
            scale: 1.0,
        }
    }

    /// Canvas-space point for a window-space position.
    fn to_canvas(&self, p: Vec2) -> Vec2 {
        let c = Vec2::new(self.bounds.min_x() + self.bounds.width() / 2.0, self.bounds.min_y() + self.bounds.height() / 2.0);
        (p - c - Vec2::new(self.pan[0], self.pan[1]) * self.scale) / (self.zoom * self.scale)
    }

    fn zoom_at(&mut self, factor: f32, cursor: Vec2) {
        let old = self.zoom;
        let new = (self.zoom * factor).clamp(0.02, 64.0);
        if old == 0.0 {
            return;
        }
        let ratio = new / old;
        let cx = self.bounds.min_x() + self.bounds.width() / 2.0;
        let cy = self.bounds.min_y() + self.bounds.height() / 2.0;
        self.pan[0] = (cursor.x - cx) / self.scale - ((cursor.x - cx) / self.scale - self.pan[0]) * ratio;
        self.pan[1] = (cursor.y - cy) / self.scale - ((cursor.y - cy) / self.scale - self.pan[1]) * ratio;
        self.zoom = new;
    }

    /// The doc `w`×`h` centred at a zoom that fills the canvas with a small
    /// margin, capped at 100% — small documents open at their pixel size.
    fn fit_doc(&mut self, w: u32, h: u32) {
        let (bw, bh) = (self.bounds.width(), self.bounds.height());
        if w == 0 || h == 0 || bw <= 0.0 || bh <= 0.0 {
            return;
        }
        let denom = self.scale.max(0.01);
        let fit = (bw / (w as f32 * denom)).min(bh / (h as f32 * denom)) * 0.92;
        self.zoom = fit.min(1.0).clamp(0.02, 64.0);
        self.pan = [0.0, 0.0];
    }
}

impl Widget for CanvasArea {
    fn measure(&mut self, _cx: &mut LayoutContext, _c: LayoutConstraints) -> Vec2 {
        Vec2::new(400.0, 300.0)
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.bounds = bounds;
        self.scale = cx.scale;
        if self.pending_fit {
            if let Some((w, h)) = self.doc {
                self.fit_doc(w, h);
            }
            self.pending_fit = false;
        }
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        match cx.event {
            WidgetEvent::PointerPressed { position, .. } => {
                self.last = Some(*position);
                match self.tool {
                    Tool::Brush => {
                        self.stroke_open = true;
                        let p = self.to_canvas(*position);
                        self.marks.push((p, p, self.fg));
                    }
                    Tool::Zoom => self.zoom_at(1.25, *position),
                    _ => {}
                }
                EventResponse::RequestRepaint
            }
            WidgetEvent::PointerMoved { position } => {
                if self.last.is_none() {
                    return EventResponse::Ignored;
                }
                let prev = self.last.unwrap_or(*position);
                match self.tool {
                    Tool::Hand => {
                        let d = (*position - prev) / self.scale;
                        self.pan[0] += d.x;
                        self.pan[1] += d.y;
                    }
                    Tool::Brush if self.stroke_open => {
                        let a = self.to_canvas(prev);
                        let b = self.to_canvas(*position);
                        self.marks.push((a, b, self.fg));
                    }
                    _ => return EventResponse::Ignored,
                }
                self.last = Some(*position);
                EventResponse::RequestRepaint
            }
            WidgetEvent::PointerReleased { .. } => {
                self.stroke_open = false;
                self.last = None;
                EventResponse::Handled
            }
            WidgetEvent::Scroll { position, delta } => {
                if !self.bounds.contains(*position) {
                    return EventResponse::Ignored;
                }
                let factor = if delta.y > 0.0 { 1.1 } else { 1.0 / 1.1 };
                self.zoom_at(factor, *position);
                EventResponse::RequestRepaint
            }
            _ => EventResponse::Ignored,
        }
    }

    fn paint(&self, cx: &mut PaintContext) {
        let b = cx.bounds;
        if b.width() <= 0.0 {
            return;
        }
        let s = self.scale;
        let gutter = cx.color(TokenKey::SurfaceColor, [24, 25, 30, 255]);
        cx.list.push_fill_rect(kr(b), gutter);
        let Some((w, h)) = self.doc else {
            return;
        };
        // The document can be larger than the viewport at high zoom — its
        // cells and strokes clip to the canvas, never over the chrome.
        cx.list.push_clip(kr(b));
        let c = Vec2::new(b.min_x() + b.width() / 2.0 + self.pan[0] * s, b.min_y() + b.height() / 2.0 + self.pan[1] * s);
        let dw = w as f32 * self.zoom * s;
        let dh = h as f32 * self.zoom * s;
        let dr = Rect::new(c.x - dw / 2.0, c.y - dh / 2.0, dw, dh);
        // Checkerboard in the document rect — the transparency signal.
        let cell = 12.0 * s;
        let (mut y, mut row) = (dr.min_y(), 0u32);
        while y < dr.max_y() {
            let (mut x, mut col) = (dr.min_x(), row);
            while x < dr.max_x() {
                let shade = if col % 2 == 0 { [58, 58, 62, 255] } else { [74, 74, 78, 255] };
                cx.list.push_fill_rect(kr(Rect::new(x, y, (x + cell).min(dr.max_x()) - x, (y + cell).min(dr.max_y()) - y)), shade);
                x += cell;
                col += 1;
            }
            y += cell;
            row += 1;
        }
        cx.list.push_stroke_rect(kr(dr), s.max(1.0), [140, 146, 160, 255]);
        // Brush marks in canvas space.
        for (a, bp, color) in &self.marks {
            let p0 = Vec2::new(c.x + a.x * self.zoom * s, c.y + a.y * self.zoom * s);
            let p1 = Vec2::new(c.x + bp.x * self.zoom * s, c.y + bp.y * self.zoom * s);
            let mut path = kurbo::BezPath::new();
            path.move_to((f64::from(p0.x), f64::from(p0.y)));
            path.line_to((f64::from(p1.x), f64::from(p1.y)));
            cx.list.push_stroke_path(path, (3.0 * self.zoom * s).max(1.0), *color);
        }
        cx.list.pop_clip();
    }
}

/// The windowed root: menu bar on top, tool column on the left, status bar at
/// A background channel drained each frame on the UI thread — the async preset-store load,
/// display-profile detection, adapter-selection notes. Captured `Receiver`s must be wrapped
/// in a `Mutex`: widgets are `Send + Sync`.
pub type AppDrain = Box<dyn FnMut(&mut PhotocraftApp) + Send + Sync>;

/// the bottom and the document canvas filling the middle.
pub struct PhotoCraftShell {
    pub app: PhotocraftApp,
    executor: EngineExecutor,
    /// Queued requests from the `--control` server, drained in `tick`
    /// (`Mutex`: `Receiver` is `Send` but widgets must be `Sync` too).
    control_rx: std::sync::Mutex<Option<std::sync::mpsc::Receiver<crate::control::ControlRequest>>>,
    /// Background channels drained each frame — the async preset-store load,
    /// display-profile detection, adapter-selection notes.
    drains: Vec<AppDrain>,
    menubar: MenuBar,
    palette: ToolPalette,
    statusbar: StatusBar,
    canvas: CanvasArea,
    /// `menu_commands[menu][item]` — the id(s) behind each declared row.
    menu_commands: Vec<Vec<MenuCmd>>,
    /// Rebuild the menu bar when the open-document set changed enablement.
    last_doc_count: usize,
    bounds: Rect,
    scale: f32,
    menubar_r: Rect,
    palette_r: Rect,
    statusbar_r: Rect,
    canvas_r: Rect,
    ctrl: bool,
    shift: bool,
    alt: bool,
    meta: bool,
    /// Status line the shell wants shown on the next tick.
    pending_status: Option<String>,
}

impl PhotoCraftShell {
    pub fn new(app: PhotocraftApp) -> Self {
        let engine = app.engine.clone();
        let guard = engine.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let (menubar, menu_commands) = build_menubar(&guard);
        let doc = guard.active().map(|d| (d.doc.size.width, d.doc.size.height));
        let fg = guard.tools.foreground;
        drop(guard);
        let mut canvas = CanvasArea::new();
        canvas.doc = doc;
        canvas.fg = to_u8(fg);
        Self {
            app,
            executor: EngineExecutor::default(),
            control_rx: std::sync::Mutex::new(None),
            drains: Vec::new(),
            menubar,
            palette: build_palette(tool_index(Tool::Move)),
            statusbar: StatusBar::new().message("Ready").label("status"),
            canvas,
            menu_commands,
            last_doc_count: usize::MAX,
            bounds: Rect::new(0.0, 0.0, 0.0, 0.0),
            scale: 1.0,
            menubar_r: Rect::new(0.0, 0.0, 0.0, 0.0),
            palette_r: Rect::new(0.0, 0.0, 0.0, 0.0),
            statusbar_r: Rect::new(0.0, 0.0, 0.0, 0.0),
            canvas_r: Rect::new(0.0, 0.0, 0.0, 0.0),
            ctrl: false,
            shift: false,
            alt: false,
            meta: false,
            pending_status: None,
        }
    }

    /// Installs the control-channel queue + the workspace-sandboxed file read it
    /// uses for `file.open` (the runner calls this before the shell is boxed).
    pub fn set_control(
        &mut self,
        rx: std::sync::mpsc::Receiver<crate::control::ControlRequest>,
        reader: Option<crate::commands::FileReader>,
        authorize: Option<crate::commands::CommandAuthorize>,
    ) {
        *self.control_rx.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = Some(rx);
        self.executor.reader = reader;
        self.executor.authorize = authorize;
    }

    /// Installs per-frame background drains (async preset store, monitor
    /// profiles, GPU-setup notes).
    pub fn set_drains(&mut self, drains: Vec<AppDrain>) {
        self.drains = drains;
    }

    /// Runs one engine command; the outcome lands on the status bar.
    fn run_command(&mut self, id: &'static str) {
        let engine = self.app.engine.clone();
        match self.executor.execute(&mut self.app, &engine, id, &serde_json::json!({})) {
            Ok(_) => self.pending_status = Some(id.to_string()),
            Err(e) => self.pending_status = Some(format!("{id}: {e}")),
        }
    }

    /// Drains the menu/palette/status seams into commands and tool state.
    fn drain_seams(&mut self) -> bool {
        let mut changed = false;
        while let Some(path) = self.menubar.take_activated() {
            let id = path
                .first()
                .and_then(|&m| self.menu_commands.get(m))
                .and_then(|slots| path.get(1).and_then(|&i| slots.get(i)))
                .and_then(|slot| slot.get(path.get(2..).unwrap_or(&[])));
            if let Some(id) = id {
                self.run_command(id);
                changed = true;
            }
        }
        while let Some(i) = self.palette.take_selected() {
            if let Some(&tool) = TOOLS.get(i) {
                self.app.set_tool(tool);
                self.canvas.tool = tool;
                self.pending_status = Some(format!("{} tool", TOOL_NAMES[i]));
                changed = true;
            }
        }
        while self.statusbar.take_activated().is_some() {}
        changed
    }

    /// Command shortcut (`Ctrl`/`Cmd` + key) → engine command id. Matches the
    /// engine's declared shortcuts (`Cmd+N` style) instead of a parallel table,
    /// so the two never drift.
    fn shortcut_command(&self, key: &str) -> Option<&'static str> {
        if !(self.ctrl || self.meta) {
            return None;
        }
        let shift = self.shift;
        let alt = self.alt;
        photocraft_engine::command_specs().iter().filter(|s| !s.menu.is_empty()).find_map(|s| shortcut_matches(s.shortcut?, key, shift, alt).then_some(s.id))
    }

    fn set_tool(&mut self, tool: Tool) {
        self.app.set_tool(tool);
        self.canvas.tool = tool;
        self.palette = build_palette(tool_index(tool));
        self.palette.layout(&mut LayoutContext { hot: &mut HotNode::default(), scale: self.scale }, self.palette_r);
        self.pending_status = Some(format!("{} tool", TOOL_NAMES[tool_index(tool)]));
    }

    fn on_key(&mut self, key: &str, down: bool) -> EventResponse {
        match key {
            "Control" | "ControlLeft" | "ControlRight" => {
                self.ctrl = down;
                return EventResponse::Handled;
            }
            "Shift" | "ShiftLeft" | "ShiftRight" => {
                self.shift = down;
                return EventResponse::Handled;
            }
            "Alt" | "AltLeft" | "AltRight" | "AltGraph" => {
                self.alt = down;
                return EventResponse::Handled;
            }
            "Meta" | "Super" | "SuperLeft" | "SuperRight" => {
                self.meta = down;
                return EventResponse::Handled;
            }
            _ => {}
        }
        if down {
            if let Some(id) = self.shortcut_command(key) {
                self.run_command(id);
                return EventResponse::RequestRepaint;
            }
            if let Some(tool) = self.app.keyboard.on_key_down(key, self.app.active_tool) {
                self.set_tool(tool);
                return EventResponse::RequestRepaint;
            }
        } else if let Some(tool) = self.app.keyboard.on_key_up(key) {
            self.set_tool(tool);
            return EventResponse::RequestRepaint;
        }
        EventResponse::Ignored
    }

    /// Drop-to-open: the first file (or `file://` URI) in the payload
    /// imports as a document through the same executor path the control
    /// channel uses.
    fn on_drop(&mut self, payload: &DropPayload) -> EventResponse {
        let path = match payload {
            DropPayload::Files(paths) => paths.first().cloned(),
            DropPayload::Uris(uris) => uris.first().and_then(|u| u.strip_prefix("file://").map(std::path::PathBuf::from)),
            _ => None,
        };
        let Some(path) = path else { return EventResponse::Ignored };
        let engine = self.app.engine.clone();
        let mut guard = engine.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        match self.executor.open_document(&mut guard, &path) {
            Ok(title) => self.pending_status = Some(format!("Opened {title}")),
            Err(e) => self.pending_status = Some(format!("Open failed: {e}")),
        }
        EventResponse::RequestRepaint
    }
}

impl Widget for PhotoCraftShell {
    fn measure(&mut self, _cx: &mut LayoutContext, _c: LayoutConstraints) -> Vec2 {
        Vec2::new(1280.0, 800.0)
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.bounds = bounds;
        self.scale = cx.scale;
        let s = cx.scale;
        let top = MENUBAR_H * s;
        let bottom = STATUS_H * s;
        let side = PALETTE_W * s;
        self.menubar_r = Rect::new(bounds.min_x(), bounds.min_y(), bounds.width(), top);
        self.palette_r = Rect::new(bounds.min_x(), self.menubar_r.max_y(), side, bounds.height() - top - bottom);
        self.statusbar_r = Rect::new(bounds.min_x(), bounds.max_y() - bottom, bounds.width(), bottom);
        self.canvas_r = Rect::new(self.palette_r.max_x(), self.menubar_r.max_y(), bounds.width() - side, self.palette_r.height());
        // Layout the internal children into their strips.
        for i in 0..4 {
            let (Some(r), Some(child)) = (self.child_bounds(i), self.child_mut(i)) else {
                continue;
            };
            let mut hot = HotNode { bounds: r, ..Default::default() };
            child.layout(&mut LayoutContext { hot: &mut hot, scale: s }, r);
        }
    }

    fn tick(&mut self, _dt: std::time::Duration) -> bool {
        let mut repaint = self.drain_seams();
        // Sync engine → view state that changed outside input handling.
        let engine = self.app.engine.clone();
        let guard = engine.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let docs = guard.documents().len();
        let doc = guard.active().map(|d| (d.doc.size.width, d.doc.size.height));
        let fg = guard.tools.foreground;
        drop(guard);
        if docs != self.last_doc_count {
            self.last_doc_count = docs;
            // Document set changed: menu enablement is stale — refresh the
            // items in place. Replacing the bar drops its popup stack and
            // shared state, orphaning any open menu.
            let engine = self.app.engine.clone();
            let guard = engine.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            let (menus, commands) = menu_model(&guard);
            drop(guard);
            if menus.len() == self.menubar.menu_count() {
                for (index, (_, items)) in menus.into_iter().enumerate() {
                    self.menubar.set_items(index, items);
                }
            } else {
                // Menu structure changed (a new top-level menu appeared):
                // a rebuild is unavoidable — live popups go defunct and
                // are swept by the overlay layer.
                let mut bar = MenuBar::new();
                for (label, items) in menus {
                    bar = bar.menu(label, items);
                }
                self.menubar = bar;
                let mut hot = HotNode { bounds: self.menubar_r, ..Default::default() };
                self.menubar.layout(&mut LayoutContext { hot: &mut hot, scale: self.scale }, self.menubar_r);
            }
            self.menu_commands = commands;
            repaint = true;
        }
        if self.canvas.doc != doc {
            self.canvas.doc = doc;
            // A document opening (or changing size) shows Fit on Screen; when
            // the canvas was never laid out the flag waits for `layout`.
            if let Some((w, h)) = doc {
                if self.canvas.bounds.width() > 0.0 {
                    self.canvas.fit_doc(w, h);
                } else {
                    self.canvas.pending_fit = true;
                }
            }
            repaint = true;
        }
        self.canvas.fg = to_u8(fg);
        self.app.zoom_level = self.canvas.zoom;
        self.app.pan_offset = self.canvas.pan;
        // Control-channel requests and background drains (preset store, monitor
        // profiles, adapter-selection notes) run on the UI thread here.
        {
            let rx = self.control_rx.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            if let Some(rx) = rx.as_ref() {
                let engine = self.app.engine.clone();
                let executor = EngineExecutor { reader: self.executor.reader.clone(), authorize: self.executor.authorize.clone() };
                crate::control::drain_requests(rx, &mut self.app, &engine, &executor);
            }
        }
        for drain in &mut self.drains {
            drain(&mut self.app);
        }
        // Notices pushed by the runner (non-Unicode paths, portable-mode
        // warnings, open failures) land on the status bar in order.
        if let Some(msg) = self.app.pending_notices.first().cloned() {
            self.app.pending_notices.remove(0);
            self.statusbar.set_message(msg);
            repaint = true;
        }
        if let Some(msg) = self.pending_status.take() {
            self.statusbar.set_message(msg);
            repaint = true;
        }
        repaint
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        let forwarded = self.forward_event_to_children(cx);
        let seams = self.drain_seams();
        if seams {
            return EventResponse::RequestRepaint;
        }
        if !matches!(forwarded, EventResponse::Ignored) {
            return forwarded;
        }
        match cx.event {
            WidgetEvent::KeyPressed { key, repeat } if !*repeat => self.on_key(key, true),
            WidgetEvent::KeyReleased { key } => self.on_key(key, false),
            WidgetEvent::Dropped { payload, .. } => self.on_drop(payload),
            _ => EventResponse::Ignored,
        }
    }

    fn paint(&self, cx: &mut PaintContext) {
        // The gutter behind the tool column — children paint their own chrome.
        if self.palette_r.width() > 0.0 {
            let surface = cx.color(TokenKey::SurfaceColor, [30, 31, 36, 255]);
            cx.list.push_fill_rect(kr(self.palette_r), surface);
        }
    }

    fn child_count(&self) -> usize {
        4
    }

    fn child(&self, index: usize) -> Option<&dyn Widget> {
        match index {
            0 => Some(&self.menubar),
            1 => Some(&self.palette),
            2 => Some(&self.canvas),
            3 => Some(&self.statusbar),
            _ => None,
        }
    }

    fn child_mut(&mut self, index: usize) -> Option<&mut dyn Widget> {
        match index {
            0 => Some(&mut self.menubar),
            1 => Some(&mut self.palette),
            2 => Some(&mut self.canvas),
            3 => Some(&mut self.statusbar),
            _ => None,
        }
    }

    fn child_bounds(&self, index: usize) -> Option<Rect> {
        match index {
            0 => Some(self.menubar_r),
            1 => Some(self.palette_r),
            2 => Some(self.canvas_r),
            3 => Some(self.statusbar_r),
            _ => None,
        }
    }
}

fn to_u8(c: [f32; 4]) -> [u8; 4] {
    [(c[0].clamp(0.0, 1.0) * 255.0) as u8, (c[1].clamp(0.0, 1.0) * 255.0) as u8, (c[2].clamp(0.0, 1.0) * 255.0) as u8, (c[3].clamp(0.0, 1.0) * 255.0) as u8]
}

/// The tool palette preserving selection (no public setter on the widget).
fn build_palette(selected: usize) -> ToolPalette {
    let mut palette = ToolPalette::new().columns(1).show_labels(false).label("Tools");
    for (i, name) in TOOL_NAMES.iter().enumerate() {
        palette = palette.tool(ToolItem::new(TOOL_GLYPHS[i], *name));
    }
    palette.selected(selected)
}

/// Does `spec` (`"Shift+Cmd+Z"`-style) describe `key` under these modifiers?
/// The caller already requires Ctrl-or-Meta held (the `Cmd` leg).
fn shortcut_matches(spec: &str, key: &str, shift: bool, alt: bool) -> bool {
    let parts: Vec<&str> = spec.split('+').collect();
    let Some(&tail) = parts.last() else {
        return false;
    };
    let mods = &parts[..parts.len().saturating_sub(1)];
    let mut need_shift = false;
    let mut need_alt = false;
    let mut need_cmd = false;
    for m in mods {
        match *m {
            "Shift" => need_shift = true,
            "Alt" => need_alt = true,
            "Cmd" | "Ctrl" => need_cmd = true,
            _ => return false,
        }
    }
    if !need_cmd || need_shift != shift || need_alt != alt {
        return false;
    }
    tail.eq_ignore_ascii_case(key)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shell() -> PhotoCraftShell {
        PhotoCraftShell::new(PhotocraftApp::new(Engine::new()))
    }

    #[test]
    fn shell_mounts_four_children() {
        let s = shell();
        assert_eq!(s.child_count(), 4);
        for i in 0..4 {
            assert!(s.child(i).is_some());
            assert!(s.child_bounds(i).is_some());
        }
    }

    #[test]
    fn fit_doc_scales_large_documents_down_and_keeps_small_at_100() {
        let mut canvas = CanvasArea::new();
        canvas.bounds = Rect::new(0.0, 0.0, 1000.0, 700.0);
        canvas.scale = 1.0;
        canvas.fit_doc(1920, 1080);
        assert!(canvas.zoom < 1.0, "a 1920px doc must shrink into a 1000pt canvas");
        assert!(1920.0 * canvas.zoom <= 1000.0, "fitted doc width stays inside the canvas");
        canvas.fit_doc(300, 200);
        assert_eq!(canvas.zoom, 1.0, "small docs open at 100%");
        assert_eq!(canvas.pan, [0.0, 0.0]);
    }

    #[test]
    fn layout_splits_chrome_and_canvas() {
        let mut s = shell();
        let mut hot = HotNode::default();
        let r = Rect::new(0.0, 0.0, 1280.0, 800.0);
        hot.bounds = r;
        s.layout(&mut LayoutContext { hot: &mut hot, scale: 1.0 }, r);
        assert!(s.menubar_r.max_y() <= s.palette_r.min_y());
        assert!(s.palette_r.max_x() <= s.canvas_r.min_x());
        assert!(s.canvas_r.max_y() <= s.statusbar_r.min_y());
    }

    #[test]
    fn tool_key_switches_the_tool() {
        let mut s = shell();
        let ev = WidgetEvent::KeyPressed { key: "b".to_string(), repeat: false };
        let mut cx = EventContext { event: &ev, bounds: Rect::new(0.0, 0.0, 100.0, 100.0), scale: 1.0 };
        let r = s.event(&mut cx);
        assert!(matches!(r, EventResponse::RequestRepaint | EventResponse::Handled));
        assert_eq!(s.app.active_tool, Tool::Brush);
    }

    #[test]
    fn shortcut_matcher_honours_modifiers() {
        assert!(shortcut_matches("Cmd+N", "n", false, false));
        assert!(shortcut_matches("Shift+Cmd+Z", "z", true, false));
        assert!(!shortcut_matches("Shift+Cmd+Z", "z", false, false));
        assert!(shortcut_matches("Alt+Cmd+I", "i", false, true));
        assert!(!shortcut_matches("N", "n", false, false));
    }

    #[test]
    fn menubar_built_from_engine_commands() {
        let engine = Engine::new();
        let (bar, commands) = build_menubar(&engine);
        assert!(bar.menu_count() >= 2, "File + Edit at least");
        assert!(!commands.is_empty());
        let file = &commands[0];
        assert!(file.iter().any(|c| matches!(c, MenuCmd::Cmd("file.new"))));
    }

    /// Every row in every menu must resolve to a command id the engine
    /// actually registers — a MenuBar path is `[menu, item, sub…]`.
    #[test]
    fn every_menu_row_resolves_to_a_registered_command() {
        let engine = Engine::new();
        let specs: std::collections::HashSet<&str> = photocraft_engine::command_specs().iter().map(|s| s.id).collect();
        let (_bar, commands) = build_menubar(&engine);
        let mut leaf_rows = 0usize;
        for (m, slots) in commands.iter().enumerate() {
            for (i, slot) in slots.iter().enumerate() {
                match slot {
                    MenuCmd::Cmd(id) => {
                        leaf_rows += 1;
                        assert_eq!(slot.get(&[]), Some(*id), "menu {m} item {i} resolves");
                        assert!(specs.contains(id), "menu {m} item {i}: {id} registered");
                    }
                    MenuCmd::Sub(ids) => {
                        assert!(!ids.is_empty(), "menu {m} item {i}: empty submenu");
                        for (j, id) in ids.iter().enumerate() {
                            leaf_rows += 1;
                            assert_eq!(slot.get(&[j]), Some(*id), "menu {m} item {i} sub {j} resolves");
                            assert!(specs.contains(id), "menu {m} item {i} sub {j}: {id} registered");
                        }
                    }
                }
            }
        }
        assert!(leaf_rows > 50, "PhotoCraft ships a full menu surface ({leaf_rows} rows)");
    }
}
