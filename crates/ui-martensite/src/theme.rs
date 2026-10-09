//! Martensite design system tokens calibrated for color-neutral digital raster workflows.

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RgbaColor {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl RgbaColor {
    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b, a: 255 }
    }

    pub const fn rgba(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }

    pub fn to_f32_array(self) -> [f32; 4] {
        [self.r as f32 / 255.0, self.g as f32 / 255.0, self.b as f32 / 255.0, self.a as f32 / 255.0]
    }

    pub fn luminance(&self) -> f32 {
        0.2126 * (self.r as f32 / 255.0) + 0.7152 * (self.g as f32 / 255.0) + 0.0722 * (self.b as f32 / 255.0)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CraftTheme {
    pub surface_app_bg: RgbaColor,
    pub surface_panel: RgbaColor,
    pub surface_canvas_bg: RgbaColor,
    pub surface_input: RgbaColor,
    pub surface_active_tab: RgbaColor,
    pub surface_inactive_tab: RgbaColor,
    pub border_divider: RgbaColor,
    pub text_primary: RgbaColor,
    pub text_dimmed: RgbaColor,
    pub text_scrubby: RgbaColor,
    pub accent_active: RgbaColor,
    pub accent_focus: RgbaColor,
    pub corner_radius: f32,
    pub widget_spacing: f32,
}

impl CraftTheme {
    pub fn dark_neutral() -> Self {
        Self {
            surface_app_bg: RgbaColor::rgb(50, 50, 50),
            surface_panel: RgbaColor::rgb(38, 38, 38),
            surface_canvas_bg: RgbaColor::rgb(30, 30, 30),
            surface_input: RgbaColor::rgb(24, 24, 24),
            surface_active_tab: RgbaColor::rgb(38, 38, 38),
            surface_inactive_tab: RgbaColor::rgb(56, 56, 56),
            border_divider: RgbaColor::rgb(26, 26, 26),
            text_primary: RgbaColor::rgb(223, 223, 223),
            text_dimmed: RgbaColor::rgb(158, 158, 158),
            text_scrubby: RgbaColor::rgb(181, 181, 181),
            accent_active: RgbaColor::rgb(0, 164, 228),
            accent_focus: RgbaColor::rgb(62, 142, 208),
            corner_radius: 4.0,
            widget_spacing: 6.0,
        }
    }

    pub fn studio_obsidian() -> Self {
        Self {
            surface_app_bg: RgbaColor::rgb(18, 21, 27),
            surface_panel: RgbaColor::rgb(22, 26, 34),
            surface_canvas_bg: RgbaColor::rgb(14, 17, 22),
            surface_input: RgbaColor::rgb(10, 12, 16),
            surface_active_tab: RgbaColor::rgb(22, 26, 34),
            surface_inactive_tab: RgbaColor::rgb(33, 39, 52),
            border_divider: RgbaColor::rgb(42, 50, 65),
            text_primary: RgbaColor::rgb(240, 246, 252),
            text_dimmed: RgbaColor::rgb(139, 148, 158),
            text_scrubby: RgbaColor::rgb(170, 180, 195),
            accent_active: RgbaColor::rgb(0, 229, 255),
            accent_focus: RgbaColor::rgb(124, 58, 237),
            corner_radius: 6.0,
            widget_spacing: 8.0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_color_conversions() {
        let c = RgbaColor::rgba(255, 128, 0, 255);
        let f = c.to_f32_array();
        assert_eq!(f[0], 1.0);
        assert!((f[1] - 0.50196).abs() < 1e-4);
        assert_eq!(f[2], 0.0);
        assert_eq!(f[3], 1.0);
    }

    #[test]
    fn test_luminance_calculation() {
        let black = RgbaColor::rgb(0, 0, 0);
        let white = RgbaColor::rgb(255, 255, 255);
        assert_eq!(black.luminance(), 0.0);
        assert!((white.luminance() - 1.0).abs() < 1e-4);
    }

    #[test]
    fn test_theme_contrast_sanity() {
        let theme = CraftTheme::dark_neutral();
        assert!(theme.text_primary.luminance() > theme.surface_panel.luminance() + 0.4);
        assert!(theme.surface_panel.luminance() > theme.surface_canvas_bg.luminance());
    }
}
