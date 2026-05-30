//! assistant-panel — a streaming multi-provider AI chat for the margo shell,
//! written with the margo plugin authoring SDK (`mplugin-sdk`).
//!
//! Inspired by Dank Material Shell's `dms-ai-assistant`: a chat surface backed
//! by Gemini, any OpenAI-compatible API (OpenAI, LocalAI, LM Studio, vLLM,
//! Inception, …), Anthropic, or a local Ollama. Streaming over `http-start`
//! with provider-specific SSE / NDJSON parsing; persistent history (cleared
//! on provider change); header actions for Stop / Retry / Copy / New.
//!
//! Settings (declarative `[[setting]]` tier):
//!   provider · model · api_key (secret) · endpoint · temperature · max_tokens
//!   · system_prompt · persist_history.

use mplugin_sdk::{export_component, host, Component, El, Event, EventKind};
use std::cell::RefCell;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Role {
    You,
    Ai,
}

#[derive(Clone)]
struct Msg {
    role: Role,
    text: String,
}

thread_local! {
    static LOG: RefCell<Vec<Msg>> = const { RefCell::new(Vec::new()) };
    /// Streaming line-assembly buffer (works for both SSE and NDJSON).
    static SSE_BUF: RefCell<String> = const { RefCell::new(String::new()) };
    /// The full raw response body, kept so a non-streaming error (bad key,
    /// quota, model name) can be surfaced instead of a silent empty reply.
    static RAW: RefCell<String> = const { RefCell::new(String::new()) };
    /// True while an `http-start` request is in flight.
    static STREAMING: RefCell<bool> = const { RefCell::new(false) };
    /// True if the user clicked Stop — incoming chunks get dropped (the host
    /// has no cancel verb yet, so the bytes still arrive but we ignore them).
    static STOPPED: RefCell<bool> = const { RefCell::new(false) };
    /// Provider the in-flight stream is for. Lets us parse different formats.
    static ACTIVE_PROVIDER: RefCell<String> = const { RefCell::new(String::new()) };
    /// Provider the existing log was sent to. Switching providers clears it.
    static LOG_PROVIDER: RefCell<String> = const { RefCell::new(String::new()) };
    /// Inline error shown above the input on the last failure.
    static ERROR: RefCell<String> = const { RefCell::new(String::new()) };
    /// Have we tried to restore the on-disk session yet?
    static LOADED: RefCell<bool> = const { RefCell::new(false) };
}

const SESSION_FILE: &str = "session.json";
const MAX_MSGS: usize = 80;

// ── Settings ────────────────────────────────────────────────────────────────

fn provider_key() -> &'static str {
    match host::get_setting("provider").to_lowercase().as_str() {
        "openai" | "openai-compatible" => "openai",
        "anthropic" | "claude" => "anthropic",
        "ollama" => "ollama",
        "custom" => "custom",
        _ => "gemini",
    }
}

fn provider_label() -> &'static str {
    match provider_key() {
        "openai" => "OpenAI",
        "anthropic" => "Anthropic",
        "ollama" => "Ollama",
        "custom" => "Custom",
        _ => "Gemini",
    }
}

fn model() -> String {
    let m = host::get_setting("model");
    if !m.trim().is_empty() {
        return m;
    }
    match provider_key() {
        "openai" => "gpt-4o-mini".into(),
        "anthropic" => "claude-sonnet-4-5-20250929".into(),
        "ollama" => "llama3".into(),
        "custom" => String::new(),
        _ => "gemini-2.5-flash".into(),
    }
}

fn endpoint() -> String {
    let e = host::get_setting("endpoint");
    let e = e.trim().trim_end_matches('/');
    if !e.is_empty() {
        return e.to_string();
    }
    match provider_key() {
        "openai" => "https://api.openai.com".into(),
        "anthropic" => "https://api.anthropic.com".into(),
        "ollama" => "http://localhost:11434".into(),
        "custom" => "http://localhost:8080".into(),
        _ => "https://generativelanguage.googleapis.com".into(),
    }
}

fn temperature() -> f64 {
    host::get_setting("temperature").trim().parse().unwrap_or(0.7)
}

fn max_tokens() -> u32 {
    host::get_setting("max_tokens").trim().parse().unwrap_or(2048)
}

fn system_prompt() -> String {
    host::get_setting("system_prompt").trim().to_string()
}

fn persist_enabled() -> bool {
    !matches!(
        host::get_setting("persist_history").to_lowercase().as_str(),
        "no" | "false" | "0" | "off"
    )
}

// ── Persistence ────────────────────────────────────────────────────────────

fn load_session() {
    if LOADED.with(|l| *l.borrow()) {
        return;
    }
    LOADED.with(|l| *l.borrow_mut() = true);
    if !persist_enabled() {
        return;
    }
    let Ok(bytes) = host::read_file(SESSION_FILE) else {
        return;
    };
    let Ok(text) = std::str::from_utf8(&bytes) else {
        return;
    };
    let Ok(value) = serde_json::from_str::<serde_json::Value>(text) else {
        return;
    };
    let saved_provider = value["provider"].as_str().unwrap_or("");
    // Drop the on-disk log if the user switched providers between sessions.
    if saved_provider != provider_key() {
        return;
    }
    LOG_PROVIDER.with(|p| *p.borrow_mut() = saved_provider.to_string());
    if let Some(msgs) = value["messages"].as_array() {
        let log: Vec<Msg> = msgs
            .iter()
            .filter_map(|m| {
                let role = match m["role"].as_str()? {
                    "you" | "user" => Role::You,
                    _ => Role::Ai,
                };
                let text = m["text"].as_str()?.to_string();
                Some(Msg { role, text })
            })
            .collect();
        LOG.with(|l| *l.borrow_mut() = log);
    }
}

fn save_session() {
    if !persist_enabled() {
        return;
    }
    let messages: Vec<serde_json::Value> = LOG.with(|l| {
        l.borrow()
            .iter()
            .filter(|m| !m.text.is_empty())
            .map(|m| {
                serde_json::json!({
                    "role": match m.role { Role::You => "you", Role::Ai => "ai" },
                    "text": m.text,
                })
            })
            .collect()
    });
    let provider = LOG_PROVIDER.with(|p| p.borrow().clone());
    let value = serde_json::json!({
        "provider": provider,
        "messages": messages,
    });
    let _ = host::write_file(SESSION_FILE, &value.to_string().into_bytes());
}

fn truncate_log() {
    LOG.with(|l| {
        let mut log = l.borrow_mut();
        if log.len() > MAX_MSGS {
            let drop = log.len() - MAX_MSGS;
            log.drain(0..drop);
        }
    });
}

// ── Request building (provider-specific) ───────────────────────────────────

fn build_request() -> host::HttpRequest {
    let api_key = host::get_setting("api_key");
    let msgs: Vec<Msg> = LOG.with(|l| {
        l.borrow()
            .iter()
            // Skip the trailing empty ai bubble we just opened.
            .filter(|m| !(m.role == Role::Ai && m.text.is_empty()))
            .cloned()
            .collect()
    });

    match provider_key() {
        "gemini" => gemini_request(&msgs, &api_key),
        "anthropic" => anthropic_request(&msgs, &api_key),
        "ollama" => ollama_request(&msgs),
        "openai" | "custom" => openai_request(&msgs, &api_key),
        _ => gemini_request(&msgs, &api_key),
    }
}

fn gemini_request(msgs: &[Msg], api_key: &str) -> host::HttpRequest {
    let url = format!(
        "{}/v1beta/models/{}:streamGenerateContent?alt=sse",
        endpoint(),
        model()
    );
    let contents: Vec<serde_json::Value> = msgs
        .iter()
        .map(|m| {
            serde_json::json!({
                "role": if m.role == Role::You { "user" } else { "model" },
                "parts": [{"text": m.text}],
            })
        })
        .collect();
    let mut body = serde_json::json!({
        "contents": contents,
        "generationConfig": {
            "temperature": temperature(),
            "maxOutputTokens": max_tokens(),
        }
    });
    let sys = system_prompt();
    if !sys.is_empty() {
        body["systemInstruction"] = serde_json::json!({
            "parts": [{"text": sys}],
        });
    }
    host::HttpRequest {
        method: "POST".into(),
        url,
        headers: vec![
            ("content-type".into(), "application/json".into()),
            ("x-goog-api-key".into(), api_key.to_string()),
        ],
        body: body.to_string(),
    }
}

fn openai_request(msgs: &[Msg], api_key: &str) -> host::HttpRequest {
    // Bases like https://api.openai.com get /v1/chat/completions; bases that
    // already include a version segment (…/v1, …/v4) get /chat/completions —
    // matches DMS's `openaiChatCompletionsUrl` heuristic.
    let base = endpoint();
    let url = if base
        .rsplit('/')
        .next()
        .map(|seg| seg.starts_with('v') && seg[1..].chars().all(|c| c.is_ascii_digit()))
        .unwrap_or(false)
    {
        format!("{base}/chat/completions")
    } else {
        format!("{base}/v1/chat/completions")
    };

    let sys = system_prompt();
    let mut messages: Vec<serde_json::Value> = Vec::new();
    if !sys.is_empty() {
        messages.push(serde_json::json!({"role": "system", "content": sys}));
    }
    messages.extend(msgs.iter().map(|m| {
        serde_json::json!({
            "role": if m.role == Role::You { "user" } else { "assistant" },
            "content": m.text,
        })
    }));
    let body = serde_json::json!({
        "model": model(),
        "messages": messages,
        "temperature": temperature(),
        "max_tokens": max_tokens(),
        "stream": true,
    });
    let mut headers = vec![("content-type".into(), "application/json".into())];
    if !api_key.trim().is_empty() {
        headers.push(("authorization".into(), format!("Bearer {api_key}")));
    }
    host::HttpRequest {
        method: "POST".into(),
        url,
        headers,
        body: body.to_string(),
    }
}

fn anthropic_request(msgs: &[Msg], api_key: &str) -> host::HttpRequest {
    let url = format!("{}/v1/messages", endpoint());
    let messages: Vec<serde_json::Value> = msgs
        .iter()
        .map(|m| {
            serde_json::json!({
                "role": if m.role == Role::You { "user" } else { "assistant" },
                "content": m.text,
            })
        })
        .collect();
    let mut body = serde_json::json!({
        "model": model(),
        "messages": messages,
        "max_tokens": max_tokens(),
        "temperature": temperature(),
        "stream": true,
    });
    let sys = system_prompt();
    if !sys.is_empty() {
        body["system"] = serde_json::Value::String(sys);
    }
    host::HttpRequest {
        method: "POST".into(),
        url,
        headers: vec![
            ("content-type".into(), "application/json".into()),
            ("x-api-key".into(), api_key.to_string()),
            ("anthropic-version".into(), "2023-06-01".into()),
        ],
        body: body.to_string(),
    }
}

fn ollama_request(msgs: &[Msg]) -> host::HttpRequest {
    let url = format!("{}/api/chat", endpoint());
    let sys = system_prompt();
    let mut messages: Vec<serde_json::Value> = Vec::new();
    if !sys.is_empty() {
        messages.push(serde_json::json!({"role": "system", "content": sys}));
    }
    messages.extend(msgs.iter().map(|m| {
        serde_json::json!({
            "role": if m.role == Role::You { "user" } else { "assistant" },
            "content": m.text,
        })
    }));
    let body = serde_json::json!({
        "model": model(),
        "messages": messages,
        "stream": true,
        "options": {
            "temperature": temperature(),
            "num_predict": max_tokens(),
        },
    });
    host::HttpRequest {
        method: "POST".into(),
        url,
        headers: vec![("content-type".into(), "application/json".into())],
        body: body.to_string(),
    }
}

// ── Streaming chunk parsers ────────────────────────────────────────────────

fn drain_lines(chunk: &str) -> Vec<String> {
    SSE_BUF.with(|b| {
        let mut buf = b.borrow_mut();
        buf.push_str(chunk);
        let mut out = Vec::new();
        while let Some(nl) = buf.find('\n') {
            let line: String = buf.drain(..=nl).collect();
            out.push(line.trim_end().to_string());
        }
        out
    })
}

fn consume_chunk(chunk: &str) {
    RAW.with(|r| r.borrow_mut().push_str(chunk));
    let active = ACTIVE_PROVIDER.with(|p| p.borrow().clone());
    let lines = drain_lines(chunk);
    if active == "ollama" {
        for line in lines {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            let Ok(v) = serde_json::from_str::<serde_json::Value>(trimmed) else {
                continue;
            };
            if let Some(text) = v["message"]["content"].as_str() {
                append_to_last_ai(text);
            }
        }
    } else {
        for line in lines {
            parse_sse_line(&active, &line);
        }
    }
}

fn parse_sse_line(provider: &str, line: &str) {
    let Some(payload) = line.strip_prefix("data:") else {
        return;
    };
    let payload = payload.trim();
    if payload.is_empty() || payload == "[DONE]" {
        return;
    }
    let Ok(v) = serde_json::from_str::<serde_json::Value>(payload) else {
        return;
    };
    match provider {
        "gemini" => {
            if let Some(text) = v["candidates"][0]["content"]["parts"][0]["text"].as_str() {
                append_to_last_ai(text);
            }
        }
        "anthropic" => {
            if v["type"] == "content_block_delta" {
                if let Some(text) = v["delta"]["text"].as_str() {
                    append_to_last_ai(text);
                }
            }
        }
        _ => {
            // OpenAI-compatible: choices[0].delta.content
            if let Some(text) = v["choices"][0]["delta"]["content"].as_str() {
                append_to_last_ai(text);
            }
        }
    }
}

fn append_to_last_ai(text: &str) {
    LOG.with(|l| {
        if let Some(last) = l.borrow_mut().last_mut() {
            if last.role == Role::Ai {
                last.text.push_str(text);
            }
        }
    });
}

fn error_from_raw(raw: &str) -> String {
    let raw = raw.trim();
    if raw.is_empty() {
        return "⚠ no response from the API — check network and endpoint".into();
    }
    if let Ok(json) = serde_json::from_str::<serde_json::Value>(raw) {
        for key in ["error", "detail"] {
            if let Some(msg) = json[key]["message"].as_str() {
                return format!("⚠ {msg}");
            }
            if let Some(msg) = json[key].as_str() {
                return format!("⚠ {msg}");
            }
        }
    }
    let snippet: String = raw.chars().take(300).collect();
    format!("⚠ {snippet}")
}

// ── Submit & actions ───────────────────────────────────────────────────────

fn submit(text: String) {
    load_session();

    // Provider switched mid-session: drop the log so we don't send Ollama
    // history to OpenAI (and vice versa).
    let now = provider_key().to_string();
    let prev = LOG_PROVIDER.with(|p| p.borrow().clone());
    if !prev.is_empty() && prev != now {
        LOG.with(|l| l.borrow_mut().clear());
    }
    LOG_PROVIDER.with(|p| *p.borrow_mut() = now.clone());

    LOG.with(|l| {
        let mut log = l.borrow_mut();
        // Replace a leftover empty ai bubble (a previous stream that never
        // produced text) instead of stacking another one.
        if matches!(log.last(), Some(m) if m.role == Role::Ai && m.text.is_empty()) {
            log.pop();
        }
        log.push(Msg {
            role: Role::You,
            text,
        });
        log.push(Msg {
            role: Role::Ai,
            text: String::new(),
        });
    });
    truncate_log();

    SSE_BUF.with(|b| b.borrow_mut().clear());
    RAW.with(|r| r.borrow_mut().clear());
    ERROR.with(|e| e.borrow_mut().clear());
    STOPPED.with(|s| *s.borrow_mut() = false);
    STREAMING.with(|s| *s.borrow_mut() = true);
    ACTIVE_PROVIDER.with(|p| *p.borrow_mut() = now);

    let _ = host::http_start(&build_request());
}

fn retry() {
    // Pop trailing ai bubble (failed or empty), grab the last user text, resend.
    let last_user = LOG.with(|l| {
        let mut log = l.borrow_mut();
        if matches!(log.last(), Some(m) if m.role == Role::Ai) {
            log.pop();
        }
        log.last()
            .filter(|m| m.role == Role::You)
            .map(|m| m.text.clone())
    });
    if let Some(text) = last_user {
        // submit() pushes a fresh "you" bubble, but the prior one is still in
        // the log — drop it first so we don't echo the question twice.
        LOG.with(|l| {
            if matches!(l.borrow().last(), Some(m) if m.role == Role::You) {
                l.borrow_mut().pop();
            }
        });
        submit(text);
    }
}

fn clear_chat() {
    LOG.with(|l| l.borrow_mut().clear());
    ERROR.with(|e| e.borrow_mut().clear());
    SSE_BUF.with(|b| b.borrow_mut().clear());
    RAW.with(|r| r.borrow_mut().clear());
    save_session();
}

fn copy_last_reply() {
    let text = LOG.with(|l| {
        l.borrow()
            .iter()
            .rev()
            .find(|m| m.role == Role::Ai && !m.text.is_empty())
            .map(|m| m.text.clone())
    });
    if let Some(text) = text {
        host::copy(&text);
        host::notify("Assistant", "Last reply copied to clipboard");
    }
}

// ── UI ─────────────────────────────────────────────────────────────────────

fn header_card() -> El {
    let streaming = STREAMING.with(|s| *s.borrow());
    let has_history = LOG.with(|l| !l.borrow().is_empty());

    let mut actions: Vec<El> = Vec::new();
    if streaming {
        actions.push(
            El::button("act:stop", "Stop").class("plugin-action plugin-action-danger"),
        );
    } else if has_history {
        actions.push(El::button("act:retry", "Retry").class("plugin-action"));
        actions.push(El::button("act:copy", "Copy").class("plugin-action"));
        actions.push(El::button("act:clear", "New").class("plugin-action"));
    }

    let status = if streaming {
        format!("{} · {} · streaming…", provider_label(), model())
    } else {
        format!("{} · {}", provider_label(), model())
    };

    El::hbox(vec![
        El::image("starred-symbolic"),
        El::vbox(vec![
            El::label("Assistant").class("label-large-bold").halign("start"),
            El::label(status).class("dim-label").halign("start"),
        ])
        .spacing(4)
        .hexpand(true),
        El::hbox(actions).spacing(8),
    ])
    .class("plugin-panel-header")
    .spacing(12)
}

fn empty_state() -> El {
    El::vbox(vec![
        El::image("starred-symbolic"),
        El::label("Ask anything").class("label-large-bold"),
        El::label(format!(
            "Streaming chat with {}. Start by typing below.",
            provider_label()
        ))
        .class("dim-label"),
    ])
    .halign("center")
    .valign("center")
    .spacing(8)
    .padding(24)
}

fn bubble(m: &Msg) -> El {
    let role_label = match m.role {
        Role::You => "You",
        Role::Ai => provider_label(),
    };
    let body = if m.role == Role::Ai && m.text.is_empty() {
        "_…thinking_".to_string()
    } else {
        m.text.clone()
    };
    let bubble_class = match m.role {
        Role::You => "plugin-bubble-you",
        Role::Ai => "plugin-bubble-ai",
    };
    El::vbox(vec![
        El::label(role_label).class("dim-label").halign("start"),
        El::markdown(body).class(bubble_class),
    ])
    .spacing(4)
}

fn log_view() -> El {
    let bubbles: Vec<El> = LOG.with(|l| l.borrow().iter().map(bubble).collect());
    if bubbles.is_empty() {
        El::scroll(vec![empty_state()]).vexpand(true)
    } else {
        El::scroll(bubbles).with_id("log").vexpand(true).spacing(12)
    }
}

fn input_area() -> El {
    let err = ERROR.with(|e| e.borrow().clone());
    let streaming = STREAMING.with(|s| *s.borrow());
    let placeholder = if streaming {
        "Streaming reply… type your next question"
    } else {
        "Ask anything — Enter to send"
    };
    let mut children: Vec<El> = Vec::new();
    if !err.is_empty() {
        children.push(El::label(err).class("dim-label").halign("start"));
    }
    children.push(El::entry("input", "").prop("placeholder", placeholder));
    El::vbox(children).spacing(4).class("plugin-search")
}

fn view_tree() -> El {
    load_session();
    El::vbox(vec![header_card(), log_view(), input_area()]).spacing(12)
}

// ── Component impl ─────────────────────────────────────────────────────────

struct Assistant;

impl Component for Assistant {
    fn view() -> El {
        view_tree()
    }

    fn update(ev: Event) -> El {
        match ev.kind {
            EventKind::Click => match ev.id.as_str() {
                "act:clear" => clear_chat(),
                "act:copy" => copy_last_reply(),
                "act:stop" => {
                    STOPPED.with(|s| *s.borrow_mut() = true);
                    STREAMING.with(|s| *s.borrow_mut() = false);
                    // Drop the trailing empty ai bubble so the log doesn't keep
                    // a phantom "…thinking" row.
                    LOG.with(|l| {
                        let mut log = l.borrow_mut();
                        if matches!(log.last(), Some(m) if m.role == Role::Ai && m.text.is_empty()) {
                            log.pop();
                        }
                    });
                }
                "act:retry" => retry(),
                _ => {}
            },
            EventKind::Submit if ev.id == "input" && !ev.value.trim().is_empty() => {
                submit(ev.value);
            }
            EventKind::StreamChunk => {
                if STOPPED.with(|s| *s.borrow()) {
                    return view_tree();
                }
                consume_chunk(&ev.value);
            }
            EventKind::StreamEnd => {
                STREAMING.with(|s| *s.borrow_mut() = false);
                // Flush any trailing un-newlined fragment.
                let tail = SSE_BUF.with(|b| std::mem::take(&mut *b.borrow_mut()));
                if !tail.trim().is_empty() {
                    consume_chunk(&format!("{tail}\n"));
                }
                // Surface a non-streaming error body so the bubble explains why
                // there was no reply (bad key, model name, quota, …).
                let empty = LOG.with(|l| {
                    l.borrow()
                        .last()
                        .map(|m| m.role == Role::Ai && m.text.is_empty())
                        .unwrap_or(false)
                });
                if empty {
                    let msg = RAW.with(|r| error_from_raw(&r.borrow()));
                    LOG.with(|l| {
                        if let Some(last) = l.borrow_mut().last_mut() {
                            last.text = msg.clone();
                        }
                    });
                    ERROR.with(|e| *e.borrow_mut() = msg);
                }
                save_session();
            }
            EventKind::Keybind => {
                // Manifest hotkey fires `keybind` with id == "open"; the panel
                // is already opening when this arrives, so nothing to do.
            }
            _ => {}
        }
        view_tree()
    }
}

export_component!(Assistant);
