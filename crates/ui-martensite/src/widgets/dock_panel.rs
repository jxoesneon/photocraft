//! Retained-mode multi-tier docking panel group.

#[derive(Clone, Debug, PartialEq)]
pub struct DockPanelGroup {
    pub tabs: Vec<String>,
    pub active_tab: usize,
    pub collapsed_to_icons: bool,
    pub width: f32,
}

impl DockPanelGroup {
    pub fn new(tabs: &[&str]) -> Self {
        Self { tabs: tabs.iter().map(|s| s.to_string()).collect(), active_tab: 0, collapsed_to_icons: false, width: 280.0 }
    }

    pub fn select_tab(&mut self, idx: usize) {
        if idx < self.tabs.len() {
            self.active_tab = idx;
        }
    }

    pub fn toggle_collapsed(&mut self) -> bool {
        self.collapsed_to_icons = !self.collapsed_to_icons;
        self.collapsed_to_icons
    }

    pub fn resize(&mut self, new_width: f32) {
        self.width = new_width.clamp(180.0, 600.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dock_panel_group() {
        let mut dock = DockPanelGroup::new(&["Layers", "Channels", "Paths"]);
        assert_eq!(dock.active_tab, 0);
        assert_eq!(dock.tabs.len(), 3);

        dock.select_tab(1);
        assert_eq!(dock.active_tab, 1);

        assert!(dock.toggle_collapsed());
        assert!(dock.collapsed_to_icons);

        dock.resize(100.0);
        assert_eq!(dock.width, 180.0); // Clamped to min

        dock.resize(1000.0);
        assert_eq!(dock.width, 600.0); // Clamped to max
    }
}
