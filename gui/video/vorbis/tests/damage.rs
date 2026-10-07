//! Every stream in tests/data, its audio packets damaged -- lost, cut
//! short, bits flipped, replaced by noise, emptied, the decoder reset
//! midway -- eight ways each, against Tremor damaged the same ways
//! (`tools/reference.c`'s `damage`, `references.txt`).

#![allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "a test: a failure should be loud"
)]

mod common;

#[test]
fn damaged_streams_decode_as_tremor_decodes_them() {
    let text = std::fs::read_to_string(common::data("references.txt")).unwrap();
    let mut runs = 0;
    let mut failed = Vec::new();
    for line in text.lines() {
        let Some((key, want)) = line.split_once(" => ") else {
            continue;
        };
        let words: Vec<&str> = key.split(' ').collect();
        let [name, "damage", seed] = words[..] else {
            continue;
        };
        let packets = common::ogg_packets(&std::fs::read(common::data(name)).unwrap());
        let (got, _) = common::run(&packets, Some(seed.parse().unwrap()), false);
        if got != want {
            failed.push(format!("{name} damage {seed}\n   got {got}\n  want {want}"));
        }
        runs += 1;
    }
    assert!(runs >= 160, "{runs} runs");
    assert!(
        failed.is_empty(),
        "{} of {runs} runs differ:\n{}",
        failed.len(),
        failed.join("\n")
    );
}

/// The setup header damaged -- bits flipped, or cut short -- 24 ways for
/// each stream: refused where Tremor refuses it, and where Tremor takes it,
/// the first 24 packets decoded as Tremor decodes them. Where Tremor
/// crashes (a hostile header can make it divide by zero), the port need
/// only not.
#[test]
fn damaged_setups_are_read_as_tremor_reads_them() {
    let text = std::fs::read_to_string(common::data("references.txt")).unwrap();
    let (mut runs, mut taken, mut crashes) = (0, 0, 0);
    let mut failed = Vec::new();
    for line in text.lines() {
        let Some((key, want)) = line.split_once(" => ") else {
            continue;
        };
        let words: Vec<&str> = key.split(' ').collect();
        let [name, "setupdamage", seed] = words[..] else {
            continue;
        };
        let mut packets = common::ogg_packets(&std::fs::read(common::data(name)).unwrap());
        let mut lcg = common::Lcg(seed.parse().unwrap());
        common::damage_setup(&mut lcg, &mut packets[2]);
        runs += 1;
        let got = match vorbis::Decoder::new(&packets[0], &packets[2]) {
            Err(vorbis::Error::NotVorbis) => "headers refused 432".to_owned(),
            // Tremor refuses a book whose lengths make no tree when the
            // decoder starts, and the rest of a damaged setup as it reads
            // the header; the port refuses both as it reads it.
            Err(vorbis::Error::BadHeader) if want == "init refused" => want.to_owned(),
            Err(e) => format!("headers refused {}", 300 - e.code()),
            Ok(_) if want.starts_with("tremor crashed") => {
                crashes += 1;
                want.to_owned()
            }
            Ok(_) => {
                taken += 1;
                common::run_limited(&packets, None, false, Some(24)).0
            }
        };
        if got != want && !want.starts_with("tremor crashed") {
            failed.push(format!(
                "{name} setupdamage {seed}\n   got {got}\n  want {want}"
            ));
        }
    }
    assert!(runs >= 1000, "{runs} runs");
    assert!(taken >= 100, "only {taken} damaged setups were taken");
    println!("{runs} runs, {taken} setups taken, {crashes} that crash Tremor decoded");
    assert!(
        failed.is_empty(),
        "{} of {runs} runs differ:\n{}",
        failed.len(),
        failed.join("\n")
    );
}
