# Architecture and strategy

The plugin is a tab and a settings dialog. The core holds nothing of it: the chat is drawn by the plugin itself, on a surface the plugin interface gives it, and everything the model does goes through the interface's score changes and tool calls.

There is one protocol, OpenAI's chat completions. Presets fill in an address, but each part of it, which is the scheme, host, port, path, query, key header, model and more, is its own saved setting. HTTP uses `ureq` with `rustls`, and the root certificates come from Mozilla.

The model may run tools, and may move the caret or the selection. It may not use File menu entries, so files stay yours.
