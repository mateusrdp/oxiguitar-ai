// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Mateus Rocha de Paula and the OxiGuitar contributors

//! The conversation, and the model's turn: on a thread of the plugin's own
//! (the window never waits for the model), the request goes out with the
//! conversation and the tools; the answer streams into the chat as it
//! comes; every tool the model calls runs and its result goes back, until
//! the model answers without calling one, or the steps run out, or the
//! user stops it. The score's changes are then handed back as one.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use oxiguitar_plugin::serde_json::{Value, json};

use crate::client::Client;
use crate::settings::Settings;
use crate::skills::{self, Skill};
use crate::tools::{self, Editor, Workspace};

/// One thing the chat shows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Entry {
    /// What the user asked.
    User(String),
    /// What the model said.
    Assistant(String),
    /// A tool the model used, in words.
    Tool(String),
    /// Something that went wrong.
    Error(String),
    /// A note from the plugin (the change kept, steps run out).
    Notice(String),
}

/// The conversation: what the chat shows and what the model is sent.
#[derive(Debug, Default)]
pub struct Conversation {
    /// What the chat shows, oldest first.
    pub entries: Vec<Entry>,
    /// What the model is sent after its instructions, oldest first.
    pub messages: Vec<Value>,
    /// The model is working.
    pub running: bool,
    /// What it is doing, while it works.
    pub status: String,
    /// Counts conversations: a turn of an earlier one (cleared since)
    /// changes nothing.
    pub generation: u64,
    /// A question the user hasn't sent yet because the server isn't on
    /// this computer and wasn't allowed: the server, and the question.
    pub waiting: Option<(String, String)>,
}

/// The conversation, shared by the tab and the model's thread.
#[derive(Debug, Default)]
pub struct Shared {
    conversation: Mutex<Conversation>,
    stop: Mutex<Arc<AtomicBool>>,
}

impl Shared {
    /// The conversation, locked.
    pub fn lock(&self) -> MutexGuard<'_, Conversation> {
        self.conversation
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }

    /// Stops the model's turn, if one is going.
    pub fn stop(&self) {
        self.stop
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .store(true, Ordering::Relaxed);
        let mut c = self.lock();
        if c.running {
            c.running = false;
            c.status.clear();
            c.entries.push(Entry::Notice("Stopped.".into()));
        }
    }

    /// Starts a new conversation.
    pub fn clear(&self) {
        self.stop();
        let mut c = self.lock();
        let generation = c.generation + 1;
        *c = Conversation {
            generation,
            ..Conversation::default()
        };
    }

    fn new_turn(&self) -> Arc<AtomicBool> {
        let flag = Arc::new(AtomicBool::new(false));
        *self.stop.lock().unwrap_or_else(PoisonError::into_inner) = flag.clone();
        flag
    }
}

/// The instructions the model gets: the plugin's, the user's skills, then
/// the user's own instructions.
pub fn system(settings: &Settings, skills: &[Skill]) -> Value {
    let mut text = tools::INSTRUCTIONS.to_owned();
    text.push_str(&skills::prompt(skills));
    if !settings.instructions.trim().is_empty() {
        text.push_str("\n\nThe user's own instructions:\n");
        text.push_str(settings.instructions.trim());
    }
    json!({"role": "system", "content": text})
}

/// The user asks `question`: their turn goes into the conversation and the
/// model's begins, on a thread of its own. `redraw` shows the chat again.
/// Nothing starts while a turn is going.
pub fn ask(
    shared: &Arc<Shared>,
    settings: Settings,
    editor: Arc<dyn Editor + Send + Sync>,
    question: String,
    redraw: Arc<dyn Fn() + Send + Sync>,
) {
    let generation = {
        let mut c = shared.lock();
        if c.running {
            return;
        }
        c.entries.push(Entry::User(question.clone()));
        c.messages
            .push(json!({"role": "user", "content": question}));
        c.running = true;
        c.status = "Thinking…".into();
        c.generation
    };
    let stop = shared.new_turn();
    let thread_shared = shared.clone();
    let spawned = std::thread::Builder::new()
        .name("oxiguitar-ai".into())
        .spawn(move || {
            turn(
                &thread_shared,
                &settings,
                &*editor,
                generation,
                &stop,
                &*redraw,
            );
            redraw();
        });
    if let Err(e) = spawned {
        let mut c = shared.lock();
        c.running = false;
        c.status.clear();
        c.entries.push(Entry::Error(format!(
            "the model's thread couldn't start: {e}"
        )));
    }
}

/// One turn of the model's, until it answers without calling a tool.
fn turn(
    shared: &Shared,
    settings: &Settings,
    editor: &dyn Editor,
    generation: u64,
    stop: &AtomicBool,
    redraw: &dyn Fn(),
) {
    let ours = |c: &Conversation| c.generation == generation && !stop.load(Ordering::Relaxed);
    let client = Client::new(settings.clone());
    let loaded = skills::load(&editor.skill_folders());
    if !loaded.problems.is_empty() {
        let mut c = shared.lock();
        if c.generation == generation {
            c.entries
                .extend(loaded.problems.iter().cloned().map(Entry::Error));
        }
    }
    let skills = loaded.skills;
    let mut work = Workspace::new(editor).with_skills(skills.clone());
    let steps = settings.max_steps.clamp(1, 100);
    let mut finished = false;
    for _ in 0..steps {
        let messages = {
            let c = shared.lock();
            if !ours(&c) {
                break;
            }
            let mut m = vec![system(settings, &skills)];
            m.extend(c.messages.iter().cloned());
            m
        };
        let body = json!({"messages": messages, "tools": tools::definitions(&skills), "tool_choice": "auto"});
        // The answer streams into one entry; the chat is drawn again at
        // most every 80 ms while it does.
        {
            let mut c = shared.lock();
            if !ours(&c) {
                break;
            }
            c.entries.push(Entry::Assistant(String::new()));
        }
        let mut last = Instant::now();
        let mut delta = |text: &str| {
            let mut c = shared.lock();
            if !ours(&c) {
                return;
            }
            if let Some(Entry::Assistant(s)) = c.entries.last_mut() {
                s.push_str(text);
            }
            c.status.clear();
            drop(c);
            if last.elapsed() >= Duration::from_millis(80) {
                last = Instant::now();
                redraw();
            }
        };
        let answer = client.complete(body, stop, &mut delta);
        let mut c = shared.lock();
        if c.generation != generation {
            return;
        }
        // An answer that said nothing leaves no empty entry.
        if matches!(c.entries.last(), Some(Entry::Assistant(s)) if s.trim().is_empty()) {
            c.entries.pop();
        }
        let completion = match answer {
            Ok(completion) => completion,
            Err(e) => {
                if !stop.load(Ordering::Relaxed) {
                    c.entries.push(Entry::Error(e));
                }
                break;
            }
        };
        if stop.load(Ordering::Relaxed) {
            break;
        }
        c.messages.push(completion.message());
        if completion.calls.is_empty() {
            finished = true;
            break;
        }
        drop(c);
        for call in &completion.calls {
            {
                let mut c = shared.lock();
                if !ours(&c) {
                    break;
                }
                c.status = format!("{}…", describe(&call.name, &call.arguments));
            }
            redraw();
            let result = work.call(&call.name, &call.arguments);
            let mut c = shared.lock();
            if c.generation != generation {
                return;
            }
            let line = describe(&call.name, &call.arguments);
            c.entries.push(Entry::Tool(if result.starts_with("Error:") {
                format!("{line}: {}", result.trim_start_matches("Error: "))
            } else {
                line
            }));
            c.messages.push(json!({
                "role": "tool",
                "tool_call_id": call.id,
                "name": call.name,
                "content": result,
            }));
        }
        redraw();
    }
    // Whatever was changed is kept, even when the model was stopped: Undo
    // takes it back.
    let kept = work.flush();
    let mut c = shared.lock();
    if c.generation != generation {
        return;
    }
    match kept {
        Ok(Some(name)) => c.entries.push(Entry::Notice(format!(
            "Kept as one change, \"{name}\": Edit, Undo takes it back."
        ))),
        Ok(None) => {}
        Err(e) => c.entries.push(Entry::Error(e)),
    }
    if !finished
        && c.running
        && !stop.load(Ordering::Relaxed)
        && !matches!(c.entries.last(), Some(Entry::Error(_)))
    {
        c.entries.push(Entry::Notice(format!(
            "The model used its {steps} steps for this answer (AI Settings sets how many)."
        )));
    }
    c.running = false;
    c.status.clear();
}

/// A tool call, in words for the chat.
pub fn describe(name: &str, arguments: &str) -> String {
    let a: Value = oxiguitar_plugin::serde_json::from_str(arguments).unwrap_or(Value::Null);
    let n = |k: &str| a.get(k).and_then(Value::as_u64).map(|v| v.to_string());
    let bars = |from: Option<String>, to: Option<String>| match (from, to) {
        (Some(f), Some(t)) if f != t => format!("bars {f}–{t}"),
        (Some(f), _) => format!("bar {f}"),
        _ => "bars".to_owned(),
    };
    match name {
        "overview" => "Read the score".to_owned(),
        "use_skill" => format!(
            "Read the skill \"{}\"",
            a.get("name").and_then(Value::as_str).unwrap_or_default()
        ),
        "read_bars" => format!(
            "Read {} of track {}",
            bars(n("from_bar"), n("to_bar")),
            n("track").unwrap_or_default()
        ),
        "write_bars" => {
            let count = a.get("bars").and_then(Value::as_array).map_or(0, Vec::len);
            let from = a.get("from_bar").and_then(Value::as_u64).unwrap_or(1);
            let to = (from + count as u64).saturating_sub(1).max(from);
            format!(
                "Wrote {} of track {}",
                bars(Some(from.to_string()), Some(to.to_string())),
                n("track").unwrap_or_default()
            )
        }
        "add_track" => format!(
            "Added the track \"{}\"",
            a.get("name").and_then(Value::as_str).unwrap_or_default()
        ),
        "set_bars" => format!("Changed {}", bars(n("from_bar"), n("to_bar"))),
        "set_info" => "Changed the score's information".to_owned(),
        "get_selection" => "Looked at the selection".to_owned(),
        "select" => format!("Selected {}", bars(n("first_bar"), n("last_bar"))),
        "list_tools" => "Listed OxiGuitar's tools".to_owned(),
        "run_tool" => format!(
            "Ran the tool \"{}\"",
            a.get("tool").and_then(Value::as_str).unwrap_or_default()
        ),
        other => format!("Used {other}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, Read, Write};
    use std::net::TcpListener;
    use std::sync::mpsc;

    /// A server on this computer speaking the protocol: answers each
    /// request with the next of `answers` (status, content type, body),
    /// and hands over what it was sent.
    fn server(answers: Vec<(u16, &'static str, String)>) -> (u16, mpsc::Receiver<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            for (status, kind, body) in answers {
                let Ok((stream, _)) = listener.accept() else {
                    return;
                };
                let mut reader = BufReader::new(stream);
                let mut length = 0;
                let mut head = String::new();
                loop {
                    let mut line = String::new();
                    if reader.read_line(&mut line).unwrap_or(0) == 0 || line == "\r\n" {
                        break;
                    }
                    if let Some(v) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                        length = v.trim().parse().unwrap_or(0);
                    }
                    head.push_str(&line);
                }
                let mut sent = vec![0; length];
                reader.read_exact(&mut sent).unwrap();
                let _ = tx.send(format!("{head}\n{}", String::from_utf8_lossy(&sent)));
                let mut stream = reader.into_inner();
                let _ = write!(
                    stream,
                    "HTTP/1.1 {status} X\r\nContent-Type: {kind}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
            }
        });
        (port, rx)
    }

    fn settings(port: u16, stream: bool) -> Settings {
        Settings {
            host: "127.0.0.1".into(),
            port,
            model: "test-model".into(),
            api_key: "sk-test".into(),
            stream,
            ..Settings::default()
        }
    }

    /// A redraw that tells the test, and a wait for the turn's end on it.
    fn redraws() -> (Arc<dyn Fn() + Send + Sync>, mpsc::Receiver<()>) {
        let (tx, rx) = mpsc::channel();
        let tx = Mutex::new(tx);
        (
            Arc::new(move || {
                let _ = tx.lock().unwrap().send(());
            }),
            rx,
        )
    }

    fn wait(shared: &Shared, redrawn: &mpsc::Receiver<()>) {
        while shared.lock().running {
            redrawn
                .recv_timeout(Duration::from_secs(20))
                .expect("the turn never ended");
        }
    }

    #[test]
    fn a_turn_streams_calls_a_tool_and_keeps_the_change() {
        let call = r#"data: {"choices":[{"delta":{"tool_calls":[{"index":0,"id":"c1","type":"function","function":{"name":"write_bars","arguments":"{\"track\": 1, \"from_bar\": 1, \"bars\": [\"w 3/5\"]}"}}]}}]}"#;
        let first = format!("{call}\n\ndata: [DONE]\n\n");
        let second = "data: {\"choices\":[{\"delta\":{\"content\":\"Wrote \"}}]}\n\ndata: {\"choices\":[{\"delta\":{\"content\":\"bar 1.\"}}]}\n\ndata: [DONE]\n\n".to_owned();
        let (port, sent) = server(vec![
            (200, "text/event-stream", first),
            (200, "text/event-stream", second),
        ]);
        let shared = Arc::new(Shared::default());
        let fake = Arc::new(crate::tools::tests::Fake::with_song());
        let (redraw, redrawn) = redraws();
        ask(
            &shared,
            settings(port, true),
            fake.clone(),
            "Write a note".into(),
            redraw,
        );
        wait(&shared, &redrawn);
        let entries = shared.lock().entries.clone();
        assert_eq!(entries[0], Entry::User("Write a note".into()));
        assert_eq!(entries[1], Entry::Tool("Wrote bar 1 of track 1".into()));
        assert_eq!(entries[2], Entry::Assistant("Wrote bar 1.".into()));
        assert!(
            matches!(&entries[3], Entry::Notice(n) if n.contains("Kept as one change")),
            "{entries:?}"
        );
        // The change reached the score, once.
        let asked = fake.asked.lock().unwrap().clone();
        assert_eq!(asked.len(), 1, "{asked:?}");
        assert!(asked[0].starts_with("apply AI: track 1 bars 1-1"));
        // What went out: the key, the model, the tools; then the tool's
        // result went back with its call.
        let one = sent.recv().unwrap();
        assert!(
            one.to_ascii_lowercase()
                .contains("authorization: bearer sk-test"),
            "{one}"
        );
        assert!(
            one.contains(r#""model":"test-model""#) && one.contains(r#""stream":true"#),
            "{one}"
        );
        assert!(one.contains(r#""name":"write_bars""#), "{one}");
        let two = sent.recv().unwrap();
        assert!(
            two.contains(r#""role":"tool""#) && two.contains(r#""tool_call_id":"c1""#),
            "{two}"
        );
        assert!(two.contains("Wrote bars 1 to 1 of track 1."), "{two}");
    }

    #[test]
    fn a_skill_is_listed_then_loaded_when_the_model_asks() {
        let dir =
            std::env::temp_dir().join(format!("oxiguitar-ai-agent-skills-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("blues.md"),
            "---\ndescription: how I write blues\n---\nSwing the eighths.",
        )
        .unwrap();
        std::fs::write(dir.join("empty.md"), "").unwrap();
        let call = r#"{"choices":[{"message":{"role":"assistant","content":null,"tool_calls":[{"id":"s1","type":"function","function":{"name":"use_skill","arguments":"{\"name\": \"blues\"}"}}]}}]}"#;
        let done = r#"{"choices":[{"message":{"role":"assistant","content":"Done."}}]}"#;
        let (port, sent) = server(vec![
            (200, "application/json", call.to_owned()),
            (200, "application/json", done.to_owned()),
        ]);
        let shared = Arc::new(Shared::default());
        let fake = Arc::new(crate::tools::tests::Fake::with_song());
        *fake.skill_dirs.lock().unwrap() = vec![dir.clone()];
        let (redraw, redrawn) = redraws();
        ask(
            &shared,
            settings(port, false),
            fake,
            "A blues".into(),
            redraw,
        );
        wait(&shared, &redrawn);
        let entries = shared.lock().entries.clone();
        assert!(matches!(&entries[0], Entry::User(_)), "{entries:?}");
        assert!(
            matches!(&entries[1], Entry::Error(e) if e.contains("empty.md") && e.contains("no text")),
            "{entries:?}"
        );
        assert_eq!(entries[2], Entry::Tool("Read the skill \"blues\"".into()));
        let one = sent.recv().unwrap();
        assert!(one.contains("blues: how I write blues"), "{one}");
        assert!(one.contains(r#""name":"use_skill""#), "{one}");
        assert!(
            !one.contains("Swing the eighths."),
            "the text waits to be asked for"
        );
        let two = sent.recv().unwrap();
        assert!(two.contains("Swing the eighths."), "{two}");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn the_skills_come_after_the_plugins_instructions_and_before_the_users() {
        let s = Settings {
            instructions: "Answer in Portuguese.".into(),
            ..Settings::default()
        };
        let skill = skills::parse("---\nalways: true\n---\nNo open strings.", "habits").unwrap();
        let text = system(&s, &[skill])["content"].as_str().unwrap().to_owned();
        let at = |needle: &str| {
            text.find(needle)
                .unwrap_or_else(|| panic!("{needle} in {text}"))
        };
        assert!(at("No open strings.") < at("Answer in Portuguese."));
        assert!(at("You are the assistant") < at("No open strings."));
    }

    #[test]
    fn a_whole_answer_and_a_servers_error_show_in_the_chat() {
        let whole =
            r#"{"choices":[{"message":{"role":"assistant","content":"Hello."}}]}"#.to_owned();
        let (port, _) = server(vec![
            (200, "application/json", whole),
            (
                401,
                "application/json",
                r#"{"error":{"message":"bad key"}}"#.to_owned(),
            ),
        ]);
        let shared = Arc::new(Shared::default());
        let fake = Arc::new(crate::tools::tests::Fake::with_song());
        let (redraw, redrawn) = redraws();
        ask(
            &shared,
            settings(port, false),
            fake.clone(),
            "Hi".into(),
            redraw.clone(),
        );
        wait(&shared, &redrawn);
        assert_eq!(shared.lock().entries[1], Entry::Assistant("Hello.".into()));
        ask(&shared, settings(port, false), fake, "Again".into(), redraw);
        wait(&shared, &redrawn);
        let last = shared.lock().entries.last().cloned().unwrap();
        assert_eq!(
            last,
            Entry::Error("the server answered 401: bad key (check the key in AI Settings)".into())
        );
    }

    #[test]
    fn a_server_away_from_this_computer_needs_permission() {
        let shared = Arc::new(Shared::default());
        let fake = Arc::new(crate::tools::tests::Fake::with_song());
        let mut s = Settings::default();
        s.apply_preset("openai");
        s.model = "m".into();
        let (redraw, redrawn) = redraws();
        ask(&shared, s, fake, "Hi".into(), redraw);
        wait(&shared, &redrawn);
        let last = shared.lock().entries.last().cloned().unwrap();
        assert!(
            matches!(&last, Entry::Error(e) if e.contains("wasn't allowed")),
            "{last:?}"
        );
    }

    #[test]
    fn tool_calls_are_described_in_words() {
        assert_eq!(
            describe("read_bars", r#"{"track": 2, "from_bar": 1, "to_bar": 8}"#),
            "Read bars 1–8 of track 2"
        );
        assert_eq!(
            describe(
                "write_bars",
                r#"{"track": 1, "from_bar": 3, "bars": ["a"]}"#
            ),
            "Wrote bar 3 of track 1"
        );
        assert_eq!(
            describe("run_tool", r#"{"owner": "oxiguitar", "tool": "fit-bars"}"#),
            "Ran the tool \"fit-bars\""
        );
        assert_eq!(
            describe("use_skill", r#"{"name": "blues"}"#),
            "Read the skill \"blues\""
        );
        assert_eq!(describe("nope", "x"), "Used nope");
    }

    #[test]
    fn clearing_starts_a_new_conversation() {
        let s = Shared::default();
        s.lock().entries.push(Entry::User("hi".into()));
        s.clear();
        let c = s.lock();
        assert!(c.entries.is_empty());
        assert_eq!(c.generation, 1);
    }

    #[test]
    fn the_users_instructions_follow_the_plugins() {
        let s = Settings {
            instructions: "Answer in Portuguese.".into(),
            ..Settings::default()
        };
        let text = system(&s, &[])["content"].as_str().unwrap().to_owned();
        assert!(text.starts_with("You are the assistant inside OxiGuitar"));
        assert!(text.ends_with("Answer in Portuguese."));
    }
}
