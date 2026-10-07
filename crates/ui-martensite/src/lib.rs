//! Sovereign retained-mode interface for PhotoCraft built on the Martensite GUI engine.
//!
//! Replaces immediate-mode egui with Martensite's 64-byte HotNode generational arena,
//! push-pull reactive signal DAG, and Vello GPU compute rasterization while preserving
//! 100% of professional raster editing muscle memory within strict legal boundaries.

pub mod command_reg;
pub mod menus;
pub mod shortcuts;
pub mod theme;
pub mod widgets;

use std::sync::{Arc, Mutex};
use photocraft_engine::Engine;

/// Application state container managing the Martensite GUI pipeline.
pub struct PhotocraftApp {
    pub engine: Arc<Mutex<Engine>>,
    pub theme: theme::CraftTheme,
    pub keyboard: shortcuts::KeyboardEngine,
    pub active_tool: photocraft_engine::Tool,
    pub zoom_level: f32,
    pub pan_offset: [f32; 2],
    pub rulers_visible: bool,
    pub quick_mask_active: bool,
    pub is_dirty: bool,
}

impl PhotocraftApp {
    pub fn new(engine: Engine) -> Self {
        Self {
            engine: Arc::new(Mutex::new(engine)),
            theme: theme::CraftTheme::dark_neutral(),
            keyboard: shortcuts::KeyboardEngine::new(),
            active_tool: photocraft_engine::Tool::Move,
            zoom_level: 1.0,
            pan_offset: [0.0, 0.0],
            rulers_visible: true,
            quick_mask_active: false,
            is_dirty: false,
        }
    }

    pub fn set_tool(&mut self, tool: photocraft_engine::Tool) {
        self.active_tool = tool;
    }

    pub fn set_zoom(&mut self, zoom: f32) {
        self.zoom_level = zoom.clamp(0.01, 64.0);
    }

    pub fn pan_by(&mut self, dx: f32, dy: f32) {
        self.pan_offset[0] += dx;
        self.pan_offset[1] += dy;
    }

    pub fn reset_view(&mut self) {
        self.zoom_level = 1.0;
        self.pan_offset = [0.0, 0.0];
    }

    pub fn toggle_rulers(&mut self) -> bool {
        self.rulers_visible = !self.rulers_visible;
        self.rulers_visible
    }

    pub fn toggle_quick_mask(&mut self) -> bool {
        self.quick_mask_active = !self.quick_mask_active;
        self.quick_mask_active
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_app_initialization() {
        let engine = Engine::new();
        let app = PhotocraftApp::new(engine);
        assert_eq!(app.active_tool, photocraft_engine::Tool::Move);
        assert_eq!(app.zoom_level, 1.0);
        assert_eq!(app.pan_offset, [0.0, 0.0]);
        assert!(app.rulers_visible);
        assert!(!app.quick_mask_active);
        assert!(!app.is_dirty);
    }

    #[test]
    fn test_zoom_clamping() {
        let engine = Engine::new();
        let mut app = PhotocraftApp::new(engine);
        
        app.set_zoom(2.5);
        assert_eq!(app.zoom_level, 2.5);

        app.set_zoom(0.0001);
        assert_eq!(app.zoom_level, 0.01);

        app.set_zoom(1000.0);
        assert_eq!(app.zoom_level, 64.0);
    }

    #[test]
    fn test_pan_and_reset() {
        let engine = Engine::new();
        let mut app = PhotocraftApp::new(engine);

        app.pan_by(120.0, -45.0);
        assert_eq!(app.pan_offset, [120.0, -45.0]);

        app.set_zoom(3.0);
        app.reset_view();
        assert_eq!(app.zoom_level, 1.0);
        assert_eq!(app.pan_offset, [0.0, 0.0]);
    }

    #[test]
    fn test_toggles() {
        let engine = Engine::new();
        let mut app = PhotocraftApp::new(engine);

        assert!(app.rulers_visible);
        assert!(!app.toggle_rulers());
        assert!(!app.rulers_visible);
        assert!(app.toggle_rulers());

        assert!(!app.quick_mask_active);
        assert!(app.toggle_quick_mask());
        assert!(app.quick_mask_active);
        assert!(!app.toggle_quick_mask());
    }
}
