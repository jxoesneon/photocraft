//! The PhotoCraft icon overlay — app-private glyphs for tools the builtin
//! martensite pack doesn't carry, drawn by hand on the same 24px stroke
//! grid (`d` path data, stroke convention) so they sit seamlessly next to
//! the builtin family. Installed as the ambient [`IconSet`] on the UI
//! thread at launch; widgets resolving a namespaced icon name consult it
//! before the builtin pack.

use martensite::icons::{IconEntry, IconPack, IconSet};

/// `"photo.lasso"` — freeform loop with a dangling knot tail.
const LASSO: &str =
    "M12 4C7.9 4 4.5 6.4 4.5 9.5S7.9 15 12 15s7.5-2.4 7.5-5.5S16.1 4 12 4zM7.6 13.6C6.4 15.4 5 17.1 3.8 18.8c-.7 1-.1 2.3 1.1 2.2 2-.2 3.8-1.4 4.9-3.2";
/// `"photo.gradient"` — panel fading across vertical bands.
const GRADIENT: &str = "M4 5h16a1 1 0 011 1v12a1 1 0 01-1 1H4a1 1 0 01-1-1V6a1 1 0 011-1zM8 5v14M12 5v14M16 5v14";
/// `"photo.marquee"` — dashed selection rectangle.
const MARQUEE: &str =
    "M5 3H4a1 1 0 00-1 1v1M9 3h2M13 3h2M19 3h1a1 1 0 011 1v1M21 9v2M21 13v2M21 19v1a1 1 0 01-1 1h-1M15 21h-2M11 21H9M3 15v-2M3 11V9M3 20v-1a1 1 0 001 1h1";

/// PhotoCraft's overlay entries, in catalog order.
pub static ENTRIES: &[IconEntry] =
    &[IconEntry::new("photo.lasso", LASSO), IconEntry::new("photo.gradient", GRADIENT), IconEntry::new("photo.marquee", MARQUEE)];

/// The ambient set for the app: the PhotoCraft pack overlaying builtin.
pub fn ambient_set() -> IconSet {
    IconSet::new().with_pack(IconPack::new("photo", ENTRIES))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overlay_resolves_and_falls_through() {
        let set = ambient_set();
        assert!(set.resolve("photo.lasso").is_some());
        assert!(set.resolve("photo.gradient").is_some());
        // Untouched names still resolve to the builtin pack.
        assert!(set.resolve("edit.crop").is_some());
        assert!(set.resolve("misc.type").is_some());
        assert!(set.resolve("bogus.name").is_none());
    }
}
