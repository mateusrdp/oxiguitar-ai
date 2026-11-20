// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Mateus Rocha de Paula and the OxiGuitar contributors

//! OxiGuitar's AI plugin (plugin interface 1.14; mateusrdp/oxiguitar#227,
//! docs/design.md): a chat with a language model in a tab of its own, on
//! this computer (llama.cpp, Ollama, LM Studio) or hosted (OpenAI,
//! Anthropic, Google, OpenRouter, any server speaking OpenAI's chat
//! completions), which reads and writes the open score and runs
//! OxiGuitar's tools, each answer kept as one change the user can undo.
//!
//! - `settings`: where the server is and how to sign in;
//! - `client`: the chat completions protocol, streamed or whole;
//! - `notation`: the score as text, the model's way to read and write it;
//! - `tools`: what the model may do, on a copy of the score;
//! - `agent`: the conversation and the model's turn, on a thread of its
//!   own;
//! - `ui`: the chat tab and the settings dialog, drawn on surfaces.

use std::sync::Arc;

use oxiguitar_plugin::{Error, Host, Plugin, Tab, Tool};

pub mod agent;
pub mod client;
pub mod notation;
pub mod settings;
pub mod tools;
pub mod ui;

/// The plugin's id.
pub const ID: &str = "oxiguitar.ai";
/// The chat tab's id.
pub const CHAT: &str = "chat";
/// The settings dialog's id.
pub const SETTINGS: &str = "settings";

/// The plugin.
#[derive(Debug)]
pub struct AiPlugin {
    chat: ui::chat::ChatTab,
    settings: ui::settings_dialog::SettingsDialog,
    open_settings: Open,
    open_chat: Open,
}

/// A tool that only shows something: the settings dialog, or the chat tab.
#[derive(Debug)]
struct Open {
    host: Host,
    dialog: bool,
}

impl Tool for Open {
    fn run(&self) -> Result<String, Error> {
        if self.dialog {
            self.host.open_dialog(SETTINGS)?;
        } else {
            self.host.show_tab(CHAT);
        }
        Ok(String::new())
    }
}

impl Plugin for AiPlugin {
    fn start(host: Host) -> Result<AiPlugin, Error> {
        let measure = Arc::new(ui::text::Measured::new(host.clone()));
        let shared = Arc::new(agent::Shared::default());
        Ok(AiPlugin {
            chat: ui::chat::ChatTab::new(
                host.clone(),
                shared,
                measure.clone(),
                Arc::new(host.clone()),
            ),
            settings: ui::settings_dialog::SettingsDialog::new(host.clone(), measure),
            open_settings: Open {
                host: host.clone(),
                dialog: true,
            },
            open_chat: Open {
                host: host.clone(),
                dialog: false,
            },
        })
    }

    fn tab(&self, id: &str) -> Option<&dyn Tab> {
        match id {
            CHAT => Some(&self.chat),
            SETTINGS => Some(&self.settings),
            _ => None,
        }
    }

    fn tool(&self, id: &str) -> Option<&dyn Tool> {
        match id {
            SETTINGS => Some(&self.open_settings),
            CHAT => Some(&self.open_chat),
            _ => None,
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
}
