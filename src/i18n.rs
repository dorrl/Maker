use std::{
    collections::HashMap,
    sync::{
        LazyLock,
        atomic::{AtomicBool, Ordering},
    },
};

#[derive(Clone, Copy, Default, Eq, PartialEq)]
pub enum Locale {
    #[default]
    Korean,
    English,
}

impl Locale {
    pub fn code(self) -> &'static str {
        match self {
            Self::Korean => "ko",
            Self::English => "en",
        }
    }

    pub fn read(code: &str) -> Self {
        if code == "en" {
            Self::English
        } else {
            Self::Korean
        }
    }
}

static ENGLISH: AtomicBool = AtomicBool::new(false);
static KO: LazyLock<HashMap<String, String>> = LazyLock::new(|| {
    serde_json::from_str(include_str!("locales/ko.json")).expect("valid Korean catalog")
});
static EN: LazyLock<HashMap<String, String>> = LazyLock::new(|| {
    serde_json::from_str(include_str!("locales/en.json")).expect("valid English catalog")
});

pub fn select(locale: Locale) {
    ENGLISH.store(locale == Locale::English, Ordering::Relaxed);
}

pub fn text(key: &'static str) -> &'static str {
    let catalog = if ENGLISH.load(Ordering::Relaxed) {
        &*EN
    } else {
        &*KO
    };
    catalog
        .get(key)
        .or_else(|| KO.get(key))
        .map_or(key, String::as_str)
}

pub fn message(key: &'static str, args: &[String]) -> String {
    let mut template = text(key);
    let mut result = String::new();
    while let Some((before, rest)) = template.split_once('{') {
        result.push_str(before);
        if let Some((slot, after)) = rest.split_once('}')
            && let Some(value) = slot.parse::<usize>().ok().and_then(|index| args.get(index))
        {
            result.push_str(value);
            template = after;
        } else {
            result.push('{');
            template = rest;
        }
    }
    result.push_str(template);
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalogs_have_matching_keys_and_placeholder_sets() {
        fn slots(text: &str) -> Vec<usize> {
            let mut slots = text
                .split('{')
                .skip(1)
                .filter_map(|text| {
                    text.split_once('}')
                        .and_then(|(index, _)| index.parse().ok())
                })
                .collect::<Vec<_>>();
            slots.sort_unstable();
            slots.dedup();
            slots
        }
        assert_eq!(KO.len(), EN.len());
        for (key, ko) in &*KO {
            let en = EN
                .get(key)
                .unwrap_or_else(|| panic!("missing English text for {key}"));
            assert_eq!(slots(ko), slots(en), "placeholders for {key}");
        }
    }
}
