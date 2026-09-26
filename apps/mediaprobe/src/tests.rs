use super::*;
use crate::testing::{
    Mp4Track, avi_file, avi_list, avih, bitmap, chunk, ebml_header, element, float_element,
    full_box, mkv_audio, mkv_segment, mkv_video, mp4_box, mp4_trak, mvhd, strh, uint_element,
    wave_format,
};
use std::io::Cursor;

fn read(bytes: &[u8]) -> Probe {
    probe(&mut Cursor::new(bytes.to_vec())).expect("read")
}

fn close(a: Option<f64>, b: f64) -> bool {
    a.is_some_and(|a| (a - b).abs() < 1e-3)
}

/// A video `Mp4Track` of `width` by `height`, `samples` frames over
/// `duration` ticks of `timescale`.
fn video_trak(width: u16, height: u16, timescale: u32, duration: u32, samples: u32) -> Vec<u8> {
    mp4_trak(&Mp4Track {
        handler: b"vide",
        entry: b"avc1",
        width,
        height,
        channels: 0,
        sample_rate: 0,
        timescale,
        duration,
        samples,
        language: "eng",
        enabled: true,
        object_type: None,
    })
}

fn mp4_of(moov_children: &[Vec<u8>]) -> Vec<u8> {
    let mut file = mp4_box(b"ftyp", b"isom\0\0\x02\0isom");
    file.extend(mp4_box(b"moov", &moov_children.concat()));
    file
}

// ============================================================================
// MP4
// ============================================================================

#[test]
fn an_mp4_is_read_for_its_length_its_picture_and_its_sound() {
    let p = read(&testing::mp4(1920, 1080, 10, 25));
    assert_eq!(p.container, Container::Mp4);
    assert_eq!(p.duration_secs, Some(10.0));
    assert_eq!(p.tracks.len(), 2);
    let v = p.first(Kind::Video).expect("a picture");
    assert_eq!(v.codec, Codec::H264);
    assert_eq!((v.width, v.height), (Some(1920), Some(1080)));
    assert!(close(v.frame_rate, 25.0), "{:?}", v.frame_rate);
    assert_eq!(v.language.as_deref(), Some("eng"));
    assert!(v.default);
    let a = p.first(Kind::Audio).expect("sound");
    assert_eq!(a.codec, Codec::Aac);
    assert_eq!((a.sample_rate, a.channels), (Some(48_000), Some(2)));
    assert_eq!(a.frame_rate, None, "sound has no frame rate");
}

#[test]
fn a_quicktime_file_is_named_so_even_without_ftyp() {
    let mut qt = mp4_box(b"ftyp", b"qt  \0\0\x02\0qt  ");
    qt.extend(mp4_box(b"moov", &mvhd(600, 1200)));
    let p = read(&qt);
    assert_eq!(p.container, Container::QuickTime);
    assert_eq!(p.duration_secs, Some(2.0));
    // An old QuickTime file: its first box is `moov` itself.
    let old = mp4_box(b"moov", &mvhd(600, 1800));
    let p = read(&old);
    assert_eq!(p.container, Container::QuickTime);
    assert_eq!(p.duration_secs, Some(3.0));
    // "moov" after a size too small to be a box is not one.
    assert_eq!(Container::detect(b"\0\0\0\x03moov"), Container::Unknown);
}

#[test]
fn an_mp4a_entry_is_mp3_when_its_descriptor_says_so() {
    let audio = |object_type| {
        mp4_trak(&Mp4Track {
            handler: b"soun",
            entry: b"mp4a",
            width: 0,
            height: 0,
            channels: 1,
            sample_rate: 44_100,
            timescale: 44_100,
            duration: 44_100,
            samples: 38,
            language: "eng",
            enabled: true,
            object_type,
        })
    };
    let p = read(&mp4_of(&[mvhd(1000, 1000), audio(Some(0x6B))]));
    assert_eq!(p.tracks[0].codec, Codec::Mp3);
    let p = read(&mp4_of(&[mvhd(1000, 1000), audio(Some(0x40))]));
    assert_eq!(p.tracks[0].codec, Codec::Aac);
    let p = read(&mp4_of(&[mvhd(1000, 1000), audio(None)]));
    assert_eq!(p.tracks[0].codec, Codec::Aac, "mp4a without a descriptor");
    assert_eq!(p.tracks[0].channels, Some(1));
}

#[test]
fn version_1_headers_have_64_bit_times() {
    let mut body = vec![0; 16];
    body.extend_from_slice(&1000_u32.to_be_bytes());
    body.extend_from_slice(&7500_u64.to_be_bytes());
    body.extend_from_slice(&[0; 80]);
    let movie = full_box(b"mvhd", 1, 0, &body);
    // A version 1 media header: its language after 64-bit times.
    let mut mdhd = vec![0; 16];
    mdhd.extend_from_slice(&1000_u32.to_be_bytes());
    mdhd.extend_from_slice(&7500_u64.to_be_bytes());
    mdhd.extend_from_slice(&testing::packed("fra").to_be_bytes());
    mdhd.extend_from_slice(&[0, 0]);
    let mdia = mp4_box(
        b"mdia",
        &[
            full_box(b"mdhd", 1, 0, &mdhd),
            full_box(b"hdlr", 0, 0, b"\0\0\0\0soun\0\0\0\0\0\0\0\0\0\0\0\0\0"),
        ]
        .concat(),
    );
    let p = read(&mp4_of(&[movie, mp4_box(b"trak", &mdia)]));
    assert_eq!(p.duration_secs, Some(7.5));
    assert_eq!(p.tracks[0].language.as_deref(), Some("fra"));
    assert_eq!(p.tracks[0].kind, Kind::Audio);
}

#[test]
fn a_fragmented_mp4_is_timed_by_its_mehd_and_else_by_its_longest_track() {
    let mehd = full_box(b"mehd", 0, 0, &4000_u32.to_be_bytes());
    let p = read(&mp4_of(&[mvhd(1000, 0), mp4_box(b"mvex", &mehd)]));
    assert_eq!(p.duration_secs, Some(4.0));
    let mehd = full_box(b"mehd", 1, 0, &6000_u64.to_be_bytes());
    let p = read(&mp4_of(&[mvhd(1000, 0), mp4_box(b"mvex", &mehd)]));
    assert_eq!(p.duration_secs, Some(6.0));
    // No movie length and no mehd: the longest track's.
    let p = read(&mp4_of(&[
        mvhd(1000, 0),
        video_trak(320, 240, 90_000, 180_000, 50),
        video_trak(320, 240, 90_000, 270_000, 75),
    ]));
    assert_eq!(p.duration_secs, Some(3.0));
    assert!(close(p.tracks[0].frame_rate, 25.0));
}

#[test]
fn a_box_with_a_64_bit_size_is_stepped_over() {
    // An `mdat` sized in 64 bits before `moov`, as a camera writes one.
    let mut file = mp4_box(b"ftyp", b"isom\0\0\x02\0isom");
    let mut large = 1_u32.to_be_bytes().to_vec();
    large.extend_from_slice(b"mdat");
    large.extend_from_slice(&(16_u64 + 100).to_be_bytes());
    large.extend(std::iter::repeat_n(0xEE, 100));
    file.extend(large);
    file.extend(mp4_box(b"moov", &mvhd(10, 50)));
    let p = read(&file);
    assert_eq!(p.duration_secs, Some(5.0));
}

#[test]
fn the_title_is_read_in_both_styles() {
    let name = [0xA9, b'n', b'a', b'm'];
    // iTunes-style: meta (a full box) / hdlr, ilst / name / data.
    let mut data = vec![0, 0, 0, 1, 0, 0, 0, 0];
    data.extend_from_slice(b"A Film");
    let ilst = mp4_box(b"ilst", &mp4_box(&name, &mp4_box(b"data", &data)));
    let hdlr = full_box(b"hdlr", 0, 0, b"\0\0\0\0mdirappl\0\0\0\0\0\0\0\0\0");
    let meta = full_box(b"meta", 0, 0, &[hdlr.clone(), ilst.clone()].concat());
    let p = read(&mp4_of(&[mvhd(1, 1), mp4_box(b"udta", &meta)]));
    assert_eq!(p.title.as_deref(), Some("A Film"));
    // QuickTime's meta is not a full box.
    let meta = mp4_box(b"meta", &[hdlr, ilst].concat());
    let p = read(&mp4_of(&[mvhd(1, 1), mp4_box(b"udta", &meta)]));
    assert_eq!(p.title.as_deref(), Some("A Film"));
    // QuickTime's own text atom: a length and a language, then the text.
    let mut atom = 7_u16.to_be_bytes().to_vec();
    atom.extend_from_slice(&[0x55, 0xC4]);
    atom.extend_from_slice(b"Old One");
    let p = read(&mp4_of(&[
        mvhd(1, 1),
        mp4_box(b"udta", &mp4_box(&name, &atom)),
    ]));
    assert_eq!(p.title.as_deref(), Some("Old One"));
}

#[test]
fn a_packed_language_is_iso_639_2_and_undetermined_is_none() {
    assert_eq!(
        mp4::packed_language(testing::packed("deu")).as_deref(),
        Some("deu")
    );
    assert_eq!(mp4::packed_language(testing::packed("und")), None);
    assert_eq!(mp4::packed_language(0), None, "a Macintosh language number");
    assert_eq!(mp4::packed_language(0x7FFF), None, "not letters");
}

#[test]
fn subtitles_disabled_tracks_and_a_presentation_size() {
    let text = mp4_trak(&Mp4Track {
        handler: b"sbtl",
        entry: b"tx3g",
        width: 0,
        height: 0,
        channels: 0,
        sample_rate: 0,
        timescale: 1000,
        duration: 1000,
        samples: 3,
        language: "spa",
        enabled: false,
        object_type: None,
    });
    // A video entry of no size: the track header's presentation size stands.
    let mut video = video_trak(0, 0, 1000, 1000, 30);
    let at = video.windows(4).position(|w| w == b"tkhd").unwrap() + 4 + 76;
    video[at..at + 4].copy_from_slice(&(1280_u32 << 16).to_be_bytes());
    video[at + 4..at + 8].copy_from_slice(&(720_u32 << 16).to_be_bytes());
    let p = read(&mp4_of(&[mvhd(1000, 1000), video, text]));
    let s = p.first(Kind::Subtitle).expect("a subtitle track");
    assert_eq!(s.codec, Codec::Text);
    assert_eq!(s.language.as_deref(), Some("spa"));
    assert!(!s.default, "a disabled track is not the default");
    let v = p.first(Kind::Video).unwrap();
    assert_eq!((v.width, v.height), (Some(1280), Some(720)));
}

#[test]
fn a_quicktime_version_2_sound_description_is_read_where_it_moved() {
    let mut entry = vec![0, 0, 0, 0, 0, 0, 0, 1];
    entry.extend_from_slice(&2_u16.to_be_bytes()); // version 2
    entry.extend_from_slice(&[0; 6]);
    entry.extend_from_slice(&3_u16.to_be_bytes()); // always 3
    entry.extend_from_slice(&16_u16.to_be_bytes());
    entry.extend_from_slice(&[0xFF, 0xFE, 0, 0]);
    entry.extend_from_slice(&0x0001_0000_u32.to_be_bytes());
    entry.extend_from_slice(&72_u32.to_be_bytes());
    entry.extend_from_slice(&96_000.0_f64.to_be_bytes());
    entry.extend_from_slice(&6_u32.to_be_bytes());
    entry.extend_from_slice(&[0; 20]);
    let mut stsd = 1_u32.to_be_bytes().to_vec();
    stsd.extend(mp4_box(b"lpcm", &entry));
    let stbl = mp4_box(b"stbl", &full_box(b"stsd", 0, 0, &stsd));
    let hdlr = full_box(b"hdlr", 0, 0, b"\0\0\0\0soun\0\0\0\0\0\0\0\0\0\0\0\0\0");
    let mdia = mp4_box(b"mdia", &[hdlr, mp4_box(b"minf", &stbl)].concat());
    let p = read(&mp4_of(&[mvhd(1, 1), mp4_box(b"trak", &mdia)]));
    let a = &p.tracks[0];
    assert_eq!(a.codec, Codec::Pcm);
    assert_eq!((a.sample_rate, a.channels), (Some(96_000), Some(6)));
}

#[test]
fn every_sample_entry_names_its_codec() {
    for (code, codec) in [
        (b"avc1", Codec::H264),
        (b"hev1", Codec::H265),
        (b"av01", Codec::Av1),
        (b"vp09", Codec::Vp9),
        (b"mp4v", Codec::Mpeg4Visual),
        (b"Opus", Codec::Opus),
        (b"fLaC", Codec::Flac),
        (b"ac-3", Codec::Ac3),
        (b"ec-3", Codec::Eac3),
        (b"wvtt", Codec::WebVtt),
        (b"apch", Codec::Other(String::from("ProRes"))),
        (b"xyz1", Codec::Other(String::from("xyz1"))),
        (&[0, 1, 2, 3], Codec::Unknown),
    ] {
        let trak = mp4_trak(&Mp4Track {
            handler: b"vide",
            entry: code,
            width: 16,
            height: 16,
            channels: 0,
            sample_rate: 0,
            timescale: 1,
            duration: 1,
            samples: 1,
            language: "eng",
            enabled: true,
            object_type: None,
        });
        let p = read(&mp4_of(&[mvhd(1, 1), trak]));
        assert_eq!(p.tracks[0].codec, codec, "{code:?}");
    }
}

#[test]
fn a_box_that_lies_about_its_size_ends_the_walk() {
    // A child claiming less than its own header, then one claiming more than
    // its parent holds: the walk ends, nothing panics, what came before stands.
    let mut moov = mvhd(100, 300);
    moov.extend_from_slice(&4_u32.to_be_bytes());
    moov.extend_from_slice(b"trak");
    let p = read(&mp4_of(&[moov]));
    assert_eq!(p.duration_secs, Some(3.0));
    assert!(p.tracks.is_empty());
    let mut moov = mvhd(100, 300);
    moov.extend_from_slice(&u32::MAX.to_be_bytes());
    moov.extend_from_slice(b"trak");
    moov.extend_from_slice(&[0; 16]);
    let p = read(&mp4_of(&[moov]));
    assert_eq!(p.duration_secs, Some(3.0));
    assert_eq!(
        p.tracks.len(),
        1,
        "a box cut short is read as far as it goes"
    );
}

// ============================================================================
// Matroska
// ============================================================================

#[test]
fn a_matroska_file_is_read_for_its_length_its_picture_and_its_sound() {
    let p = read(&testing::mkv(1280, 720, 60, 24));
    assert_eq!(p.container, Container::Matroska);
    assert!(close(p.duration_secs, 60.0), "{:?}", p.duration_secs);
    let v = p.first(Kind::Video).expect("a picture");
    assert_eq!(v.codec, Codec::H264);
    assert_eq!((v.width, v.height), (Some(1280), Some(720)));
    assert!(close(v.frame_rate, 24.0), "{:?}", v.frame_rate);
    assert_eq!(
        v.language.as_deref(),
        Some("eng"),
        "Matroska's default language"
    );
    assert!(v.default, "Matroska's default flag");
    let a = p.first(Kind::Audio).expect("sound");
    assert_eq!(a.codec, Codec::Opus);
    assert_eq!((a.sample_rate, a.channels), (Some(48_000), Some(2)));
}

#[test]
fn a_webm_file_is_named_so_by_its_header() {
    let p = read(&testing::webm(640, 360, 5, 30));
    assert_eq!(p.container, Container::WebM);
    assert_eq!(p.first(Kind::Video).unwrap().codec, Codec::Vp9);
}

#[test]
fn a_track_says_its_name_language_and_flags() {
    let entry = |fields: &[Vec<u8>]| element(0xAE, &fields.concat());
    let tracks = [
        entry(&[
            uint_element(0x83, 2),
            element(0x86, b"A_AC3"),
            element(0x536E, b"Commentary"),
            element(0x22_B59C, b"ger"),
            element(0x22_B59D, b"de-CH"),
            uint_element(0x88, 0),
        ]),
        entry(&[
            uint_element(0x83, 17),
            element(0x86, b"S_TEXT/UTF8"),
            element(0x22_B59C, b"und"),
            uint_element(0x55AA, 1),
        ]),
    ];
    let mut file = ebml_header("matroska");
    file.extend(mkv_segment(1.0, Some("The Title"), &tracks));
    let p = read(&file);
    assert_eq!(p.title.as_deref(), Some("The Title"));
    let a = &p.tracks[0];
    assert_eq!(a.codec, Codec::Ac3);
    assert_eq!(a.name.as_deref(), Some("Commentary"));
    assert_eq!(
        a.language.as_deref(),
        Some("de-CH"),
        "BCP 47 over ISO 639-2"
    );
    assert!(!a.default);
    let s = &p.tracks[1];
    assert_eq!((s.kind, s.codec.clone()), (Kind::Subtitle, Codec::Text));
    assert_eq!(s.language, None, "undetermined");
    assert!(s.forced && s.default);
}

#[test]
fn the_duration_is_in_ticks_of_the_timestamp_scale() {
    // A tick of a whole second, and the duration as a four-byte float.
    let info = [
        uint_element(0x2A_D7B1, 1_000_000_000),
        element(0x4489, &90.0_f32.to_be_bytes()),
    ]
    .concat();
    let segment = element(0x1853_8067, &element(0x1549_A966, &info));
    let mut file = ebml_header("matroska");
    file.extend(segment);
    assert_eq!(read(&file).duration_secs, Some(90.0));
    // A NaN duration is no duration.
    let info = float_element(0x4489, f64::NAN);
    let mut file = ebml_header("matroska");
    file.extend(element(0x1853_8067, &element(0x1549_A966, &info)));
    assert_eq!(read(&file).duration_secs, None);
}

#[test]
fn an_audio_track_takes_matroskas_defaults_and_its_output_rate() {
    let bare = element(
        0xAE,
        &[
            uint_element(0x83, 2),
            element(0x86, b"A_VORBIS"),
            element(0xE1, &[]),
        ]
        .concat(),
    );
    let sbr = element(
        0xAE,
        &[
            uint_element(0x83, 2),
            element(0x86, b"A_AAC/MPEG4/LC/SBR"),
            element(
                0xE1,
                &[
                    float_element(0xB5, 22_050.0),
                    float_element(0x78B5, 44_100.0),
                    uint_element(0x9F, 6),
                ]
                .concat(),
            ),
        ]
        .concat(),
    );
    let mut file = ebml_header("matroska");
    file.extend(mkv_segment(1.0, None, &[bare, sbr]));
    let p = read(&file);
    assert_eq!(
        (p.tracks[0].sample_rate, p.tracks[0].channels),
        (Some(8000), Some(1))
    );
    assert_eq!(p.tracks[0].codec, Codec::Vorbis);
    assert_eq!(
        (p.tracks[1].sample_rate, p.tracks[1].channels),
        (Some(44_100), Some(6))
    );
    assert_eq!(p.tracks[1].codec, Codec::Aac);
}

#[test]
fn what_a_cluster_of_unknown_length_hides_is_found_through_the_seek_head() {
    // SeekHead, then a Cluster whose length was never written, then Info
    // and Tracks: stepping cannot pass the cluster; the seek head can.
    let info = element(0x1549_A966, &float_element(0x4489, 12_000.0));
    let tracks = element(0x1654_AE6B, &mkv_video("V_AV1", 3840, 2160, 60));
    let mut cluster = vec![
        0x1F, 0x43, 0xB6, 0x75, 0x01, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
    ];
    cluster.extend(uint_element(0xE7, 0));
    let seek = |id: u64, position: usize| {
        element(
            0x4DBB,
            &[
                uint_element(0x53AB, id),
                uint_element(0x53AC, position as u64),
            ]
            .concat(),
        )
    };
    // The seek head's own length is fixed by its two entries, so the
    // positions can be worked out before it is built.
    let seek_len = element(0x114D_9B74, &[seek(0, 0), seek(0, 0)].concat()).len();
    let info_at = seek_len + cluster.len();
    let tracks_at = info_at + info.len();
    let head = element(
        0x114D_9B74,
        &[seek(0x1549_A966, info_at), seek(0x1654_AE6B, tracks_at)].concat(),
    );
    assert_eq!(head.len(), seek_len);
    let body = [head, cluster, info, tracks].concat();
    let mut file = ebml_header("webm");
    file.extend(element(0x1853_8067, &body));
    let p = read(&file);
    assert_eq!(p.duration_secs, Some(12.0));
    let v = p.first(Kind::Video).expect("the tracks behind the cluster");
    assert_eq!(
        (v.codec.clone(), v.width, v.height),
        (Codec::Av1, Some(3840), Some(2160))
    );
}

#[test]
fn every_matroska_codec_id_is_named() {
    for (id, codec) in [
        ("V_MPEGH/ISO/HEVC", Codec::H265),
        ("V_MPEG4/ISO/ASP", Codec::Mpeg4Visual),
        ("V_THEORA", Codec::Theora),
        ("A_MPEG/L3", Codec::Mp3),
        ("A_FLAC", Codec::Flac),
        ("A_PCM/INT/LIT", Codec::Pcm),
        ("A_DTS/EXPRESS", Codec::Dts),
        ("A_TRUEHD", Codec::TrueHd),
        ("S_TEXT/ASS", Codec::Ass),
        ("S_HDMV/PGS", Codec::Pgs),
        ("V_QUICKTIME", Codec::Other(String::from("V_QUICKTIME"))),
    ] {
        let entry = element(
            0xAE,
            &[uint_element(0x83, 1), element(0x86, id.as_bytes())].concat(),
        );
        let mut file = ebml_header("matroska");
        file.extend(mkv_segment(1.0, None, &[entry]));
        assert_eq!(read(&file).tracks[0].codec, codec, "{id}");
    }
}

#[test]
fn an_ebml_integer_is_as_long_as_its_leading_zeros_say() {
    assert_eq!(mkv::vint(&[0x81], 0, false), Some((1, 1)));
    assert_eq!(mkv::vint(&[0x40, 0x02], 0, false), Some((2, 2)));
    assert_eq!(
        mkv::vint(&[0x1A, 0x45, 0xDF, 0xA3], 0, true),
        Some((0x1A45_DFA3, 4))
    );
    assert_eq!(
        mkv::vint(&[0x00], 0, false),
        None,
        "no marker in the first byte"
    );
    assert_eq!(mkv::vint(&[0x41], 0, false), None, "cut short");
}

// ============================================================================
// AVI
// ============================================================================

#[test]
fn an_avi_is_read_for_its_length_its_picture_and_its_sound() {
    let p = read(&testing::avi(640, 480, 30, 25));
    assert_eq!(p.container, Container::Avi);
    assert_eq!(p.duration_secs, Some(30.0));
    let v = p.first(Kind::Video).expect("a picture");
    assert_eq!(v.codec, Codec::H264);
    assert_eq!((v.width, v.height), (Some(640), Some(480)));
    assert_eq!(v.frame_rate, Some(25.0));
    let a = p.first(Kind::Audio).expect("sound");
    assert_eq!(a.codec, Codec::Mp3);
    assert_eq!((a.sample_rate, a.channels), (Some(44_100), Some(2)));
}

#[test]
fn an_opendml_avi_counts_its_frames_past_the_first_gigabyte() {
    let video = [
        strh(b"vids", b"XVID", 1001, 30_000, 1000),
        bitmap(720, -480, b"XVID"),
    ]
    .concat();
    let odml = avi_list(b"odml", &chunk(b"dmlh", &3000_u32.to_le_bytes()));
    let header = [
        avih(33_367, 1000, 720, 480),
        avi_list(b"strl", &video),
        odml,
    ]
    .concat();
    let p = read(&avi_file(&header));
    assert!(
        close(p.duration_secs, 3000.0 * 1001.0 / 30_000.0),
        "{:?}",
        p.duration_secs
    );
    let v = &p.tracks[0];
    assert_eq!(v.codec, Codec::Mpeg4Visual);
    assert_eq!(
        v.height,
        Some(480),
        "a top-down picture's height is positive"
    );
    assert!(close(v.frame_rate, 29.97));
}

#[test]
fn a_stream_says_its_name_and_whether_it_is_off() {
    let mut off = strh(b"auds", &[0; 4], 1, 48_000, 48_000);
    off[16] = 1; // flags, after the id and length: disabled
    let audio = [
        off,
        wave_format(0x0001, 2, 48_000),
        chunk(b"strn", b"Director\0"),
    ]
    .concat();
    let header = [avih(40_000, 25, 0, 0), avi_list(b"strl", &audio)].concat();
    let p = read(&avi_file(&header));
    let a = &p.tracks[0];
    assert_eq!(a.name.as_deref(), Some("Director"));
    assert!(!a.default);
    assert_eq!(a.codec, Codec::Pcm);
    // No picture: the main header's frames at its frame time.
    assert_eq!(p.duration_secs, Some(1.0));
}

#[test]
fn an_extensible_wave_format_names_its_sub_format_and_a_blank_compression_its_handler() {
    // A wave format grown to hold the extension: its GUID's first two bytes
    // at 24.
    let mut body = 0xFFFE_u16.to_le_bytes().to_vec();
    body.extend_from_slice(&6_u16.to_le_bytes());
    body.extend_from_slice(&48_000_u32.to_le_bytes());
    body.extend_from_slice(&[0; 16]);
    body.extend_from_slice(&0x2000_u16.to_le_bytes());
    body.extend_from_slice(&[0; 14]);
    let audio = [strh(b"auds", &[0; 4], 1, 48_000, 0), chunk(b"strf", &body)].concat();
    let video = [strh(b"vids", b"DIVX", 1, 24, 48), bitmap(320, 240, &[0; 4])].concat();
    let header = [
        avih(41_666, 48, 320, 240),
        avi_list(b"strl", &video),
        avi_list(b"strl", &audio),
    ]
    .concat();
    let p = read(&avi_file(&header));
    assert_eq!(
        p.tracks[0].codec,
        Codec::Mpeg4Visual,
        "the handler stands in"
    );
    assert_eq!(p.tracks[1].codec, Codec::Ac3);
    assert_eq!(p.tracks[1].channels, Some(6));
    assert_eq!(p.duration_secs, Some(2.0));
}

// ============================================================================
// Anything
// ============================================================================

#[test]
fn what_is_not_a_video_says_nothing() {
    for bytes in [
        &b""[..],
        b"not a video",
        b"ID3\x04\0\0\0\0\0\0",
        b"RIFF\0\0\0\0WAVEfmt ",
    ] {
        let p = read(bytes);
        assert_eq!(p, Probe::default(), "{bytes:?}");
    }
}

#[test]
fn every_prefix_of_every_fixture_reads_without_a_panic() {
    for file in [
        testing::mp4(320, 240, 2, 25),
        testing::mkv(320, 240, 2, 25),
        testing::webm(320, 240, 2, 25),
        testing::avi(320, 240, 2, 25),
    ] {
        for cut in 0..file.len() {
            let _ = read(&file[..cut]);
        }
    }
}

#[test]
fn the_preferred_track_is_the_default_one() {
    let mut p = read(&testing::mkv(320, 240, 2, 25));
    let mut second = p.tracks[1].clone();
    second.name = Some(String::from("Second"));
    p.tracks[1].default = false;
    p.tracks.push(second);
    assert_eq!(
        p.preferred(Kind::Audio).unwrap().name.as_deref(),
        Some("Second")
    );
    assert_eq!(p.first(Kind::Audio).unwrap().name, None);
    assert!(p.preferred(Kind::Subtitle).is_none());
}

#[test]
fn a_codec_and_a_container_have_names() {
    assert_eq!(Codec::H264.name(), "H.264");
    assert_eq!(Codec::Other(String::from("XYZ")).name(), "XYZ");
    assert_eq!(Container::WebM.name(), "WebM");
    assert_eq!(Container::default(), Container::Unknown);
}

#[test]
fn a_video_with_a_matroska_audio_entry_is_read_for_both() {
    let tracks = [
        mkv_video("V_VP8", 176, 144, 15),
        mkv_audio("A_VORBIS", 1, 11_025.0),
    ];
    let mut file = ebml_header("webm");
    file.extend(mkv_segment(3.5, None, &tracks));
    let p = read(&file);
    assert_eq!(p.first(Kind::Video).unwrap().codec, Codec::Vp8);
    assert_eq!(p.first(Kind::Audio).unwrap().sample_rate, Some(11_025));
    assert!(close(p.duration_secs, 3.5));
}
