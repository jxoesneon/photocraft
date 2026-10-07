//! Tool strip widget: single/double column layout, tool flyouts, and color chips.

use photocraft_engine::Tool;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ToolSlot {
    pub primary: Tool,
    pub alternatives: &'static [Tool],
}

pub const TOOL_SLOTS: &[ToolSlot] = &[
    ToolSlot { primary: Tool::Move, alternatives: &[] },
    ToolSlot { primary: Tool::Marquee, alternatives: &[] },
    ToolSlot { primary: Tool::Lasso, alternatives: &[] },
    ToolSlot { primary: Tool::QuickSelection, alternatives: &[] },
    ToolSlot { primary: Tool::Crop, alternatives: &[] },
    ToolSlot { primary: Tool::Brush, alternatives: &[] },
    ToolSlot { primary: Tool::Eraser, alternatives: &[] },
    ToolSlot { primary: Tool::Gradient, alternatives: &[] },
    ToolSlot { primary: Tool::Type, alternatives: &[] },
    ToolSlot { primary: Tool::Pen, alternatives: &[] },
    ToolSlot { primary: Tool::Hand, alternatives: &[] },
    ToolSlot { primary: Tool::Zoom, alternatives: &[] },
];

pub struct ToolStripWidget {
    pub active_tool: Tool,
    pub double_column: bool,
    pub foreground_color: [u8; 4],
    pub background_color: [u8; 4],
    pub quick_mask: bool,
}

impl ToolStripWidget {
    pub fn new() -> Self {
        Self {
            active_tool: Tool::Move,
            double_column: false,
            foreground_color: [0, 0, 0, 255],       // Default black
            background_color: [255, 255, 255, 255], // Default white
            quick_mask: false,
        }
    }

    pub fn toggle_column_mode(&mut self) -> bool {
        self.double_column = !self.double_column;
        self.double_column
    }

    pub fn swap_colors(&mut self) {
        std::mem::swap(&mut self.foreground_color, &mut self.background_color);
    }

    pub fn reset_default_colors(&mut self) {
        self.foreground_color = [0, 0, 0, 255];
        self.background_color = [255, 255, 255, 255];
    }

    pub fn toggle_quick_mask(&mut self) -> bool {
        self.quick_mask = !self.quick_mask;
        self.quick_mask
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tool_strip_state() {
        let mut strip = ToolStripWidget::new();
        assert_eq!(strip.active_tool, Tool::Move);
        assert!(!strip.double_column);
        assert_eq!(strip.foreground_color, [0, 0, 0, 255]);
        assert_eq!(strip.background_color, [255, 255, 255, 255]);

        strip.swap_colors();
        assert_eq!(strip.foreground_color, [255, 255, 255, 255]);
        assert_eq!(strip.background_color, [0, 0, 0, 255]);

        strip.reset_default_colors();
        assert_eq!(strip.foreground_color, [0, 0, 0, 255]);

        assert!(strip.toggle_column_mode());
        assert!(strip.double_column);
        assert!(!strip.toggle_column_mode());
    }
}
