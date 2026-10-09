//! Tonal blending widget: Dual split-slider luminance thresholding ("Blend If").

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SplitSliderRange {
    pub min_start: u8,
    pub min_end: u8,
    pub max_start: u8,
    pub max_end: u8,
}

impl SplitSliderRange {
    pub const fn default_full() -> Self {
        Self { min_start: 0, min_end: 0, max_start: 255, max_end: 255 }
    }

    pub fn is_split_min(&self) -> bool {
        self.min_start != self.min_end
    }

    pub fn is_split_max(&self) -> bool {
        self.max_start != self.max_end
    }

    pub fn set_min_split(&mut self, start: u8, end: u8) {
        let s = start.min(end);
        let e = start.max(end);
        self.min_start = s;
        self.min_end = e.min(self.max_start);
    }

    pub fn set_max_split(&mut self, start: u8, end: u8) {
        let s = start.min(end);
        let e = start.max(end);
        self.max_start = s.max(self.min_end);
        self.max_end = e;
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TonalBlendingWidget {
    pub this_layer: SplitSliderRange,
    pub underlying_layer: SplitSliderRange,
}

impl TonalBlendingWidget {
    pub fn new() -> Self {
        Self { this_layer: SplitSliderRange::default_full(), underlying_layer: SplitSliderRange::default_full() }
    }

    pub fn reset(&mut self) {
        self.this_layer = SplitSliderRange::default_full();
        self.underlying_layer = SplitSliderRange::default_full();
    }
}

impl Default for TonalBlendingWidget {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tonal_blending_split_thresholds() {
        let mut widget = TonalBlendingWidget::new();
        assert_eq!(widget.this_layer.min_start, 0);
        assert_eq!(widget.this_layer.max_end, 255);
        assert!(!widget.this_layer.is_split_min());

        // Alt-drag split min thumb
        widget.this_layer.set_min_split(20, 50);
        assert_eq!(widget.this_layer.min_start, 20);
        assert_eq!(widget.this_layer.min_end, 50);
        assert!(widget.this_layer.is_split_min());

        // Alt-drag split max thumb
        widget.this_layer.set_max_split(200, 230);
        assert_eq!(widget.this_layer.max_start, 200);
        assert_eq!(widget.this_layer.max_end, 230);
        assert!(widget.this_layer.is_split_max());

        widget.reset();
        assert_eq!(widget.this_layer, SplitSliderRange::default_full());
    }
}
