//! WebVTT carried in an MP4 file: ISO/IEC 14496-30's `wvtt` sample entry,
//! what DASH and HLS segments, MP4Box and Shaka Packager write.
//!
//! A sample is a stretch of time and every cue showing through all of it,
//! each in a box of its own (`vttc`: the cue's identifier `iden`, its
//! settings `sttg`, its text `payl`); a stretch where none shows is a sample
//! saying so (`vtte`). So a cue is split wherever another begins or ends:
//! two cues overlapping are three samples, the one showing longer in all
//! three. Reading them, the cues are made whole again ([`Joined`]): a cue in
//! a sample that begins where the one before ended, with the same
//! identifier, settings and text, goes on; one in no sample after its last
//! has ended there. Each whole cue is then a WebVTT cue as WebM's are, its
//! text and settings read by the same rules (`webvtt.rs`).
//!
//! FFmpeg reads none of this: `wvtt` is not in its table of subtitle
//! codecs, and a track of it is data to it. GPAC, the format's reference
//! implementation, joins cues the same way when it writes a track back out
//! as a `.vtt` (`gf_webvtt_merge_cues`) -- but walks the samples' cues in
//! order, so that two cues showing through a split in the other order are
//! ended and begun again. Here a cue goes on wherever it is in the sample.

use super::joined;

/// A cue as a sample carries it (`vttc`).
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub(crate) struct SampleCue {
    /// The cue's identifier (`iden`); empty where it has none.
    pub id: String,
    /// Its settings (`sttg`) -- `line:0 align:start` -- as in a `.vtt`.
    pub settings: String,
    /// Its text (`payl`): WebVTT's cue text, tags and all.
    pub text: String,
}

/// The cues a sample carries, in its order: a `vttc` box each. `vtte` (no
/// cue showing) and `vtta` (text between cues: a comment) carry none, and a
/// box of any other kind is passed over. `None` for a sample that cannot be
/// read: a box size under its header or past what holds it, a cue's string
/// not UTF-8.
pub(crate) fn cues(sample: &[u8]) -> Option<Vec<SampleCue>> {
    let mut out = Vec::new();
    for (kind, body) in boxes(sample)? {
        if &kind != b"vttc" {
            continue;
        }
        let mut cue = SampleCue::default();
        for (inner, value) in boxes(body)? {
            let slot = match &inner {
                b"iden" => &mut cue.id,
                b"sttg" => &mut cue.settings,
                b"payl" => &mut cue.text,
                // `ctim` (the cue's time, for the timestamps in its text)
                // and `vsid` (its source) say nothing of what shows.
                _ => continue,
            };
            core::str::from_utf8(value).ok()?.clone_into(slot);
        }
        out.push(cue);
    }
    Some(out)
}

/// An ISO base media file's boxes, one after another: each one's type and
/// what it holds. A size of 0 runs to the end, one of 1 is given in the 64
/// bits after the type (ISO/IEC 14496-12). `None` at a size under its
/// header or past the data.
fn boxes(data: &[u8]) -> Option<Vec<([u8; 4], &[u8])>> {
    let mut out = Vec::new();
    let mut rest = data;
    while !rest.is_empty() {
        let size = u32::from_be_bytes(rest.get(..4)?.try_into().ok()?);
        let kind: [u8; 4] = rest.get(4..8)?.try_into().ok()?;
        let (header, size) = match size {
            0 => (8, rest.len()),
            1 => {
                let large = u64::from_be_bytes(rest.get(8..16)?.try_into().ok()?);
                (16, usize::try_from(large).ok()?)
            }
            n => (8, usize::try_from(n).ok()?),
        };
        if size < header || size > rest.len() {
            return None;
        }
        out.push((kind, rest.get(header..size)?));
        rest = rest.get(size..)?;
    }
    Some(out)
}

/// A cue showing: from when its first sample began to when its last ended,
/// in the track's ticks.
pub(crate) type Whole = joined::Whole<i64, SampleCue>;

/// A track's cues made whole again from its samples: each sample's cues
/// pieces as long as the sample ([`joined::Joined`]).
///
/// Given in the order they began, as every reader's cues are: a cue that
/// has ended waits while one that began before it still shows. Cues that
/// began together come in the order their first sample carries them, which
/// is the order the `.vtt` they were made from gives them.
#[derive(Debug, Default)]
pub(crate) struct Joined(joined::Joined<i64, SampleCue>);

impl Joined {
    /// A sample from `start` to `end` (ticks) carrying `cues`: each of them
    /// the last sample showed, to its end, goes on -- each at most once, so
    /// that two alike in a sample are two cues -- and the others begin. The
    /// cues showing that it does not carry end where their last sample did.
    /// Answers the cues ended that can now be given, in the order they
    /// began.
    pub(crate) fn sample(&mut self, start: i64, end: i64, cues: Vec<SampleCue>) -> Vec<Whole> {
        self.0
            .sample(end, cues.into_iter().map(|cue| (start, end, cue)).collect())
    }

    /// The track's end: every cue still showing has ended, where its last
    /// sample did. Answers every cue not yet given, in the order they began.
    pub(crate) fn finish(&mut self) -> Vec<Whole> {
        self.0.finish()
    }

    /// Everything forgotten: after a seek, the samples read next are from
    /// elsewhere in the track.
    pub(crate) fn clear(&mut self) {
        self.0.clear();
    }
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        clippy::indexing_slicing,
        clippy::arithmetic_side_effects,
        reason = "a test: a failure should be loud"
    )]

    use super::*;

    fn bx(kind: &[u8; 4], body: &[u8]) -> Vec<u8> {
        let mut out = u32::try_from(body.len() + 8)
            .unwrap()
            .to_be_bytes()
            .to_vec();
        out.extend_from_slice(kind);
        out.extend_from_slice(body);
        out
    }

    fn vttc(id: &str, settings: &str, text: &str) -> Vec<u8> {
        let mut body = Vec::new();
        if !id.is_empty() {
            body.extend(bx(b"iden", id.as_bytes()));
        }
        if !settings.is_empty() {
            body.extend(bx(b"sttg", settings.as_bytes()));
        }
        body.extend(bx(b"payl", text.as_bytes()));
        bx(b"vttc", &body)
    }

    fn cue(id: &str, settings: &str, text: &str) -> SampleCue {
        SampleCue {
            id: id.to_owned(),
            settings: settings.to_owned(),
            text: text.to_owned(),
        }
    }

    #[test]
    fn a_sample_is_its_cues_in_order_and_an_empty_one_none() {
        let sample = [
            vttc("1", "line:0", "first"),
            bx(b"vtta", b"NOTE a comment"),
            vttc("", "", "second\nline"),
        ]
        .concat();
        assert_eq!(
            cues(&sample),
            Some(vec![
                cue("1", "line:0", "first"),
                cue("", "", "second\nline")
            ])
        );
        assert_eq!(cues(&bx(b"vtte", b"")), Some(Vec::new()));
        assert_eq!(cues(&[]), Some(Vec::new()));
        // A cue's boxes it does not read -- its time, its source -- and boxes
        // of kinds it does not know beside the cues, passed over.
        let timed = bx(
            b"vttc",
            &[
                bx(b"ctim", b"00:00:01.000"),
                bx(b"vsid", &7u32.to_be_bytes()),
                bx(b"payl", b"t"),
            ]
            .concat(),
        );
        assert_eq!(
            cues(&[bx(b"free", b"xx"), timed].concat()),
            Some(vec![cue("", "", "t")])
        );
    }

    #[test]
    fn a_box_of_size_zero_runs_to_the_end_and_one_of_size_one_is_long() {
        let mut to_end = 0u32.to_be_bytes().to_vec();
        to_end.extend_from_slice(b"vttc");
        to_end.extend(bx(b"payl", b"rest"));
        assert_eq!(cues(&to_end), Some(vec![cue("", "", "rest")]));
        let payl = bx(b"payl", b"long");
        let mut long = 1u32.to_be_bytes().to_vec();
        long.extend_from_slice(b"vttc");
        long.extend_from_slice(&u64::try_from(payl.len() + 16).unwrap().to_be_bytes());
        long.extend(payl);
        assert_eq!(cues(&long), Some(vec![cue("", "", "long")]));
    }

    #[test]
    fn a_sample_that_cannot_be_read_is_none() {
        let whole = vttc("", "", "text");
        // Cut anywhere inside: a size past the data.
        for cut in 1..whole.len() {
            assert_eq!(cues(&whole[..cut]), None, "cut at {cut}");
        }
        // A size under its header.
        let mut small = whole.clone();
        small[..4].copy_from_slice(&7u32.to_be_bytes());
        assert_eq!(cues(&small), None);
        // Text that is not UTF-8.
        assert_eq!(cues(&bx(b"vttc", &bx(b"payl", b"\xff"))), None);
    }

    #[test]
    fn a_cue_split_across_samples_is_whole_again() {
        let (a, b) = (cue("", "", "long"), cue("", "", "short"));
        let mut joined = Joined::default();
        // a from 0 to 30, b from 10 to 20: three samples.
        assert!(joined.sample(0, 10, vec![a.clone()]).is_empty());
        assert!(joined.sample(10, 20, vec![a.clone(), b.clone()]).is_empty());
        // b has ended, but began after a, which still shows: it waits.
        assert!(joined.sample(20, 30, vec![a.clone()]).is_empty());
        assert_eq!(
            joined.sample(30, 40, Vec::new()),
            [
                Whole {
                    start: 0,
                    end: 30,
                    cue: a
                },
                Whole {
                    start: 10,
                    end: 20,
                    cue: b
                },
            ]
        );
        assert!(joined.finish().is_empty());
    }

    #[test]
    fn a_cue_goes_on_wherever_it_is_in_the_sample() {
        let (a, b) = (cue("a", "", "x"), cue("b", "", "y"));
        let mut joined = Joined::default();
        joined.sample(0, 10, vec![a.clone(), b.clone()]);
        // The other order: both go on, neither ended and begun again.
        joined.sample(10, 20, vec![b.clone(), a.clone()]);
        assert_eq!(
            joined.finish(),
            [
                Whole {
                    start: 0,
                    end: 20,
                    cue: a
                },
                Whole {
                    start: 0,
                    end: 20,
                    cue: b
                },
            ]
        );
    }

    #[test]
    fn a_cue_alike_but_for_its_identifier_settings_or_text_is_another() {
        let base = cue("1", "line:0", "text");
        for other in [
            cue("2", "line:0", "text"),
            cue("1", "line:1", "text"),
            cue("1", "line:0", "text!"),
        ] {
            let mut joined = Joined::default();
            joined.sample(0, 10, vec![base.clone()]);
            let ended = joined.sample(10, 20, vec![other.clone()]);
            assert_eq!(
                ended,
                [Whole {
                    start: 0,
                    end: 10,
                    cue: base.clone()
                }]
            );
            assert_eq!(
                joined.finish(),
                [Whole {
                    start: 10,
                    end: 20,
                    cue: other
                }]
            );
        }
    }

    #[test]
    fn a_cue_goes_on_only_into_the_sample_its_last_one_ended_at() {
        let a = cue("", "", "x");
        let mut joined = Joined::default();
        joined.sample(0, 10, vec![a.clone()]);
        // Nothing from 10 to 15: the cue ended at 10, and the same text at 15
        // is a cue of its own.
        assert_eq!(
            joined.sample(15, 25, vec![a.clone()]),
            [Whole {
                start: 0,
                end: 10,
                cue: a.clone()
            }]
        );
        assert_eq!(
            joined.finish(),
            [Whole {
                start: 15,
                end: 25,
                cue: a
            }]
        );
    }

    #[test]
    fn two_cues_alike_in_one_sample_are_two_cues() {
        let a = cue("", "", "same");
        let mut joined = Joined::default();
        joined.sample(0, 10, vec![a.clone(), a.clone()]);
        // One of them goes on -- the first, which so comes first -- and the
        // other ends, waiting for it.
        assert!(joined.sample(10, 20, vec![a.clone()]).is_empty());
        assert_eq!(
            joined.finish(),
            [
                Whole {
                    start: 0,
                    end: 20,
                    cue: a.clone()
                },
                Whole {
                    start: 0,
                    end: 10,
                    cue: a.clone()
                },
            ]
        );
        // Both alike in the next sample too: both go on, neither taken
        // twice.
        joined.sample(0, 10, vec![a.clone(), a.clone()]);
        joined.sample(10, 20, vec![a.clone(), a.clone()]);
        let both = Whole {
            start: 0,
            end: 20,
            cue: a,
        };
        assert_eq!(joined.finish(), [both.clone(), both]);
    }

    #[test]
    fn cues_come_in_the_order_they_began() {
        let mut joined = Joined::default();
        let (a, b, c) = (cue("", "", "a"), cue("", "", "b"), cue("", "", "c"));
        // a 0-50, b 10-20, c 20-30: b and c wait for a.
        joined.sample(0, 10, vec![a.clone()]);
        joined.sample(10, 20, vec![a.clone(), b.clone()]);
        joined.sample(20, 30, vec![a.clone(), c.clone()]);
        assert!(joined.sample(30, 50, vec![a.clone()]).is_empty());
        let given: Vec<i64> = joined.finish().iter().map(|w| w.start).collect();
        assert_eq!(given, [0, 10, 20]);
    }

    /// Two cues beginning together come in the order their first sample
    /// carries them -- the `.vtt`'s -- whichever ends first.
    #[test]
    fn cues_that_began_together_come_in_their_samples_order() {
        let (long, short) = (cue("", "", "long"), cue("", "", "short"));
        let mut joined = Joined::default();
        joined.sample(0, 10, vec![long.clone(), short.clone()]);
        // The short one ends first, and still waits for the long one.
        assert!(joined.sample(10, 20, vec![long.clone()]).is_empty());
        assert_eq!(
            joined.finish(),
            [
                Whole {
                    start: 0,
                    end: 20,
                    cue: long
                },
                Whole {
                    start: 0,
                    end: 10,
                    cue: short
                },
            ]
        );
    }

    #[test]
    fn a_cleared_join_forgets_what_showed() {
        let mut joined = Joined::default();
        joined.sample(0, 10, vec![cue("", "", "a")]);
        joined.clear();
        assert!(joined.finish().is_empty());
    }
}
