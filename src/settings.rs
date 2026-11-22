// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Mateus Rocha de Paula and the OxiGuitar contributors

//! The plugin's settings, kept by OxiGuitar in settings.json under the
//! plugin's id: where the model's server is (each part of its address on
//! its own, so any of them can be changed), how to sign in, which model,
//! and the servers away from this computer the user allowed.
//!
//! Every server speaks OpenAI's chat completions protocol
//! (`POST {address}/chat/completions`); the presets fill in the address
//! and the sign-in of the usual ones.

use serde::{Deserialize, Serialize};

/// A server the settings can start from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Preset {
    /// Its id in the settings.
    pub id: &'static str,
    /// Its name in the settings dialog.
    pub name: &'static str,
    /// `http` or `https`.
    pub scheme: &'static str,
    /// The server's host name.
    pub host: &'static str,
    /// Its port.
    pub port: u16,
    /// The path the protocol's endpoints follow.
    pub path: &'static str,
    /// Whether it needs a key.
    pub needs_key: bool,
    /// Where its keys come from, for the dialog's hint.
    pub keys_at: &'static str,
}

/// The presets, in the dialog's order; `custom` keeps what the user typed.
pub const PRESETS: &[Preset] = &[
    Preset {
        id: "ollama",
        name: "Ollama (this computer)",
        scheme: "http",
        host: "127.0.0.1",
        port: 11434,
        path: "/v1",
        needs_key: false,
        keys_at: "",
    },
    Preset {
        id: "llama-cpp",
        name: "llama.cpp server (this computer)",
        scheme: "http",
        host: "127.0.0.1",
        port: 8080,
        path: "/v1",
        needs_key: false,
        keys_at: "",
    },
    Preset {
        id: "lm-studio",
        name: "LM Studio (this computer)",
        scheme: "http",
        host: "127.0.0.1",
        port: 1234,
        path: "/v1",
        needs_key: false,
        keys_at: "",
    },
    Preset {
        id: "openai",
        name: "OpenAI (ChatGPT's models)",
        scheme: "https",
        host: "api.openai.com",
        port: 443,
        path: "/v1",
        needs_key: true,
        keys_at: "https://platform.openai.com/api-keys",
    },
    Preset {
        id: "anthropic",
        name: "Anthropic (Claude)",
        scheme: "https",
        host: "api.anthropic.com",
        port: 443,
        path: "/v1",
        needs_key: true,
        keys_at: "https://platform.claude.com/settings/keys",
    },
    Preset {
        id: "gemini",
        name: "Google (Gemini)",
        scheme: "https",
        host: "generativelanguage.googleapis.com",
        port: 443,
        path: "/v1beta/openai",
        needs_key: true,
        keys_at: "https://aistudio.google.com/apikey",
    },
    Preset {
        id: "openrouter",
        name: "OpenRouter",
        scheme: "https",
        host: "openrouter.ai",
        port: 443,
        path: "/api/v1",
        needs_key: true,
        keys_at: "https://openrouter.ai/keys",
    },
    Preset {
        id: "custom",
        name: "Another OpenAI-compatible server",
        scheme: "http",
        host: "127.0.0.1",
        port: 8000,
        path: "/v1",
        needs_key: false,
        keys_at: "",
    },
];

/// The preset `id`, or the first.
pub fn preset(id: &str) -> &'static Preset {
    PRESETS.iter().find(|p| p.id == id).unwrap_or(&PRESETS[0])
}

/// What the plugin keeps. Every field has a default, so settings written
/// by an older version, or edited by hand, still read.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// The preset the user started from.
    pub preset: String,
    /// `http` or `https`.
    pub scheme: String,
    /// The server's host name or address.
    pub host: String,
    /// Its port.
    pub port: u16,
    /// The path before `/chat/completions`, such as `/v1`.
    pub path: String,
    /// A query string added to every request, without `?` (Azure's
    /// `api-version=…`, say); usually empty.
    pub query: String,
    /// The key (a personal access token), kept as typed. Empty for none.
    pub api_key: String,
    /// The name of an environment variable holding the key, read when a
    /// request is made, for those who don't keep keys in settings files;
    /// it wins over `api_key` when it is set.
    pub api_key_env: String,
    /// The header that carries the key.
    pub auth_header: String,
    /// What comes before the key in it.
    pub auth_prefix: String,
    /// More headers, one `Name: value` a line (OpenAI's organisation and
    /// project, say).
    pub extra_headers: String,
    /// The model's name, as the server knows it.
    pub model: String,
    /// The sampling temperature, empty for the server's own.
    pub temperature: String,
    /// The most tokens an answer may have, empty for the server's own.
    pub max_tokens: String,
    /// How long one request may take, in seconds.
    pub timeout: u32,
    /// How many times the model may call OxiGuitar's tools in one answer.
    pub max_steps: u32,
    /// Whether answers come word by word (streaming) rather than whole.
    pub stream: bool,
    /// Instructions added to the plugin's own for the model.
    pub instructions: String,
    /// Servers away from this computer the user allowed the plugin to
    /// send the chat and the score to, as `host:port`.
    pub allowed: Vec<String>,
}

impl Default for Settings {
    fn default() -> Settings {
        let p = &PRESETS[0];
        Settings {
            preset: p.id.to_owned(),
            scheme: p.scheme.to_owned(),
            host: p.host.to_owned(),
            port: p.port,
            path: p.path.to_owned(),
            query: String::new(),
            api_key: String::new(),
            api_key_env: String::new(),
            auth_header: "Authorization".to_owned(),
            auth_prefix: "Bearer ".to_owned(),
            extra_headers: String::new(),
            model: String::new(),
            temperature: String::new(),
            max_tokens: String::new(),
            timeout: 300,
            max_steps: 16,
            stream: true,
            instructions: String::new(),
            allowed: Vec::new(),
        }
    }
}

impl Settings {
    /// The settings as the plugin's settings JSON holds them; the defaults
    /// for what it doesn't (or for JSON that can't be read).
    pub fn from_json(json: &str) -> Settings {
        serde_json_value(json)
            .and_then(|v| serde_json::from_value(v).ok())
            .unwrap_or_default()
    }

    /// The settings as OxiGuitar keeps them for the plugin.
    pub fn from_map(map: serde_json::Map<String, serde_json::Value>) -> Settings {
        serde_json::from_value(serde_json::Value::Object(map)).unwrap_or_default()
    }

    /// As OxiGuitar keeps them.
    pub fn to_map(&self) -> serde_json::Map<String, serde_json::Value> {
        match serde_json::to_value(self) {
            Ok(serde_json::Value::Object(m)) => m,
            _ => serde_json::Map::new(),
        }
    }

    /// The plugin's settings as the host has them now.
    pub fn load(host: &oxiguitar_plugin::Host) -> Settings {
        host.settings().map(Settings::from_map).unwrap_or_default()
    }

    /// As JSON for the plugin's settings.
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_else(|_| "{}".to_owned())
    }

    /// Takes the preset `id`'s address and sign-in, keeping the key, the
    /// model and the rest.
    pub fn apply_preset(&mut self, id: &str) {
        let p = preset(id);
        self.preset = p.id.to_owned();
        if p.id == "custom" {
            return;
        }
        self.scheme = p.scheme.to_owned();
        self.host = p.host.to_owned();
        self.port = p.port;
        self.path = p.path.to_owned();
        self.auth_header = "Authorization".to_owned();
        self.auth_prefix = "Bearer ".to_owned();
    }

    /// The address the protocol's endpoints follow, with `endpoint`
    /// (`chat/completions`, `models`) and the query.
    pub fn url(&self, endpoint: &str) -> Result<String, String> {
        let scheme = self.scheme.trim().to_lowercase();
        if scheme != "http" && scheme != "https" {
            return Err(format!(
                "the server's scheme is http or https, not {:?}",
                self.scheme
            ));
        }
        let host = self.host.trim();
        if host.is_empty() || host.contains(['/', '?', '#', ' ', '@']) {
            return Err(format!("{host:?} isn't a host name"));
        }
        // An IPv6 address goes in brackets.
        let host = if host.contains(':') && !host.starts_with('[') {
            format!("[{host}]")
        } else {
            host.to_owned()
        };
        let path = self.path.trim().trim_matches('/');
        let mut url = format!("{scheme}://{host}:{}", self.port);
        if !path.is_empty() {
            url.push('/');
            url.push_str(path);
        }
        url.push('/');
        url.push_str(endpoint);
        let query = self.query.trim().trim_start_matches('?');
        if !query.is_empty() {
            url.push('?');
            url.push_str(query);
        }
        Ok(url)
    }

    /// `host:port`, as the user allows a server.
    pub fn server(&self) -> String {
        format!("{}:{}", self.host.trim().to_lowercase(), self.port)
    }

    /// Whether the server is on this computer: nothing leaves it.
    pub fn is_local(&self) -> bool {
        let host = self
            .host
            .trim()
            .trim_start_matches('[')
            .trim_end_matches(']')
            .to_lowercase();
        if host == "localhost" || host.ends_with(".localhost") {
            return true;
        }
        host.parse::<std::net::IpAddr>()
            .is_ok_and(|ip| ip.is_loopback())
    }

    /// Whether requests may go to the server: it is on this computer, or
    /// the user allowed it.
    pub fn may_send(&self) -> bool {
        self.is_local() || self.allowed.contains(&self.server())
    }

    /// The key to send: from the environment variable when it is named and
    /// set, otherwise as typed. Empty for none.
    pub fn key(&self) -> String {
        let name = self.api_key_env.trim();
        if !name.is_empty()
            && let Ok(v) = std::env::var(name)
            && !v.trim().is_empty()
        {
            return v.trim().to_owned();
        }
        self.api_key.trim().to_owned()
    }

    /// Every header to send: the key's (when there is one) and the extra
    /// ones; a line that isn't `Name: value` is an error naming it.
    pub fn headers(&self) -> Result<Vec<(String, String)>, String> {
        let mut out = Vec::new();
        let key = self.key();
        if !key.is_empty() {
            let name = self.auth_header.trim();
            if !is_header_name(name) {
                return Err(format!("{name:?} isn't a header's name"));
            }
            out.push((name.to_owned(), format!("{}{key}", self.auth_prefix)));
        }
        for line in self.extra_headers.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let Some((name, value)) = line.split_once(':') else {
                return Err(format!("the header line {line:?} isn't \"Name: value\""));
            };
            let name = name.trim();
            if !is_header_name(name) {
                return Err(format!("{name:?} isn't a header's name"));
            }
            out.push((name.to_owned(), value.trim().to_owned()));
        }
        Ok(out)
    }

    /// The temperature to ask for, if the user set one.
    pub fn temperature(&self) -> Result<Option<f64>, String> {
        let t = self.temperature.trim();
        if t.is_empty() {
            return Ok(None);
        }
        t.parse::<f64>()
            .ok()
            .filter(|v| (0.0..=2.0).contains(v))
            .map(Some)
            .ok_or_else(|| format!("the temperature is 0 to 2, not {t:?}"))
    }

    /// The most tokens to ask for, if the user set it.
    pub fn max_tokens(&self) -> Result<Option<u32>, String> {
        let t = self.max_tokens.trim();
        if t.is_empty() {
            return Ok(None);
        }
        t.parse::<u32>()
            .ok()
            .filter(|v| *v > 0)
            .map(Some)
            .ok_or_else(|| format!("the most tokens is a whole number, not {t:?}"))
    }
}

fn serde_json_value(json: &str) -> Option<serde_json::Value> {
    serde_json::from_str(json).ok()
}

/// A header's name: letters, digits and `-`, as HTTP allows.
fn is_header_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "-_!#$%&'*+.^`|~".contains(c))
}

/// serde_json, as the SDK re-exports it.
use oxiguitar_plugin::serde_json;

#[cfg(test)]
mod tests {
    use super::*;

    /// The key comes from the environment variable the settings name, when it
    /// is set; otherwise, as typed. (PATH stands in for a key's variable: it
    /// is always set, and a test changing the environment would race the
    /// others.)
    #[test]
    fn a_named_environment_variable_gives_the_key() {
        let path = std::env::var("PATH").expect("PATH is set");
        let typed = Settings {
            api_key: " typed ".into(),
            ..Settings::default()
        };
        assert_eq!(typed.key(), "typed");
        let from_env = Settings {
            api_key_env: "PATH".into(),
            ..typed.clone()
        };
        assert_eq!(from_env.key(), path.trim());
        let unset = Settings {
            api_key_env: "OXIGUITAR_AI_NO_SUCH_VARIABLE".into(),
            ..typed.clone()
        };
        assert_eq!(unset.key(), "typed");
    }

    #[test]
    fn a_preset_fills_the_address_and_keeps_the_key_and_model() {
        let mut s = Settings {
            api_key: "sk-1".into(),
            model: "m".into(),
            ..Settings::default()
        };
        s.apply_preset("anthropic");
        assert_eq!(
            s.url("chat/completions").unwrap(),
            "https://api.anthropic.com:443/v1/chat/completions"
        );
        assert_eq!((s.api_key.as_str(), s.model.as_str()), ("sk-1", "m"));
        s.apply_preset("gemini");
        assert_eq!(
            s.url("models").unwrap(),
            "https://generativelanguage.googleapis.com:443/v1beta/openai/models"
        );
        s.port = 8443;
        s.apply_preset("custom");
        assert_eq!(s.port, 8443);
    }

    #[test]
    fn every_part_of_the_address_is_its_own() {
        let s = Settings {
            scheme: "HTTP".into(),
            host: "::1".into(),
            port: 9000,
            path: "/openai/".into(),
            query: "?api-version=2024-10-21".into(),
            ..Settings::default()
        };
        assert_eq!(
            s.url("chat/completions").unwrap(),
            "http://[::1]:9000/openai/chat/completions?api-version=2024-10-21"
        );
        assert!(s.is_local());
        let bad = Settings {
            host: "a/b".into(),
            ..Settings::default()
        };
        assert!(bad.url("models").is_err());
        let ftp = Settings {
            scheme: "ftp".into(),
            ..Settings::default()
        };
        assert!(ftp.url("models").is_err());
    }

    #[test]
    fn only_this_computer_needs_no_permission() {
        let mut s = Settings::default();
        assert!(s.may_send());
        for host in [
            "localhost",
            "127.0.0.1",
            "127.1.2.3",
            "[::1]",
            "x.localhost",
        ] {
            s.host = host.into();
            assert!(s.is_local(), "{host}");
        }
        s.apply_preset("openai");
        assert!(!s.is_local() && !s.may_send());
        s.allowed.push(s.server());
        assert!(s.may_send());
        s.host = "192.168.1.5".into();
        assert!(!s.may_send(), "another computer on the network is away");
    }

    #[test]
    fn headers_carry_the_key_and_the_extra_lines() {
        let s = Settings {
            api_key: " sk-abc ".into(),
            extra_headers: "OpenAI-Organization: org-1\n\nOpenAI-Project: p".into(),
            ..Settings::default()
        };
        assert_eq!(
            s.headers().unwrap(),
            [
                ("Authorization".into(), "Bearer sk-abc".into()),
                ("OpenAI-Organization".into(), "org-1".into()),
                ("OpenAI-Project".into(), "p".into()),
            ]
        );
        let azure = Settings {
            api_key: "k".into(),
            auth_header: "api-key".into(),
            auth_prefix: String::new(),
            ..Settings::default()
        };
        assert_eq!(azure.headers().unwrap(), [("api-key".into(), "k".into())]);
        let bad = Settings {
            extra_headers: "no colon".into(),
            ..Settings::default()
        };
        assert!(bad.headers().is_err());
        assert!(Settings::default().headers().unwrap().is_empty());
    }

    #[test]
    fn settings_read_back_and_old_ones_take_defaults() {
        let s = Settings {
            model: "qwen3".into(),
            allowed: vec!["api.openai.com:443".into()],
            ..Settings::default()
        };
        assert_eq!(Settings::from_json(&s.to_json()), s);
        let old = Settings::from_json(r#"{"model": "x", "unknown": 1}"#);
        assert_eq!(old.model, "x");
        assert_eq!(old.port, 11434);
        assert_eq!(Settings::from_json("not json"), Settings::default());
    }

    #[test]
    fn numbers_are_checked() {
        let mut s = Settings::default();
        assert_eq!(s.temperature().unwrap(), None);
        s.temperature = "0.4".into();
        assert_eq!(s.temperature().unwrap(), Some(0.4));
        s.temperature = "3".into();
        assert!(s.temperature().is_err());
        s.max_tokens = "2048".into();
        assert_eq!(s.max_tokens().unwrap(), Some(2048));
        s.max_tokens = "-1".into();
        assert!(s.max_tokens().is_err());
    }
}
