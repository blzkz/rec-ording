//! Languages of the interface. The Spanish text is the key: `lz("Grabar")` gives it in the
//! language chosen in the settings (English unless changed), and `tr!` does the same for text
//! with values in it. The English text of every key is in `en.rs`; a test checks none is missing.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::OnceLock;

mod en;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    En,
    Es,
}

impl Lang {
    pub const ALL: [Lang; 2] = [Lang::En, Lang::Es];

    /// The language's own name.
    pub fn name(self) -> &'static str {
        match self {
            Lang::En => "English",
            Lang::Es => "Español",
        }
    }

    fn code(self) -> &'static str {
        match self {
            Lang::En => "en",
            Lang::Es => "es",
        }
    }

    fn from_code(c: &str) -> Option<Lang> {
        Lang::ALL.into_iter().find(|l| l.code() == c)
    }
}

static LANG: AtomicU8 = AtomicU8::new(0);

pub fn lang() -> Lang {
    if LANG.load(Ordering::Relaxed) == 1 {
        Lang::Es
    } else {
        Lang::En
    }
}

fn store(l: Lang) {
    LANG.store(if l == Lang::Es { 1 } else { 0 }, Ordering::Relaxed);
}

/// Takes the language saved in the settings (English if none).
pub fn init() {
    let saved = crate::update::setting("idioma").and_then(|v| v.as_str().and_then(Lang::from_code));
    store(saved.unwrap_or(Lang::En));
}

/// Uses `l` in this process without saving it (the MCP server always speaks English, which
/// language models handle best, whatever the app shows).
pub fn use_lang(l: Lang) {
    store(l);
}

/// Changes the language now and for the next times.
pub fn set(l: Lang) {
    store(l);
    crate::update::save_setting("idioma", serde_json::json!(l.code()));
}

fn english() -> &'static HashMap<&'static str, &'static str> {
    static MAP: OnceLock<HashMap<&'static str, &'static str>> = OnceLock::new();
    MAP.get_or_init(|| en::EN.iter().copied().collect())
}

/// `s` (Spanish) in the current language. Text without a translation stays as it is. When the
/// same Spanish word means different things, a context after `@` tells them apart in the key
/// (`"Salida@móvil"`); it is never shown.
pub fn lz<'a>(s: &'a str) -> &'a str {
    lz_in(lang(), s)
}

fn lz_in(l: Lang, s: &str) -> &str {
    let shown = s.split('@').next().unwrap_or(s);
    match l {
        Lang::Es => shown,
        Lang::En => english().get(s).copied().unwrap_or(shown),
    }
}

/// A translated template with its values: `{}` takes the next one, `{0}`, `{1}`… a given one.
pub fn fill(template: &str, args: &[String]) -> String {
    let mut out = String::with_capacity(template.len() + 16);
    let mut next = 0;
    let mut rest = template;
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open].replace("}}", "}"));
        let after = &rest[open + 1..];
        if let Some(stripped) = after.strip_prefix('{') {
            out.push('{');
            rest = stripped;
            continue;
        }
        let Some(close) = after.find('}') else {
            out.push_str(&rest[open..]);
            return out;
        };
        let inside = &after[..close];
        let index = if inside.is_empty() {
            next += 1;
            next - 1
        } else {
            inside.parse().unwrap_or(usize::MAX)
        };
        out.push_str(args.get(index).map(String::as_str).unwrap_or(""));
        rest = &after[close + 1..];
    }
    out.push_str(&rest.replace("}}", "}"));
    out
}

/// Decimal separator of the current language, for numbers shown to people:
/// `format!("{v:.1}").replace('.', comma())`.
pub fn comma() -> &'static str {
    match lang() {
        Lang::Es => ",",
        Lang::En => ".",
    }
}

/// `lz` for text with values: `tr!("De {} a {}", a, b)`.
#[macro_export]
macro_rules! tr {
    ($s:literal) => {
        $crate::i18n::lz($s).to_string()
    };
    ($s:literal, $($a:expr),+ $(,)?) => {
        $crate::i18n::fill($crate::i18n::lz($s), &[$(format!("{}", $a)),+])
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fills_templates() {
        assert_eq!(fill("De {} a {}", &["1".into(), "2".into()]), "De 1 a 2");
        assert_eq!(fill("{1} of {0}", &["a".into(), "b".into()]), "b of a");
        assert_eq!(fill("{{literal}} {}", &["x".into()]), "{literal} x");
        assert_eq!(lz_in(Lang::Es, "Salida@móvil"), "Salida");
        assert_eq!((lz_in(Lang::En, "Salida@móvil"), lz_in(Lang::En, "Salida")), ("Exit", "Out"));
    }

    /// Every text passed to `lz` or `tr!` in the code has its English version, with the same
    /// values in it.
    #[test]
    fn every_text_has_its_english() {
        let map = english();
        let mut missing = Vec::new();
        let mut dirs = vec![std::path::PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/src"))];
        let mut found = 0;
        while let Some(d) = dirs.pop() {
            for e in std::fs::read_dir(&d).unwrap().flatten() {
                let p = e.path();
                if p.is_dir() {
                    dirs.push(p);
                    continue;
                }
                if p.extension().is_none_or(|x| x != "rs") || p.ends_with("i18n/en.rs") || p.ends_with("i18n/mod.rs") {
                    continue;
                }
                let src = std::fs::read_to_string(&p).unwrap();
                for key in keys_in(&src) {
                    found += 1;
                    match map.get(key.as_str()) {
                        None => missing.push(format!("{}: {key:?}", p.display())),
                        Some(en) if en.matches('{').count() != key.matches('{').count() => missing.push(format!("{}: values differ in {key:?}", p.display())),
                        _ => {}
                    }
                }
            }
        }
        assert!(found > 300, "only {found} texts found");
        assert!(missing.is_empty(), "{} without English:\n{}", missing.len(), missing.join("\n"));
    }

    /// The literals in `lz("…")` and `tr!("…", …)` calls, unescaped.
    fn keys_in(src: &str) -> Vec<String> {
        let mut out = Vec::new();
        for start in ["lz(", "tr!("] {
            let mut rest = src;
            while let Some(i) = rest.find(start) {
                // Only whole names (not "glz("), and only literals, which may start on the next line.
                let before = rest[..i].chars().last();
                rest = rest[i + start.len()..].trim_start();
                if before.is_some_and(|c| c.is_alphanumeric() || c == '_') || !rest.starts_with('"') {
                    continue;
                }
                rest = &rest[1..];
                let mut s = String::new();
                let mut chars = rest.chars();
                while let Some(c) = chars.next() {
                    match c {
                        '"' => break,
                        '\\' => match chars.next() {
                            Some('n') => s.push('\n'),
                            Some(o) => s.push(o),
                            None => break,
                        },
                        c => s.push(c),
                    }
                }
                out.push(s);
            }
        }
        out
    }
}
