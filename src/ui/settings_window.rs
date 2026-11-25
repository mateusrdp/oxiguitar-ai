// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Mateus Rocha de Paula and the OxiGuitar contributors

//! The AI Settings window (`SettingsWindow` in `ui/ai.slint`), in six
//! tabs: the server (a preset, then each part of its address on its own),
//! the key, the key's header and more headers, the model (typed, or picked
//! from the server's list), the answers' limits, and the user's own
//! instructions with the servers away from this computer they allowed. A
//! draft while it is open: OK keeps it, Cancel forgets it.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use oxiguitar_plugin::{Error, ErrorKind, Host, Level, Value};

use crate::client::Client;
use crate::settings::{PRESETS, Settings};

/// The view's id in the manifest.
pub const VIEW: &str = crate::SETTINGS;

/// The settings' texts, by their ids in `edited`.
const TEXTS: [&str; 15] = [
    "host",
    "port",
    "path",
    "query",
    "key",
    "key-env",
    "auth-header",
    "auth-prefix",
    "headers",
    "model",
    "temperature",
    "max-tokens",
    "timeout",
    "steps",
    "instructions",
];

/// A text's property in the window: its id, but for the path (a
/// window's `path` would be easy to mistake).
fn property(id: &str) -> String {
    match id {
        "path" => "url-path".into(),
        other => other.into(),
    }
}

/// A text's value in the settings.
fn value(s: &Settings, id: &str) -> String {
    match id {
        "host" => s.host.clone(),
        "port" => s.port.to_string(),
        "path" => s.path.clone(),
        "query" => s.query.clone(),
        "key" => s.api_key.clone(),
        "key-env" => s.api_key_env.clone(),
        "auth-header" => s.auth_header.clone(),
        "auth-prefix" => s.auth_prefix.clone(),
        "headers" => s.extra_headers.clone(),
        "model" => s.model.clone(),
        "temperature" => s.temperature.clone(),
        "max-tokens" => s.max_tokens.clone(),
        "timeout" => s.timeout.to_string(),
        "steps" => s.max_steps.to_string(),
        "instructions" => s.instructions.clone(),
        _ => String::new(),
    }
}

/// Sets a text's value in the settings; a number that isn't one is an
/// error naming it, and the setting keeps its value.
fn set(s: &mut Settings, id: &str, v: &str) -> Result<(), String> {
    let number = |what: &str, lo: u32, hi: u32| -> Result<u32, String> {
        v.trim()
            .parse::<u32>()
            .ok()
            .filter(|n| (lo..=hi).contains(n))
            .ok_or_else(|| format!("{what} is a whole number from {lo} to {hi}"))
    };
    match id {
        "host" => s.host = v.trim().to_owned(),
        "port" => s.port = u16::try_from(number("The port", 1, 65535)?).unwrap_or(s.port),
        "path" => s.path = v.trim().to_owned(),
        "query" => s.query = v.trim().to_owned(),
        "key" => s.api_key = v.trim().to_owned(),
        "key-env" => s.api_key_env = v.trim().to_owned(),
        "auth-header" => s.auth_header = v.trim().to_owned(),
        // Spaces matter here ("Bearer "): kept as typed.
        "auth-prefix" => s.auth_prefix = v.to_owned(),
        "headers" => s.extra_headers = v.to_owned(),
        "model" => s.model = v.trim().to_owned(),
        "temperature" => s.temperature = v.trim().to_owned(),
        "max-tokens" => s.max_tokens = v.trim().to_owned(),
        "timeout" => s.timeout = number("The timeout", 10, 3600)?,
        "steps" => s.max_steps = number("The tool calls", 1, 100)?,
        "instructions" => s.instructions = v.to_owned(),
        _ => {}
    }
    Ok(())
}

/// The window's draft.
#[derive(Debug, Default)]
struct Draft {
    /// The settings as the window opened, with the choices made since;
    /// `None` while it is closed.
    settings: Option<Settings>,
    texts: HashMap<&'static str, String>,
    show_key: bool,
    models: Vec<String>,
    /// What the last model list request said.
    status: String,
    loading: bool,
    picked_server: Option<usize>,
}

impl Draft {
    fn open(s: Settings) -> Draft {
        let mut d = Draft::default();
        d.fill(&s);
        d.settings = Some(s);
        d
    }

    /// The settings with every text's value, and the first problem, if a
    /// number isn't one.
    fn current(&self) -> (Settings, Option<String>) {
        let mut s = self.settings.clone().unwrap_or_default();
        let mut problem = None;
        for id in TEXTS {
            if let Some(t) = self.texts.get(id)
                && let Err(e) = set(&mut s, id, t)
            {
                problem.get_or_insert(e);
            }
        }
        (s, problem)
    }

    /// The texts show `s` again (a preset was chosen, say).
    fn fill(&mut self, s: &Settings) {
        for id in TEXTS {
            self.texts.insert(id, value(s, id));
        }
    }

    /// What the window shows; the texts only when `texts`, so what the
    /// user types isn't set back under their caret.
    fn properties(&self, texts: bool) -> Vec<(String, Value)> {
        let (s, problem) = self.current();
        let string = |t: String| Value::String(t);
        let items = |v: &[String]| {
            Value::List(
                v.iter()
                    .map(|t| Value::Struct(vec![("text".into(), string(t.clone()))]))
                    .collect(),
            )
        };
        let index = |i: Option<usize>| Value::Number(i.map_or(-1.0, |i| i as f64));
        let preset = crate::settings::preset(&s.preset);
        let mut p = vec![
            (
                "presets".into(),
                Value::List(PRESETS.iter().map(|p| string(p.name.into())).collect()),
            ),
            (
                "preset".into(),
                index(Some(
                    PRESETS.iter().position(|p| p.id == s.preset).unwrap_or(0),
                )),
            ),
            (
                "scheme".into(),
                index(Some(usize::from(s.scheme.eq_ignore_ascii_case("https")))),
            ),
            (
                "address".into(),
                string(match s.url("chat/completions") {
                    Ok(url) => format!("Requests go to {url}"),
                    Err(e) => format!("The address isn't complete: {e}."),
                }),
            ),
            (
                "away".into(),
                string(if s.is_local() {
                    String::new()
                } else if s.may_send() {
                    "Not on this computer, and allowed: the conversation and what the model reads of the score are sent there.".into()
                } else {
                    "Not on this computer: the chat asks before anything is sent there.".into()
                }),
            ),
            ("show-key".into(), Value::Bool(self.show_key)),
            (
                "keys-at".into(),
                string(if preset.keys_at.is_empty() {
                    String::new()
                } else {
                    format!("Get a key: {}", preset.name)
                }),
            ),
            ("loading".into(), Value::Bool(self.loading)),
            ("status".into(), string(self.status.clone())),
            ("models".into(), items(&self.models)),
            (
                "model-picked".into(),
                index(self.models.iter().position(|m| *m == s.model)),
            ),
            ("stream".into(), Value::Bool(s.stream)),
            ("allowed".into(), items(&s.allowed)),
            ("server-picked".into(), index(self.picked_server)),
            (
                "problem".into(),
                string(problem.map(|p| format!("{p}.")).unwrap_or_default()),
            ),
        ];
        if texts {
            for id in TEXTS {
                p.push((
                    property(id),
                    string(self.texts.get(id).cloned().unwrap_or_default()),
                ));
            }
        }
        p
    }
}

/// Every property the window is set.
pub fn property_names() -> Vec<String> {
    Draft::open(Settings::default())
        .properties(true)
        .into_iter()
        .map(|p| p.0)
        .collect()
}

/// The window.
#[derive(Debug)]
pub struct SettingsWindow {
    host: Host,
    draft: Arc<Mutex<Draft>>,
}

fn lock(d: &Mutex<Draft>) -> MutexGuard<'_, Draft> {
    d.lock().unwrap_or_else(PoisonError::into_inner)
}

fn number(args: &[Value], i: usize) -> Option<usize> {
    match args.get(i) {
        Some(Value::Number(n)) if *n >= 0.0 && n.fract() == 0.0 => Some(*n as usize),
        _ => None,
    }
}

impl SettingsWindow {
    /// The window, closed.
    pub fn new(host: Host) -> SettingsWindow {
        SettingsWindow {
            host,
            draft: Arc::default(),
        }
    }

    /// Opens the window on the saved settings, or brings it forward if it
    /// is open.
    pub fn open(&self) -> Result<(), Error> {
        let properties = {
            let mut d = lock(&self.draft);
            if d.settings.is_none() {
                *d = Draft::open(Settings::load(&self.host));
            }
            d.properties(true)
        };
        self.host.set_view(VIEW, properties)?;
        self.host.show_view(VIEW)
    }

    /// The window was made: it shows the draft, if it is open.
    pub fn shown(&self) -> Result<(), Error> {
        let d = lock(&self.draft);
        if d.settings.is_none() {
            return Ok(());
        }
        let properties = d.properties(true);
        drop(d);
        self.host.set_view(VIEW, properties)
    }

    /// Asks the server for its models, on a thread of its own; the window
    /// shows them when they come.
    fn load_models(&self, d: &mut Draft) {
        if d.loading {
            return;
        }
        let (s, _) = d.current();
        if !s.may_send() {
            d.status = format!(
                "{} isn't on this computer: ask a question in the chat first and allow it there.",
                s.server()
            );
            return;
        }
        d.loading = true;
        d.status = "Asking the server…".into();
        let draft = self.draft.clone();
        let host = self.host.clone();
        let spawned = std::thread::Builder::new()
            .name("oxiguitar-ai-models".into())
            .spawn(move || {
                let answer = Client::new(s).models();
                let properties = {
                    let mut d = lock(&draft);
                    d.loading = false;
                    match answer {
                        Ok(models) => {
                            d.status = format!("The server has {} models.", models.len());
                            d.models = models;
                        }
                        Err(e) => d.status = e,
                    }
                    d.properties(false)
                };
                let _ = host.set_view(VIEW, properties);
            });
        if spawned.is_err() {
            d.loading = false;
            d.status = "The model list couldn't be asked for.".into();
        }
    }

    /// A callback of the window but OK and Cancel. Returns whether the
    /// texts are to be set again (a preset filled them, say).
    fn change(&self, d: &mut Draft, callback: &str, args: &[Value]) -> Result<bool, Error> {
        let Some(s) = d.settings.as_mut() else {
            return Ok(false);
        };
        match callback {
            "edited" => {
                if let (Some(Value::String(id)), Some(Value::String(t))) =
                    (args.first(), args.get(1))
                    && let Some(id) = TEXTS.iter().find(|i| *i == id)
                {
                    d.texts.insert(id, t.clone());
                }
            }
            "preset-chosen" => {
                if let Some(p) = number(args, 0).and_then(|i| PRESETS.get(i)) {
                    let (mut s, _) = d.current();
                    s.apply_preset(p.id);
                    d.fill(&s);
                    d.settings = Some(s);
                    return Ok(true);
                }
            }
            "scheme-chosen" => {
                if let Some(i) = number(args, 0) {
                    s.scheme = if i == 1 { "https" } else { "http" }.into();
                }
            }
            "stream-toggled" => {
                if let Some(Value::Bool(on)) = args.first() {
                    s.stream = *on;
                }
            }
            "show-key-toggled" => {
                if let Some(Value::Bool(on)) = args.first() {
                    d.show_key = *on;
                }
            }
            "get-key" => {
                let p = crate::settings::preset(&d.current().0.preset);
                if !p.keys_at.is_empty() {
                    crate::links::open(p.keys_at)
                        .map_err(|why| Error::new(ErrorKind::Failed, why))?;
                }
            }
            "list-models" => self.load_models(d),
            "model-chosen" => {
                if let Some(m) = number(args, 0).and_then(|i| d.models.get(i)).cloned() {
                    d.texts.insert("model", m);
                    return Ok(true);
                }
            }
            "server-chosen" => d.picked_server = number(args, 0),
            "forget-server" => {
                if let Some(i) = d.picked_server.take()
                    && i < s.allowed.len()
                {
                    s.allowed.remove(i);
                }
            }
            _ => {}
        }
        Ok(false)
    }

    /// A callback of the window.
    pub fn callback(&self, callback: &str, args: &[Value]) -> Result<(), Error> {
        if matches!(callback, "ok" | "cancel") {
            self.finish(callback == "ok")?;
            return self.host.close_view(VIEW);
        }
        let properties = {
            let mut d = lock(&self.draft);
            let texts = self.change(&mut d, callback, args)?;
            d.properties(texts)
        };
        self.host.set_view(VIEW, properties)
    }

    /// OK (`ok`) keeps the draft; Cancel, or the window closed, forgets
    /// it.
    pub fn finish(&self, ok: bool) -> Result<(), Error> {
        let draft = std::mem::take(&mut *lock(&self.draft));
        if !ok || draft.settings.is_none() {
            return Ok(());
        }
        let (s, problem) = draft.current();
        if let Some(p) = problem {
            self.host.notify(
                Level::Warning,
                format!("AI Settings: {p}; the rest is kept."),
            );
        }
        if let Err(e) = s.headers() {
            self.host
                .notify(Level::Warning, format!("AI Settings: {e}."));
        }
        self.host.set_settings(&s.to_map())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn get(p: &[(String, Value)], name: &str) -> Value {
        p.iter().find(|(n, _)| n == name).unwrap().1.clone()
    }

    fn text(t: &str) -> Value {
        Value::String(t.into())
    }

    fn window() -> SettingsWindow {
        let w = SettingsWindow::new(Host::detached());
        *lock(&w.draft) = Draft::open(Settings::default());
        w
    }

    fn shown(w: &SettingsWindow) -> Vec<(String, Value)> {
        lock(&w.draft).properties(true)
    }

    #[test]
    fn a_preset_fills_the_texts_and_cancel_forgets() {
        let w = window();
        let p = shown(&w);
        assert_eq!(get(&p, "host"), text("127.0.0.1"));
        assert_eq!(get(&p, "url-path"), text("/v1"));
        assert_eq!(get(&p, "keys-at"), text(""));
        w.callback("preset-chosen", &[Value::Number(3.0)]).unwrap();
        let p = shown(&w);
        assert_eq!(get(&p, "host"), text("api.openai.com"));
        assert_eq!(get(&p, "scheme"), Value::Number(1.0));
        assert!(matches!(get(&p, "keys-at"), Value::String(k) if k.starts_with("Get a key: ")));
        assert!(
            matches!(get(&p, "away"), Value::String(a) if a.starts_with("Not on this computer"))
        );
        // What is typed counts, and isn't set back while typed.
        w.callback("edited", &[text("model"), text("gpt-x")])
            .unwrap();
        let p = lock(&w.draft).properties(false);
        assert!(!p.iter().any(|(n, _)| n == "model"));
        assert_eq!(lock(&w.draft).current().0.model, "gpt-x");
        w.callback("show-key-toggled", &[Value::Bool(true)])
            .unwrap();
        assert_eq!(get(&shown(&w), "show-key"), Value::Bool(true));
        w.finish(false).unwrap();
        assert!(lock(&w.draft).settings.is_none(), "Cancel forgot it");
        // A callback while it is closed changes nothing.
        w.callback("edited", &[text("host"), text("x")]).unwrap();
        assert!(lock(&w.draft).texts.is_empty());
    }

    #[test]
    fn a_number_that_isnt_one_is_said_and_the_rest_kept() {
        let w = window();
        w.callback("edited", &[text("port"), text("x")]).unwrap();
        let p = shown(&w);
        assert!(
            matches!(get(&p, "problem"), Value::String(t) if t.contains("The port is a whole number"))
        );
        let (s, problem) = lock(&w.draft).current();
        assert!(problem.is_some());
        assert_eq!(s.port, 11434);
    }

    #[test]
    fn models_and_allowed_servers_are_picked_from_lists() {
        let w = window();
        {
            let mut d = lock(&w.draft);
            d.models = vec!["a".into(), "b".into()];
            d.settings.as_mut().unwrap().allowed = vec!["one".into(), "two".into()];
        }
        w.callback("model-chosen", &[Value::Number(1.0)]).unwrap();
        let p = shown(&w);
        assert_eq!(get(&p, "model"), text("b"));
        assert_eq!(get(&p, "model-picked"), Value::Number(1.0));
        assert_eq!(get(&p, "server-picked"), Value::Number(-1.0));
        w.callback("server-chosen", &[Value::Number(0.0)]).unwrap();
        w.callback("forget-server", &[]).unwrap();
        let p = shown(&w);
        assert_eq!(
            get(&p, "allowed"),
            Value::List(vec![Value::Struct(vec![("text".into(), text("two"))])])
        );
        assert_eq!(get(&p, "server-picked"), Value::Number(-1.0));
        w.callback("stream-toggled", &[Value::Bool(false)]).unwrap();
        assert!(!lock(&w.draft).current().0.stream);
    }
}
