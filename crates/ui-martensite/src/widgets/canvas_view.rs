//! Canvas view widget with subpixel pan/zoom, rulers, and selection outline.

pub struct CanvasViewWidget {
    pub zoom: f32,
    pub pan_offset: [f32; 2],
    pub canvas_size: [u32; 2],
    pub rulers_visible: bool,
    pub selection_phase: f32,
}

impl CanvasViewWidget {
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            zoom: 1.0,
            pan_offset: [0.0, 0.0],
            canvas_size: [width, height],
            rulers_visible: true,
            selection_phase: 0.0,
        }
    }

    pub fn zoom_at(&mut self, factor: f32, cursor: [f32; 2]) {
        let old_zoom = self.zoom;
        let new_zoom = (self.zoom * factor).clamp(0.01, 64.0);
        let ratio = new_zoom / old_zoom;

        self.pan_offset[0] = cursor[0] - (cursor[0] - self.pan_offset[0]) * ratio;
        self.pan_offset[1] = cursor[1] - (cursor[1] - self.pan_offset[1]) * ratio;
        self.zoom = new_zoom;
    }

    pub fn advance_selection_animation(&mut self, dt: f32) {
        self.selection_phase = (self.selection_phase + dt * 2.0) % 1.0;
    }

    pub fn screen_to_canvas(&self, screen: [f32; 2]) -> [f32; 2] {
        [
            (screen[0] - self.pan_offset[0]) / self.zoom,
            (screen[1] - self.pan_offset[1]) / self.zoom,
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_canvas_coordinates_and_zoom() {
        let mut canvas = CanvasViewWidget::new(1920, 1080);
        assert_eq!(canvas.screen_to_canvas([100.0, 100.0]), [100.0, 100.0]);

        canvas.zoom_at(2.0, [0.0, 0.0]);
        assert_eq!(canvas.zoom, 2.0);
        assert_eq!(canvas.screen_to_canvas([100.0, 100.0]), [50.0, 50.0]);

        canvas.advance_selection_animation(0.25);
        assert!((canvas.selection_phase - 0.5).abs() < 1e-4);
    }
}
