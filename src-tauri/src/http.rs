// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 EON contributors
//
// Additional permission under GNU GPL version 3 section 7:
// see LICENSE-EXCEPTION.md (EON Module ABI Exception 1.0).

//! The transport.
//!
//! `eon-stream-core` pulls in no network stack — it takes an [`HttpClient`] and
//! lets the binary choose one. This is that choice, and it is the same one the
//! headless build makes, with the same reasoning: `ureq` is blocking, small,
//! and already in the tree.
//!
//! It is a copy of `cli/src/main.rs`'s implementation rather than a shared
//! module. The two differ in one line — the user-agent product — and the
//! alternative was a third crate holding sixty lines, which costs more to read
//! than it saves. If a third binary ever needs it, that is the moment to move
//! it.
//!
//! ## The URL never reaches a message
//!
//! An addon's configured address can carry a credential in its path (madde 22),
//! so a transport error reports what went wrong and never what it was asked
//! for. That rule holds here with more force than in the headless build,
//! because an error from here is rendered in a window by JavaScript that must
//! never learn the address.

use std::{io::Read, time::Duration};

use eon_stream_core::http::{HttpClient, HttpError, HttpResponse, Limits};

/// A real HTTP client.
pub struct Http {
    agent: ureq::Agent,
    limits: Limits,
}

impl Http {
    /// Build one with the library's own limits.
    pub fn new() -> Self {
        let limits = Limits::default();
        let agent = ureq::AgentBuilder::new()
            .timeout(Duration::from_millis(limits.timeout_ms))
            .redirects(u32::from(limits.max_redirects))
            .user_agent(concat!(
                "EON-Stream/",
                env!("CARGO_PKG_VERSION"),
                " (+https://github.com/EON-Extensible-Open-Network)"
            ))
            .build();
        Self { agent, limits }
    }
}

impl HttpClient for Http {
    fn get(&self, url: &str) -> Result<HttpResponse, HttpError> {
        let response = match self.agent.get(url).call() {
            Ok(response) => response,
            // A non-success status is a response, not a transport failure: the
            // caller needs the status the addon actually sent.
            Err(ureq::Error::Status(status, response)) => {
                let mut body = Vec::new();
                let _ = response.into_reader().read_to_end(&mut body);
                return Ok(HttpResponse { status, body });
            }
            Err(ureq::Error::Transport(t)) => {
                return Err(HttpError::new(
                    t.message()
                        .map_or_else(|| t.kind().to_string(), ToOwned::to_owned),
                ));
            }
        };
        let status = response.status();
        let mut body = Vec::new();
        let cap = self.limits.max_body_bytes as u64 + 1;
        response
            .into_reader()
            .take(cap)
            .read_to_end(&mut body)
            .map_err(|e| HttpError::new(e.to_string()))?;
        Ok(HttpResponse { status, body })
    }

    fn limits(&self) -> Limits {
        self.limits
    }
}
