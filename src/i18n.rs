//! Small, typed language selection shared by the UI and background worker.
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Language {
    #[default]
    Cs,
    En,
}
static ENGLISH: AtomicBool = AtomicBool::new(false);

pub fn set(language: Language) {
    ENGLISH.store(language == Language::En, Ordering::Relaxed);
}
pub fn english() -> bool {
    ENGLISH.load(Ordering::Relaxed)
}

#[macro_export]
macro_rules! tr {
    ($cs:literal, $en:literal) => {
        if $crate::i18n::english() { $en } else { $cs }
    };
}
#[macro_export]
macro_rules! tf {
    ($cs:literal, $en:literal $(, $args:expr)* $(,)?) => {
        if $crate::i18n::english() {
            format!($en $(, $args)*)
        } else {
            format!($cs $(, $args)*)
        }
    };
}
#[macro_export]
macro_rules! terr {
    ($($args:tt)*) => { anyhow::anyhow!($crate::tf!($($args)*)) };
}
#[macro_export]
macro_rules! tbail {
    ($($args:tt)*) => { return Err($crate::terr!($($args)*)) };
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn language_codes_roundtrip_and_default_preserves_czech() {
        assert_eq!(Language::default(), Language::Cs);
        for (language, code) in [(Language::Cs, "\"cs\""), (Language::En, "\"en\"")] {
            assert_eq!(serde_json::to_string(&language).unwrap(), code);
            assert_eq!(serde_json::from_str::<Language>(code).unwrap(), language);
        }
        assert!(serde_json::from_str::<Language>("\"unknown\"").is_err());
    }
}
