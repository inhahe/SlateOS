#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use super::*;

/// Whether `text` holds a skin-tone modifier.
fn toned(text: &str) -> bool {
    text.chars().any(|c| SkinTone::ALL.iter().any(|t| t.modifier() == c))
}

/// **Every emoji is there once, untoned, in CLDR's order, with its name and
/// group** -- the grinning face first, the flags last.
#[test]
fn the_emoji_are_in_cldrs_order() {
    let all: Vec<Emoji> = emoji().collect();
    assert!(all.len() > 1800, "{}", all.len());
    assert_eq!(all[0].text, "\u{1F600}");
    assert_eq!(all[0].name, "grinning face");
    assert_eq!(groups()[all[0].group], "Smileys & Emotion");
    assert_eq!(all[0].subgroup, "face-smiling");
    let last = all.last().unwrap();
    assert_eq!(groups()[last.group], "Flags");
    assert!(emoji_in(0).all(|e| e.group == 0));
    assert!(emoji_in(0).count() > 100);
    assert!(all.iter().all(|e| !toned(e.text)), "no toned variant listed");
    // Nor any emoji twice.
    let texts: std::collections::HashSet<&str> = all.iter().map(|e| e.text).collect();
    assert_eq!(texts.len(), all.len());
}

/// **An emoji that comes in skin tones is drawn in the one asked for**, its
/// name the variant's; one that comes in none stays itself.
#[test]
fn an_emoji_is_drawn_in_a_skin_tone() {
    let wave = emoji().find(|e| e.name == "waving hand").unwrap().found();
    assert!(wave.has_tones());
    let medium = wave.in_tone(SkinTone::Medium);
    assert_eq!(medium.text, "\u{1F44B}\u{1F3FD}");
    assert_eq!(medium.name, "waving hand: medium skin tone");
    assert!(medium.has_tones(), "and can be toned again");
    assert_eq!(medium.in_tone(SkinTone::Dark).text, "\u{1F44B}\u{1F3FF}");
    for tone in SkinTone::ALL {
        let variant = wave.in_tone(tone);
        assert!(variant.text.ends_with(tone.modifier()), "{tone:?}");
        assert!(variant.name.contains(tone.name()), "{tone:?}");
    }
    let grin = emoji().next().unwrap().found();
    assert!(!grin.has_tones());
    assert_eq!(grin.in_tone(SkinTone::Dark), grin);
    let euro = characters_in("Currency").find(|f| f.text == "\u{20AC}").unwrap();
    assert!(!euro.has_tones());
    assert_eq!(euro.in_tone(SkinTone::Light), euro);
    // Many emoji come in tones, and every one of them in all five.
    let with_tones: Vec<Found> = emoji().map(Emoji::found).filter(Found::has_tones).collect();
    assert!(with_tones.len() > 300, "{}", with_tones.len());
    for e in with_tones {
        for tone in SkinTone::ALL {
            assert_ne!(e.in_tone(tone).text, e.text, "{} in {tone:?}", e.name);
        }
    }
}

/// **The categories besides emoji hold their characters**, each with its
/// Unicode name, by code point -- and nothing that cannot be seen alone.
#[test]
fn the_categories_hold_their_characters() {
    let names: Vec<&str> = categories().collect();
    assert_eq!(
        names,
        ["Symbols", "Math", "Arrows", "Currency", "Latin", "Greek", "Cyrillic"]
    );
    let arrows: Vec<Found> = characters_in("Arrows").collect();
    assert!(arrows.iter().any(|f| f.text == "\u{2192}" && f.name == "RIGHTWARDS ARROW"));
    let euro = characters_in("Currency").find(|f| f.text == "\u{20AC}").unwrap();
    assert_eq!(euro.name, "EURO SIGN");
    assert_eq!(characters_in("Nothing").count(), 0);
    // A line separator is in General Punctuation's block, and no picker
    // offers it.
    assert!(characters_in("Symbols").all(|f| f.text != "\u{2028}"));
    let latin: Vec<Found> = characters_in("Latin").collect();
    assert!(latin.windows(2).all(|w| w[0].text < w[1].text), "by code point");
}

/// **A character's name is its emoji name, else its Unicode name** -- an
/// emoji found with or without its presentation selector, and in a tone.
#[test]
fn a_characters_name_is_found() {
    assert_eq!(name_of('\u{E9}'), Some("LATIN SMALL LETTER E WITH ACUTE"));
    assert_eq!(name_of('\u{1F600}'), Some("grinning face"));
    assert_eq!(name_of('\u{263A}'), Some("smiling face"), "without U+FE0F");
    assert_eq!(name_of('a'), None, "not among a picker's");
    assert_eq!(
        name_of_text("\u{1F44B}\u{1F3FD}"),
        Some("waving hand: medium skin tone")
    );
    assert_eq!(name_of_text("\u{263A}\u{FE0F}"), Some("smiling face"));
    assert_eq!(name_of_text("ab"), None);
}

/// **A code point is read whatever its case, and only as a character that
/// can be inserted.**
#[test]
fn a_code_point_is_read() {
    assert_eq!(code_point("U+00E9"), Some('\u{E9}'));
    assert_eq!(code_point(" u+4e00 "), Some('\u{4E00}'), "named or not");
    assert_eq!(code_point("U+0007"), None, "a control character");
    assert_eq!(code_point("U+D800"), None, "a surrogate");
    assert_eq!(code_point("U+110000"), None, "past the last");
    assert_eq!(code_point("U+"), None);
    assert_eq!(code_point("U+12345678"), None);
    assert_eq!(code_point("U+-12"), None);
    assert_eq!(code_point("00E9"), None);
}

/// **A search finds by the starts of names' and keywords' words, in any order
/// and case; a code point or the character itself finds that one.**
#[test]
fn a_search_finds_by_words_code_points_and_characters() {
    let found = search("arrow right");
    assert!(found.iter().any(|f| f.text == "\u{2192}"), "RIGHTWARDS ARROW");
    assert!(search("RIGHT arrow").iter().any(|f| f.text == "\u{2192}"));
    assert!(search("shrug").iter().any(|f| f.name == "person shrugging"));
    let e = search("U+00E9");
    assert_eq!(e.len(), 1);
    assert_eq!(e[0].text, "\u{E9}");
    assert_eq!(e[0].name, "LATIN SMALL LETTER E WITH ACUTE");
    assert_eq!(search("u+1f600")[0].name, "grinning face");
    assert!(search("U+4E00").is_empty(), "no name for it here");
    assert_eq!(search("\u{20AC}")[0].name, "EURO SIGN", "the character itself");
    assert_eq!(
        search("\u{1F44B}\u{1F3FD}")[0].name,
        "waving hand: medium skin tone"
    );
    assert!(search("").is_empty());
    assert!(search("   ").is_empty());
    assert!(search("zzzzqqq").is_empty());
    assert!(search("?!").is_empty(), "no words");
    // An apostrophe is no part of a word, whichever was typed.
    assert!(search("one o'clock").iter().any(|f| f.name == "one o\u{2019}clock"));
}

/// **What a search finds comes best first**: a name that is the query, then
/// names that match it, then what only a keyword matched -- emoji before
/// characters within each, and a character that is an emoji too found once.
#[test]
fn a_search_ranks_what_it_finds() {
    let cat = search("cat");
    assert_eq!(cat[0].name, "cat", "the name that is the query");
    assert!(cat.iter().any(|f| f.name == "cat face"));
    // The dollar sign has "money" only as a keyword; the money bag has it in
    // its name.
    let money = search("money");
    let bag = money.iter().position(|f| f.name == "money bag").unwrap();
    let dollar = money.iter().position(|f| f.text == "$").unwrap();
    assert!(bag < dollar);
    // Every name match comes before every keyword-only one.
    for (query, found) in [("cat", &cat), ("money", &money)] {
        let by_name: Vec<bool> = found
            .iter()
            .map(|f| words(f.name).any(|w| starts_with(w, query)))
            .collect();
        let first_by_keyword = by_name.iter().position(|&n| !n).unwrap_or(by_name.len());
        assert!(
            by_name[first_by_keyword..].iter().all(|&n| !n),
            "{query}: a name match after a keyword-only one"
        );
    }
    assert!(money.iter().any(|f| !words(f.name).any(|w| starts_with(w, "money"))));
    assert_eq!(search("flag united states")[0].text, "\u{1F1FA}\u{1F1F8}");
    // The smiling face once, as the emoji.
    let smiling = search("smiling face")
        .iter()
        .filter(|f| f.text.trim_end_matches('\u{fe0f}') == "\u{263A}")
        .count();
    assert_eq!(smiling, 1);
    // A character found only by name, where the emoji is not found, is
    // offered as the character.
    assert!(search("white smiling").iter().any(|f| f.text == "\u{263A}"));
}
