// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Mateus Rocha de Paula and the OxiGuitar contributors

//! The contract suite: builds this crate as an `.oxp` file and checks it
//! through OxiGuitar's own plugin host, as the app would load it, with
//! every view shown and closed, and a question asked in the chat. `cargo
//! test` runs it with the rest.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use oxiguitar_host::api::view::Value;
use oxiguitar_host::{Core, Level, Plugins, SharedSettings, ViewId};

/// This plugin as the `.oxp` file we ship (a debug build), built once.
fn ours() -> &'static Path {
    static BUILT: OnceLock<PathBuf> = OnceLock::new();
    BUILT.get_or_init(|| {
        let crate_folder = Path::new(env!("CARGO_MANIFEST_DIR"));
        oxiguitar_plugin_test::build(crate_folder, env!("CARGO_PKG_NAME"), "ai", false)
            .expect("the .oxp file builds")
    })
}

#[test]
fn the_plugin_file_keeps_the_contract() {
    let report = oxiguitar_plugin_test::check(ours());
    for passed in &report.passed {
        println!("ok    {passed}");
    }
    assert!(report.ok(), "failed: {:#?}", report.failed);
}

/// What the window shows: the properties last set on each view.
#[derive(Default)]
struct Window {
    properties: Mutex<Vec<(String, String, Value)>>,
}

impl Core for Window {
    fn notify(&self, _level: Level, _text: String) {}

    fn set_view(&self, view: &ViewId, properties: Vec<(String, Value)>) {
        let mut kept = self.properties.lock().unwrap();
        for (name, value) in properties {
            kept.retain(|(v, n, _)| !(*v == view.view && *n == name));
            kept.push((view.view.clone(), name, value));
        }
    }
}

impl Window {
    fn get(&self, view: &str, name: &str) -> Option<Value> {
        self.properties
            .lock()
            .unwrap()
            .iter()
            .find(|(v, n, _)| v == view && n == name)
            .map(|p| p.2.clone())
    }
}

/// Through the host: the chat panel shows its hint, a question sent shows
/// in the conversation and clears the box, and with no score and no
/// server to ask it ends in a sentence, never a crash.
#[test]
fn a_question_goes_through_the_host() {
    let scratch =
        std::env::temp_dir().join(format!("oxiguitar-ai-contract-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&scratch);
    std::fs::create_dir_all(&scratch).unwrap();
    std::fs::copy(ours(), scratch.join("ai.oxp")).unwrap();
    let window = Arc::new(Window::default());
    // Nothing answers on this port, on this computer.
    let settings = SharedSettings::default();
    settings
        .set(
            "oxiguitar.ai",
            oxiguitar_plugin::serde_json::json!({"host": "127.0.0.1", "port": 9, "timeout": 10}),
        )
        .unwrap();
    let plugins = Plugins::discover(
        std::slice::from_ref(&scratch),
        settings,
        oxiguitar_host::NO_DISPLAY,
        window.clone(),
    );
    let chat = ViewId {
        plugin: "oxiguitar.ai".into(),
        view: "chat".into(),
    };
    plugins.view_shown(&chat, None);
    assert!(
        matches!(window.get("chat", "hint"), Some(Value::String(h)) if h.starts_with("Ask about the score"))
    );
    plugins.view_callback(
        &chat,
        "send",
        vec![Value::String("What key is this in?".into())],
    );
    assert_eq!(
        window.get("chat", "question"),
        Some(Value::String(String::new()))
    );
    let ended = |w: &Window| match w.get("chat", "entries") {
        Some(Value::List(entries)) => {
            let shown = format!("{entries:?}");
            assert!(shown.contains("What key is this in?"), "{shown}");
            w.get("chat", "running") == Some(Value::Bool(false)) && entries.len() > 1
        }
        _ => false,
    };
    let start = Instant::now();
    while !ended(&window) {
        assert!(
            start.elapsed() < Duration::from_secs(60),
            "the turn never ended"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
    let _ = std::fs::remove_dir_all(&scratch);
}
