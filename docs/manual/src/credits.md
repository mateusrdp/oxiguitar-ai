# Credits

Thank you to the people whose work this plugin is built on.

## Code we use

| Project | Authors | What it does here | Licence |
| --- | --- | --- | --- |
| [OxiGuitar's plugin SDK](https://github.com/mateusrdp/oxiguitar) (`oxiguitar-plugin`, and `oxiguitar-host` and `oxiguitar-plugin-test` for the tests) | Mateus Rocha de Paula and the OxiGuitar contributors | The plugin interface, and OxiGuitar's own host to test against | MIT OR Apache-2.0 |
| [ureq](https://github.com/algesten/ureq) | Martin Algesten, Jacob Hoffman-Andrews and contributors | HTTP requests to the model server | MIT OR Apache-2.0 |
| [Rustls](https://github.com/rustls/rustls) | The Rustls contributors | Encrypted connections (HTTPS) | Apache-2.0 OR ISC OR MIT |
| [ring](https://github.com/briansmith/ring) | Brian Smith and contributors | The cryptography under Rustls | Apache-2.0 AND ISC |
| [webpki-roots](https://github.com/rustls/webpki-roots) | The Rustls contributors, from Mozilla's list | The trusted root certificates | CDLA-Permissive-2.0 |
| [serde](https://serde.rs) and [serde_json](https://github.com/serde-rs/json) | Erick Tryzelaar, David Tolnay and contributors | Reading and writing the protocol's JSON | MIT OR Apache-2.0 |
| [Rust](https://www.rust-lang.org/) and its standard library | The Rust Project | Everything else | MIT OR Apache-2.0 |

## Standards and services

The protocol is OpenAI's [chat completions](https://platform.openai.com/docs/api-reference/chat) format with tool calling, as implemented by many servers: [Ollama](https://ollama.com), [llama.cpp](https://github.com/ggml-org/llama.cpp), [LM Studio](https://lmstudio.ai) and the hosted services. The plugin works with them; it has nothing to do with the companies behind them.

## Tools

| Tool | Use |
| --- | --- |
| [mdBook](https://github.com/rust-lang/mdBook) | Builds this manual |
| [cargo-deny](https://github.com/EmbarkStudios/cargo-deny) | Checks the licences of everything we build with |

## Thanks

To the people who build open model servers that anyone can run on their own computer, and to everyone who tries this plugin and tells us what worked.
