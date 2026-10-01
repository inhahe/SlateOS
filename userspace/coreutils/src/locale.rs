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
    hard_locale_with(category, |name| {
        std::env::var_os(name).map(|value| crate::quote::os_bytes(&value).into_owned())
    })
}

#[cfg(test)]
mod tests {
    use super::{Category, hard_locale_with, is_hard, selected_with};

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
