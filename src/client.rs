// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Mateus Rocha de Paula and the OxiGuitar contributors

//! The model's server, spoken to in OpenAI's chat completions protocol:
//! `POST {address}/chat/completions` with the conversation and the tools
//! the model may call, answered whole or as server-sent events (streaming),
//! and `GET {address}/models` for the models it has. llama.cpp's server,
//! Ollama, LM Studio, OpenAI, Anthropic's and Google's compatible endpoints
//! and OpenRouter all speak it.
//!
//! The HTTP is ureq's; what crosses is plain JSON (serde_json), parsed here
//! so it is unit-tested without a server.

use std::io::BufRead;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use oxiguitar_plugin::serde_json::{self, Value, json};

use crate::settings::Settings;

/// A call the model made to one of the tools it was given.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ToolCall {
    /// Its id, which the answer names.
    pub id: String,
    /// The tool's name.
    pub name: String,
    /// Its arguments, JSON text as the model wrote them.
    pub arguments: String,
}

/// The model's answer to one request.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Completion {
    /// What it said.
    pub content: String,
    /// The tools it called, in order.
    pub calls: Vec<ToolCall>,
}

impl Completion {
    /// The answer as the conversation keeps it: an assistant message.
    pub fn message(&self) -> Value {
        // Some servers refuse empty text beside tool calls: none then.
        let content = if self.content.is_empty() && !self.calls.is_empty() {
            Value::Null
        } else {
            json!(self.content)
        };
        let mut m = json!({"role": "assistant", "content": content});
        if !self.calls.is_empty() {
            m["tool_calls"] = Value::Array(
                self.calls
                    .iter()
                    .map(|c| {
                        json!({
                            "id": c.id,
                            "type": "function",
                            "function": {"name": c.name, "arguments": c.arguments},
                        })
                    })
                    .collect(),
            );
        }
        m
    }
}

/// The server, as the settings describe it.
#[derive(Clone, Debug)]
pub struct Client {
    settings: Settings,
}

impl Client {
    /// A client for the server in `settings`.
    pub fn new(settings: Settings) -> Client {
        Client { settings }
    }

    fn agent(&self) -> ureq::Agent {
        ureq::Agent::config_builder()
            // The server's errors are read, not turned into ureq's.
            .http_status_as_error(false)
            .timeout_connect(Some(Duration::from_secs(20)))
            .timeout_global(Some(Duration::from_secs(u64::from(
                self.settings.timeout.max(10),
            ))))
            .user_agent(concat!("oxiguitar-ai/", env!("CARGO_PKG_VERSION")))
            .build()
            .into()
    }

    fn request(&self, endpoint: &str) -> Result<RequestParts, String> {
        if !self.settings.may_send() {
            return Err(format!(
                "{} isn't on this computer and wasn't allowed",
                self.settings.server()
            ));
        }
        Ok(RequestParts {
            url: self.settings.url(endpoint)?,
            headers: self.settings.headers()?,
        })
    }

    /// The models the server has, by name, sorted.
    pub fn models(&self) -> Result<Vec<String>, String> {
        let parts = self.request("models")?;
        let mut get = self.agent().get(&parts.url);
        for (k, v) in &parts.headers {
            get = get.header(k, v);
        }
        let mut response = get.call().map_err(|e| network(&parts.url, &e))?;
        let status = response.status().as_u16();
        let text = response
            .body_mut()
            .read_to_string()
            .map_err(|e| network(&parts.url, &e))?;
        if status >= 400 {
            return Err(server_error(status, &text));
        }
        parse_models(&text)
    }

    /// Asks the model to go on with `body` (a chat completions request
    /// without `model` and `stream`, which come from the settings). While
    /// it streams, `delta` hears each piece of text as it comes. `cancel`
    /// stops reading.
    pub fn complete(
        &self,
        mut body: Value,
        cancel: &AtomicBool,
        delta: &mut dyn FnMut(&str),
    ) -> Result<Completion, String> {
        let s = &self.settings;
        if s.model.trim().is_empty() {
            return Err("no model is chosen: pick one in AI Settings".to_owned());
        }
        body["model"] = json!(s.model.trim());
        body["stream"] = json!(s.stream);
        if let Some(t) = s.temperature()? {
            body["temperature"] = json!(t);
        }
        if let Some(n) = s.max_tokens()? {
            body["max_tokens"] = json!(n);
        }
        let parts = self.request("chat/completions")?;
        let mut post = self
            .agent()
            .post(&parts.url)
            .header("Content-Type", "application/json");
        if s.stream {
            post = post.header("Accept", "text/event-stream");
        }
        for (k, v) in &parts.headers {
            post = post.header(k, v);
        }
        let mut response = post
            .send(body.to_string())
            .map_err(|e| network(&parts.url, &e))?;
        let status = response.status().as_u16();
        if status >= 400 {
            let text = response.body_mut().read_to_string().unwrap_or_default();
            return Err(server_error(status, &text));
        }
        let streamed = response
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .is_some_and(|v| v.contains("event-stream"));
        if !streamed {
            let text = response
                .body_mut()
                .read_to_string()
                .map_err(|e| network(&parts.url, &e))?;
            let completion = parse_whole(&text)?;
            delta(&completion.content);
            return Ok(completion);
        }
        let reader = std::io::BufReader::new(response.into_body().into_reader());
        let mut events = Events::default();
        for line in reader.lines() {
            if cancel.load(Ordering::Relaxed) {
                return Err("stopped".to_owned());
            }
            let line = line.map_err(|e| format!("the answer broke off: {e}"))?;
            match events.feed(&line)? {
                Fed::Text(t) => delta(&t),
                Fed::Done => break,
                Fed::Nothing => {}
            }
        }
        Ok(events.finish())
    }
}

/// A request's address and headers, checked.
struct RequestParts {
    url: String,
    headers: Vec<(String, String)>,
}

fn network(url: &str, e: &ureq::Error) -> String {
    format!("couldn't reach {url}: {e}")
}

/// A sentence from the server's error answer: OpenAI's
/// `{"error": {"message"}}` (most servers use it), or the text.
pub fn server_error(status: u16, text: &str) -> String {
    let message = serde_json::from_str::<Value>(text)
        .ok()
        .and_then(|v| {
            let e = v.get("error")?;
            e.get("message")
                .and_then(Value::as_str)
                .or_else(|| e.as_str())
                .map(str::to_owned)
        })
        .unwrap_or_else(|| text.trim().chars().take(300).collect());
    let hint = match status {
        401 | 403 => " (check the key in AI Settings)",
        404 => " (check the address and the model's name in AI Settings)",
        429 => " (too many requests, or the account's credit ran out)",
        _ => "",
    };
    format!("the server answered {status}: {message}{hint}")
}

/// The models' names in a `/models` answer.
pub fn parse_models(text: &str) -> Result<Vec<String>, String> {
    let v: Value =
        serde_json::from_str(text).map_err(|e| format!("the model list can't be read: {e}"))?;
    let list = v
        .get("data")
        .or_else(|| v.get("models"))
        .and_then(Value::as_array)
        .ok_or("the model list can't be read: it has no data")?;
    let mut names: Vec<String> = list
        .iter()
        .filter_map(|m| {
            m.get("id")
                .or_else(|| m.get("name"))
                .and_then(Value::as_str)
                .map(|s| s.trim_start_matches("models/").to_owned())
        })
        .collect();
    names.sort();
    names.dedup();
    Ok(names)
}

/// A whole (not streamed) answer.
pub fn parse_whole(text: &str) -> Result<Completion, String> {
    let v: Value =
        serde_json::from_str(text).map_err(|e| format!("the answer can't be read: {e}"))?;
    let message = v
        .pointer("/choices/0/message")
        .ok_or("the answer has no message")?;
    let content = message
        .get("content")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();
    let calls = message
        .get("tool_calls")
        .and_then(Value::as_array)
        .map(|calls| {
            calls
                .iter()
                .enumerate()
                .map(|(i, c)| ToolCall {
                    id: c
                        .get("id")
                        .and_then(Value::as_str)
                        .map_or_else(|| format!("call-{i}"), str::to_owned),
                    name: c
                        .pointer("/function/name")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_owned(),
                    arguments: arguments(c.pointer("/function/arguments")),
                })
                .collect()
        })
        .unwrap_or_default();
    Ok(Completion { content, calls })
}

/// Arguments as text: most servers send a JSON string, a few the object.
fn arguments(v: Option<&Value>) -> String {
    match v {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Null) | None => String::new(),
        Some(other) => other.to_string(),
    }
}

/// What one line of the event stream brought.
#[derive(Debug, PartialEq, Eq)]
pub enum Fed {
    /// Text to show.
    Text(String),
    /// The end.
    Done,
    /// Nothing to show (a part of a tool call, a comment, a blank line).
    Nothing,
}

/// The server-sent events of a streamed answer, put together: text as it
/// comes, tool calls by their index.
#[derive(Debug, Default)]
pub struct Events {
    content: String,
    calls: Vec<ToolCall>,
}

impl Events {
    /// One line of the stream.
    pub fn feed(&mut self, line: &str) -> Result<Fed, String> {
        let Some(data) = line.strip_prefix("data:") else {
            return Ok(Fed::Nothing);
        };
        let data = data.trim();
        if data == "[DONE]" {
            return Ok(Fed::Done);
        }
        if data.is_empty() {
            return Ok(Fed::Nothing);
        }
        let v: Value = serde_json::from_str(data)
            .map_err(|e| format!("a piece of the answer can't be read: {e}"))?;
        if let Some(e) = v.get("error") {
            let message = e
                .get("message")
                .and_then(Value::as_str)
                .map_or_else(|| e.to_string(), str::to_owned);
            return Err(format!("the server stopped: {message}"));
        }
        let Some(delta) = v.pointer("/choices/0/delta") else {
            return Ok(Fed::Nothing);
        };
        if let Some(calls) = delta.get("tool_calls").and_then(Value::as_array) {
            for (n, c) in calls.iter().enumerate() {
                let index = c
                    .get("index")
                    .and_then(Value::as_u64)
                    .map_or(self.calls.len().saturating_sub(1).max(n), |i| i as usize);
                while self.calls.len() <= index {
                    self.calls.push(ToolCall::default());
                }
                let call = &mut self.calls[index];
                if let Some(id) = c.get("id").and_then(Value::as_str) {
                    call.id = id.to_owned();
                }
                if let Some(name) = c.pointer("/function/name").and_then(Value::as_str) {
                    call.name.push_str(name);
                }
                match c.pointer("/function/arguments") {
                    Some(Value::String(s)) => call.arguments.push_str(s),
                    Some(Value::Null) | None => {}
                    Some(other) => call.arguments.push_str(&other.to_string()),
                }
            }
        }
        match delta.get("content").and_then(Value::as_str) {
            Some(text) if !text.is_empty() => {
                self.content.push_str(text);
                Ok(Fed::Text(text.to_owned()))
            }
            _ => Ok(Fed::Nothing),
        }
    }

    /// The answer as it came.
    pub fn finish(self) -> Completion {
        let calls = self
            .calls
            .into_iter()
            .enumerate()
            .filter(|(_, c)| !c.name.is_empty())
            .map(|(i, mut c)| {
                if c.id.is_empty() {
                    c.id = format!("call-{i}");
                }
                c
            })
            .collect();
        Completion {
            content: self.content,
            calls,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_stream_brings_text_and_tool_calls_in_pieces() {
        let mut e = Events::default();
        let lines = [
            ": keep-alive",
            r#"data: {"choices":[{"delta":{"role":"assistant","content":"Hel"}}]}"#,
            "",
            r#"data: {"choices":[{"delta":{"content":"lo"}}]}"#,
            r#"data: {"choices":[{"delta":{"tool_calls":[{"index":0,"id":"c1","type":"function","function":{"name":"read_bars","arguments":"{\"tra"}}]}}]}"#,
            r#"data: {"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":"ck\": 1}"}}]}}]}"#,
            r#"data: {"choices":[{"delta":{"tool_calls":[{"index":1,"id":"c2","function":{"name":"overview","arguments":"{}"}}]}}]}"#,
            r#"data: {"choices":[{"delta":{},"finish_reason":"tool_calls"}]}"#,
            "data: [DONE]",
        ];
        let fed: Vec<Fed> = lines.iter().map(|l| e.feed(l).unwrap()).collect();
        assert_eq!(fed[1], Fed::Text("Hel".into()));
        assert_eq!(fed[3], Fed::Text("lo".into()));
        assert_eq!(fed[8], Fed::Done);
        let c = e.finish();
        assert_eq!(c.content, "Hello");
        assert_eq!(
            c.calls,
            [
                ToolCall {
                    id: "c1".into(),
                    name: "read_bars".into(),
                    arguments: r#"{"track": 1}"#.into()
                },
                ToolCall {
                    id: "c2".into(),
                    name: "overview".into(),
                    arguments: "{}".into()
                }
            ]
        );
        assert_eq!(c.message()["tool_calls"][1]["function"]["name"], "overview");
    }

    #[test]
    fn a_stream_without_indexes_or_with_an_error_is_understood() {
        let mut e = Events::default();
        e.feed(r#"data: {"choices":[{"delta":{"tool_calls":[{"id":"g","function":{"name":"overview","arguments":{}}}]}}]}"#)
            .unwrap();
        let c = e.finish();
        assert_eq!(c.calls[0].arguments, "{}");
        let mut e = Events::default();
        assert!(
            e.feed(r#"data: {"error":{"message":"overloaded"}}"#)
                .unwrap_err()
                .contains("overloaded")
        );
    }

    #[test]
    fn a_whole_answer_and_the_model_list_are_read() {
        let c = parse_whole(
            r#"{"choices":[{"message":{"role":"assistant","content":null,
                "tool_calls":[{"id":"x","type":"function","function":{"name":"overview","arguments":"{}"}}]}}]}"#,
        )
        .unwrap();
        assert_eq!(c.content, "");
        assert_eq!(c.calls[0].name, "overview");
        assert_eq!(
            parse_models(r#"{"object":"list","data":[{"id":"b"},{"id":"models/a"},{"id":"b"}]}"#)
                .unwrap(),
            ["a", "b"]
        );
        assert!(parse_models("{}").is_err());
    }

    #[test]
    fn server_errors_are_sentences_with_a_hint() {
        let e = server_error(401, r#"{"error":{"message":"Invalid API key"}}"#);
        assert_eq!(
            e,
            "the server answered 401: Invalid API key (check the key in AI Settings)"
        );
        assert!(server_error(500, "boom").ends_with("boom"));
    }
}
