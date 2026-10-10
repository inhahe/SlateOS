//! Drawing a hostile SVG neither panics nor hangs.
//!
//! The renderer draws files it did not make: every icon theme a user
//! installs, the file manager's thumbnail of whatever was downloaded, the
//! image viewer's document. Each feature it has grown -- `<use>`, clip paths,
//! masks, patterns, filters, markers, dashes, blending -- came with a bound
//! on what a document can make it do, and with tests of the cases its author
//! thought of. This is for the cases nobody thought of: a corpus of documents
//! using every feature, mutated thousands of ways -- extreme numbers,
//! references to themselves, values from the wrong property, elements
//! doubled and cut -- each parsed and drawn on a thread of its own, against
//! a deadline.
//!
//! Deterministic: the same seed gives the same documents, so a failure is
//! reproduced by running again, and the failing document is printed whole.
//! `SVG_FUZZ_CASES` raises the number of documents (the default keeps the
//! run to seconds); `SVG_FUZZ_SEED` starts from another seed.

// A test fails loudly the instant the code under test is wrong; the
// defensive lints that forbid that in production code are off here, as
// `CLAUDE.md` prescribes for test code.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation
)]

use guitk::svg::SvgDocument;
use std::sync::mpsc;
use std::time::{Duration, Instant};

/// How long one document may take to parse and draw, in a debug build on a
/// loaded machine, before it counts as a hang.
const DEADLINE: Duration = Duration::from_secs(20);

/// A small PNG -- one opaque red pixel -- for the pictures.
const PNG: &str = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8DwHwAFBQIAX8jx0gAAAABJRU5ErkJggg==";

/// Documents using every feature the renderer has, each small.
fn corpus() -> Vec<String> {
    let docs = [
        r##"<rect x="1" y="1" width="18" height="18" rx="3" fill="#36c" stroke="#123" stroke-width="2"/>"##.to_owned(),
        r#"<path d="M2 2 L18 2 Q19 10 18 18 C10 19 4 19 2 18 S1 10 2 2 Z M5 5 A4 3 30 1 0 15 15 T 5 5 H8 V12 h1 v1 l1 1 c1 1 2 2 3 3 s1 1 2 2 q1 1 2 2 t1 1 a1 1 0 0 1 1 1 z" fill-rule="evenodd" fill="currentColor"/>"#.to_owned(),
        r##"<defs><linearGradient id="g" x1="0" y1="0" x2="1" y2="1" spreadMethod="reflect" gradientTransform="rotate(30)"><stop offset="0" stop-color="red"/><stop offset="50%" stop-color="lime" stop-opacity="0.5"/><stop offset="1" stop-color="blue"/></linearGradient><radialGradient id="r" href="#g" cx="50%" cy="50%" r="50%" fx="30%" fy="30%" gradientUnits="userSpaceOnUse"/></defs><circle cx="10" cy="10" r="8" fill="url(#g)" stroke="url(#r) blue"/>"##.to_owned(),
        r##"<defs><symbol id="s" viewBox="0 0 4 4" preserveAspectRatio="xMidYMid slice"><rect width="4" height="4" fill="red"/></symbol><g id="a"><use href="#s" width="5" height="5"/></g></defs><use href="#a" x="2" y="2"/><use href="#a" x="10" y="10" transform="scale(2)"/>"##.to_owned(),
        r#"<clipPath id="c" clipPathUnits="objectBoundingBox"><circle cx="0.5" cy="0.5" r="0.5" clip-path="url(#c2)"/></clipPath><clipPath id="c2"><rect width="15" height="15"/></clipPath><rect width="20" height="20" fill="green" clip-path="url(#c)"/>"#.to_owned(),
        r#"<mask id="m" maskContentUnits="objectBoundingBox" mask-type="alpha"><rect width="1" height="0.5" fill="white" opacity="0.5"/></mask><rect width="20" height="20" fill="navy" mask="url(#m)"/>"#.to_owned(),
        r#"<pattern id="p" width="4" height="4" patternUnits="userSpaceOnUse" patternTransform="rotate(45)" viewBox="0 0 2 2"><circle cx="1" cy="1" r="1" fill="purple"/></pattern><ellipse cx="10" cy="10" rx="9" ry="6" fill="url(#p)" stroke="url(#p)"/>"#.to_owned(),
        r##"<filter id="f" x="-50%" y="-50%" width="200%" height="200%" color-interpolation-filters="sRGB"><feGaussianBlur in="SourceAlpha" stdDeviation="2" result="b"/><feOffset dx="2" dy="2"/><feMerge><feMergeNode/><feMergeNode in="SourceGraphic"/></feMerge><feColorMatrix type="hueRotate" values="90"/><feComponentTransfer><feFuncR type="table" tableValues="0 1 0"/><feFuncA type="gamma" exponent="2"/></feComponentTransfer><feMorphology operator="dilate" radius="1"/><feTurbulence baseFrequency="0.1" numOctaves="3" stitchTiles="stitch"/><feComposite operator="arithmetic" k1="1" k2="0.5" in2="b"/><feConvolveMatrix order="3" kernelMatrix="1 1 1 1 -8 1 1 1 1" edgeMode="wrap"/><feDisplacementMap in2="b" scale="5" xChannelSelector="R" yChannelSelector="A"/><feDiffuseLighting surfaceScale="2"><feSpotLight x="10" y="10" z="20" pointsAtX="0" limitingConeAngle="30"/></feDiffuseLighting><feSpecularLighting specularExponent="10"><fePointLight x="5" y="5" z="5"/></feSpecularLighting><feTile/><feFlood flood-color="red" flood-opacity="0.3"/><feBlend mode="screen" in2="SourceGraphic"/><feDropShadow dx="1" dy="1" stdDeviation="1"/><feImage href="#a"/></filter><g id="a" filter="url(#f)"><rect x="5" y="5" width="10" height="10" fill="orange"/></g><rect width="20" height="20" style="filter: blur(1px) drop-shadow(1px 1px 2px red) grayscale(50%) hue-rotate(1turn)"/>"##.to_owned(),
        r#"<marker id="mk" viewBox="0 0 4 4" refX="center" refY="2" markerWidth="4" markerHeight="4" orient="auto-start-reverse" markerUnits="strokeWidth"><path d="M0 0 L4 2 L0 4 z" marker-end="url(#mk)"/></marker><polyline points="2 2 10 18 18 2" fill="none" stroke="black" marker-start="url(#mk)" marker-mid="url(#mk)" marker-end="url(#mk)"/>"#.to_owned(),
        format!(r#"<image href="{PNG}" x="1" y="1" width="18" height="18" preserveAspectRatio="xMinYMax slice" image-rendering="pixelated"/><filter id="fi"><feImage href="{PNG}"/></filter><rect width="10" height="10" filter="url(#fi)"/>"#),
        r#"<g stroke="black" stroke-width="3" stroke-dasharray="2 1 0" stroke-dashoffset="-3" stroke-linecap="round" stroke-linejoin="round"><line x1="1" y1="1" x2="19" y2="19" pathLength="10"/><circle cx="10" cy="10" r="7" fill="none" vector-effect="non-scaling-stroke" transform="scale(1 0.5)"/></g>"#.to_owned(),
        r#"<style>.a { fill: red; mix-blend-mode: multiply } #b > rect { stroke: blue; paint-order: stroke } * { shape-rendering: crispEdges }</style><g id="b" isolation="isolate" opacity="0.7"><rect class="a" width="12" height="12"/><rect class="a" x="8" y="8" width="12" height="12" visibility="hidden"/></g>"#.to_owned(),
        r#"<switch><g requiredExtensions="x"><rect width="20" height="20"/></g><a href="x" systemLanguage="en, fr"><rect width="10" height="10" fill="teal"/></a><rect width="5" height="5"/></switch><svg x="10" y="10" width="10" height="10" viewBox="0 0 1 1" overflow="visible"><circle r="1" fill="gold"/></svg>"#.to_owned(),
    ];
    docs.into_iter()
        .map(|body| {
            format!(
                r#"<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" viewBox="0 0 20 20" width="20" height="20">{body}</svg>"#
            )
        })
        .collect()
}

/// xorshift64*: deterministic, and enough to choose mutations by.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    /// A number below `n`, which must not be nought.
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }

    fn pick<'a, T>(&mut self, items: &'a [T]) -> &'a T {
        &items[self.below(items.len())]
    }

    /// One of `words`.
    fn word(&mut self, words: &[&'static str]) -> &'static str {
        words[self.below(words.len())]
    }
}

/// Numbers at the edges of what a renderer meets.
const NUMBERS: &[&str] = &[
    "0",
    "-0",
    "1",
    "-1",
    "0.5",
    "1e30",
    "-1e30",
    "1e-30",
    "1e38",
    "3.4e38",
    "-3.4e38",
    "NaN",
    "inf",
    "-inf",
    "65536",
    "2147483647",
    "-2147483648",
    "4294967296",
    "0.0001",
    "999999",
    "1e5",
    "1e-7",
    "100%",
    "-50%",
    "1e309",
];

/// Values that belong to some property, put where another is expected.
const VALUES: &[&str] = &[
    "",
    "none",
    "url(#a)",
    "url(#)",
    "url(#missing)",
    "url(#f)",
    "url(#m)",
    "url(#c)",
    "url(#p)",
    "url(#g)",
    "url(#mk)",
    "#000",
    "#12",
    "rgb(300,-5,NaN)",
    "auto",
    "inherit",
    "currentColor",
    "0 0 0 0",
    "1 2 3",
    "0 0 1e30 1e30",
    "M0 0",
    "M0 0 L",
    "M1e30 1e30 L-1e30 -1e30",
    "matrix(0 0 0 0 0 0)",
    "matrix(1e30 0 0 1e30 0 0)",
    "scale(0)",
    "scale(1e-30)",
    "rotate(1e30)",
    "translate(NaN)",
    "skewX(90)",
    "objectBoundingBox",
    "userSpaceOnUse",
    "evenodd",
    "round",
    "crispEdges",
    "multiply",
    "luminosity",
    "isolate",
    "hidden",
    "collapse",
    "non-scaling-stroke",
    "stroke markers fill",
    "en-US",
    "x",
    "1e309 1e309",
    "-1 -1",
    "0 1e-30",
    "data:image/png;base64,AAAA",
    "data:,",
    "SourceAlpha",
    "BackgroundImage",
    "reflect",
    "repeat",
    "alpha",
    "luminance",
    "auto-start-reverse",
    "slice",
    "xMaxYMax meet",
];

/// Fragments that reach for the bounds: references to themselves, sizes
/// past the drawing, nesting.
const FRAGMENTS: &[&str] = &[
    r##"<use href="#a"/>"##,
    r##"<g id="a"><use href="#a"/></g>"##,
    r##"<g id="u1"><use href="#u2"/><use href="#u2"/></g><g id="u2"><use href="#u1"/><use href="#u1"/></g><use href="#u1"/>"##,
    r##"<clipPath id="c"><use href="#c"/><rect width="1" height="1" clip-path="url(#c)"/></clipPath><rect width="20" height="20" clip-path="url(#c)"/>"##,
    r#"<mask id="m"><rect width="1e9" height="1e9" fill="white" mask="url(#m)"/></mask><rect width="20" height="20" mask="url(#m)"/>"#,
    r#"<filter id="f" filterUnits="userSpaceOnUse" x="-1e9" y="-1e9" width="2e9" height="2e9"><feGaussianBlur stdDeviation="1e9"/><feMorphology radius="1e9"/><feConvolveMatrix order="1000" kernelMatrix="1"/><feTurbulence baseFrequency="1e9" numOctaves="1000"/></filter><rect width="20" height="20" filter="url(#f)"/>"#,
    r##"<marker id="mk" markerWidth="1e9" markerHeight="1e9"><path d="M0 0 L1e9 1e9" marker-start="url(#mk)"/><use href="#mk"/></marker><path d="M0 0 L1 1 L2 2 L3 3 L4 4 L5 5 L6 6 L7 7" marker-mid="url(#mk)" stroke="red"/>"##,
    r#"<pattern id="p" width="1e-9" height="1e-9"><rect width="1" height="1" fill="url(#p)"/></pattern><rect width="20" height="20" fill="url(#p)"/>"#,
    r#"<line x2="1e9" stroke="black" stroke-dasharray="1e-9" vector-effect="non-scaling-stroke"/>"#,
    r#"<g opacity="0.5" isolation="isolate"><g opacity="0.5"><g opacity="0.5"><rect width="20" height="20" mix-blend-mode="difference" filter="url(#f)"/></g></g></g>"#,
    r##"<switch><switch><switch><use href="#a"/></switch></switch></switch>"##,
    r#"<svg viewBox="0 0 0 0"><svg viewBox="0 0 1e-30 1e-30"><rect width="1" height="1"/></svg></svg>"#,
    r#"<image href="data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8DwHwAFBQIAX8jx0gAAAABJRU5ErkJggg==" width="1e9" height="1e-9"/>"#,
    r#"<path d="M0 0 A1e30 1e-30 1e30 1 1 1e-30 1e30 A0 0 0 0 0 0 0 Z"/>"#,
    r#"<text x="1" y="10">words</text><foreignObject width="20" height="20"><div xmlns="http://www.w3.org/1999/xhtml">x</div></foreignObject>"#,
];

/// The spans of `doc` that are numbers.
fn numbers(doc: &str) -> Vec<(usize, usize)> {
    let bytes = doc.as_bytes();
    let mut spans = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i].is_ascii_digit() {
            let start = i;
            while i < bytes.len() && (bytes[i].is_ascii_digit() || bytes[i] == b'.') {
                i += 1;
            }
            spans.push((start, i));
        } else {
            i += 1;
        }
    }
    spans
}

/// The spans of `doc` that are attribute values: what is between a `="`
/// and the next `"`.
fn values(doc: &str) -> Vec<(usize, usize)> {
    let mut spans = Vec::new();
    let mut from = 0;
    while let Some(at) = doc[from..].find("=\"") {
        let start = from + at + 2;
        let Some(len) = doc[start..].find('"') else {
            break;
        };
        spans.push((start, start + len));
        from = start + len + 1;
    }
    spans
}

/// The spans of `doc` that are whole elements with no children: `<` to the
/// `/>` that ends them.
fn elements(doc: &str) -> Vec<(usize, usize)> {
    let mut spans = Vec::new();
    let mut from = 0;
    while let Some(at) = doc[from..].find('<') {
        let start = from + at;
        let Some(len) = doc[start..].find('>') else {
            break;
        };
        let end = start + len + 1;
        if doc[..end].ends_with("/>") && !doc[start..].starts_with("</") {
            spans.push((start, end));
        }
        from = start + 1;
    }
    spans
}

/// Where an element may go: just after a `>`.
fn after_tags(doc: &str) -> Vec<usize> {
    doc.char_indices()
        .filter(|&(_, c)| c == '>')
        .map(|(i, _)| i + 1)
        .collect()
}

/// `doc` with `span` replaced by `with`.
fn replace(doc: &str, span: (usize, usize), with: &str) -> String {
    format!("{}{}{}", &doc[..span.0], with, &doc[span.1..])
}

/// `doc` changed once, at random.
fn mutate(doc: &str, rng: &mut Rng) -> String {
    match rng.below(6) {
        0 => {
            let spans = numbers(doc);
            if spans.is_empty() {
                return doc.to_owned();
            }
            let span = *rng.pick(&spans);
            replace(doc, span, rng.word(NUMBERS))
        }
        1 => {
            let spans = values(doc);
            if spans.is_empty() {
                return doc.to_owned();
            }
            let span = *rng.pick(&spans);
            replace(doc, span, rng.word(VALUES))
        }
        2 => {
            let spans = elements(doc);
            let places = after_tags(doc);
            if spans.is_empty() || places.is_empty() {
                return doc.to_owned();
            }
            let (start, end) = *rng.pick(&spans);
            let at = *rng.pick(&places);
            let copy = doc[start..end].to_owned();
            replace(doc, (at, at), &copy)
        }
        3 => {
            let places = after_tags(doc);
            if places.len() < 2 {
                return doc.to_owned();
            }
            // Cut from after one tag to after a later one: whole markup,
            // more often than a cut through the middle of a name.
            let a = *rng.pick(&places);
            let b = *rng.pick(&places);
            let (from, to) = (a.min(b), a.max(b));
            if to == doc.len() || from == to {
                return doc.to_owned();
            }
            replace(doc, (from, to), "")
        }
        4 => {
            let places = after_tags(doc);
            // Anywhere but after the closing tag.
            if places.len() < 2 {
                return doc.to_owned();
            }
            let at = *rng.pick(&places[..places.len() - 1]);
            replace(doc, (at, at), rng.word(FRAGMENTS))
        }
        _ => {
            // The whole drawing under a transform at an extreme.
            let Some(open) = doc.find('>') else {
                return doc.to_owned();
            };
            let Some(close) = doc.rfind("</svg>") else {
                return doc.to_owned();
            };
            let transform = rng.pick(&[
                "scale(1e6)",
                "scale(1e-6)",
                "scale(-1 1)",
                "rotate(45 10 10)",
                "matrix(1 1 1 1 0 0)",
                "translate(1e30)",
            ]);
            format!(
                "{}<g transform=\"{transform}\">{}</g>{}",
                &doc[..=open],
                &doc[open + 1..close],
                &doc[close..]
            )
        }
    }
}

/// Parse and draw `doc` at `size` on a thread of its own: `Err` with what
/// went wrong where it panicked or ran past [`DEADLINE`].
fn draw(doc: &str, size: (u32, u32)) -> Result<Duration, String> {
    let (send, receive) = mpsc::channel();
    let text = doc.to_owned();
    let start = Instant::now();
    std::thread::Builder::new()
        .stack_size(8 << 20)
        .spawn(move || {
            let drawn = std::panic::catch_unwind(|| {
                if let Ok(document) = SvgDocument::parse(&text) {
                    std::hint::black_box(document.render(size.0, size.1));
                }
            });
            // The receiver may have gone on a deadline; nothing to tell then.
            let _ = send.send(drawn.is_ok());
        })
        .expect("a thread");
    match receive.recv_timeout(DEADLINE) {
        Ok(true) => Ok(start.elapsed()),
        Ok(false) => Err("panicked".to_owned()),
        Err(_) => Err(format!("took longer than {DEADLINE:?}")),
    }
}

/// **No mutation of any document in the corpus panics the renderer or runs
/// past the deadline.**
#[test]
fn hostile_documents_neither_panic_nor_hang() {
    let cases: usize = std::env::var("SVG_FUZZ_CASES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(300);
    let seed: u64 = std::env::var("SVG_FUZZ_SEED")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(0x5EED_5167);
    let corpus = corpus();
    // The corpus itself first, unchanged, at a few sizes.
    for doc in &corpus {
        for size in [(1, 1), (20, 20), (64, 64)] {
            if let Err(what) = draw(doc, size) {
                panic!("the corpus document {what} at {size:?}:\n{doc}");
            }
        }
    }
    let mut rng = Rng(seed | 1);
    let mut slowest = (Duration::ZERO, String::new());
    // A panic printed by the default hook for each failing case would bury
    // the report; the first failure is reported whole below.
    let hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let mut failure = None;
    for case in 0..cases {
        let mut doc = rng.pick(&corpus).clone();
        for _ in 0..=rng.below(4) {
            doc = mutate(&doc, &mut rng);
        }
        let size = *rng.pick(&[(1, 1), (7, 3), (16, 16), (32, 32), (48, 20), (64, 64)]);
        match draw(&doc, size) {
            Ok(took) if took > slowest.0 => slowest = (took, doc),
            Ok(_) => {}
            Err(what) => {
                failure = Some(format!(
                    "case {case} (seed {seed:#x}) {what} at {size:?}:\n{doc}"
                ));
                break;
            }
        }
    }
    std::panic::set_hook(hook);
    if let Some(failure) = failure {
        panic!("{failure}");
    }
    eprintln!(
        "{cases} documents; the slowest took {:?}:\n{}",
        slowest.0, slowest.1
    );
}
