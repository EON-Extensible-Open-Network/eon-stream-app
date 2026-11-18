// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 EON contributors
//
// Additional permission under GNU GPL version 3 section 7:
// see LICENSE-EXCEPTION.md (EON Module ABI Exception 1.0).

//! The community client: accounts, shared scores, reviews and friends,
//! against `eon-stream-spec/docs/community-api.md`.
//!
//! **Dormant by default.** Nothing here sends a byte until the viewer enters
//! a server address, and no server run by the project exists: madde 30's
//! Faz L1 gate (a legal entity among its five conditions) is not met, and
//! madde 7 keeps the viewer fully usable without any of this. The viewer's
//! own scores live on the device regardless (`ratings`).
//!
//! The server address and the session token are kept here, in
//! `eon-community.json` beside the executable, and never handed to the
//! interface: it asks by path, and only for the paths the contract lists.

use std::io::Read;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::state::beside_exe;

const FILE: &str = "eon-community.json";
const MAX_BODY: u64 = 2 * 1024 * 1024;

#[derive(Default, Clone, Serialize, Deserialize)]
pub struct Community {
    pub server: Option<String>,
    #[serde(default)]
    token: Option<String>,
    pub username: Option<String>,
}

/// What the interface may know: whether there is a server, and who is
/// signed in. Not the token.
#[derive(Serialize)]
pub struct CommunityView {
    pub server: Option<String>,
    pub username: Option<String>,
}

impl Community {
    pub fn load() -> Self {
        std::fs::read_to_string(beside_exe(FILE))
            .ok()
            .and_then(|json| serde_json::from_str(&json).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) {
        if let Ok(json) = serde_json::to_string_pretty(self) {
            let _ = std::fs::write(beside_exe(FILE), json);
        }
    }

    #[must_use]
    pub fn view(&self) -> CommunityView {
        CommunityView {
            server: self.server.clone(),
            username: self.username.clone(),
        }
    }

    /// Set the server, or clear it with an empty address. A new server means
    /// signing in again.
    ///
    /// # Errors
    ///
    /// The address is not `https://`, or `http://` to this machine.
    pub fn set_server(&mut self, address: &str) -> Result<(), &'static str> {
        let address = address.trim().trim_end_matches('/');
        if address.is_empty() {
            *self = Self::default();
            self.save();
            return Ok(());
        }
        let loopback = ["http://127.0.0.1", "http://localhost", "http://[::1]"]
            .iter()
            .any(|prefix| address.starts_with(prefix));
        if !address.starts_with("https://") && !loopback {
            return Err("ui.community.https");
        }
        self.server = Some(address.to_owned());
        self.token = None;
        self.username = None;
        self.save();
        Ok(())
    }

    pub fn signed_in(&mut self, token: String, username: String) {
        self.token = Some(token);
        self.username = Some(username);
        self.save();
    }

    pub fn signed_out(&mut self) {
        self.token = None;
        self.username = None;
        self.save();
    }
}

/// One request to the community server, prepared under the lock and sent
/// without it.
pub struct Call {
    method: &'static str,
    url: String,
    token: Option<String>,
    body: Option<serde_json::Value>,
}

/// The methods and paths the contract has, and nothing else: the interface
/// cannot use this to reach anything but what `community-api.md` lists.
fn allowed(method: &str, path: &str) -> bool {
    let parts: Vec<&str> = path
        .split('?')
        .next()
        .unwrap_or_default()
        .trim_start_matches('/')
        .split('/')
        .collect();
    let safe = parts.iter().all(|part| {
        !part.is_empty()
            && part
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || "_-:.".contains(c))
    });
    if !safe || parts.first() != Some(&"v0") {
        return false;
    }
    matches!(
        (method, &parts[1..]),
        ("POST", ["accounts"])
            | ("POST" | "DELETE", ["sessions"])
            | ("GET" | "DELETE", ["me"])
            | ("GET", ["me", "export"])
            | ("PUT" | "DELETE", ["ratings", _, _])
            | ("GET", ["titles", _, _])
            | ("GET", ["users"] | ["users", _])
            | ("GET", ["friends"] | ["feed"])
            | ("POST" | "DELETE", ["friends", _])
            | ("POST", ["reports"])
    )
}

/// Prepare a call. `path` is the contract's, `/v0/...`, with its query.
///
/// # Errors
///
/// As a message key: no server set, or a path the contract does not have.
pub fn prepare(
    community: &Community,
    method: &str,
    path: &str,
    body: Option<serde_json::Value>,
) -> Result<Call, &'static str> {
    let server = community.server.clone().ok_or("ui.community.noserver")?;
    let method = match method {
        "GET" => "GET",
        "POST" => "POST",
        "PUT" => "PUT",
        "DELETE" => "DELETE",
        _ => return Err("ui.community.badcall"),
    };
    if !allowed(method, path) {
        return Err("ui.community.badcall");
    }
    Ok(Call {
        method,
        url: format!("{server}{path}"),
        token: community.token.clone(),
        body,
    })
}

/// The answer: the status and the JSON body (null when there was none).
pub struct Answer {
    pub status: u16,
    pub body: serde_json::Value,
}

/// Send a call, with nothing locked.
///
/// # Errors
///
/// A transport failure, as text. A non-2xx status is an answer, not an
/// error: the contract puts a code and a sentence in its body.
pub fn send(call: &Call) -> Result<Answer, String> {
    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(15))
        .redirects(0)
        .user_agent(concat!("EON-Stream/", env!("CARGO_PKG_VERSION")))
        .build();
    let mut request = agent.request(call.method, &call.url);
    if let Some(token) = &call.token {
        request = request.set("Authorization", &format!("Bearer {token}"));
    }
    let result = match &call.body {
        // By hand rather than through ureq's `json` feature: one call does
        // not justify another feature in the tree.
        Some(body) => request
            .set("Content-Type", "application/json")
            .send_string(&body.to_string()),
        None => request.call(),
    };
    let response = match result {
        Ok(response) => response,
        Err(ureq::Error::Status(_, response)) => response,
        Err(ureq::Error::Transport(t)) => {
            return Err(t
                .message()
                .map_or_else(|| t.kind().to_string(), ToOwned::to_owned));
        }
    };
    let status = response.status();
    let mut text = String::new();
    response
        .into_reader()
        .take(MAX_BODY)
        .read_to_string(&mut text)
        .map_err(|e| e.to_string())?;
    let body = if text.trim().is_empty() {
        serde_json::Value::Null
    } else {
        serde_json::from_str(&text).unwrap_or(serde_json::Value::Null)
    };
    Ok(Answer { status, body })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_contract_can_be_called() {
        assert!(allowed("POST", "/v0/sessions"));
        assert!(allowed("GET", "/v0/titles/movie/tt0111161"));
        assert!(allowed("GET", "/v0/users?q=ali"));
        assert!(allowed("PUT", "/v0/ratings/series/tt0903747"));
        assert!(allowed("POST", "/v0/friends/ayse_1"));
        assert!(!allowed("GET", "/v0/admin"));
        assert!(!allowed("DELETE", "/v0/titles/movie/tt1"));
        assert!(!allowed("GET", "/v0/../etc/passwd"));
        assert!(!allowed("GET", "/v1/users"));
        assert!(!allowed("GET", "/v0/users/a/b"));
    }

    #[test]
    fn a_server_must_be_https_or_this_machine() {
        let mut community = Community::default();
        assert!(community.set_server("http://example.org").is_err());
        assert!(community
            .set_server("https://community.example.org/")
            .is_ok());
        assert_eq!(
            community.server.as_deref(),
            Some("https://community.example.org")
        );
        assert!(community.set_server("http://127.0.0.1:8770").is_ok());
    }
}
