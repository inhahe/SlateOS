//! The colour names SubRip's `<font color=...>` may use: the 140 ffmpeg
//! knows, with ffmpeg's values.
//!
//! Taken from ffmpeg's own answers (`tests/data/subrip_colours.srt`, every
//! CSS colour name asked about in turn), not from a CSS table: ffmpeg's is
//! older. It spells `lightgrey` but not `lightgray`, has no `grey` spellings
//! and no `rebeccapurple`, and keeps two values the W3C later corrected
//! (`mediumpurple` `#9370d8`, `palevioletred` `#d87093`). Its `random`, a
//! different colour every time, is not here.

/// Name and `0xRRGGBB`, sorted by name for a binary search.
const COLOURS: &[(&[u8], u32)] = &[
    (b"aliceblue", 0xF0F8FF),
    (b"antiquewhite", 0xFAEBD7),
    (b"aqua", 0x00FFFF),
    (b"aquamarine", 0x7FFFD4),
    (b"azure", 0xF0FFFF),
    (b"beige", 0xF5F5DC),
    (b"bisque", 0xFFE4C4),
    (b"black", 0x000000),
    (b"blanchedalmond", 0xFFEBCD),
    (b"blue", 0x0000FF),
    (b"blueviolet", 0x8A2BE2),
    (b"brown", 0xA52A2A),
    (b"burlywood", 0xDEB887),
    (b"cadetblue", 0x5F9EA0),
    (b"chartreuse", 0x7FFF00),
    (b"chocolate", 0xD2691E),
    (b"coral", 0xFF7F50),
    (b"cornflowerblue", 0x6495ED),
    (b"cornsilk", 0xFFF8DC),
    (b"crimson", 0xDC143C),
    (b"cyan", 0x00FFFF),
    (b"darkblue", 0x00008B),
    (b"darkcyan", 0x008B8B),
    (b"darkgoldenrod", 0xB8860B),
    (b"darkgray", 0xA9A9A9),
    (b"darkgreen", 0x006400),
    (b"darkkhaki", 0xBDB76B),
    (b"darkmagenta", 0x8B008B),
    (b"darkolivegreen", 0x556B2F),
    (b"darkorange", 0xFF8C00),
    (b"darkorchid", 0x9932CC),
    (b"darkred", 0x8B0000),
    (b"darksalmon", 0xE9967A),
    (b"darkseagreen", 0x8FBC8F),
    (b"darkslateblue", 0x483D8B),
    (b"darkslategray", 0x2F4F4F),
    (b"darkturquoise", 0x00CED1),
    (b"darkviolet", 0x9400D3),
    (b"deeppink", 0xFF1493),
    (b"deepskyblue", 0x00BFFF),
    (b"dimgray", 0x696969),
    (b"dodgerblue", 0x1E90FF),
    (b"firebrick", 0xB22222),
    (b"floralwhite", 0xFFFAF0),
    (b"forestgreen", 0x228B22),
    (b"fuchsia", 0xFF00FF),
    (b"gainsboro", 0xDCDCDC),
    (b"ghostwhite", 0xF8F8FF),
    (b"gold", 0xFFD700),
    (b"goldenrod", 0xDAA520),
    (b"gray", 0x808080),
    (b"green", 0x008000),
    (b"greenyellow", 0xADFF2F),
    (b"honeydew", 0xF0FFF0),
    (b"hotpink", 0xFF69B4),
    (b"indianred", 0xCD5C5C),
    (b"indigo", 0x4B0082),
    (b"ivory", 0xFFFFF0),
    (b"khaki", 0xF0E68C),
    (b"lavender", 0xE6E6FA),
    (b"lavenderblush", 0xFFF0F5),
    (b"lawngreen", 0x7CFC00),
    (b"lemonchiffon", 0xFFFACD),
    (b"lightblue", 0xADD8E6),
    (b"lightcoral", 0xF08080),
    (b"lightcyan", 0xE0FFFF),
    (b"lightgoldenrodyellow", 0xFAFAD2),
    (b"lightgreen", 0x90EE90),
    (b"lightgrey", 0xD3D3D3),
    (b"lightpink", 0xFFB6C1),
    (b"lightsalmon", 0xFFA07A),
    (b"lightseagreen", 0x20B2AA),
    (b"lightskyblue", 0x87CEFA),
    (b"lightslategray", 0x778899),
    (b"lightsteelblue", 0xB0C4DE),
    (b"lightyellow", 0xFFFFE0),
    (b"lime", 0x00FF00),
    (b"limegreen", 0x32CD32),
    (b"linen", 0xFAF0E6),
    (b"magenta", 0xFF00FF),
    (b"maroon", 0x800000),
    (b"mediumaquamarine", 0x66CDAA),
    (b"mediumblue", 0x0000CD),
    (b"mediumorchid", 0xBA55D3),
    (b"mediumpurple", 0x9370D8),
    (b"mediumseagreen", 0x3CB371),
    (b"mediumslateblue", 0x7B68EE),
    (b"mediumspringgreen", 0x00FA9A),
    (b"mediumturquoise", 0x48D1CC),
    (b"mediumvioletred", 0xC71585),
    (b"midnightblue", 0x191970),
    (b"mintcream", 0xF5FFFA),
    (b"mistyrose", 0xFFE4E1),
    (b"moccasin", 0xFFE4B5),
    (b"navajowhite", 0xFFDEAD),
    (b"navy", 0x000080),
    (b"oldlace", 0xFDF5E6),
    (b"olive", 0x808000),
    (b"olivedrab", 0x6B8E23),
    (b"orange", 0xFFA500),
    (b"orangered", 0xFF4500),
    (b"orchid", 0xDA70D6),
    (b"palegoldenrod", 0xEEE8AA),
    (b"palegreen", 0x98FB98),
    (b"paleturquoise", 0xAFEEEE),
    (b"palevioletred", 0xD87093),
    (b"papayawhip", 0xFFEFD5),
    (b"peachpuff", 0xFFDAB9),
    (b"peru", 0xCD853F),
    (b"pink", 0xFFC0CB),
    (b"plum", 0xDDA0DD),
    (b"powderblue", 0xB0E0E6),
    (b"purple", 0x800080),
    (b"red", 0xFF0000),
    (b"rosybrown", 0xBC8F8F),
    (b"royalblue", 0x4169E1),
    (b"saddlebrown", 0x8B4513),
    (b"salmon", 0xFA8072),
    (b"sandybrown", 0xF4A460),
    (b"seagreen", 0x2E8B57),
    (b"seashell", 0xFFF5EE),
    (b"sienna", 0xA0522D),
    (b"silver", 0xC0C0C0),
    (b"skyblue", 0x87CEEB),
    (b"slateblue", 0x6A5ACD),
    (b"slategray", 0x708090),
    (b"snow", 0xFFFAFA),
    (b"springgreen", 0x00FF7F),
    (b"steelblue", 0x4682B4),
    (b"tan", 0xD2B48C),
    (b"teal", 0x008080),
    (b"thistle", 0xD8BFD8),
    (b"tomato", 0xFF6347),
    (b"turquoise", 0x40E0D0),
    (b"violet", 0xEE82EE),
    (b"wheat", 0xF5DEB3),
    (b"white", 0xFFFFFF),
    (b"whitesmoke", 0xF5F5F5),
    (b"yellow", 0xFFFF00),
    (b"yellowgreen", 0x9ACD32),
];

/// The colour `name` names, in any case.
pub(crate) fn by_name(name: &str) -> Option<u32> {
    let lower = name.to_ascii_lowercase();
    COLOURS
        .binary_search_by(|&(n, _)| n.cmp(lower.as_bytes()))
        .ok()
        .and_then(|i| COLOURS.get(i))
        .map(|&(_, rgb)| rgb)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_table_is_sorted_and_whole() {
        assert!(COLOURS.windows(2).all(|w| matches!(w, [a, b] if a.0 < b.0)));
        assert_eq!(COLOURS.len(), 140);
    }

    #[test]
    fn a_name_is_found_in_any_case() {
        assert_eq!(by_name("navy"), Some(0x00_0080));
        assert_eq!(by_name("LightGoldenRodYellow"), Some(0xFA_FAD2));
        assert_eq!(by_name("grey"), None);
        assert_eq!(by_name(""), None);
    }
}
