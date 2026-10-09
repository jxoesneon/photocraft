//! Keystroke state machine providing 100% Photoshop keyboard ergonomics.

use photocraft_engine::Tool;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct KeyModifiers {
    pub shift: bool,
    pub ctrl: bool,
    pub alt: bool,
    pub cmd: bool,
}

impl KeyModifiers {
    pub const fn empty() -> Self {
        Self { shift: false, ctrl: false, alt: false, cmd: false }
    }
}

pub struct KeyboardEngine {
    pub prior_tool: Option<Tool>,
    pub space_held: bool,
    pub z_held: bool,
    pub alt_held: bool,
}

impl KeyboardEngine {
    pub fn new() -> Self {
        Self { prior_tool: None, space_held: false, z_held: false, alt_held: false }
    }

    pub fn on_key_down(&mut self, key: &str, current: Tool) -> Option<Tool> {
        match key {
            "Space" if !self.space_held => {
                self.space_held = true;
                self.prior_tool = Some(current);
                Some(Tool::Hand)
            }
            "z" | "Z" if !self.z_held => {
                self.z_held = true;
                self.prior_tool = Some(current);
                Some(Tool::Zoom)
            }
            "Alt" => {
                self.alt_held = true;
                None
            }
            // Standard Photoshop single-key shortcuts
            "v" | "V" => Some(Tool::Move),
            "m" | "M" => Some(Tool::Marquee),
            "l" | "L" => Some(Tool::Lasso),
            "w" | "W" => Some(Tool::QuickSelection),
            "c" | "C" => Some(Tool::Crop),
            "b" | "B" => Some(Tool::Brush),
            "e" | "E" => Some(Tool::Eraser),
            "g" | "G" => Some(Tool::Gradient),
            "t" | "T" => Some(Tool::Type),
            "p" | "P" => Some(Tool::Pen),
            "h" | "H" => Some(Tool::Hand),
            _ => None,
        }
    }

    pub fn on_key_up(&mut self, key: &str) -> Option<Tool> {
        match key {
            "Space" if self.space_held => {
                self.space_held = false;
                self.prior_tool.take()
            }
            "z" | "Z" if self.z_held => {
                self.z_held = false;
                self.prior_tool.take()
            }
            "Alt" => {
                self.alt_held = false;
                None
            }
            _ => None,
        }
    }
}

impl Default for KeyboardEngine {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_single_key_tool_switching() {
        let mut k = KeyboardEngine::new();
        assert_eq!(k.on_key_down("v", Tool::Brush), Some(Tool::Move));
        assert_eq!(k.on_key_down("B", Tool::Move), Some(Tool::Brush));
        assert_eq!(k.on_key_down("e", Tool::Brush), Some(Tool::Eraser));
        assert_eq!(k.on_key_down("m", Tool::Eraser), Some(Tool::Marquee));
        assert_eq!(k.on_key_down("l", Tool::Marquee), Some(Tool::Lasso));
        assert_eq!(k.on_key_down("w", Tool::Lasso), Some(Tool::QuickSelection));
        assert_eq!(k.on_key_down("c", Tool::QuickSelection), Some(Tool::Crop));
        assert_eq!(k.on_key_down("g", Tool::Crop), Some(Tool::Gradient));
        assert_eq!(k.on_key_down("t", Tool::Gradient), Some(Tool::Type));
        assert_eq!(k.on_key_down("p", Tool::Type), Some(Tool::Pen));
    }

    #[test]
    fn test_spring_loaded_hand_tool() {
        let mut k = KeyboardEngine::new();
        let initial = Tool::Brush;

        // Press Space: temporary Hand
        assert_eq!(k.on_key_down("Space", initial), Some(Tool::Hand));
        assert!(k.space_held);

        // Multiple down events shouldn't overwrite prior tool
        assert_eq!(k.on_key_down("Space", Tool::Hand), None);

        // Release Space: restores initial tool
        assert_eq!(k.on_key_up("Space"), Some(initial));
        assert!(!k.space_held);
    }

    #[test]
    fn test_spring_loaded_zoom_tool() {
        let mut k = KeyboardEngine::new();
        let initial = Tool::Lasso;

        assert_eq!(k.on_key_down("z", initial), Some(Tool::Zoom));
        assert!(k.z_held);

        assert_eq!(k.on_key_up("z"), Some(initial));
        assert!(!k.z_held);
    }
}
