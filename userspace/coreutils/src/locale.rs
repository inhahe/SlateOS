//! Which locale the C library would select for a category, as far as the
//! utilities need to know.
//!
//! GNU's utilities call `setlocale (LC_ALL, "")` first thing, and a few of
//! them later ask gnulib's `hard_locale (CATEGORY)` whether the locale that
//! selected for a category is anything but the portable one. The answer
//! changes output a script may be parsing: `ls` honours
//! `--time-style=posix-…` only in a hard `LC_TIME`, `pinky` writes its login
//! times as `%Y-%m-%d %H:%M` in one and `%b %e %H:%M` otherwise, and `cmp`
//! words the byte counter of its "differ" line by `LC_MESSAGES`.
//!
//! # The selection rule
//!
//! POSIX's, which glibc's `setlocale` follows: `LC_ALL` if it is set and not
//! empty, else the category's own variable (`LC_TIME`, `LC_MESSAGES`) under
//! the same condition, else `LANG` under the same condition, else the default
//! locale, which is `C`. An *empty* variable counts as unset.
//!
//! `ls` and `cmp` each carried a private copy of this lookup before it was
//! shared, and `ls`'s got that last rule wrong: it read `LC_ALL=` as naming
//! the `C` locale rather than falling through, so
//! `LC_ALL= LANG=C.UTF-8 ls -l --time-style=posix-iso` wrote dates in the
//! POSIX format where GNU writes ISO ones.
//!
//! # `hard_locale`
//!
//! True unless the selected name is exactly `C` or `POSIX` -- so it is *true*
//! for `C.UTF-8`, which is the trap: `C.UTF-8` is not `C`.
//!
//! # What this does not model
//!
//! A locale that is named but not installed. glibc's `setlocale (LC_ALL, "")`
//! fails as a whole when any category names a locale it cannot load, leaving
//! every category at `C`, so on a machine without `de_DE.UTF-8` a
//! `LANG=de_DE.UTF-8` makes no category hard. Which locales exist is the C
//! library's business, and a name is taken here at its word.

/// A locale category some utility asks about.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Category {
    /// `LC_CTYPE`: what a character is -- whether bytes or UTF-8 sequences.
    Ctype,
    /// `LC_MESSAGES`: the language diagnostics and prompts are written in.
    Messages,
    /// `LC_TIME`: how dates and times are written.
    Time,
}

impl Category {
    /// The variable that names this category's locale on its own.
    #[must_use]
    pub const fn variable(self) -> &'static str {
        match self {
            Category::Ctype => "LC_CTYPE",
            Category::Messages => "LC_MESSAGES",
            Category::Time => "LC_TIME",
        }
    }
}

/// The name of the locale selected for `category`, or `None` when no
/// variable names one, which is the default `C` locale.
///
/// `var` returns a variable's value, `None` when it is unset. An empty value
/// counts as unset here, so a caller that captures its environment verbatim
/// need not filter it first.
#[must_use]
pub fn selected_with(category: Category, var: impl Fn(&str) -> Option<Vec<u8>>) -> Option<Vec<u8>> {
    ["LC_ALL", category.variable(), "LANG"]
        .into_iter()
        .find_map(|name| var(name).filter(|value| !value.is_empty()))
}

/// gnulib's `hard_locale` for the locale named `name`: false for exactly `C`
/// and `POSIX`, and for no name at all, since the default locale is `C`.
#[must_use]
pub fn is_hard(name: Option<&[u8]>) -> bool {
    !matches!(name, None | Some(b"C" | b"POSIX"))
}

/// gnulib's `hard_locale (CATEGORY)`, with the variables read through `var`:
/// for a caller that captures its environment once, as `ls` does.
#[must_use]
pub fn hard_locale_with(category: Category, var: impl Fn(&str) -> Option<Vec<u8>>) -> bool {
    is_hard(selected_with(category, var).as_deref())
}

/// gnulib's `hard_locale (CATEGORY)` for this process's environment.
#[must_use]
pub fn hard_locale(category: Category) -> bool {
    hard_locale_with(category, env_var)
}

/// Whether the locale named `name` reads text as UTF-8: its codeset, after
/// the `.`, is `UTF-8` or `utf8` in any case -- `C.UTF-8`, `en_US.utf8`.
/// `C`, `POSIX` and no name at all read bytes.
#[must_use]
pub fn is_utf8(name: Option<&[u8]>) -> bool {
    name.and_then(|n| n.iter().rposition(|&b| b == b'.').and_then(|dot| n.get(dot + 1..)))
        .map(|codeset| codeset.split(|&b| b == b'@').next().unwrap_or(codeset))
        .is_some_and(|c| c.eq_ignore_ascii_case(b"UTF-8") || c.eq_ignore_ascii_case(b"utf8"))
}

/// Whether this process's `LC_CTYPE` reads text as UTF-8 -- which decides,
/// for a utility handed a regular expression, whether `.` is a byte or a
/// character.
#[must_use]
pub fn ctype_is_utf8() -> bool {
    is_utf8(selected_with(Category::Ctype, env_var).as_deref())
}

/// A variable of this process's environment, as bytes.
fn env_var(name: &str) -> Option<Vec<u8>> {
    std::env::var_os(name).map(|value| crate::quote::os_bytes(&value).into_owned())
}

#[cfg(test)]
mod tests {
    use super::{Category, hard_locale_with, is_hard, is_utf8, selected_with};

    #[test]
    fn a_utf8_codeset_is_recognised_in_either_spelling() {
        for name in ["C.UTF-8", "en_US.UTF-8", "en_US.utf8", "de_DE.utf-8", "sr_RS.UTF-8@latin"] {
            assert!(is_utf8(Some(name.as_bytes())), "{name}");
        }
        for name in ["C", "POSIX", "en_US", "en_US.ISO-8859-1", "UTF-8", ""] {
            assert!(!is_utf8(Some(name.as_bytes())), "{name}");
        }
        assert!(!is_utf8(None));
        let vars = [("LC_CTYPE", "C"), ("LANG", "C.UTF-8")];
        let name = selected_with(Category::Ctype, env(&vars));
        assert!(!is_utf8(name.as_deref()));
    }

    /// An environment holding exactly `pairs`.
    fn env<'a>(pairs: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<Vec<u8>> + 'a {
        move |name| {
            pairs
                .iter()
                .find(|(key, _)| *key == name)
                .map(|(_, value)| value.as_bytes().to_vec())
        }
    }

    #[test]
    fn nothing_set_is_the_c_locale() {
        assert_eq!(selected_with(Category::Time, env(&[])), None);
        assert!(!hard_locale_with(Category::Time, env(&[])));
    }

    #[test]
    fn lc_all_outranks_the_category_and_lang() {
        let vars = [("LC_ALL", "C"), ("LC_TIME", "C.UTF-8"), ("LANG", "C.UTF-8")];
        assert_eq!(
            selected_with(Category::Time, env(&vars)),
            Some(b"C".to_vec())
        );
        assert!(!hard_locale_with(Category::Time, env(&vars)));
    }

    #[test]
    fn the_category_outranks_lang_and_only_its_own_variable_counts() {
        let vars = [("LC_TIME", "POSIX"), ("LANG", "C.UTF-8")];
        assert!(!hard_locale_with(Category::Time, env(&vars)));
        // `LC_TIME` says nothing about messages, which fall through to `LANG`.
        assert!(hard_locale_with(Category::Messages, env(&vars)));
    }

    /// The rule `ls` had wrong: `LC_ALL=` is unset, not a name for `C`.
    #[test]
    fn an_empty_variable_counts_as_unset() {
        let vars = [("LC_ALL", ""), ("LC_TIME", ""), ("LANG", "C.UTF-8")];
        assert_eq!(
            selected_with(Category::Time, env(&vars)),
            Some(b"C.UTF-8".to_vec())
        );
        assert!(hard_locale_with(Category::Time, env(&vars)));
        let all_empty = [("LC_ALL", ""), ("LC_TIME", ""), ("LANG", "")];
        assert_eq!(selected_with(Category::Time, env(&all_empty)), None);
    }

    #[test]
    fn only_exactly_c_and_posix_are_soft() {
        assert!(!is_hard(None));
        assert!(!is_hard(Some(b"C")));
        assert!(!is_hard(Some(b"POSIX")));
        // `C.UTF-8` is not `C`.
        assert!(is_hard(Some(b"C.UTF-8")));
        assert!(is_hard(Some(b"en_US.UTF-8")));
        assert!(is_hard(Some(b"POSIX.UTF-8")));
    }
}
