// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Mateus Rocha de Paula and the OxiGuitar contributors

//! The score as the model reads and writes it: a short text notation of
//! our own, the same both ways, compact enough for a model's context.
//!
//! A bar of one voice is its beats separated by `|`; a beat is its
//! duration, then what sounds:
//!
//! - durations: `w` `h` `q` `e` `s` `t` `x` (whole to sixty-fourth), a `.`
//!   for each dot, and `/3` for a triplet (`/5`, `/6`, `/7`, or `/5:3` for
//!   five in the time of three);
//! - a rest: `r`;
//! - a note on a staff with strings: `string/fret`, string 1 the highest
//!   (`3/5`); a dead note `3/x`; a tie from the note before: `~3/5`;
//! - a pitch, on a staff without strings or anywhere the model would
//!   rather give one: `C4`, `F#3`, `Bb2` (C4 is middle C); on a staff with
//!   strings OxiGuitar puts it on a string;
//! - a drum, on a drum track: its General MIDI key, `k36`;
//! - a chord: several in brackets, `(3/5 2/5 1/3)`.
//!
//! So `q 3/5 | e 3/7 | e r | h (3/5 2/5)` is a bar of 4/4. Everything here
//! is the plugin's own; OxiGuitar sees only the score it hands back.

use oxiguitar_plugin::score::BeatKind;
use oxiguitar_plugin::score::{
    BeatEffects, Clef, Duration, Dynamic, KeyMode, KeySignature, NoteEffects, NotePlace, NoteValue,
    Pitch, Ratio, TimeSignature, Tuplet,
};
use oxiguitar_plugin::score2::{Bar2, Beat2, MasterBar2, Note2, Score2, Voice2};

/// The most bars one read gives.
pub const MAX_READ: usize = 64;

const VALUES: [(char, NoteValue); 7] = [
    ('w', NoteValue::Whole),
    ('h', NoteValue::Half),
    ('q', NoteValue::Quarter),
    ('e', NoteValue::Eighth),
    ('s', NoteValue::Sixteenth),
    ('t', NoteValue::ThirtySecond),
    ('x', NoteValue::SixtyFourth),
];

const NAMES: [&str; 12] = [
    "C", "C#", "D", "Eb", "E", "F", "F#", "G", "Ab", "A", "Bb", "B",
];

/// A pitch's name: `C4` is middle C (MIDI 60).
pub fn pitch_name(p: u8) -> String {
    let octave = i32::from(p) / 12 - 1;
    format!("{}{octave}", NAMES[usize::from(p % 12)])
}

/// The pitch a name gives: a letter, any `#` or `b`, an octave (`-1` to
/// `9`).
pub fn parse_pitch(name: &str) -> Option<u8> {
    let mut chars = name.chars().peekable();
    let letter = chars.next()?.to_ascii_uppercase();
    let base: i32 = match letter {
        'C' => 0,
        'D' => 2,
        'E' => 4,
        'F' => 5,
        'G' => 7,
        'A' => 9,
        'B' => 11,
        _ => return None,
    };
    let mut shift = 0;
    while let Some(&c) = chars.peek() {
        match c {
            '#' | '♯' => shift += 1,
            'b' | '♭' => shift -= 1,
            _ => break,
        }
        chars.next();
    }
    let octave: i32 = chars.collect::<String>().parse().ok()?;
    let midi = (octave + 1) * 12 + base + shift;
    u8::try_from(midi).ok().filter(|m| *m <= 127)
}

/// A duration as its code.
pub fn duration_code(d: &Duration) -> String {
    let c = VALUES.iter().find(|(_, v)| *v == d.value).map_or_else(
        || match d.value {
            NoteValue::DoubleWhole => "W".to_owned(),
            _ => "x".to_owned(),
        },
        |(c, _)| c.to_string(),
    );
    let mut out = c;
    out.push_str(&".".repeat(usize::from(d.dots)));
    for t in &d.tuplets {
        if Some(t.in_time_of) == usual_time(t.notes) {
            out.push_str(&format!("/{}", t.notes));
        } else {
            out.push_str(&format!("/{}:{}", t.notes, t.in_time_of));
        }
    }
    out
}

/// What a tuplet of `notes` is usually in the time of.
fn usual_time(notes: u8) -> Option<u8> {
    match notes {
        3 => Some(2),
        5..=7 => Some(4),
        9 => Some(8),
        2 => Some(3),
        _ => None,
    }
}

/// The duration a code gives.
pub fn parse_duration(code: &str) -> Result<Duration, String> {
    let mut chars = code.chars();
    let first = chars.next().ok_or("a beat needs a duration")?;
    let value = if first == 'W' {
        NoteValue::DoubleWhole
    } else {
        VALUES
            .iter()
            .find(|(c, _)| *c == first)
            .map(|(_, v)| *v)
            .ok_or_else(|| format!("{code:?} isn't a duration (w h q e s t x)"))?
    };
    let rest: String = chars.collect();
    let dots = rest.chars().take_while(|c| *c == '.').count();
    let mut tuplets = Vec::new();
    for part in rest[dots..].split('/').skip(1) {
        let (notes, time) = match part.split_once(':') {
            Some((n, t)) => (n.parse::<u8>().ok(), t.parse::<u8>().ok()),
            None => {
                let n = part.parse::<u8>().ok();
                (n, n.and_then(usual_time))
            }
        };
        match (notes, time) {
            (Some(notes), Some(in_time_of)) if notes > 0 && in_time_of > 0 => {
                tuplets.push(Tuplet { notes, in_time_of });
            }
            _ => {
                return Err(format!(
                    "{code:?} has a tuplet that isn't /3, /5, /6, /7 or /N:M"
                ));
            }
        }
    }
    if !rest[dots..].is_empty() && tuplets.is_empty() {
        return Err(format!("{code:?} isn't a duration"));
    }
    if dots > 3 {
        return Err(format!("{code:?} has more than three dots"));
    }
    Ok(Duration {
        value,
        dots: u8::try_from(dots).unwrap_or(3),
        tuplets,
    })
}

/// How long a duration is, in whole notes.
pub fn length(d: &Duration) -> f64 {
    let base = match d.value {
        NoteValue::DoubleWhole => 2.0,
        NoteValue::Whole => 1.0,
        NoteValue::Half => 0.5,
        NoteValue::Quarter => 0.25,
        NoteValue::Eighth => 0.125,
        NoteValue::Sixteenth => 0.0625,
        NoteValue::ThirtySecond => 1.0 / 32.0,
        NoteValue::SixtyFourth => 1.0 / 64.0,
        NoteValue::OneTwentyEighth => 1.0 / 128.0,
        NoteValue::TwoFiftySixth => 1.0 / 256.0,
    };
    let mut l = base;
    let mut add = base;
    for _ in 0..d.dots {
        add /= 2.0;
        l += add;
    }
    for t in &d.tuplets {
        l *= f64::from(t.in_time_of) / f64::from(t.notes);
    }
    l
}

/// A note as text, on a staff with `strings` strings (0 for none) of a
/// drum track or not.
fn note_text(n: &Note2, drums: bool) -> String {
    let tie = if n.tie { "~" } else { "" };
    match n.place {
        NotePlace::Pitched(Pitch(p)) if drums => format!("{tie}k{p}"),
        NotePlace::Pitched(Pitch(p)) => format!("{tie}{}", pitch_name(p)),
        NotePlace::Fretted { string, .. } if n.effects.dead => format!("{tie}{string}/x"),
        NotePlace::Fretted { string, fret } => format!("{tie}{string}/{fret}"),
    }
}

/// A beat as text.
pub fn beat_text(b: &Beat2, drums: bool) -> String {
    let d = duration_code(&b.duration);
    let grace = if b.grace.is_some() { "g" } else { "" };
    let what = match b.kind {
        BeatKind::Rest | BeatKind::Empty => "r".to_owned(),
        BeatKind::Notes if b.notes.is_empty() => "r".to_owned(),
        BeatKind::Notes if b.notes.len() == 1 => note_text(&b.notes[0], drums),
        BeatKind::Notes => format!(
            "({})",
            b.notes
                .iter()
                .map(|n| note_text(n, drums))
                .collect::<Vec<_>>()
                .join(" ")
        ),
    };
    let text = if b.text.trim().is_empty() {
        String::new()
    } else {
        format!(" \"{}\"", b.text.trim().replace('"', "'"))
    };
    format!("{grace}{d} {what}{text}")
}

/// A voice's beats as one bar's text.
pub fn voice_text(v: &Voice2, drums: bool) -> String {
    if v.beats.is_empty() {
        return "(empty)".to_owned();
    }
    v.beats
        .iter()
        .map(|b| beat_text(b, drums))
        .collect::<Vec<_>>()
        .join(" | ")
}

/// A key signature's name.
pub fn key_name(k: &KeySignature) -> String {
    const MAJOR: [&str; 15] = [
        "Cb", "Gb", "Db", "Ab", "Eb", "Bb", "F", "C", "G", "D", "A", "E", "B", "F#", "C#",
    ];
    const MINOR: [&str; 15] = [
        "Ab", "Eb", "Bb", "F", "C", "G", "D", "A", "E", "B", "F#", "C#", "G#", "D#", "A#",
    ];
    let i = usize::try_from(i32::from(k.fifths.clamp(-7, 7)) + 7).unwrap_or(7);
    match k.mode {
        KeyMode::Minor => format!("{} minor", MINOR[i]),
        _ => format!("{} major", MAJOR[i]),
    }
}

/// The key signature a name gives: `G major`, `E minor`, `Bb major`.
pub fn parse_key(name: &str) -> Result<KeySignature, String> {
    let words: Vec<&str> = name.split_whitespace().collect();
    let (tonic, minor) = match words.as_slice() {
        [t] => (*t, false),
        [t, m] if m.eq_ignore_ascii_case("major") => (*t, false),
        [t, m] if m.eq_ignore_ascii_case("minor") => (*t, true),
        _ => {
            return Err(format!(
                "{name:?} isn't a key such as \"G major\" or \"E minor\""
            ));
        }
    };
    let pc = parse_pitch(&format!("{tonic}4")).ok_or_else(|| format!("{tonic:?} isn't a note"))?;
    // The major key whose tonic is `pc`, or a minor third up from a minor one.
    let major_pc = if minor { (pc + 3) % 12 } else { pc % 12 };
    let fifths: i8 = match major_pc {
        0 => 0,
        7 => 1,
        2 => 2,
        9 => 3,
        4 => 4,
        11 => 5,
        6 if tonic.contains('#') || (minor && tonic.starts_with(['D', 'd'])) => 6,
        6 => -6,
        1 if tonic.contains('#') => 7,
        1 => -5,
        8 => -4,
        3 => -3,
        10 => -2,
        5 => -1,
        _ => 0,
    };
    Ok(KeySignature {
        fifths,
        mode: if minor {
            KeyMode::Minor
        } else {
            KeyMode::Major
        },
    })
}

/// What a General MIDI program is, roughly, for the overview.
pub fn program_name(p: u8) -> &'static str {
    match p {
        0..=7 => "piano",
        8..=15 => "chromatic percussion",
        16..=23 => "organ",
        24 => "nylon guitar",
        25 => "steel guitar",
        26..=28 => "electric guitar (clean)",
        29 => "overdriven guitar",
        30 => "distorted guitar",
        31 => "guitar harmonics",
        32..=39 => "bass",
        40..=47 => "strings",
        48..=55 => "ensemble",
        56..=63 => "brass",
        64..=71 => "reed",
        72..=79 => "pipe",
        80..=87 => "synth lead",
        88..=95 => "synth pad",
        _ => "other",
    }
}

/// The score in a few lines: its information, tracks, bars and where time,
/// key, tempo and sections change.
pub fn overview(score: &Score2) -> String {
    let mut out = String::new();
    let i = &score.info;
    for (what, v) in [
        ("Title", &i.title),
        ("Artist", &i.artist),
        ("Album", &i.album),
    ] {
        if !v.trim().is_empty() {
            out.push_str(&format!("{what}: {}\n", v.trim()));
        }
    }
    out.push_str(&format!(
        "{} bars, {} tracks. Tempo {} at the start.\n",
        score.master_bars.len(),
        score.tracks.len(),
        score.initial_tempo
    ));
    for (n, t) in score.tracks.iter().enumerate() {
        let what = if t.instrument.percussion {
            "drum kit (notes are General MIDI keys)".to_owned()
        } else {
            format!(
                "{} (General MIDI program {})",
                program_name(t.instrument.midi_program),
                t.instrument.midi_program
            )
        };
        out.push_str(&format!("Track {} \"{}\": {what}", n + 1, t.name));
        for (s, staff) in t.staves.iter().enumerate() {
            if t.staves.len() > 1 {
                out.push_str(&format!("; staff {}", s + 1));
            }
            if !t.instrument.percussion && !staff.tuning.is_empty() {
                let tuning: Vec<String> = staff.tuning.iter().map(|p| pitch_name(p.0)).collect();
                out.push_str(&format!(
                    "; {} strings tuned {} (string 1 first, the highest)",
                    staff.tuning.len(),
                    tuning.join(" ")
                ));
                if staff.capo > 0 {
                    out.push_str(&format!(", capo at fret {}", staff.capo));
                }
            } else if !t.instrument.percussion {
                out.push_str("; no strings (notes are pitches)");
            }
        }
        if t.mix.mute {
            out.push_str("; muted");
        }
        out.push_str(".\n");
    }
    let mut changes: Vec<String> = Vec::new();
    let mut last: Option<&MasterBar2> = None;
    for (n, m) in score.master_bars.iter().enumerate() {
        let mut parts = Vec::new();
        if last.is_none_or(|l| l.time_signature != m.time_signature) {
            parts.push(format!(
                "{}/{}",
                m.time_signature.numerator, m.time_signature.denominator
            ));
        }
        if last.is_none_or(|l| l.key_signature != m.key_signature) {
            parts.push(key_name(&m.key_signature));
        }
        for t in &m.tempo_changes {
            parts.push(format!("tempo {}", t.bpm));
        }
        if let Some(marker) = &m.marker {
            parts.push(format!("section {} \"{}\"", marker.letter, marker.text));
        }
        if m.repeat_start {
            parts.push("repeat starts".to_owned());
        }
        if let Some(times) = m.repeat_end {
            parts.push(format!("repeat ends (played {times} times)"));
        }
        if !parts.is_empty() {
            changes.push(format!("bar {}: {}", n + 1, parts.join(", ")));
        }
        last = Some(m);
    }
    if !changes.is_empty() {
        out.push_str("Changes: ");
        out.push_str(&changes.join("; "));
        out.push_str(".\n");
    }
    out
}

/// Bars `from` to `to` (from 1, both included) of a track's staff (from 1),
/// every voice that has beats, as text.
pub fn read_bars(
    score: &Score2,
    track: usize,
    staff: usize,
    from: usize,
    to: usize,
) -> Result<String, String> {
    let t = score.tracks.get(track.wrapping_sub(1)).ok_or_else(|| {
        format!(
            "there is no track {track}: the score has {}",
            score.tracks.len()
        )
    })?;
    let s = t
        .staves
        .get(staff.wrapping_sub(1))
        .ok_or_else(|| format!("track {track} has no staff {staff}"))?;
    let bars = score.master_bars.len();
    if from == 0 || from > bars || to < from {
        return Err(format!("the score has bars 1 to {bars}"));
    }
    let to = to.min(bars).min(from + MAX_READ - 1);
    let drums = t.instrument.percussion;
    let mut out = format!(
        "Track {track} \"{}\", staff {staff}, bars {from}-{to}{}:\n",
        t.name,
        if drums {
            " (drums: notes are General MIDI keys)"
        } else if s.tuning.is_empty() {
            " (notes are pitches)"
        } else {
            " (notes are string/fret, string 1 the highest)"
        }
    );
    for n in from..=to {
        let m = &score.master_bars[n - 1];
        out.push_str(&format!(
            "bar {n} ({}/{}):",
            m.time_signature.numerator, m.time_signature.denominator
        ));
        let bar = &s.bars[n - 1];
        let voices: Vec<(usize, &Voice2)> = bar
            .voices
            .iter()
            .enumerate()
            .filter(|(i, v)| *i == 0 || !v.beats.is_empty())
            .collect();
        if voices.len() == 1 {
            out.push(' ');
            out.push_str(&voice_text(voices[0].1, drums));
        } else {
            for (i, v) in voices {
                out.push_str(&format!(" v{}: {}", i + 1, voice_text(v, drums)));
            }
        }
        out.push('\n');
    }
    if to < bars && to - from + 1 == MAX_READ {
        out.push_str(&format!(
            "(at most {MAX_READ} bars a read: ask for more from bar {})\n",
            to + 1
        ));
    }
    Ok(out)
}

/// A note the model wrote, before it is placed on the staff.
#[derive(Clone, Debug, PartialEq)]
enum Wrote {
    Fret { string: u8, fret: Option<u8> },
    Pitch(u8),
    Drum(u8),
}

fn parse_note(token: &str) -> Result<(Wrote, bool), String> {
    let (tie, t) = match token.strip_prefix('~') {
        Some(rest) => (true, rest),
        None => (false, token),
    };
    if let Some(key) = t.strip_prefix('k').or_else(|| t.strip_prefix('K')) {
        let key: u8 = key
            .parse()
            .ok()
            .filter(|k| (27..=87).contains(k))
            .ok_or_else(|| format!("{token:?}: a drum is a General MIDI key, k27 to k87"))?;
        return Ok((Wrote::Drum(key), tie));
    }
    if let Some((s, f)) = t.split_once('/') {
        let string: u8 = s
            .parse()
            .ok()
            .filter(|s| *s > 0)
            .ok_or_else(|| format!("{token:?}: the string is a number from 1"))?;
        let fret =
            if f.eq_ignore_ascii_case("x") {
                None
            } else {
                Some(f.parse::<u8>().map_err(|_| {
                    format!("{token:?}: the fret is a number, or x for a dead note")
                })?)
            };
        return Ok((Wrote::Fret { string, fret }, tie));
    }
    parse_pitch(t)
        .map(|p| (Wrote::Pitch(p), tie))
        .ok_or_else(|| format!("{token:?} isn't a note (string/fret, a pitch like C4, or k36)"))
}

/// One bar of one voice from its text: the beats, or the first problem.
/// A beat as read: its duration, its notes (each with whether it is tied)
/// and whether it is a grace note.
type ReadBeat = (Duration, Vec<(Wrote, bool)>, bool);

fn parse_bar(text: &str) -> Result<Vec<ReadBeat>, String> {
    let text = text.trim();
    if text.is_empty() || text == "(empty)" {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    for beat in text.split('|') {
        let beat = beat.trim();
        // Text in quotes (a beat's text) is dropped: the model writes music.
        let beat = match beat.find('"') {
            Some(i) => beat[..i].trim(),
            None => beat,
        };
        let (code, rest) = beat.split_once(char::is_whitespace).ok_or_else(|| {
            format!("{beat:?}: a beat is a duration and what sounds, as \"q 3/5\"")
        })?;
        let (grace, code) = match code.strip_prefix('g') {
            Some(c) if !c.is_empty() => (true, c),
            _ => (false, code),
        };
        let duration = parse_duration(code)?;
        let rest = rest.trim();
        let notes = if rest == "r" || rest == "-" {
            Vec::new()
        } else {
            let inner = rest
                .strip_prefix('(')
                .and_then(|r| r.strip_suffix(')'))
                .unwrap_or(rest);
            inner
                .split_whitespace()
                .map(parse_note)
                .collect::<Result<Vec<_>, _>>()?
        };
        out.push((duration, notes, grace));
    }
    Ok(out)
}

/// A string and fret for `pitch` on a staff tuned `tuning` (string 1 first)
/// with `frets` frets, avoiding strings already used: the lowest fret, so
/// open strings and the first position come first.
fn finger(pitch: u8, tuning: &[Pitch], capo: u8, frets: u8, used: &[u8]) -> Option<(u8, u8)> {
    tuning
        .iter()
        .enumerate()
        .filter_map(|(i, open)| {
            let string = u8::try_from(i + 1).ok()?;
            if used.contains(&string) {
                return None;
            }
            let open = open.0.checked_add(capo)?;
            let fret = pitch.checked_sub(open)?;
            (fret <= frets).then_some((string, fret))
        })
        .min_by_key(|(_, fret)| *fret)
}

/// What [`write_bars`] did, for the model.
#[derive(Debug, Default, PartialEq)]
pub struct Written {
    /// Bars written.
    pub bars: usize,
    /// Bars added at the end of the score to hold them.
    pub added: usize,
    /// Bars whose beats don't add up to their time signature, and how far.
    pub uneven: Vec<String>,
}

/// Writes `bars` (each one bar of text) into a track's staff and voice
/// (all from 1) from bar `from` on, replacing what that voice held there,
/// and adds bars at the end of the score for those past it. Nothing
/// changes when any bar can't be read: the error names it.
pub fn write_bars(
    score: &mut Score2,
    track: usize,
    staff: usize,
    voice: usize,
    from: usize,
    bars: &[String],
) -> Result<Written, String> {
    let tracks = score.tracks.len();
    let t = score
        .tracks
        .get(track.wrapping_sub(1))
        .ok_or_else(|| format!("there is no track {track}: the score has {tracks}"))?;
    let s = t
        .staves
        .get(staff.wrapping_sub(1))
        .ok_or_else(|| format!("track {track} has no staff {staff}"))?;
    if !(1..=4).contains(&voice) {
        return Err("the voice is 1 to 4".to_owned());
    }
    let count = score.master_bars.len();
    if from == 0 || from > count + 1 {
        return Err(format!(
            "the first bar is 1 to {} (the score has {count})",
            count + 1
        ));
    }
    if bars.is_empty() {
        return Err("no bars to write".to_owned());
    }
    if from + bars.len() - 1 > count + 256 {
        return Err("at most 256 bars can be added at once".to_owned());
    }
    let drums = t.instrument.percussion;
    let (tuning, capo, frets) = (s.tuning.clone(), s.capo, s.fret_count.max(12));
    // Read every bar first: nothing changes if one is wrong.
    let mut parsed = Vec::with_capacity(bars.len());
    for (i, text) in bars.iter().enumerate() {
        let n = from + i;
        let beats = parse_bar(text).map_err(|e| format!("bar {n}: {e}"))?;
        let mut made = Vec::with_capacity(beats.len());
        for (duration, notes, grace) in beats {
            let mut used: Vec<u8> = Vec::new();
            let mut out_notes = Vec::new();
            for (wrote, tie) in notes {
                let (place, dead) = match wrote {
                    Wrote::Drum(k) if drums => (NotePlace::Pitched(Pitch(k)), false),
                    Wrote::Drum(_) => {
                        return Err(format!(
                            "bar {n}: k-keys are for drum tracks; track {track} isn't one"
                        ));
                    }
                    Wrote::Pitch(p) if drums => (NotePlace::Pitched(Pitch(p)), false),
                    Wrote::Pitch(p) if tuning.is_empty() => (NotePlace::Pitched(Pitch(p)), false),
                    Wrote::Pitch(p) => {
                        let (string, fret) =
                            finger(p, &tuning, capo, frets, &used).ok_or_else(|| {
                                format!(
                                    "bar {n}: {} can't be played on track {track}'s strings",
                                    pitch_name(p)
                                )
                            })?;
                        (NotePlace::Fretted { string, fret }, false)
                    }
                    Wrote::Fret { .. } if drums || tuning.is_empty() => {
                        return Err(format!(
                            "bar {n}: track {track} has no strings: write pitches (C4){}",
                            if drums { " or drum keys (k36)" } else { "" }
                        ));
                    }
                    Wrote::Fret { string, fret } => {
                        if usize::from(string) > tuning.len() {
                            return Err(format!(
                                "bar {n}: track {track} has {} strings, not {string}",
                                tuning.len()
                            ));
                        }
                        if fret.is_some_and(|f| f > frets) {
                            return Err(format!("bar {n}: the frets go up to {frets}"));
                        }
                        (
                            NotePlace::Fretted {
                                string,
                                fret: fret.unwrap_or(0),
                            },
                            fret.is_none(),
                        )
                    }
                };
                if let NotePlace::Fretted { string, .. } = place {
                    if used.contains(&string) {
                        return Err(format!("bar {n}: string {string} plays twice in one beat"));
                    }
                    used.push(string);
                }
                out_notes.push(Note2 {
                    place,
                    tie,
                    dynamic: Dynamic::MF,
                    effects: NoteEffects {
                        dead,
                        ..NoteEffects::default()
                    },
                    ext: Vec::new(),
                });
            }
            made.push(Beat2 {
                kind: if out_notes.is_empty() {
                    BeatKind::Rest
                } else {
                    BeatKind::Notes
                },
                duration,
                notes: out_notes,
                grace: grace.then_some(oxiguitar_plugin::score::GraceKind::BeforeBeat),
                effects: BeatEffects::default(),
                chord: None,
                text: String::new(),
                ext: Vec::new(),
            });
        }
        parsed.push(made);
    }
    let mut written = Written::default();
    let needed = from + bars.len() - 1;
    while score.master_bars.len() < needed {
        add_bar(score);
        written.added += 1;
    }
    for (i, beats) in parsed.into_iter().enumerate() {
        let n = from + i;
        let m = &score.master_bars[n - 1];
        let wanted =
            f64::from(m.time_signature.numerator) / f64::from(m.time_signature.denominator.max(1));
        let have: f64 = beats
            .iter()
            .filter(|b| b.grace.is_none())
            .map(|b| length(&b.duration))
            .sum();
        if !beats.is_empty() && !m.free_time() && (have - wanted).abs() > 1e-9 {
            written.uneven.push(format!(
                "bar {n} holds {} of a whole note, its {}/{} needs {}",
                round(have),
                m.time_signature.numerator,
                m.time_signature.denominator,
                round(wanted)
            ));
        }
        let bar = &mut score.tracks[track - 1].staves[staff - 1].bars[n - 1];
        while bar.voices.len() < voice {
            bar.voices.push(Voice2::default());
        }
        bar.voices[voice - 1].beats = beats;
        written.bars += 1;
    }
    Ok(written)
}

fn round(v: f64) -> String {
    let r = (v * 1000.0).round() / 1000.0;
    format!("{r}")
}

/// Adds a bar at the end of the score, in the last bar's time, key and
/// feel, an empty voice on every staff in its last bar's clef.
pub fn add_bar(score: &mut Score2) {
    let last = score.master_bars.last().cloned();
    score.master_bars.push(MasterBar2 {
        time_signature: last.as_ref().map_or(
            TimeSignature {
                numerator: 4,
                denominator: 4,
            },
            |l| l.time_signature,
        ),
        key_signature: last.as_ref().map_or(
            KeySignature {
                fifths: 0,
                mode: KeyMode::Major,
            },
            |l| l.key_signature,
        ),
        repeat_start: false,
        repeat_end: None,
        alternate_endings: 0,
        marker: None,
        double_bar: false,
        triplet_feel: last
            .as_ref()
            .map_or(oxiguitar_plugin::score::TripletFeel::None, |l| {
                l.triplet_feel
            }),
        tempo_changes: Vec::new(),
        directions: Vec::new(),
        ext: Vec::new(),
    });
    for t in &mut score.tracks {
        for s in &mut t.staves {
            let clef = s.bars.last().map_or(Clef::Treble8vb, |b| b.clef);
            s.bars.push(empty_bar(clef));
        }
    }
}

/// A bar with one empty voice.
pub fn empty_bar(clef: Clef) -> Bar2 {
    Bar2 {
        clef,
        voices: vec![Voice2::default()],
        ext: Vec::new(),
    }
}

/// A tempo change at the start of a bar.
pub fn tempo_at_start(bpm: f64) -> oxiguitar_plugin::score::TempoChange {
    oxiguitar_plugin::score::TempoChange {
        position: Ratio { num: 0, den: 1 },
        bpm,
        label: String::new(),
        linear: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use oxiguitar_plugin::score::{Instrument, Mix};
    use oxiguitar_plugin::score2::{Staff2, Track2};

    fn song() -> Score2 {
        let mut s = Score2 {
            initial_tempo: 100.0,
            ..Score2::default()
        };
        s.info.title = "Song".into();
        s.tracks.push(Track2 {
            name: "Lead".into(),
            instrument: Instrument {
                midi_program: 29,
                percussion: false,
            },
            mix: Mix::default(),
            staves: vec![Staff2 {
                tuning: [64, 59, 55, 50, 45, 40].map(Pitch).to_vec(),
                fret_count: 24,
                ..Staff2::default()
            }],
            ..Track2::default()
        });
        s.tracks.push(Track2 {
            name: "Drums".into(),
            instrument: Instrument {
                midi_program: 0,
                percussion: true,
            },
            staves: vec![Staff2::default()],
            ..Track2::default()
        });
        add_bar(&mut s);
        s
    }

    #[test]
    fn pitches_and_durations_read_and_write() {
        assert_eq!(pitch_name(60), "C4");
        assert_eq!(pitch_name(40), "E2");
        for p in [0u8, 40, 60, 61, 127] {
            assert_eq!(parse_pitch(&pitch_name(p)), Some(p));
        }
        assert_eq!(parse_pitch("Bb2"), Some(46));
        assert_eq!(parse_pitch("H2"), None);
        for code in ["q", "e.", "h..", "e/3", "s/5", "q/5:3"] {
            assert_eq!(duration_code(&parse_duration(code).unwrap()), code);
        }
        assert!(parse_duration("z").is_err());
        assert!(parse_duration("q/0").is_err());
        assert!((length(&parse_duration("q.").unwrap()) - 0.375).abs() < 1e-12);
        assert!((length(&parse_duration("e/3").unwrap()) - 1.0 / 12.0).abs() < 1e-12);
    }

    #[test]
    fn keys_read_and_write() {
        for (name, fifths) in [
            ("C major", 0),
            ("G major", 1),
            ("E minor", 1),
            ("Bb major", -2),
            ("D minor", -1),
            ("F# major", 6),
            ("Eb minor", -6),
        ] {
            let k = parse_key(name).unwrap();
            assert_eq!(k.fifths, fifths, "{name}");
            assert_eq!(key_name(&k), name);
        }
        assert!(parse_key("blue").is_err());
    }

    #[test]
    fn bars_written_read_back_the_same() {
        let mut s = song();
        let w = write_bars(
            &mut s,
            1,
            1,
            1,
            1,
            &[
                "q 3/5 | e 3/7 | e r | h (3/5 2/5)".into(),
                "q ~3/5 | q 6/x | h E2".into(),
            ],
        )
        .unwrap();
        assert_eq!(
            w,
            Written {
                bars: 2,
                added: 1,
                uneven: vec![]
            }
        );
        let text = read_bars(&s, 1, 1, 1, 2).unwrap();
        assert!(
            text.contains("bar 1 (4/4): q 3/5 | e 3/7 | e r | h (3/5 2/5)"),
            "{text}"
        );
        // E2 is the open low string.
        assert!(
            text.contains("bar 2 (4/4): q ~3/5 | q 6/x | h 6/0"),
            "{text}"
        );
        write_bars(
            &mut s,
            2,
            1,
            1,
            1,
            &["q k36 | q k38 | q (k36 k42) | q k38".into()],
        )
        .unwrap();
        let drums = read_bars(&s, 2, 1, 1, 1).unwrap();
        assert!(drums.contains("q (k36 k42)"), "{drums}");
        assert_eq!(s.master_bars.len(), 2);
        assert_eq!(s.tracks[1].staves[0].bars.len(), 2);
    }

    #[test]
    fn uneven_bars_are_written_and_said() {
        let mut s = song();
        let w = write_bars(&mut s, 1, 1, 1, 1, &["q 3/5 | q 3/7".into()]).unwrap();
        assert_eq!(
            w.uneven,
            ["bar 1 holds 0.5 of a whole note, its 4/4 needs 1"]
        );
    }

    #[test]
    fn wrong_bars_change_nothing_and_say_why() {
        let s0 = song();
        for (track, bars, why) in [
            (1, "q 7/5", "6 strings"),
            (1, "q 3/30", "frets go up to 24"),
            (1, "q (3/5 3/7)", "plays twice"),
            (1, "q k36", "drum tracks"),
            (2, "q 3/5", "no strings"),
            (1, "z 3/5", "isn't a duration"),
            (1, "q", "a duration and what sounds"),
            (1, "q C9", "can't be played"),
            (3, "q r", "no track 3"),
        ] {
            let mut s = s0.clone();
            let e = write_bars(&mut s, track, 1, 1, 1, &[bars.to_owned()]).unwrap_err();
            assert!(e.contains(why), "{bars}: {e}");
            assert_eq!(s, s0);
        }
    }

    #[test]
    fn the_overview_names_tracks_tunings_and_changes() {
        let mut s = song();
        s.master_bars[0].marker = Some(oxiguitar_plugin::score::Marker {
            letter: "A".into(),
            text: "Intro".into(),
            color: Default::default(),
        });
        let o = overview(&s);
        assert!(o.contains("Title: Song"), "{o}");
        assert!(o.contains("Track 1 \"Lead\": overdriven guitar"), "{o}");
        assert!(o.contains("6 strings tuned E4 B3 G3 D3 A2 E2"), "{o}");
        assert!(o.contains("Track 2 \"Drums\": drum kit"), "{o}");
        assert!(
            o.contains("bar 1: 4/4, C major, section A \"Intro\""),
            "{o}"
        );
    }
}
