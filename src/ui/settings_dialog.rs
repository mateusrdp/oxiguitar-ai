// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Mateus Rocha de Paula and the OxiGuitar contributors

//! The dialog `settings`, in six pages chosen at its top (a dialog doesn't
//! scroll): the server (a preset, then each part of its address on its
//! own), the key, the key's header and more headers, the model (typed, or
//! picked from the server's list), the answers' limits, and the user's own
//! instructions with the servers away from this computer they allowed. A
//! draft while it is open: OK keeps it, Cancel forgets it.
//!
//! OxiGuitar's own widgets for choices, lists, tickboxes and buttons; every
//! text is a text box of the plugin's own on a surface, so what is typed
//! counts as it is typed, the key is hidden, and the headers and the
//! instructions have several lines.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use oxiguitar_plugin::display::Item;
use oxiguitar_plugin::{
    Cursor, Error, Host, Level, PointerKind, Row, Surface, Tab, TabContent, TabEvent, Widget,
};

use crate::client::Client;
use crate::settings::{PRESETS, Settings};
use crate::ui::text::{Measure, Outcome, Rect, TextBox};

/// A text of the settings: its id (its surface's), what it is called,
/// what shows while it is empty, how many lines it shows, and whether it
/// is hidden.
struct Text {
    id: &'static str,
    label: &'static str,
    placeholder: &'static str,
    lines: usize,
    hidden: bool,
}

const fn t(id: &'static str, label: &'static str, placeholder: &'static str) -> Text {
    Text {
        id,
        label,
        placeholder,
        lines: 1,
        hidden: false,
    }
}

const TEXTS: [Text; 15] = [
    t("host", "Host", "Host name or address"),
    t("port", "Port", "Such as 443"),
    t("path", "Path", "Such as /v1"),
    t("query", "Query", "Usually empty; api-version=… for Azure"),
    Text {
        id: "key",
        label: "Key (personal access token)",
        placeholder: "Paste it from the provider's site",
        lines: 1,
        hidden: true,
    },
    t(
        "key-env",
        "Or the environment variable that holds it",
        "Such as OPENAI_API_KEY",
    ),
    t("auth-header", "The key's header", "Authorization"),
    t("auth-prefix", "Before the key", "Bearer (and a space)"),
    Text {
        id: "headers",
        label: "More headers, one a line",
        placeholder: "Name: value",
        lines: 4,
        hidden: false,
    },
    t("model", "Model", "Its name, as the server knows it"),
    t("temperature", "Temperature, 0 to 2", "Empty: the server's"),
    t("max-tokens", "Most tokens an answer", "Empty: the server's"),
    t("timeout", "Seconds a request may take", "300"),
    t("steps", "Tool calls an answer may make", "16"),
    Text {
        id: "instructions",
        label: "Your instructions for the model",
        placeholder: "Such as: answer in Portuguese; keep to the first five frets",
        lines: 5,
        hidden: false,
    },
];

const PAGES: [&str; 6] = [
    "Server",
    "Key",
    "Headers",
    "Model",
    "Answers",
    "Instructions",
];

fn text_spec(id: &str) -> Option<&'static Text> {
    TEXTS.iter().find(|t| t.id == id)
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

#[derive(Debug, Default)]
struct Draft {
    /// The settings as the dialog opened, with the choices made since.
    settings: Option<Settings>,
    boxes: HashMap<&'static str, TextBox>,
    widths: HashMap<&'static str, f32>,
    focused: Option<&'static str>,
    page: usize,
    show_key: bool,
    models: Vec<String>,
    /// What the last model list request said.
    status: String,
    loading: bool,
    picked_server: Option<usize>,
}

impl Draft {
    /// The settings with every text box's value, and the first problem,
    /// if a number isn't one.
    fn current(&self) -> (Settings, Option<String>) {
        let mut s = self.settings.clone().unwrap_or_default();
        let mut problem = None;
        for t in &TEXTS {
            if let Some(b) = self.boxes.get(t.id)
                && let Err(e) = set(&mut s, t.id, &b.text)
            {
                problem.get_or_insert(e);
            }
        }
        (s, problem)
    }

    /// The text boxes show `s` again (a preset was chosen, say).
    fn fill(&mut self, s: &Settings) {
        for t in &TEXTS {
            let b = self
                .boxes
                .entry(t.id)
                .or_insert_with(|| TextBox::new("", t.lines > 1, false, t.hidden));
            b.set(&value(s, t.id));
        }
    }
}

/// The dialog.
pub struct SettingsDialog {
    host: Host,
    measure: Arc<dyn Measure + Send + Sync>,
    draft: Arc<Mutex<Draft>>,
}

impl std::fmt::Debug for SettingsDialog {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SettingsDialog").finish_non_exhaustive()
    }
}

fn lock(d: &Mutex<Draft>) -> MutexGuard<'_, Draft> {
    d.lock().unwrap_or_else(PoisonError::into_inner)
}

fn label(text: &str, muted: bool) -> Row {
    Row {
        widgets: vec![Widget::Label {
            text: text.into(),
            muted,
        }],
    }
}

impl SettingsDialog {
    /// The dialog.
    pub fn new(host: Host, measure: Arc<dyn Measure + Send + Sync>) -> SettingsDialog {
        SettingsDialog {
            host,
            measure,
            draft: Arc::default(),
        }
    }

    /// The draft, made from the saved settings when the dialog opens.
    fn draft(&self) -> MutexGuard<'_, Draft> {
        let mut d = lock(&self.draft);
        if d.settings.is_none() {
            let s = Settings::load(&self.host);
            d.fill(&s);
            d.settings = Some(s);
        }
        d
    }

    /// Asks the server for its models, on a thread of its own; the dialog
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
                {
                    let mut d = lock(&draft);
                    d.loading = false;
                    match answer {
                        Ok(models) => {
                            d.status = format!("The server has {} models.", models.len());
                            d.models = models;
                        }
                        Err(e) => d.status = e,
                    }
                }
                host.redraw(crate::SETTINGS);
            });
        if spawned.is_err() {
            d.loading = false;
            d.status = "The model list couldn't be asked for.".into();
        }
    }

    /// A text's label and box.
    fn text_rows(&self, d: &mut Draft, id: &'static str) -> Vec<Row> {
        let Some(spec) = text_spec(id) else {
            return Vec::new();
        };
        let width = d.widths.get(id).copied().unwrap_or(480.0);
        let focused = d.focused == Some(id);
        let show_key = d.show_key;
        let Some(b) = d.boxes.get_mut(id) else {
            return Vec::new();
        };
        if spec.hidden {
            b.masked = !show_key;
        }
        let height = spec.lines as f32 * crate::ui::text::line_height() + 12.0;
        let mut items: Vec<Item> = Vec::new();
        b.draw(
            Rect {
                x: 0.0,
                y: 0.0,
                w: width,
                h: height,
            },
            focused,
            spec.placeholder,
            &*self.measure,
            &mut items,
        );
        vec![
            label(spec.label, false),
            Row {
                widgets: vec![Widget::Surface(Surface {
                    id: id.into(),
                    height,
                    items,
                    keys: true,
                    cursor: Cursor::Text,
                })],
            },
        ]
    }

    fn page_rows(&self, d: &mut Draft, s: &Settings) -> Vec<Row> {
        let mut rows = Vec::new();
        match d.page {
            0 => {
                let preset = PRESETS.iter().position(|p| p.id == s.preset).unwrap_or(0);
                rows.push(Row {
                    widgets: vec![Widget::Choice {
                        id: "preset".into(),
                        options: PRESETS.iter().map(|p| p.name.to_owned()).collect(),
                        selected: u32::try_from(preset).unwrap_or(0),
                        enabled: true,
                    }],
                });
                rows.push(Row {
                    widgets: vec![Widget::Choice {
                        id: "scheme".into(),
                        options: vec!["http".into(), "https".into()],
                        selected: u32::from(s.scheme.eq_ignore_ascii_case("https")),
                        enabled: true,
                    }],
                });
                for id in ["host", "port", "path", "query"] {
                    rows.extend(self.text_rows(d, id));
                }
                rows.push(label(
                    &match s.url("chat/completions") {
                        Ok(url) => format!("Requests go to {url}"),
                        Err(e) => format!("The address isn't complete: {e}."),
                    },
                    true,
                ));
                if !s.is_local() {
                    rows.push(label(
                        if s.may_send() {
                            "Not on this computer, and allowed: the conversation and what the model reads of the score are sent there."
                        } else {
                            "Not on this computer: the chat asks before anything is sent there."
                        },
                        true,
                    ));
                }
            }
            1 => {
                rows.extend(self.text_rows(d, "key"));
                rows.push(Row {
                    widgets: vec![Widget::Toggle {
                        id: "show-key".into(),
                        text: "Show the key".into(),
                        on: d.show_key,
                        enabled: true,
                    }],
                });
                let p = crate::settings::preset(&s.preset);
                if !p.keys_at.is_empty() {
                    rows.push(Row {
                        widgets: vec![Widget::Link {
                            text: format!("Get a key: {}", p.name),
                            url: p.keys_at.to_owned(),
                        }],
                    });
                }
                rows.extend(self.text_rows(d, "key-env"));
            }
            2 => {
                for id in ["auth-header", "auth-prefix", "headers"] {
                    rows.extend(self.text_rows(d, id));
                }
            }
            3 => {
                rows.extend(self.text_rows(d, "model"));
                rows.push(Row {
                    widgets: vec![Widget::Button {
                        id: "models".into(),
                        text: if d.loading {
                            "Asking…".into()
                        } else {
                            "List the Server's Models".into()
                        },
                        enabled: !d.loading,
                    }],
                });
                if !d.status.is_empty() {
                    rows.push(label(&d.status, true));
                }
                if !d.models.is_empty() {
                    let selected = d
                        .models
                        .iter()
                        .position(|m| *m == s.model)
                        .and_then(|i| u32::try_from(i).ok());
                    rows.push(Row {
                        widgets: vec![Widget::List {
                            id: "model-list".into(),
                            items: d.models.clone(),
                            selected,
                        }],
                    });
                }
            }
            4 => {
                for id in ["temperature", "max-tokens", "timeout", "steps"] {
                    rows.extend(self.text_rows(d, id));
                }
                rows.push(Row {
                    widgets: vec![Widget::Toggle {
                        id: "stream".into(),
                        text: "Show answers as they are written".into(),
                        on: s.stream,
                        enabled: true,
                    }],
                });
            }
            _ => {
                rows.extend(self.text_rows(d, "instructions"));
                if !s.allowed.is_empty() {
                    rows.push(label("Servers away from this computer you allowed", false));
                    rows.push(Row {
                        widgets: vec![Widget::List {
                            id: "allowed".into(),
                            items: s.allowed.clone(),
                            selected: d.picked_server.and_then(|i| u32::try_from(i).ok()),
                        }],
                    });
                    rows.push(Row {
                        widgets: vec![Widget::Button {
                            id: "forget".into(),
                            text: "Forget the Selected Server".into(),
                            enabled: d.picked_server.is_some(),
                        }],
                    });
                }
            }
        }
        rows
    }
}

impl Tab for SettingsDialog {
    fn content(&self) -> TabContent {
        let mut d = self.draft();
        let (s, problem) = d.current();
        let mut rows = vec![Row {
            widgets: vec![Widget::Choice {
                id: "page".into(),
                options: PAGES.iter().map(|p| (*p).to_owned()).collect(),
                selected: u32::try_from(d.page).unwrap_or(0),
                enabled: true,
            }],
        }];
        rows.extend(self.page_rows(&mut d, &s));
        if let Some(p) = problem {
            rows.push(label(&format!("{p}."), false));
        }
        TabContent { rows }
    }

    fn on(&self, widget: &str, event: TabEvent) -> Result<(), Error> {
        let mut d = self.draft();
        if let Some(spec) = text_spec(widget) {
            let id = spec.id;
            match event {
                TabEvent::Resized { width, .. } => {
                    d.widths.insert(id, width.max(40.0));
                    return Ok(());
                }
                TabEvent::Focus(true) => {
                    d.focused = Some(id);
                    return Ok(());
                }
                TabEvent::Focus(false) => {
                    if d.focused == Some(id) {
                        d.focused = None;
                    }
                    return Ok(());
                }
                _ => {}
            }
            let width = d.widths.get(id).copied().unwrap_or(480.0);
            let r = Rect {
                x: 0.0,
                y: 0.0,
                w: width,
                h: 200.0,
            };
            let measure = &*self.measure;
            let Some(b) = d.boxes.get_mut(id) else {
                return Ok(());
            };
            let mut copied = None;
            match event {
                TabEvent::Typed(t) => {
                    b.typed(&t);
                }
                TabEvent::Key(k) => {
                    if let Outcome::Copy(t) = b.key(k, measure, TextBox::inner(width)) {
                        copied = Some(t);
                    }
                }
                TabEvent::Pointer(p) if p.kind == PointerKind::Down => {
                    b.click(p.at, r, measure, false);
                }
                TabEvent::Pointer(p) if p.kind == PointerKind::Drag => b.drag(p.at, r, measure),
                _ => {}
            }
            drop(d);
            if let Some(t) = copied {
                let _ = self.host.copy(&t);
            }
            return Ok(());
        }
        match (widget, event) {
            ("page", TabEvent::Chosen(i)) => d.page = (i as usize).min(PAGES.len() - 1),
            ("preset", TabEvent::Chosen(i)) => {
                if let Some(p) = PRESETS.get(i as usize) {
                    let (mut s, _) = d.current();
                    s.apply_preset(p.id);
                    d.fill(&s);
                    d.settings = Some(s);
                }
            }
            ("scheme", TabEvent::Chosen(i)) => {
                if let Some(s) = d.settings.as_mut() {
                    s.scheme = if i == 1 { "https" } else { "http" }.into();
                }
            }
            ("stream", TabEvent::Toggled(on)) => {
                if let Some(s) = d.settings.as_mut() {
                    s.stream = on;
                }
            }
            ("show-key", TabEvent::Toggled(on)) => d.show_key = on,
            ("models", TabEvent::Clicked) => self.load_models(&mut d),
            ("model-list", TabEvent::Chosen(i)) => {
                if let Some(m) = d.models.get(i as usize).cloned()
                    && let Some(b) = d.boxes.get_mut("model")
                {
                    b.set(&m);
                }
            }
            ("allowed", TabEvent::Chosen(i)) => d.picked_server = Some(i as usize),
            ("forget", TabEvent::Clicked) => {
                if let Some(i) = d.picked_server.take()
                    && let Some(s) = d.settings.as_mut()
                    && i < s.allowed.len()
                {
                    s.allowed.remove(i);
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn closed(&self, ok: bool) -> Result<(), Error> {
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
    use crate::ui::text::Fixed;
    use oxiguitar_plugin::{Key, KeyName};

    fn page(d: &SettingsDialog, n: u32) -> TabContent {
        d.on("page", TabEvent::Chosen(n)).unwrap();
        d.content()
    }

    fn shows(c: &TabContent, id: &str, want: &str) -> bool {
        let Some(Widget::Surface(s)) = c.widget(id) else {
            panic!("{id}")
        };
        s.items
            .iter()
            .any(|i| matches!(i, Item::Text { text, .. } if text == want))
    }

    #[test]
    fn every_page_shows_its_parts_and_cancel_forgets() {
        let d = SettingsDialog::new(Host::detached(), Arc::new(Fixed));
        let pages = [
            vec!["preset", "scheme", "host", "port", "path", "query"],
            vec!["key", "show-key", "key-env"],
            vec!["auth-header", "auth-prefix", "headers"],
            vec!["model", "models"],
            vec!["temperature", "max-tokens", "timeout", "steps", "stream"],
            vec!["instructions"],
        ];
        for (n, ids) in pages.iter().enumerate() {
            let c = page(&d, n as u32);
            assert!(c.problems().is_empty(), "{:?}", c.problems());
            for id in ids {
                assert!(c.widget(id).is_some(), "page {n}: {id}");
            }
        }
        page(&d, 0);
        d.on("preset", TabEvent::Chosen(3)).unwrap();
        assert!(shows(&d.content(), "host", "api.openai.com"));
        d.on("key", TabEvent::Typed("sk-secret".into())).unwrap();
        let c = page(&d, 1);
        assert!(shows(&c, "key", "•••••••••"), "the key is hidden");
        d.on("show-key", TabEvent::Toggled(true)).unwrap();
        assert!(shows(&d.content(), "key", "sk-secret"));
        d.closed(false).unwrap();
        assert!(shows(&page(&d, 0), "host", "127.0.0.1"), "Cancel forgot it");
    }

    #[test]
    fn a_number_that_isnt_one_is_said_and_the_rest_kept() {
        let d = SettingsDialog::new(Host::detached(), Arc::new(Fixed));
        page(&d, 0);
        let backspace = Key {
            name: KeyName::Backspace,
            shift: false,
            ctrl: false,
            alt: false,
        };
        for _ in 0..5 {
            d.on("port", TabEvent::Key(backspace)).unwrap();
        }
        d.on("port", TabEvent::Typed("x".into())).unwrap();
        let c = d.content();
        assert!(format!("{c:?}").contains("The port is a whole number"));
        let (s, problem) = lock(&d.draft).current();
        assert!(problem.is_some());
        assert_eq!(s.port, 11434);
    }
}
