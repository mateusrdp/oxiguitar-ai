// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Mateus Rocha de Paula and the OxiGuitar contributors

//! OxiGuitar's AI plugin (plugin interface 1.16; mateusrdp/oxiguitar#227,
//! #346, docs/design.md): a chat with a language model in a panel of its own, on
//! this computer (llama.cpp, Ollama, LM Studio) or hosted (OpenAI,
//! Anthropic, Google, OpenRouter, any server speaking OpenAI's chat
//! completions), which reads and writes the open score and runs
//! OxiGuitar's tools, each answer kept as one change the user can undo.
//!
//! - `settings`: where the server is and how to sign in;
//! - `client`: the chat completions protocol, streamed or whole;
//! - `notation`: the score as text, the model's way to read and write it;
//! - `skills`: the user's own notes the model reads when they fit;
//! - `tools`: what the model may do, on a copy of the score;
//! - `agent`: the conversation and the model's turn, on a thread of its
//!   own;
//! - `ui`: the chat panel and the AI Settings window, views of
//!   `ui/ai.slint`;
//! - `links`: a provider's page for keys, opened in the browser.

use std::sync::Arc;

use oxiguitar_plugin::{Error, Host, Plugin, Tool, TrackView, Ui, Value};

pub mod agent;
pub mod client;
mod links;
pub mod notation;
pub mod settings;
pub mod skills;
pub mod tools;
pub mod ui;

/// The plugin's id.
pub const ID: &str = "oxiguitar.ai";
/// The chat panel's id.
pub const CHAT: &str = "chat";
/// The AI Settings window's id (and its tool's).
pub const SETTINGS: &str = "settings";
/// The shared folder the user's skills are in.
pub const SKILLS: &str = "skills";

/// The plugin.
#[derive(Debug)]
pub struct AiPlugin {
    chat: ui::chat::Chat,
    settings: ui::settings_window::SettingsWindow,
}

/// The tool AI Settings… opens the window.
impl Tool for ui::settings_window::SettingsWindow {
    fn run(&self) -> Result<String, Error> {
        self.open()?;
        Ok(String::new())
    }
}

impl Plugin for AiPlugin {
    fn start(host: Host) -> Result<AiPlugin, Error> {
        let shared = Arc::new(agent::Shared::default());
        Ok(AiPlugin {
            chat: ui::chat::Chat::new(host.clone(), shared, Arc::new(host.clone())),
            settings: ui::settings_window::SettingsWindow::new(host),
        })
    }

    fn tool(&self, id: &str) -> Option<&dyn Tool> {
        match id {
            SETTINGS => Some(&self.settings),
            _ => None,
        }
    }

    fn ui(&self) -> Option<&dyn Ui> {
        Some(self)
    }
}

/// The chat panel and the AI Settings window (`ui/ai.slint`).
impl Ui for AiPlugin {
    fn shown(&self, view: &str, _track: Option<&TrackView>) -> Result<(), Error> {
        match view {
            CHAT => self.chat.show(),
            SETTINGS => self.settings.shown(),
            _ => Ok(()),
        }
    }

    fn callback(&self, view: &str, callback: &str, args: Vec<Value>) -> Result<(), Error> {
        match view {
            CHAT => self.chat.callback(callback, &args),
            SETTINGS => self.settings.callback(callback, &args),
            _ => Ok(()),
        }
    }

    /// The window closed by the user (its close button, Escape) forgets
    /// the draft, as Cancel does.
    fn closed(&self, view: &str, _accepted: bool) -> Result<(), Error> {
        match view {
            SETTINGS => self.settings.finish(false),
            _ => Ok(()),
        }
    }
}

oxiguitar_plugin::export_plugin!(AiPlugin);

#[cfg(test)]
mod tests {
    use super::*;

    const MANIFEST: &str = include_str!("../manifest.json");

    #[test]
    fn the_manifest_is_valid_for_the_plugin() {
        let problems = oxiguitar_plugin::check::<AiPlugin>(MANIFEST, env!("CARGO_PKG_VERSION"));
        assert!(problems.is_empty(), "{}", problems.join("; "));
    }

    /// The file compiles as OxiGuitar compiles it, with every property and
    /// callback the chat and the window use.
    #[test]
    fn the_slint_file_has_what_the_views_set_and_hear() {
        let file = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("ui/ai.slint");
        let compiled = oxiguitar_plugin_test::compile_ui_file(&file).unwrap();
        let component = |name: &str| compiled.component(name).unwrap();
        let chat = component("Chat");
        let shared = agent::Shared::default();
        let mut properties: Vec<String> = ui::chat::properties(&shared, &String::new)
            .into_iter()
            .map(|p| p.0)
            .collect();
        properties.push("question".into());
        for name in properties {
            assert!(chat.properties.contains(&name), "{name}");
        }
        for name in ["new-chat", "send", "stop", "copy", "allow", "deny"] {
            assert!(chat.callbacks.iter().any(|c| c == name), "{name}");
        }
        let window = component("SettingsWindow");
        for name in ui::settings_window::property_names() {
            assert!(window.properties.contains(&name), "{name}");
        }
        for name in [
            "preset-chosen",
            "scheme-chosen",
            "edited",
            "show-key-toggled",
            "get-key",
            "list-models",
            "model-chosen",
            "stream-toggled",
            "server-chosen",
            "forget-server",
            "ok",
            "cancel",
        ] {
            assert!(window.callbacks.iter().any(|c| c == name), "{name}");
        }
    }
}
