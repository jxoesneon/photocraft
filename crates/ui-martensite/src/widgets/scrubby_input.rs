//! Scrubby input widget: click-and-drag label to scrub numeric parameters.

#[derive(Clone, Debug, PartialEq)]
pub struct ScrubbyInputWidget {
    pub label: &'static str,
    pub value: f32,
    pub min: f32,
    pub max: f32,
    pub unit: &'static str,
    pub step: f32,
    pub is_dragging: bool,
    start_x: f32,
    start_val: f32,
}

impl ScrubbyInputWidget {
    pub fn new(label: &'static str, initial: f32, min: f32, max: f32, unit: &'static str) -> Self {
        Self { label, value: initial.clamp(min, max), min, max, unit, step: 1.0, is_dragging: false, start_x: 0.0, start_val: initial }
    }

    pub fn on_pointer_down(&mut self, x: f32) {
        self.is_dragging = true;
        self.start_x = x;
        self.start_val = self.value;
    }

    pub fn on_pointer_move(&mut self, x: f32, shift_held: bool, alt_held: bool) -> f32 {
        if !self.is_dragging {
            return self.value;
        }
        let delta = x - self.start_x;
        let mult = if shift_held {
            10.0
        } else if alt_held {
            0.1
        } else {
            1.0
        };
        let new_val = (self.start_val + delta * self.step * mult).clamp(self.min, self.max);
        self.value = (new_val * 100.0).round() / 100.0;
        self.value
    }

    pub fn on_pointer_up(&mut self) {
        self.is_dragging = false;
    }

    pub fn set_direct_value(&mut self, val: f32) {
        self.value = val.clamp(self.min, self.max);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scrubby_input_drag_and_clamp() {
        let mut widget = ScrubbyInputWidget::new("Opacity", 100.0, 0.0, 100.0, "%");
        assert_eq!(widget.value, 100.0);

        widget.on_pointer_down(50.0);
        assert!(widget.is_dragging);

        // Drag left by 20px with standard step
        widget.on_pointer_move(30.0, false, false);
        assert_eq!(widget.value, 80.0);

        // Drag right with shift (10x step)
        widget.on_pointer_move(60.0, true, false);
        assert_eq!(widget.value, 100.0); // Clamped to 100

        // Drag past min
        widget.on_pointer_move(-100.0, true, false);
        assert_eq!(widget.value, 0.0); // Clamped to 0

        widget.on_pointer_up();
        assert!(!widget.is_dragging);
    }

    #[test]
    fn test_fine_tune_alt_drag() {
        let mut widget = ScrubbyInputWidget::new("W", 50.0, 0.0, 200.0, "px");
        widget.on_pointer_down(0.0);
        // Drag 5px with Alt held (0.1x multiplier) -> 50.0 + 0.5 = 50.5
        widget.on_pointer_move(5.0, false, true);
        assert_eq!(widget.value, 50.5);
    }
}
