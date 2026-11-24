# How to use it

## Set it up

1. Start a model server, or get a key for a hosted one (see [Which model](#which-model) below).
2. Choose **Plugins → AI Settings…**.
3. Pick a preset under **Server**, such as Ollama on this computer or a hosted service, and change what you need: the host, the port, the key and the model.

![The AI Settings dialog with the Ollama (this computer) preset: http, host 127.0.0.1, port 11434, path /v1, and the address requests go to](images/ai-settings.png)

4. Click **OK**.

## Ask

Open the **AI** tab on the right, type in the box at the bottom and click **Send**. Ask about the song, or for a change: "write a bass line for bars 1 to 8", "fit every bar to its time signature", "transpose the whole song down a whole step".

![The AI tab: a short hint about what to ask, the server in use, the question box and the Send button](images/ai-tab.png)

**New Chat** starts over. Everything the model changes in one answer is a single step called "AI: …", so **Edit → Undo** takes it all back.

## What you can do with it

- Ask for music. The model can write bars into a track's voice, add tracks, change time and key signatures, tempo and sections, and set the title.
- Ask about a score. It reads an overview of the song and any bars of it.
- Hand it routine work. It can run OxiGuitar's tools, such as Transpose, Find Fingering, Fit Bars to Time Signature, Harmonize and Infer Key Signatures, and other plugins' tools.

Everything the model changes in one answer is one change, named "AI: …". **Edit → Undo** takes it all back.

## Skills: your own habits

A skill is a note of your own that the model reads when it fits: how you like a blues shuffle written, that you never want the top string in a bass line, a rule such as "answer in Portuguese". You write it once and every chat can use it. It matters most with a model on your computer, which doesn't know your habits and has little room to spare.

Make a Markdown file in the `skills` folder, which sits beside your `plugins` folder:

| System | Your skills folder |
| --- | --- |
| Linux | `~/.local/share/oxiguitar/skills/` |
| Linux, Flatpak | `~/.var/app/io.github.mateusrdp.OxiGuitar/data/oxiguitar/skills/` |
| macOS | `~/Library/Application Support/OxiGuitar/skills/` |
| Windows | `%APPDATA%\OxiGuitar\skills\` |

OxiGuitar makes the folder when it starts with this plugin installed. A skill is either `NAME.md` or a folder `NAME/SKILL.md`. It can begin with a header between two lines of `---`:

```text
---
name: blues-shuffle
description: How I write a 12-bar blues shuffle.
---
Write it in 4/4 with eighth-note swing: e/3 triplets, the first and
third of each group tied. Keep the bass on the low strings, a dominant
seventh on every chord.
```

- `name` is what the model calls it. Without it, the file's name (or the folder's) is used.
- `description` says when it applies, in a line. Without it, the first line of the text is used.
- `always: true` puts the whole text in the model's instructions for every request, for habits that hold everywhere. Leave it out for the rest.

The model is told each skill's name and description and loads the text when your request fits, as the chat shows ("Read the skill ..."). Ask for it by name if you want to be sure: "use my blues-shuffle skill".

Things to know:

- A skill's file may hold up to 64 KiB, and at most 64 skills are used. A file that is empty, isn't text or is too big is left out, and the chat says so.
- If two folders hold a skill with the same name, yours wins over one a package installed.
- Skills are read each time you send a message, so you can edit one while the chat is open.
- A skill is sent to the server with your question, like the rest of the chat. With a server away from this computer, anything you write in a skill goes there once you have allowed that server.

## Which model

Any server that speaks OpenAI's chat completions protocol, with tool calling. On your own computer, that means Ollama, llama.cpp's `llama-server` (start it with `--jinja`) and LM Studio. Hosted, with your own key, it means OpenAI, Anthropic, Google, OpenRouter, or any compatible server. The plugin doesn't start a model for you.

Small local models call tools less reliably and write weaker music than large hosted ones. Pick a model that supports tool calling.

## Your data

A server on your own computer is used at once. Before anything goes to a server anywhere else, the chat asks, naming the server. Only what you type and what the model reads of the score is sent, and only when you send a question. OxiGuitar itself never connects anywhere. This plugin does, as you set it up.

Keys are stored as typed in OxiGuitar's `settings.json`, or read from an environment variable you name instead.

## In the Flatpak

OxiGuitar's Flatpak has no network access, so the plugin can't reach even a server on your own computer. To allow it:

```sh
flatpak override --user --share=network io.github.mateusrdp.OxiGuitar
```

To take it away again, use `--unshare=network`.
