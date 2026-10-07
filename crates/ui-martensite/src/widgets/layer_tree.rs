//! Layer hierarchy tree widget.

#[derive(Clone, Debug, PartialEq)]
pub struct LayerItemDef {
    pub id: u64,
    pub name: String,
    pub visible: bool,
    pub locked: bool,
    pub is_clipping: bool,
    pub has_mask: bool,
    pub mask_linked: bool,
    pub is_group: bool,
    pub expanded: bool,
    pub children: Vec<LayerItemDef>,
}

pub struct LayerTreeWidget {
    pub layers: Vec<LayerItemDef>,
    pub selected_layer_id: Option<u64>,
    pub opacity: f32,
    pub fill: f32,
    pub blend_mode: String,
}

impl LayerTreeWidget {
    pub fn new() -> Self {
        Self {
            layers: Vec::new(),
            selected_layer_id: None,
            opacity: 100.0,
            fill: 100.0,
            blend_mode: "Normal".to_string(),
        }
    }

    pub fn select_layer(&mut self, id: u64) {
        self.selected_layer_id = Some(id);
    }

    pub fn toggle_visibility(&mut self, id: u64) {
        if let Some(item) = find_layer_mut(&mut self.layers, id) {
            item.visible = !item.visible;
        }
    }

    pub fn toggle_lock(&mut self, id: u64) {
        if let Some(item) = find_layer_mut(&mut self.layers, id) {
            item.locked = !item.locked;
        }
    }
}

fn find_layer_mut(items: &mut [LayerItemDef], id: u64) -> Option<&mut LayerItemDef> {
    for item in items.iter_mut() {
        if item.id == id {
            return Some(item);
        }
        if let Some(found) = find_layer_mut(&mut item.children, id) {
            return Some(found);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_layer_tree_mutation() {
        let mut tree = LayerTreeWidget::new();
        tree.layers.push(LayerItemDef {
            id: 1,
            name: "Background".to_string(),
            visible: true,
            locked: true,
            is_clipping: false,
            has_mask: false,
            mask_linked: false,
            is_group: false,
            expanded: false,
            children: vec![],
        });
        tree.layers.push(LayerItemDef {
            id: 2,
            name: "Layer 1".to_string(),
            visible: true,
            locked: false,
            is_clipping: false,
            has_mask: true,
            mask_linked: true,
            is_group: false,
            expanded: false,
            children: vec![],
        });

        tree.select_layer(2);
        assert_eq!(tree.selected_layer_id, Some(2));

        tree.toggle_visibility(2);
        assert!(!tree.layers[1].visible);
        tree.toggle_visibility(2);
        assert!(tree.layers[1].visible);

        tree.toggle_lock(2);
        assert!(tree.layers[1].locked);
    }
}
