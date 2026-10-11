//! The right dock: the Photoshop panel column — six tabbed groups (Color,
//! Properties, Character, Navigator, History, Layers) against an always-on
//! icon rail, ported from the egui reference's `dock.rs`/`panels.rs`.
//!
//! Data flows through [`DockIo`]: the shell snapshots the engine into
//! [`DockModel`] each tick and drains [`DockCmd`]s the panels push back —
//! the same command seam the menu bar uses, so every row click is a real
//! engine command.

use glam::Vec2;
use martensite::core::{
    EventContext, EventResponse, HotNode, ImageData, LayoutConstraints, LayoutContext, PaintContext, PointerButton, Rect, TokenKey, Widget, WidgetEvent,
};
use martensite::widgets::menu::MenuItem;
use martensite::widgets::menu_button::MenuButton;
use martensite::widgets::tabs::Tabs;
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};

/// `PaintList` primitives take `kurbo` rects; node bounds are core `Rect`s.
fn kr(r: Rect) -> kurbo::Rect {
    kurbo::Rect::new(f64::from(r.min_x()), f64::from(r.min_y()), f64::from(r.max_x()), f64::from(r.max_y()))
}

/// `Rect::new` is `(x, y, w, h)`; panel geometry reads more clearly in
/// corners.
fn rr(x0: f32, y0: f32, x1: f32, y1: f32) -> Rect {
    Rect::new(x0, y0, (x1 - x0).max(0.0), (y1 - y0).max(0.0))
}

fn contains(r: &Rect, p: Vec2) -> bool {
    p.x >= r.min_x() && p.x < r.max_x() && p.y >= r.min_y() && p.y < r.max_y()
}

/// Width of the icon rail at the window's right edge.
const RAIL_W: f32 = 34.0;
/// Gap between docked groups — also the height splitter's grab area.
const GAP: f32 = 6.0;
/// Tab strip height (martensite's `Tabs` strip).
const STRIP_H: f32 = 32.0;
/// Dock width clamps: (min, default, max).
const DOCK_W: (f32, f32, f32) = (180.0, 300.0, 600.0);
/// Row height for list-style panels.
const ROW_H: f32 = 24.0;

// ---------------------------------------------------------------- model

/// One flattened layer row (display order: top of stack first).
#[derive(Clone, Debug)]
pub struct LayerRowModel {
    pub id: u64,
    pub name: String,
    pub depth: usize,
    pub visible: bool,
    pub is_group: bool,
    pub expanded: bool,
    pub selected: bool,
    pub primary: bool,
    pub locked: bool,
    pub clipped: bool,
    /// `pixel` / `adjustment` / `type` / `shape` / `smart` / `group`.
    pub kind: &'static str,
}

/// One channel row (composite and colour channels first, then the layer's
/// mask, alphas and the quick mask — the reference's row order).
#[derive(Clone, Debug)]
pub struct ChannelRowModel {
    /// The `channel` reference `channel.setVisible` and
    /// `select.loadSelection` take: `"composite"`, `{"color":k}`, `i`,
    /// `"quickMask"` or `"mask"`.
    pub channel_ref: serde_json::Value,
    /// The reference `channel.target` takes — `"composite"` for the
    /// temporary rows, which can't be targeted directly.
    pub target_ref: serde_json::Value,
    /// The mask row's layer id (`view.layerMask` / `select.loadSelection`
    /// `"layer"` params).
    pub layer: Option<u64>,
    /// The layer's mask row: its eye runs `view.layerMask` and a plain
    /// click targets the mask (egui's `ui.mask_target`), not a channel.
    pub is_mask: bool,
    /// Composite and colour rows clear `mask_target` when clicked.
    pub clears_mask: bool,
    pub name: String,
    pub visible: bool,
    pub targeted: bool,
    /// Italic in Photoshop: quick mask, layer mask, spot channels.
    pub temporary: bool,
}

/// One named path row.
#[derive(Clone, Debug)]
pub struct PathRowModel {
    pub key: String,
    pub name: String,
    pub selected: bool,
    /// Temporary rows (work path, layer paths) render italic in Photoshop.
    pub temporary: bool,
}

/// One layer comp row.
#[derive(Clone, Debug)]
pub struct CompRowModel {
    /// The comp's stable id (PSD `compID`) — `layerComp.*` params.
    pub id: u32,
    pub name: String,
    pub applied: bool,
}

/// The active layer's header state (blend/opacity/fill/locks).
#[derive(Clone, Debug)]
pub struct LayerHeader {
    pub id: u64,
    pub blend: String,
    pub opacity: f32,
    pub fill: f32,
    pub locked: bool,
    pub is_group: bool,
    pub is_background: bool,
}

/// Everything the dock paints, snapshotted from the engine each tick.
#[derive(Clone, Default)]
pub struct DockModel {
    pub has_doc: bool,
    pub doc_name: String,
    pub doc_w: u32,
    pub doc_h: u32,
    pub doc_mode: String,
    /// Active layer's header controls (`None` = header greys out).
    pub layer: Option<LayerHeader>,
    /// Blend-mode choices for the header dropdown.
    pub blend_modes: Vec<String>,
    pub layers: Vec<LayerRowModel>,
    pub channels: Vec<ChannelRowModel>,
    pub paths: Vec<PathRowModel>,
    /// Undo labels, oldest first; `history_current` is the selected row.
    pub history: Vec<String>,
    pub history_current: usize,
    /// Redo labels (dimmed rows under the current state).
    pub redo: Vec<String>,
    pub actions: Vec<String>,
    pub comps: Vec<CompRowModel>,
    pub gradient_presets: Vec<String>,
    /// `(pattern id, display name)` — the commands take the id.
    pub pattern_presets: Vec<(String, String)>,
    pub fg: [u8; 4],
    pub bg: [u8; 4],
    /// The session's swatch library, flattened across groups (display RGB).
    pub swatches: Vec<[u8; 3]>,
    /// 256-bin luminance histogram; empty when no document.
    pub histogram: Vec<u32>,
    /// Pointer position in doc px + the sampled RGBA (Info panel).
    pub pointer: Option<(f32, f32, [u8; 4])>,
    /// Normalised viewport rect on the document (Navigator), 0..1.
    pub viewport: Option<(f32, f32, f32, f32)>,
    /// Composited document preview for the Navigator — the shell shares
    /// the canvas's `ImageData` (Arc-backed, cheap to clone).
    pub nav_preview: Option<ImageData>,
    /// Character panel state from the active type layer (`None` when the
    /// active layer isn't text).
    pub char: Option<CharModel>,
    /// Paragraph panel state.
    pub para: Option<ParaModel>,
}

/// The Character panel's read of the active text layer.
#[derive(Clone, Debug)]
pub struct CharModel {
    pub family: String,
    pub style: String,
    pub size_pt: f32,
    /// `None` = Auto leading.
    pub leading_pt: Option<f32>,
    /// 1/1000 em.
    pub tracking: f32,
    pub color: [u8; 4],
}

/// The Paragraph panel's read of the active text layer.
#[derive(Clone, Debug)]
pub struct ParaModel {
    pub align: String,
    pub first_line_pt: f32,
    pub start_indent_pt: f32,
    pub end_indent_pt: f32,
    pub space_before_pt: f32,
    pub space_after_pt: f32,
}

/// What a panel asks the shell to do. `Engine` runs a command through the
/// executor; the rest are view-state the shell owns.
#[derive(Clone, Debug)]
pub enum DockCmd {
    Engine(String, Value),
    /// Pan the canvas so the doc-space point (x, y) centres the viewport.
    PanTo(f32, f32),
    /// Select a Paths row (view state, like egui's `ui.selected_path`).
    SelectPath(String),
}

/// Shared shell ⇄ dock channel.
#[derive(Default)]
pub struct DockIo {
    pub model: DockModel,
    pub actions: Vec<DockCmd>,
    /// The selected Paths row key (view state).
    pub selected_path: Option<String>,
    /// Pointer modifiers — pointer events carry no modifier state, so the
    /// shell mirrors what its key tracking sees (⇧/⌃ for layer selection).
    pub mods: (bool, bool),
    /// The Channels panel's "target the layer's mask" state (egui's
    /// `ui.mask_target`): the mask row is highlighted and tools edit it.
    pub mask_target: bool,
}

type Shared = Arc<Mutex<DockIo>>;

fn io_lock(io: &Shared) -> std::sync::MutexGuard<'_, DockIo> {
    io.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

// ---------------------------------------------------------------- groups

/// The six dock groups, in Photoshop's top-to-bottom order.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DockGroup {
    Color,
    Properties,
    Character,
    Navigator,
    History,
    Layers,
}

impl DockGroup {
    pub const ALL: [Self; 6] = [Self::Color, Self::Properties, Self::Character, Self::Navigator, Self::History, Self::Layers];

    fn icon(self) -> &'static str {
        match self {
            Self::Color => "edit.palette",
            Self::Properties => "nav.settings",
            Self::Character => "misc.type",
            Self::Navigator => "map.navigation",
            Self::History => "time.clock",
            Self::Layers => "edit.layers",
        }
    }

    fn tabs(self) -> &'static [&'static str] {
        match self {
            Self::Color => &["Swatches", "Color", "Gradients", "Patterns"],
            Self::Properties => &["Properties", "Adjustments"],
            Self::Character => &["Character", "Paragraph"],
            Self::Navigator => &["Navigator", "Histogram", "Info"],
            Self::History => &["History", "Actions", "Layer Comps"],
            Self::Layers => &["Layers", "Channels", "Paths"],
        }
    }

    /// Default docked height (egui `Group::default_height`).
    fn default_height(self) -> f32 {
        match self {
            Self::Color => 190.0,
            Self::Properties => 250.0,
            Self::Character => 270.0,
            Self::Navigator => 210.0,
            Self::History => 200.0,
            Self::Layers => 320.0,
        }
    }

    /// Minimum docked height including the strip.
    fn min_height(self) -> f32 {
        match self {
            Self::Color => 130.0,
            Self::Properties => 160.0,
            Self::Character => 160.0,
            Self::Navigator => 140.0,
            Self::History => 130.0,
            Self::Layers => 200.0,
        }
    }

    /// The filler group's preferred height (egui `preferred_fill`).
    fn preferred_fill(self) -> f32 {
        match self {
            Self::Layers => 500.0,
            g => g.min_height(),
        }
    }
}

fn build_tabs(group: DockGroup, io: &Shared) -> Tabs {
    let mut tabs = Tabs::new();
    for (i, name) in group.tabs().iter().enumerate() {
        tabs = tabs.tab(*name, PanelBox(panel_body(group, i, io)));
    }
    tabs
}

/// `Tabs::tab` takes a concrete `impl Widget`; the panels are a heterogenous
/// set behind `Box<dyn Widget>`, so a thin forwarder adapts them.
struct PanelBox(Box<dyn Widget>);

impl Widget for PanelBox {
    fn measure(&mut self, cx: &mut LayoutContext, c: LayoutConstraints) -> Vec2 {
        self.0.measure(cx, c)
    }
    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.0.layout(cx, bounds);
    }
    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        self.0.event(cx)
    }
    fn paint(&self, cx: &mut PaintContext) {
        // Panels are culled by bounds, not clipped — a collapsed frame
        // leaves the body with zero height and it would paint into the
        // group below (the paint walk treats empty bounds as paintable).
        // Gate on real area, then clip whatever is left to the panel rect.
        if cx.bounds.width() <= 0.0 || cx.bounds.height() <= 0.0 {
            return;
        }
        cx.list.push_clip(kr(cx.bounds));
        self.0.paint(cx);
        cx.list.pop_clip();
    }
    fn child_count(&self) -> usize {
        self.0.child_count()
    }
    fn child(&self, index: usize) -> Option<&dyn Widget> {
        self.0.child(index)
    }
    fn child_mut(&mut self, index: usize) -> Option<&mut dyn Widget> {
        self.0.child_mut(index)
    }
    fn child_bounds(&self, index: usize) -> Option<Rect> {
        self.0.child_bounds(index)
    }
}

fn panel_body(group: DockGroup, tab: usize, io: &Shared) -> Box<dyn Widget> {
    match (group, tab) {
        (DockGroup::Color, 0) => Box::new(SwatchesPanel::new(io)),
        (DockGroup::Color, 1) => Box::new(ColorPanel::new(io)),
        (DockGroup::Color, 2) => Box::new(PresetPanel::new(io, PresetKind::Gradients)),
        (DockGroup::Color, _) => Box::new(PresetPanel::new(io, PresetKind::Patterns)),
        (DockGroup::Properties, 0) => Box::new(PropertiesPanel::new(io)),
        (DockGroup::Properties, _) => Box::new(AdjustmentsPanel::new(io)),
        (DockGroup::Character, 0) => Box::new(TypePanel::new(io, false)),
        (DockGroup::Character, _) => Box::new(TypePanel::new(io, true)),
        (DockGroup::Navigator, 0) => Box::new(NavigatorPanel::new(io)),
        (DockGroup::Navigator, 1) => Box::new(HistogramPanel::new(io)),
        (DockGroup::Navigator, _) => Box::new(InfoPanel::new(io)),
        (DockGroup::History, 0) => Box::new(HistoryPanel::new(io)),
        (DockGroup::History, 1) => Box::new(ActionsPanel::new(io)),
        (DockGroup::History, _) => Box::new(CompsPanel::new(io)),
        (DockGroup::Layers, 0) => Box::new(LayersPanel::new(io)),
        (DockGroup::Layers, 1) => Box::new(ChannelsPanel::new(io)),
        (DockGroup::Layers, _) => Box::new(PathsPanel::new(io)),
    }
}

// ---------------------------------------------------------------- paint helpers

/// Paints the ambient icon `name` into `r` — SVG `d` path data stroked on the
/// icon grid, mirroring `morph_icon`'s `paint_icon_named` (which is
/// crate-private to martensite). Returns `false` for unknown names.
fn paint_icon(cx: &mut PaintContext, r: Rect, name: &str, ink: [u8; 4]) -> bool {
    if r.width() <= 0.0 || r.height() <= 0.0 {
        return false;
    }
    let Some(d) = martensite::icons::resolve_icon(name) else {
        return false;
    };
    let Ok(path) = kurbo::BezPath::from_svg(&d) else {
        return false;
    };
    // Icons are authored on a 24-unit grid, stroked at ~1.5 units.
    let s = f64::from(r.width().min(r.height())) / 24.0;
    let xform = kurbo::Affine::translate((f64::from(r.min_x()), f64::from(r.min_y()))) * kurbo::Affine::scale(s);
    cx.list.push_stroke_path(xform * path, (1.5 * s) as f32, ink);
    true
}

/// Baseline-origin text in the paint list's coordinate space.
fn text(cx: &mut PaintContext, x: f32, baseline_y: f32, s: impl Into<String>, size: f32, color: [u8; 4]) {
    // The paint audit floors legible text at 12pt — small panel labels
    // still read at that size, so clamp instead of policing every call.
    cx.list.push_text(kurbo::Point::new(f64::from(x), f64::from(baseline_y)), s.into(), size.max(12.0), color);
}

/// Empty-state label, egui's `empty()` helper.
fn paint_empty(cx: &mut PaintContext, bounds: Rect, s: &str) {
    let dim = cx.color(TokenKey::TextColor, [110, 116, 128, 255]);
    text(cx, bounds.min_x() + 8.0, bounds.min_y() + 22.0, s, 11.0, dim);
}

fn row_rect(y0: f32, w_min: f32, w_max: f32, i: usize, scroll: f32, h: f32) -> Rect {
    let y = y0 + i as f32 * h - scroll;
    rr(w_min, y, w_max, y + h)
}

/// `count` square footer buttons, right-aligned in `row` (index 0 = rightmost).
fn footer_rects(row: Rect, count: usize, size: f32) -> Vec<Rect> {
    (0..count)
        .map(|i| {
            let x = row.max_x() - size * (i as f32 + 1.0) - 4.0;
            rr(x, row.min_y() + (row.height() - size) / 2.0, x + size, row.min_y() + (row.height() + size) / 2.0)
        })
        .collect()
}

// ---------------------------------------------------------------- the dock

/// The right-dock widget: a resizable panel column plus the always-visible
/// icon rail that shows, expands and collapses each group.
pub struct RightDock {
    io: Shared,
    /// One `Tabs` frame per group, in `DockGroup::ALL` order.
    frames: Vec<(DockGroup, Tabs)>,
    /// Groups shown in the column (rail toggles).
    shown: Vec<DockGroup>,
    /// Groups collapsed to their tab strip.
    collapsed: Vec<DockGroup>,
    width: f32,
    bounds: Rect,
    rail_r: Rect,
    col_r: Rect,
    /// Per-shown-group rects from the last layout.
    frame_r: Vec<(DockGroup, Rect)>,
    /// Per-group rail icon rects.
    rail_slots: Vec<(DockGroup, Rect)>,
    /// User-dragged group heights (shown-index → height).
    heights: std::collections::BTreeMap<usize, f32>,
    /// Active splitter drag: (frame index, start y, start height).
    split_drag: Option<(usize, f32, f32)>,
    /// Width-drag state (left column edge).
    resize: Option<(f32, f32)>,
}

impl RightDock {
    pub fn new() -> Self {
        let io: Shared = Arc::new(Mutex::new(DockIo::default()));
        let frames = DockGroup::ALL.iter().map(|&g| (g, build_tabs(g, &io))).collect();
        Self {
            io,
            frames,
            shown: vec![DockGroup::Navigator, DockGroup::Color, DockGroup::Layers],
            collapsed: Vec::new(),
            width: DOCK_W.1,
            bounds: rr(0.0, 0.0, 0.0, 0.0),
            rail_r: rr(0.0, 0.0, 0.0, 0.0),
            col_r: rr(0.0, 0.0, 0.0, 0.0),
            frame_r: Vec::new(),
            rail_slots: Vec::new(),
            heights: std::collections::BTreeMap::new(),
            split_drag: None,
            resize: None,
        }
    }

    /// The shared shell ⇄ dock channel.
    pub fn io(&self) -> Shared {
        self.io.clone()
    }

    /// Updates the snapshot the panels paint.
    pub fn set_model(&self, model: DockModel) {
        io_lock(&self.io).model = model;
    }

    /// Drains commands the panels queued since the last call.
    pub fn take_actions(&mut self) -> Vec<DockCmd> {
        std::mem::take(&mut io_lock(&self.io).actions)
    }

    /// Mirrors pointer modifier state into the shared channel.
    pub fn set_mods(&self, shift: bool, ctrl: bool) {
        io_lock(&self.io).mods = (shift, ctrl);
    }

    /// Total width the dock reserves at the window's right edge.
    pub fn reserved_width(&self) -> f32 {
        RAIL_W + if self.shown.is_empty() { 0.0 } else { self.width }
    }

    /// egui `DockLayout::heights_for`: the last expanded group fills the
    /// column; groups above it keep their stored/default height, shrinking to
    /// minimums when the column runs short.
    fn heights_for(&self, avail: f32) -> Vec<f32> {
        let n = self.shown.len();
        if n == 0 {
            return Vec::new();
        }
        let filler = self.shown.iter().rposition(|g| !self.collapsed.contains(g));
        let mut hs: Vec<f32> = self
            .shown
            .iter()
            .enumerate()
            .map(|(i, g)| {
                if self.collapsed.contains(g) {
                    STRIP_H
                } else if Some(i) == filler {
                    0.0
                } else {
                    self.heights.get(&i).copied().unwrap_or_else(|| g.default_height()).max(g.min_height())
                }
            })
            .collect();
        if let Some(f) = filler {
            let gaps = GAP * (n - 1) as f32;
            let used: f32 = hs.iter().sum::<f32>() + gaps;
            let mut deficit = (used + self.shown[f].preferred_fill() - avail).max(0.0);
            for i in (0..f).rev() {
                if deficit <= 0.0 {
                    break;
                }
                let give = (hs[i] - self.shown[i].min_height()).min(deficit).max(0.0);
                hs[i] -= give;
                deficit -= give;
            }
            hs[f] = (avail - hs.iter().sum::<f32>() - gaps).max(self.shown[f].min_height());
        }
        hs
    }

    /// A rail press shows a hidden group, expands a collapsed one, or
    /// collapses an open one.
    fn toggle_group(&mut self, g: DockGroup) {
        if !self.shown.contains(&g) {
            self.shown = DockGroup::ALL.iter().copied().filter(|x| self.shown.contains(x) || *x == g).collect();
            self.collapsed.retain(|x| *x != g);
        } else if self.collapsed.contains(&g) {
            self.collapsed.retain(|x| *x != g);
        } else {
            self.collapsed.push(g);
        }
    }
    /// Recomputes all geometry — rail, column, slots, per-group frames —
    /// and re-lays out the `Tabs` frames. Called from `layout` with the
    /// real context, and from `event` with a scratch node after any
    /// state change that moves geometry: the runner only re-lays out on
    /// window resizes, so rail toggles and drags must reflow eagerly
    /// (the same `split_view` live-geometry pattern).
    fn reflow(&mut self, cx: &mut LayoutContext) {
        let bounds = self.bounds;
        let s = cx.scale;
        let rail_w = RAIL_W * s;
        self.rail_r = rr(bounds.max_x() - rail_w, bounds.min_y(), bounds.max_x(), bounds.max_y());
        // The dock is right-anchored: its left edge is `bounds.max_x -
        // reserved_width * s` even when `bounds` predates a width drag.
        let col_x = if self.shown.is_empty() { self.rail_r.min_x() } else { bounds.max_x() - self.reserved_width() * s };
        self.col_r = rr(col_x, bounds.min_y(), self.rail_r.min_x(), bounds.max_y());
        self.rail_slots = DockGroup::ALL
            .iter()
            .enumerate()
            .map(|(i, g)| {
                let y = self.rail_r.min_y() + (i as f32 + 0.25) * RAIL_W * s;
                (*g, rr(self.rail_r.min_x() + 2.0, y, self.rail_r.max_x() - 2.0, y + RAIL_W * s - 4.0))
            })
            .collect();
        self.frame_r.clear();
        if !self.shown.is_empty() {
            // `heights_for` works in logical points — the constants it
            // balances against (`STRIP_H`, min/default/fill) are egui
            // logical sizes. Scale back to device on the way out.
            let heights = self.heights_for(self.col_r.height() / s);
            let mut y = self.col_r.min_y();
            for (i, h) in heights.into_iter().enumerate() {
                let h = (h * s).max(STRIP_H * s);
                let r = rr(self.col_r.min_x(), y, self.col_r.max_x(), (y + h).min(self.col_r.max_y()));
                self.frame_r.push((self.shown[i], r));
                y = r.max_y() + GAP * s;
            }
        }
        for (g, tabs) in &mut self.frames {
            let b = self.frame_r.iter().find(|(fg, _)| fg == g).map(|(_, r)| *r).unwrap_or_default();
            cx.layout_child(tabs, b);
        }
    }
}

impl Default for RightDock {
    fn default() -> Self {
        Self::new()
    }
}

impl Widget for RightDock {
    fn measure(&mut self, cx: &mut LayoutContext, _constraints: LayoutConstraints) -> Vec2 {
        Vec2::new(cx.pt(self.reserved_width()), 0.0)
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.bounds = bounds;
        self.reflow(cx);
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        match cx.event {
            WidgetEvent::PointerPressed { position, button: PointerButton::Primary, .. } => {
                if contains(&self.rail_r, *position) {
                    for (g, r) in &self.rail_slots {
                        if contains(r, *position) {
                            self.toggle_group(*g);
                            let mut hot = HotNode::default();
                            self.reflow(&mut LayoutContext { hot: &mut hot, scale: cx.scale });
                            return EventResponse::RequestRepaint;
                        }
                    }
                    return EventResponse::Handled;
                }
                for i in 0..self.frame_r.len().saturating_sub(1) {
                    let gap_top = self.frame_r[i].1.max_y();
                    let gap = rr(self.col_r.min_x(), gap_top, self.col_r.max_x(), gap_top + GAP * cx.scale);
                    if contains(&gap, *position) {
                        self.split_drag = Some((i, position.y, self.frame_r[i].1.height() / cx.scale));
                        return EventResponse::Handled;
                    }
                }
                if !self.shown.is_empty() && position.x >= self.col_r.min_x() && position.x < self.col_r.min_x() + 4.0 {
                    self.resize = Some((position.x, self.width));
                    return EventResponse::Handled;
                }
                match self.forward_event_to_children(cx) {
                    EventResponse::Ignored => EventResponse::Ignored,
                    r => r,
                }
            }
            WidgetEvent::PointerMoved { position, .. } => {
                if let Some((i, start_y, start_h)) = self.split_drag {
                    let new_h = (start_h + (position.y - start_y) / cx.scale).max(self.shown[i].min_height());
                    self.heights.insert(i, new_h);
                    let mut hot = HotNode::default();
                    self.reflow(&mut LayoutContext { hot: &mut hot, scale: cx.scale });
                    return EventResponse::RequestRepaint;
                }
                if let Some((start_x, start_w)) = self.resize {
                    let s = if self.width > 0.0 { self.col_r.width() / self.width } else { 1.0 };
                    self.width = (start_w + (start_x - position.x) / s.max(0.01)).clamp(DOCK_W.0, DOCK_W.2);
                    let mut hot = HotNode::default();
                    self.reflow(&mut LayoutContext { hot: &mut hot, scale: cx.scale });
                    return EventResponse::RequestRepaint;
                }
                match self.forward_event_to_children(cx) {
                    EventResponse::Ignored => EventResponse::Ignored,
                    r => r,
                }
            }
            WidgetEvent::PointerReleased { .. } => {
                if self.split_drag.take().is_some() || self.resize.take().is_some() {
                    return EventResponse::RequestRepaint;
                }
                self.forward_event_to_children(cx)
            }
            _ => self.forward_event_to_children(cx),
        }
    }

    fn paint(&self, cx: &mut PaintContext) {
        if !self.shown.is_empty() {
            let dock_bg = cx.color(TokenKey::SurfaceColor, [24, 25, 30, 255]);
            let border = cx.color(TokenKey::BorderColor, [60, 62, 70, 255]);
            cx.list.push_fill_rect(kr(self.col_r), dock_bg);
            cx.list.push_stroke_rect(kr(rr(self.col_r.min_x(), self.col_r.min_y(), self.col_r.min_x() + 1.0, self.col_r.max_y())), 0.5, border);
        }
        // The rail always paints — collapse still needs its icons.
        if self.rail_r.width() > 0.0 {
            let surface = cx.color(TokenKey::SurfaceColor, [30, 31, 36, 255]);
            cx.list.push_fill_rect(kr(self.rail_r), surface);
            for (g, r) in &self.rail_slots {
                let on = self.shown.contains(g) && !self.collapsed.contains(g);
                let ink = if on { cx.color(TokenKey::AccentColor, [96, 165, 250, 255]) } else { cx.color(TokenKey::TextColor, [172, 178, 190, 255]) };
                let icon_r = rr(
                    r.min_x() + (r.width() - 18.0) / 2.0,
                    r.min_y() + (r.height() - 18.0) / 2.0,
                    r.min_x() + (r.width() + 18.0) / 2.0,
                    r.min_y() + (r.height() + 18.0) / 2.0,
                );
                if !paint_icon(cx, icon_r, g.icon(), ink) {
                    text(cx, icon_r.min_x() + 3.0, icon_r.min_y() + 14.0, &g.tabs()[0][..1], 11.0, ink);
                }
            }
        }
    }

    fn child_count(&self) -> usize {
        self.frames.len()
    }

    fn child(&self, index: usize) -> Option<&dyn Widget> {
        self.frames.get(index).map(|(_, t)| t as &dyn Widget)
    }

    fn child_mut(&mut self, index: usize) -> Option<&mut dyn Widget> {
        self.frames.get_mut(index).map(|(_, t)| t as &mut dyn Widget)
    }

    fn child_bounds(&self, index: usize) -> Option<Rect> {
        let (g, _) = self.frames.get(index)?;
        self.frame_r.iter().find(|(fg, _)| fg == g).map(|(_, r)| *r)
    }
}

// ---------------------------------------------------------------- list base

/// Shared bits every list-style panel needs: bounds, scroll offset, hover.
struct ListState {
    bounds: Rect,
    scroll: f32,
    hover: Option<Vec2>,
    content_h: f32,
}

impl ListState {
    fn new() -> Self {
        Self { bounds: rr(0.0, 0.0, 0.0, 0.0), scroll: 0.0, hover: None, content_h: 0.0 }
    }

    fn clamp_scroll(&mut self, delta: f32) {
        let max = (self.content_h - self.bounds.height()).max(0.0);
        self.scroll = (self.scroll + delta).clamp(0.0, max);
    }

    /// Which `row_h` row is under `p` (content coords account for scroll).
    fn row_at(&self, row_h: f32, y0: f32, p: Vec2) -> Option<usize> {
        if !contains(&self.bounds, p) || p.y < y0 {
            return None;
        }
        Some(((p.y - y0 + self.scroll) / row_h) as usize)
    }
}

/// Declares a `model()`/`push()` pair over the shared channel plus the list
/// plumbing for panels with no extra state.
macro_rules! list_widget {
    ($name:ident) => {
        struct $name {
            io: Shared,
            st: ListState,
        }
        impl $name {
            fn new(io: &Shared) -> Self {
                Self { io: io.clone(), st: ListState::new() }
            }
        }
        impl $name {
            #[allow(dead_code)]
            fn model(&self) -> DockModel {
                io_lock(&self.io).model.clone()
            }
            #[allow(dead_code)]
            fn push(&self, cmd: DockCmd) {
                io_lock(&self.io).actions.push(cmd);
            }
        }
    };
    ($name:ident, { $($field:ident : $fty:ty = $finit:expr),* $(,)? }) => {
        struct $name {
            io: Shared,
            st: ListState,
            $($field : $fty,)*
        }
        impl $name {
            #[allow(dead_code)]
            fn model(&self) -> DockModel {
                io_lock(&self.io).model.clone()
            }
            #[allow(dead_code)]
            fn push(&self, cmd: DockCmd) {
                io_lock(&self.io).actions.push(cmd);
            }
        }
    };
}

/// Scroll-wheel + hover plumbing shared by the list panels. Returns `true`
/// when the event was consumed as chrome (scroll inside bounds).
fn list_event(st: &mut ListState, cx: &mut EventContext) -> bool {
    match cx.event {
        WidgetEvent::Scroll { position, delta } if contains(&st.bounds, *position) => {
            st.clamp_scroll(delta.y);
            true
        }
        WidgetEvent::PointerMoved { position, .. } => {
            st.hover = if contains(&st.bounds, *position) { Some(*position) } else { None };
            false
        }
        _ => false,
    }
}

/// Paints a hovered/selected/text row and its label.
fn paint_row(cx: &mut PaintContext, st: &ListState, r: Rect, selected: bool, label: &str, extra: Option<&str>) {
    let txt = cx.color(TokenKey::TextColor, [200, 205, 215, 255]);
    let sel = cx.color(TokenKey::SurfaceColor, [52, 60, 84, 255]);
    let hover_c = cx.color(TokenKey::SurfaceColor, [44, 46, 54, 255]);
    if selected {
        cx.list.push_fill_rect(kr(r), sel);
    } else if st.hover.is_some_and(|p| contains(&r, p)) {
        cx.list.push_fill_rect(kr(r), hover_c);
    }
    let x = r.min_x() + extra.map_or(8.0, |_| 26.0);
    if let Some(e) = extra {
        text(cx, r.min_x() + 8.0, r.min_y() + 16.0, e, 10.0, txt);
    }
    text(cx, x, r.min_y() + 16.0, label, 11.5, txt);
}

// ---------------------------------------------------------------- swatches

/// The Swatches panel's grid cells per row.
const SWATCH_COLS: usize = 10;

list_widget!(SwatchesPanel);

impl Widget for SwatchesPanel {
    fn measure(&mut self, _cx: &mut LayoutContext, _c: LayoutConstraints) -> Vec2 {
        Vec2::new(100.0, 150.0)
    }

    fn layout(&mut self, _cx: &mut LayoutContext, bounds: Rect) {
        self.st.bounds = bounds;
        let cell = ((bounds.width() - 4.0 * (SWATCH_COLS - 1) as f32) / SWATCH_COLS as f32).floor().max(8.0);
        self.st.content_h = (self.model().swatches.len() as f32 / SWATCH_COLS as f32).ceil() * (cell + 4.0) + 18.0;
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        if list_event(&mut self.st, cx) {
            return EventResponse::Handled;
        }
        if let WidgetEvent::PointerPressed { position, button, .. } = cx.event {
            let cell = ((self.st.bounds.width() - 4.0 * (SWATCH_COLS - 1) as f32) / SWATCH_COLS as f32).floor().max(8.0);
            for (i, _) in self.model().swatches.iter().enumerate() {
                let (gx, gy) = ((i % SWATCH_COLS) as f32, (i / SWATCH_COLS) as f32);
                let x = self.st.bounds.min_x() + gx * (cell + 4.0);
                let y = self.st.bounds.min_y() + gy * (cell + 4.0) - self.st.scroll;
                if contains(&rr(x, y, x + cell, y + cell), *position) {
                    let target = if *button == PointerButton::Secondary { "background" } else { "foreground" };
                    self.push(DockCmd::Engine("swatches.use".into(), json!({"index": i, "target": target})));
                    return EventResponse::Handled;
                }
            }
            return EventResponse::Handled;
        }
        EventResponse::Ignored
    }

    fn paint(&self, cx: &mut PaintContext) {
        cx.list.push_clip(kr(self.st.bounds));
        let cell = ((self.st.bounds.width() - 4.0 * (SWATCH_COLS - 1) as f32) / SWATCH_COLS as f32).floor().max(8.0);
        for (i, s) in self.model().swatches.iter().enumerate() {
            let (gx, gy) = ((i % SWATCH_COLS) as f32, (i / SWATCH_COLS) as f32);
            let x = self.st.bounds.min_x() + gx * (cell + 4.0);
            let y = self.st.bounds.min_y() + gy * (cell + 4.0) - self.st.scroll;
            let r = rr(x, y, x + cell, y + cell);
            cx.list.push_fill_rect(kr(r), [s[0], s[1], s[2], 255]);
            if self.st.hover.is_some_and(|p| contains(&r, p)) {
                cx.list.push_stroke_rect(kr(r), 1.5, [230, 234, 240, 255]);
            }
        }
        let dim = cx.color(TokenKey::TextColor, [160, 166, 178, 255]);
        let surface = cx.color(TokenKey::SurfaceColor, [24, 25, 30, 255]);
        // A backing strip keeps the hint readable over whichever swatch
        // lands underneath (the audit flagged text-on-yellow).
        cx.list.push_fill_rect(kr(rr(self.st.bounds.min_x(), self.st.bounds.max_y() - 18.0, self.st.bounds.max_x(), self.st.bounds.max_y())), surface);
        text(cx, self.st.bounds.min_x() + 2.0, self.st.bounds.max_y() - 4.0, "Click sets foreground · right-click sets background", 9.5, dim);
        cx.list.pop_clip();
    }
}

// ---------------------------------------------------------------- color

struct ColorPanel {
    io: Shared,
    st: ListState,
    /// Which HSV slider is being dragged (0=H, 1=S, 2=V).
    drag: Option<usize>,
    /// The last edited HSB while it still round-trips to the foreground —
    /// greys have no hue of their own, so recomputing from RGB would snap
    /// the Hue slider to 0 (the egui picker keeps the same memory).
    hsv: Option<(f32, f32, f32)>,
}

impl ColorPanel {
    fn new(io: &Shared) -> Self {
        Self { io: io.clone(), st: ListState::new(), drag: None, hsv: None }
    }
    fn model(&self) -> DockModel {
        io_lock(&self.io).model.clone()
    }
    fn push(&self, cmd: DockCmd) {
        io_lock(&self.io).actions.push(cmd);
    }
    /// HSV to show: the remembered edit while it still gives the current
    /// foreground, else a fresh conversion.
    fn hsv(&self) -> (f32, f32, f32) {
        let fg = self.model().fg;
        if let Some(hsv) = self.hsv {
            let (r, g, b) = hsv_to_rgb(hsv.0, hsv.1, hsv.2);
            if (r, g, b) == (fg[0], fg[1], fg[2]) {
                return hsv;
            }
        }
        rgb_to_hsv(fg)
    }
    /// Track zone of slider `row` (label left, track right of x+92).
    fn track(&self, row: usize) -> Rect {
        let r = row_rect(self.st.bounds.min_y() + 8.0, self.st.bounds.min_x(), self.st.bounds.max_x(), row, 0.0, 34.0);
        rr(r.min_x() + 92.0, r.min_y() + r.height() / 2.0 - 6.0, r.max_x() - 4.0, r.min_y() + r.height() / 2.0 + 6.0)
    }
}

impl Widget for ColorPanel {
    fn measure(&mut self, _cx: &mut LayoutContext, _c: LayoutConstraints) -> Vec2 {
        Vec2::new(100.0, 150.0)
    }

    fn layout(&mut self, _cx: &mut LayoutContext, bounds: Rect) {
        self.st.bounds = bounds;
        self.st.content_h = 3.0 * 34.0 + 44.0;
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        match cx.event {
            WidgetEvent::PointerPressed { position, .. } => {
                self.drag = (0..3).find(|&i| contains(&self.track(i), *position));
                if self.drag.is_some() { EventResponse::Handled } else { EventResponse::Ignored }
            }
            WidgetEvent::PointerMoved { position, .. } => {
                let Some(row) = self.drag else {
                    return EventResponse::Ignored;
                };
                let t = ((position.x - self.track(row).min_x()) / self.track(row).width()).clamp(0.0, 1.0);
                let (h, s, v) = self.hsv();
                let mut out = [h, s, v];
                out[row] = t;
                self.hsv = Some((out[0], out[1], out[2]));
                let (r, g, b) = hsv_to_rgb(out[0], out[1], out[2]);
                self.push(DockCmd::Engine("tools.setColors".into(), json!({"foreground": format!("#{r:02x}{g:02x}{b:02x}")})));
                EventResponse::Handled
            }
            WidgetEvent::PointerReleased { .. } => {
                self.drag = None;
                EventResponse::Ignored
            }
            _ => EventResponse::Ignored,
        }
    }

    fn paint(&self, cx: &mut PaintContext) {
        cx.list.push_clip(kr(self.st.bounds));
        let m = self.model();
        let (h, s, v) = self.hsv();
        let labels = ["Hue", "Saturation", "Brightness"];
        let vals = [h * 360.0, s * 100.0, v * 100.0];
        let maxes = [360.0_f32, 100.0, 100.0];
        let txt = cx.color(TokenKey::TextColor, [200, 205, 215, 255]);
        let track_c = cx.color(TokenKey::BorderColor, [60, 62, 70, 255]);
        let accent = cx.color(TokenKey::AccentColor, [96, 165, 250, 255]);
        for (i, ((l, val), max)) in labels.iter().zip(vals.iter()).zip(maxes.iter()).enumerate() {
            let r = row_rect(self.st.bounds.min_y() + 8.0, self.st.bounds.min_x(), self.st.bounds.max_x(), i, 0.0, 34.0);
            text(cx, r.min_x() + 2.0, r.min_y() + 14.0, *l, 10.5, txt);
            text(cx, r.min_x() + 2.0, r.min_y() + 28.0, format!("{val:.0}"), 9.5, txt);
            let tr = self.track(i);
            cx.list.push_fill_rect(kr(tr), track_c);
            let kx = tr.min_x() + (val / max) * tr.width();
            cx.list.push_fill_rect(kr(rr(kx - 2.0, tr.min_y(), kx + 2.0, tr.max_y())), accent);
        }
        let sw = rr(
            self.st.bounds.min_x() + 2.0,
            self.st.bounds.min_y() + 8.0 + 3.0 * 34.0 + 4.0,
            self.st.bounds.min_x() + 28.0,
            self.st.bounds.min_y() + 8.0 + 3.0 * 34.0 + 30.0,
        );
        cx.list.push_fill_rect(kr(sw), m.fg);
        text(cx, sw.max_x() + 6.0, sw.min_y() + 18.0, format!("#{:02X}{:02X}{:02X}", m.fg[0], m.fg[1], m.fg[2]), 12.0, txt);
        text(cx, sw.max_x() + 92.0, sw.min_y() + 18.0, format!("RGB {} {} {}", m.fg[0], m.fg[1], m.fg[2]), 10.0, txt);
        cx.list.pop_clip();
    }
}

fn rgb_to_hsv(c: [u8; 4]) -> (f32, f32, f32) {
    let (r, g, b) = (c[0] as f32 / 255.0, c[1] as f32 / 255.0, c[2] as f32 / 255.0);
    let (max, min) = (r.max(g).max(b), r.min(g).min(b));
    let d = max - min;
    let s = if max <= 0.0 { 0.0 } else { d / max };
    let h = if d <= 0.0 {
        0.0
    } else if max == r {
        ((g - b) / d).rem_euclid(6.0)
    } else if max == g {
        (b - r) / d + 2.0
    } else {
        (r - g) / d + 4.0
    };
    (h / 6.0, s, max)
}

fn hsv_to_rgb(h: f32, s: f32, v: f32) -> (u8, u8, u8) {
    let i = (h * 6.0).floor() as i32;
    let f = h * 6.0 - i as f32;
    let (p, q, t) = (v * (1.0 - s), v * (1.0 - f * s), v * (1.0 - (1.0 - f) * s));
    let (r, g, b) = match i.rem_euclid(6) {
        0 => (v, t, p),
        1 => (q, v, p),
        2 => (p, v, t),
        3 => (p, q, v),
        4 => (t, p, v),
        _ => (v, p, q),
    };
    ((r * 255.0) as u8, (g * 255.0) as u8, (b * 255.0) as u8)
}

// ---------------------------------------------------------------- presets

#[derive(Clone, Copy, PartialEq)]
enum PresetKind {
    Gradients,
    Patterns,
}

struct PresetPanel {
    io: Shared,
    st: ListState,
    kind: PresetKind,
}

impl PresetPanel {
    fn new(io: &Shared, kind: PresetKind) -> Self {
        Self { io: io.clone(), st: ListState::new(), kind }
    }
    fn model(&self) -> DockModel {
        io_lock(&self.io).model.clone()
    }
    fn push(&self, cmd: DockCmd) {
        io_lock(&self.io).actions.push(cmd);
    }
    /// `(command key, display label)` rows.
    fn rows(&self) -> Vec<(String, String)> {
        let m = self.model();
        match self.kind {
            PresetKind::Gradients => m.gradient_presets.into_iter().map(|n| (n.clone(), n)).collect(),
            PresetKind::Patterns => m.pattern_presets,
        }
    }
}

impl Widget for PresetPanel {
    fn measure(&mut self, _cx: &mut LayoutContext, _c: LayoutConstraints) -> Vec2 {
        Vec2::new(100.0, 150.0)
    }

    fn layout(&mut self, _cx: &mut LayoutContext, bounds: Rect) {
        self.st.bounds = bounds;
        self.st.content_h = self.rows().len() as f32 * ROW_H;
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        if list_event(&mut self.st, cx) {
            return EventResponse::Handled;
        }
        if let WidgetEvent::PointerPressed { position, .. } = cx.event {
            if let Some(i) = self.st.row_at(ROW_H, self.st.bounds.min_y(), *position)
                && let Some((key, _)) = self.rows().get(i).cloned()
            {
                let (cmd, param) = match self.kind {
                    PresetKind::Gradients => ("gradient.presets.select", "preset"),
                    PresetKind::Patterns => ("pattern.presets.select", "pattern"),
                };
                self.push(DockCmd::Engine(cmd.into(), json!({param: key})));
            }
            return EventResponse::Handled;
        }
        EventResponse::Ignored
    }

    fn paint(&self, cx: &mut PaintContext) {
        cx.list.push_clip(kr(self.st.bounds));
        let rows = self.rows();
        if rows.is_empty() {
            paint_empty(cx, self.st.bounds, "No presets");
            cx.list.pop_clip();
            return;
        }
        for (i, (_, name)) in rows.iter().enumerate() {
            let r = row_rect(self.st.bounds.min_y(), self.st.bounds.min_x(), self.st.bounds.max_x(), i, self.st.scroll, ROW_H);
            if r.max_y() < self.st.bounds.min_y() || r.min_y() > self.st.bounds.max_y() {
                continue;
            }
            paint_row(cx, &self.st, r, false, name, None);
        }
        cx.list.pop_clip();
    }
}

// ---------------------------------------------------------------- properties

list_widget!(PropertiesPanel);

impl Widget for PropertiesPanel {
    fn measure(&mut self, _cx: &mut LayoutContext, _c: LayoutConstraints) -> Vec2 {
        Vec2::new(100.0, 150.0)
    }

    fn layout(&mut self, _cx: &mut LayoutContext, bounds: Rect) {
        self.st.bounds = bounds;
        self.st.content_h = 140.0;
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        let _ = list_event(&mut self.st, cx);
        EventResponse::Ignored
    }

    fn paint(&self, cx: &mut PaintContext) {
        cx.list.push_clip(kr(self.st.bounds));
        let m = self.model();
        if !m.has_doc {
            paint_empty(cx, self.st.bounds, "No document");
            cx.list.pop_clip();
            return;
        }
        let txt = cx.color(TokenKey::TextColor, [200, 205, 215, 255]);
        let dim = cx.color(TokenKey::TextColor, [110, 116, 128, 255]);
        let mut y = self.st.bounds.min_y() + 18.0;
        if let Some(l) = &m.layer {
            for (k, v) in [
                ("Blend mode", l.blend.clone()),
                ("Opacity", format!("{:.0}%", l.opacity * 100.0)),
                ("Fill", format!("{:.0}%", l.fill * 100.0)),
                ("Locked", if l.locked { "Yes".into() } else { "No".into() }),
            ] {
                text(cx, self.st.bounds.min_x() + 8.0, y, k, 10.5, dim);
                text(cx, self.st.bounds.min_x() + 96.0, y, v, 11.5, txt);
                y += 22.0;
            }
        } else {
            paint_empty(cx, self.st.bounds, "No active layer");
        }
        cx.list.pop_clip();
    }
}

// ---------------------------------------------------------------- adjustments

list_widget!(AdjustmentsPanel);

/// The new-adjustment-layer grid, engine command per cell.
fn adjustment_ids() -> &'static [(&'static str, &'static str)] {
    &[
        ("layer.newAdjustmentLayer.brightnessContrast", "Brightness"),
        ("layer.newAdjustmentLayer.levels", "Levels"),
        ("layer.newAdjustmentLayer.curves", "Curves"),
        ("layer.newAdjustmentLayer.exposure", "Exposure"),
        ("layer.newAdjustmentLayer.vibrance", "Vibrance"),
        ("layer.newAdjustmentLayer.hueSaturation", "Hue/Sat"),
        ("layer.newAdjustmentLayer.colorBalance", "Color Bal"),
        ("layer.newAdjustmentLayer.blackWhite", "B&W"),
        ("layer.newAdjustmentLayer.photoFilter", "Photo Flt"),
        ("layer.newAdjustmentLayer.channelMixer", "Channel"),
        ("layer.newAdjustmentLayer.colorLookup", "Color Lkp"),
        ("layer.newAdjustmentLayer.invert", "Invert"),
        ("layer.newAdjustmentLayer.posterize", "Posterize"),
        ("layer.newAdjustmentLayer.threshold", "Threshold"),
        ("layer.newAdjustmentLayer.gradientMap", "Grad Map"),
        ("layer.newAdjustmentLayer.selectiveColor", "Selective"),
    ]
}

impl Widget for AdjustmentsPanel {
    fn measure(&mut self, _cx: &mut LayoutContext, _c: LayoutConstraints) -> Vec2 {
        Vec2::new(100.0, 150.0)
    }

    fn layout(&mut self, _cx: &mut LayoutContext, bounds: Rect) {
        self.st.bounds = bounds;
        self.st.content_h = (adjustment_ids().len() as f32 / 4.0).ceil() * 44.0 + 8.0;
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        if list_event(&mut self.st, cx) {
            return EventResponse::Handled;
        }
        if let WidgetEvent::PointerPressed { position, .. } = cx.event {
            let w = (self.st.bounds.width() - 8.0) / 4.0;
            for (i, (id, _)) in adjustment_ids().iter().enumerate() {
                let (gx, gy) = ((i % 4) as f32, (i / 4) as f32);
                let x = self.st.bounds.min_x() + 4.0 + gx * w;
                let y = self.st.bounds.min_y() + 4.0 + gy * 44.0 - self.st.scroll;
                if contains(&rr(x, y, x + w - 4.0, y + 40.0), *position) {
                    self.push(DockCmd::Engine((*id).into(), json!({})));
                    return EventResponse::Handled;
                }
            }
            return EventResponse::Handled;
        }
        EventResponse::Ignored
    }

    fn paint(&self, cx: &mut PaintContext) {
        cx.list.push_clip(kr(self.st.bounds));
        let m = self.model();
        if !m.has_doc {
            paint_empty(cx, self.st.bounds, "No document");
            cx.list.pop_clip();
            return;
        }
        let txt = cx.color(TokenKey::TextColor, [200, 205, 215, 255]);
        let cell_c = cx.color(TokenKey::SurfaceColor, [38, 40, 47, 255]);
        let hover_c = cx.color(TokenKey::SurfaceColor, [52, 55, 64, 255]);
        let w = (self.st.bounds.width() - 8.0) / 4.0;
        for (i, (_, label)) in adjustment_ids().iter().enumerate() {
            let (gx, gy) = ((i % 4) as f32, (i / 4) as f32);
            let x = self.st.bounds.min_x() + 4.0 + gx * w;
            let y = self.st.bounds.min_y() + 4.0 + gy * 44.0 - self.st.scroll;
            let r = rr(x, y, x + w - 4.0, y + 40.0);
            if r.max_y() < self.st.bounds.min_y() || r.min_y() > self.st.bounds.max_y() {
                continue;
            }
            cx.list.push_fill_rect(kr(r), if self.st.hover.is_some_and(|p| contains(&r, p)) { hover_c } else { cell_c });
            text(cx, r.min_x() + 5.0, r.min_y() + r.height() / 2.0 + 4.0, *label, 9.5, txt);
        }
        cx.list.pop_clip();
    }
}

// ---------------------------------------------------------------- type panels

struct TypePanel {
    io: Shared,
    st: ListState,
    paragraph: bool,
}

impl TypePanel {
    fn new(io: &Shared, paragraph: bool) -> Self {
        Self { io: io.clone(), st: ListState::new(), paragraph }
    }
}

impl Widget for TypePanel {
    fn measure(&mut self, _cx: &mut LayoutContext, _c: LayoutConstraints) -> Vec2 {
        Vec2::new(100.0, 160.0)
    }

    fn layout(&mut self, _cx: &mut LayoutContext, bounds: Rect) {
        self.st.bounds = bounds;
        self.st.content_h = if self.paragraph { 160.0 } else { 200.0 };
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        let _ = list_event(&mut self.st, cx);
        EventResponse::Ignored
    }

    fn paint(&self, cx: &mut PaintContext) {
        cx.list.push_clip(kr(self.st.bounds));
        let m = io_lock(&self.io).model.clone();
        let txt = cx.color(TokenKey::TextColor, [200, 205, 215, 255]);
        let dim = cx.color(TokenKey::TextColor, [110, 116, 128, 255]);
        let mut y = self.st.bounds.min_y() + 18.0 - self.st.scroll;
        let rows: Vec<(String, String)> = if self.paragraph {
            match &m.para {
                Some(p) => [
                    ("Align", p.align.clone()),
                    ("Indent left", format!("{:.1} pt", p.start_indent_pt)),
                    ("Indent right", format!("{:.1} pt", p.end_indent_pt)),
                    ("First line", format!("{:.1} pt", p.first_line_pt)),
                    ("Space before", format!("{:.1} pt", p.space_before_pt)),
                    ("Space after", format!("{:.1} pt", p.space_after_pt)),
                ]
                .iter()
                .map(|(k, v)| ((*k).to_string(), v.clone()))
                .collect(),
                None => return_empty(cx, &self.st, m.has_doc),
            }
        } else {
            match &m.char {
                Some(c) => [
                    ("Font", c.family.clone()),
                    ("Style", c.style.clone()),
                    ("Size", format!("{:.1} pt", c.size_pt)),
                    ("Leading", c.leading_pt.map_or("Auto".into(), |l| format!("{l:.1} pt"))),
                    ("Tracking", format!("{:.0}", c.tracking)),
                    ("Colour", format!("#{:02X}{:02X}{:02X}", c.color[0], c.color[1], c.color[2])),
                ]
                .iter()
                .map(|(k, v)| ((*k).to_string(), v.clone()))
                .collect(),
                None => return_empty(cx, &self.st, m.has_doc),
            }
        };
        for (k, v) in rows {
            text(cx, self.st.bounds.min_x() + 8.0, y, k, 10.5, dim);
            text(cx, self.st.bounds.min_x() + 96.0, y, v, 11.5, txt);
            y += 22.0;
        }
        cx.list.pop_clip();
    }
}

/// Empty-state helper for the type panels: the panel is only meaningful on a
/// text layer.
fn return_empty(cx: &mut PaintContext, st: &ListState, has_doc: bool) -> Vec<(String, String)> {
    paint_empty(cx, st.bounds, if has_doc { "No type layer selected" } else { "No document" });
    cx.list.pop_clip();
    Vec::new()
}

// ---------------------------------------------------------------- navigator

list_widget!(NavigatorPanel);

impl NavigatorPanel {
    /// The scaled doc rect inside the panel.
    fn doc_rect(&self) -> Option<(Rect, f32)> {
        let m = self.model();
        if !m.has_doc || m.doc_w == 0 || m.doc_h == 0 {
            return None;
        }
        let b = self.st.bounds;
        let pad = 8.0;
        let (aw, ah) = (b.width() - pad * 2.0, b.height() - pad * 2.0);
        let scale = (aw / m.doc_w as f32).min(ah / m.doc_h as f32);
        if scale <= 0.0 {
            return None;
        }
        let (dw, dh) = (m.doc_w as f32 * scale, m.doc_h as f32 * scale);
        Some((
            rr(b.min_x() + pad + (aw - dw) / 2.0, b.min_y() + pad + (ah - dh) / 2.0, b.min_x() + pad + (aw + dw) / 2.0, b.min_y() + pad + (ah + dh) / 2.0),
            scale,
        ))
    }
}

impl Widget for NavigatorPanel {
    fn measure(&mut self, _cx: &mut LayoutContext, _c: LayoutConstraints) -> Vec2 {
        Vec2::new(100.0, 120.0)
    }

    fn layout(&mut self, _cx: &mut LayoutContext, bounds: Rect) {
        self.st.bounds = bounds;
        self.st.content_h = bounds.height();
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        if let WidgetEvent::PointerPressed { position, .. } | WidgetEvent::PointerMoved { position, .. } = cx.event
            && let Some((r, scale)) = self.doc_rect()
            && contains(&r, *position)
        {
            self.push(DockCmd::PanTo((position.x - r.min_x()) / scale, (position.y - r.min_y()) / scale));
            return EventResponse::Handled;
        }
        EventResponse::Ignored
    }

    fn paint(&self, cx: &mut PaintContext) {
        cx.list.push_clip(kr(self.st.bounds));
        let m = self.model();
        let Some((doc_r, _)) = self.doc_rect() else {
            paint_empty(cx, self.st.bounds, "No document");
            cx.list.pop_clip();
            return;
        };
        cx.list.push_fill_rect(kr(doc_r), [40, 41, 46, 255]);
        if let Some(img) = &m.nav_preview {
            cx.list.push_image(kr(doc_r), img.clone());
        }
        cx.list.push_stroke_rect(kr(doc_r), 1.0, cx.color(TokenKey::BorderColor, [80, 82, 92, 255]));
        if let Some((vx, vy, vw, vh)) = m.viewport {
            let vr = rr(
                doc_r.min_x() + vx * doc_r.width(),
                doc_r.min_y() + vy * doc_r.height(),
                doc_r.min_x() + (vx + vw) * doc_r.width(),
                doc_r.min_y() + (vy + vh) * doc_r.height(),
            );
            cx.list.push_stroke_rect(kr(vr), 1.0, [237, 80, 80, 255]);
        }
        cx.list.pop_clip();
    }
}

// ---------------------------------------------------------------- histogram

list_widget!(HistogramPanel);

impl Widget for HistogramPanel {
    fn measure(&mut self, _cx: &mut LayoutContext, _c: LayoutConstraints) -> Vec2 {
        Vec2::new(100.0, 100.0)
    }

    fn layout(&mut self, _cx: &mut LayoutContext, bounds: Rect) {
        self.st.bounds = bounds;
        self.st.content_h = bounds.height();
    }

    fn event(&mut self, _cx: &mut EventContext) -> EventResponse {
        EventResponse::Ignored
    }

    fn paint(&self, cx: &mut PaintContext) {
        cx.list.push_clip(kr(self.st.bounds));
        let m = self.model();
        if !m.has_doc || m.histogram.is_empty() {
            paint_empty(cx, self.st.bounds, "No document");
            cx.list.pop_clip();
            return;
        }
        let b = self.st.bounds;
        let area = rr(b.min_x() + 4.0, b.min_y() + 4.0, b.max_x() - 4.0, b.max_y() - 18.0);
        let max = m.histogram.iter().copied().max().unwrap_or(1).max(1) as f32;
        let col_w = area.width() / m.histogram.len() as f32;
        for (i, &c) in m.histogram.iter().enumerate() {
            let h = (c as f32 / max) * area.height();
            let x = area.min_x() + i as f32 * col_w;
            cx.list.push_fill_rect(kr(rr(x, area.max_y() - h, x + col_w.max(1.0), area.max_y())), [150, 160, 175, 255]);
        }
        let dim = cx.color(TokenKey::TextColor, [110, 116, 128, 255]);
        text(cx, b.min_x() + 4.0, b.max_y() - 5.0, "Luminance · 256 bins", 9.5, dim);
        cx.list.pop_clip();
    }
}

// ---------------------------------------------------------------- info

list_widget!(InfoPanel);

impl Widget for InfoPanel {
    fn measure(&mut self, _cx: &mut LayoutContext, _c: LayoutConstraints) -> Vec2 {
        Vec2::new(100.0, 130.0)
    }

    fn layout(&mut self, _cx: &mut LayoutContext, bounds: Rect) {
        self.st.bounds = bounds;
        self.st.content_h = 140.0;
    }

    fn event(&mut self, _cx: &mut EventContext) -> EventResponse {
        EventResponse::Ignored
    }

    fn paint(&self, cx: &mut PaintContext) {
        cx.list.push_clip(kr(self.st.bounds));
        let m = self.model();
        if !m.has_doc {
            paint_empty(cx, self.st.bounds, "No document");
            cx.list.pop_clip();
            return;
        }
        let txt = cx.color(TokenKey::TextColor, [200, 205, 215, 255]);
        let dim = cx.color(TokenKey::TextColor, [110, 116, 128, 255]);
        let mut y = self.st.bounds.min_y() + 18.0;
        for (k, v) in [("Document", format!("{} × {}", m.doc_w, m.doc_h)), ("Mode", m.doc_mode.clone())] {
            text(cx, self.st.bounds.min_x() + 8.0, y, k, 10.5, dim);
            text(cx, self.st.bounds.min_x() + 96.0, y, v, 11.5, txt);
            y += 20.0;
        }
        y += 4.0;
        if let Some((px, py, c)) = m.pointer {
            let sw = rr(self.st.bounds.min_x() + 8.0, y - 10.0, self.st.bounds.min_x() + 22.0, y + 4.0);
            cx.list.push_fill_rect(kr(sw), c);
            cx.list.push_stroke_rect(kr(sw), 1.0, [80, 82, 92, 255]);
            text(cx, sw.max_x() + 8.0, y, format!("RGB {} {} {}", c[0], c[1], c[2]), 11.5, txt);
            y += 20.0;
            text(cx, self.st.bounds.min_x() + 8.0, y, format!("X {:.0}  Y {:.0}", px, py), 11.5, txt);
        } else {
            text(cx, self.st.bounds.min_x() + 8.0, y, "—", 11.5, dim);
        }
        cx.list.pop_clip();
    }
}

// ---------------------------------------------------------------- history

list_widget!(HistoryPanel);

impl Widget for HistoryPanel {
    fn measure(&mut self, _cx: &mut LayoutContext, _c: LayoutConstraints) -> Vec2 {
        Vec2::new(100.0, 160.0)
    }

    fn layout(&mut self, _cx: &mut LayoutContext, bounds: Rect) {
        self.st.bounds = bounds;
        let m = self.model();
        self.st.content_h = 42.0 + (m.history.len() + m.redo.len()) as f32 * ROW_H;
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        if list_event(&mut self.st, cx) {
            return EventResponse::Handled;
        }
        if let WidgetEvent::PointerPressed { position, .. } = cx.event {
            let m = self.model();
            let y0 = self.st.bounds.min_y() + 42.0;
            if let Some(i) = self.st.row_at(ROW_H, y0, *position) {
                let total = m.history.len();
                // Undo rows index 0..total-1 around `history_current`; redo
                // rows follow. The click jumps by repeating undo/redo — the
                // same mechanics the egui panel uses.
                let delta = if i < total { i as isize - m.history_current as isize } else { (i - total + 1) as isize };
                let (cmd, n) = if delta < 0 { ("edit.undo", (-delta) as usize) } else { ("edit.redo", delta as usize) };
                for _ in 0..n {
                    self.push(DockCmd::Engine(cmd.into(), json!({})));
                }
            }
            return EventResponse::Handled;
        }
        EventResponse::Ignored
    }

    fn paint(&self, cx: &mut PaintContext) {
        cx.list.push_clip(kr(self.st.bounds));
        let m = self.model();
        if !m.has_doc {
            paint_empty(cx, self.st.bounds, "No document");
            cx.list.pop_clip();
            return;
        }
        let txt = cx.color(TokenKey::TextColor, [200, 205, 215, 255]);
        let dim = cx.color(TokenKey::TextColor, [110, 116, 128, 255]);
        // Snapshot row.
        let thumb = rr(self.st.bounds.min_x() + 8.0, self.st.bounds.min_y() + 5.0, self.st.bounds.min_x() + 36.0, self.st.bounds.min_y() + 33.0);
        cx.list.push_fill_rect(kr(thumb), [40, 41, 46, 255]);
        cx.list.push_stroke_rect(kr(thumb), 1.0, [80, 82, 92, 255]);
        text(cx, thumb.max_x() + 8.0, self.st.bounds.min_y() + 23.0, m.doc_name.clone(), 11.5, txt);
        let border = cx.color(TokenKey::BorderColor, [60, 62, 70, 255]);
        cx.list.push_stroke_rect(
            kr(rr(self.st.bounds.min_x(), self.st.bounds.min_y() + 41.0, self.st.bounds.max_x(), self.st.bounds.min_y() + 41.0)),
            0.5,
            border,
        );
        let y0 = self.st.bounds.min_y() + 42.0;
        for (i, e) in m.history.iter().enumerate() {
            let r = row_rect(y0, self.st.bounds.min_x(), self.st.bounds.max_x(), i, self.st.scroll, ROW_H);
            if r.max_y() < self.st.bounds.min_y() || r.min_y() > self.st.bounds.max_y() {
                continue;
            }
            paint_row(cx, &self.st, r, i == m.history_current, e, None);
        }
        let off = m.history.len();
        for (j, e) in m.redo.iter().enumerate() {
            let r = row_rect(y0, self.st.bounds.min_x(), self.st.bounds.max_x(), off + j, self.st.scroll, ROW_H);
            if r.max_y() < self.st.bounds.min_y() || r.min_y() > self.st.bounds.max_y() {
                continue;
            }
            if self.st.hover.is_some_and(|p| contains(&r, p)) {
                cx.list.push_fill_rect(kr(r), cx.color(TokenKey::SurfaceColor, [44, 46, 54, 255]));
            }
            text(cx, r.min_x() + 8.0, r.min_y() + 16.0, e.clone(), 11.5, dim);
        }
        cx.list.pop_clip();
    }
}

// ---------------------------------------------------------------- actions

list_widget!(ActionsPanel);

impl Widget for ActionsPanel {
    fn measure(&mut self, _cx: &mut LayoutContext, _c: LayoutConstraints) -> Vec2 {
        Vec2::new(100.0, 140.0)
    }

    fn layout(&mut self, _cx: &mut LayoutContext, bounds: Rect) {
        self.st.bounds = bounds;
        self.st.content_h = self.model().actions.len() as f32 * ROW_H;
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        if list_event(&mut self.st, cx) {
            return EventResponse::Handled;
        }
        if let WidgetEvent::PointerPressed { position, .. } = cx.event
            && let Some(i) = self.st.row_at(ROW_H, self.st.bounds.min_y(), *position)
            && let Some(name) = self.model().actions.get(i).cloned()
        {
            self.push(DockCmd::Engine("actions.play".into(), json!({"action": name})));
            return EventResponse::Handled;
        }
        EventResponse::Ignored
    }

    fn paint(&self, cx: &mut PaintContext) {
        cx.list.push_clip(kr(self.st.bounds));
        let m = self.model();
        if m.actions.is_empty() {
            paint_empty(cx, self.st.bounds, "No actions");
            cx.list.pop_clip();
            return;
        }
        for (i, name) in m.actions.iter().enumerate() {
            let r = row_rect(self.st.bounds.min_y(), self.st.bounds.min_x(), self.st.bounds.max_x(), i, self.st.scroll, ROW_H);
            paint_row(cx, &self.st, r, false, name, Some("▶"));
        }
        cx.list.pop_clip();
    }
}

// ---------------------------------------------------------------- comps

list_widget!(CompsPanel);

impl Widget for CompsPanel {
    fn measure(&mut self, _cx: &mut LayoutContext, _c: LayoutConstraints) -> Vec2 {
        Vec2::new(100.0, 140.0)
    }

    fn layout(&mut self, _cx: &mut LayoutContext, bounds: Rect) {
        self.st.bounds = bounds;
        self.st.content_h = self.model().comps.len() as f32 * ROW_H;
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        if list_event(&mut self.st, cx) {
            return EventResponse::Handled;
        }
        if let WidgetEvent::PointerPressed { position, .. } = cx.event
            && let Some(i) = self.st.row_at(ROW_H, self.st.bounds.min_y(), *position)
            && let Some(c) = self.model().comps.get(i)
        {
            self.push(DockCmd::Engine("layerComp.apply".into(), json!({"comp": c.id})));
            return EventResponse::Handled;
        }
        EventResponse::Ignored
    }

    fn paint(&self, cx: &mut PaintContext) {
        cx.list.push_clip(kr(self.st.bounds));
        let m = self.model();
        if !m.has_doc || m.comps.is_empty() {
            paint_empty(cx, self.st.bounds, if m.has_doc { "No layer comps" } else { "No document" });
            cx.list.pop_clip();
            return;
        }
        for (i, c) in m.comps.iter().enumerate() {
            let r = row_rect(self.st.bounds.min_y(), self.st.bounds.min_x(), self.st.bounds.max_x(), i, self.st.scroll, ROW_H);
            paint_row(cx, &self.st, r, c.applied, &c.name, None);
        }
        cx.list.pop_clip();
    }
}

// ---------------------------------------------------------------- layers

struct LayersPanel {
    io: Shared,
    st: ListState,
    /// The blend-mode dropdown — a real `MenuButton` child whose label the
    /// tick updates to the active layer's mode.
    blend: MenuButton,
    /// Scrub drag on the opacity/fill field.
    scrub: Option<usize>,
    /// Armed row drag (reorder): the pressed layer id and press position.
    drag_row: Option<(u64, Vec2)>,
}

impl LayersPanel {
    fn new(io: &Shared) -> Self {
        Self { io: io.clone(), st: ListState::new(), blend: MenuButton::new("Normal", blend_items(false)), scrub: None, drag_row: None }
    }
    fn model(&self) -> DockModel {
        io_lock(&self.io).model.clone()
    }
    fn push(&self, cmd: DockCmd) {
        io_lock(&self.io).actions.push(cmd);
    }
    fn header_h(&self) -> f32 {
        54.0
    }
    fn footer_h(&self) -> f32 {
        34.0
    }
    /// The opacity/fill field rects (right-aligned, rows 1 and 2).
    fn field_r(&self, zone: usize) -> Rect {
        let b = self.st.bounds;
        let (y0, y1) = if zone == 0 { (b.min_y() + 4.0, b.min_y() + 28.0) } else { (b.min_y() + 30.0, b.min_y() + self.header_h()) };
        rr(b.max_x() - 70.0, y0, b.max_x() - 4.0, y1)
    }
}

/// Blend-mode menu items for the header dropdown.
fn blend_items(group: bool) -> Vec<MenuItem> {
    // Photoshop's mode list — `Pass Through` first for groups.
    let modes: &[&str] = &[
        "Normal",
        "Dissolve",
        "Darken",
        "Multiply",
        "Color Burn",
        "Linear Burn",
        "Darker Color",
        "Lighten",
        "Screen",
        "Color Dodge",
        "Linear Dodge",
        "Lighter Color",
        "Overlay",
        "Soft Light",
        "Hard Light",
        "Vivid Light",
        "Linear Light",
        "Pin Light",
        "Hard Mix",
        "Difference",
        "Exclusion",
        "Subtract",
        "Divide",
        "Hue",
        "Saturation",
        "Color",
        "Luminosity",
    ];
    let mut v: Vec<MenuItem> = modes.iter().map(|m| MenuItem::action(*m)).collect();
    if group {
        v.insert(0, MenuItem::action("Pass Through"));
    }
    v
}

/// The Layers-panel footer buttons, left-to-right in Photoshop's order; the
/// rects come back right-to-left so index maps `6 - i`.
const FOOTER: [(&str, &str); 7] = [
    ("link", "text.link"),
    ("fx", "data.percent"),
    ("mask", "status.eye"),
    ("adjustment", "data.filter"),
    ("group", "file.folder"),
    ("new", "status.plus"),
    ("delete", "edit.trash"),
];

impl Widget for LayersPanel {
    fn measure(&mut self, _cx: &mut LayoutContext, _c: LayoutConstraints) -> Vec2 {
        Vec2::new(160.0, 240.0)
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.st.bounds = bounds;
        // Keep the dropdown label and items in sync with the model.
        let m = self.model();
        if let Some(l) = &m.layer {
            self.blend.label = l.blend.clone();
        }
        if !m.blend_modes.is_empty() {
            let items: Vec<MenuItem> = m.blend_modes.iter().map(|s| MenuItem::action(s.clone())).collect();
            self.blend.set_items(items);
        }
        self.st.content_h = m.layers.len() as f32 * ROW_H;
        let blend_r = rr(bounds.min_x() + 4.0, bounds.min_y() + 4.0, bounds.max_x() - 74.0, bounds.min_y() + 28.0);
        cx.layout_child(&mut self.blend, blend_r);
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        let m = self.model();
        let y0 = self.st.bounds.min_y() + self.header_h();
        match cx.event {
            WidgetEvent::Scroll { position, delta } if contains(&self.st.bounds, *position) => {
                self.st.clamp_scroll(delta.y);
                EventResponse::Handled
            }
            WidgetEvent::PointerMoved { position, .. } => {
                self.st.hover = if contains(&self.st.bounds, *position) { Some(*position) } else { None };
                if let Some(zone) = self.scrub
                    && let Some(l) = &m.layer
                {
                    let f = self.field_r(zone);
                    let t = ((position.x - f.min_x()) / f.width()).clamp(0.0, 1.0);
                    let key = if zone == 0 { "opacity" } else { "fill" };
                    self.push(DockCmd::Engine("layer.setProps".into(), json!({"layer": l.id, key: t, "coalesce": key})));
                    return EventResponse::Handled;
                }
                // The dropdown forwards its own events.
                let r = self.forward_event_to_children(cx);
                if !matches!(r, EventResponse::Ignored) {
                    return r;
                }
                EventResponse::RequestRepaint
            }
            WidgetEvent::PointerPressed { position, button, .. } => {
                if !contains(&self.st.bounds, *position) {
                    return EventResponse::Ignored;
                }
                // The blend dropdown is a real child — give it first claim.
                if *button == PointerButton::Primary {
                    let r = self.forward_event_to_children(cx);
                    if !matches!(r, EventResponse::Ignored) {
                        return r;
                    }
                }
                for zone in 0..2 {
                    if contains(&self.field_r(zone), *position) {
                        self.scrub = Some(zone);
                        return EventResponse::Handled;
                    }
                }
                // Lock toggle (row 2, left block) — the egui `simple_lock_toggle`:
                // the Background converts to a normal layer; elsewhere any lock
                // clears them all, none locks everything.
                let lock_r =
                    rr(self.st.bounds.min_x() + 4.0, self.st.bounds.min_y() + 30.0, self.st.bounds.min_x() + 76.0, self.st.bounds.min_y() + self.header_h());
                if contains(&lock_r, *position)
                    && let Some(l) = &m.layer
                {
                    if l.is_background {
                        self.push(DockCmd::Engine("layer.new.layerFromBackground".into(), json!({})));
                    } else {
                        let locks = if l.locked {
                            json!({"transparency": false, "pixels": false, "position": false, "artboard": false, "all": false})
                        } else {
                            json!({"all": true})
                        };
                        self.push(DockCmd::Engine("layer.setProps".into(), json!({"layer": l.id, "locks": locks})));
                    }
                    return EventResponse::Handled;
                }
                // Footer.
                let footer = rr(self.st.bounds.min_x(), self.st.bounds.max_y() - self.footer_h(), self.st.bounds.max_x(), self.st.bounds.max_y());
                if contains(&footer, *position) {
                    for (i, r) in footer_rects(footer, FOOTER.len(), 26.0).iter().enumerate() {
                        if contains(r, *position)
                            && let Some(cmd) = match FOOTER[FOOTER.len() - 1 - i].0 {
                                "link" => Some(("layer.linkLayers", json!({}))),
                                "fx" => Some(("layer.layerStyle.blendingOptions", json!({}))),
                                "mask" => Some(("layer.layerMask.revealAll", json!({}))),
                                "adjustment" => Some(("layer.newAdjustmentLayer.brightnessContrast", json!({}))),
                                "group" => Some(("layer.new.group", json!({}))),
                                "new" => Some(("layer.new.layer", json!({}))),
                                "delete" => Some(("layer.delete", json!({}))),
                                _ => None,
                            }
                        {
                            self.push(DockCmd::Engine(cmd.0.into(), cmd.1));
                            return EventResponse::Handled;
                        }
                    }
                    return EventResponse::Handled;
                }
                // Rows.
                if let Some(i) = self.st.row_at(ROW_H, y0, *position)
                    && let Some(row) = m.layers.get(i)
                {
                    let r = row_rect(y0, self.st.bounds.min_x(), self.st.bounds.max_x(), i, self.st.scroll, ROW_H);
                    if position.x < r.min_x() + 24.0 {
                        self.push(DockCmd::Engine("layer.setProps".into(), json!({"layer": row.id, "visible": !row.visible, "coalesce": "layer-eye-sweep"})));
                        return EventResponse::Handled;
                    }
                    let chevron_end = r.min_x() + 24.0 + row.depth as f32 * 14.0 + 14.0;
                    if row.is_group && position.x < chevron_end {
                        self.push(DockCmd::Engine("layer.setExpanded".into(), json!({"layer": row.id})));
                        return EventResponse::Handled;
                    }
                    let (shift, ctrl) = io_lock(&self.io).mods;
                    let mode = if shift {
                        "range"
                    } else if ctrl {
                        "toggle"
                    } else {
                        "replace"
                    };
                    self.push(DockCmd::Engine("layer.select".into(), json!({"layer": row.id, "mode": mode})));
                    // A plain press arms a reorder drag — the release decides
                    // click vs. drag by distance.
                    if mode == "replace" {
                        self.drag_row = Some((row.id, *position));
                    }
                    return EventResponse::Handled;
                }
                EventResponse::Handled
            }
            WidgetEvent::PointerReleased { position, .. } => {
                self.scrub = None;
                // A drag that left the pressed row reorders via `layer.moveTo`:
                // top half of the target row drops above, bottom half below,
                // and a group's middle band drops into it.
                if let Some((id, start)) = self.drag_row.take() {
                    let m = self.model();
                    let moved = (*position - start).length() > 6.0;
                    let y0 = self.st.bounds.min_y() + self.header_h();
                    if moved
                        && let Some(i) = self.st.row_at(ROW_H, y0, *position)
                        && let Some(target) = m.layers.get(i)
                        && target.id != id
                    {
                        let r = row_rect(y0, self.st.bounds.min_x(), self.st.bounds.max_x(), i, self.st.scroll, ROW_H);
                        let frac = (position.y - r.min_y()) / r.height();
                        let pos = if target.is_group && (0.25..0.75).contains(&frac) {
                            "into"
                        } else if frac < 0.5 {
                            "above"
                        } else {
                            "below"
                        };
                        self.push(DockCmd::Engine("layer.moveTo".into(), json!({"layer": id, "target": target.id, "position": pos})));
                    }
                }
                // The dropdown drains activated items here — the press itself
                // reaches it as a child, but the activation lands in shared
                // state for us to turn into a command.
                while let Some(path) = self.blend.take_activated() {
                    if let Some(i) = path.first()
                        && let Some(l) = &m.layer
                        && let Some(name) = m.blend_modes.get(*i).cloned()
                    {
                        self.push(DockCmd::Engine("layer.setProps".into(), json!({"layer": l.id, "blend": name})));
                    }
                }
                self.forward_event_to_children(cx)
            }
            _ => self.forward_event_to_children(cx),
        }
    }

    fn paint(&self, cx: &mut PaintContext) {
        cx.list.push_clip(kr(self.st.bounds));
        let m = self.model();
        let b = self.st.bounds;
        let txt = cx.color(TokenKey::TextColor, [200, 205, 215, 255]);
        let dim = cx.color(TokenKey::TextColor, [110, 116, 128, 255]);
        let field = cx.color(TokenKey::SurfaceColor, [38, 40, 47, 255]);
        let sel = cx.color(TokenKey::SurfaceColor, [52, 60, 84, 255]);
        let hover_c = cx.color(TokenKey::SurfaceColor, [44, 46, 54, 255]);
        let border = cx.color(TokenKey::BorderColor, [60, 62, 70, 255]);
        if !m.has_doc {
            paint_empty(cx, b, "No document");
            cx.list.pop_clip();
            return;
        }
        // Header row 2: lock + fill (row 1's dropdown paints as the child).
        let lock_r = rr(b.min_x() + 4.0, b.min_y() + 30.0, b.min_x() + 76.0, b.min_y() + self.header_h());
        cx.list.push_fill_rect(kr(lock_r), field);
        cx.list.push_stroke_rect(kr(lock_r), 1.0, border);
        let locked = m.layer.as_ref().is_some_and(|l| l.locked);
        text(cx, lock_r.min_x() + 5.0, lock_r.min_y() + 16.0, format!("Lock{}", if locked { "  ✕" } else { ":" }), 10.5, if locked { txt } else { dim });
        let fill_r = self.field_r(1);
        cx.list.push_fill_rect(kr(fill_r), field);
        cx.list.push_stroke_rect(kr(fill_r), 1.0, border);
        if let Some(l) = &m.layer {
            text(cx, fill_r.min_x() + 4.0, fill_r.min_y() + 16.0, format!("{:.0}", l.fill * 100.0), 11.0, txt);
        }
        text(cx, fill_r.min_x() - 32.0, fill_r.min_y() + 16.0, "Fill:", 10.5, dim);
        let op_r = self.field_r(0);
        text(cx, op_r.min_x() - 50.0, op_r.min_y() + 16.0, "Opacity:", 10.5, dim);
        cx.list.push_fill_rect(kr(op_r), field);
        cx.list.push_stroke_rect(kr(op_r), 1.0, border);
        if let Some(l) = &m.layer {
            text(cx, op_r.min_x() + 4.0, op_r.min_y() + 16.0, format!("{:.0}", l.opacity * 100.0), 11.0, txt);
        }
        let hr = rr(b.min_x(), b.min_y() + self.header_h(), b.max_x(), b.min_y() + self.header_h());
        cx.list.push_stroke_rect(kr(hr), 0.5, border);
        // Rows.
        let y0 = b.min_y() + self.header_h();
        let body_end = b.max_y() - self.footer_h();
        for (i, row) in m.layers.iter().enumerate() {
            let r = row_rect(y0, b.min_x(), b.max_x(), i, self.st.scroll, ROW_H);
            if r.max_y() < y0 || r.min_y() > body_end {
                continue;
            }
            if row.selected {
                cx.list.push_fill_rect(kr(r), sel);
            } else if self.st.hover.is_some_and(|p| contains(&r, p)) {
                cx.list.push_fill_rect(kr(r), hover_c);
            }
            let eye = if row.visible { "●" } else { "○" };
            text(cx, r.min_x() + 6.0, r.min_y() + 16.0, eye, 10.0, if row.visible { txt } else { dim });
            let indent = 24.0 + row.depth as f32 * 14.0;
            if row.is_group {
                text(cx, r.min_x() + indent, r.min_y() + 16.0, if row.expanded { "▾" } else { "▸" }, 10.0, dim);
            }
            let name_x = r.min_x() + indent + if row.is_group { 14.0 } else { 4.0 };
            let label = if row.clipped { format!("↳ {}", row.name) } else { row.name.clone() };
            text(cx, name_x, r.min_y() + 16.0, label, 11.5, txt);
            if row.locked {
                text(cx, r.max_x() - 14.0, r.min_y() + 16.0, "◦", 10.0, dim);
            }
        }
        // Footer buttons.
        let footer = rr(b.min_x(), b.max_y() - self.footer_h(), b.max_x(), b.max_y());
        cx.list.push_stroke_rect(kr(rr(footer.min_x(), footer.min_y(), footer.max_x(), footer.min_y() + 1.0)), 0.5, border);
        for (i, r) in footer_rects(footer, FOOTER.len(), 26.0).iter().enumerate() {
            let hovered = self.st.hover.is_some_and(|p| contains(r, p));
            cx.list.push_fill_rect(kr(*r), if hovered { hover_c } else { field });
            cx.list.push_stroke_rect(kr(*r), 1.0, border);
            let (name, icon) = FOOTER[FOOTER.len() - 1 - i];
            let ir = rr(r.min_x() + 4.0, r.min_y() + 4.0, r.max_x() - 4.0, r.max_y() - 4.0);
            if name == "fx" || !paint_icon(cx, ir, icon, txt) {
                text(cx, r.min_x() + (r.width() - 8.0) / 2.0, r.min_y() + r.height() / 2.0 + 4.0, if name == "fx" { "fx" } else { &name[..1] }, 9.0, txt);
            }
        }
        cx.list.pop_clip();
    }

    fn child_count(&self) -> usize {
        1
    }

    fn child(&self, _index: usize) -> Option<&dyn Widget> {
        Some(&self.blend)
    }

    fn child_mut(&mut self, _index: usize) -> Option<&mut dyn Widget> {
        Some(&mut self.blend)
    }

    fn child_bounds(&self, _index: usize) -> Option<Rect> {
        let b = self.st.bounds;
        Some(rr(b.min_x() + 4.0, b.min_y() + 4.0, b.max_x() - 74.0, b.min_y() + 28.0))
    }
}

// ---------------------------------------------------------------- channels

list_widget!(ChannelsPanel);

/// Channels footer buttons, left→right (load/save) and right→left (delete/new).
const CH_LEFT: [(&str, &str); 2] = [("load", "select.loadSelection"), ("save", "select.saveSelection")];
const CH_RIGHT: [(&str, &str); 2] = [("delete", "channel.delete"), ("new", "channel.new")];

impl ChannelsPanel {
    /// The `channel` reference of the engine's target — the footer's
    /// load-as-selection acts on it, like Photoshop.
    fn target_ref(&self) -> serde_json::Value {
        self.model().channels.iter().find(|c| c.targeted).map(|c| c.target_ref.clone()).unwrap_or_else(|| json!("composite"))
    }
}

/// Photoshop's ⌘-click selection operations (⇧ adds; ⌥ would subtract but
/// pointer events carry no Alt — the shell mirrors ⇧/⌘ only).
fn load_op(shift: bool) -> &'static str {
    if shift { "add" } else { "new" }
}

impl Widget for ChannelsPanel {
    fn measure(&mut self, _cx: &mut LayoutContext, _c: LayoutConstraints) -> Vec2 {
        Vec2::new(140.0, 160.0)
    }

    fn layout(&mut self, _cx: &mut LayoutContext, bounds: Rect) {
        self.st.bounds = bounds;
        self.st.content_h = self.model().channels.len() as f32 * ROW_H;
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        if list_event(&mut self.st, cx) {
            return EventResponse::Handled;
        }
        if let WidgetEvent::PointerPressed { position, .. } = cx.event {
            let footer = rr(self.st.bounds.min_x(), self.st.bounds.max_y() - 34.0, self.st.bounds.max_x(), self.st.bounds.max_y());
            if contains(&footer, *position) {
                let y = footer.min_y() + (footer.height() - 26.0) / 2.0;
                for i in 0..CH_LEFT.len() {
                    let l = rr(footer.min_x() + 4.0 + i as f32 * 30.0, y, footer.min_x() + 30.0 + i as f32 * 30.0, y + 26.0);
                    if contains(&l, *position) {
                        match CH_LEFT[i].0 {
                            "load" => {
                                let target = self.target_ref();
                                self.push(DockCmd::Engine("select.loadSelection".into(), json!({"channel": target})));
                            }
                            _ => self.push(DockCmd::Engine(CH_LEFT[i].1.into(), json!({}))),
                        }
                        return EventResponse::Handled;
                    }
                    let r = rr(footer.max_x() - 30.0 - i as f32 * 30.0, y, footer.max_x() - 4.0 - i as f32 * 30.0, y + 26.0);
                    if contains(&r, *position) {
                        self.push(DockCmd::Engine(CH_RIGHT[i].1.into(), json!({})));
                        return EventResponse::Handled;
                    }
                }
                return EventResponse::Handled;
            }
            if let Some(i) = self.st.row_at(ROW_H, self.st.bounds.min_y(), *position)
                && let Some(ch) = self.model().channels.get(i)
            {
                let r = row_rect(self.st.bounds.min_y(), self.st.bounds.min_x(), self.st.bounds.max_x(), i, self.st.scroll, ROW_H);
                let (shift, ctrl) = io_lock(&self.io).mods;
                if position.x < r.min_x() + 24.0 {
                    // The eye — the mask row's eye toggles the overlay view
                    // (`view.layerMask`), not the channel eyes.
                    if ch.is_mask {
                        self.push(DockCmd::Engine("view.layerMask".into(), json!({"layer": ch.layer, "mode": if ch.visible { "off" } else { "overlay" }})));
                    } else {
                        self.push(DockCmd::Engine("channel.setVisible".into(), json!({"channel": ch.channel_ref, "visible": !ch.visible})));
                    }
                } else if ctrl {
                    // ⌘-click loads the channel as a selection (⇧ adds).
                    let mut p = json!({"channel": ch.channel_ref, "operation": load_op(shift)});
                    if ch.is_mask {
                        p["layer"] = json!(ch.layer);
                    }
                    self.push(DockCmd::Engine("select.loadSelection".into(), p));
                } else {
                    if ch.is_mask {
                        io_lock(&self.io).mask_target = true;
                    } else if ch.clears_mask {
                        io_lock(&self.io).mask_target = false;
                    }
                    self.push(DockCmd::Engine("channel.target".into(), json!({"channel": ch.target_ref})));
                }
            }
            return EventResponse::Handled;
        }
        EventResponse::Ignored
    }

    fn paint(&self, cx: &mut PaintContext) {
        cx.list.push_clip(kr(self.st.bounds));
        let m = self.model();
        if !m.has_doc {
            paint_empty(cx, self.st.bounds, "No document");
            cx.list.pop_clip();
            return;
        }
        let txt = cx.color(TokenKey::TextColor, [200, 205, 215, 255]);
        for (i, ch) in m.channels.iter().enumerate() {
            let r = row_rect(self.st.bounds.min_y(), self.st.bounds.min_x(), self.st.bounds.max_x(), i, self.st.scroll, ROW_H);
            if r.max_y() < self.st.bounds.min_y() || r.min_y() > self.st.bounds.max_y() {
                continue;
            }
            let eye = if ch.visible { "●" } else { "○" };
            paint_row(cx, &self.st, r, ch.targeted, &ch.name, Some(eye));
        }
        let b = self.st.bounds;
        let footer = rr(b.min_x(), b.max_y() - 34.0, b.max_x(), b.max_y());
        let field = cx.color(TokenKey::SurfaceColor, [38, 40, 47, 255]);
        let hover_c = cx.color(TokenKey::SurfaceColor, [44, 46, 54, 255]);
        let border = cx.color(TokenKey::BorderColor, [60, 62, 70, 255]);
        cx.list.push_stroke_rect(kr(rr(footer.min_x(), footer.min_y(), footer.max_x(), footer.min_y() + 1.0)), 0.5, border);
        let y = footer.min_y() + (footer.height() - 26.0) / 2.0;
        let mut buttons: Vec<(Rect, &str)> = Vec::new();
        for i in 0..CH_LEFT.len() {
            buttons.push((rr(footer.min_x() + 4.0 + i as f32 * 30.0, y, footer.min_x() + 30.0 + i as f32 * 30.0, y + 26.0), CH_LEFT[i].0));
            buttons.push((rr(footer.max_x() - 30.0 - i as f32 * 30.0, y, footer.max_x() - 4.0 - i as f32 * 30.0, y + 26.0), CH_RIGHT[i].0));
        }
        for (r, name) in buttons {
            let hovered = self.st.hover.is_some_and(|p| contains(&r, p));
            cx.list.push_fill_rect(kr(r), if hovered { hover_c } else { field });
            cx.list.push_stroke_rect(kr(r), 1.0, border);
            let label = match name {
                "load" => "Sel",
                "save" => "Sav",
                "new" => "+",
                _ => "🗑",
            };
            text(cx, r.min_x() + (r.width() - 14.0) / 2.0, r.min_y() + r.height() / 2.0 + 4.0, label, 9.0, txt);
        }
        cx.list.pop_clip();
    }
}

// ---------------------------------------------------------------- paths

list_widget!(PathsPanel);

/// Paths footer: fill, stroke, load as selection, delete — all act on the
/// panel's selected path (`name` param takes the row key).
const PATH_FOOTER: [&str; 4] = ["path.fill", "path.stroke", "path.toSelection", "path.delete"];

impl PathsPanel {
    fn selected_key(&self) -> Option<String> {
        io_lock(&self.io).selected_path.clone()
    }
}

impl Widget for PathsPanel {
    fn measure(&mut self, _cx: &mut LayoutContext, _c: LayoutConstraints) -> Vec2 {
        Vec2::new(140.0, 160.0)
    }

    fn layout(&mut self, _cx: &mut LayoutContext, bounds: Rect) {
        self.st.bounds = bounds;
        self.st.content_h = self.model().paths.len() as f32 * ROW_H;
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        if list_event(&mut self.st, cx) {
            return EventResponse::Handled;
        }
        if let WidgetEvent::PointerPressed { position, count, .. } = cx.event {
            let footer = rr(self.st.bounds.min_x(), self.st.bounds.max_y() - 34.0, self.st.bounds.max_x(), self.st.bounds.max_y());
            if contains(&footer, *position) {
                for (i, r) in footer_rects(footer, PATH_FOOTER.len(), 26.0).iter().enumerate() {
                    if contains(r, *position)
                        && let Some(name) = self.selected_key()
                    {
                        self.push(DockCmd::Engine(PATH_FOOTER[PATH_FOOTER.len() - 1 - i].into(), json!({"name": name})));
                        return EventResponse::Handled;
                    }
                }
                return EventResponse::Handled;
            }
            if let Some(i) = self.st.row_at(ROW_H, self.st.bounds.min_y(), *position)
                && let Some(p) = self.model().paths.get(i)
            {
                if *count >= 2 && p.temporary {
                    // Double-clicking the work path saves it with the next
                    // free `Path N` name (Photoshop's "Save Path").
                    let n = self.model().paths.iter().filter(|r| !r.temporary).count() + 1;
                    self.push(DockCmd::Engine("path.rename".into(), json!({"name": "work", "to": format!("Path {n}")})));
                } else {
                    self.push(DockCmd::SelectPath(p.key.clone()));
                }
            }
            return EventResponse::Handled;
        }
        EventResponse::Ignored
    }

    fn paint(&self, cx: &mut PaintContext) {
        cx.list.push_clip(kr(self.st.bounds));
        let m = self.model();
        if !m.has_doc || m.paths.is_empty() {
            paint_empty(cx, self.st.bounds, if m.has_doc { "Draw with the Pen tool (P) or make a work path from a selection." } else { "No document" });
            cx.list.pop_clip();
            return;
        }
        for (i, p) in m.paths.iter().enumerate() {
            let r = row_rect(self.st.bounds.min_y(), self.st.bounds.min_x(), self.st.bounds.max_x(), i, self.st.scroll, ROW_H);
            if r.max_y() < self.st.bounds.min_y() || r.min_y() > self.st.bounds.max_y() {
                continue;
            }
            let name = if p.temporary { format!("_  {}", p.name) } else { p.name.clone() };
            paint_row(cx, &self.st, r, p.selected, &name, None);
        }
        let b = self.st.bounds;
        let footer = rr(b.min_x(), b.max_y() - 34.0, b.max_x(), b.max_y());
        let field = cx.color(TokenKey::SurfaceColor, [38, 40, 47, 255]);
        let hover_c = cx.color(TokenKey::SurfaceColor, [44, 46, 54, 255]);
        let border = cx.color(TokenKey::BorderColor, [60, 62, 70, 255]);
        let txt = cx.color(TokenKey::TextColor, [200, 205, 215, 255]);
        cx.list.push_stroke_rect(kr(rr(footer.min_x(), footer.min_y(), footer.max_x(), footer.min_y() + 1.0)), 0.5, border);
        for (i, r) in footer_rects(footer, PATH_FOOTER.len(), 26.0).iter().enumerate() {
            let hovered = self.st.hover.is_some_and(|p| contains(r, p));
            cx.list.push_fill_rect(kr(*r), if hovered { hover_c } else { field });
            cx.list.push_stroke_rect(kr(*r), 1.0, border);
            let label = match PATH_FOOTER[PATH_FOOTER.len() - 1 - i] {
                "path.fill" => "Fill",
                "path.stroke" => "Stk",
                "path.toSelection" => "Sel",
                _ => "🗑",
            };
            text(cx, r.min_x() + 4.0, r.min_y() + r.height() / 2.0 + 4.0, label, 9.0, txt);
        }
        cx.list.pop_clip();
    }
}

// ---------------------------------------------------------------- tests

#[cfg(test)]
mod tests {
    use super::*;
    use martensite::core::HotNode;
    use std::collections::HashSet;

    fn press(x: f32, y: f32) -> WidgetEvent {
        WidgetEvent::PointerPressed { position: Vec2::new(x, y), button: PointerButton::Primary, count: 1 }
    }

    fn lay(w: &mut dyn Widget, r: Rect) {
        let mut hot = HotNode::default();
        w.layout(&mut LayoutContext { hot: &mut hot, scale: 1.0 }, r);
    }

    fn fire(w: &mut dyn Widget, r: Rect, ev: &WidgetEvent) -> EventResponse {
        w.event(&mut EventContext { event: ev, bounds: r, scale: 1.0 })
    }

    fn io() -> Shared {
        Arc::new(Mutex::new(DockIo::default()))
    }

    fn actions(io: &Shared) -> Vec<DockCmd> {
        std::mem::take(&mut io_lock(io).actions)
    }

    fn layer_row(id: u64, name: &str) -> LayerRowModel {
        LayerRowModel {
            id,
            name: name.into(),
            depth: 0,
            visible: true,
            is_group: false,
            expanded: false,
            selected: false,
            primary: false,
            locked: false,
            clipped: false,
            kind: "pixel",
        }
    }

    // ------------------------------------------------------------ dock chrome

    #[test]
    fn rail_toggle_cycles_groups_through_collapse() {
        let mut dock = RightDock::new();
        assert_eq!(dock.reserved_width(), RAIL_W + DOCK_W.1);
        // shown → collapsed → expanded again (Photoshop's rail cycle).
        dock.toggle_group(DockGroup::Layers);
        assert!(dock.collapsed.contains(&DockGroup::Layers));
        assert!(dock.shown.contains(&DockGroup::Layers));
        dock.toggle_group(DockGroup::Layers);
        assert!(!dock.collapsed.contains(&DockGroup::Layers));
        // A hidden group rejoins `shown` in DockGroup::ALL order.
        dock.shown.retain(|g| *g != DockGroup::Color);
        dock.toggle_group(DockGroup::Color);
        assert!(dock.shown.contains(&DockGroup::Color));
        assert_eq!(dock.shown[0], DockGroup::Color);
        // Nothing shown → only the rail reserves width.
        dock.shown.clear();
        assert_eq!(dock.reserved_width(), RAIL_W);
    }

    #[test]
    fn rail_press_collapses_the_group() {
        let mut dock = RightDock::new();
        lay(&mut dock, rr(0.0, 0.0, 400.0, 600.0));
        // First rail slot = DockGroup::Color (shown by default) → collapses.
        let (g, slot) = dock.rail_slots[0];
        assert!(dock.shown.contains(&g));
        let bounds = dock.bounds;
        let ev = press(slot.min_x() + 4.0, slot.min_y() + 4.0);
        assert!(matches!(fire(&mut dock, bounds, &ev), EventResponse::RequestRepaint));
        assert!(dock.collapsed.contains(&g));
        // The runner only re-lays out on window resizes — the press must
        // reflow eagerly so the collapsed frame shrinks to its strip now,
        // not after a resize.
        let (_, fr) = dock.frame_r.iter().find(|(fg, _)| *fg == g).unwrap();
        assert!((fr.height() - STRIP_H).abs() < 0.01);
    }

    #[test]
    fn dock_layout_places_rail_at_the_right_edge() {
        let mut dock = RightDock::new();
        lay(&mut dock, rr(100.0, 50.0, 500.0, 650.0));
        assert!((dock.rail_r.max_x() - 500.0).abs() < 0.01);
        assert!((dock.col_r.max_x() - dock.rail_r.min_x()).abs() < 0.01);
        assert!(!dock.frame_r.is_empty());
        assert!(dock.frame_r.iter().all(|(_, r)| r.min_x() >= 100.0 && r.max_x() <= dock.rail_r.min_x() + 0.01));
        // Frames stack top-to-bottom without overlap.
        for w in dock.frame_r.windows(2) {
            assert!(w[0].1.max_y() <= w[1].1.min_y() + GAP + 0.01);
        }
    }

    #[test]
    fn split_drag_records_a_group_height() {
        let mut dock = RightDock::new();
        dock.shown = vec![DockGroup::Color, DockGroup::Layers];
        lay(&mut dock, rr(0.0, 0.0, 400.0, 600.0));
        let gap_y = dock.frame_r[0].1.max_y() + 2.0;
        let x = dock.col_r.min_x() + 20.0;
        let bounds = dock.bounds;
        fire(&mut dock, bounds, &press(x, gap_y));
        fire(&mut dock, bounds, &WidgetEvent::PointerMoved { position: Vec2::new(x, gap_y + 40.0) });
        assert!(dock.heights.contains_key(&0), "the drag wrote a stored height for group 0");
    }

    // ------------------------------------------------------------ layers panel

    #[test]
    fn layers_row_press_selects_the_layer() {
        let io = io();
        io_lock(&io).model = DockModel { has_doc: true, layers: vec![layer_row(7, "Top"), layer_row(3, "Bottom")], ..Default::default() };
        let mut p = LayersPanel::new(&io);
        let b = rr(0.0, 0.0, 240.0, 300.0);
        lay(&mut p, b);
        let y0 = b.min_y() + p.header_h();
        fire(&mut p, b, &press(b.min_x() + 120.0, y0 + ROW_H + 8.0));
        let acts = actions(&io);
        assert!(
            acts.iter().any(|c| matches!(c, DockCmd::Engine(id, v) if id == "layer.select" && v["layer"] == json!(3) && v["mode"] == json!("replace"))),
            "row 1 selects layer 3: {acts:?}"
        );
    }

    #[test]
    fn layers_eye_toggles_visibility() {
        let io = io();
        io_lock(&io).model = DockModel { has_doc: true, layers: vec![layer_row(7, "Top")], ..Default::default() };
        let mut p = LayersPanel::new(&io);
        let b = rr(0.0, 0.0, 240.0, 300.0);
        lay(&mut p, b);
        let y0 = b.min_y() + p.header_h();
        fire(&mut p, b, &press(b.min_x() + 8.0, y0 + 8.0));
        let acts = actions(&io);
        assert!(
            acts.iter().any(|c| matches!(c, DockCmd::Engine(id, v) if id == "layer.setProps" && v["visible"] == json!(false))),
            "the eye toggles visibility off: {acts:?}"
        );
    }

    #[test]
    fn layers_group_chevron_expands_not_selects() {
        let io = io();
        let mut row = layer_row(9, "Set");
        row.is_group = true;
        row.expanded = true;
        io_lock(&io).model = DockModel { has_doc: true, layers: vec![row], ..Default::default() };
        let mut p = LayersPanel::new(&io);
        let b = rr(0.0, 0.0, 240.0, 300.0);
        lay(&mut p, b);
        let y0 = b.min_y() + p.header_h();
        // chevron sits left of the name, past the eye zone (x in 24..38).
        fire(&mut p, b, &press(b.min_x() + 30.0, y0 + 8.0));
        let acts = actions(&io);
        assert!(
            acts.iter().any(|c| matches!(c, DockCmd::Engine(id, v) if id == "layer.setExpanded" && v["layer"] == json!(9))),
            "the chevron expands the group: {acts:?}"
        );
        assert!(!acts.iter().any(|c| matches!(c, DockCmd::Engine(id, _) if id == "layer.select")));
    }

    #[test]
    fn layers_shift_ctrl_select_modes() {
        for (mods, mode) in [((true, false), "range"), ((false, true), "toggle")] {
            let io = io();
            io_lock(&io).mods = mods;
            io_lock(&io).model = DockModel { has_doc: true, layers: vec![layer_row(5, "L")], ..Default::default() };
            let mut p = LayersPanel::new(&io);
            let b = rr(0.0, 0.0, 240.0, 300.0);
            lay(&mut p, b);
            let y0 = b.min_y() + p.header_h();
            fire(&mut p, b, &press(b.min_x() + 120.0, y0 + 8.0));
            let acts = actions(&io);
            assert!(
                acts.iter().any(|c| matches!(c, DockCmd::Engine(id, v) if id == "layer.select" && v["mode"] == json!(mode))),
                "mods {mods:?} → {mode}: {acts:?}"
            );
        }
    }

    // ------------------------------------------------------------ channels panel

    fn chan(name: &str, cref: Value, tref: Value) -> ChannelRowModel {
        ChannelRowModel {
            channel_ref: cref,
            target_ref: tref,
            layer: None,
            is_mask: false,
            clears_mask: true,
            name: name.into(),
            visible: true,
            targeted: false,
            temporary: false,
        }
    }

    #[test]
    fn channels_row_click_targets_typed_refs() {
        let io = io();
        io_lock(&io).model = DockModel {
            has_doc: true,
            channels: vec![
                chan("RGB", json!("composite"), json!("composite")),
                chan("Green", json!({"color": 1}), json!({"color": 1})),
                chan("Alpha 1", json!(0), json!(0)),
            ],
            ..Default::default()
        };
        let mut p = ChannelsPanel::new(&io);
        let b = rr(0.0, 0.0, 240.0, 300.0);
        lay(&mut p, b);
        fire(&mut p, b, &press(120.0, ROW_H + 8.0));
        let acts = actions(&io);
        assert!(
            acts.iter().any(|c| matches!(c, DockCmd::Engine(id, v) if id == "channel.target" && v["channel"] == json!({"color": 1}))),
            "row 1 targets colour channel 1: {acts:?}"
        );
        fire(&mut p, b, &press(120.0, 2.0 * ROW_H + 8.0));
        let acts = actions(&io);
        assert!(
            acts.iter().any(|c| matches!(c, DockCmd::Engine(id, v) if id == "channel.target" && v["channel"] == json!(0))),
            "row 2 targets alpha 0: {acts:?}"
        );
    }

    #[test]
    fn channels_eye_toggles_visibility() {
        let io = io();
        io_lock(&io).model = DockModel { has_doc: true, channels: vec![chan("RGB", json!("composite"), json!("composite"))], ..Default::default() };
        let mut p = ChannelsPanel::new(&io);
        let b = rr(0.0, 0.0, 240.0, 300.0);
        lay(&mut p, b);
        fire(&mut p, b, &press(10.0, 8.0));
        let acts = actions(&io);
        assert!(
            acts.iter().any(
                |c| matches!(c, DockCmd::Engine(id, v) if id == "channel.setVisible" && v["channel"] == json!("composite") && v["visible"] == json!(false))
            ),
            "the eye toggles the composite off: {acts:?}"
        );
    }

    #[test]
    fn channels_ctrl_click_loads_a_selection() {
        let io = io();
        io_lock(&io).mods = (true, true);
        io_lock(&io).model = DockModel { has_doc: true, channels: vec![chan("Alpha 1", json!(0), json!(0))], ..Default::default() };
        let mut p = ChannelsPanel::new(&io);
        let b = rr(0.0, 0.0, 240.0, 300.0);
        lay(&mut p, b);
        fire(&mut p, b, &press(120.0, 8.0));
        let acts = actions(&io);
        assert!(
            acts.iter()
                .any(|c| matches!(c, DockCmd::Engine(id, v) if id == "select.loadSelection" && v["channel"] == json!(0) && v["operation"] == json!("add"))),
            "⇧⌘-click adds the channel to the selection: {acts:?}"
        );
    }

    #[test]
    fn channels_mask_row_targets_the_mask() {
        let io = io();
        io_lock(&io).model = DockModel {
            has_doc: true,
            channels: vec![ChannelRowModel {
                channel_ref: json!("mask"),
                target_ref: json!("composite"),
                layer: Some(11),
                is_mask: true,
                clears_mask: false,
                name: "Layer 1 Mask".into(),
                visible: false,
                targeted: false,
                temporary: true,
            }],
            ..Default::default()
        };
        let mut p = ChannelsPanel::new(&io);
        let b = rr(0.0, 0.0, 240.0, 300.0);
        lay(&mut p, b);
        fire(&mut p, b, &press(120.0, 8.0));
        assert!(io_lock(&io).mask_target, "clicking the mask row sets mask_target");
        let acts = actions(&io);
        assert!(acts.iter().any(|c| matches!(c, DockCmd::Engine(id, v) if id == "channel.target" && v["channel"] == json!("composite"))));
        // The mask row's eye drives view.layerMask, not channel.setVisible.
        fire(&mut p, b, &press(10.0, 8.0));
        let acts = actions(&io);
        assert!(
            acts.iter().any(|c| matches!(c, DockCmd::Engine(id, v) if id == "view.layerMask" && v["layer"] == json!(11))),
            "mask eye → view.layerMask: {acts:?}"
        );
    }

    // ------------------------------------------------------------ paths panel

    #[test]
    fn paths_row_selects_and_footer_acts_on_it() {
        let io = io();
        io_lock(&io).model = DockModel {
            has_doc: true,
            paths: vec![
                PathRowModel { key: "Path 1".into(), name: "Path 1".into(), selected: false, temporary: false },
                PathRowModel { key: "work".into(), name: "Work Path".into(), selected: false, temporary: true },
            ],
            ..Default::default()
        };
        let mut p = PathsPanel::new(&io);
        let b = rr(0.0, 0.0, 240.0, 300.0);
        lay(&mut p, b);
        fire(&mut p, b, &press(120.0, 8.0));
        let acts = actions(&io);
        assert!(acts.iter().any(|c| matches!(c, DockCmd::SelectPath(k) if k == "Path 1")), "row 0 selects Path 1: {acts:?}");
        io_lock(&io).selected_path = Some("Path 1".into());
        // The rightmost footer button is path.delete.
        let footer = rr(b.min_x(), b.max_y() - 34.0, b.max_x(), b.max_y());
        let del = footer_rects(footer, PATH_FOOTER.len(), 26.0)[0];
        fire(&mut p, b, &press(del.min_x() + 4.0, del.min_y() + 4.0));
        let acts = actions(&io);
        assert!(
            acts.iter().any(|c| matches!(c, DockCmd::Engine(id, v) if id == "path.delete" && v["name"] == json!("Path 1"))),
            "footer delete acts on the selected path: {acts:?}"
        );
    }

    #[test]
    fn work_path_double_click_renames_it() {
        let io = io();
        io_lock(&io).model = DockModel {
            has_doc: true,
            paths: vec![PathRowModel { key: "work".into(), name: "Work Path".into(), selected: false, temporary: true }],
            ..Default::default()
        };
        let mut p = PathsPanel::new(&io);
        let b = rr(0.0, 0.0, 240.0, 300.0);
        lay(&mut p, b);
        let ev = WidgetEvent::PointerPressed { position: Vec2::new(120.0, 8.0), button: PointerButton::Primary, count: 2 };
        fire(&mut p, b, &ev);
        let acts = actions(&io);
        assert!(
            acts.iter().any(|c| matches!(c, DockCmd::Engine(id, v) if id == "path.rename" && v["name"] == json!("work"))),
            "double-click saves the work path: {acts:?}"
        );
    }

    // ------------------------------------------------------------ history panel

    #[test]
    fn history_rows_jump_by_repeating_undo() {
        let io = io();
        io_lock(&io).model =
            DockModel { has_doc: true, history: vec!["Open".into(), "New Layer".into(), "Brush".into()], history_current: 2, ..Default::default() };
        let mut p = HistoryPanel::new(&io);
        let b = rr(0.0, 0.0, 240.0, 300.0);
        lay(&mut p, b);
        // Row 0 (y0 = min_y + 42 snapshot area) → two undos.
        fire(&mut p, b, &press(120.0, b.min_y() + 42.0 + 8.0));
        let acts = actions(&io);
        let undos = acts.iter().filter(|c| matches!(c, DockCmd::Engine(id, _) if id == "edit.undo")).count();
        assert_eq!(undos, 2, "jumping to the first entry repeats edit.undo: {acts:?}");
    }

    // ------------------------------------------------------------ navigator panel

    #[test]
    fn navigator_press_pans_to_the_doc_point() {
        let io = io();
        io_lock(&io).model = DockModel { has_doc: true, doc_w: 1000, doc_h: 800, ..Default::default() };
        let mut p = NavigatorPanel::new(&io);
        let b = rr(0.0, 0.0, 200.0, 200.0);
        lay(&mut p, b);
        fire(&mut p, b, &press(100.0, 100.0));
        let acts = actions(&io);
        assert!(acts.iter().any(|c| matches!(c, DockCmd::PanTo(x, y) if *x > 0.0 && *y > 0.0)), "the preview click pans the canvas: {acts:?}");
    }

    // ------------------------------------------------------------ command surface

    /// Every engine id a panel can emit must be a real registered command —
    /// the menu surface has the same guarantee in shell.rs tests.
    #[test]
    fn every_emitted_command_id_is_registered() {
        let specs: HashSet<&str> = photocraft_engine::command_specs().iter().map(|s| s.id).collect();
        let mut ids: Vec<&'static str> = adjustment_ids().iter().map(|(id, _)| *id).collect();
        ids.extend(PATH_FOOTER);
        ids.extend(CH_LEFT.iter().map(|(_, c)| *c));
        ids.extend(CH_RIGHT.iter().map(|(_, c)| *c));
        ids.extend([
            "layer.select",
            "layer.setProps",
            "layer.setExpanded",
            "layer.moveTo",
            "layer.linkLayers",
            "layer.layerStyle.blendingOptions",
            "layer.layerMask.revealAll",
            "layer.new.layer",
            "layer.new.group",
            "layer.delete",
            "layer.new.layerFromBackground",
            "channel.target",
            "channel.setVisible",
            "select.loadSelection",
            "select.saveSelection",
            "view.layerMask",
            "edit.undo",
            "edit.redo",
            "actions.play",
            "layerComp.apply",
            "path.rename",
            "gradient.presets.select",
            "pattern.presets.select",
            "swatches.use",
            "tools.setColors",
        ]);
        for id in ids {
            assert!(specs.contains(id), "{id} is not a registered engine command");
        }
    }
}
