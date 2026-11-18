// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 EON contributors
//
// Additional permission under GNU GPL version 3 section 7:
// see LICENSE-EXCEPTION.md (EON Module ABI Exception 1.0).

//! Search results in pages, the way a search engine gives them.
//!
//! A search used to ask every searchable catalogue once and show what came
//! back -- the first page of each, and nothing after. Now every catalogue
//! keeps a cursor, and a page of forty-two is filled by asking each one for
//! its next page only when the page being opened needs it. Page three is not
//! fetched until someone opens page three. An addon that ignores `skip` and
//! sends its first page again is noticed -- nothing new came back -- and
//! counted as finished, rather than repeating forever.

use std::collections::HashSet;

use eon_stream_core::MetaPreview;
use serde::Serialize;

use crate::state::Session;
use crate::view::{ItemView, Net};

/// Titles per page.
pub const PAGE: usize = 42;

/// One searchable catalogue, and how far into it the search has read.
#[derive(Clone)]
struct Cursor {
    addon_id: String,
    content_type: String,
    catalog_id: String,
    skip: usize,
    done: bool,
}

/// Everything read so far for one query.
#[derive(Clone, Default)]
pub struct Search {
    query: String,
    corrected_from: Option<String>,
    cursors: Vec<Cursor>,
    found: Vec<MetaPreview>,
    seen: HashSet<(String, String)>,
    failures: Vec<String>,
}

/// One page, as the interface draws it.
#[derive(Serialize)]
pub struct PageView {
    pub query: String,
    pub corrected_from: Option<String>,
    pub page: usize,
    pub items: Vec<ItemView>,
    /// Whether there is a page after this one.
    pub more: bool,
    /// How many pages are known to exist so far -- the pager shows these.
    pub known_pages: usize,
    pub failures: Vec<String>,
}

/// What a page needs, copied out of the session.
pub struct Request {
    net: Net,
    search: Search,
    page: usize,
    vocabulary: Vec<String>,
}

/// Step one: the search so far, or a new one for a new query.
///
/// # Errors
///
/// As text: an empty query, or no addon to ask.
pub fn request(session: &Session, query: &str, page: usize) -> Result<Request, String> {
    let query = query.trim();
    if query.is_empty() {
        return Err(session.text("ui.error.emptyquery"));
    }
    if session.registry.enabled().count() == 0 {
        return Err(session.text("ui.error.noaddons"));
    }
    let search = match &session.search {
        Some(kept) if kept.query == query || kept.corrected_from.as_deref() == Some(query) => {
            kept.clone()
        }
        _ => fresh(session, query),
    };
    let mut vocabulary: Vec<String> = session
        .library
        .names()
        .into_iter()
        .map(ToOwned::to_owned)
        .collect();
    vocabulary.extend(session.history.names().into_iter().map(ToOwned::to_owned));
    Ok(Request {
        net: Net::of(session),
        search,
        page,
        vocabulary,
    })
}

fn fresh(session: &Session, query: &str) -> Search {
    let mut cursors = Vec::new();
    for addon in session.registry.enabled() {
        for catalog in addon.manifest.catalogs.iter().filter(|c| c.is_searchable()) {
            cursors.push(Cursor {
                addon_id: addon.id().to_owned(),
                content_type: catalog.content_type.clone(),
                catalog_id: catalog.id.clone(),
                skip: 0,
                done: false,
            });
        }
    }
    Search {
        query: query.to_owned(),
        cursors,
        ..Search::default()
    }
}

/// Step two, with nothing locked: read on until the page can be filled --
/// and one title past it, to know whether there is a next page.
#[must_use]
pub fn fetch(request: &Request) -> Search {
    let net = &request.net;
    let mut search = request.search.clone();
    let page = request.page;
    let vocabulary = &request.vocabulary;
    let wanted = (page + 1) * PAGE + 1;
    read_until(net, &mut search, wanted);

    // A query that finds one title or none on its first page is worth
    // checking for a misspelling, as the search box always has.
    if page == 0 && search.found.len() < 2 && search.corrected_from.is_none() {
        let candidates: Vec<&str> = vocabulary.iter().map(String::as_str).collect();
        if let Some(suggestion) = eon_stream_core::suggest(&search.query, &candidates)
            .filter(eon_stream_core::Suggestion::is_meaningful)
        {
            let mut retry = Search {
                query: suggestion.corrected.clone(),
                corrected_from: Some(search.query.clone()),
                cursors: search
                    .cursors
                    .iter()
                    .map(|c| Cursor {
                        skip: 0,
                        done: false,
                        ..c.clone()
                    })
                    .collect(),
                ..Search::default()
            };
            read_until(net, &mut retry, wanted);
            if retry.found.len() > search.found.len() {
                search = retry;
            }
        }
    }
    search
}

fn read_until(net: &Net, search: &mut Search, wanted: usize) {
    while search.found.len() < wanted && search.cursors.iter().any(|c| !c.done) {
        for at in 0..search.cursors.len() {
            if search.found.len() >= wanted {
                break;
            }
            let cursor = search.cursors[at].clone();
            if cursor.done {
                continue;
            }
            let Some(addon) = net.registry.get(&cursor.addon_id) else {
                search.cursors[at].done = true;
                continue;
            };
            let skip = cursor.skip.to_string();
            let mut extra: Vec<(&str, &str)> = vec![("search", search.query.as_str())];
            if cursor.skip > 0 {
                extra.push(("skip", skip.as_str()));
            }
            match net
                .client
                .catalog(addon, &cursor.content_type, &cursor.catalog_id, &extra)
            {
                Ok(page) => {
                    let count = page.len();
                    let mut fresh = 0;
                    for item in page {
                        if search
                            .seen
                            .insert((item.content_type.clone(), item.id.clone()))
                        {
                            search.found.push(item);
                            fresh += 1;
                        }
                    }
                    search.cursors[at].skip += count;
                    // Nothing new -- an empty page, or the same page again
                    // from an addon that ignores `skip` -- is the end.
                    if fresh == 0 {
                        search.cursors[at].done = true;
                    }
                }
                Err(error) => {
                    search
                        .failures
                        .push(format!("{}: {error}", addon.summary().name));
                    search.cursors[at].done = true;
                }
            }
        }
    }
}

/// Step three: keep what was read, and draw the page.
pub fn view(session: &mut Session, mut search: Search, page: usize) -> PageView {
    // A child's profile sees what a catalogue files under family or
    // animation -- and a title that says nothing of its genres is not shown.
    if session.profiles.kids() {
        search
            .found
            .retain(|p| crate::profiles::for_kids(&p.genres));
    }
    let start = page * PAGE;
    let slice: Vec<MetaPreview> = search
        .found
        .iter()
        .skip(start)
        .take(PAGE)
        .cloned()
        .collect();
    let items = slice
        .iter()
        .map(|preview| crate::view::item_from_preview(preview, &session.library, &session.history))
        .collect();
    crate::view::remember(session, slice);
    let more = search.found.len() > start + PAGE || search.cursors.iter().any(|c| !c.done);
    let known_pages = search.found.len().div_ceil(PAGE).max(page + 1)
        + usize::from(more && search.found.len() <= start + PAGE);
    let out = PageView {
        query: search.query.clone(),
        corrected_from: search.corrected_from.clone(),
        page,
        items,
        more,
        known_pages,
        failures: search.failures.clone(),
    };
    session.search = Some(search);
    out
}
