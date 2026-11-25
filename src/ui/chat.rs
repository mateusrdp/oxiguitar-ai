// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Mateus Rocha de Paula and the OxiGuitar contributors

//! The view `chat`, a panel in the window's right-hand area (`Chat` in
//! `ui/ai.slint`): New Chat at the top, the conversation, following the
//! newest, the box the user types in, and Send (Stop while the model
//! works). The model's thread sets the view again as it writes.

use std::sync::Arc;

use oxiguitar_plugin::{Error, Host, Value};

use crate::agent::{self, Entry, Shared};
use crate::settings::Settings;
use crate::tools::Editor;

/// The view's id in the manifest.
pub const VIEW: &str = crate::CHAT;

/// The chat.
pub struct Chat {
    host: Host,
    shared: Arc<Shared>,
    editor: Arc<dyn Editor + Send + Sync>,
}

impl std::fmt::Debug for Chat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Chat").finish_non_exhaustive()
    }
}

/// What the model said, without the thinking some models write out
/// (`<think>…</think>`).
pub fn visible(s: &str) -> String {
    let mut out = String::new();
    let mut rest = s;
    loop {
        match rest.find("<think>") {
            Some(i) => {
                out.push_str(&rest[..i]);
                match rest[i..].find("</think>") {
                    Some(j) => rest = &rest[i + j + "</think>".len()..],
                    None => break,
                }
            }
            None => {
                out.push_str(rest);
                break;
            }
        }
    }
    out.trim().to_owned()
}

/// What shows while the conversation is empty: what to ask, and where.
pub fn hint(s: &Settings) -> String {
    let model = if s.model.trim().is_empty() {
        "no model chosen yet".to_owned()
    } else {
        format!("model {}", s.model.trim())
    };
    format!(
        "Ask about the score, or ask for changes: \"write a bass line for bars 1 to 8\", \"fit every bar to its time signature\".\n\nServer: {} ({model}). Plugins → AI Settings… chooses another.",
        s.server()
    )
}

fn entry(kind: &str, text: String, can_copy: bool) -> Value {
    Value::Struct(vec![
        ("kind".into(), Value::String(kind.into())),
        ("text".into(), Value::String(text)),
        ("can-copy".into(), Value::Bool(can_copy)),
    ])
}

/// What the view shows of the conversation; `hint` is asked for only
/// while there is nothing else.
pub fn properties(shared: &Shared, hint: &dyn Fn() -> String) -> Vec<(String, Value)> {
    let c = shared.lock();
    let entries: Vec<Value> = c
        .entries
        .iter()
        .map(|e| match e {
            Entry::User(s) => entry("user", s.clone(), false),
            Entry::Assistant(s) => {
                let shown = visible(s);
                let copy = !shown.is_empty();
                entry("assistant", if copy { shown } else { "…".into() }, copy)
            }
            Entry::Tool(s) => entry("tool", s.clone(), false),
            Entry::Notice(s) => entry("notice", s.clone(), false),
            Entry::Error(s) => entry("error", s.clone(), false),
        })
        .collect();
    let waiting = c.waiting.as_ref().map(|(server, _)| {
        format!(
            "{server} isn't on this computer. Sending this question sends the conversation and what the model reads of the score there. Allow it?"
        )
    });
    let empty = entries.is_empty() && waiting.is_none();
    let (running, status) = (c.running, c.status.clone());
    drop(c);
    vec![
        ("entries".into(), Value::List(entries)),
        ("running".into(), Value::Bool(running)),
        ("status".into(), Value::String(status)),
        (
            "hint".into(),
            Value::String(if empty { hint() } else { String::new() }),
        ),
        ("waiting".into(), Value::String(waiting.unwrap_or_default())),
    ]
}

impl Chat {
    /// The chat, changing the score through `editor`.
    pub fn new(host: Host, shared: Arc<Shared>, editor: Arc<dyn Editor + Send + Sync>) -> Chat {
        Chat {
            host,
            shared,
            editor,
        }
    }

    /// Sets the view to the conversation as it is.
    pub fn show(&self) -> Result<(), Error> {
        self.redraw()();
        Ok(())
    }

    /// What the model's thread calls as it writes: the view set again.
    fn redraw(&self) -> Arc<dyn Fn() + Send + Sync> {
        let (host, shared) = (self.host.clone(), self.shared.clone());
        Arc::new(move || {
            let hint = || hint(&Settings::load(&host));
            let _ = host.set_view(VIEW, properties(&shared, &hint));
        })
    }

    /// The user sends a question.
    fn submit(&self, question: &str) -> Result<(), Error> {
        let question = question.trim().to_owned();
        if question.is_empty() || self.shared.lock().running {
            return Ok(());
        }
        self.host.set_view(
            VIEW,
            vec![("question".into(), Value::String(String::new()))],
        )?;
        let settings = Settings::load(&self.host);
        if !settings.may_send() {
            self.shared.lock().waiting = Some((settings.server(), question));
            return Ok(());
        }
        agent::ask(
            &self.shared,
            settings,
            self.editor.clone(),
            question,
            self.redraw(),
        );
        Ok(())
    }

    /// The user allows the server they were asked about, and the question
    /// waiting for it goes.
    fn allow(&self) {
        let Some((server, question)) = self.shared.lock().waiting.take() else {
            return;
        };
        let mut settings = Settings::load(&self.host);
        if !settings.allowed.contains(&server) {
            settings.allowed.push(server);
        }
        if let Err(e) = self.host.set_settings(&settings.to_map()) {
            self.shared.lock().entries.push(Entry::Error(format!(
                "the permission couldn't be saved: {}",
                e.message
            )));
            return;
        }
        agent::ask(
            &self.shared,
            settings,
            self.editor.clone(),
            question,
            self.redraw(),
        );
    }

    /// A callback of the view.
    pub fn callback(&self, callback: &str, args: &[Value]) -> Result<(), Error> {
        match (callback, args.first()) {
            ("send", Some(Value::String(question))) => self.submit(question)?,
            ("new-chat", _) => self.shared.clear(),
            ("stop", _) => self.shared.stop(),
            ("copy", Some(Value::Number(i))) if *i >= 0.0 => {
                let copied = match self.shared.lock().entries.get(*i as usize) {
                    Some(Entry::Assistant(s)) => Some(visible(s)),
                    _ => None,
                };
                if let Some(t) = copied {
                    self.host.copy(&t)?;
                }
                return Ok(());
            }
            ("allow", _) => self.allow(),
            ("deny", _) => self.shared.lock().waiting = None,
            _ => return Ok(()),
        }
        self.show()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn get<'a>(p: &'a [(String, Value)], name: &str) -> &'a Value {
        &p.iter().find(|(n, _)| n == name).unwrap().1
    }

    #[test]
    fn thinking_is_left_out_of_what_shows() {
        assert_eq!(visible("<think>hmm</think>\nThe answer."), "The answer.");
        assert_eq!(visible("A <think>x</think>B"), "A B");
        assert_eq!(visible("<think>still thinking"), "");
        assert_eq!(visible("plain"), "plain");
    }

    /// The conversation as the view shows it: each entry's kind and words,
    /// Copy only on what the model said, the hint only while it is empty,
    /// a server to allow as a question.
    #[test]
    fn the_view_shows_the_conversation() {
        let shared = Shared::default();
        let hint = || "Ask me.".to_owned();
        let p = properties(&shared, &hint);
        assert_eq!(get(&p, "hint"), &Value::String("Ask me.".into()));
        assert_eq!(get(&p, "entries"), &Value::List(Vec::new()));
        {
            let mut c = shared.lock();
            c.entries.push(Entry::User("What key?".into()));
            c.entries.push(Entry::Tool("read bars 1 to 4".into()));
            c.entries
                .push(Entry::Assistant("<think>hmm</think>G major.".into()));
            c.entries.push(Entry::Assistant("<think>still".into()));
            c.entries
                .push(Entry::Error("the server didn't answer".into()));
            c.running = true;
            c.status = "Reading the score…".into();
        }
        let p = properties(&shared, &|| panic!("no hint once it isn't empty"));
        let Value::List(entries) = get(&p, "entries") else {
            panic!()
        };
        let shown: Vec<_> = entries
            .iter()
            .map(|e| {
                let Value::Struct(f) = e else { panic!() };
                (f[0].1.clone(), f[1].1.clone(), f[2].1.clone())
            })
            .collect();
        let s = |t: &str| Value::String(t.into());
        assert_eq!(
            shown,
            [
                (s("user"), s("What key?"), Value::Bool(false)),
                (s("tool"), s("read bars 1 to 4"), Value::Bool(false)),
                (s("assistant"), s("G major."), Value::Bool(true)),
                (s("assistant"), s("…"), Value::Bool(false)),
                (
                    s("error"),
                    s("the server didn't answer"),
                    Value::Bool(false)
                ),
            ]
        );
        assert_eq!(get(&p, "running"), &Value::Bool(true));
        assert_eq!(get(&p, "hint"), &Value::String(String::new()));
        shared.clear();
        shared.lock().waiting = Some(("api.example.com".into(), "Hi".into()));
        let p = properties(&shared, &|| panic!("no hint while a server waits"));
        assert!(
            matches!(get(&p, "waiting"), Value::String(w) if w.starts_with("api.example.com isn't on this computer"))
        );
    }

    /// Sending: an empty question does nothing; one for a server away from
    /// this computer waits to be allowed, and Cancel forgets it; New Chat
    /// starts again.
    #[test]
    fn a_question_for_a_server_away_waits_and_new_chat_clears() {
        let shared = Arc::new(Shared::default());
        let chat = Chat::new(Host::detached(), shared.clone(), Arc::new(Host::detached()));
        chat.callback("send", &[Value::String("  ".into())])
            .unwrap();
        assert!(shared.lock().entries.is_empty() && !shared.lock().running);
        shared.lock().waiting = Some(("api.example.com".into(), "Hi".into()));
        chat.callback("deny", &[]).unwrap();
        assert!(shared.lock().waiting.is_none());
        shared.lock().entries.push(Entry::User("hi".into()));
        chat.callback("new-chat", &[]).unwrap();
        assert!(shared.lock().entries.is_empty());
        // Copy of something that isn't the model's copies nothing.
        chat.callback("copy", &[Value::Number(7.0)]).unwrap();
    }
}
