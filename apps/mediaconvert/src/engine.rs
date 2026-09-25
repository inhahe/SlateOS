//! The conversions this program can actually do.
//!
//! **WAV to WAV** at another sample rate, channel count or sample format,
//! through `wavpcm`; and **PNG or JPEG to BMP**, at the picture's own size or
//! fitted inside a width and height, through `imagecodec` and the BMP writer
//! below. Everything else is refused *before it is queued*, with the reason
//! ([`recipe`]): a queue is acted on -- somebody deletes the originals once it
//! says done -- so a job that cannot finish must never be in it.
//!
//! A job runs on a thread of its own ([`Worker`]), reads its source through a
//! cap, and writes its output through `safeio`, so a crash mid-write leaves no
//! half a file. [`output_path`] never names a file that exists -- the source
//! included -- so nothing is ever written over.

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::thread::JoinHandle;

use crate::{AudioFormat, AudioSettings, ImageFormat, ImageSettings, OutputFormat};

/// The most of a source one job reads. A WAV this long is about 90 minutes
/// of 48 kHz stereo 24-bit audio; a picture this large is far past the
/// decoder's own limit.
pub const MAX_SOURCE_BYTES: usize = 1 << 30;

/// What one job does, fixed when it is queued.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Recipe {
    /// WAV to WAV.
    Wav {
        sample_rate: u32,
        channels: u16,
        format: wavpcm::SampleFormat,
    },
    /// PNG or JPEG to BMP, fitted inside a width and height when either is set.
    Bmp {
        max_width: Option<u32>,
        max_height: Option<u32>,
    },
}

impl Recipe {
    /// What it does, for the queue.
    #[must_use]
    pub fn describe(&self) -> String {
        match self {
            Self::Wav {
                sample_rate,
                channels,
                format,
            } => format!(
                "WAV, {sample_rate} Hz, {}, {}",
                match channels {
                    1 => String::from("mono"),
                    2 => String::from("stereo"),
                    n => format!("{n} channels"),
                },
                format.label()
            ),
            Self::Bmp {
                max_width,
                max_height,
            } => match (max_width, max_height) {
                (None, None) => String::from("BMP, same size"),
                (Some(w), None) => format!("BMP, at most {w} wide"),
                (None, Some(h)) => format!("BMP, at most {h} high"),
                (Some(w), Some(h)) => format!("BMP, within {w}x{h}"),
            },
        }
    }
}

/// A file's extension, lower case, if it has one that is text.
fn extension(name: &OsStr) -> String {
    Path::new(name)
        .extension()
        .and_then(OsStr::to_str)
        .map(str::to_ascii_lowercase)
        .unwrap_or_default()
}

/// How to make `output` from the file `source`, or why it cannot be made here.
///
/// # Errors
///
/// The reason, written for the person who asked: which side is missing -- a
/// decoder for the source or an encoder for the output -- and what can be
/// done instead.
pub fn recipe(
    source: &OsStr,
    output: &OutputFormat,
    audio: &AudioSettings,
    image: &ImageSettings,
) -> Result<Recipe, String> {
    let ext = extension(source);
    match output {
        OutputFormat::Audio(AudioFormat::Wav) => {
            if ext != "wav" {
                return Err(format!(
                    "{} cannot be read here: WAV is the audio this build decodes",
                    shown_ext(&ext)
                ));
            }
            let format = match audio.bit_depth {
                8 => wavpcm::SampleFormat::U8,
                16 => wavpcm::SampleFormat::I16,
                24 => wavpcm::SampleFormat::I24,
                32 => wavpcm::SampleFormat::F32,
                other => return Err(format!("a WAV cannot be {other}-bit")),
            };
            if audio.sample_rate == 0 || audio.channels == 0 {
                return Err(String::from(
                    "the audio settings name no sample rate or no channels",
                ));
            }
            Ok(Recipe::Wav {
                sample_rate: audio.sample_rate,
                channels: u16::from(audio.channels),
                format,
            })
        }
        OutputFormat::Audio(other) => Err(format!(
            "no {} encoder in this build: WAV is the audio it writes",
            other.label()
        )),
        OutputFormat::Image(ImageFormat::Bmp) => {
            if !matches!(ext.as_str(), "png" | "jpg" | "jpeg") {
                return Err(format!(
                    "{} cannot be read here: PNG and JPEG are the pictures this build decodes",
                    shown_ext(&ext)
                ));
            }
            Ok(Recipe::Bmp {
                max_width: image.max_width,
                max_height: image.max_height,
            })
        }
        OutputFormat::Image(other) => Err(format!(
            "no {} encoder in this build: BMP is the picture it writes (PNG waits on \
             requests/e-f-a-png-encoder-applications-can-save-with.md)",
            other.label()
        )),
        OutputFormat::Video(other) => Err(format!(
            "no video can be read or written in this build ({} included)",
            other.label()
        )),
    }
}

/// An extension as a person reads it: `FLAC`, or "a file with no extension".
fn shown_ext(ext: &str) -> String {
    if ext.is_empty() {
        String::from("A file with no extension")
    } else {
        format!("A .{ext} file")
    }
}

/// Where a job's output goes: `dir` (or the source's own folder), with the
/// file name `name` -- and when that is taken, the source itself included,
/// " (2)", " (3)" and so on before the extension. Nothing is written over.
///
/// `taken` says whether a path is in use: on disk, or planned by another job.
#[must_use]
pub fn output_path(
    source: &Path,
    dir: Option<&Path>,
    name: &OsStr,
    taken: &dyn Fn(&Path) -> bool,
) -> PathBuf {
    let folder = dir
        .map(Path::to_path_buf)
        .or_else(|| source.parent().map(Path::to_path_buf))
        .unwrap_or_default();
    let first = folder.join(name);
    if first != source && !taken(&first) {
        return first;
    }
    let stem = Path::new(name).file_stem().unwrap_or(name).to_os_string();
    let ext = Path::new(name).extension().map(OsStr::to_os_string);
    for n in 2_u32.. {
        let mut candidate = OsString::from(&stem);
        candidate.push(format!(" ({n})"));
        if let Some(ext) = &ext {
            candidate.push(".");
            candidate.push(ext);
        }
        let path = folder.join(candidate);
        if path != source && !taken(&path) {
            return path;
        }
    }
    // Four billion names in one folder: unreachable, but not a panic.
    folder.join(name)
}

/// Run `recipe`: read `input`, convert, write `output`. `progress` holds the
/// fraction done as `f32` bits; `cancel`, once set, stops it before it
/// writes. Answers the bytes written.
///
/// # Errors
///
/// Why it stopped: the source could not be read or decoded, the output could
/// not be written, or it was cancelled.
pub fn run(
    recipe: &Recipe,
    input: &Path,
    output: &Path,
    progress: &AtomicU32,
    cancel: &AtomicBool,
) -> Result<u64, String> {
    let set = |fraction: f32| progress.store(fraction.clamp(0.0, 1.0).to_bits(), Ordering::Relaxed);
    set(0.0);
    let read = safeio::read_capped(input, MAX_SOURCE_BYTES)
        .map_err(|err| format!("could not read {}: {err}", input.display()))?;
    if read.truncated {
        return Err(format!("{} is larger than 1 GiB", input.display()));
    }
    set(0.1);
    let bytes = match recipe {
        Recipe::Wav {
            sample_rate,
            channels,
            format,
        } => {
            let audio =
                wavpcm::decode(&read.bytes).map_err(|err| format!("{}: {err}", input.display()))?;
            let mixed = wavpcm::to_channels(&audio, *channels);
            if cancel.load(Ordering::Relaxed) {
                return Err(String::from("cancelled"));
            }
            let resampled = wavpcm::resample(&mixed, *sample_rate, &mut |f| {
                set(0.15 + 0.75 * f);
            });
            if cancel.load(Ordering::Relaxed) {
                return Err(String::from("cancelled"));
            }
            // The seed is fixed so the same conversion makes the same file.
            wavpcm::encode(&resampled, *format, 0x5EED).map_err(|err| err.to_string())?
        }
        Recipe::Bmp {
            max_width,
            max_height,
        } => {
            let limits = imagecodec::Limits::default();
            let picture = match (max_width, max_height) {
                (None, None) => imagecodec::decode(&read.bytes, limits),
                (w, h) => imagecodec::decode_scaled(
                    &read.bytes,
                    limits,
                    w.unwrap_or(u32::MAX),
                    h.unwrap_or(u32::MAX),
                ),
            }
            .map_err(|err| format!("{}: {err}", input.display()))?;
            set(0.7);
            if cancel.load(Ordering::Relaxed) {
                return Err(String::from("cancelled"));
            }
            encode_bmp(picture.width, picture.height, &picture.pixels)?
        }
    };
    if cancel.load(Ordering::Relaxed) {
        return Err(String::from("cancelled"));
    }
    set(0.95);
    // The name was free when the job was queued, which may have been a while
    // ago. Whatever has it now is somebody's, and is not written over.
    safeio::write_new_atomically(output, &bytes).map_err(|err| {
        if err.kind() == std::io::ErrorKind::AlreadyExists {
            format!(
                "{} appeared while this waited; nothing was written over it",
                output.display()
            )
        } else {
            format!("could not write {}: {err}", output.display())
        }
    })?;
    set(1.0);
    Ok(u64::try_from(bytes.len()).unwrap_or(u64::MAX))
}

/// A picture as a BMP file: 24-bit when every pixel is opaque, 32-bit with an
/// alpha mask (a `BITMAPV4HEADER`) when any is not. Rows bottom-up, padded
/// to four bytes, as the format has them.
///
/// # Errors
///
/// When the picture is too large for a BMP's 32-bit sizes, or `pixels` is not
/// `width * height` long.
pub fn encode_bmp(width: u32, height: u32, pixels: &[u32]) -> Result<Vec<u8>, String> {
    let (w, h) = (
        usize::try_from(width).map_err(|_| String::from("too wide"))?,
        usize::try_from(height).map_err(|_| String::from("too high"))?,
    );
    if pixels.len() != w.saturating_mul(h) || w == 0 || h == 0 {
        return Err(String::from("the picture's pixels do not match its size"));
    }
    let alpha = pixels.iter().any(|p| p >> 24 != 0xFF);
    let bytes_per_pixel: usize = if alpha { 4 } else { 3 };
    let row = w
        .checked_mul(bytes_per_pixel)
        .and_then(|r| r.checked_add(3))
        .map(|r| r & !3)
        .ok_or_else(|| String::from("too wide for a BMP"))?;
    let header: usize = if alpha { 14 + 108 } else { 14 + 40 };
    let image_size = row
        .checked_mul(h)
        .ok_or_else(|| String::from("too large for a BMP"))?;
    let file_size = image_size
        .checked_add(header)
        .and_then(|s| u32::try_from(s).ok())
        .ok_or_else(|| String::from("too large for a BMP, whose sizes are 32 bits"))?;
    let image_size = u32::try_from(image_size).map_err(|_| String::from("too large for a BMP"))?;
    let signed = |v: u32| i32::try_from(v).map_err(|_| String::from("too large for a BMP"));

    let mut out = Vec::with_capacity(usize::try_from(file_size).unwrap_or(0));
    out.extend_from_slice(b"BM");
    out.extend_from_slice(&file_size.to_le_bytes());
    out.extend_from_slice(&0_u32.to_le_bytes());
    out.extend_from_slice(&u32::try_from(header).unwrap_or(54).to_le_bytes());
    // The info header.
    out.extend_from_slice(&(if alpha { 108_u32 } else { 40 }).to_le_bytes());
    out.extend_from_slice(&signed(width)?.to_le_bytes());
    // Positive: the rows are stored bottom-up.
    out.extend_from_slice(&signed(height)?.to_le_bytes());
    out.extend_from_slice(&1_u16.to_le_bytes());
    out.extend_from_slice(&(if alpha { 32_u16 } else { 24 }).to_le_bytes());
    // BI_BITFIELDS for the alpha form, BI_RGB otherwise.
    out.extend_from_slice(&(if alpha { 3_u32 } else { 0 }).to_le_bytes());
    out.extend_from_slice(&image_size.to_le_bytes());
    // 2835 pixels a metre is 72 dots an inch.
    out.extend_from_slice(&2835_i32.to_le_bytes());
    out.extend_from_slice(&2835_i32.to_le_bytes());
    out.extend_from_slice(&0_u32.to_le_bytes());
    out.extend_from_slice(&0_u32.to_le_bytes());
    if alpha {
        for mask in [0x00FF_0000_u32, 0x0000_FF00, 0x0000_00FF, 0xFF00_0000] {
            out.extend_from_slice(&mask.to_le_bytes());
        }
        // LCS_sRGB, and the unused endpoints and gammas it leaves at zero.
        out.extend_from_slice(b"BGRs");
        out.extend_from_slice(&[0_u8; 36 + 12]);
    }
    let pad = row.saturating_sub(w.saturating_mul(bytes_per_pixel));
    for y in (0..h).rev() {
        let start = y.saturating_mul(w);
        for px in pixels.get(start..start.saturating_add(w)).unwrap_or(&[]) {
            let [b, g, r, a] = px.to_le_bytes();
            out.extend_from_slice(&[b, g, r]);
            if alpha {
                out.push(a);
            }
        }
        out.extend(std::iter::repeat_n(0_u8, pad));
    }
    Ok(out)
}

/// A job running on a thread of its own.
pub struct Worker {
    /// The queue's id for the job.
    pub job_id: u64,
    handle: Option<JoinHandle<Result<u64, String>>>,
    progress: Arc<AtomicU32>,
    cancel: Arc<AtomicBool>,
}

impl Worker {
    /// Start `recipe` from `input` into `output`.
    #[must_use]
    pub fn start(job_id: u64, recipe: Recipe, input: PathBuf, output: PathBuf) -> Self {
        let progress = Arc::new(AtomicU32::new(0));
        let cancel = Arc::new(AtomicBool::new(false));
        let (p, c) = (Arc::clone(&progress), Arc::clone(&cancel));
        let handle = std::thread::Builder::new()
            .name(format!("convert-{job_id}"))
            .spawn(move || run(&recipe, &input, &output, &p, &c));
        match handle {
            Ok(handle) => Self {
                job_id,
                handle: Some(handle),
                progress,
                cancel,
            },
            Err(err) => {
                // No thread: the job is finished, having failed. `poll` reports
                // it through the same path as any other failure.
                let failed = std::thread::spawn(move || Err(format!("could not start: {err}")));
                Self {
                    job_id,
                    handle: Some(failed),
                    progress,
                    cancel,
                }
            }
        }
    }

    /// How far it has got, from 0 to 1.
    #[must_use]
    pub fn progress(&self) -> f32 {
        f32::from_bits(self.progress.load(Ordering::Relaxed))
    }

    /// Ask it to stop. It stops before it writes anything.
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }

    /// Its result once it has finished; `None` while it runs.
    pub fn poll(&mut self) -> Option<Result<u64, String>> {
        if !self.handle.as_ref().is_some_and(JoinHandle::is_finished) {
            return None;
        }
        let handle = self.handle.take()?;
        Some(
            handle
                .join()
                .unwrap_or_else(|_| Err(String::from("the conversion stopped unexpectedly"))),
        )
    }

    /// Wait for it to finish. For tests, which cannot wait on a clock.
    #[cfg(test)]
    pub fn wait(&mut self) -> Result<u64, String> {
        let handle = self
            .handle
            .take()
            .ok_or_else(|| String::from("already taken"))?;
        handle
            .join()
            .unwrap_or_else(|_| Err(String::from("the conversion stopped unexpectedly")))
    }
}

impl Drop for Worker {
    /// A window closed mid-job stops the job rather than leaving a thread
    /// writing into the user's folder after the program is gone.
    fn drop(&mut self) {
        self.cancel();
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss
)]
mod tests {
    use super::*;
    use crate::{QualityPreset, VideoFormat};

    /// A directory for one test, removed when it ends.
    struct Scratch(PathBuf);

    impl Scratch {
        fn new(tag: &str) -> Self {
            let dir =
                std::env::temp_dir().join(format!("mediaconvert-{tag}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            // Best effort: a leftover temp directory is harmless.
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn cd_audio() -> AudioSettings {
        AudioSettings {
            sample_rate: 44_100,
            channels: 2,
            bit_depth: 16,
            ..AudioSettings::from_preset(QualityPreset::Medium)
        }
    }

    #[test]
    fn what_cannot_be_done_is_refused_with_the_reason() {
        let audio = cd_audio();
        let image = ImageSettings::default();
        let wav = OutputFormat::Audio(AudioFormat::Wav);
        assert!(recipe(OsStr::new("a.wav"), &wav, &audio, &image).is_ok());
        let flac_in = recipe(OsStr::new("a.flac"), &wav, &audio, &image).unwrap_err();
        assert!(
            flac_in.contains(".flac") && flac_in.contains("WAV"),
            "{flac_in}"
        );
        let mp3_out = recipe(
            OsStr::new("a.wav"),
            &OutputFormat::Audio(AudioFormat::Mp3),
            &audio,
            &image,
        )
        .unwrap_err();
        assert!(
            mp3_out.contains("MP3") && mp3_out.contains("encoder"),
            "{mp3_out}"
        );
        let bmp = OutputFormat::Image(ImageFormat::Bmp);
        assert!(
            recipe(OsStr::new("photo.JPG"), &bmp, &audio, &image).is_ok(),
            "case"
        );
        assert!(recipe(OsStr::new("photo.gif"), &bmp, &audio, &image).is_err());
        let webp = recipe(
            OsStr::new("p.png"),
            &OutputFormat::Image(ImageFormat::WebP),
            &audio,
            &image,
        )
        .unwrap_err();
        assert!(webp.contains("encoder"), "{webp}");
        let video = recipe(
            OsStr::new("v.mkv"),
            &OutputFormat::Video(VideoFormat::Mp4),
            &audio,
            &image,
        )
        .unwrap_err();
        assert!(video.contains("video"), "{video}");
        let odd = AudioSettings {
            bit_depth: 12,
            ..cd_audio()
        };
        assert!(recipe(OsStr::new("a.wav"), &wav, &odd, &image).is_err());
    }

    #[test]
    fn nothing_is_ever_written_over() {
        let source = Path::new("/music/song.wav");
        let taken = |p: &Path| p == Path::new("/music/song (2).wav");
        // The name the rule gives is the source itself: the next free one.
        let out = output_path(source, None, OsStr::new("song.wav"), &taken);
        assert_eq!(out, Path::new("/music/song (3).wav"));
        // Another folder, free: as named.
        let out = output_path(
            source,
            Some(Path::new("/out")),
            OsStr::new("song.wav"),
            &|_| false,
        );
        assert_eq!(out, Path::new("/out/song.wav"));
        let out = output_path(source, None, OsStr::new("song.bmp"), &|_| false);
        assert_eq!(out, Path::new("/music/song.bmp"));
    }

    fn tone_wav(rate: u32, channels: u16, frames: usize) -> Vec<u8> {
        let mut samples = Vec::new();
        for i in 0..frames {
            let v = 0.5 * (2.0 * std::f32::consts::PI * 440.0 * i as f32 / rate as f32).sin();
            for _ in 0..channels {
                samples.push(v);
            }
        }
        wavpcm::encode(
            &wavpcm::Audio {
                sample_rate: rate,
                channels,
                samples,
            },
            wavpcm::SampleFormat::I24,
            1,
        )
        .unwrap()
    }

    #[test]
    fn a_wav_becomes_the_wav_the_recipe_names() {
        let dir = Scratch::new("wav");
        let input = dir.0.join("in.wav");
        std::fs::write(&input, tone_wav(48_000, 2, 4800)).unwrap();
        let output = dir.0.join("out.wav");
        let recipe = Recipe::Wav {
            sample_rate: 16_000,
            channels: 1,
            format: wavpcm::SampleFormat::I16,
        };
        let mut worker = Worker::start(1, recipe, input, output.clone());
        let written = worker.wait().unwrap();
        let bytes = std::fs::read(&output).unwrap();
        assert_eq!(written, bytes.len() as u64);
        let info = wavpcm::parse_header(&bytes).unwrap();
        assert_eq!(
            (info.sample_rate, info.channels, info.format, info.frames),
            (16_000, 1, wavpcm::SampleFormat::I16, 1600)
        );
        assert!((worker.progress() - 1.0).abs() < f32::EPSILON);
    }

    /// The output's name was free when the job was queued; a file that took
    /// it while the job waited is somebody's, and is left exactly as it is.
    #[test]
    fn a_name_taken_while_the_job_waited_is_not_written_over() {
        let dir = Scratch::new("taken");
        let input = dir.0.join("in.wav");
        std::fs::write(&input, tone_wav(8_000, 1, 800)).unwrap();
        let output = dir.0.join("out.wav");
        std::fs::write(&output, b"somebody's file").unwrap();
        let recipe = Recipe::Wav {
            sample_rate: 8_000,
            channels: 1,
            format: wavpcm::SampleFormat::I16,
        };
        let mut worker = Worker::start(3, recipe, input, output.clone());
        let why = worker.wait().unwrap_err();
        assert!(why.contains("appeared while this waited"), "{why}");
        assert_eq!(std::fs::read(&output).unwrap(), b"somebody's file");
        let names: Vec<_> = std::fs::read_dir(&dir.0)
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(names.len(), 2, "no temporary left beside them: {names:?}");
    }

    #[test]
    fn a_picture_becomes_a_bmp_of_the_same_pixels() {
        let dir = Scratch::new("bmp");
        let input = dir.0.join("in.png");
        let png = imagecodec::testing::png_rgba(5, 3, |x, y| {
            [(x * 40) as u8, (y * 80) as u8, 0x40, 0xFF]
        });
        std::fs::write(&input, png).unwrap();
        let output = dir.0.join("out.bmp");
        let mut worker = Worker::start(
            2,
            Recipe::Bmp {
                max_width: None,
                max_height: None,
            },
            input,
            output.clone(),
        );
        worker.wait().unwrap();
        let bmp = std::fs::read(&output).unwrap();
        assert_eq!(&bmp[..2], b"BM");
        assert_eq!(
            u32::from_le_bytes(bmp[2..6].try_into().unwrap()) as usize,
            bmp.len()
        );
        assert_eq!(
            u16::from_le_bytes([bmp[28], bmp[29]]),
            24,
            "an opaque picture is 24-bit"
        );
        // Rows are bottom-up and padded: 5 pixels of 3 bytes is 15, padded to 16.
        let offset = u32::from_le_bytes(bmp[10..14].try_into().unwrap()) as usize;
        let row = 16;
        // The top row (y = 0) is stored last.
        let top = offset + 2 * row;
        assert_eq!(
            &bmp[top + 3 * 4..top + 3 * 4 + 3],
            &[0x40, 0, 160],
            "pixel (4, 0) as B, G, R"
        );
        let bottom = offset;
        assert_eq!(&bmp[bottom..bottom + 3], &[0x40, 160, 0], "pixel (0, 2)");
    }

    #[test]
    fn a_picture_is_fitted_inside_the_size_asked() {
        let dir = Scratch::new("fit");
        let input = dir.0.join("in.png");
        std::fs::write(
            &input,
            imagecodec::testing::png_rgba(40, 20, |_, _| [9, 9, 9, 255]),
        )
        .unwrap();
        let output = dir.0.join("out.bmp");
        let mut worker = Worker::start(
            3,
            Recipe::Bmp {
                max_width: Some(10),
                max_height: None,
            },
            input,
            output.clone(),
        );
        worker.wait().unwrap();
        let bmp = std::fs::read(&output).unwrap();
        let w = i32::from_le_bytes(bmp[18..22].try_into().unwrap());
        let h = i32::from_le_bytes(bmp[22..26].try_into().unwrap());
        assert_eq!((w, h), (10, 5), "the aspect was not kept");
    }

    #[test]
    fn transparency_keeps_its_alpha() {
        let pixels = [0x80FF_0000_u32, 0xFF00_FF00];
        let bmp = encode_bmp(2, 1, &pixels).unwrap();
        assert_eq!(u16::from_le_bytes([bmp[28], bmp[29]]), 32);
        assert_eq!(
            u32::from_le_bytes(bmp[30..34].try_into().unwrap()),
            3,
            "BI_BITFIELDS"
        );
        let offset = u32::from_le_bytes(bmp[10..14].try_into().unwrap()) as usize;
        assert_eq!(
            &bmp[offset..offset + 8],
            &[0, 0, 0xFF, 0x80, 0, 0xFF, 0, 0xFF]
        );
        assert!(
            encode_bmp(2, 2, &pixels).is_err(),
            "a pixel count that does not match"
        );
    }

    #[test]
    fn a_source_that_is_not_what_it_says_fails_the_job_and_writes_nothing() {
        let dir = Scratch::new("bad");
        let input = dir.0.join("fake.wav");
        std::fs::write(&input, b"not audio at all").unwrap();
        let output = dir.0.join("out.wav");
        let mut worker = Worker::start(
            4,
            Recipe::Wav {
                sample_rate: 8000,
                channels: 1,
                format: wavpcm::SampleFormat::I16,
            },
            input,
            output.clone(),
        );
        let err = worker.wait().unwrap_err();
        assert!(err.contains("not a WAV"), "{err}");
        assert!(!output.exists(), "a failed job left a file");
    }

    #[test]
    fn a_cancelled_job_writes_nothing() {
        let dir = Scratch::new("cancel");
        let input = dir.0.join("in.wav");
        std::fs::write(&input, tone_wav(8000, 1, 800)).unwrap();
        let output = dir.0.join("out.wav");
        let progress = AtomicU32::new(0);
        let cancel = AtomicBool::new(true);
        let result = run(
            &Recipe::Wav {
                sample_rate: 16_000,
                channels: 1,
                format: wavpcm::SampleFormat::I16,
            },
            &input,
            &output,
            &progress,
            &cancel,
        );
        assert_eq!(result, Err(String::from("cancelled")));
        assert!(!output.exists());
    }
}
