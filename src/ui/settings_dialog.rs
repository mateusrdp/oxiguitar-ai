// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Mateus Rocha de Paula and the OxiGuitar contributors

//! The dialog `settings`: the server (a preset, then each part of its
//! address on its own), the key and the other headers, the model (typed,
//! or picked from the server's list), the answers' limits, the user's own
//! instructions and the servers away from this computer they allowed. A
//! draft while it is open: OK keeps it, Cancel forgets it.
//!
//! OxiGuitar's own widgets where they fit; the key (hidden as it is
//! typed), the extra headers and the instructions (several lines) are text
//! boxes of the plugin's own, on surfaces.

use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use oxiguitar_plugin::display::Item;
use oxiguitar_plugin::{
    Cursor, Error, Host, Level, PointerKind, Row, Surface, Tab, TabContent, TabEvent, Widget,
};

use crate::client::Client;
use crate::settings::{PRESETS, Settings};
use crate::ui::text::{Measure, Outcome, Rect, TextBox};

/// The dialog's text boxes, by their surface's id.
const BOXES: [&str; 3] = ["key", "headers", "instructions"];

#[derive(Debug, Default)]
struct Draft {
    settings: Option<Settings>,
    key: TextBox,
    headers: TextBox,
    instructions: TextBox,
    widths: [f32; 3],
    focused: Option<usize>,
    show_key: bool,
    models: Vec<String>,
    /// What the last model list request said.
    status: String,
    loading: bool,
    picked_server: Option<usize>,
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
            d.key = TextBox::new(&s.api_key, false, false, true);
            d.headers = TextBox::new(&s.extra_headers, true, false, false);
            d.instructions = TextBox::new(&s.instructions, true, false, false);
            d.widths = [300.0; 3];
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
        let Some(mut s) = d.settings.clone() else {
            return;
        };
        s.api_key = d.key.text.clone();
        s.extra_headers = d.headers.text.clone();
        if !s.may_send() {
            d.status = format!(
                "{} isn't on this computer: ask a question in the chat first and allow it, or choose a server here.",
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

    fn text_box(&self, d: &mut Draft, i: usize, height: f32) -> Widget {
        let width = d.widths[i];
        let focused = d.focused == Some(i);
        let mut items: Vec<Item> = Vec::new();
        let r = Rect {
            x: 0.0,
            y: 0.0,
            w: width,
            h: height,
        };
        let placeholder = match i {
            0 => "Pasted from the provider's site",
            1 => "One a line, Name: value",
            _ => "Such as: answer in Portuguese; keep to the guitar's first five frets",
        };
        let b = match i {
            0 => &mut d.key,
            1 => &mut d.headers,
            _ => &mut d.instructions,
        };
        if i == 0 {
            b.masked = !d.show_key;
        }
        b.draw(r, focused, placeholder, &*self.measure, &mut items);
        Widget::Surface(Surface {
            id: BOXES[i].into(),
            height,
            items,
            keys: true,
            cursor: Cursor::Text,
        })
    }
}

fn label(text: &str) -> Widget {
    Widget::Label {
        text: text.into(),
        muted: false,
    }
}

fn hint(text: &str) -> Widget {
    Widget::Label {
        text: text.into(),
        muted: true,
    }
}

fn field(id: &str, text: &str, placeholder: &str) -> Widget {
    Widget::Field {
        id: id.into(),
        text: text.into(),
        placeholder: placeholder.into(),
        enabled: true,
    }
}

fn row(widgets: Vec<Widget>) -> Row {
    Row { widgets }
}

impl Tab for SettingsDialog {
    fn content(&self) -> TabContent {
        let mut d = self.draft();
        let s = d.settings.clone().unwrap_or_default();
        let preset = PRESETS.iter().position(|p| p.id == s.preset).unwrap_or(0);
        let mut rows = vec![
            row(vec![label("Server")]),
            row(vec![Widget::Choice {
                id: "preset".into(),
                options: PRESETS.iter().map(|p| p.name.to_owned()).collect(),
                selected: u32::try_from(preset).unwrap_or(0),
                enabled: true,
            }]),
            row(vec![Widget::Choice {
                id: "scheme".into(),
                options: vec!["http".into(), "https".into()],
                selected: u32::from(s.scheme.eq_ignore_ascii_case("https")),
                enabled: true,
            }]),
            row(vec![field("host", &s.host, "Host name or address")]),
            row(vec![field("port", &s.port.to_string(), "Port")]),
            row(vec![field("path", &s.path, "Path, such as /v1")]),
            row(vec![field(
                "query",
                &s.query,
                "Query, such as api-version=2024-10-21 (usually empty)",
            )]),
            row(vec![hint(&match s.url("chat/completions") {
                Ok(url) => format!("Requests go to {url}"),
                Err(e) => format!("The address isn't complete: {e}."),
            })]),
        ];
        if !s.is_local() {
            rows.push(row(vec![hint(if s.may_send() {
                "This server isn't on this computer, and you allowed it: the conversation and what the model reads of the score are sent there."
            } else {
                "This server isn't on this computer: the chat asks before anything is sent there."
            })]));
        }
        rows.push(row(vec![label("Key (personal access token)")]));
        rows.push(row(vec![self.text_box(&mut d, 0, 32.0)]));
        rows.push(row(vec![Widget::Toggle {
            id: "show-key".into(),
            text: "Show the key".into(),
            on: d.show_key,
            enabled: true,
        }]));
        let p = crate::settings::preset(&s.preset);
        if !p.keys_at.is_empty() {
            rows.push(row(vec![Widget::Link {
                text: format!("Get a key for {}", p.name),
                url: p.keys_at.to_owned(),
            }]));
        }
        rows.push(row(vec![field(
            "key-env",
            &s.api_key_env,
            "Or the environment variable that holds it, such as OPENAI_API_KEY",
        )]));
        rows.push(row(vec![field(
            "auth-header",
            &s.auth_header,
            "The key's header",
        )]));
        rows.push(row(vec![field(
            "auth-prefix",
            &s.auth_prefix,
            "Before the key, such as \"Bearer \"",
        )]));
        rows.push(row(vec![label("More headers")]));
        rows.push(row(vec![self.text_box(&mut d, 1, 56.0)]));
        rows.push(row(vec![label("Model")]));
        rows.push(row(vec![field(
            "model",
            &s.model,
            "The model's name, as the server knows it",
        )]));
        rows.push(row(vec![Widget::Button {
            id: "models".into(),
            text: if d.loading {
                "Asking…".into()
            } else {
                "List the Server's Models".into()
            },
            enabled: !d.loading,
        }]));
        if !d.status.is_empty() {
            rows.push(row(vec![hint(&d.status)]));
        }
        if !d.models.is_empty() {
            let selected = d
                .models
                .iter()
                .position(|m| *m == s.model)
                .and_then(|i| u32::try_from(i).ok());
            rows.push(row(vec![Widget::List {
                id: "model-list".into(),
                items: d.models.clone(),
                selected,
            }]));
        }
        rows.push(row(vec![label("Answers")]));
        rows.push(row(vec![field(
            "temperature",
            &s.temperature,
            "Temperature, 0 to 2 (empty: the server's)",
        )]));
        rows.push(row(vec![field(
            "max-tokens",
            &s.max_tokens,
            "Most tokens an answer (empty: the server's)",
        )]));
        rows.push(row(vec![field(
            "timeout",
            &s.timeout.to_string(),
            "Seconds a request may take",
        )]));
        rows.push(row(vec![field(
            "steps",
            &s.max_steps.to_string(),
            "Tool calls an answer may make",
        )]));
        rows.push(row(vec![Widget::Toggle {
            id: "stream".into(),
            text: "Show answers as they are written".into(),
            on: s.stream,
            enabled: true,
        }]));
        rows.push(row(vec![label("Your instructions for the model")]));
        rows.push(row(vec![self.text_box(&mut d, 2, 72.0)]));
        if !s.allowed.is_empty() {
            rows.push(row(vec![label(
                "Servers away from this computer you allowed",
            )]));
            rows.push(row(vec![Widget::List {
                id: "allowed".into(),
                items: s.allowed.clone(),
                selected: d.picked_server.and_then(|i| u32::try_from(i).ok()),
            }]));
            rows.push(row(vec![Widget::Button {
                id: "forget".into(),
                text: "Forget the Selected Server".into(),
                enabled: d.picked_server.is_some(),
            }]));
        }
        TabContent { rows }
    }

    fn on(&self, widget: &str, event: TabEvent) -> Result<(), Error> {
        let mut d = self.draft();
        if let Some(i) = BOXES.iter().position(|b| *b == widget) {
            let measure = &*self.measure;
            match event {
                TabEvent::Resized { width, .. } => {
                    d.widths[i] = width.max(40.0);
                    return Ok(());
                }
                TabEvent::Focus(true) => {
                    d.focused = Some(i);
                    return Ok(());
                }
                TabEvent::Focus(false) => {
                    if d.focused == Some(i) {
                        d.focused = None;
                    }
                    return Ok(());
                }
                _ => {}
            }
            let width = d.widths[i];
            let r = Rect {
                x: 0.0,
                y: 0.0,
                w: width,
                h: 200.0,
            };
            let b = match i {
                0 => &mut d.key,
                1 => &mut d.headers,
                _ => &mut d.instructions,
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
                    b.click(p.at, r, measure, false)
                }
                TabEvent::Pointer(p) if p.kind == PointerKind::Drag => b.drag(p.at, r, measure),
                _ => {}
            }
            if let Some(t) = copied {
                let _ = self.host.copy(&t);
            }
            return Ok(());
        }
        let Some(s) = d.settings.as_mut() else {
            return Ok(());
        };
        let text = |e: &TabEvent| match e {
            TabEvent::Edited(t) => Some(t.clone()),
            _ => None,
        };
        let number = |e: &TabEvent, what: &str| -> Result<Option<u32>, Error> {
            match text(e) {
                None => Ok(None),
                Some(t) => t.trim().parse::<u32>().map(Some).map_err(|_| {
                    Error::new(
                        oxiguitar_plugin::ErrorKind::Invalid,
                        format!("{what} is a whole number"),
                    )
                }),
            }
        };
        match (widget, &event) {
            ("preset", TabEvent::Chosen(i)) => {
                if let Some(p) = PRESETS.get(*i as usize) {
                    s.apply_preset(p.id);
                }
            }
            ("scheme", TabEvent::Chosen(i)) => {
                s.scheme = if *i == 1 { "https" } else { "http" }.into();
            }
            ("host", e) => {
                if let Some(t) = text(e) {
                    s.host = t.trim().to_owned();
                }
            }
            ("port", e) => {
                if let Some(p) = number(e, "the port")? {
                    s.port = u16::try_from(p).ok().filter(|p| *p > 0).ok_or_else(|| {
                        Error::new(
                            oxiguitar_plugin::ErrorKind::Invalid,
                            "the port is 1 to 65535",
                        )
                    })?;
                }
            }
            ("path", e) => {
                if let Some(t) = text(e) {
                    s.path = t.trim().to_owned();
                }
            }
            ("query", e) => {
                if let Some(t) = text(e) {
                    s.query = t.trim().to_owned();
                }
            }
            ("key-env", e) => {
                if let Some(t) = text(e) {
                    s.api_key_env = t.trim().to_owned();
                }
            }
            ("auth-header", e) => {
                if let Some(t) = text(e) {
                    s.auth_header = t.trim().to_owned();
                }
            }
            // Spaces matter here ("Bearer "): kept as typed.
            ("auth-prefix", e) => {
                if let Some(t) = text(e) {
                    s.auth_prefix = t;
                }
            }
            ("model", e) => {
                if let Some(t) = text(e) {
                    s.model = t.trim().to_owned();
                }
            }
            ("temperature", e) => {
                if let Some(t) = text(e) {
                    s.temperature = t.trim().to_owned();
                }
            }
            ("max-tokens", e) => {
                if let Some(t) = text(e) {
                    s.max_tokens = t.trim().to_owned();
                }
            }
            ("timeout", e) => {
                if let Some(t) = number(e, "the timeout")? {
                    s.timeout = t.clamp(10, 3600);
                }
            }
            ("steps", e) => {
                if let Some(t) = number(e, "the steps")? {
                    s.max_steps = t.clamp(1, 100);
                }
            }
            ("stream", TabEvent::Toggled(on)) => s.stream = *on,
            ("show-key", TabEvent::Toggled(on)) => d.show_key = *on,
            ("models", TabEvent::Clicked) => self.load_models(&mut d),
            ("model-list", TabEvent::Chosen(i)) => {
                if let Some(m) = d.models.get(*i as usize).cloned()
                    && let Some(s) = d.settings.as_mut()
                {
                    s.model = m;
                }
            }
            ("allowed", TabEvent::Chosen(i)) => d.picked_server = Some(*i as usize),
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
        let mut d = lock(&self.draft);
        let draft = std::mem::take(&mut *d);
        if !ok {
            return Ok(());
        }
        let Some(mut s) = draft.settings else {
            return Ok(());
        };
        s.api_key = draft.key.text.trim().to_owned();
        s.extra_headers = draft.headers.text;
        s.instructions = draft.instructions.text;
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
    use oxiguitar_plugin::display::Paint;

    #[test]
    fn the_dialog_shows_every_part_and_cancel_forgets() {
        let host = Host::detached();
        let d = SettingsDialog::new(host, Arc::new(Fixed));
        let c = d.content();
        assert!(c.problems().is_empty(), "{:?}", c.problems());
        for id in [
            "preset",
            "scheme",
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
            "models",
            "temperature",
            "max-tokens",
            "timeout",
            "steps",
            "stream",
            "instructions",
        ] {
            assert!(c.widget(id).is_some(), "{id}");
        }
        d.on("preset", TabEvent::Chosen(3)).unwrap();
        d.on("key", TabEvent::Typed("sk-secret".into())).unwrap();
        let c = d.content();
        assert!(
            matches!(c.widget("host"), Some(Widget::Field { text, .. }) if text == "api.openai.com")
        );
        // The key shows as dots.
        let Some(Widget::Surface(key)) = c.widget("key") else {
            panic!()
        };
        assert!(
            key.items
                .iter()
                .any(|i| matches!(i, Item::Text { text, .. } if text == "•••••••••"))
        );
        assert!(d.on("port", TabEvent::Edited("x".into())).is_err());
        d.closed(false).unwrap();
        let again = d.content();
        assert!(
            matches!(again.widget("host"), Some(Widget::Field { text, .. }) if text == "127.0.0.1")
        );
    }

    #[test]
    fn a_detached_host_paints_the_boxes_in_the_themes_roles() {
        let d = SettingsDialog::new(Host::detached(), Arc::new(Fixed));
        let c = d.content();
        let Some(Widget::Surface(s)) = c.widget("instructions") else {
            panic!()
        };
        assert!(s.items.iter().any(|i| matches!(
            i,
            Item::Rect {
                paint: Paint::Page,
                ..
            }
        )));
    }
}
