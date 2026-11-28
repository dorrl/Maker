use std::{fs, path::PathBuf};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Theme {
    #[default]
    Dark,
    Light,
    Graphite,
    Midnight,
    Paper,
}

impl Theme {
    pub const ALL: [Self; 5] = [
        Self::Dark,
        Self::Light,
        Self::Graphite,
        Self::Midnight,
        Self::Paper,
    ];

    pub const fn name(self) -> &'static str {
        match self {
            Self::Dark => "dark",
            Self::Light => "light",
            Self::Graphite => "graphite",
            Self::Midnight => "midnight",
            Self::Paper => "paper",
        }
    }

    pub const fn label_key(self) -> &'static str {
        match self {
            Self::Dark => "theme.dark",
            Self::Light => "theme.light",
            Self::Graphite => "theme.graphite",
            Self::Midnight => "theme.midnight",
            Self::Paper => "theme.paper",
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|theme| theme.name() == name)
    }

    pub const fn from_dark(dark: bool) -> Self {
        if dark { Self::Dark } else { Self::Light }
    }

    pub const fn dark(self) -> bool {
        matches!(self, Self::Dark | Self::Graphite | Self::Midnight)
    }

    fn bundled(self) -> &'static str {
        match self {
            Self::Dark => include_str!("themes/dark.json"),
            Self::Light => include_str!("themes/light.json"),
            Self::Graphite => include_str!("themes/graphite.json"),
            Self::Midnight => include_str!("themes/midnight.json"),
            Self::Paper => include_str!("themes/paper.json"),
        }
    }

    pub fn palette(self) -> Palette {
        let built_in: serde_json::Value =
            serde_json::from_str(self.bundled()).expect("valid built-in theme");
        let overrides = std::env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .map(|root| {
                root.join("Maker")
                    .join("themes")
                    .join(format!("{}.json", self.name()))
            })
            .and_then(|path| {
                (fs::metadata(&path).ok()?.len() <= 32 * 1024)
                    .then(|| fs::read_to_string(path).ok())
                    .flatten()
            })
            .and_then(|data| serde_json::from_str(&data).ok());
        Palette {
            built_in,
            overrides,
        }
    }
}

const SYNTAX: [&str; 15] = [
    "keyword",
    "comment",
    "string",
    "number",
    "markup",
    "function",
    "type",
    "property",
    "literal",
    "bracket_0",
    "bracket_1",
    "bracket_2",
    "unmatched_bracket",
    "diff_added",
    "diff_removed",
];

pub fn syntax_index(color: crate::text::Color) -> usize {
    use crate::text::Color;
    match color {
        Color::Keyword => 0,
        Color::Comment => 1,
        Color::String => 2,
        Color::Number => 3,
        Color::Markup => 4,
        Color::Function => 5,
        Color::Type => 6,
        Color::Property => 7,
        Color::Literal => 8,
        Color::Bracket(0) => 9,
        Color::Bracket(1) => 10,
        Color::Bracket(_) => 11,
        Color::UnmatchedBracket => 12,
        Color::DiffAdded => 13,
        Color::DiffRemoved => 14,
    }
}

pub struct Palette {
    built_in: serde_json::Value,
    overrides: Option<serde_json::Value>,
}

fn color(value: &serde_json::Value) -> Option<u32> {
    let [r, g, b] = value.as_array()?.as_slice() else {
        return None;
    };
    let r = u8::try_from(r.as_u64()?).ok()?;
    let g = u8::try_from(g.as_u64()?).ok()?;
    let b = u8::try_from(b.as_u64()?).ok()?;
    Some(u32::from_le_bytes([r, g, b, 0]))
}

impl Palette {
    pub fn get(&self, key: &str) -> u32 {
        self.overrides
            .as_ref()
            .and_then(|value| value.get(key))
            .and_then(color)
            .or_else(|| self.built_in.get(key).and_then(color))
            .expect("built-in color")
    }

    pub fn syntax(&self) -> [u32; 15] {
        SYNTAX.map(|key| {
            let diff_key = match key {
                "diff_added" => Some("added_text"),
                "diff_removed" => Some("removed_text"),
                _ => None,
            };
            if let Some(diff_key) = diff_key {
                return self
                    .override_color("diff", diff_key)
                    .or_else(|| self.override_color("syntax", key))
                    .unwrap_or_else(|| self.built_in_diff(diff_key));
            }
            self.override_color("syntax", key)
                .or_else(|| self.built_in["syntax"].get(key).and_then(color))
                .expect("built-in syntax color")
        })
    }

    pub fn diff_backgrounds(&self) -> [u32; 2] {
        ["added_background", "removed_background"].map(|key| {
            self.override_color("diff", key)
                .unwrap_or_else(|| self.built_in_diff(key))
        })
    }

    fn override_color(&self, section: &str, key: &str) -> Option<u32> {
        color(self.overrides.as_ref()?.get(section)?.get(key)?)
    }

    fn built_in_diff(&self, key: &str) -> u32 {
        color(&self.built_in["diff"][key]).expect("built-in diff color")
    }

    pub fn icons(&self) -> [u32; 6] {
        std::array::from_fn(|i| {
            self.overrides
                .as_ref()
                .and_then(|value| value.get("icons"))
                .and_then(|icons| icons.get(i))
                .and_then(color)
                .or_else(|| self.built_in["icons"].get(i).and_then(color))
                .expect("built-in icon color")
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bundled_palettes_cover_the_interface() {
        for theme in Theme::ALL {
            let palette = Palette {
                built_in: serde_json::from_str(theme.bundled()).unwrap(),
                overrides: None,
            };
            for name in [
                "background",
                "panel",
                "chrome",
                "menu",
                "control",
                "selected",
                "border",
                "accent",
                "thumb",
                "thumb_hover",
                "foreground",
                "muted",
            ] {
                assert!(palette.get(name) != 0, "{} {name}", theme.name());
            }
            assert_eq!(palette.icons().len(), 6);
            assert_eq!(palette.syntax().len(), 15);
            for background in palette.diff_backgrounds() {
                assert_ne!(background, palette.get("background"));
            }
        }
    }
}
