# Changelog

All notable changes to the AI plugin. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/); versions follow [Semantic Versioning](https://semver.org/).

## [Unreleased]

### Added

- **The AI plugin** (mateusrdp/oxiguitar#227): a tab **AI** with a chat with a language model, on this computer (Ollama, llama.cpp, LM Studio) or hosted (OpenAI, Anthropic, Google, OpenRouter, any server speaking OpenAI's chat completions). The model reads the score and writes bars, adds tracks, changes bars' time, key, tempo and sections, sets the title, reads and moves the selection, and runs OxiGuitar's tools and other plugins'; each answer's changes are one change to undo. **AI Settings…** sets the server (each part of its address on its own), the key or its environment variable, the key's header, more headers, the model (or picks it from the server's list), the temperature, the most tokens, the timeout, the steps an answer may take, streaming and instructions of your own. A server away from this computer is used only once you allow it. Needs plugin interface 1.14.
