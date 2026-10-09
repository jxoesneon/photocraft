//! Menu bar items and accessible hierarchy generation.

use crate::command_reg::{CommandCategory, commands_by_category};

#[derive(Clone, Debug, PartialEq)]
pub struct MenuItemDef {
    pub label: String,
    pub command_id: Option<&'static str>,
    pub shortcut: Option<String>,
    pub is_separator: bool,
    pub children: Vec<MenuItemDef>,
}

impl MenuItemDef {
    pub fn action(label: impl Into<String>, command_id: &'static str, shortcut: Option<&str>) -> Self {
        Self { label: label.into(), command_id: Some(command_id), shortcut: shortcut.map(|s| s.to_string()), is_separator: false, children: Vec::new() }
    }

    pub fn separator() -> Self {
        Self { label: String::new(), command_id: None, shortcut: None, is_separator: true, children: Vec::new() }
    }
}

pub struct MenuCategoryDef {
    pub category: CommandCategory,
    pub title: &'static str,
    pub items: Vec<MenuItemDef>,
}

pub fn generate_main_menu() -> Vec<MenuCategoryDef> {
    vec![
        MenuCategoryDef { category: CommandCategory::File, title: "File", items: build_category_items(CommandCategory::File) },
        MenuCategoryDef { category: CommandCategory::Edit, title: "Edit", items: build_category_items(CommandCategory::Edit) },
        MenuCategoryDef { category: CommandCategory::Document, title: "Document", items: build_category_items(CommandCategory::Document) },
        MenuCategoryDef { category: CommandCategory::Layers, title: "Layers", items: build_category_items(CommandCategory::Layers) },
        MenuCategoryDef { category: CommandCategory::Selection, title: "Selection", items: build_category_items(CommandCategory::Selection) },
        MenuCategoryDef { category: CommandCategory::View, title: "View", items: build_category_items(CommandCategory::View) },
    ]
}

fn build_category_items(cat: CommandCategory) -> Vec<MenuItemDef> {
    commands_by_category(cat).into_iter().map(|cmd| MenuItemDef::action(cmd.label, cmd.id, cmd.default_shortcut)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_menu_generation_coverage() {
        let menus = generate_main_menu();
        assert_eq!(menus.len(), 6);
        for m in menus {
            assert!(!m.items.is_empty(), "Menu {} has no items", m.title);
            for it in m.items {
                if !it.is_separator {
                    assert!(it.command_id.is_some());
                    assert!(!it.label.is_empty());
                }
            }
        }
    }
}
