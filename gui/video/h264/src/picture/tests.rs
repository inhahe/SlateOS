#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::panic)]

use super::*;

#[test]
fn a_frame_has_chroma_planes_rounded_up_and_samples_of_its_depth() {
    let f = AnyFrame::new(32, 16, 1, 1, 8).unwrap();
    let AnyFrame::Eight(b) = &f else {
        panic!("8 bits are bytes")
    };
    let sizes: Vec<_> = b
        .planes
        .iter()
        .map(|p| (p.width, p.height, p.stride, p.data.len()))
        .collect();
    assert_eq!(
        sizes,
        [(32, 16, 32, 512), (16, 8, 16, 128), (16, 8, 16, 128)]
    );

    let f = AnyFrame::new(33, 17, 1, 0, 10).unwrap();
    let AnyFrame::High(b) = &f else {
        panic!("10 bits are u16")
    };
    assert_eq!((b.planes[1].width, b.planes[1].height), (17, 17));
    assert_eq!(f.size(), (33, 17));
    assert_eq!(f.stride(1), 17);
}

#[test]
fn fill_sets_each_plane_to_its_value() {
    let mut f = AnyFrame::new(16, 16, 1, 1, 10).unwrap();
    f.fill([512, 256, 128]);
    let AnyFrame::High(b) = &f else { panic!() };
    assert!(b.planes[0].data.iter().all(|&v| v == 512));
    assert!(b.planes[1].data.iter().all(|&v| v == 256));
    assert!(b.planes[2].data.iter().all(|&v| v == 128));
}

#[test]
fn a_second_holder_shares_the_frame_but_not_the_picture_number() {
    let mut src = Picture {
        f: Some(Arc::new(AnyFrame::new(16, 16, 1, 1, 8).unwrap())),
        poc: 7,
        frame_num: 3,
        pic_id: 9,
        reference: 3,
        ..Picture::default()
    };
    src.props.format = PixelFormat::Yuv420(8);
    let mut dst = Picture {
        pic_id: 4,
        ..Picture::default()
    };
    dst.ref_from(&src);
    assert!(Arc::ptr_eq(
        dst.f.as_ref().unwrap(),
        src.f.as_ref().unwrap()
    ));
    assert_eq!(
        (dst.poc, dst.frame_num, dst.reference, dst.pic_id),
        (7, 3, 3, 4)
    );
    assert!(dst.same_shape(&src));

    // Let go, it is as new; a free slot is left as it is.
    dst.unref();
    assert!(!dst.in_use());
    assert_eq!((dst.poc, dst.pic_id), (0, 0));
    let mut free = Picture {
        reference: 2,
        ..Picture::default()
    };
    free.unref();
    assert_eq!(free.reference, 2);
}

#[test]
fn frames_differ_in_shape_by_size_depth_or_layout() {
    let pic = |w, depth, format| {
        let mut p = Picture {
            f: Some(Arc::new(AnyFrame::new(w, 16, 1, 1, depth).unwrap())),
            ..Picture::default()
        };
        p.props.format = format;
        p
    };
    let a = pic(16, 8, PixelFormat::Yuv420(8));
    assert!(a.same_shape(&pic(16, 8, PixelFormat::Yuv420(8))));
    assert!(!a.same_shape(&pic(32, 8, PixelFormat::Yuv420(8))));
    assert!(!a.same_shape(&pic(16, 8, PixelFormat::Yuvj420)));
    assert!(!a.same_shape(&Picture::default()));
    assert_eq!(PixelFormat::Yuvj420.non_j(), PixelFormat::Yuv420(8));
    assert_eq!(PixelFormat::Gbr(8).non_j(), PixelFormat::Gbr(8));
}
