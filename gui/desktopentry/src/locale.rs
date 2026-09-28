//! The reader's language, as the specification matches it against `Key[xx]`.

/// A POSIX locale as a desktop entry matches it: `lang_COUNTRY@MODIFIER`, the
/// `.ENCODING` part dropped (the specification matches without it).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Locale {
    lang: String,
    country: Option<String>,
    modifier: Option<String>,
}

impl Locale {
    /// Parse `lang[_COUNTRY][.ENCODING][@MODIFIER]`.
    ///
    /// `None` for `C`, `POSIX` and anything that names no language: those
    /// mean "no translation", which is the unlocalized value.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        let (rest, modifier) = match text.split_once('@') {
            Some((rest, modifier)) => (rest, Some(modifier)),
            None => (text, None),
        };
        let rest = rest.split_once('.').map_or(rest, |(before, _)| before);
        let (lang, country) = match rest.split_once('_') {
            Some((lang, country)) => (lang, Some(country)),
            None => (rest, None),
        };
        let word =
            |s: &str| !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '-');
        if !word(lang) || lang == "C" || lang == "POSIX" {
            return None;
        }
        if country.is_some_and(|c| !word(c)) || modifier.is_some_and(|m| !word(m)) {
            return None;
        }
        Some(Self {
            lang: lang.to_owned(),
            country: country.map(str::to_owned),
            modifier: modifier.map(str::to_owned),
        })
    }

    /// The locale messages are shown in, from the environment as the C
    /// library reads it: `LC_ALL`, then `LC_MESSAGES`, then `LANG`, the first
    /// that is set and not empty.
    ///
    /// `get` is the environment -- `|name| std::env::var(name).ok()` in a
    /// program, a table in a test.
    #[must_use]
    pub fn from_env(get: impl Fn(&str) -> Option<String>) -> Option<Self> {
        ["LC_ALL", "LC_MESSAGES", "LANG"]
            .into_iter()
            .filter_map(&get)
            .find(|value| !value.is_empty())
            .and_then(|value| Self::parse(&value))
    }

    /// The locale suffixes to try, best first -- the specification's order:
    /// `lang_COUNTRY@MODIFIER`, `lang_COUNTRY`, `lang@MODIFIER`, `lang`. The
    /// unlocalized value, which comes last, is the caller's.
    #[must_use]
    pub fn candidates(&self) -> Vec<String> {
        let lang = &self.lang;
        let mut out = Vec::with_capacity(4);
        match (&self.country, &self.modifier) {
            (Some(country), Some(modifier)) => {
                out.push(format!("{lang}_{country}@{modifier}"));
                out.push(format!("{lang}_{country}"));
                out.push(format!("{lang}@{modifier}"));
            }
            (Some(country), None) => out.push(format!("{lang}_{country}")),
            (None, Some(modifier)) => out.push(format!("{lang}@{modifier}")),
            (None, None) => {}
        }
        out.push(lang.clone());
        out
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;

    /// The specification's four cases, each in its order.
    #[test]
    fn the_candidates_follow_the_specifications_order() {
        let c = |s: &str| Locale::parse(s).expect("a locale").candidates();
        assert_eq!(
            c("sr_RS.UTF-8@latin"),
            ["sr_RS@latin", "sr_RS", "sr@latin", "sr"]
        );
        assert_eq!(c("de_DE.UTF-8"), ["de_DE", "de"]);
        assert_eq!(c("sr@latin"), ["sr@latin", "sr"]);
        assert_eq!(c("fr"), ["fr"]);
    }

    /// `C` and `POSIX` are "untranslated", and nonsense is no locale.
    #[test]
    fn the_untranslated_locales_and_nonsense_are_none() {
        for text in ["C", "POSIX", "C.UTF-8", "", "_DE", "de_", "de@", "d e"] {
            assert_eq!(Locale::parse(text), None, "{text:?}");
        }
    }

    /// The C library's order: `LC_ALL`, `LC_MESSAGES`, `LANG`, skipping any
    /// that is set but empty.
    #[test]
    fn the_environment_is_read_in_the_c_librarys_order() {
        let env = |pairs: &'static [(&'static str, &'static str)]| {
            move |name: &str| {
                pairs
                    .iter()
                    .find(|(k, _)| *k == name)
                    .map(|(_, v)| (*v).to_owned())
            }
        };
        let lang = |l: Option<Locale>| l.expect("a locale").candidates().last().cloned();
        assert_eq!(
            lang(Locale::from_env(env(&[
                ("LANG", "fr_FR"),
                ("LC_MESSAGES", "de_DE")
            ]))),
            Some("de".to_owned())
        );
        assert_eq!(
            lang(Locale::from_env(env(&[
                ("LANG", "fr_FR"),
                ("LC_ALL", "it_IT")
            ]))),
            Some("it".to_owned())
        );
        assert_eq!(
            lang(Locale::from_env(env(&[("LANG", "fr_FR"), ("LC_ALL", "")]))),
            Some("fr".to_owned())
        );
        assert_eq!(Locale::from_env(env(&[])), None);
        assert_eq!(Locale::from_env(env(&[("LANG", "C.UTF-8")])), None);
    }
}
