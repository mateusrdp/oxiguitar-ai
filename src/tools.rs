// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Mateus Rocha de Paula and the OxiGuitar contributors

//! What the model may do, as the tools (functions) it is given: read the
//! score, write bars, add tracks, change bars' time, key, tempo and
//! sections, read and move the selection, and list and run OxiGuitar's
//! tools and other plugins'. Each takes JSON arguments and answers with
//! text for the model.
//!
//! Changes are made on a copy of the score and handed back to OxiGuitar as
//! one change the user can undo ("AI: …"), at the end of the model's
//! answer, or before a tool of OxiGuitar's runs or the selection moves, so
//! those see them.

use oxiguitar_plugin::api::event::Position;
use oxiguitar_plugin::score::{Color, Instrument, Marker, Mix, Pitch, TimeSignature};
use oxiguitar_plugin::score2::{Score2, Staff2, Track2};
use oxiguitar_plugin::serde_json::{self, Map, Value, json};
use oxiguitar_plugin::{Host, Selection2, ToolEntry};

use crate::notation;
use crate::skills::{self, Skill};

/// What the tools reach of OxiGuitar: the host, or a fake in tests.
pub trait Editor {
    /// The open score and its revision.
    fn score(&self) -> Result<Option<(u64, Score2)>, String>;
    /// Hands back the score changed, as one change named `name`.
    fn apply(&self, revision: u64, name: &str, score: Score2) -> Result<(), String>;
    /// The caret or the selection.
    fn selection(&self) -> Result<Option<Selection2>, String>;
    /// Moves the caret or the selection.
    fn select(&self, selection: Selection2) -> Result<(), String>;
    /// Every tool there is.
    fn tools(&self) -> Result<Vec<ToolEntry>, String>;
    /// Runs one.
    fn run_tool(&self, owner: &str, tool: &str, options: Option<&Value>) -> Result<String, String>;
    /// The folders the user's skills are in, the user's own first.
    fn skill_folders(&self) -> Vec<std::path::PathBuf> {
        Vec::new()
    }
}

impl Editor for Host {
    fn score(&self) -> Result<Option<(u64, Score2)>, String> {
        self.score2().map_err(|e| e.message)
    }
    fn apply(&self, revision: u64, name: &str, score: Score2) -> Result<(), String> {
        Host::apply(self, revision, name, score).map_err(|e| e.message)
    }
    fn selection(&self) -> Result<Option<Selection2>, String> {
        Host::selection(self).map_err(|e| e.message)
    }
    fn select(&self, selection: Selection2) -> Result<(), String> {
        Host::select(self, selection).map_err(|e| e.message)
    }
    fn tools(&self) -> Result<Vec<ToolEntry>, String> {
        Host::tools(self).map_err(|e| e.message)
    }
    fn run_tool(&self, owner: &str, tool: &str, options: Option<&Value>) -> Result<String, String> {
        Host::run_tool(self, owner, tool, options).map_err(|e| e.message)
    }
    fn skill_folders(&self) -> Vec<std::path::PathBuf> {
        self.folder(crate::SKILLS)
            .map(|f| f.all().map(std::path::Path::to_path_buf).collect())
            .unwrap_or_default()
    }
}

/// The tools, as a chat completions request's `tools`.
pub fn definitions(skills: &[Skill]) -> Value {
    let f = |name: &str, description: &str, parameters: Value| {
        json!({"type": "function", "function": {
            "name": name, "description": description, "parameters": parameters}})
    };
    let none = json!({"type": "object", "properties": {}});
    let int = |d: &str| json!({"type": "integer", "minimum": 1, "description": d});
    let mut tools = vec![
        f(
            "overview",
            "The open score in a few lines: title, tracks (instrument, strings and tuning), how many bars, and where the time, key, tempo and sections change. Read it first.",
            none.clone(),
        ),
        f(
            "read_bars",
            "The notes of bars of one track, in the notation of the instructions. At most 64 bars a call.",
            json!({"type": "object", "properties": {
                "track": int("The track, from 1"),
                "from_bar": int("The first bar, from 1"),
                "to_bar": int("The last bar, from 1"),
                "staff": int("The staff in the track, from 1; 1 unless the track has more")
            }, "required": ["track", "from_bar", "to_bar"]}),
        ),
        f(
            "write_bars",
            "Writes bars of one track's voice, replacing what it held there: one string of the notation a bar. Bars past the end of the score are added. Nothing changes if a bar can't be read; the answer says which and why, and which bars don't add up to their time signature.",
            json!({"type": "object", "properties": {
                "track": int("The track, from 1"),
                "from_bar": int("The first bar to write, from 1"),
                "bars": {"type": "array", "items": {"type": "string"}, "description": "One bar each, such as \"q 3/5 | q 3/7 | h (3/5 2/5)\""},
                "voice": int("The voice, 1 to 4; 1 unless writing a second voice"),
                "staff": int("The staff in the track, from 1")
            }, "required": ["track", "from_bar", "bars"]}),
        ),
        f(
            "add_track",
            "Adds a track at the end, with as many empty bars as the score. Its notes are then written with write_bars.",
            json!({"type": "object", "properties": {
                "name": {"type": "string"},
                "kind": {"type": "string", "enum": ["guitar", "bass", "drums", "keys", "other"]},
                "program": {"type": "integer", "minimum": 0, "maximum": 127, "description": "Its General MIDI program (not for drums); a usual one for the kind when absent"},
                "tuning": {"type": "string", "description": "Its strings' open notes from string 1, the highest, down: \"E4 B3 G3 D3 A2 E2\". Standard for the kind when absent; empty for an instrument without strings"}
            }, "required": ["name", "kind"]}),
        ),
        f(
            "set_bars",
            "Changes bars of every track: their time signature, key signature, tempo (at the first bar) or section (at the first bar).",
            json!({"type": "object", "properties": {
                "from_bar": int("The first bar, from 1"),
                "to_bar": int("The last bar, from 1; the first when absent"),
                "time": {"type": "string", "description": "Such as \"3/4\""},
                "key": {"type": "string", "description": "Such as \"G major\" or \"E minor\""},
                "tempo": {"type": "number", "description": "Beats a minute"},
                "section": {"type": "string", "description": "The section's name, such as \"Chorus\"; empty to remove it"}
            }, "required": ["from_bar"]}),
        ),
        f(
            "set_info",
            "Changes the score's title, artist or album.",
            json!({"type": "object", "properties": {
                "title": {"type": "string"}, "artist": {"type": "string"}, "album": {"type": "string"}
            }}),
        ),
        f(
            "get_selection",
            "Where the user's caret is, or what they selected: what \"this\", \"here\" and \"the selection\" mean.",
            none.clone(),
        ),
        f(
            "select",
            "Selects whole bars of tracks, as the user would, so that a tool of OxiGuitar's (run_tool) works on them.",
            json!({"type": "object", "properties": {
                "first_track": int("From 1"), "last_track": int("From 1"),
                "first_bar": int("From 1"), "last_bar": int("From 1")
            }, "required": ["first_track", "last_track", "first_bar", "last_bar"]}),
        ),
        f(
            "list_tools",
            "OxiGuitar's tools and other plugins' (transposing, fingering, fitting bars to their time signature, finding chords and more), with the options that run each as told.",
            none,
        ),
        f(
            "run_tool",
            "Runs a tool from list_tools, as the user would. With options as its schema says, it runs as told; without, a tool with a dialog asks the user.",
            json!({"type": "object", "properties": {
                "owner": {"type": "string", "description": "The tool's owner, as list_tools gives it"},
                "tool": {"type": "string", "description": "Its id"},
                "options": {"type": "object", "description": "Its options, as its schema says"}
            }, "required": ["owner", "tool"]}),
        ),
    ];
    let names: Vec<&str> = skills
        .iter()
        .filter(|s| !s.always)
        .map(|s| s.name.as_str())
        .collect();
    if !names.is_empty() {
        tools.push(f(
            "use_skill",
            "Reads one of the user's skills, notes of their own on how they want things done. Call it when a request fits one, before you write.",
            json!({"type": "object", "properties": {
                "name": {"type": "string", "enum": names, "description": "The skill's name"}
            }, "required": ["name"]}),
        ));
    }
    Value::Array(tools)
}

/// What the plugin tells the model about OxiGuitar and its notation.
pub const INSTRUCTIONS: &str = "You are the assistant inside OxiGuitar, a guitar tablature editor. You talk with the user about the score they have open, and you can read it and change it with your tools. Read before you write: call overview, then read_bars where you need the notes. Bars, tracks, staves, voices and strings count from 1.

The notation, the same for reading and writing: a bar is its beats separated by |, a beat its duration and then what sounds. Durations: w h q e s t x (whole, half, quarter, eighth, sixteenth, thirty-second, sixty-fourth), a dot each after it for dotted (q.), /3 for a triplet (e/3). What sounds: r for a rest; string/fret on a track with strings, string 1 the highest (3/5 is string 3, fret 5; 6/0 the open low string); 3/x a dead note; ~3/5 tied from the note before; a pitch such as C4 (middle C), F#3, Bb2, on a track without strings (or on one with them: OxiGuitar picks a string); k36 a drum, its General MIDI key, on a drum track (k36 kick, k38 snare, k42 closed hi-hat, k46 open hi-hat, k49 crash, k51 ride, k45 k47 k48 k50 toms); several at once in brackets: (3/5 2/5 1/3). Example of a 4/4 bar: q 3/5 | e 3/7 | e r | h (3/5 2/5). Make every bar add up to its time signature.

Your changes are kept as one change the user can undo with Edit, Undo. Say briefly what you changed. Don't make up what the score holds: read it. Answer in the user's language, in plain text without Markdown tables.";

/// The tools' copy of the score while the model works, and what it changed.
pub struct Workspace<'a> {
    editor: &'a dyn Editor,
    skills: Vec<Skill>,
    copy: Option<(u64, Score2)>,
    dirty: bool,
    changes: Vec<String>,
}

impl<'a> Workspace<'a> {
    /// A workspace over `editor`, which holds no copy until a tool asks.
    pub fn new(editor: &'a dyn Editor) -> Workspace<'a> {
        Workspace {
            editor,
            skills: Vec::new(),
            copy: None,
            dirty: false,
            changes: Vec::new(),
        }
    }

    /// The same, with the user's skills the model may load.
    pub fn with_skills(mut self, skills: Vec<Skill>) -> Workspace<'a> {
        self.skills = skills;
        self
    }

    fn score(&mut self) -> Result<&mut Score2, String> {
        if self.copy.is_none() {
            self.copy = self.editor.score()?;
        }
        self.copy.as_mut().map(|(_, s)| s).ok_or_else(|| {
            "no score is open: the user can open one or start a new one with File, New".to_owned()
        })
    }

    fn changed(&mut self, what: String) {
        self.dirty = true;
        self.changes.push(what);
    }

    /// Hands the changes back to OxiGuitar as one change; what it was
    /// named, if anything changed.
    pub fn flush(&mut self) -> Result<Option<String>, String> {
        if !self.dirty {
            self.copy = None;
            return Ok(None);
        }
        let Some((revision, score)) = self.copy.take() else {
            return Ok(None);
        };
        self.dirty = false;
        let mut name = format!("AI: {}", self.changes.join(", "));
        if name.chars().count() > 60 {
            name = format!("AI: {} changes", self.changes.len());
        }
        self.changes.clear();
        self.editor
            .apply(revision, &name, score)
            .map_err(|e| format!("the changes couldn't be kept: {e}"))?;
        Ok(Some(name))
    }

    /// Runs the tool `name` with `arguments` (JSON text): its answer for
    /// the model, an error as a sentence.
    pub fn call(&mut self, name: &str, arguments: &str) -> String {
        let args: Map<String, Value> = match serde_json::from_str::<Value>(arguments) {
            Ok(Value::Object(m)) => m,
            Ok(Value::Null) => Map::new(),
            _ if arguments.trim().is_empty() => Map::new(),
            _ => return format!("Error: the arguments of {name} aren't a JSON object."),
        };
        match self.dispatch(name, &args) {
            Ok(text) => text,
            Err(e) => format!("Error: {e}."),
        }
    }

    fn dispatch(&mut self, name: &str, args: &Map<String, Value>) -> Result<String, String> {
        match name {
            "overview" => Ok(notation::overview(self.score()?)),
            "use_skill" => {
                let name = args.get("name").and_then(Value::as_str).unwrap_or_default();
                match skills::find(&self.skills, name) {
                    Some(skill) => Ok(skill.text.clone()),
                    None => {
                        let names: Vec<&str> = self
                            .skills
                            .iter()
                            .filter(|s| !s.always)
                            .map(|s| s.name.as_str())
                            .collect();
                        Err(format!(
                            "there is no skill called {name:?}; the skills are {}",
                            names.join(", ")
                        ))
                    }
                }
            }
            "read_bars" => {
                let track = number(args, "track")?;
                let from = number(args, "from_bar")?;
                let to = optional(args, "to_bar")?.unwrap_or(from);
                let staff = optional(args, "staff")?.unwrap_or(1);
                notation::read_bars(self.score()?, track, staff, from, to)
            }
            "write_bars" => {
                let track = number(args, "track")?;
                let from = number(args, "from_bar")?;
                let voice = optional(args, "voice")?.unwrap_or(1);
                let staff = optional(args, "staff")?.unwrap_or(1);
                let bars: Vec<String> = args
                    .get("bars")
                    .and_then(Value::as_array)
                    .ok_or("bars is a list of strings, one a bar")?
                    .iter()
                    .map(|b| {
                        b.as_str()
                            .map(str::to_owned)
                            .ok_or("bars is a list of strings")
                    })
                    .collect::<Result<_, _>>()?;
                let w = notation::write_bars(self.score()?, track, staff, voice, from, &bars)?;
                let last = from + w.bars - 1;
                self.changed(format!("track {track} bars {from}-{last}"));
                let mut out = format!("Wrote bars {from} to {last} of track {track}.");
                if w.added > 0 {
                    out.push_str(&format!(" Added {} bars at the end of the score.", w.added));
                }
                if !w.uneven.is_empty() {
                    out.push_str(&format!(
                        " These don't add up to their time signature, rewrite them: {}.",
                        w.uneven.join("; ")
                    ));
                }
                Ok(out)
            }
            "add_track" => self.add_track(args),
            "set_bars" => self.set_bars(args),
            "set_info" => {
                let score = self.score()?;
                let mut set = Vec::new();
                for (key, field) in [
                    ("title", &mut score.info.title),
                    ("artist", &mut score.info.artist),
                    ("album", &mut score.info.album),
                ] {
                    if let Some(v) = args.get(key).and_then(Value::as_str) {
                        *field = v.to_owned();
                        set.push(key);
                    }
                }
                if set.is_empty() {
                    return Err("give a title, an artist or an album".to_owned());
                }
                self.changed(format!("the {}", set.join(" and ")));
                Ok(format!("Set the {}.", set.join(" and ")))
            }
            "get_selection" => Ok(describe_selection(self.editor.selection()?)),
            "select" => {
                let n = |k| number(args, k).map(|v| u32::try_from(v - 1).unwrap_or(u32::MAX));
                let selection = Selection2::Bars {
                    first_track: n("first_track")?,
                    last_track: n("last_track")?,
                    first_bar: n("first_bar")?,
                    last_bar: n("last_bar")?,
                };
                // OxiGuitar sees the changes made so far first.
                self.flush()?;
                self.editor.select(selection)?;
                Ok("Selected.".to_owned())
            }
            "list_tools" => {
                let tools = self.editor.tools()?;
                let mut out = String::new();
                for t in tools.iter().filter(|t| t.owner != crate::ID) {
                    out.push_str(&format!(
                        "owner {:?}, tool {:?}: {}. {}",
                        t.owner, t.id, t.name, t.description
                    ));
                    match &t.options {
                        Some(o) => out.push_str(&format!(" Options: {o}\n")),
                        None => out.push_str(" Runs only as from its menu.\n"),
                    }
                }
                Ok(out)
            }
            "run_tool" => {
                let owner = text(args, "owner")?;
                let tool = text(args, "tool")?;
                if owner == crate::ID {
                    return Err("the AI plugin's own tools aren't for the model".to_owned());
                }
                let options = args.get("options").filter(|o| !o.is_null());
                self.flush()?;
                let report = self.editor.run_tool(&owner, &tool, options)?;
                Ok(if report.trim().is_empty() {
                    "Done.".to_owned()
                } else {
                    report
                })
            }
            other => Err(format!("there is no tool {other:?}")),
        }
    }

    fn add_track(&mut self, args: &Map<String, Value>) -> Result<String, String> {
        let name = text(args, "name")?;
        let kind = args.get("kind").and_then(Value::as_str).unwrap_or("other");
        let drums = kind == "drums";
        let (program, tuning): (u8, &str) = match kind {
            "guitar" => (29, "E4 B3 G3 D3 A2 E2"),
            "bass" => (33, "G2 D2 A1 E1"),
            "drums" => (0, ""),
            "keys" => (0, ""),
            _ => (48, ""),
        };
        let program = match args.get("program").and_then(Value::as_u64) {
            Some(p) => u8::try_from(p)
                .ok()
                .filter(|p| *p <= 127)
                .ok_or("program is 0 to 127")?,
            None => program,
        };
        let tuning = args.get("tuning").and_then(Value::as_str).unwrap_or(tuning);
        let tuning: Vec<Pitch> = if drums {
            Vec::new()
        } else {
            tuning
                .split_whitespace()
                .map(|n| {
                    notation::parse_pitch(n)
                        .map(Pitch)
                        .ok_or_else(|| format!("{n:?} in the tuning isn't a note such as E2"))
                })
                .collect::<Result<_, _>>()?
        };
        if tuning.len() > 10 {
            return Err("a track has at most 10 strings".to_owned());
        }
        let score = self.score()?;
        let mix = score.tracks.first().map_or(
            Mix {
                volume: 0.8,
                pan: 0.0,
                mute: false,
                solo: false,
            },
            |t| Mix {
                mute: false,
                solo: false,
                pan: 0.0,
                ..t.mix
            },
        );
        let clef = if drums {
            oxiguitar_plugin::score::Clef::Neutral
        } else if kind == "bass" {
            oxiguitar_plugin::score::Clef::Bass8vb
        } else if tuning.is_empty() {
            oxiguitar_plugin::score::Clef::Treble
        } else {
            oxiguitar_plugin::score::Clef::Treble8vb
        };
        let stringed = !tuning.is_empty();
        let colours = [
            Color {
                r: 0xc0,
                g: 0x39,
                b: 0x2b,
            },
            Color {
                r: 0x27,
                g: 0x80,
                b: 0xb9,
            },
            Color {
                r: 0x27,
                g: 0xae,
                b: 0x60,
            },
            Color {
                r: 0x8e,
                g: 0x44,
                b: 0xad,
            },
            Color {
                r: 0xd3,
                g: 0x54,
                b: 0x00,
            },
        ];
        let colour = colours[score.tracks.len() % colours.len()];
        let bars = score.master_bars.len();
        score.tracks.push(Track2 {
            name: name.clone(),
            short_name: name.chars().take(4).collect(),
            color: colour,
            instrument: Instrument {
                midi_program: if drums { 0 } else { program },
                percussion: drums,
            },
            mix,
            staves: vec![Staff2 {
                tuning,
                capo: 0,
                fret_count: if stringed { 24 } else { 0 },
                transposition: 0,
                show_standard: true,
                show_tablature: stringed || drums,
                bars: (0..bars).map(|_| notation::empty_bar(clef)).collect(),
                ext: Vec::new(),
            }],
            ..Track2::default()
        });
        let n = score.tracks.len();
        self.changed(format!("track {n} added"));
        Ok(format!(
            "Added track {n} \"{name}\" with {bars} empty bars."
        ))
    }

    fn set_bars(&mut self, args: &Map<String, Value>) -> Result<String, String> {
        let from = number(args, "from_bar")?;
        let to = optional(args, "to_bar")?.unwrap_or(from);
        let time = match args.get("time").and_then(Value::as_str) {
            None => None,
            Some(t) => {
                let (n, d) = t.split_once('/').ok_or("time is like \"3/4\"")?;
                let numerator: u8 = n
                    .trim()
                    .parse()
                    .ok()
                    .filter(|n| *n > 0)
                    .ok_or("time is like \"3/4\"")?;
                let denominator: u8 = d
                    .trim()
                    .parse()
                    .ok()
                    .filter(|d: &u8| [1, 2, 4, 8, 16, 32].contains(d))
                    .ok_or("a time signature's bottom is 1, 2, 4, 8, 16 or 32")?;
                Some(TimeSignature {
                    numerator,
                    denominator,
                })
            }
        };
        let key = args
            .get("key")
            .and_then(Value::as_str)
            .map(notation::parse_key)
            .transpose()?;
        let tempo = match args.get("tempo") {
            None => None,
            Some(v) => Some(
                v.as_f64()
                    .filter(|t| (10.0..=500.0).contains(t))
                    .ok_or("tempo is 10 to 500 beats a minute")?,
            ),
        };
        let section = args
            .get("section")
            .and_then(Value::as_str)
            .map(str::to_owned);
        let score = self.score()?;
        let bars = score.master_bars.len();
        if from == 0 || to < from || to > bars {
            return Err(format!("the score has bars 1 to {bars}"));
        }
        let mut what = Vec::new();
        for m in &mut score.master_bars[from - 1..to] {
            if let Some(t) = time {
                m.time_signature = t;
            }
            if let Some(k) = key {
                m.key_signature = k;
            }
        }
        if time.is_some() {
            what.push("time");
        }
        if key.is_some() {
            what.push("key");
        }
        if let Some(bpm) = tempo {
            if from == 1 {
                score.initial_tempo = bpm;
                score.master_bars[0]
                    .tempo_changes
                    .retain(|t| t.position.num != 0);
            } else {
                let m = &mut score.master_bars[from - 1];
                m.tempo_changes.retain(|t| t.position.num != 0);
                m.tempo_changes.insert(0, notation::tempo_at_start(bpm));
            }
            what.push("tempo");
        }
        if let Some(name) = section {
            if name.trim().is_empty() {
                score.master_bars[from - 1].marker = None;
            } else {
                let letter = match &score.master_bars[from - 1].marker {
                    Some(mk) => mk.letter.clone(),
                    None => next_letter(score_markers(&score.master_bars)),
                };
                score.master_bars[from - 1].marker = Some(Marker {
                    letter,
                    text: name.trim().to_owned(),
                    color: Color {
                        r: 0xff,
                        g: 0,
                        b: 0,
                    },
                });
            }
            what.push("section");
        }
        if what.is_empty() {
            return Err("give a time, a key, a tempo or a section".to_owned());
        }
        self.changed(format!("bars {from}-{to} {}", what.join("/")));
        Ok(format!(
            "Set the {} of bars {from} to {to}.",
            what.join(", ")
        ))
    }
}

fn score_markers(bars: &[oxiguitar_plugin::score2::MasterBar2]) -> usize {
    bars.iter().filter(|m| m.marker.is_some()).count()
}

/// The section letter after `count` sections: A, B, … Z, AA.
fn next_letter(count: usize) -> String {
    let mut n = count;
    let mut s = String::new();
    loop {
        s.insert(0, char::from(b'A' + u8::try_from(n % 26).unwrap_or(0)));
        if n < 26 {
            break;
        }
        n = n / 26 - 1;
    }
    s
}

/// The selection, in words for the model (counting from 1).
pub fn describe_selection(selection: Option<Selection2>) -> String {
    let n = |v: u32| v.saturating_add(1);
    let pos = |p: Position| format!("bar {}, beat {}", n(p.bar), n(p.beat));
    match selection {
        None => "No score is open.".to_owned(),
        Some(Selection2::Caret {
            track,
            staff,
            voice,
            bar,
            beat,
            string,
        }) => format!(
            "Nothing is selected. The caret is on track {}, staff {}, voice {}, bar {}, beat {}{}.",
            n(track),
            n(staff),
            n(voice),
            n(bar),
            n(beat),
            if string > 0 {
                format!(", string {string}")
            } else {
                String::new()
            }
        ),
        Some(Selection2::Beats {
            track,
            staff,
            voice,
            from,
            to,
        }) => format!(
            "Beats of track {}, staff {}, voice {} are selected, from {} to {}.",
            n(track),
            n(staff),
            n(voice),
            pos(from),
            pos(to)
        ),
        Some(Selection2::Bars {
            first_track,
            last_track,
            first_bar,
            last_bar,
        }) => format!(
            "Whole bars {} to {} of tracks {} to {} are selected.",
            n(first_bar),
            n(last_bar),
            n(first_track),
            n(last_track)
        ),
    }
}

fn number(args: &Map<String, Value>, key: &str) -> Result<usize, String> {
    optional(args, key)?.ok_or_else(|| format!("{key} is needed"))
}

fn optional(args: &Map<String, Value>, key: &str) -> Result<Option<usize>, String> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => v
            .as_u64()
            .or_else(|| v.as_str().and_then(|s| s.trim().parse().ok()))
            .or_else(|| {
                v.as_f64()
                    .filter(|f| f.fract() == 0.0 && *f >= 0.0)
                    .map(|f| f as u64)
            })
            .and_then(|v| usize::try_from(v).ok())
            .filter(|v| *v > 0)
            .map(Some)
            .ok_or_else(|| format!("{key} is a whole number from 1")),
    }
}

fn text(args: &Map<String, Value>, key: &str) -> Result<String, String> {
    args.get(key)
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| format!("{key} is needed, as text"))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::sync::Mutex;

    /// A fake OxiGuitar: a score, what it was asked.
    #[derive(Default)]
    pub(crate) struct Fake {
        pub(crate) score: Mutex<Option<(u64, Score2)>>,
        pub(crate) asked: Mutex<Vec<String>>,
        /// The folders its skills are in.
        pub(crate) skill_dirs: Mutex<Vec<std::path::PathBuf>>,
    }

    impl Fake {
        pub(crate) fn with_song() -> Fake {
            let mut s = Score2 {
                initial_tempo: 120.0,
                ..Score2::default()
            };
            s.tracks.push(Track2 {
                name: "Lead".into(),
                instrument: Instrument {
                    midi_program: 29,
                    percussion: false,
                },
                staves: vec![Staff2 {
                    tuning: [64, 59, 55, 50, 45, 40].map(Pitch).to_vec(),
                    fret_count: 24,
                    ..Staff2::default()
                }],
                ..Track2::default()
            });
            notation::add_bar(&mut s);
            notation::add_bar(&mut s);
            Fake {
                score: Mutex::new(Some((7, s))),
                asked: Mutex::default(),
                skill_dirs: Mutex::default(),
            }
        }
        fn heard(&self, what: String) {
            self.asked.lock().unwrap().push(what);
        }
    }

    impl Editor for Fake {
        fn skill_folders(&self) -> Vec<std::path::PathBuf> {
            self.skill_dirs.lock().unwrap().clone()
        }
        fn score(&self) -> Result<Option<(u64, Score2)>, String> {
            Ok(self.score.lock().unwrap().clone())
        }
        fn apply(&self, revision: u64, name: &str, score: Score2) -> Result<(), String> {
            let mut s = self.score.lock().unwrap();
            if s.as_ref().map(|(r, _)| *r) != Some(revision) {
                return Err("late".into());
            }
            *s = Some((revision + 1, score));
            self.heard(format!("apply {name}"));
            Ok(())
        }
        fn selection(&self) -> Result<Option<Selection2>, String> {
            Ok(Some(Selection2::Caret {
                track: 0,
                staff: 0,
                voice: 0,
                bar: 1,
                beat: 0,
                string: 3,
            }))
        }
        fn select(&self, selection: Selection2) -> Result<(), String> {
            self.heard(format!("select {selection:?}"));
            Ok(())
        }
        fn tools(&self) -> Result<Vec<ToolEntry>, String> {
            Ok(vec![
                ToolEntry {
                    owner: "oxiguitar".into(),
                    id: "transpose".into(),
                    name: "Transpose…".into(),
                    description: "Moves notes.".into(),
                    edits: true,
                    options: Some(r#"{"type":"object"}"#.into()),
                },
                ToolEntry {
                    owner: crate::ID.into(),
                    id: "settings".into(),
                    name: "AI Settings…".into(),
                    description: "".into(),
                    edits: false,
                    options: None,
                },
            ])
        }
        fn run_tool(
            &self,
            owner: &str,
            tool: &str,
            options: Option<&Value>,
        ) -> Result<String, String> {
            self.heard(format!("run {owner}/{tool} {options:?}"));
            Ok("Transposed.".into())
        }
    }

    #[test]
    fn the_tools_are_well_formed() {
        let defs = definitions(&[]);
        let names: Vec<&str> = defs
            .as_array()
            .unwrap()
            .iter()
            .map(|d| d["function"]["name"].as_str().unwrap())
            .collect();
        assert!(names.contains(&"write_bars") && names.contains(&"run_tool"));
        for d in defs.as_array().unwrap() {
            assert_eq!(d["function"]["parameters"]["type"], "object");
        }
    }

    #[test]
    fn the_model_loads_a_skill_by_name() {
        let skills = vec![
            Skill {
                name: "blues".into(),
                description: "for blues".into(),
                always: false,
                text: "Swing the eighths.".into(),
            },
            Skill {
                name: "habits".into(),
                description: "always".into(),
                always: true,
                text: "Short answers.".into(),
            },
        ];
        let defs = definitions(&skills);
        let tool = defs
            .as_array()
            .unwrap()
            .iter()
            .find(|d| d["function"]["name"] == "use_skill")
            .unwrap();
        assert_eq!(
            tool["function"]["parameters"]["properties"]["name"]["enum"],
            json!(["blues"])
        );
        assert!(
            !definitions(&skills[1..]).to_string().contains("use_skill"),
            "nothing to load, no tool"
        );
        let fake = Fake::with_song();
        let mut w = Workspace::new(&fake).with_skills(skills);
        assert_eq!(
            w.call("use_skill", r#"{"name": "Blues"}"#),
            "Swing the eighths."
        );
        let r = w.call("use_skill", r#"{"name": "jazz"}"#);
        assert!(
            r.starts_with("Error: there is no skill called \"jazz\"") && r.contains("blues"),
            "{r}"
        );
    }

    #[test]
    fn writes_are_kept_as_one_change_at_the_end() {
        let fake = Fake::with_song();
        let mut w = Workspace::new(&fake);
        let r = w.call(
            "write_bars",
            r#"{"track": 1, "from_bar": 1, "bars": ["q 3/5 | q 3/7 | h (3/5 2/5)", "w r", "w 6/0"]}"#,
        );
        assert!(
            r.starts_with("Wrote bars 1 to 3 of track 1. Added 1 bars"),
            "{r}"
        );
        let read = w.call("read_bars", r#"{"track": 1, "from_bar": 1, "to_bar": 3}"#);
        assert!(read.contains("bar 3 (4/4): w 6/0"), "{read}");
        assert!(
            fake.asked.lock().unwrap().is_empty(),
            "nothing is handed back yet"
        );
        let r = w.call("add_track", r#"{"name": "Bass", "kind": "bass"}"#);
        assert_eq!(r, "Added track 2 \"Bass\" with 3 empty bars.");
        let r = w.call(
            "set_bars",
            r#"{"from_bar": 2, "time": "3/4", "section": "Verse", "tempo": 90}"#,
        );
        assert!(r.starts_with("Set the time, tempo, section"), "{r}");
        let name = w.flush().unwrap().unwrap();
        assert!(name.starts_with("AI: "), "{name}");
        let (revision, score) = fake.score.lock().unwrap().clone().unwrap();
        assert_eq!(revision, 8);
        assert_eq!(score.tracks.len(), 2);
        assert_eq!(score.tracks[1].staves[0].bars.len(), 3);
        assert_eq!(score.master_bars[1].time_signature.numerator, 3);
        assert_eq!(score.master_bars[1].marker.as_ref().unwrap().letter, "A");
        assert_eq!(score.master_bars[1].tempo_changes[0].bpm, 90.0);
        assert_eq!(w.flush().unwrap(), None, "nothing more to hand back");
    }

    /// set_info changes the title, artist and album it is given, and only
    /// those; with none of them it asks for one.
    #[test]
    fn set_info_changes_the_title_artist_and_album() {
        let fake = Fake::with_song();
        let mut w = Workspace::new(&fake);
        assert_eq!(
            w.call(
                "set_info",
                r#"{"title": "Night Ride", "artist": "The Frets"}"#
            ),
            "Set the title and artist."
        );
        assert!(
            w.call("set_info", "{}")
                .starts_with("Error: give a title, an artist or an album")
        );
        w.flush().unwrap().unwrap();
        let (_, score) = fake.score.lock().unwrap().clone().unwrap();
        assert_eq!(score.info.title, "Night Ride");
        assert_eq!(score.info.artist, "The Frets");
        assert_eq!(score.info.album, "");
    }

    #[test]
    fn a_tool_of_oxiguitar_runs_after_the_changes_are_handed_back() {
        let fake = Fake::with_song();
        let mut w = Workspace::new(&fake);
        w.call(
            "write_bars",
            r#"{"track": 1, "from_bar": 1, "bars": ["w 3/5"]}"#,
        );
        let list = w.call("list_tools", "{}");
        assert!(list.contains("tool \"transpose\""), "{list}");
        assert!(!list.contains("AI Settings"), "{list}");
        let r = w.call(
            "run_tool",
            r#"{"owner": "oxiguitar", "tool": "transpose", "options": {"amount": 2}}"#,
        );
        assert_eq!(r, "Transposed.");
        let asked = fake.asked.lock().unwrap().clone();
        assert!(
            asked[0].starts_with("apply AI: track 1 bars 1-1"),
            "{asked:?}"
        );
        assert!(
            asked[1].starts_with("run oxiguitar/transpose Some("),
            "{asked:?}"
        );
        let r = w.call(
            "select",
            r#"{"first_track": 1, "last_track": 1, "first_bar": 1, "last_bar": 2}"#,
        );
        assert_eq!(r, "Selected.");
        assert!(fake.asked.lock().unwrap()[2].contains("first_bar: 0"));
        let sel = w.call("get_selection", "");
        assert!(sel.contains("bar 2, beat 1, string 3"), "{sel}");
    }

    #[test]
    fn mistakes_are_sentences_for_the_model() {
        let fake = Fake::with_song();
        let mut w = Workspace::new(&fake);
        assert!(
            w.call("read_bars", r#"{"track": 9, "from_bar": 1}"#)
                .starts_with("Error: there is no track 9")
        );
        assert!(
            w.call("read_bars", "[1]")
                .starts_with("Error: the arguments")
        );
        assert!(
            w.call("nothing", "{}")
                .starts_with("Error: there is no tool")
        );
        assert!(
            w.call("set_bars", r#"{"from_bar": 1, "time": "3/5"}"#)
                .contains("bottom")
        );
        assert!(
            w.call(
                "run_tool",
                r#"{"owner": "oxiguitar.ai", "tool": "settings"}"#
            )
            .contains("aren't for the model")
        );
        let none = Fake::default();
        let mut w = Workspace::new(&none);
        assert!(w.call("overview", "{}").contains("no score is open"));
    }

    #[test]
    fn section_letters_count_on() {
        assert_eq!(next_letter(0), "A");
        assert_eq!(next_letter(25), "Z");
        assert_eq!(next_letter(26), "AA");
    }
}
