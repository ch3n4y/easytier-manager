use std::fs;

use regex::Regex;
use serde::{Deserialize, Serialize};

use crate::paths::{default_conf_path, default_config, service_env_path};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigPayload {
    pub mode: String,
    pub config_server: String,
    pub default_conf: String,
}

/// Read the managed config files. Missing files are not an error: a fresh
/// install simply reports the built-in defaults.
pub fn read_config_files() -> ConfigPayload {
    let mut payload = ConfigPayload {
        mode: String::from("config"),
        config_server: String::new(),
        default_conf: default_config(),
    };

    if let Ok(data) = fs::read_to_string(default_conf_path()) {
        payload.default_conf = data;
    }

    if let Ok(data) = fs::read_to_string(service_env_path()) {
        let server = parse_config_server(&data);
        if !server.is_empty() {
            payload.config_server = server;
            payload.mode = String::from("web");
        }
    }

    payload
}

/// Pull `EASYTIER_CONFIG_SERVER` out of a shell `service.env` file, handling
/// both single- and double-quoted values.
pub fn parse_config_server(content: &str) -> String {
    let pattern = Regex::new(r"(?m)^\s*EASYTIER_CONFIG_SERVER\s*=\s*(.*)\s*$").unwrap();
    let Some(captures) = pattern.captures(content) else {
        return String::new();
    };
    let Some(value) = captures.get(1) else {
        return String::new();
    };

    let value = value.as_str().trim();
    if value.starts_with('"') && value.ends_with('"') {
        return value.trim_matches('"').to_string();
    }
    if value.starts_with('\'') && value.ends_with('\'') {
        let inner = value.strip_prefix('\'').unwrap_or_default();
        let inner = inner.strip_suffix('\'').unwrap_or(inner);
        return inner.replace("'\\''", "'");
    }
    value.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_quoted_config_server() {
        let cases = [
            (r#"EASYTIER_CONFIG_SERVER="""#, ""),
            (
                r#"EASYTIER_CONFIG_SERVER="udp://config.example.com:22020/admin""#,
                "udp://config.example.com:22020/admin",
            ),
            (
                "EASYTIER_CONFIG_SERVER='tcp://127.0.0.1:22020/admin'",
                "tcp://127.0.0.1:22020/admin",
            ),
            (
                "OTHER=1\nEASYTIER_CONFIG_SERVER='udp://host/user'\nNEXT=2",
                "udp://host/user",
            ),
        ];

        for (input, want) in cases {
            assert_eq!(parse_config_server(input), want, "input: {input}");
        }
    }

    #[test]
    fn unescapes_single_quotes() {
        assert_eq!(
            parse_config_server(r#"EASYTIER_CONFIG_SERVER='udp://host/a'\''b'"#),
            "udp://host/a'b"
        );
    }
}
