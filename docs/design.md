# The AI plugin: design

mateusrdp/oxiguitar#227, as a proof of concept (Mateus, 20 Nov 2026): a plugin that talks to a language model, reads and writes the score, runs OxiGuitar's tools and answers the user, in a chat tab of its own with a settings window. Its other purpose is to find what the plugin interface still lacks before v1.0 freezes it; what it found became interface 1.14 (mateusrdp/oxiguitar#323).

## Decided by Mateus (20 Nov 2026)

- **One protocol: OpenAI's chat completions**, which local servers (llama.cpp, Ollama, LM Studio) and the hosted ones (OpenAI, and Anthropic's and Google's compatible endpoints) all speak. Paid services are supported with the user's own keys and anything else they need (headers, a query).
- **Presets fill the address, but every part is its own field**, saved on its own in `settings.json`: scheme, host, port, path, query, the key, the key's environment variable, header and prefix, more headers, the model, the temperature, the most tokens, the timeout, the steps, streaming, the user's instructions.
- **Privacy**: a server on this computer is used at once; one anywhere else only once the user allows it, asked in the chat, naming the server. The packages don't suggest the plugin.
- **The Flatpak** has no network: documented (`flatpak override --share=network`), the app's manifest unchanged.
- **The model may also run tools**, OxiGuitar's and other plugins' ("automate tasks and procedures"), and select bars; not File's entries (files stay the user's).
- **The core owns nothing of the plugin.** No chat widget, text box or hidden field in the core: the plugin drew its own on surfaces (interface 1.14) until plugins could bring their own UI; since interface 1.16 (mateusrdp/oxiguitar#346) the chat and the settings are views of its own Slint file, `ui/ai.slint`, with Slint's text boxes.
- One answer's changes are **one undoable change**; no preview to accept.
- **The key in `settings.json`** at this stage (no keyring); **the chat in memory only** (not saved with the score).
- HTTP: anything pure Rust and reliable. ureq 3 with rustls; rustls's cryptography is ring, the mature provider (it holds C and assembly: the pure-Rust providers, rustls-rustcrypto and graviola, are marked experimental or young). Mozilla's roots built in (webpki-roots), so nothing depends on the system's certificates. A server on this computer is plain HTTP and needs none of it.

## How it works

- `settings.rs`: the settings, the presets, the address built from its parts (`url`), whether the server is on this computer (`is_local`: `localhost`, `*.localhost`, loopback addresses; another computer on the network counts as away), the headers. `Settings::load` reads them from the host every time they are used, so the window's OK takes effect at once.
- `client.rs`: `POST {address}/chat/completions` with the conversation, the tools and `tool_choice: auto`; streamed (server-sent events, `Events` puts text and tool calls together by index, as OpenAI sends them; a server that sends whole objects or no index is understood too) or whole. `GET {address}/models` for the window's list. A server's error is a sentence with a hint (`server_error`).
- `notation.rs`: the score as the model reads and writes it. A bar is its beats separated by `|`; a beat is a duration (`w h q e s t x`, dots, `/3`) and what sounds: `r`, `string/fret` (string 1 the highest), `3/x` dead, `~` tied, a pitch (`C4`, put on the lowest fret free when the track has strings), `k36` a drum's General MIDI key, chords in brackets. The same notation both ways, so the model writes what it reads. `write_bars` checks everything before changing anything (strings, frets, a string twice in a beat, keys only on drum tracks) and reports bars that don't add up, which it still writes, as the editor does.
- `tools.rs`: the model's tools (`definitions`) and the instructions (`INSTRUCTIONS`), and `Workspace`, which works on a copy of the score and hands it back once (`flush`) at the end of the answer, or before a tool of OxiGuitar's runs or the selection moves, so they see the changes. `Editor` is the host's side, a fake in tests.
- `agent.rs`: the conversation (`Shared`: what the chat shows and what the model is sent) and the model's turn, on a thread of its own: the request, the answer streamed into the chat (drawn again at most every 80 ms), each tool call run and its result sent back, until the model answers without a call or its steps run out; Stop and New Chat end a turn (a generation number keeps an old turn's results out).
- `skills.rs` (mateusrdp/oxiguitar-ai#10): the user's own notes. A skill is `NAME.md` or `NAME/SKILL.md` in the `skills` folder (a shared folder of the manifest, beside `plugins`, so `Host::folder("skills")` gives the user's and the system's), with an optional header (`name`, `description`, `always`). `load` reads the folders when a question is sent, the user's first, and the first of a name wins; a file over 64 KiB, empty or not UTF-8 text is left out and said in the chat. `prompt` tells the model each on-request skill's name and description, and holds the text of the `always` ones; the `use_skill` tool, offered only when there is a skill to load, gives the rest on request, so a small model's context holds only what it asked for. The layout follows other assistants' skills so a folder can be shared; nothing else of theirs (scripts, resources, tool permissions) is read.
- `ui/ai.slint`: the views. `Chat`, the panel: a New Chat button (the settings are in the Plugins menu only), the conversation in a scroll view that follows the newest unless scrolled up, each answer with Copy; the question waiting for the user's permission; a text box where Enter sends, Shift + Enter starts a new line and Escape stops the model; Send, or Stop while the model works. `SettingsWindow`: six tabs, OK and Cancel.
- `ui/chat.rs`: what the panel shows (`properties`), set again from the model's thread as it writes, and what its callbacks do.
- `ui/settings_window.rs`: the window's draft: what it shows, the texts set only when the window opens or a preset or a listed model fills them (never back under the user's caret), the key hidden unless Show the Key is ticked; the server's models listed on a thread of its own; OK keeps the draft, Cancel or closing the window forgets it.
- `links.rs`: the preset's page for keys, opened in the browser (`https` only) when the user clicks it.

## Skills (mateusrdp/oxiguitar-ai#10, 24 Nov 2026)

The issue asks for a way to use skills, important with local models, so the user can reuse composition preferences and habit directives. Taken as: text files of the user's own, which the model loads when they fit. Not decided by Mateus yet; open to change:

- **Files, not settings.** One box of instructions already exists (AI Settings); skills are many notes, each for one kind of request, kept as files the user edits in any editor, copies and shares.
- **Loaded on request.** Listing every note in every request would fill a small local model's context. The model gets names and descriptions and asks for the text (`use_skill`), as it asks for the score's bars.
- **`always: true`** for habits that hold for every request; the cost is the text in every request.
- **Plain text only.** A skill holds no scripts or tools: the model's tools stay the plugin's own, and nothing a skill says runs by itself.
- **Read at each question**, so edits apply at once and nothing is cached or watched.
- Skills go to the server with the chat, so the privacy rule covers them: a server away from this computer gets them only once allowed.

## Later, maybe

- Skills from the chat ("save this as a skill"), and a skills page in AI Settings listing what was found.

- The conversation saved with the score (plugin data on the score, kept in the `.oxg`).
- A keyring for keys.
- Anthropic's own Messages API and Google's own API, for what their OpenAI-compatible endpoints leave out.
- Selecting text in the conversation (Copy takes a whole answer now); a preview of the changes before they are kept.
- The model hearing the playback position, or playing the score.
- A small model of our own, fine-tuned to write tablature (the issue's hope).
