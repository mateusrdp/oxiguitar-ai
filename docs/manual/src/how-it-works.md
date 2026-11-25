# Architecture and strategy

The plugin is a tab and a settings window. The core holds nothing of it: both are the plugin's own Slint views (`ui/ai.slint`), which OxiGuitar shows with its own widgets and colours, and everything the model does goes through the interface's score changes and tool calls.

There is one protocol, OpenAI's chat completions. Presets fill in an address, but each part of it, which is the scheme, host, port, path, query, key header, model and more, is its own saved setting. HTTP uses `ureq` with `rustls`, and the root certificates come from Mozilla.

The model may run tools, and may move the caret or the selection. It may not use File menu entries, so files stay yours.

## Current limitations

- **It reads and writes a compact text form of the score**: notes and frets, rests, durations, ties, chords, dead notes and drums. Effects such as bends, slides, palm mutes and dynamics aren't in it, so the model can't see or write them; notes it writes get a plain mezzo-forte.
- **Small local models struggle.** Tool calling and music both take a capable model. Small ones may call tools wrongly or write weak parts.
- **The conversation isn't saved** with the song.
- **No audio.** The model never hears the song; it only reads the notes.

## Left out on purpose

- **Running a model inside OxiGuitar.** Bundling a model would make the download huge and tie it to one model. You run the one you like with a program made for it.
- **Being on by default.** This is the one official plugin that can connect to the internet, so the package repositories don't suggest it: you install it on purpose, and it asks before sending anything to a server that isn't on your computer.
- **The File menu.** The model can't open, save, export or delete files. Your files stay yours.
- **Provider-specific protocols.** One protocol, OpenAI's chat completions, covers local servers and the big hosted ones alike, so there is one code path to keep right.

## Where you can help

- **Prompts and tools.** Better descriptions of the tools, or new ones, make every model better at the job (`src/`).
- **Testing models.** Tell us which local and hosted models work well, and which don't, with an example.
- **Effects in the text notation**, starting with bends and slides, so the model can write expressive guitar parts (`src/notation.rs`).

Every change needs its tests and a row on the [Features](features.md) page; the [Testing](testing.md) page says how to run them.
