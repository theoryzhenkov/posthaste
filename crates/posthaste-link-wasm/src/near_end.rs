//! The wasm-bindgen boundary for the [`posthaste_link_near_end`] engine (D41).
//!
//! This is a NEW boundary shape beside the sync [`crate::entity_store`] exports:
//! JS hands an **IO object** at construction (its `postJson` + `openStream`
//! callbacks are the engine's [`Transport`]; the `onFrame`/`onMalformed`/
//! `onStatus` callbacks are its [`FrameSink`]; the `neverDispatched`/
//! `onReconciled` callbacks are its [`OutboxHooks`]), and the handle exposes
//! **Promise-returning** lifecycle methods (`connect`/`disconnect`/`forward`).
//! Every scrap of policy — deadlines, backoff, the resume cursor, the reconciler,
//! the typed frame parse, the 4xx classification — lives in the engine. The JS
//! shim is pure IO: `fetch` + `fetchEventSource`, zero policy.
//!
//! Timing/jitter come from `gloo-timers` + `Math.random` (browser primitives),
//! keeping them out of the wasm-pure engine crate.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use futures_channel::mpsc;
use futures_util::future::LocalBoxFuture;
use futures_util::stream::{LocalBoxStream, Stream};
use futures_util::{FutureExt, StreamExt};
use js_sys::{Function, Promise, Reflect};
use serde::Deserialize;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::{future_to_promise, spawn_local, JsFuture};

use posthaste_contract_core::{MutationReceipt, MutationRequest, RuntimeFrame};
use posthaste_link_near_end::{
    ConnectionStatus, FrameSink, NearEnd, NearEndConfig, OutboxHooks, PostRequest, PostResponse,
    Scheduler, StreamEvent, StreamRequest, Transport, TransportError,
};

fn js_error_string(value: &JsValue) -> String {
    value
        .as_string()
        .or_else(|| {
            Reflect::get(value, &JsValue::from_str("message"))
                .ok()
                .and_then(|m| m.as_string())
        })
        .unwrap_or_else(|| "javascript error".to_string())
}

fn get_function(io: &JsValue, name: &str) -> Result<Function, JsError> {
    Reflect::get(io, &JsValue::from_str(name))
        .ok()
        .and_then(|value| value.dyn_into::<Function>().ok())
        .ok_or_else(|| JsError::new(&format!("io.{name} must be a function")))
}

// ---- Transport -------------------------------------------------------------

/// Binds the JS `postJson` / `openStream` callbacks as the engine's transport.
struct JsTransport {
    post_json: Function,
    open_stream: Function,
}

/// A [`Stream`] fed by the JS `openStream` event callback. Holds the closure
/// alive for the stream's life and calls the JS-returned abort fn on drop.
struct JsEventStream {
    receiver: mpsc::UnboundedReceiver<StreamEvent>,
    _on_event: Closure<dyn FnMut(String, String, f64)>,
    abort: Option<Function>,
}

impl Stream for JsEventStream {
    type Item = StreamEvent;
    fn poll_next(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        self.receiver.poll_next_unpin(cx)
    }
}

impl Drop for JsEventStream {
    fn drop(&mut self) {
        if let Some(abort) = &self.abort {
            let _ = abort.call0(&JsValue::NULL);
        }
    }
}

impl Transport for JsTransport {
    fn post_json(
        &self,
        request: PostRequest,
    ) -> LocalBoxFuture<'static, Result<PostResponse, TransportError>> {
        let func = self.post_json.clone();
        let headers_json = serde_json::to_string(&request.headers).unwrap_or_else(|_| "[]".into());
        async move {
            let returned = func
                .call3(
                    &JsValue::NULL,
                    &JsValue::from_str(&request.url),
                    &JsValue::from_str(&headers_json),
                    &JsValue::from_str(&request.body),
                )
                .map_err(|e| TransportError::new(js_error_string(&e)))?;
            let promise: Promise = returned
                .dyn_into()
                .map_err(|_| TransportError::new("postJson must return a promise"))?;
            let value = JsFuture::from(promise)
                .await
                .map_err(|e| TransportError::new(js_error_string(&e)))?;
            let status = Reflect::get(&value, &JsValue::from_str("status"))
                .ok()
                .and_then(|v| v.as_f64())
                .unwrap_or(0.0) as u16;
            let body = Reflect::get(&value, &JsValue::from_str("body"))
                .ok()
                .and_then(|v| v.as_string())
                .unwrap_or_default();
            Ok(PostResponse { status, body })
        }
        .boxed_local()
    }

    fn open_stream(&self, request: StreamRequest) -> LocalBoxStream<'static, StreamEvent> {
        let (sender, receiver) = mpsc::unbounded();
        // JS calls onEvent(kind, data, status): "open"/"message"/"closed"/
        // "error" — status is the HTTP status on an open failure, or -1.
        let on_event = Closure::wrap(Box::new(move |kind: String, data: String, status: f64| {
            let event = match kind.as_str() {
                "open" => StreamEvent::Open,
                "message" => StreamEvent::Message(data),
                "closed" => StreamEvent::Closed,
                _ => StreamEvent::Error {
                    status: if status >= 0.0 { Some(status as u16) } else { None },
                    message: data,
                },
            };
            let _ = sender.unbounded_send(event);
        }) as Box<dyn FnMut(String, String, f64)>);

        let abort = self
            .open_stream
            .call2(
                &JsValue::NULL,
                &JsValue::from_str(&request.url),
                on_event.as_ref().unchecked_ref(),
            )
            .ok()
            .and_then(|v| v.dyn_into::<Function>().ok());

        JsEventStream {
            receiver,
            _on_event: on_event,
            abort,
        }
        .boxed_local()
    }
}

// ---- Scheduler -------------------------------------------------------------

struct BrowserScheduler;

impl Scheduler for BrowserScheduler {
    fn sleep(&self, duration: Duration) -> LocalBoxFuture<'static, ()> {
        let ms = duration.as_millis().min(u128::from(u32::MAX)) as u32;
        async move {
            gloo_timers::future::TimeoutFuture::new(ms).await;
        }
        .boxed_local()
    }

    fn jitter(&self) -> f64 {
        js_sys::Math::random()
    }
}

// ---- FrameSink -------------------------------------------------------------

struct JsFrameSink {
    on_frame: Function,
    on_malformed: Function,
    on_status: Function,
}

impl FrameSink for JsFrameSink {
    fn on_frame(&self, frame: RuntimeFrame) {
        if let Ok(json) = serde_json::to_string(&frame) {
            let _ = self.on_frame.call1(&JsValue::NULL, &JsValue::from_str(&json));
        }
    }

    fn on_malformed(&self, raw: String, error: String) {
        let _ = self.on_malformed.call2(
            &JsValue::NULL,
            &JsValue::from_str(&raw),
            &JsValue::from_str(&error),
        );
    }

    fn on_status(&self, status: ConnectionStatus) {
        let (label, message) = match status {
            ConnectionStatus::Connecting => ("connecting", String::new()),
            ConnectionStatus::Connected => ("connected", String::new()),
            ConnectionStatus::Reconnecting => ("reconnecting", String::new()),
            ConnectionStatus::TransientError(m) => ("transientError", m),
            ConnectionStatus::PermanentError(m) => ("permanentError", m),
        };
        let _ = self.on_status.call2(
            &JsValue::NULL,
            &JsValue::from_str(label),
            &JsValue::from_str(&message),
        );
    }
}

// ---- OutboxHooks -----------------------------------------------------------

struct JsOutbox {
    never_dispatched: Function,
    on_reconciled: Function,
}

impl OutboxHooks for JsOutbox {
    fn never_dispatched(&self) -> LocalBoxFuture<'static, Vec<MutationRequest>> {
        let func = self.never_dispatched.clone();
        async move {
            let returned = match func.call0(&JsValue::NULL) {
                Ok(value) => value,
                Err(_) => return Vec::new(),
            };
            // Accept either a Promise<string> or a synchronous string.
            let json = match returned.dyn_into::<Promise>() {
                Ok(promise) => match JsFuture::from(promise).await {
                    Ok(value) => value.as_string().unwrap_or_default(),
                    Err(_) => return Vec::new(),
                },
                Err(value) => value.as_string().unwrap_or_default(),
            };
            serde_json::from_str::<Vec<MutationRequest>>(&json).unwrap_or_default()
        }
        .boxed_local()
    }

    fn on_reconciled(&self, receipt: MutationReceipt) -> LocalBoxFuture<'static, ()> {
        let func = self.on_reconciled.clone();
        let json = serde_json::to_string(&receipt).unwrap_or_default();
        async move {
            let _ = func.call1(&JsValue::NULL, &JsValue::from_str(&json));
        }
        .boxed_local()
    }
}

// ---- config ----------------------------------------------------------------

#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct JsConfig {
    base_url: Option<String>,
    view_delta: Option<bool>,
    source_id: Option<String>,
    initial_cursor: Option<u64>,
    request_deadline_ms: Option<u64>,
    forward_max_attempts: Option<u32>,
}

impl JsConfig {
    fn into_config(self) -> NearEndConfig {
        let mut config = NearEndConfig::default();
        if let Some(base) = self.base_url {
            config.base_url = base;
        }
        if let Some(view_delta) = self.view_delta {
            config.view_delta = view_delta;
        }
        config.source_id = self.source_id;
        config.initial_cursor = self.initial_cursor;
        if let Some(ms) = self.request_deadline_ms {
            config.request_deadline = Duration::from_millis(ms);
        }
        if let Some(attempts) = self.forward_max_attempts {
            config.forward_max_attempts = attempts.max(1);
        }
        config
    }
}

// ---- the handle ------------------------------------------------------------

/// The near-end engine, driven from JS. Constructed with an IO object + a
/// config JSON string; `connect`/`disconnect`/`forward` return Promises.
#[wasm_bindgen]
pub struct NearEndHandle {
    engine: Rc<NearEnd>,
    running: RefCell<bool>,
}

#[wasm_bindgen]
impl NearEndHandle {
    /// Build the engine from the JS IO object and a config JSON string.
    ///
    /// `io` must expose: `postJson(url, headersJson, body) => Promise<{status,
    /// body}>`, `openStream(url, onEvent) => abortFn` (where `onEvent(kind, data,
    /// status)`), `onFrame(json)`, `onMalformed(raw, error)`, `onStatus(label,
    /// message)`, `neverDispatched() => Promise<string>` (a JSON array of
    /// forward requests), and `onReconciled(receiptJson)`.
    #[wasm_bindgen(constructor)]
    pub fn new(io: &JsValue, config_json: &str) -> Result<NearEndHandle, JsError> {
        let transport = JsTransport {
            post_json: get_function(io, "postJson")?,
            open_stream: get_function(io, "openStream")?,
        };
        let sink = JsFrameSink {
            on_frame: get_function(io, "onFrame")?,
            on_malformed: get_function(io, "onMalformed")?,
            on_status: get_function(io, "onStatus")?,
        };
        let outbox = JsOutbox {
            never_dispatched: get_function(io, "neverDispatched")?,
            on_reconciled: get_function(io, "onReconciled")?,
        };
        let config: JsConfig = if config_json.trim().is_empty() {
            JsConfig::default()
        } else {
            serde_json::from_str(config_json)
                .map_err(|e| JsError::new(&format!("invalid config: {e}")))?
        };

        let engine = NearEnd::new(
            Rc::new(transport),
            Rc::new(BrowserScheduler),
            Rc::new(sink),
            Rc::new(outbox),
            config.into_config(),
        );
        Ok(NearEndHandle {
            engine,
            running: RefCell::new(false),
        })
    }

    /// Open the session and start the frame loop (idempotent). The Promise
    /// resolves once the session is open; the reconnect loop then runs in the
    /// background (`spawn_local`) until [`Self::disconnect`].
    pub fn connect(&self) -> Promise {
        let engine = self.engine.clone();
        let already_running = std::mem::replace(&mut *self.running.borrow_mut(), true);
        future_to_promise(async move {
            engine
                .open()
                .await
                .map_err(|e| JsValue::from(JsError::new(&e.message)))?;
            if !already_running {
                spawn_local(engine.clone().run());
            }
            Ok(JsValue::UNDEFINED)
        })
    }

    /// Stop the frame loop (no further reconnects). Session close is a host
    /// concern (a policy-free `DELETE` via the api client) since the transport is
    /// post-only.
    pub fn disconnect(&self) -> Promise {
        self.engine.request_shutdown();
        *self.running.borrow_mut() = false;
        Promise::resolve(&JsValue::UNDEFINED)
    }

    /// Forward a mutation (JSON of a `MutationRequest`). Resolves with the
    /// receipt JSON on 2xx (including an authority `failed` verdict); rejects on
    /// a permanent 4xx or exhausted transient retries.
    pub fn forward(&self, request_json: String) -> Promise {
        let engine = self.engine.clone();
        future_to_promise(async move {
            let request: MutationRequest = serde_json::from_str(&request_json)
                .map_err(|e| JsValue::from(JsError::new(&format!("invalid request: {e}"))))?;
            let receipt = engine
                .forward(request)
                .await
                .map_err(|e| JsValue::from(JsError::new(&e.message)))?;
            let json = serde_json::to_string(&receipt)
                .map_err(|e| JsValue::from(JsError::new(&e.to_string())))?;
            Ok(JsValue::from_str(&json))
        })
    }

    /// The current session id, once connected.
    #[wasm_bindgen(js_name = sessionId)]
    pub fn session_id(&self) -> Option<String> {
        self.engine.session_id().map(|id| id.as_str().to_string())
    }

    /// The engine-owned resume cursor (last seen `sessionSeq`). The host mirrors
    /// this to durable storage so a reload resumes where it left off — callers no
    /// longer thread `afterSeq`.
    pub fn cursor(&self) -> Option<f64> {
        self.engine.cursor().map(|c| c as f64)
    }
}
