//! assistant-panel — a streaming Google Gemini chat for the margo shell,
//! written with the margo plugin authoring SDK (`mplugin-sdk`).
//!
//! A scrollable log of markdown bubbles over a text entry. Submitting the entry
//! appends a "you" bubble, opens an empty "ai" bubble, and streams Gemini's
//! reply into it token-by-token (via `http-start` + `stream-chunk`, parsing the
//! `alt=sse` event stream).
//!
//! Settings (declarative `[[setting]]` tier): `api_key` (secret), `model`
//! (choice), and `endpoint` (base URL — overridable to a proxy).

use mplugin_sdk::{export_component, host, Component, El, Event, EventKind};
use std::cell::RefCell;

struct Msg {
    role: &'static str, // "you" | "ai"
    text: String,
}

thread_local! {
    static LOG: RefCell<Vec<Msg>> = const { RefCell::new(Vec::new()) };
    /// Bytes of the in-flight SSE response not yet split into complete lines.
    static SSE_BUF: RefCell<String> = const { RefCell::new(String::new()) };
    /// The full raw response body, kept so a non-SSE error (bad key, quota,
    /// model name) can be surfaced instead of a silent empty reply.
    static RAW: RefCell<String> = const { RefCell::new(String::new()) };
}

const DEFAULT_ENDPOINT: &str = "https://generativelanguage.googleapis.com";

struct Assistant;

fn view_tree() -> El {
    let bubbles = LOG.with(|log| {
        log.borrow()
            .iter()
            .map(|m| El::markdown(format!("**{}:** {}", m.role, m.text)))
            .collect::<Vec<_>>()
    });
    El::vbox(vec![
        El::hbox(vec![El::button("copy", "Copy chat")]),
        El::scroll(bubbles).with_id("log"),
        El::entry("input", ""),
    ])
}

/// The whole conversation as plain text, for the clipboard.
fn conversation_text() -> String {
    LOG.with(|log| {
        log.borrow()
            .iter()
            .map(|m| format!("{}: {}", m.role, m.text))
            .collect::<Vec<_>>()
            .join("\n")
    })
}

/// Build the Gemini request body from the conversation so far (excluding the
/// trailing empty ai bubble we're about to fill).
fn request_body() -> String {
    let contents: Vec<serde_json::Value> = LOG.with(|log| {
        log.borrow()
            .iter()
            .filter(|m| !(m.role == "ai" && m.text.is_empty()))
            .map(|m| {
                let role = if m.role == "you" { "user" } else { "model" };
                serde_json::json!({ "role": role, "parts": [{ "text": m.text }] })
            })
            .collect()
    });
    serde_json::json!({ "contents": contents }).to_string()
}

/// Turn a non-SSE response body into a readable error for the bubble: Gemini's
/// `{"error":{"message":…}}`, else the raw text (so the user sees *why* there
/// was no reply instead of silence).
fn error_from_raw(raw: &str) -> String {
    let raw = raw.trim();
    if raw.is_empty() {
        return "⚠ no response from the API".into();
    }
    if let Ok(json) = serde_json::from_str::<serde_json::Value>(raw) {
        if let Some(msg) = json["error"]["message"].as_str() {
            return format!("⚠ {msg}");
        }
    }
    let snippet: String = raw.chars().take(300).collect();
    format!("⚠ {snippet}")
}

/// Append a completed SSE `data:` payload's text delta to the open ai bubble.
fn consume_sse_line(line: &str) {
    let Some(payload) = line.strip_prefix("data:") else {
        return;
    };
    let payload = payload.trim();
    if payload.is_empty() || payload == "[DONE]" {
        return;
    }
    let Ok(json) = serde_json::from_str::<serde_json::Value>(payload) else {
        return;
    };
    if let Some(text) = json["candidates"][0]["content"]["parts"][0]["text"].as_str() {
        LOG.with(|log| {
            if let Some(last) = log.borrow_mut().last_mut() {
                last.text.push_str(text);
            }
        });
    }
}

impl Component for Assistant {
    fn view() -> El {
        view_tree()
    }

    fn update(ev: Event) -> El {
        match ev.kind {
            // Copy the whole conversation to the clipboard.
            EventKind::Click if ev.id == "copy" => {
                host::copy(&conversation_text());
                host::notify("Assistant", "Chat copied to clipboard");
            }
            EventKind::Submit if ev.id == "input" && !ev.value.trim().is_empty() => {
                LOG.with(|log| {
                    let mut log = log.borrow_mut();
                    log.push(Msg {
                        role: "you",
                        text: ev.value.clone(),
                    });
                    log.push(Msg {
                        role: "ai",
                        text: String::new(),
                    });
                });
                SSE_BUF.with(|b| b.borrow_mut().clear());
                RAW.with(|r| r.borrow_mut().clear());

                let endpoint = {
                    let e = host::get_setting("endpoint");
                    if e.trim().is_empty() {
                        DEFAULT_ENDPOINT.to_string()
                    } else {
                        e
                    }
                };
                let model = {
                    let m = host::get_setting("model");
                    if m.trim().is_empty() {
                        "gemini-2.5-flash".to_string()
                    } else {
                        m
                    }
                };
                let api_key = host::get_setting("api_key");
                let url =
                    format!("{endpoint}/v1beta/models/{model}:streamGenerateContent?alt=sse");
                let _ = host::http_start(&host::HttpRequest {
                    method: "POST".into(),
                    url,
                    headers: vec![
                        ("content-type".into(), "application/json".into()),
                        ("x-goog-api-key".into(), api_key),
                    ],
                    body: request_body(),
                });
            }
            EventKind::StreamChunk => {
                RAW.with(|r| r.borrow_mut().push_str(&ev.value));
                let lines: Vec<String> = SSE_BUF.with(|b| {
                    let mut buf = b.borrow_mut();
                    buf.push_str(&ev.value);
                    let mut complete = Vec::new();
                    while let Some(nl) = buf.find('\n') {
                        let line: String = buf.drain(..=nl).collect();
                        complete.push(line.trim_end().to_string());
                    }
                    complete
                });
                for line in lines {
                    consume_sse_line(&line);
                }
            }
            EventKind::StreamEnd => {
                let tail = SSE_BUF.with(|b| std::mem::take(&mut *b.borrow_mut()));
                if !tail.is_empty() {
                    consume_sse_line(tail.trim_end());
                }
                // No text parsed (e.g. a non-SSE error body) → surface why,
                // rather than leaving a silent empty bubble.
                let empty = LOG.with(|log| {
                    log.borrow()
                        .last()
                        .map(|m| m.role == "ai" && m.text.is_empty())
                        .unwrap_or(false)
                });
                if empty {
                    let msg = RAW.with(|r| error_from_raw(&r.borrow()));
                    LOG.with(|log| {
                        if let Some(last) = log.borrow_mut().last_mut() {
                            last.text = msg;
                        }
                    });
                }
            }
            _ => {}
        }
        view_tree()
    }
}

export_component!(Assistant);
