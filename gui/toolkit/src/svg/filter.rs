//! Filters: what a `filter` property says -- the `<filter>` elements it
//! names by `url(#id)`, and CSS's filter functions -- built into primitives,
//! and applied to what an element draws.
//!
//! # What is drawn
//!
//! - **Every Filter Effects 1 primitive**: `feBlend` (all sixteen blend
//!   modes), `feColorMatrix`, `feComponentTransfer`, `feComposite` (and
//!   Filter Effects 2's `lighter`), `feConvolveMatrix`, `feDiffuseLighting`
//!   and `feSpecularLighting` with distant, point and spot lights,
//!   `feDisplacementMap`, `feDropShadow`, `feFlood`, `feGaussianBlur`,
//!   `feImage` naming an element, `feMerge`, `feMorphology`, `feOffset`,
//!   `feTile` and `feTurbulence` -- each as the `effects` module computes it.
//! - **Inputs and results**: `SourceGraphic`, `SourceAlpha`, and any earlier
//!   primitive's `result` by name (the nearest before it with that name); no
//!   `in` takes the previous primitive's result, or the source for the
//!   first. `BackgroundImage`, `BackgroundAlpha`, `FillPaint` and
//!   `StrokePaint`, which no browser draws either, are transparent.
//! - **Regions**: the filter's region by `filterUnits` (the element's box,
//!   by default, -10%, -10%, 120%, 120% of it) and each primitive's
//!   subregion by `primitiveUnits`, defaulting -- as the specification says
//!   -- to the union of its inputs' subregions, or the filter's region where
//!   it reads a standard input, reads nothing, or tiles. Everything outside a
//!   subregion is transparent, and everything outside the region is cut.
//! - **Units**: lengths -- a deviation, an offset, a radius, a light's
//!   position -- are in `primitiveUnits`, fractions of the box under
//!   `objectBoundingBox`, and are carried to the pixels by the element's
//!   transform.
//! - **Colour**: each primitive works in its `color-interpolation-filters`,
//!   inherited from the `<filter>`, linear light by default; `flood-color`,
//!   `flood-opacity` and `lighting-color` are read as properties.
//! - **Filter functions**: `blur()`, `brightness()`, `contrast()`,
//!   `drop-shadow()`, `grayscale()`, `hue-rotate()`, `invert()`,
//!   `opacity()`, `saturate()` and `sepia()`, as the primitives the
//!   specification defines them by, in sRGB; and a list of filters, each
//!   applied to what the one before it made.
//!
//! An element whose filter is measured against a box it does not have (a
//! horizontal line, under the default units), whose region has no area, or
//! whose `<filter>` holds no primitive, is not drawn at all, as browsers do;
//! a `filter` naming no `<filter>` filters nothing.
//!
//! Not drawn: `feImage` naming a picture file (transparent), and a
//! convolution kernel of more than [`MAX_KERNEL_VALUES`] values (passed
//! through). A filter is computed at a lower resolution where its region
//! would cover more than [`MAX_FILTER_PIXELS`] pixels, and scaled up, as
//! Firefox does.

use std::borrow::Cow;
use std::collections::HashMap;

use super::clip::Referable;
use super::effects::{
    self, Area, BlendMode, CompositeOp, EdgeMode, Image, Kernel, Light, Lighting, Space, Transfer,
    Turbulence,
};
use super::paint::Units;
use super::{
    Extent, SvgPaint, SvgRenderer, Transform, XmlElement, parse_color, property, viewport_length,
};

/// The most values a convolution kernel may have: one larger passes its
/// input through. Each output pixel costs one multiplication per value.
pub(super) const MAX_KERNEL_VALUES: usize = 1024;

/// The most octaves of turbulence summed: past this many, each adds less
/// than an `f64` holds of the sum.
const MAX_OCTAVES: u32 = 32;

/// The most pixels a filter is computed over: a region larger on the surface
/// is computed at a lower resolution and scaled up -- 2048 by 2048.
pub(super) const MAX_FILTER_PIXELS: usize = 1 << 22;

// ─── The model ──────────────────────────────────────────────────────────────

/// A `<filter>`, or one filter function, built.
#[derive(Clone, Debug)]
pub(super) struct FilterDef {
    /// What its region is measured in: `filterUnits`.
    pub(super) units: Units,
    /// What its primitives' subregions and lengths are measured in.
    pub(super) primitive_units: Units,
    /// Its region, `x, y, width, height`: fractions of the filtered
    /// element's box under [`Units::ObjectBoundingBox`], user units
    /// otherwise.
    pub(super) rect: [f32; 4],
    /// What it does, in order.
    pub(super) primitives: Vec<Primitive>,
}

/// One filter primitive.
#[derive(Clone, Debug)]
pub(super) struct Primitive {
    /// Its `x`, `y`, `width` and `height`, where it says them: fractions of
    /// the box or user units, as [`FilterDef::primitive_units`] says.
    pub(super) subregion: [Option<f32>; 4],
    /// The colour space it works in.
    pub(super) space: Space,
    /// What it computes.
    pub(super) kind: Kind,
}

/// What a primitive reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Input {
    /// What the element drew.
    SourceGraphic,
    /// Its alpha alone.
    SourceAlpha,
    /// `BackgroundImage`, `BackgroundAlpha`, `FillPaint` and `StrokePaint`:
    /// transparent.
    Nothing,
    /// The result of the primitive at this place.
    Result(usize),
}

/// A light source, as written: in `primitiveUnits`.
#[derive(Clone, Debug, PartialEq)]
pub(super) enum LightDef {
    Distant {
        azimuth: f32,
        elevation: f32,
    },
    Point {
        x: f32,
        y: f32,
        z: f32,
    },
    Spot {
        x: f32,
        y: f32,
        z: f32,
        at: (f32, f32, f32),
        exponent: f32,
        cone: Option<f32>,
    },
}

/// What a primitive computes, and from what.
#[derive(Clone, Debug)]
pub(super) enum Kind {
    Blend {
        input: Input,
        backdrop: Input,
        mode: BlendMode,
    },
    ColorMatrix {
        input: Input,
        matrix: [f32; 20],
    },
    ComponentTransfer {
        input: Input,
        funcs: Box<[Transfer; 4]>,
    },
    Composite {
        input: Input,
        other: Input,
        op: CompositeOp,
    },
    /// `None` passes the input through: a kernel the attributes do not
    /// make.
    Convolve {
        input: Input,
        kernel: Option<Kernel>,
    },
    /// `light` `None` lights nothing: no light source in it.
    Lighting {
        input: Input,
        surface_scale: f32,
        lighting: Lighting,
        light: Option<LightDef>,
        /// `lighting-color`, straight sRGB.
        color: [f32; 4],
    },
    Displace {
        input: Input,
        map: Input,
        scale: f32,
        channels: (usize, usize),
    },
    DropShadow {
        input: Input,
        offset: (f32, f32),
        deviation: (f32, f32),
        /// `flood-color` and `flood-opacity`, straight sRGB.
        color: [f32; 4],
    },
    Flood {
        /// `flood-color` and `flood-opacity`, straight sRGB.
        color: [f32; 4],
    },
    Blur {
        input: Input,
        deviation: (f32, f32),
    },
    /// An element drawn: the place among the document's reused content of
    /// the one it names, or `None` for a picture file, which is not drawn.
    Image {
        content: Option<usize>,
    },
    Merge {
        inputs: Vec<Input>,
    },
    Morphology {
        input: Input,
        dilate: bool,
        radius: (f32, f32),
    },
    Offset {
        input: Input,
        offset: (f32, f32),
    },
    Tile {
        input: Input,
    },
    Turbulence {
        base: (f32, f32),
        octaves: u32,
        seed: i64,
        fractal: bool,
        stitch: bool,
    },
}

impl Kind {
    /// What it reads.
    fn inputs(&self) -> Vec<Input> {
        match self {
            Self::Blend {
                input, backdrop, ..
            } => vec![*input, *backdrop],
            Self::Composite { input, other, .. } => vec![*input, *other],
            Self::Displace { input, map, .. } => vec![*input, *map],
            Self::ColorMatrix { input, .. }
            | Self::ComponentTransfer { input, .. }
            | Self::Convolve { input, .. }
            | Self::Lighting { input, .. }
            | Self::DropShadow { input, .. }
            | Self::Blur { input, .. }
            | Self::Morphology { input, .. }
            | Self::Offset { input, .. }
            | Self::Tile { input } => vec![*input],
            Self::Merge { inputs } => inputs.clone(),
            Self::Flood { .. } | Self::Image { .. } | Self::Turbulence { .. } => Vec::new(),
        }
    }
}

// ─── Reading `<filter>` elements ────────────────────────────────────────────

/// A `<filter>`, built: its units and region, and its primitives, each
/// input resolved to what it names. `content_of` gives the place, among the
/// document's reused content, of the element an `feImage` names by `id`.
pub(super) fn build_filter(
    elem: &XmlElement,
    viewport: (f32, f32),
    content_of: &dyn Fn(&str) -> Option<usize>,
) -> FilterDef {
    let units = units_of(elem, "filterUnits", Units::ObjectBoundingBox);
    let primitive_units = units_of(elem, "primitiveUnits", Units::UserSpaceOnUse);
    let rect = region_rect(elem, units, viewport);
    let inherited = space_of(elem).unwrap_or(Space::LinearRgb);
    let mut primitives: Vec<Primitive> = Vec::new();
    let mut results: HashMap<&str, usize> = HashMap::new();
    for child in &elem.children {
        let index = primitives.len();
        let default = index
            .checked_sub(1)
            .map_or(Input::SourceGraphic, Input::Result);
        let resolve = |value: Option<&str>| input(value, default, &results);
        let Some(kind) = build_kind(child, &resolve, content_of) else {
            continue;
        };
        let place = |name: &str, extent: f32| {
            let value = child.attr(name)?;
            match primitive_units {
                Units::ObjectBoundingBox => fraction(value),
                Units::UserSpaceOnUse => viewport_length(value, extent),
            }
        };
        let (view_w, view_h) = viewport;
        let subregion = [
            place("x", view_w),
            place("y", view_h),
            place("width", view_w).map(|w| w.max(0.0)),
            place("height", view_h).map(|h| h.max(0.0)),
        ];
        primitives.push(Primitive {
            subregion,
            space: space_of(child).unwrap_or(inherited),
            kind,
        });
        if let Some(name) = child
            .attr("result")
            .map(str::trim)
            .filter(|n| !n.is_empty())
        {
            results.insert(name, index);
        }
    }
    FilterDef {
        units,
        primitive_units,
        rect,
        primitives,
    }
}

/// What an `in` or `in2` attribute names, `default` where it names nothing
/// -- or a result no earlier primitive has, which the specification reads
/// as not naming one.
fn input(value: Option<&str>, default: Input, results: &HashMap<&str, usize>) -> Input {
    match value.map(str::trim) {
        None | Some("") => default,
        Some("SourceGraphic") => Input::SourceGraphic,
        Some("SourceAlpha") => Input::SourceAlpha,
        Some("BackgroundImage" | "BackgroundAlpha" | "FillPaint" | "StrokePaint") => Input::Nothing,
        Some(name) => results.get(name).map_or(default, |&i| Input::Result(i)),
    }
}

/// The units an attribute says, `default` where it says none it knows.
fn units_of(elem: &XmlElement, name: &str, default: Units) -> Units {
    match elem.attr(name).map(str::trim) {
        Some("objectBoundingBox") => Units::ObjectBoundingBox,
        Some("userSpaceOnUse") => Units::UserSpaceOnUse,
        _ => default,
    }
}

/// A filter's region as written -- by default -10%, -10%, 120%, 120% -- in
/// fractions of the box, or in user units with percentages of `viewport`.
fn region_rect(elem: &XmlElement, units: Units, viewport: (f32, f32)) -> [f32; 4] {
    let (view_w, view_h) = viewport;
    let side = |name: &str, extent: f32, default: f32| match units {
        Units::ObjectBoundingBox => elem.attr(name).and_then(fraction).unwrap_or(default),
        Units::UserSpaceOnUse => elem
            .attr(name)
            .and_then(|value| viewport_length(value, extent))
            .unwrap_or(default * extent),
    };
    [
        side("x", view_w, -0.1),
        side("y", view_h, -0.1),
        side("width", view_w, 1.2),
        side("height", view_h, 1.2),
    ]
}

/// A fraction of the box: a number, or a percentage of one.
fn fraction(value: &str) -> Option<f32> {
    let value = value.trim();
    match value.strip_suffix('%') {
        Some(percent) => number_text(percent).map(|p| p / 100.0),
        None => number_text(value),
    }
}

/// A number, finite.
fn number_text(value: &str) -> Option<f32> {
    value.trim().parse::<f32>().ok().filter(|n| n.is_finite())
}

/// An attribute's number, `default` where it has none or one that is not.
fn number(elem: &XmlElement, name: &str, default: f32) -> f32 {
    elem.attr(name).and_then(number_text).unwrap_or(default)
}

/// A list of numbers, separated by spaces or commas; `None` where any is
/// not one.
fn numbers(value: &str) -> Option<Vec<f32>> {
    value
        .split(|c: char| c == ',' || c.is_whitespace())
        .filter(|s| !s.is_empty())
        .map(number_text)
        .collect()
}

/// A number-optional-number: one number for both, or two.
fn pair(elem: &XmlElement, name: &str, default: (f32, f32)) -> (f32, f32) {
    match elem.attr(name).and_then(numbers).as_deref() {
        Some([both]) => (*both, *both),
        Some([x, y]) => (*x, *y),
        _ => default,
    }
}

/// An element's `color-interpolation-filters`, if it says one; `auto` is
/// linear light, as the initial value is.
fn space_of(elem: &XmlElement) -> Option<Space> {
    match property(elem, "color-interpolation-filters")?.trim() {
        "sRGB" => Some(Space::Srgb),
        "linearRGB" | "auto" => Some(Space::LinearRgb),
        _ => None,
    }
}

/// A colour property with its opacity -- `flood-color` and
/// `flood-opacity`, say -- as straight sRGB; `currentColor`, and anything
/// unreadable, black.
fn color_property(
    elem: &XmlElement,
    color: &str,
    opacity: Option<&str>,
    default: [f32; 4],
) -> [f32; 4] {
    let mut rgba = match property(elem, color).map(parse_color) {
        Some(Ok(SvgPaint::Color(c))) => [
            f32::from(c.r) / 255.0,
            f32::from(c.g) / 255.0,
            f32::from(c.b) / 255.0,
            f32::from(c.a) / 255.0,
        ],
        Some(Ok(SvgPaint::CurrentColor)) => [0.0, 0.0, 0.0, 1.0],
        _ => default,
    };
    if let Some(alpha) = opacity
        .and_then(|name| property(elem, name))
        .and_then(fraction)
    {
        rgba[3] *= alpha.clamp(0.0, 1.0);
    }
    rgba
}

/// What one child of a `<filter>` computes, or `None` for one that is not
/// a primitive. `resolve` reads the value of an `in`-like attribute -- the
/// primitive's own, or an `feMergeNode`'s -- through the results named so
/// far.
fn build_kind(
    elem: &XmlElement,
    resolve: &dyn Fn(Option<&str>) -> Input,
    content_of: &dyn Fn(&str) -> Option<usize>,
) -> Option<Kind> {
    let attr = |name: &str| elem.attr(name).map(str::trim);
    let input_of = |name: &str| resolve(elem.attr(name));
    Some(match elem.tag.as_str() {
        "feBlend" => Kind::Blend {
            input: input_of("in"),
            backdrop: input_of("in2"),
            mode: attr("mode")
                .and_then(BlendMode::named)
                .unwrap_or(BlendMode::Normal),
        },
        "feColorMatrix" => Kind::ColorMatrix {
            input: input_of("in"),
            matrix: color_matrix(elem),
        },
        "feComponentTransfer" => Kind::ComponentTransfer {
            input: input_of("in"),
            funcs: Box::new(transfer_functions(elem)),
        },
        "feComposite" => Kind::Composite {
            input: input_of("in"),
            other: input_of("in2"),
            op: match attr("operator") {
                Some("in") => CompositeOp::In,
                Some("out") => CompositeOp::Out,
                Some("atop") => CompositeOp::Atop,
                Some("xor") => CompositeOp::Xor,
                Some("lighter") => CompositeOp::Lighter,
                Some("arithmetic") => CompositeOp::Arithmetic([
                    number(elem, "k1", 0.0),
                    number(elem, "k2", 0.0),
                    number(elem, "k3", 0.0),
                    number(elem, "k4", 0.0),
                ]),
                _ => CompositeOp::Over,
            },
        },
        "feConvolveMatrix" => Kind::Convolve {
            input: input_of("in"),
            kernel: kernel(elem),
        },
        "feDiffuseLighting" => Kind::Lighting {
            input: input_of("in"),
            surface_scale: number(elem, "surfaceScale", 1.0),
            lighting: Lighting::Diffuse {
                constant: number(elem, "diffuseConstant", 1.0).max(0.0),
            },
            light: light_source(elem),
            color: color_property(elem, "lighting-color", None, [1.0; 4]),
        },
        "feSpecularLighting" => Kind::Lighting {
            input: input_of("in"),
            surface_scale: number(elem, "surfaceScale", 1.0),
            lighting: Lighting::Specular {
                constant: number(elem, "specularConstant", 1.0).max(0.0),
                exponent: number(elem, "specularExponent", 1.0).clamp(1.0, 128.0),
            },
            light: light_source(elem),
            color: color_property(elem, "lighting-color", None, [1.0; 4]),
        },
        "feDisplacementMap" => {
            let channel = |name: &str| match attr(name) {
                Some("R") => 0,
                Some("G") => 1,
                Some("B") => 2,
                _ => 3,
            };
            Kind::Displace {
                input: input_of("in"),
                map: input_of("in2"),
                scale: number(elem, "scale", 0.0),
                channels: (channel("xChannelSelector"), channel("yChannelSelector")),
            }
        }
        "feDropShadow" => Kind::DropShadow {
            input: input_of("in"),
            offset: (number(elem, "dx", 2.0), number(elem, "dy", 2.0)),
            deviation: deviation(pair(elem, "stdDeviation", (2.0, 2.0))),
            color: color_property(
                elem,
                "flood-color",
                Some("flood-opacity"),
                [0.0, 0.0, 0.0, 1.0],
            ),
        },
        "feFlood" => Kind::Flood {
            color: color_property(
                elem,
                "flood-color",
                Some("flood-opacity"),
                [0.0, 0.0, 0.0, 1.0],
            ),
        },
        "feGaussianBlur" => Kind::Blur {
            input: input_of("in"),
            deviation: deviation(pair(elem, "stdDeviation", (0.0, 0.0))),
        },
        "feImage" => Kind::Image {
            content: elem
                .attr("href")
                .or_else(|| elem.attr("xlink:href"))
                .map(str::trim)
                .and_then(|href| href.strip_prefix('#'))
                .and_then(content_of),
        },
        // Each node reads as a primitive's own `in` would, through the same
        // names; one naming nothing reads what a primitive naming nothing
        // reads.
        "feMerge" => Kind::Merge {
            inputs: elem
                .children
                .iter()
                .filter(|node| node.tag == "feMergeNode")
                .map(|node| resolve(node.attr("in")))
                .collect(),
        },
        "feMorphology" => {
            let (rx, ry) = pair(elem, "radius", (0.0, 0.0));
            Kind::Morphology {
                input: input_of("in"),
                dilate: attr("operator") == Some("dilate"),
                // A negative radius disables the primitive: what it reads
                // passes through, as a radius of nought does.
                radius: if rx < 0.0 || ry < 0.0 {
                    (0.0, 0.0)
                } else {
                    (rx, ry)
                },
            }
        }
        "feOffset" => Kind::Offset {
            input: input_of("in"),
            offset: (number(elem, "dx", 0.0), number(elem, "dy", 0.0)),
        },
        "feTile" => Kind::Tile {
            input: input_of("in"),
        },
        "feTurbulence" => {
            let octaves = number(elem, "numOctaves", 1.0);
            // Truncated toward nought, as the specification has `seed`.
            #[allow(
                clippy::cast_possible_truncation,
                reason = "held to +-2^31 first, and truncation is what the specification asks"
            )]
            let seed = number(elem, "seed", 0.0).clamp(-2.147e9, 2.147e9).trunc() as i64;
            #[allow(
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                reason = "held to 0..=MAX_OCTAVES first"
            )]
            let octaves = octaves.trunc().clamp(0.0, MAX_OCTAVES as f32) as u32;
            Kind::Turbulence {
                base: pair(elem, "baseFrequency", (0.0, 0.0)),
                octaves,
                seed,
                fractal: attr("type") == Some("fractalNoise"),
                stitch: attr("stitchTiles") == Some("stitch"),
            }
        }
        _ => return None,
    })
}

/// A deviation of which either part is negative: the primitive disabled,
/// which for a blur is no blur at all.
fn deviation((x, y): (f32, f32)) -> (f32, f32) {
    if x < 0.0 || y < 0.0 {
        (0.0, 0.0)
    } else {
        (x, y)
    }
}

/// An `feColorMatrix`'s matrix: its `type` and `values` read, the identity
/// where they do not make one.
fn color_matrix(elem: &XmlElement) -> [f32; 20] {
    let values = elem.attr("values").and_then(numbers);
    let single = |default: f32| match values.as_deref() {
        Some([v]) => *v,
        _ => default,
    };
    match elem.attr("type").map(str::trim).unwrap_or("matrix") {
        "saturate" => effects::saturate_matrix(single(1.0).max(0.0)),
        "hueRotate" => effects::hue_rotate_matrix(single(0.0)),
        "luminanceToAlpha" => effects::LUMINANCE_TO_ALPHA,
        _ => values
            .and_then(|v| <[f32; 20]>::try_from(v.as_slice()).ok())
            .unwrap_or(effects::IDENTITY_MATRIX),
    }
}

/// An `feComponentTransfer`'s four functions -- red, green, blue, alpha --
/// each the last of its kind among the children; identity where none says.
fn transfer_functions(elem: &XmlElement) -> [Transfer; 4] {
    let mut funcs = [
        Transfer::Identity,
        Transfer::Identity,
        Transfer::Identity,
        Transfer::Identity,
    ];
    for child in &elem.children {
        let slot = match child.tag.as_str() {
            "feFuncR" => funcs.get_mut(0),
            "feFuncG" => funcs.get_mut(1),
            "feFuncB" => funcs.get_mut(2),
            "feFuncA" => funcs.get_mut(3),
            _ => None,
        };
        if let Some(slot) = slot {
            *slot = transfer(child);
        }
    }
    funcs
}

/// One `feFunc*`.
fn transfer(elem: &XmlElement) -> Transfer {
    let table = || {
        elem.attr("tableValues")
            .and_then(numbers)
            .unwrap_or_default()
    };
    match elem.attr("type").map(str::trim) {
        Some("table") => Transfer::Table(table()),
        Some("discrete") => Transfer::Discrete(table()),
        Some("linear") => Transfer::Linear {
            slope: number(elem, "slope", 1.0),
            intercept: number(elem, "intercept", 0.0),
        },
        Some("gamma") => Transfer::Gamma {
            amplitude: number(elem, "amplitude", 1.0),
            exponent: number(elem, "exponent", 1.0),
            offset: number(elem, "offset", 0.0),
        },
        _ => Transfer::Identity,
    }
}

/// An `feConvolveMatrix`'s kernel, or `None` where its attributes do not
/// make one -- an order that is not a positive whole number, a matrix of
/// the wrong length or longer than [`MAX_KERNEL_VALUES`], a target outside
/// it -- which the specification has pass its input through.
fn kernel(elem: &XmlElement) -> Option<Kernel> {
    let whole = |v: f32| -> Option<usize> {
        let v = v.trunc();
        // Held to a kernel's size first, so the cast is exact.
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            clippy::cast_precision_loss,
            reason = "checked to be in 1..=MAX_KERNEL_VALUES first"
        )]
        let n = (v >= 1.0 && v <= MAX_KERNEL_VALUES as f32).then_some(v as usize);
        n
    };
    let (ox, oy) = pair(elem, "order", (3.0, 3.0));
    let (order_x, order_y) = (whole(ox)?, whole(oy)?);
    let count = order_x.checked_mul(order_y)?;
    if count > MAX_KERNEL_VALUES {
        return None;
    }
    let values = numbers(elem.attr("kernelMatrix")?)?;
    if values.len() != count {
        return None;
    }
    let sum: f32 = values.iter().sum();
    let divisor = match elem.attr("divisor").and_then(number_text) {
        Some(d) if d != 0.0 => d,
        _ if sum != 0.0 => sum,
        _ => 1.0,
    };
    let target = |name: &str, order: usize| -> Option<usize> {
        match elem.attr(name).and_then(number_text) {
            None => Some(order >> 1),
            Some(t) => {
                let t = t.trunc();
                #[allow(
                    clippy::cast_precision_loss,
                    reason = "an order of at most MAX_KERNEL_VALUES, exact in f32"
                )]
                let fits = t >= 0.0 && t < order as f32;
                // Checked to be in 0..order just above.
                #[allow(
                    clippy::cast_possible_truncation,
                    clippy::cast_sign_loss,
                    reason = "checked to be in 0..order first"
                )]
                let t = fits.then_some(t as usize);
                t
            }
        }
    };
    Some(Kernel {
        order_x,
        order_y,
        values,
        divisor,
        bias: number(elem, "bias", 0.0),
        target_x: target("targetX", order_x)?,
        target_y: target("targetY", order_y)?,
        edge: match elem.attr("edgeMode").map(str::trim) {
            Some("wrap") => EdgeMode::Wrap,
            Some("none") => EdgeMode::None,
            _ => EdgeMode::Duplicate,
        },
        preserve_alpha: elem.attr("preserveAlpha").map(str::trim) == Some("true"),
    })
}

/// A lighting primitive's light: the first light source among its children.
fn light_source(elem: &XmlElement) -> Option<LightDef> {
    elem.children.iter().find_map(|child| {
        let n = |name: &str, default: f32| number(child, name, default);
        match child.tag.as_str() {
            "feDistantLight" => Some(LightDef::Distant {
                azimuth: n("azimuth", 0.0),
                elevation: n("elevation", 0.0),
            }),
            "fePointLight" => Some(LightDef::Point {
                x: n("x", 0.0),
                y: n("y", 0.0),
                z: n("z", 0.0),
            }),
            "feSpotLight" => Some(LightDef::Spot {
                x: n("x", 0.0),
                y: n("y", 0.0),
                z: n("z", 0.0),
                at: (
                    n("pointsAtX", 0.0),
                    n("pointsAtY", 0.0),
                    n("pointsAtZ", 0.0),
                ),
                exponent: n("specularExponent", 1.0),
                cone: child.attr("limitingConeAngle").and_then(number_text),
            }),
            _ => None,
        }
    })
}

// ─── Reading `filter` properties ────────────────────────────────────────────

/// Every `filter` value a document says, read once each: the filters each
/// applies, and the filter functions among them, built.
#[derive(Default)]
pub(super) struct FilterLists {
    /// Each value's filters, in the order they apply: places among the
    /// document's filters -- its `<filter>` elements', then [`Self::functions`].
    pub(super) lists: Vec<Vec<usize>>,
    /// The filter functions the values hold, built; their places follow the
    /// `<filter>` elements'.
    pub(super) functions: Vec<FilterDef>,
    /// Each value, trimmed, and its place in [`Self::lists`] -- `None` for
    /// one that applies no filter: `none`, a value that cannot be read, or a
    /// list naming something that is not a `<filter>`.
    by_value: HashMap<String, Option<usize>>,
}

impl FilterLists {
    /// Every `filter` value under `root`, read against the document's
    /// `<filter>` elements.
    pub(super) fn collect(root: &XmlElement, filters: &Referable<'_>) -> Self {
        let mut lists = Self::default();
        lists.gather(root, filters);
        lists
    }

    fn gather(&mut self, elem: &XmlElement, filters: &Referable<'_>) {
        if let Some(value) = property(elem, "filter").map(str::trim)
            && !self.by_value.contains_key(value)
        {
            let read = self.read(value, filters);
            let place = read.map(|steps| {
                self.lists.push(steps);
                self.lists.len().saturating_sub(1)
            });
            self.by_value.insert(value.to_owned(), place);
        }
        for child in &elem.children {
            self.gather(child, filters);
        }
    }

    /// The place of the list a `filter` value applies, if it applies one.
    pub(super) fn place(&self, value: &str) -> Option<usize> {
        self.by_value.get(value.trim()).copied().flatten()
    }

    /// The filters `value` applies, in order, or `None` for none.
    fn read(&mut self, value: &str, filters: &Referable<'_>) -> Option<Vec<usize>> {
        if value.is_empty() || value == "none" {
            return None;
        }
        let elements = filters.elements.len();
        let mut steps = Vec::new();
        let mut rest = value;
        while !rest.is_empty() {
            let (name, args, after) = call(rest)?;
            if name.eq_ignore_ascii_case("url") {
                steps.push(filters.place_of(url_id(args)?)?);
            } else {
                self.functions.push(function(name, args)?);
                steps.push(elements.checked_add(self.functions.len().checked_sub(1)?)?);
            }
            rest = after.trim_start();
        }
        (!steps.is_empty()).then_some(steps)
    }
}

/// The first `name(args)` of `text`, and what follows it: the arguments end
/// at the parenthesis that closes the first, however many open inside --
/// `drop-shadow(rgb(0 0 0) 1px 1px)`.
fn call(text: &str) -> Option<(&str, &str, &str)> {
    let open = text.find('(')?;
    let name = text.get(..open)?.trim();
    if name.is_empty() || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
        return None;
    }
    let mut depth = 0usize;
    for (i, c) in text.char_indices().skip_while(|&(i, _)| i < open) {
        match c {
            '(' => depth = depth.saturating_add(1),
            ')' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    let args = text.get(open.saturating_add(1)..i)?;
                    let after = text.get(i.saturating_add(1)..)?;
                    return Some((name, args, after));
                }
            }
            _ => {}
        }
    }
    None
}

/// The `id` a `url(...)`'s arguments name, `#` and quotes taken off.
fn url_id(args: &str) -> Option<&str> {
    args.trim()
        .trim_matches(|c| c == '"' || c == '\'')
        .strip_prefix('#')
        .filter(|id| !id.is_empty())
}

/// A filter function, as the one-primitive filter the specification defines
/// it by: measured in user units over the default region, in sRGB.
fn function(name: &str, args: &str) -> Option<FilterDef> {
    let args = args.trim();
    let rgb = |f: Transfer| -> Box<[Transfer; 4]> {
        Box::new([f.clone(), f.clone(), f, Transfer::Identity])
    };
    let kind = match name.to_ascii_lowercase().as_str() {
        "blur" => {
            let r = if args.is_empty() {
                0.0
            } else {
                css_length(args)?
            };
            if r < 0.0 {
                return None;
            }
            Kind::Blur {
                input: Input::SourceGraphic,
                deviation: (r, r),
            }
        }
        "brightness" => {
            let a = amount(args)?;
            Kind::ComponentTransfer {
                input: Input::SourceGraphic,
                funcs: rgb(Transfer::Linear {
                    slope: a,
                    intercept: 0.0,
                }),
            }
        }
        "contrast" => {
            let a = amount(args)?;
            Kind::ComponentTransfer {
                input: Input::SourceGraphic,
                funcs: rgb(Transfer::Linear {
                    slope: a,
                    intercept: -0.5 * a + 0.5,
                }),
            }
        }
        "drop-shadow" => drop_shadow(args)?,
        "grayscale" => Kind::ColorMatrix {
            input: Input::SourceGraphic,
            matrix: grayscale_matrix(amount(args)?.min(1.0)),
        },
        "hue-rotate" => Kind::ColorMatrix {
            input: Input::SourceGraphic,
            matrix: effects::hue_rotate_matrix(angle(args)?),
        },
        "invert" => {
            let a = amount(args)?.min(1.0);
            Kind::ComponentTransfer {
                input: Input::SourceGraphic,
                funcs: rgb(Transfer::Table(vec![a, 1.0 - a])),
            }
        }
        "opacity" => {
            let a = amount(args)?.min(1.0);
            Kind::ComponentTransfer {
                input: Input::SourceGraphic,
                funcs: Box::new([
                    Transfer::Identity,
                    Transfer::Identity,
                    Transfer::Identity,
                    Transfer::Table(vec![0.0, a]),
                ]),
            }
        }
        "saturate" => Kind::ColorMatrix {
            input: Input::SourceGraphic,
            matrix: effects::saturate_matrix(amount(args)?),
        },
        "sepia" => Kind::ColorMatrix {
            input: Input::SourceGraphic,
            matrix: sepia_matrix(amount(args)?.min(1.0)),
        },
        _ => return None,
    };
    Some(FilterDef {
        units: Units::ObjectBoundingBox,
        primitive_units: Units::UserSpaceOnUse,
        rect: [-0.1, -0.1, 1.2, 1.2],
        primitives: vec![Primitive {
            subregion: [None; 4],
            space: Space::Srgb,
            kind,
        }],
    })
}

/// A filter function's amount: a number or a percentage, not negative; one
/// where none is given.
fn amount(args: &str) -> Option<f32> {
    if args.is_empty() {
        return Some(1.0);
    }
    fraction(args).filter(|a| *a >= 0.0)
}

/// A CSS length in user units: pixels, or a bare number.
fn css_length(text: &str) -> Option<f32> {
    let text = text.trim();
    number_text(text.strip_suffix("px").unwrap_or(text))
}

/// A CSS angle, in degrees; nought where none is given.
fn angle(args: &str) -> Option<f32> {
    if args.is_empty() {
        return Some(0.0);
    }
    let units: [(&str, f32); 4] = [
        ("deg", 1.0),
        ("grad", 0.9),
        ("rad", 180.0 / core::f32::consts::PI),
        ("turn", 360.0),
    ];
    for (unit, scale) in units {
        if let Some(n) = args.strip_suffix(unit) {
            return number_text(n).map(|n| n * scale);
        }
    }
    number_text(args).filter(|n| *n == 0.0)
}

/// `drop-shadow(color? x y blur?)`, the colour on either side: the blur is
/// a radius, twice the deviation, as `box-shadow` reads it.
fn drop_shadow(args: &str) -> Option<Kind> {
    let mut lengths = Vec::new();
    let mut color = None;
    for token in words(args) {
        if let Some(len) = css_length(token) {
            lengths.push(len);
        } else if color.is_none() {
            color = Some(match parse_color(token).ok()? {
                SvgPaint::Color(c) => [
                    f32::from(c.r) / 255.0,
                    f32::from(c.g) / 255.0,
                    f32::from(c.b) / 255.0,
                    f32::from(c.a) / 255.0,
                ],
                SvgPaint::CurrentColor => [0.0, 0.0, 0.0, 1.0],
                _ => return None,
            });
        } else {
            return None;
        }
    }
    let (dx, dy, radius) = match lengths.as_slice() {
        [dx, dy] => (*dx, *dy, 0.0),
        [dx, dy, r] if *r >= 0.0 => (*dx, *dy, *r),
        _ => return None,
    };
    Some(Kind::DropShadow {
        input: Input::SourceGraphic,
        offset: (dx, dy),
        deviation: (radius / 2.0, radius / 2.0),
        color: color.unwrap_or([0.0, 0.0, 0.0, 1.0]),
    })
}

/// `args` split at the spaces outside parentheses.
fn words(args: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut depth = 0usize;
    let mut start = None;
    for (i, c) in args.char_indices() {
        match c {
            '(' => depth = depth.saturating_add(1),
            ')' => depth = depth.saturating_sub(1),
            _ => {}
        }
        if c.is_whitespace() && depth == 0 {
            if let Some(s) = start.take() {
                out.extend(args.get(s..i));
            }
        } else if start.is_none() {
            start = Some(i);
        }
    }
    if let Some(s) = start {
        out.extend(args.get(s..));
    }
    out
}

/// `grayscale()`'s matrix for amount `a`.
fn grayscale_matrix(a: f32) -> [f32; 20] {
    let k = 1.0 - a;
    [
        0.2126 + 0.7874 * k,
        0.7152 - 0.7152 * k,
        0.0722 - 0.0722 * k,
        0.0,
        0.0,
        0.2126 - 0.2126 * k,
        0.7152 + 0.2848 * k,
        0.0722 - 0.0722 * k,
        0.0,
        0.0,
        0.2126 - 0.2126 * k,
        0.7152 - 0.7152 * k,
        0.0722 + 0.9278 * k,
        0.0,
        0.0,
        0.0,
        0.0,
        0.0,
        1.0,
        0.0,
    ]
}

/// `sepia()`'s matrix for amount `a`.
fn sepia_matrix(a: f32) -> [f32; 20] {
    let k = 1.0 - a;
    [
        0.393 + 0.607 * k,
        0.769 - 0.769 * k,
        0.189 - 0.189 * k,
        0.0,
        0.0,
        0.349 - 0.349 * k,
        0.686 + 0.314 * k,
        0.168 - 0.168 * k,
        0.0,
        0.0,
        0.272 - 0.272 * k,
        0.534 - 0.534 * k,
        0.131 + 0.869 * k,
        0.0,
        0.0,
        0.0,
        0.0,
        0.0,
        1.0,
        0.0,
    ]
}

// ─── Applying filters ───────────────────────────────────────────────────────

/// A filter's region in the user space of the element it filters, `x, y,
/// width, height` -- or `None` where the element is not drawn: measured in
/// a box it does not have, or with no area.
pub(super) fn region(def: &FilterDef, bbox: Option<(f32, f32, f32, f32)>) -> Option<[f32; 4]> {
    let [x, y, w, h] = def.rect;
    let rect = match def.units {
        Units::ObjectBoundingBox => {
            let (bx, by, bw, bh) = bbox.filter(|&(_, _, bw, bh)| bw > 0.0 && bh > 0.0)?;
            [bx + x * bw, by + y * bh, w * bw, h * bh]
        }
        Units::UserSpaceOnUse => [x, y, w, h],
    };
    (rect.iter().all(|v| v.is_finite()) && rect[2] > 0.0 && rect[3] > 0.0).then_some(rect)
}

/// How far, in the pixels `local` carries the filtered element's user space
/// to, a filter's primitives read from where they write -- a blur three
/// deviations, an offset its length, one primitive after another -- and a
/// pixel more. Where what the element draws lies further than this past the
/// surface, nothing of it can reach the surface. Infinite for a tile, which
/// repeats what it reads across its whole subregion.
pub(super) fn reach(def: &FilterDef, local: Transform, bbox: Option<(f32, f32, f32, f32)>) -> f32 {
    let in_box = match def.primitive_units {
        Units::ObjectBoundingBox => bbox,
        Units::UserSpaceOnUse => None,
    };
    let target = Target {
        to_layer: local,
        bbox,
        width: 0,
        height: 0,
    };
    let measure = Measure {
        target: &target,
        in_box,
    };
    let length = |(x, y): (f32, f32)| {
        let (a, b) = measure.extent((x.abs(), y.abs()));
        a.max(b)
    };
    let step = |d: (f32, f32)| {
        let (a, b) = measure.step(d);
        a.hypot(b)
    };
    let read: f32 = def
        .primitives
        .iter()
        .map(|p| match &p.kind {
            Kind::Blur { deviation, .. } => 3.0 * length(*deviation),
            Kind::DropShadow {
                deviation, offset, ..
            } => 3.0 * length(*deviation) + step(*offset),
            Kind::Offset { offset, .. } => step(*offset),
            Kind::Morphology { radius, .. } => length(*radius),
            Kind::Displace { scale, .. } => 0.5 * length((*scale, *scale)),
            Kind::Convolve {
                kernel: Some(kernel),
                ..
            } => {
                #[allow(
                    clippy::cast_precision_loss,
                    reason = "an order of at most MAX_KERNEL_VALUES, exact in f32"
                )]
                let order = kernel.order_x.max(kernel.order_y) as f32;
                order
            }
            Kind::Lighting { .. } => 1.0,
            Kind::Tile { .. } => f32::INFINITY,
            _ => 0.0,
        })
        .sum();
    if read.is_nan() {
        f32::INFINITY
    } else {
        read + 1.0
    }
}

/// What a filter is applied to: a layer of pixels, and how the filtered
/// element's user space lies on it.
pub(super) struct Target {
    /// From the element's user space to the layer's pixels.
    pub(super) to_layer: Transform,
    /// The element's box in its user space, if it has one.
    pub(super) bbox: Option<(f32, f32, f32, f32)>,
    pub(super) width: usize,
    pub(super) height: usize,
}

/// How a filter's primitives measure: lengths in `primitiveUnits`, carried
/// to the layer's pixels.
struct Measure<'t> {
    target: &'t Target,
    /// The box, where lengths are fractions of it.
    in_box: Option<(f32, f32, f32, f32)>,
}

impl Measure<'_> {
    /// A coordinate across, in user units.
    fn x(&self, v: f32) -> f32 {
        self.in_box.map_or(v, |(bx, _, bw, _)| bx + v * bw)
    }

    /// A coordinate down, in user units.
    fn y(&self, v: f32) -> f32 {
        self.in_box.map_or(v, |(_, by, _, bh)| by + v * bh)
    }

    /// A length across, in user units.
    fn across(&self, v: f32) -> f32 {
        self.in_box.map_or(v, |(_, _, bw, _)| v * bw)
    }

    /// A length down, in user units.
    fn down(&self, v: f32) -> f32 {
        self.in_box.map_or(v, |(_, _, _, bh)| v * bh)
    }

    /// Lengths across and down -- a deviation, a radius -- in pixels.
    fn extent(&self, (x, y): (f32, f32)) -> (f32, f32) {
        let (sx, sy) = self.target.to_layer.axis_scales();
        (self.across(x) * sx, self.down(y) * sy)
    }

    /// A step `(dx, dy)` -- an offset -- in pixels, turned as the element is.
    fn step(&self, (dx, dy): (f32, f32)) -> (f32, f32) {
        let t = self.target.to_layer;
        let (ux, uy) = (self.across(dx), self.down(dy));
        (t.a * ux + t.b * uy, t.c * ux + t.d * uy)
    }

    /// A point, in pixels.
    fn point(&self, x: f32, y: f32) -> (f32, f32) {
        self.target.to_layer.apply(self.x(x), self.y(y))
    }

    /// A height above the surface -- a light's `z` -- in pixels.
    fn height(&self, z: f32) -> f32 {
        // Under the box's units a height is of the box's mean side: the
        // square root of the mean of the sides' squares, as Filter Effects
        // measures a length that is neither across nor down.
        let z = self.in_box.map_or(z, |(_, _, bw, bh)| {
            z * f32::midpoint(bw * bw, bh * bh).sqrt()
        });
        z * self.target.to_layer.length_scale()
    }

    /// A light, in pixels.
    fn light(&self, light: &LightDef) -> Light {
        match *light {
            LightDef::Distant { azimuth, elevation } => Light::Distant { azimuth, elevation },
            LightDef::Point { x, y, z } => {
                let (px, py) = self.point(x, y);
                Light::Point {
                    x: px,
                    y: py,
                    z: self.height(z),
                }
            }
            LightDef::Spot {
                x,
                y,
                z,
                at,
                exponent,
                cone,
            } => {
                let (px, py) = self.point(x, y);
                let (ax, ay) = self.point(at.0, at.1);
                Light::Spot {
                    x: px,
                    y: py,
                    z: self.height(z),
                    at: (ax, ay, self.height(at.2)),
                    exponent,
                    cone,
                }
            }
        }
    }

    /// The pixels a user-space rectangle covers on the layer.
    fn area(&self, rect: [f32; 4]) -> Area {
        layer_area(rect, self.target)
    }
}

/// The pixels `rect` -- in the filtered element's user space -- covers on
/// the target's layer.
pub(super) fn layer_area(rect: [f32; 4], target: &Target) -> Area {
    let [x, y, w, h] = rect;
    let mut extent = Extent::default();
    for (cx, cy) in [(x, y), (x + w, y), (x, y + h), (x + w, y + h)] {
        let (px, py) = target.to_layer.apply(cx, cy);
        extent.add(px, py);
    }
    let (Ok(width), Ok(height)) = (u32::try_from(target.width), u32::try_from(target.height))
    else {
        return Area::EMPTY;
    };
    extent
        .pixels(width, height)
        .map_or(Area::EMPTY, |(x0, y0, x1, y1)| {
            let at = |v: u32| usize::try_from(v).unwrap_or(0);
            Area {
                x0: at(x0),
                y0: at(y0),
                x1: at(x1),
                y1: at(y1),
            }
        })
}

/// A primitive's result, while something may still read it.
struct Done {
    image: Image,
    /// Its subregion on the layer.
    area: Area,
    /// Its subregion in user space.
    rect: [f32; 4],
    space: Space,
}

/// `rect` and `other`'s overlap, or nothing.
fn intersect(rect: [f32; 4], other: [f32; 4]) -> [f32; 4] {
    let x0 = rect[0].max(other[0]);
    let y0 = rect[1].max(other[1]);
    let x1 = (rect[0] + rect[2]).min(other[0] + other[2]);
    let y1 = (rect[1] + rect[3]).min(other[1] + other[3]);
    [x0, y0, (x1 - x0).max(0.0), (y1 - y0).max(0.0)]
}

/// The smallest rectangle holding both.
fn union(rect: [f32; 4], other: [f32; 4]) -> [f32; 4] {
    let x0 = rect[0].min(other[0]);
    let y0 = rect[1].min(other[1]);
    let x1 = (rect[0] + rect[2]).max(other[0] + other[2]);
    let y1 = (rect[1] + rect[3]).max(other[1] + other[3]);
    [x0, y0, x1 - x0, y1 - y0]
}

/// The whole pixels nearest `v`, held far inside `isize`.
fn whole_pixels(v: f32) -> isize {
    if !v.is_finite() {
        return 0;
    }
    #[allow(clippy::cast_possible_truncation, reason = "clamped to +-2^30 first")]
    let n = v.round().clamp(-1.0e9, 1.0e9) as isize;
    n
}

/// A count of whole pixels nearest `v`, not negative.
fn count_pixels(v: f32) -> usize {
    usize::try_from(whole_pixels(v)).unwrap_or(0)
}

/// `image`'s alpha alone: `SourceAlpha` of it.
fn alpha_of(image: &Image) -> Image {
    let mut out = Image::transparent(image.width(), image.height());
    for y in 0..image.height() {
        for (x, px) in image.row(y).iter().enumerate() {
            out.set(x, y, [0.0, 0.0, 0.0, px[3]]);
        }
    }
    out
}

impl SvgRenderer<'_> {
    /// `layer` -- what an element drew, straight sRGB bytes -- with the
    /// filters at `places` among the document's applied one after another;
    /// `false` where the element is not to be drawn at all.
    pub(super) fn apply_filters(
        &mut self,
        places: &[usize],
        layer: &mut [u8],
        target: &Target,
    ) -> bool {
        let doc = self.doc;
        for &place in places {
            let Some(def) = doc.filters.get(place) else {
                return false;
            };
            if !self.apply_filter(def, layer, target) {
                return false;
            }
        }
        true
    }

    /// One filter applied to `layer`.
    fn apply_filter(&mut self, def: &FilterDef, layer: &mut [u8], target: &Target) -> bool {
        let Some(region) = region(def, target.bbox) else {
            return false;
        };
        let Some(last) = def.primitives.len().checked_sub(1) else {
            return false;
        };
        let in_box = match def.primitive_units {
            Units::ObjectBoundingBox => {
                match target.bbox.filter(|&(_, _, bw, bh)| bw > 0.0 && bh > 0.0) {
                    Some(bbox) => Some(bbox),
                    None => return false,
                }
            }
            Units::UserSpaceOnUse => None,
        };
        let measure = Measure { target, in_box };
        let region_area = measure.area(region);
        if region_area.is_empty() {
            return false;
        }
        let source = Image::from_rgba8(target.width, target.height, layer).cropped(region_area);
        // What reads each result last, so it is let go of then.
        let mut last_read = vec![0usize; def.primitives.len()];
        for (i, p) in def.primitives.iter().enumerate() {
            for input in p.kind.inputs() {
                if let Input::Result(j) = input
                    && let Some(slot) = last_read.get_mut(j)
                {
                    *slot = (*slot).max(i);
                }
            }
        }
        let mut results: Vec<Option<Done>> = Vec::with_capacity(def.primitives.len());
        for (i, p) in def.primitives.iter().enumerate() {
            // Every primitive is paid for, one region's worth of pixels, out
            // of the drawing's budget for scratch work.
            let Some(left) = self.scratch_budget.checked_sub(region_area.pixels()) else {
                self.scratch_budget = 0;
                return false;
            };
            self.scratch_budget = left;
            let rect = self.subregion(p, region, &measure, &results);
            let area = measure.area(rect).intersect(region_area);
            let image = self.evaluate(p, area, rect, &source, &results, &measure);
            results.push(Some(Done {
                image,
                area,
                rect,
                space: p.space,
            }));
            // A result nothing reads after this one -- or nothing reads at
            // all -- is let go of, but for the last, which is the output.
            for (j, done) in results.iter_mut().enumerate() {
                if j != last && last_read.get(j).is_some_and(|&r| r <= i) {
                    *done = None;
                }
            }
        }
        let Some(Some(out)) = results.pop() else {
            return false;
        };
        let out = effects::convert(out.image, out.space, Space::Srgb).cropped(region_area);
        out.write_rgba8(layer);
        true
    }

    /// A primitive's subregion in user space: what it says of its `x`,
    /// `y`, `width` and `height`, the rest from its default -- the union of
    /// its inputs' subregions, or the filter's region where it reads a
    /// standard input or nothing, or tiles -- all cut to the region.
    fn subregion(
        &self,
        p: &Primitive,
        region: [f32; 4],
        measure: &Measure<'_>,
        results: &[Option<Done>],
    ) -> [f32; 4] {
        let inputs = p.kind.inputs();
        let from_results = !inputs.is_empty()
            && !matches!(p.kind, Kind::Tile { .. })
            && inputs.iter().all(|i| matches!(i, Input::Result(_)));
        let default = if from_results {
            inputs
                .iter()
                .filter_map(|i| match i {
                    Input::Result(j) => results.get(*j).and_then(Option::as_ref).map(|d| d.rect),
                    _ => None,
                })
                .reduce(union)
                .unwrap_or(region)
        } else {
            region
        };
        let [sx, sy, sw, sh] = p.subregion;
        let rect = [
            sx.map_or(default[0], |v| measure.x(v)),
            sy.map_or(default[1], |v| measure.y(v)),
            sw.map_or(default[2], |v| measure.across(v)),
            sh.map_or(default[3], |v| measure.down(v)),
        ];
        intersect(rect, region)
    }

    /// What one primitive makes over `area` -- its subregion on the layer,
    /// `rect` in user space -- in its own colour space.
    fn evaluate(
        &mut self,
        p: &Primitive,
        area: Area,
        rect: [f32; 4],
        source: &Image,
        results: &[Option<Done>],
        measure: &Measure<'_>,
    ) -> Image {
        let target = measure.target;
        let (w, h) = (target.width, target.height);
        let space = p.space;
        let get = |input: Input| -> Cow<'_, Image> { fetch(input, space, source, results, w, h) };
        let whole = Area::whole(w, h);
        match &p.kind {
            Kind::Blend {
                input,
                backdrop,
                mode,
            } => effects::blend(&get(*input), &get(*backdrop), *mode, area),
            Kind::ColorMatrix { input, matrix } => {
                effects::color_matrix(&get(*input), matrix, area)
            }
            Kind::ComponentTransfer { input, funcs } => {
                effects::component_transfer(&get(*input), funcs, area)
            }
            Kind::Composite { input, other, op } => {
                effects::composite(&get(*input), &get(*other), *op, area)
            }
            Kind::Convolve { input, kernel } => match kernel {
                Some(kernel) => effects::convolve(&get(*input), kernel, area),
                None => get(*input).into_owned().cropped(area),
            },
            Kind::Lighting {
                input,
                surface_scale,
                lighting,
                light,
                color,
            } => {
                let Some(light) = light else {
                    return Image::transparent(w, h);
                };
                let c = effects::color_in([color[0], color[1], color[2], 1.0], space);
                effects::light(
                    &get(*input),
                    *surface_scale,
                    *lighting,
                    &measure.light(light),
                    [c[0], c[1], c[2]],
                    area,
                )
            }
            Kind::Displace {
                input,
                map,
                scale,
                channels,
            } => {
                let scale = measure.extent((*scale, *scale));
                effects::displace(&get(*input), &get(*map), scale, *channels, area)
            }
            Kind::DropShadow {
                input,
                offset,
                deviation,
                color,
            } => {
                let shown = get(*input);
                let (sx, sy) = measure.extent(*deviation);
                let blurred = effects::blur(&alpha_of(&shown), sx, sy, whole);
                let (dx, dy) = measure.step(*offset);
                let moved = effects::offset(&blurred, whole_pixels(dx), whole_pixels(dy), whole);
                let flood = effects::flood(w, h, effects::color_in(*color, space), whole);
                let shadow = effects::composite(&flood, &moved, CompositeOp::In, whole);
                effects::merge(w, h, &[&shadow, &shown], area)
            }
            Kind::Flood { color } => effects::flood(w, h, effects::color_in(*color, space), area),
            Kind::Blur { input, deviation } => {
                let (sx, sy) = measure.extent(*deviation);
                effects::blur(&get(*input), sx, sy, area)
            }
            Kind::Image { content } => self.image_primitive(*content, target, space, area),
            Kind::Merge { inputs } => {
                let layers: Vec<Cow<'_, Image>> = inputs.iter().map(|i| get(*i)).collect();
                let refs: Vec<&Image> = layers.iter().map(AsRef::as_ref).collect();
                effects::merge(w, h, &refs, area)
            }
            Kind::Morphology {
                input,
                dilate,
                radius,
            } => {
                let (rx, ry) = measure.extent(*radius);
                effects::morphology(
                    &get(*input),
                    count_pixels(rx),
                    count_pixels(ry),
                    *dilate,
                    area,
                )
            }
            Kind::Offset { input, offset } => {
                let (dx, dy) = measure.step(*offset);
                effects::offset(&get(*input), whole_pixels(dx), whole_pixels(dy), area)
            }
            Kind::Tile { input } => {
                let source_area = match input {
                    Input::Result(j) => results
                        .get(*j)
                        .and_then(Option::as_ref)
                        .map_or(Area::EMPTY, |d| d.area),
                    _ => measure.area(rect),
                };
                effects::tile(&get(*input), source_area, area)
            }
            Kind::Turbulence {
                base,
                octaves,
                seed,
                fractal,
                stitch,
            } => {
                let Some(inverse) = target.to_layer.inverse() else {
                    return Image::transparent(w, h);
                };
                let in_box = measure.in_box;
                let to_noise = |px: f64, py: f64| {
                    #[allow(
                        clippy::cast_possible_truncation,
                        reason = "a pixel coordinate, far inside f32's range"
                    )]
                    let (ux, uy) = inverse.apply(px as f32, py as f32);
                    let (ux, uy) = match in_box {
                        Some((bx, by, bw, bh)) => ((ux - bx) / bw, (uy - by) / bh),
                        None => (ux, uy),
                    };
                    (f64::from(ux), f64::from(uy))
                };
                let tile = stitch.then(|| {
                    let [x, y, tw, th] = rect;
                    match in_box {
                        Some((bx, by, bw, bh)) => (
                            f64::from((x - bx) / bw),
                            f64::from((y - by) / bh),
                            f64::from(tw / bw),
                            f64::from(th / bh),
                        ),
                        None => (f64::from(x), f64::from(y), f64::from(tw), f64::from(th)),
                    }
                });
                let params = Turbulence {
                    base: (f64::from(base.0), f64::from(base.1)),
                    octaves: *octaves,
                    seed: *seed,
                    fractal: *fractal,
                    stitch: tile,
                };
                effects::turbulence(&params, w, h, to_noise, area)
            }
        }
    }

    /// What an `feImage` naming an element draws: the element, in the
    /// filtered element's user space, as a `<use>` would draw it -- nothing
    /// for a picture file, or for an element drawn inside itself.
    fn image_primitive(
        &mut self,
        content: Option<usize>,
        target: &Target,
        space: Space,
        area: Area,
    ) -> Image {
        let (w, h) = (target.width, target.height);
        let blank = || Image::transparent(w, h);
        let doc = self.doc;
        let Some((place, shown)) = content.and_then(|c| Some((c, doc.reused.get(c)?))) else {
            return blank();
        };
        if self.drawing.contains(&place) {
            return blank();
        }
        let (Ok(wu), Ok(hu)) = (u32::try_from(w), u32::try_from(h)) else {
            return blank();
        };
        let Some(mut scratch) = self.scratch(wu, hu) else {
            return blank();
        };
        scratch.drawing.push(place);
        scratch.render_node(shown, target.to_layer, &super::ResolvedStyle::default());
        self.absorb(&scratch);
        let image = Image::from_rgba8(w, h, &scratch.buffer);
        effects::convert(image, Space::Srgb, space).cropped(area)
    }
}

/// What `input` names, in `space`: borrowed where nothing need change.
fn fetch<'a>(
    input: Input,
    space: Space,
    source: &'a Image,
    results: &'a [Option<Done>],
    width: usize,
    height: usize,
) -> Cow<'a, Image> {
    match input {
        Input::SourceGraphic => {
            if space == Space::Srgb {
                Cow::Borrowed(source)
            } else {
                Cow::Owned(effects::convert(source.clone(), Space::Srgb, space))
            }
        }
        Input::SourceAlpha => Cow::Owned(alpha_of(source)),
        Input::Nothing => Cow::Owned(Image::transparent(width, height)),
        Input::Result(j) => match results.get(j).and_then(Option::as_ref) {
            Some(done) if done.space == space => Cow::Borrowed(&done.image),
            Some(done) => Cow::Owned(effects::convert(done.image.clone(), done.space, space)),
            None => Cow::Owned(Image::transparent(width, height)),
        },
    }
}

#[cfg(test)]
#[path = "filter_tests.rs"]
mod tests;
