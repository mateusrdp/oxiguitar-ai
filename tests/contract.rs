// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Mateus Rocha de Paula and the OxiGuitar contributors

//! The contract suite: builds this crate as an `.oxp` file and checks it
//! through OxiGuitar's own plugin host, as the app would load it: every
//! widget of the chat tab and the settings dialog used once, the surfaces
//! with a bit of everything they hear. `cargo test` runs it with the rest.

use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

use oxiguitar_host::api::tab::{TabEvent, Widget};
use oxiguitar_host::{Plugins, SharedSettings, TabId, Unheard};

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

/// Through the host: the chat tab holds its buttons and a surface that
/// takes the keyboard; text typed shows in it; a question with no score
/// and no server to ask ends in a sentence, never a crash.
#[test]
fn the_chat_takes_typing_through_the_host() {
    let scratch =
        std::env::temp_dir().join(format!("oxiguitar-ai-contract-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&scratch);
    std::fs::create_dir_all(&scratch).unwrap();
    std::fs::copy(ours(), scratch.join("ai.oxp")).unwrap();
    let plugins = Plugins::discover(
        std::slice::from_ref(&scratch),
        SharedSettings::default(),
        "x11",
        Arc::new(Unheard),
    );
    let chat = TabId {
        plugin: "oxiguitar.ai".into(),
        tab: "chat".into(),
    };
    let content = plugins.tab_content(&chat).unwrap();
    assert!(content.widget("settings").is_some() && content.widget("new").is_some());
    let Some(Widget::Surface(s)) = content.widget("chat") else {
        panic!("{content:?}")
    };
    assert!(s.keys && s.height == 0.0);
    plugins
        .tab_event(
            &chat,
            "chat",
            TabEvent::Resized {
                width: 320.0,
                height: 500.0,
            },
        )
        .unwrap();
    plugins
        .tab_event(&chat, "chat", TabEvent::Focus(true))
        .unwrap();
    let typed = plugins
        .tab_event(
            &chat,
            "chat",
            TabEvent::Typed("What key is this in?".into()),
        )
        .unwrap();
    assert!(typed.problems().is_empty());
    let shown = format!("{typed:?}");
    assert!(shown.contains("What key is this in?"), "{shown}");
    let _ = std::fs::remove_dir_all(&scratch);
}
