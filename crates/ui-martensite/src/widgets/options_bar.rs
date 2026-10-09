//! Options bar widget adapting dynamically to the active tool.

use crate::widgets::scrubby_input::ScrubbyInputWidget;
use photocraft_engine::Tool;

pub struct OptionsBarWidget {
    pub active_tool: Tool,
    pub brush_size: ScrubbyInputWidget,
    pub brush_hardness: ScrubbyInputWidget,
    pub opacity: ScrubbyInputWidget,
    pub flow: ScrubbyInputWidget,
    pub smoothing: ScrubbyInputWidget,
    pub pressure_size: bool,
    pub pressure_opacity: bool,
}

impl OptionsBarWidget {
    pub fn new() -> Self {
        Self {
            active_tool: Tool::Brush,
            brush_size: ScrubbyInputWidget::new("Size", 30.0, 1.0, 5000.0, "px"),
            brush_hardness: ScrubbyInputWidget::new("Hardness", 100.0, 0.0, 100.0, "%"),
            opacity: ScrubbyInputWidget::new("Opacity", 100.0, 1.0, 100.0, "%"),
            flow: ScrubbyInputWidget::new("Flow", 100.0, 1.0, 100.0, "%"),
            smoothing: ScrubbyInputWidget::new("Smoothing", 10.0, 0.0, 100.0, "%"),
            pressure_size: false,
            pressure_opacity: false,
        }
    }

    pub fn set_tool(&mut self, tool: Tool) {
        self.active_tool = tool;
    }

    pub fn toggle_pressure_size(&mut self) -> bool {
        self.pressure_size = !self.pressure_size;
        self.pressure_size
    }

    pub fn toggle_pressure_opacity(&mut self) -> bool {
        self.pressure_opacity = !self.pressure_opacity;
        self.pressure_opacity
    }
}

impl Default for OptionsBarWidget {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_options_bar_defaults() {
        let mut bar = OptionsBarWidget::new();
        assert_eq!(bar.active_tool, Tool::Brush);
        assert_eq!(bar.brush_size.value, 30.0);
        assert_eq!(bar.opacity.value, 100.0);
        assert!(!bar.pressure_size);

        bar.set_tool(Tool::Eraser);
        assert_eq!(bar.active_tool, Tool::Eraser);

        assert!(bar.toggle_pressure_size());
        assert!(bar.pressure_size);
        assert!(!bar.toggle_pressure_size());
    }
}
