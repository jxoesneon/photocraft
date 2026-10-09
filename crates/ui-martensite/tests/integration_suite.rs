//! Comprehensive integration test suite for PhotoCraft Martensite UI.
//!
//! Validates end-to-end integration across commands, menus, shortcuts,
//! scrubby inputs, tonal blending, and canvas coordinates.

use photocraft_engine::{Engine, Tool};
use photocraft_ui_martensite::{
    PhotocraftApp,
    command_reg::find_command,
    menus::generate_main_menu,
    theme::CraftTheme,
    widgets::{DockPanelGroup, LayerItemDef, LayerTreeWidget, OptionsBarWidget, TonalBlendingWidget},
};

#[test]
fn test_end_to_end_workspace_interaction() {
    let engine = Engine::new();
    let mut app = PhotocraftApp::new(engine);

    // 1. Initial State Verification
    assert_eq!(app.active_tool, Tool::Move);
    assert_eq!(app.zoom_level, 1.0);
    assert!(app.rulers_visible);
    assert!(!app.quick_mask_active);

    // 2. Keystroke Workflow: Switch to Brush, Zoom in, Hold space to pan
    let new_tool = app.keyboard.on_key_down("b", app.active_tool);
    assert_eq!(new_tool, Some(Tool::Brush));
    app.set_tool(Tool::Brush);

    app.set_zoom(2.0);
    assert_eq!(app.zoom_level, 2.0);

    // Spring-loaded Hand tool
    let hand_tool = app.keyboard.on_key_down("Space", app.active_tool);
    assert_eq!(hand_tool, Some(Tool::Hand));
    app.set_tool(Tool::Hand);

    app.pan_by(50.0, 100.0);
    assert_eq!(app.pan_offset, [50.0, 100.0]);

    // Release Space restores Brush
    let restored_tool = app.keyboard.on_key_up("Space");
    assert_eq!(restored_tool, Some(Tool::Brush));
    app.set_tool(Tool::Brush);

    // 3. Options Bar Interaction for Active Brush
    let mut options = OptionsBarWidget::new();
    options.set_tool(Tool::Brush);
    options.brush_size.on_pointer_down(0.0);
    options.brush_size.on_pointer_move(20.0, false, false);
    options.brush_size.on_pointer_up();
    assert_eq!(options.brush_size.value, 50.0); // 30 + 20

    // 4. Layer Tree & Hierarchy Updates
    let mut layers = LayerTreeWidget::new();
    layers.layers.push(LayerItemDef {
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
    layers.layers.push(LayerItemDef {
        id: 2,
        name: "Artwork".to_string(),
        visible: true,
        locked: false,
        is_clipping: false,
        has_mask: true,
        mask_linked: true,
        is_group: false,
        expanded: false,
        children: vec![],
    });
    layers.select_layer(2);
    assert_eq!(layers.selected_layer_id, Some(2));
    layers.toggle_visibility(2);
    assert!(!layers.layers[1].visible);

    // 5. Tonal Blending ("Blend If") Split Feathering
    let mut tonal = TonalBlendingWidget::new();
    tonal.this_layer.set_min_split(10, 40);
    tonal.this_layer.set_max_split(210, 245);
    assert!(tonal.this_layer.is_split_min());
    assert!(tonal.this_layer.is_split_max());

    // 6. Docking System Validation
    let mut dock = DockPanelGroup::new(&["Layers", "History", "Properties"]);
    dock.select_tab(2);
    assert_eq!(dock.active_tab, 2);
    dock.toggle_collapsed();
    assert!(dock.collapsed_to_icons);

    // 7. Menu Generation Consistency
    let menus = generate_main_menu();
    assert!(!menus.is_empty());
    for menu in &menus {
        for item in &menu.items {
            if let Some(cmd_id) = item.command_id {
                assert!(find_command(cmd_id).is_some(), "Unknown command in menu: {}", cmd_id);
            }
        }
    }

    // 8. Theme Color Space Consistency
    let theme = CraftTheme::dark_neutral();
    let obsidian = CraftTheme::studio_obsidian();
    assert_ne!(theme.surface_app_bg, obsidian.surface_app_bg);
}
