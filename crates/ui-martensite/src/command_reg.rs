//! Decoupled command catalog and taxonomy for PhotoCraft.

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CommandCategory {
    File,
    Edit,
    Document,
    Layers,
    Selection,
    Effects,
    View,
    Window,
    Help,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CommandSpec {
    pub id: &'static str,
    pub label: &'static str,
    pub category: CommandCategory,
    pub default_shortcut: Option<&'static str>,
    pub secondary_shortcut: Option<&'static str>,
}

pub const COMMAND_REGISTRY: &[CommandSpec] = &[
    // File
    CommandSpec { id: "file.new", label: "New…", category: CommandCategory::File, default_shortcut: Some("Cmd+N"), secondary_shortcut: None },
    CommandSpec { id: "file.open", label: "Open…", category: CommandCategory::File, default_shortcut: Some("Cmd+O"), secondary_shortcut: None },
    CommandSpec { id: "file.save", label: "Save", category: CommandCategory::File, default_shortcut: Some("Cmd+S"), secondary_shortcut: None },
    CommandSpec { id: "file.save_as", label: "Save As…", category: CommandCategory::File, default_shortcut: Some("Shift+Cmd+S"), secondary_shortcut: None },
    CommandSpec {
        id: "file.export",
        label: "Export Surface…",
        category: CommandCategory::File,
        default_shortcut: Some("Alt+Shift+Cmd+W"),
        secondary_shortcut: None,
    },
    // Edit
    CommandSpec { id: "edit.undo", label: "Undo", category: CommandCategory::Edit, default_shortcut: Some("Cmd+Z"), secondary_shortcut: None },
    CommandSpec { id: "edit.redo", label: "Redo", category: CommandCategory::Edit, default_shortcut: Some("Shift+Cmd+Z"), secondary_shortcut: None },
    CommandSpec { id: "edit.transform", label: "Free Transform", category: CommandCategory::Edit, default_shortcut: Some("Cmd+T"), secondary_shortcut: None },
    CommandSpec {
        id: "edit.fill",
        label: "Fill Surface…",
        category: CommandCategory::Edit,
        default_shortcut: Some("Shift+F5"),
        secondary_shortcut: Some("Shift+Backspace"),
    },
    // Document
    CommandSpec {
        id: "doc.dimensions",
        label: "Document Dimensions…",
        category: CommandCategory::Document,
        default_shortcut: Some("Alt+Cmd+I"),
        secondary_shortcut: None,
    },
    CommandSpec {
        id: "doc.canvas_bounds",
        label: "Canvas Bounds…",
        category: CommandCategory::Document,
        default_shortcut: Some("Alt+Cmd+C"),
        secondary_shortcut: None,
    },
    // Layers
    CommandSpec { id: "layer.new", label: "New Layer…", category: CommandCategory::Layers, default_shortcut: Some("Shift+Cmd+N"), secondary_shortcut: None },
    CommandSpec {
        id: "layer.duplicate",
        label: "Duplicate Layer…",
        category: CommandCategory::Layers,
        default_shortcut: Some("Cmd+J"),
        secondary_shortcut: None,
    },
    CommandSpec {
        id: "layer.clip",
        label: "Create Clipping Mask",
        category: CommandCategory::Layers,
        default_shortcut: Some("Alt+Cmd+G"),
        secondary_shortcut: None,
    },
    CommandSpec { id: "layer.group", label: "Group Layers", category: CommandCategory::Layers, default_shortcut: Some("Cmd+G"), secondary_shortcut: None },
    // Selection
    CommandSpec { id: "select.all", label: "All", category: CommandCategory::Selection, default_shortcut: Some("Cmd+A"), secondary_shortcut: None },
    CommandSpec { id: "select.deselect", label: "Deselect", category: CommandCategory::Selection, default_shortcut: Some("Cmd+D"), secondary_shortcut: None },
    CommandSpec {
        id: "select.inverse",
        label: "Inverse",
        category: CommandCategory::Selection,
        default_shortcut: Some("Shift+Cmd+I"),
        secondary_shortcut: None,
    },
    // View
    CommandSpec { id: "view.fit", label: "Fit on Screen", category: CommandCategory::View, default_shortcut: Some("Cmd+0"), secondary_shortcut: None },
    CommandSpec { id: "view.actual", label: "100%", category: CommandCategory::View, default_shortcut: Some("Cmd+1"), secondary_shortcut: None },
    CommandSpec { id: "view.rulers", label: "Rulers", category: CommandCategory::View, default_shortcut: Some("Cmd+R"), secondary_shortcut: None },
];

pub fn find_command(id: &str) -> Option<&'static CommandSpec> {
    COMMAND_REGISTRY.iter().find(|cmd| cmd.id == id)
}

pub fn commands_by_category(category: CommandCategory) -> Vec<&'static CommandSpec> {
    COMMAND_REGISTRY.iter().filter(|cmd| cmd.category == category).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn test_command_ids_are_unique() {
        let mut ids = HashSet::new();
        for cmd in COMMAND_REGISTRY {
            assert!(ids.insert(cmd.id), "Duplicate command ID detected: {}", cmd.id);
        }
    }

    #[test]
    fn test_lookup_finds_all_commands() {
        for cmd in COMMAND_REGISTRY {
            let found = find_command(cmd.id);
            assert!(found.is_some());
            assert_eq!(found.unwrap().label, cmd.label);
        }
    }

    #[test]
    fn test_categories_populated() {
        assert!(!commands_by_category(CommandCategory::File).is_empty());
        assert!(!commands_by_category(CommandCategory::Edit).is_empty());
        assert!(!commands_by_category(CommandCategory::Layers).is_empty());
        assert!(!commands_by_category(CommandCategory::Selection).is_empty());
        assert!(!commands_by_category(CommandCategory::View).is_empty());
    }
}
