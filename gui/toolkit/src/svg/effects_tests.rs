//! Tests for the filter primitives' pixel operations, each against what
//! Filter Effects 1 says it computes.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::float_cmp,
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]

use super::*;

/// An image `w` by `h` of one premultiplied colour.
fn solid(w: usize, h: usize, px: Pixel) -> Image {
    flood(w, h, px, Area::whole(w, h))
}

/// An image `w` by `h`, transparent but for an opaque white pixel at `(x, y)`.
fn dot(w: usize, h: usize, x: usize, y: usize) -> Image {
    let mut image = Image::transparent(w, h);
    image.set(x, y, [1.0; 4]);
    image
}

fn close(a: f32, b: f32, tolerance: f32) -> bool {
    (a - b).abs() <= tolerance
}

fn close_px(a: Pixel, b: Pixel, tolerance: f32) -> bool {
    a.iter().zip(&b).all(|(x, y)| close(*x, *y, tolerance))
}

/// The sum of every pixel's alpha.
fn mass(image: &Image) -> f32 {
    (0..image.height())
        .flat_map(|y| image.row(y).iter().map(|p| p[3]))
        .sum()
}

// ─── Areas and images ───────────────────────────────────────────────────────

#[test]
fn areas_meet_and_count() {
    let a = Area {
        x0: 1,
        y0: 1,
        x1: 5,
        y1: 4,
    };
    let b = Area {
        x0: 3,
        y0: 0,
        x1: 8,
        y1: 2,
    };
    assert_eq!(
        a.intersect(b),
        Area {
            x0: 3,
            y0: 1,
            x1: 5,
            y1: 2
        }
    );
    assert_eq!(a.pixels(), 12);
    assert!(a.contains(4, 3) && !a.contains(5, 3) && !a.contains(0, 1));
    let apart = Area {
        x0: 9,
        y0: 9,
        x1: 10,
        y1: 10,
    };
    assert_eq!(a.intersect(apart), Area::EMPTY);
    assert!(Area::EMPTY.is_empty());
}

/// **Bytes read in and written out are the same bytes** for opaque colour,
/// and transparent black stays so; colour under a little alpha survives
/// the division to within a step.
#[test]
fn bytes_survive_the_round_trip() {
    let bytes = [
        255u8, 0, 0, 255, 0, 128, 255, 255, 0, 0, 0, 0, 200, 100, 50, 128,
    ];
    let image = Image::from_rgba8(2, 2, &bytes);
    assert!(close_px(
        image.pixel(1, 0),
        [0.0, 128.0 / 255.0, 1.0, 1.0],
        1e-6
    ));
    let mut out = [0u8; 16];
    image.write_rgba8(&mut out);
    assert_eq!(out[..12], bytes[..12]);
    for (got, want) in out[12..].iter().zip(&bytes[12..]) {
        assert!(got.abs_diff(*want) <= 1, "{out:?}");
    }
}

#[test]
fn cropping_clears_outside_the_area() {
    let image = solid(3, 3, [1.0; 4]).cropped(Area {
        x0: 1,
        y0: 1,
        x1: 2,
        y1: 3,
    });
    assert_eq!(image.pixel(1, 1), [1.0; 4]);
    assert_eq!(image.pixel(1, 2), [1.0; 4]);
    assert_eq!(image.pixel(0, 1), [0.0; 4]);
    assert_eq!(image.pixel(2, 2), [0.0; 4]);
    // Outside the image is transparent, however far.
    assert_eq!(image.at(-5, 1), [0.0; 4]);
    assert_eq!(image.at(1, 99), [0.0; 4]);
}

// ─── Colour spaces ──────────────────────────────────────────────────────────

/// **sRGB and linear light convert both ways**, at the ends exactly, and a
/// mid-grey to the linear value the sRGB curve gives it.
#[test]
fn colour_spaces_convert_both_ways() {
    assert_eq!(srgb_to_linear(0.0), 0.0);
    assert!(close(srgb_to_linear(1.0), 1.0, 1e-6));
    assert!(close(srgb_to_linear(0.5), 0.214_04, 1e-4));
    for i in 0..=20 {
        let c = i as f32 / 20.0;
        assert!(close(linear_to_srgb(srgb_to_linear(c)), c, 1e-5), "{c}");
    }
    // Premultiplied colour converts its straight colour, keeping its alpha.
    let half = solid(1, 1, [0.25, 0.0, 0.5, 0.5]);
    let linear = convert(half.clone(), Space::Srgb, Space::LinearRgb);
    let px = linear.pixel(0, 0);
    assert!(close(px[3], 0.5, 1e-6));
    assert!(close(px[0], srgb_to_linear(0.5) * 0.5, 1e-6));
    assert!(close(px[2], 0.5, 1e-6));
    let back = convert(linear, Space::LinearRgb, Space::Srgb);
    assert!(close_px(back.pixel(0, 0), half.pixel(0, 0), 1e-5));
    // Nothing to do between a space and itself.
    assert_eq!(convert(half.clone(), Space::Srgb, Space::Srgb), half);
    // A straight colour premultiplied into either space.
    assert_eq!(
        color_in([1.0, 0.0, 0.5, 0.5], Space::Srgb),
        [0.5, 0.0, 0.25, 0.5]
    );
    let lin = color_in([1.0, 0.0, 0.5, 0.5], Space::LinearRgb);
    assert!(close(lin[2], srgb_to_linear(0.5) * 0.5, 1e-6));
}

// ─── Flood, offset, tile, merge ─────────────────────────────────────────────

#[test]
fn a_flood_fills_its_area_and_nothing_else() {
    let area = Area {
        x0: 1,
        y0: 0,
        x1: 3,
        y1: 1,
    };
    let image = flood(4, 2, [0.5, 0.0, 0.0, 0.5], area);
    assert_eq!(image.pixel(1, 0), [0.5, 0.0, 0.0, 0.5]);
    assert_eq!(image.pixel(2, 0), [0.5, 0.0, 0.0, 0.5]);
    assert_eq!(image.pixel(0, 0), [0.0; 4]);
    assert_eq!(image.pixel(1, 1), [0.0; 4]);
}

#[test]
fn an_offset_moves_the_image_and_leaves_transparency_behind() {
    let image = dot(5, 5, 1, 1);
    let moved = offset(&image, 2, 1, image.area());
    assert_eq!(moved.pixel(3, 2), [1.0; 4]);
    assert_eq!(moved.pixel(1, 1), [0.0; 4]);
    // Backwards, and off the edge: gone.
    let back = offset(&image, -3, 0, image.area());
    assert_eq!(mass(&back), 0.0);
}

/// **A tile repeats its source across the area, either way from it.**
#[test]
fn a_tile_repeats_its_source() {
    let mut image = Image::transparent(6, 6);
    image.set(2, 2, [1.0; 4]);
    let source = Area {
        x0: 2,
        y0: 2,
        x1: 4,
        y1: 4,
    };
    let tiled = tile(&image, source, image.area());
    for (x, y) in [(0, 0), (2, 2), (4, 4), (0, 4), (4, 0)] {
        assert_eq!(tiled.pixel(x, y), [1.0; 4], "({x}, {y})");
    }
    for (x, y) in [(1, 0), (3, 2), (5, 5)] {
        assert_eq!(tiled.pixel(x, y), [0.0; 4], "({x}, {y})");
    }
    // An empty source tiles nothing.
    assert_eq!(mass(&tile(&image, Area::EMPTY, image.area())), 0.0);
}

/// **A merge lays the later over the earlier.**
#[test]
fn a_merge_lays_later_over_earlier() {
    let red = solid(1, 1, [1.0, 0.0, 0.0, 1.0]);
    let half_blue = solid(1, 1, [0.0, 0.0, 0.5, 0.5]);
    let merged = merge(1, 1, &[&red, &half_blue], Area::whole(1, 1));
    assert!(close_px(merged.pixel(0, 0), [0.5, 0.0, 0.5, 1.0], 1e-6));
    let other = merge(1, 1, &[&half_blue, &red], Area::whole(1, 1));
    assert_eq!(other.pixel(0, 0), [1.0, 0.0, 0.0, 1.0]);
}

// ─── Blur ───────────────────────────────────────────────────────────────────

/// **A blur spreads a point without losing it**: what was there in all is
/// there in all after -- for a deviation small enough for the Gaussian
/// itself and large enough for the three boxes -- and spread evenly either
/// way.
#[test]
fn a_blur_spreads_without_losing() {
    for sigma in [0.5, 1.5, 2.0, 3.5, 6.0] {
        let image = dot(61, 61, 30, 30);
        let blurred = blur(&image, sigma, sigma, image.area());
        assert!(
            close(mass(&blurred), 1.0, 1e-3),
            "{sigma}: {}",
            mass(&blurred)
        );
        let at = |x: usize, y: usize| blurred.pixel(x, y)[3];
        assert!(close(at(29, 30), at(31, 30), 1e-6), "{sigma}");
        assert!(close(at(30, 27), at(30, 33), 1e-6), "{sigma}");
        assert!(at(30, 30) > at(31, 30), "{sigma}");
    }
}

/// **A deviation of 2 or more is the specification's three boxes**: for 4,
/// `d = floor(4 * 3 * sqrt(2 pi) / 4 + 0.5) = 8`, even, so two boxes of 8
/// half a pixel either side and one of 9. Along an axis their centre holds
/// 52/64 of 1/9 of a point -- squared for the two axes -- and they spread it
/// about as far as the Gaussian: the variance within a tenth of 4^2.
#[test]
fn a_large_deviation_is_the_specifications_three_boxes() {
    let sigma = 4.0;
    let image = dot(61, 61, 30, 30);
    let blurred = blur(&image, sigma, sigma, image.area());
    let axis = 52.0 / 64.0 / 9.0;
    let got = blurred.pixel(30, 30)[3];
    assert!(close(got, axis * axis, 1e-6), "{got} for {}", axis * axis);
    // The spread along one row's sum.
    let variance: f32 = (0..61)
        .map(|x| {
            let column: f32 = (0..61).map(|y| blurred.pixel(x, y)[3]).sum();
            let d = x as f32 - 30.0;
            d * d * column
        })
        .sum();
    assert!((variance / (sigma * sigma) - 1.0).abs() < 0.1, "{variance}");
}

/// **A deviation of nought blurs nothing along its axis.**
#[test]
fn a_small_deviation_is_the_gaussian_itself() {
    // Under 2, the Gaussian sampled three deviations either side and
    // normalised: for 1, the centre keeps 1 / sum(e^(-k^2 / 2)) of a point
    // along each axis.
    let image = dot(21, 21, 10, 10);
    let blurred = blur(&image, 1.0, 1.0, image.area());
    let total: f32 = (-3i32..=3).map(|k| (-((k * k) as f32) / 2.0).exp()).sum();
    let axis = 1.0 / total;
    assert!(close(blurred.pixel(10, 10)[3], axis * axis, 1e-6));
}

#[test]
fn a_blur_along_one_axis_stays_on_it() {
    let image = dot(21, 21, 10, 10);
    let across = blur(&image, 2.0, 0.0, image.area());
    assert_eq!(across.pixel(10, 9)[3], 0.0);
    assert!(across.pixel(12, 10)[3] > 0.0);
    let none = blur(&image, 0.0, 0.0, image.area());
    assert_eq!(none, image);
}

/// **What a blur makes outside its area is cleared**, though it reads from
/// beyond it.
#[test]
fn a_blur_writes_only_its_area() {
    let image = dot(11, 11, 5, 5);
    let area = Area {
        x0: 0,
        y0: 0,
        x1: 6,
        y1: 11,
    };
    let blurred = blur(&image, 1.0, 1.0, area);
    assert_eq!(blurred.pixel(6, 5), [0.0; 4]);
    assert!(blurred.pixel(5, 5)[3] > 0.0);
}

// ─── Morphology ─────────────────────────────────────────────────────────────

/// **Dilating a point makes a square of it, and eroding the square takes
/// it back to the point**; a window reaching past the edge erodes to nothing.
#[test]
fn dilation_and_erosion_undo_each_other_on_a_square() {
    let image = dot(9, 9, 4, 4);
    let grown = morphology(&image, 2, 1, true, image.area());
    for y in 0..9 {
        for x in 0..9 {
            let inside = (2..=6).contains(&x) && (3..=5).contains(&y);
            assert_eq!(
                grown.pixel(x, y)[3],
                if inside { 1.0 } else { 0.0 },
                "({x}, {y})"
            );
        }
    }
    let shrunk = morphology(&grown, 2, 1, false, grown.area());
    assert_eq!(shrunk, image);
    // The whole image opaque: erosion clears what is within the radius of
    // its edge, since past it is transparent.
    let full = solid(5, 5, [1.0; 4]);
    let eroded = morphology(&full, 1, 1, false, full.area());
    assert_eq!(eroded.pixel(2, 2)[3], 1.0);
    assert_eq!(eroded.pixel(0, 2)[3], 0.0);
    assert_eq!(eroded.pixel(2, 4)[3], 0.0);
    // A radius of nought changes nothing.
    assert_eq!(morphology(&image, 0, 0, true, image.area()), image);
}

/// **Each channel takes its own extreme**, so a window over red and blue
/// dilates to purple.
#[test]
fn morphology_works_per_channel() {
    let mut image = Image::transparent(3, 1);
    image.set(0, 0, [1.0, 0.0, 0.0, 1.0]);
    image.set(2, 0, [0.0, 0.0, 1.0, 1.0]);
    let grown = morphology(&image, 1, 0, true, image.area());
    assert_eq!(grown.pixel(1, 0), [1.0, 0.0, 1.0, 1.0]);
}

// ─── Compositing and blending ───────────────────────────────────────────────

/// **Each Porter-Duff operator, on half-covered pixels**: source `a`, half
/// red; destination `b`, half blue.
#[test]
fn the_composite_operators_weigh_each_side_by_the_others_alpha() {
    // Alphas that differ, so weighing a side by its own alpha cannot pass
    // for weighing it by the other's: half red at half alpha over blue at
    // three quarters.
    let a = solid(1, 1, [0.5, 0.0, 0.0, 0.5]);
    let b = solid(1, 1, [0.0, 0.0, 0.75, 0.75]);
    let whole = Area::whole(1, 1);
    let px = |op| composite(&a, &b, op, whole).pixel(0, 0);
    assert!(close_px(
        px(CompositeOp::Over),
        [0.5, 0.0, 0.375, 0.875],
        1e-6
    ));
    assert!(close_px(
        px(CompositeOp::In),
        [0.375, 0.0, 0.0, 0.375],
        1e-6
    ));
    assert!(close_px(
        px(CompositeOp::Out),
        [0.125, 0.0, 0.0, 0.125],
        1e-6
    ));
    assert!(close_px(
        px(CompositeOp::Atop),
        [0.375, 0.0, 0.375, 0.75],
        1e-6
    ));
    assert!(close_px(
        px(CompositeOp::Xor),
        [0.125, 0.0, 0.375, 0.5],
        1e-6
    ));
    assert!(close_px(
        px(CompositeOp::Lighter),
        [0.5, 0.0, 0.75, 1.0],
        1e-6
    ));
}

#[test]
fn the_composite_operators_are_porter_duffs() {
    let a = solid(1, 1, [0.5, 0.0, 0.0, 0.5]);
    let b = solid(1, 1, [0.0, 0.0, 0.5, 0.5]);
    let whole = Area::whole(1, 1);
    let px = |op| composite(&a, &b, op, whole).pixel(0, 0);
    assert!(close_px(
        px(CompositeOp::Over),
        [0.5, 0.0, 0.25, 0.75],
        1e-6
    ));
    assert!(close_px(px(CompositeOp::In), [0.25, 0.0, 0.0, 0.25], 1e-6));
    assert!(close_px(px(CompositeOp::Out), [0.25, 0.0, 0.0, 0.25], 1e-6));
    assert!(close_px(
        px(CompositeOp::Atop),
        [0.25, 0.0, 0.25, 0.5],
        1e-6
    ));
    assert!(close_px(px(CompositeOp::Xor), [0.25, 0.0, 0.25, 0.5], 1e-6));
    assert!(close_px(
        px(CompositeOp::Lighter),
        [0.5, 0.0, 0.5, 1.0],
        1e-6
    ));
    // k2 = 1: the source alone; k4 = 1: everything white and opaque;
    // results held to a valid premultiplied colour.
    assert!(close_px(
        px(CompositeOp::Arithmetic([0.0, 1.0, 0.0, 0.0])),
        [0.5, 0.0, 0.0, 0.5],
        1e-6
    ));
    assert!(close_px(
        px(CompositeOp::Arithmetic([0.0, 0.0, 0.0, 1.0])),
        [1.0; 4],
        1e-6
    ));
    let clamped = px(CompositeOp::Arithmetic([0.0, 3.0, 0.0, 0.0]));
    assert!(
        clamped.iter().take(3).all(|c| *c <= clamped[3]),
        "{clamped:?}"
    );
}

/// **Normal blending is source-over, and each separable mode mixes the
/// colours as Compositing and Blending says** -- shown on opaque pixels,
/// where the result is the mix itself.
#[test]
fn the_separable_blend_modes_mix_as_specified() {
    let src = solid(1, 1, [0.25, 0.5, 1.0, 1.0]);
    let dst = solid(1, 1, [0.5, 0.5, 0.25, 1.0]);
    let whole = Area::whole(1, 1);
    let px = |mode| blend(&src, &dst, mode, whole).pixel(0, 0);
    assert!(close_px(px(BlendMode::Normal), [0.25, 0.5, 1.0, 1.0], 1e-6));
    assert!(close_px(
        px(BlendMode::Multiply),
        [0.125, 0.25, 0.25, 1.0],
        1e-6
    ));
    assert!(close_px(
        px(BlendMode::Screen),
        [0.625, 0.75, 1.0, 1.0],
        1e-6
    ));
    assert!(close_px(
        px(BlendMode::Darken),
        [0.25, 0.5, 0.25, 1.0],
        1e-6
    ));
    assert!(close_px(px(BlendMode::Lighten), [0.5, 0.5, 1.0, 1.0], 1e-6));
    assert!(close_px(
        px(BlendMode::Difference),
        [0.25, 0.0, 0.75, 1.0],
        1e-6
    ));
    assert!(close_px(
        px(BlendMode::Exclusion),
        [0.5, 0.5, 0.75, 1.0],
        1e-6
    ));
    // Hard light: the source decides -- multiply under half, screen above.
    assert!(close_px(
        px(BlendMode::HardLight),
        [0.25, 0.5, 1.0, 1.0],
        1e-6
    ));
    // Overlay is hard light with the roles swapped: the backdrop decides.
    assert!(close_px(
        px(BlendMode::Overlay),
        [0.25, 0.5, 0.5, 1.0],
        1e-6
    ));
    // Dodge divides the backdrop by what the source leaves of white, held to
    // one; burn is its mirror -- each at its ends and between.
    assert!(close_px(
        px(BlendMode::ColorDodge),
        [0.5 / 0.75, 1.0, 1.0, 1.0],
        1e-6
    ));
    assert!(close_px(
        px(BlendMode::ColorBurn),
        [0.0, 0.0, 0.25, 1.0],
        1e-6
    ));
    // A transparent source leaves the backdrop, whatever the mode.
    let clear = Image::transparent(1, 1);
    for mode in [BlendMode::Multiply, BlendMode::Hue, BlendMode::Difference] {
        assert_eq!(
            blend(&clear, &dst, mode, whole).pixel(0, 0),
            dst.pixel(0, 0)
        );
    }
    assert!(BlendMode::named("soft-light").is_some());
    assert!(BlendMode::named("plus").is_none());
}

/// **Soft light follows its two-part formula**: under half, a source darkens
/// by `(1 - 2 Cs) Cb (1 - Cb)`; over half it lightens toward a cubic of the
/// backdrop below a quarter and its square root above.
#[test]
fn soft_light_follows_its_formula() {
    let whole = Area::whole(1, 1);
    let mix = |cb: f32, cs: f32| {
        let src = solid(1, 1, [cs, cs, cs, 1.0]);
        let dst = solid(1, 1, [cb, cb, cb, 1.0]);
        blend(&src, &dst, BlendMode::SoftLight, whole).pixel(0, 0)[0]
    };
    assert!(close(mix(0.5, 0.25), 0.375, 1e-6));
    // Cb 0.1: D = ((16 * 0.1 - 12) * 0.1 + 4) * 0.1 = 0.296.
    assert!(close(mix(0.1, 0.75), 0.1 + 0.5 * (0.296 - 0.1), 1e-5));
    // Cb 0.64: D = sqrt(0.64) = 0.8.
    assert!(close(mix(0.64, 0.75), 0.64 + 0.5 * (0.8 - 0.64), 1e-5));
}

/// **Hue takes the source's hue at the backdrop's saturation and
/// luminosity**: (0.8, 0.5, 0.2) at the backdrop's saturation of 0.4 is
/// (0.4, 0.2, 0), raised to its luminosity of 0.362.
#[test]
fn hue_takes_the_sources_hue() {
    let src = solid(1, 1, [0.8, 0.5, 0.2, 1.0]);
    let dst = solid(1, 1, [0.2, 0.4, 0.6, 1.0]);
    let out = blend(&src, &dst, BlendMode::Hue, Area::whole(1, 1)).pixel(0, 0);
    assert!(close_px(out, [0.524, 0.324, 0.124, 1.0], 1e-4), "{out:?}");
}

/// **The non-separable modes trade hue, saturation and luminosity**: the
/// luminosity mode keeps the backdrop's colour at the source's luminosity,
/// and the colour mode the reverse.
#[test]
fn the_non_separable_modes_trade_luminosity_and_colour() {
    let grey = solid(1, 1, [0.5, 0.5, 0.5, 1.0]);
    let red = solid(1, 1, [1.0, 0.0, 0.0, 1.0]);
    let whole = Area::whole(1, 1);
    let lum_of = |p: Pixel| 0.3 * p[0] + 0.59 * p[1] + 0.11 * p[2];
    // Grey's luminosity on red's colour: as bright as the grey.
    let lumed = blend(&grey, &red, BlendMode::Luminosity, whole).pixel(0, 0);
    assert!(close(lum_of(lumed), 0.5, 1e-4), "{lumed:?}");
    assert!(
        lumed[0] > lumed[1] && close(lumed[1], lumed[2], 1e-6),
        "{lumed:?}"
    );
    // Red's colour on grey's luminosity, the same.
    let coloured = blend(&red, &grey, BlendMode::Color, whole).pixel(0, 0);
    assert!(close(lum_of(coloured), 0.5, 1e-4), "{coloured:?}");
    // Saturation from a grey source leaves a grey.
    let flat = blend(&grey, &red, BlendMode::Saturation, whole).pixel(0, 0);
    assert!(
        close(flat[0], flat[1], 1e-6) && close(flat[1], flat[2], 1e-6),
        "{flat:?}"
    );
}

// ─── Colour matrices and transfer functions ─────────────────────────────────

#[test]
fn colour_matrices_act_on_straight_colour() {
    let half_red = solid(1, 1, [0.5, 0.0, 0.0, 0.5]);
    let whole = Area::whole(1, 1);
    assert!(close_px(
        color_matrix(&half_red, &IDENTITY_MATRIX, whole).pixel(0, 0),
        [0.5, 0.0, 0.0, 0.5],
        1e-6
    ));
    // Desaturated fully: grey at red's weight, on the straight colour.
    let grey = color_matrix(&half_red, &saturate_matrix(0.0), whole).pixel(0, 0);
    assert!(
        close_px(grey, [0.213 * 0.5, 0.213 * 0.5, 0.213 * 0.5, 0.5], 1e-6),
        "{grey:?}"
    );
    // A turn of nought, or of a whole circle, is no turn.
    for degrees in [0.0, 360.0] {
        let turned = color_matrix(&half_red, &hue_rotate_matrix(degrees), whole).pixel(0, 0);
        assert!(
            close_px(turned, [0.5, 0.0, 0.0, 0.5], 1e-4),
            "{degrees}: {turned:?}"
        );
    }
    // Luminance to alpha: no colour, alpha the luminance.
    let white = solid(1, 1, [1.0; 4]);
    let lum = color_matrix(&white, &LUMINANCE_TO_ALPHA, whole).pixel(0, 0);
    assert!(close_px(lum, [0.0, 0.0, 0.0, 1.0], 1e-4), "{lum:?}");
}

/// **Each transfer function as the specification draws it**, at its ends
/// and between.
#[test]
fn transfer_functions_map_channels() {
    let table = Transfer::Table(vec![0.0, 1.0, 0.0]);
    assert_eq!(table.apply(0.0), 0.0);
    assert!(close(table.apply(0.25), 0.5, 1e-6));
    assert!(close(table.apply(0.5), 1.0, 1e-6));
    assert!(close(table.apply(1.0), 0.0, 1e-6));
    let discrete = Transfer::Discrete(vec![0.2, 0.8]);
    assert_eq!(discrete.apply(0.49), 0.2);
    assert_eq!(discrete.apply(0.5), 0.8);
    assert_eq!(discrete.apply(1.0), 0.8);
    let linear = Transfer::Linear {
        slope: 2.0,
        intercept: -0.5,
    };
    assert_eq!(linear.apply(0.5), 0.5);
    assert_eq!(linear.apply(0.0), 0.0, "held to 0 to 1");
    let gamma = Transfer::Gamma {
        amplitude: 1.0,
        exponent: 2.0,
        offset: 0.0,
    };
    assert!(close(gamma.apply(0.5), 0.25, 1e-6));
    // An empty table is the identity.
    assert_eq!(Transfer::Table(Vec::new()).apply(0.3), 0.3);
    assert_eq!(Transfer::Discrete(Vec::new()).apply(0.3), 0.3);
    // On an image: alpha through its own function, colour on straight values.
    let half_white = solid(1, 1, [0.5, 0.5, 0.5, 0.5]);
    let funcs = [
        Transfer::Identity,
        Transfer::Linear {
            slope: 0.0,
            intercept: 0.0,
        },
        Transfer::Identity,
        Transfer::Linear {
            slope: 0.0,
            intercept: 1.0,
        },
    ];
    let out = component_transfer(&half_white, &funcs, Area::whole(1, 1)).pixel(0, 0);
    assert!(close_px(out, [1.0, 0.0, 1.0, 1.0], 1e-6), "{out:?}");
}

// ─── Convolution ────────────────────────────────────────────────────────────

fn kernel3(values: [f32; 9], edge: EdgeMode) -> Kernel {
    let sum: f32 = values.iter().sum();
    Kernel {
        order_x: 3,
        order_y: 3,
        values: values.to_vec(),
        divisor: if sum == 0.0 { 1.0 } else { sum },
        bias: 0.0,
        target_x: 1,
        target_y: 1,
        edge,
        preserve_alpha: false,
    }
}

/// **The kernel is turned half a turn**, as convolution and the
/// specification's formula have it: a one in its top-left corner reads the
/// pixel below and to the right.
#[test]
fn a_convolution_turns_its_kernel() {
    let image = dot(5, 5, 3, 3);
    let identity = kernel3(
        [0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0],
        EdgeMode::None,
    );
    assert_eq!(convolve(&image, &identity, image.area()), image);
    let corner = kernel3(
        [1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
        EdgeMode::None,
    );
    let out = convolve(&image, &corner, image.area());
    assert_eq!(out.pixel(2, 2), [1.0; 4]);
    assert_eq!(out.pixel(3, 3), [0.0; 4]);
}

/// **Each edge mode reads past the edge as it says**: the nearest pixel,
/// the other side, or nothing.
#[test]
fn convolution_edges_read_as_their_mode_says() {
    let mut image = Image::transparent(3, 1);
    image.set(0, 0, [1.0; 4]);
    // Reads the pixel to the left of each: a one at the kernel's right end,
    // turned, reads leftwards.
    let left = |edge| Kernel {
        order_x: 3,
        order_y: 1,
        values: vec![0.0, 0.0, 1.0],
        divisor: 1.0,
        bias: 0.0,
        target_x: 1,
        target_y: 0,
        edge,
        preserve_alpha: false,
    };
    let at0 = |edge| convolve(&image, &left(edge), image.area()).pixel(0, 0)[3];
    assert_eq!(at0(EdgeMode::None), 0.0);
    assert_eq!(at0(EdgeMode::Duplicate), 1.0);
    assert_eq!(
        at0(EdgeMode::Wrap),
        0.0,
        "wraps to the transparent right end"
    );
    let mut right_lit = Image::transparent(3, 1);
    right_lit.set(2, 0, [1.0; 4]);
    assert_eq!(
        convolve(&right_lit, &left(EdgeMode::Wrap), right_lit.area()).pixel(0, 0)[3],
        1.0
    );
}

/// **With alpha preserved, the kernel works on straight colour and the
/// alpha stays; without, the bias is added in proportion to the alpha.**
#[test]
fn preserving_alpha_keeps_it() {
    let half_white = solid(3, 3, [0.5, 0.5, 0.5, 0.5]);
    let mut k = kernel3(
        [0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0],
        EdgeMode::Duplicate,
    );
    k.preserve_alpha = true;
    k.divisor = 2.0;
    let out = convolve(&half_white, &k, half_white.area()).pixel(1, 1);
    assert!(close_px(out, [0.25, 0.25, 0.25, 0.5], 1e-6), "{out:?}");
    k.preserve_alpha = false;
    k.divisor = 1.0;
    k.bias = 0.5;
    let out = convolve(&half_white, &k, half_white.area()).pixel(1, 1);
    assert!(close_px(out, [1.0, 1.0, 1.0, 1.0], 1e-6), "{out:?}");
    // Where the result is not opaque, the bias each colour gains is the
    // bias times the result's alpha: 0.25 more alpha, 0.25 * 0.75 more of
    // each colour.
    k.bias = 0.25;
    let out = convolve(&half_white, &k, half_white.area()).pixel(1, 1);
    assert!(
        close_px(out, [0.6875, 0.6875, 0.6875, 0.75], 1e-6),
        "{out:?}"
    );
}

// ─── Displacement ───────────────────────────────────────────────────────────

/// **A map of mid-grey moves nothing; a map of full red moves by half the
/// scale** along the axis its red drives.
#[test]
fn displacement_moves_by_the_maps_channels() {
    let image = dot(9, 1, 6, 0);
    let still = solid(9, 1, [0.5, 0.5, 0.5, 1.0]);
    assert_eq!(
        displace(&image, &still, (4.0, 4.0), (0, 1), image.area()),
        image
    );
    let red = solid(9, 1, [1.0, 0.0, 0.0, 1.0]);
    let moved = displace(&image, &red, (4.0, 0.0), (0, 1), image.area());
    // Each pixel reads two to its right: the dot shows two to its left.
    assert_eq!(moved.pixel(4, 0), [1.0; 4]);
    assert_eq!(moved.pixel(6, 0), [0.0; 4]);
}

// ─── Lighting ───────────────────────────────────────────────────────────────

/// **A flat surface lit from straight above is lit fully, and from the
/// horizon not at all**; a specular highlight straight above is the light's
/// colour with its alpha the brightest channel.
#[test]
fn a_flat_surface_is_lit_by_the_angle_of_the_light() {
    let flat = solid(5, 5, [1.0; 4]);
    let area = flat.area();
    let overhead = Light::Distant {
        azimuth: 0.0,
        elevation: 90.0,
    };
    let lit = light(
        &flat,
        1.0,
        Lighting::Diffuse { constant: 1.0 },
        &overhead,
        [1.0, 0.5, 0.0],
        area,
    );
    assert!(close_px(lit.pixel(2, 2), [1.0, 0.5, 0.0, 1.0], 1e-5));
    let horizon = Light::Distant {
        azimuth: 0.0,
        elevation: 0.0,
    };
    let dark = light(
        &flat,
        1.0,
        Lighting::Diffuse { constant: 1.0 },
        &horizon,
        [1.0; 3],
        area,
    );
    assert!(close_px(dark.pixel(2, 2), [0.0, 0.0, 0.0, 1.0], 1e-5));
    let shine = light(
        &flat,
        1.0,
        Lighting::Specular {
            constant: 1.0,
            exponent: 10.0,
        },
        &overhead,
        [0.5, 0.25, 0.0],
        area,
    );
    assert!(close_px(shine.pixel(2, 2), [0.5, 0.25, 0.0, 0.5], 1e-5));
    // From 45 degrees up, the highlight is the normal against the vector
    // half-way between the light and the eye -- 22.5 degrees from it -- to
    // the tenth power; not the light itself, which would be 45 degrees.
    let raised = Light::Distant {
        azimuth: 0.0,
        elevation: 45.0,
    };
    let glint = light(
        &flat,
        1.0,
        Lighting::Specular {
            constant: 1.0,
            exponent: 10.0,
        },
        &raised,
        [1.0; 3],
        area,
    );
    let half_way = core::f32::consts::FRAC_PI_8.cos().powi(10);
    assert!(
        close(glint.pixel(2, 2)[0], half_way, 1e-4),
        "{:?}",
        glint.pixel(2, 2)
    );
}

/// **A slope faces the light or away from it**: a ramp rising to the right,
/// lit from the right, is brighter than lit from the left.
#[test]
fn a_slope_facing_the_light_is_brighter() {
    let mut ramp = Image::transparent(5, 5);
    for y in 0..5 {
        for x in 0..5 {
            let a = x as f32 / 4.0;
            ramp.set(x, y, [a, a, a, a]);
        }
    }
    let lit_from = |azimuth: f32| {
        let light_at = Light::Distant {
            azimuth,
            elevation: 30.0,
        };
        light(
            &ramp,
            5.0,
            Lighting::Diffuse { constant: 1.0 },
            &light_at,
            [1.0; 3],
            ramp.area(),
        )
        .pixel(2, 2)[0]
    };
    // Azimuth 0 points along +x; a surface rising toward +x faces -x.
    assert!(
        lit_from(180.0) > lit_from(0.0),
        "{} vs {}",
        lit_from(180.0),
        lit_from(0.0)
    );
}

/// **A spot light lights nothing outside its cone**, and a point light
/// overhead lights the point under it fully.
#[test]
fn a_spot_lights_only_its_cone() {
    let flat = solid(21, 21, [1.0; 4]);
    let spot = Light::Spot {
        x: 10.5,
        y: 10.5,
        z: 10.0,
        at: (10.5, 10.5, 0.0),
        exponent: 1.0,
        cone: Some(10.0),
    };
    let lit = light(
        &flat,
        1.0,
        Lighting::Diffuse { constant: 1.0 },
        &spot,
        [1.0; 3],
        flat.area(),
    );
    assert!(lit.pixel(10, 10)[0] > 0.9);
    assert_eq!(lit.pixel(0, 0)[0], 0.0);
    let point = Light::Point {
        x: 10.5,
        y: 10.5,
        z: 10.0,
    };
    let lit = light(
        &flat,
        1.0,
        Lighting::Diffuse { constant: 1.0 },
        &point,
        [1.0; 3],
        flat.area(),
    );
    assert!(close(lit.pixel(10, 10)[0], 1.0, 1e-5));
    assert!(lit.pixel(0, 0)[0] < lit.pixel(10, 10)[0]);
}

// ─── Turbulence ─────────────────────────────────────────────────────────────

fn noise(seed: i64, fractal: bool, stitch: Option<(f64, f64, f64, f64)>) -> Image {
    let params = Turbulence {
        base: (0.05, 0.05),
        octaves: 3,
        seed,
        fractal,
        stitch,
    };
    turbulence(&params, 64, 64, |x, y| (x, y), Area::whole(64, 64))
}

/// **Perlin noise is nought on its lattice**: at the origin every octave is
/// nought, so fractal noise is mid-grey at half alpha and turbulence clear.
#[test]
fn noise_is_nought_on_the_lattice() {
    let fractal = noise(0, true, None);
    assert!(
        close_px(fractal.pixel(0, 0), [0.25, 0.25, 0.25, 0.5], 1e-6),
        "{:?}",
        fractal.pixel(0, 0)
    );
    let turbulent = noise(0, false, None);
    assert_eq!(turbulent.pixel(0, 0), [0.0; 4]);
}

/// **The same seed makes the same noise, another seed other noise**, and
/// a seed is the same seed however it is written below nought.
#[test]
fn each_octave_adds_half_the_last() {
    // Two octaves are one and the noise at twice the frequency, halved.
    let lattice = Lattice::new(9);
    let one = Turbulence {
        base: (0.07, 0.05),
        octaves: 1,
        seed: 9,
        fractal: true,
        stitch: None,
    };
    let two = Turbulence {
        octaves: 2,
        ..one.clone()
    };
    for point in [(3.3, 7.1), (12.6, 0.4), (25.0, 31.7)] {
        let next = lattice.noise2(1, [point.0 * 0.07 * 2.0, point.1 * 0.05 * 2.0], None);
        let gained = two.at(&lattice, 1, point) - one.at(&lattice, 1, point);
        assert!((gained - next / 2.0).abs() < 1e-12, "{point:?}");
        assert!(next.abs() > 1e-6, "a point where the octave adds something");
    }
}

#[test]
fn noise_follows_its_seed() {
    assert_eq!(noise(7, true, None), noise(7, true, None));
    assert_ne!(noise(7, true, None), noise(8, true, None));
    // Every value a colour.
    let image = noise(3, false, None);
    for y in 0..64 {
        for px in image.row(y) {
            assert!(px.iter().all(|c| (0.0..=1.0).contains(c)), "{px:?}");
            assert!(px[0] <= px[3] + 1e-6);
        }
    }
    assert_eq!(setup_seed(0), 1);
    assert_eq!(setup_seed(-5), 6);
    assert_eq!(setup_seed(i64::MAX), RAND_M - 1);
}

/// **Stitched noise wraps at its tile**: a column and the one a tile's
/// width to its right are the same.
#[test]
fn stitched_noise_wraps_at_its_tile() {
    let image = noise(5, true, Some((0.0, 0.0, 40.0, 40.0)));
    for y in [0usize, 13, 27] {
        let left = image.pixel(3, y);
        let right = image.pixel(43, y);
        assert!(close_px(left, right, 1e-4), "{y}: {left:?} {right:?}");
    }
    // Unstitched, the two are not the same.
    let free = noise(5, true, None);
    assert!(!close_px(free.pixel(3, 13), free.pixel(43, 13), 1e-4));
}

/// **The generator is Park and Miller's**: from a seed of 1, its known
/// sequence.
#[test]
fn the_generator_is_the_minimal_standard() {
    let mut seed = 1;
    let mut seen = Vec::new();
    for _ in 0..3 {
        seed = random(seed);
        seen.push(seed);
    }
    assert_eq!(seen, [16_807, 282_475_249, 1_622_650_073]);
}
