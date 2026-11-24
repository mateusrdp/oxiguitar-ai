# Changelog

All notable changes to the AI plugin. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/); versions follow [Semantic Versioning](https://semver.org/).

## [Unreleased]

### Added

- **Every test with one command** (mateusrdp/oxiguitar#273): `scripts/test-all.py` builds and runs all of this repository's tests, and tests the plugin against the newest OxiGuitar release, with a progress bar, a summary to paste into an issue and a log to diagnose from. The manual has a Features page naming the tests of every feature (CI checks it) and a Testing page.

### Changed

- **Plugins → AI Chat is gone** (mateusrdp/oxiguitar-ai#12): it did nothing, and the AI tab is already in the sidebar. The **Settings…** button is gone from the AI tab too, to clear the screen; **Plugins → AI Settings…** is the one way in.
- Built against the SDK of OxiGuitar 0.21.5 (its tag `v0.21.5`, interface 1.14) instead of a commit of its development branch. Nothing changes in how the plugin works.
- **The manual explains the ideas behind the plugin** (its overview, with pictures), shows how to use it with screenshots, says in Architecture and strategy what it can't do yet, what is left out on purpose and where help is wanted, and has a Credits page listing every project it uses, with links and thanks. The README has a plain-words introduction and a Credits section.

## [0.1.0] - 2026-11-20

### Added

- **The AI plugin** (mateusrdp/oxiguitar#227): a tab **AI** with a chat with a language model, on this computer (Ollama, llama.cpp, LM Studio) or hosted (OpenAI, Anthropic, Google, OpenRouter, any server speaking OpenAI's chat completions). The model reads the score and writes bars, adds tracks, changes bars' time, key, tempo and sections, sets the title, reads and moves the selection, and runs OxiGuitar's tools and other plugins'; each answer's changes are one change to undo. **AI Settings…** sets the server (each part of its address on its own), the key or its environment variable, the key's header, more headers, the model (or picks it from the server's list), the temperature, the most tokens, the timeout, the steps an answer may take, streaming and instructions of your own. A server away from this computer is used only once you allow it. Needs plugin interface 1.14.
