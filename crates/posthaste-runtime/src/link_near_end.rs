//! The runtime's **native near-end host** (D40/D45): the tokio/reqwest side of
//! the wasm-pure `posthaste-link-near-end` engine, mounted at the
//! runtime↔authority-server seam.
//!
//! The engine crate owns the policy — reconnect loop, resume cursor, request
//! deadline, jittered capped backoff, permanent-vs-transient classification —
//! and stays wasm-pure by expressing IO/timing as traits. This module supplies
//! the native implementations it needs:
//!
//! * [`NativeLinkTransport`] — `Transport` over a boxed `reqwest` client:
//!   `post_json`/`get_json` round-trips and an `open_stream` that performs the
//!   SSE **framing** (byte stream → one payload string per event block; framing
//!   is transport shape, not policy — the JSON parse happens in the engine's
//!   wire);
//! * [`TokioScheduler`] — `Scheduler` over `tokio::time::sleep` + a xorshift
//!   jitter source (decorrelation only, per the trait's contract);
//! * [`AuthorityLinkWire`] — the seam's [`Wire`] profile: no prepare step (the
//!   AS link has no session open), forwards on [`LINK_FORWARD_MUTATION_PATH`],
//!   subscribes on [`LINK_SUBSCRIBE_PATH`] with the engine-owned `afterSeq`
//!   resume cursor (D46), frames parse as [`SequencedFrame`];
//! * [`NativeNearEnd`] — the actor that runs the (deliberately `!Send`,
//!   `Rc`-state) engine on a dedicated thread with a current-thread runtime,
//!   exposing `Send` entry points: `forward` (deadline + retry from the engine)
//!   and the down-channel frame receiver the read-path consumer drains.

use std::cell::Cell;
use std::rc::Rc;
use std::sync::Mutex;

use futures_util::future::LocalBoxFuture;
use futures_util::stream::LocalBoxStream;
use futures_util::{FutureExt, Stream, StreamExt};
use tokio::sync::{mpsc, oneshot};

use posthaste_authority_server_link::{
    LinkCoverage, SequencedFrame, LINK_FORWARD_MUTATION_PATH, LINK_SUBSCRIBE_PATH,
};
use posthaste_contract_core::{
    MutationReceipt, MutationRequest, RuntimeError, RuntimeErrorCode,
};
use posthaste_link_near_end::{
    ConnectionStatus, EngineError, FrameSink, GetRequest, NearEnd, NearEndConfig, OutboxHooks,
    ParsedFrame, PostRequest, PostResponse, Scheduler, SentUnsettled, StreamEvent, StreamRequest,
    Transport, TransportError, Wire,
};

// ---- SSE framing -------------------------------------------------------------

/// Split an SSE byte stream into `data:` payload strings — one per
/// `\n\n`-delimited event block, non-data lines (comments, `event:`/`id:`)
/// dropped, keep-alive blocks (no data) skipped. Pure framing: no JSON here.
pub(crate) fn sse_payloads<S, B, E>(bytes: S) -> impl Stream<Item = String> + Send
where
    S: Stream<Item = Result<B, E>> + Send + 'static,
    B: AsRef<[u8]> + Send + 'static,
    E: Send + 'static,
{
    async_stream::stream! {
        let mut bytes = std::pin::pin!(bytes);
        let mut buffer = String::new();
        while let Some(chunk) = bytes.next().await {
            let Ok(chunk) = chunk else { break };
            buffer.push_str(&String::from_utf8_lossy(chunk.as_ref()));
            while let Some(boundary) = buffer.find("\n\n") {
                let block: String = buffer.drain(..boundary + 2).collect();
                if let Some(payload) = sse_block_payload(&block) {
                    yield payload;
                }
            }
        }
    }
}

/// Extract the concatenated `data:` payload of one SSE event block, or `None`
/// for a keep-alive/comment block.
fn sse_block_payload(block: &str) -> Option<String> {
    let mut data = String::new();
    for line in block.lines() {
        if let Some(value) = line.strip_prefix("data:") {
            data.push_str(value.trim_start_matches(' '));
        }
    }
    if data.is_empty() {
        None
    } else {
        Some(data)
    }
}

// ---- Transport ----------------------------------------------------------------

/// The native `Transport`: boxed `reqwest` round-trips + SSE-framed streams.
/// Zero policy — deadlines/retries/classification stay in the engine.
pub(crate) struct NativeLinkTransport {
    client: reqwest::Client,
    base_url: String,
    token: Option<String>,
}

impl NativeLinkTransport {
    pub(crate) fn new(client: reqwest::Client, base_url: String, token: Option<String>) -> Self {
        Self {
            client,
            base_url: base_url.trim_end_matches('/').to_string(),
            token,
        }
    }

    fn authed(&self, builder: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        match &self.token {
            Some(token) => builder.bearer_auth(token),
            None => builder,
        }
    }

    fn full_url(&self, path: &str) -> String {
        format!("{}{}", self.base_url, path)
    }
}

fn transport_error(error: reqwest::Error) -> TransportError {
    TransportError::new(error.to_string())
}

async fn into_post_response(response: reqwest::Response) -> Result<PostResponse, TransportError> {
    let status = response.status().as_u16();
    let body = response.text().await.map_err(transport_error)?;
    Ok(PostResponse { status, body })
}

impl Transport for NativeLinkTransport {
    fn post_json(
        &self,
        request: PostRequest,
    ) -> LocalBoxFuture<'static, Result<PostResponse, TransportError>> {
        let mut builder = self.authed(self.client.post(self.full_url(&request.url)));
        for (name, value) in &request.headers {
            builder = builder.header(name, value);
        }
        let builder = builder.body(request.body);
        async move {
            into_post_response(builder.send().await.map_err(transport_error)?).await
        }
        .boxed_local()
    }

    fn get_json(
        &self,
        request: GetRequest,
    ) -> LocalBoxFuture<'static, Result<PostResponse, TransportError>> {
        let mut builder = self.authed(self.client.get(self.full_url(&request.url)));
        for (name, value) in &request.headers {
            builder = builder.header(name, value);
        }
        async move {
            into_post_response(builder.send().await.map_err(transport_error)?).await
        }
        .boxed_local()
    }

    fn open_stream(&self, request: StreamRequest) -> LocalBoxStream<'static, StreamEvent> {
        let mut builder = self.authed(self.client.get(self.full_url(&request.url)));
        for (name, value) in &request.headers {
            builder = builder.header(name, value);
        }
        async_stream::stream! {
            let response = match builder.send().await {
                Ok(response) => response,
                Err(error) => {
                    yield StreamEvent::Error {
                        status: None,
                        message: error.to_string(),
                    };
                    return;
                }
            };
            let status = response.status();
            if !status.is_success() {
                yield StreamEvent::Error {
                    status: Some(status.as_u16()),
                    message: format!("authority server refused link subscription ({status})"),
                };
                return;
            }
            yield StreamEvent::Open;
            let mut payloads = std::pin::pin!(sse_payloads(response.bytes_stream()));
            while let Some(payload) = payloads.next().await {
                yield StreamEvent::Message(payload);
            }
            yield StreamEvent::Closed;
        }
        .boxed_local()
    }
}

// ---- Scheduler ------------------------------------------------------------------

/// Native timing + jitter: `tokio::time::sleep` and a xorshift unit-interval
/// source (the engine only needs decorrelation, per the `Scheduler` contract).
pub(crate) struct TokioScheduler {
    rng_state: Cell<u64>,
}

impl TokioScheduler {
    pub(crate) fn new() -> Self {
        let seed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0x9E37_79B9_7F4A_7C15)
            | 1;
        Self {
            rng_state: Cell::new(seed),
        }
    }
}

impl Scheduler for TokioScheduler {
    fn sleep(&self, duration: std::time::Duration) -> LocalBoxFuture<'static, ()> {
        tokio::time::sleep(duration).boxed_local()
    }

    fn jitter(&self) -> f64 {
        // xorshift64*: plenty for backoff decorrelation.
        let mut x = self.rng_state.get();
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.rng_state.set(x);
        let value = x.wrapping_mul(0x2545_F491_4F6C_DD1D);
        (value >> 11) as f64 / (1u64 << 53) as f64
    }
}

// ---- Wire -----------------------------------------------------------------------

/// The runtime↔authority-server [`Wire`] profile: no prepare step, forwards on
/// the link's mutation path, subscribes with coverage + the engine-owned
/// `afterSeq` resume cursor (coverage says WHAT to stream, the seq says WHERE
/// to resume — D46).
pub(crate) struct AuthorityLinkWire {
    coverage: LinkCoverage,
}

impl AuthorityLinkWire {
    pub(crate) fn new(coverage: LinkCoverage) -> Self {
        Self { coverage }
    }
}

impl Wire for AuthorityLinkWire {
    type Frame = SequencedFrame;

    fn prepare_request(&self) -> Option<PostRequest> {
        None
    }

    fn parse_prepared(&self, _body: &str) -> Result<String, String> {
        Err("the authority-server link has no prepare step".to_string())
    }

    fn forward_request(
        &self,
        _token: Option<&str>,
        request: &MutationRequest,
    ) -> Result<PostRequest, EngineError> {
        let body = serde_json::to_string(request)
            .map_err(|e| EngineError::permanent(format!("serialize mutation: {e}")))?;
        Ok(PostRequest {
            url: LINK_FORWARD_MUTATION_PATH.to_string(),
            headers: vec![("content-type".to_string(), "application/json".to_string())],
            body,
        })
    }

    fn stream_request(&self, _token: Option<&str>, cursor: Option<u64>) -> StreamRequest {
        let mut query = Vec::new();
        // `Complete` is the wire default — omit it rather than URL-encode JSON.
        if self.coverage != LinkCoverage::Complete {
            if let Ok(encoded) = serde_json::to_string(&self.coverage) {
                query.push(format!("coverage={}", percent_encode(&encoded)));
            }
        }
        if let Some(after) = cursor {
            query.push(format!("afterSeq={after}"));
        }
        let suffix = if query.is_empty() {
            String::new()
        } else {
            format!("?{}", query.join("&"))
        };
        StreamRequest {
            url: format!("{LINK_SUBSCRIBE_PATH}{suffix}"),
            headers: Vec::new(),
        }
    }

    fn parse_frame(&self, data: &str) -> Result<ParsedFrame<SequencedFrame>, String> {
        let frame: SequencedFrame = serde_json::from_str(data).map_err(|e| e.to_string())?;
        Ok(if frame.is_reset() {
            ParsedFrame::Reset {
                highest_seq: frame.seq(),
            }
        } else {
            // The whole `Sequenced::Frame` is the seam frame this near-end carries
            // (its consumer reads the inner assertion/settlement + seq).
            let seq = frame.seq();
            ParsedFrame::Frame { seq, frame }
        })
    }

    fn settlement_request(
        &self,
        _session_id: &str,
        _client_mutation_id: &str,
    ) -> Option<GetRequest> {
        // The runtime's own forwards settle on the receipt + down-channel
        // absorption; this seam has no cross-session settlement query.
        None
    }
}

/// Percent-encode a query value (RFC 3986 unreserved set kept verbatim).
fn percent_encode(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                out.push(byte as char)
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

// ---- sinks / hooks -----------------------------------------------------------------

/// Delivers engine-parsed frames into the down-channel mpsc the read-path
/// consumer drains; lifecycle transitions become tracing (the runtime has no
/// degraded-mode UI — its recovery IS the engine's reconnect).
struct ChannelFrameSink {
    frames: mpsc::UnboundedSender<SequencedFrame>,
}

impl FrameSink<SequencedFrame> for ChannelFrameSink {
    fn on_frame(&self, frame: SequencedFrame) {
        let _ = self.frames.send(frame);
    }

    fn on_malformed(&self, raw: String, error: String) {
        tracing::warn!(%error, raw, "dropping a malformed authority-server link frame");
    }

    fn on_reset(&self) {
        // D49: the near node's incremental base view is broken (a seq gap the
        // far-end could not replay). Signal the read-path consumer to evict its
        // whole read cache and re-read through, as a `Reset` element on the same
        // channel (order-preserving with the frames it interleaves).
        let _ = self.frames.send(SequencedFrame::reset(0));
    }

    fn on_status(&self, status: ConnectionStatus) {
        match status {
            ConnectionStatus::Connecting => {
                tracing::debug!("authority-server down-channel connecting")
            }
            ConnectionStatus::Connected => {
                tracing::debug!("authority-server down-channel connected")
            }
            ConnectionStatus::Reconnecting => {
                tracing::info!("authority-server down-channel closed; reconnecting")
            }
            ConnectionStatus::TransientError(message) => {
                tracing::warn!(%message, "authority-server down-channel transient error; will reconnect")
            }
            ConnectionStatus::Degraded(message) => {
                tracing::error!(%message, "authority-server down-channel degraded (malformed frames); stream stopped")
            }
            ConnectionStatus::PermanentError(message) => {
                tracing::error!(%message, "authority-server down-channel permanent error; stream stopped")
            }
        }
    }
}

/// The runtime keeps no engine-side outbox at this seam: its own
/// `RuntimeAuthorityServerOutbox` settles on receipts + down-channel absorption
/// (unchanged semantics — only the connection lifecycle moved to the engine).
struct NoopOutboxHooks;

impl OutboxHooks for NoopOutboxHooks {
    fn never_dispatched(&self) -> LocalBoxFuture<'static, Vec<MutationRequest>> {
        futures_util::future::ready(Vec::new()).boxed_local()
    }
    fn on_reconciled(&self, _receipt: MutationReceipt) -> LocalBoxFuture<'static, ()> {
        futures_util::future::ready(()).boxed_local()
    }
    fn sent_unsettled(&self) -> LocalBoxFuture<'static, Vec<SentUnsettled>> {
        futures_util::future::ready(Vec::new()).boxed_local()
    }
    fn on_settlement(&self, _receipt: MutationReceipt) -> LocalBoxFuture<'static, ()> {
        futures_util::future::ready(()).boxed_local()
    }
}

// ---- the actor --------------------------------------------------------------------

enum Command {
    /// Forward a mutation through the engine (deadline + jittered transient
    /// retry) and reply with the outcome.
    Forward(
        MutationRequest,
        oneshot::Sender<Result<MutationReceipt, EngineError>>,
    ),
    /// Start the engine's reconnect loop (idempotent) — frames flow into the
    /// down-channel receiver taken via [`NativeNearEnd::take_down_channel`].
    StartDownChannel,
}

/// The `Send` facade over the engine: the engine's `Rc`/`RefCell` state runs on
/// a dedicated thread (current-thread runtime + `LocalSet`), and the rest of
/// the runtime talks to it through channels. The thread exits when the last
/// command sender (the owning [`crate::transport::RemoteAuthorityServer`])
/// drops.
pub(crate) struct NativeNearEnd {
    commands: mpsc::UnboundedSender<Command>,
    down_channel: Mutex<Option<mpsc::UnboundedReceiver<SequencedFrame>>>,
}

impl NativeNearEnd {
    pub(crate) fn spawn(
        client: reqwest::Client,
        base_url: String,
        token: Option<String>,
    ) -> Self {
        let (commands_tx, commands_rx) = mpsc::unbounded_channel();
        let (frames_tx, frames_rx) = mpsc::unbounded_channel();
        std::thread::Builder::new()
            .name("as-link-near-end".to_string())
            .spawn(move || engine_thread(client, base_url, token, frames_tx, commands_rx))
            .expect("spawn the authority-server link near-end thread");
        Self {
            commands: commands_tx,
            down_channel: Mutex::new(Some(frames_rx)),
        }
    }

    /// Forward a mutation through the engine: request deadline, jittered
    /// backoff on transient failures, permanent 4xx surfaced as
    /// `GatewayRejected` — the engine config is the only policy source.
    pub(crate) async fn forward(
        &self,
        request: MutationRequest,
    ) -> Result<MutationReceipt, RuntimeError> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.commands
            .send(Command::Forward(request, reply_tx))
            .map_err(|_| engine_gone())?;
        reply_rx
            .await
            .map_err(|_| engine_gone())?
            .map_err(engine_error_to_runtime)
    }

    /// Take the down-channel frame receiver and start the engine's reconnect
    /// loop. `None` after the first take (one consumer owns the channel).
    pub(crate) fn take_down_channel(
        &self,
    ) -> Option<mpsc::UnboundedReceiver<SequencedFrame>> {
        let receiver = self
            .down_channel
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .take()?;
        let _ = self.commands.send(Command::StartDownChannel);
        Some(receiver)
    }
}

fn engine_gone() -> RuntimeError {
    RuntimeError::retryable(
        RuntimeErrorCode::TransportDisconnected,
        "runtime↔authority-server link near-end is gone".to_string(),
    )
}

/// Map an engine verdict onto the link's error vocabulary: permanent (4xx /
/// contract breakage) → `GatewayRejected`; transient (network, deadline,
/// retries exhausted) → retryable `TransportDisconnected`.
fn engine_error_to_runtime(error: EngineError) -> RuntimeError {
    if error.is_permanent() {
        RuntimeError::new(
            RuntimeErrorCode::GatewayRejected,
            format!(
                "remote authority server rejected link request: {}",
                error.message
            ),
        )
    } else {
        RuntimeError::retryable(
            RuntimeErrorCode::TransportDisconnected,
            format!("runtime↔authority-server link transport error: {}", error.message),
        )
    }
}

fn engine_thread(
    client: reqwest::Client,
    base_url: String,
    token: Option<String>,
    frames: mpsc::UnboundedSender<SequencedFrame>,
    mut commands: mpsc::UnboundedReceiver<Command>,
) {
    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(error) => {
            tracing::error!(%error, "failed to build the link near-end runtime");
            return;
        }
    };
    let local = tokio::task::LocalSet::new();
    local.block_on(&runtime, async move {
        let engine = NearEnd::new(
            AuthorityLinkWire::new(LinkCoverage::Complete),
            Rc::new(NativeLinkTransport::new(client, base_url, token)),
            Rc::new(TokioScheduler::new()),
            Rc::new(ChannelFrameSink { frames }),
            Rc::new(NoopOutboxHooks),
            NearEndConfig::default(),
        );
        let mut started = false;
        while let Some(command) = commands.recv().await {
            match command {
                Command::Forward(request, reply) => {
                    let engine = engine.clone();
                    tokio::task::spawn_local(async move {
                        let _ = reply.send(engine.forward(request).await);
                    });
                }
                Command::StartDownChannel => {
                    if !started {
                        started = true;
                        tokio::task::spawn_local(engine.clone().run());
                    }
                }
            }
        }
        // The owner dropped: stop reconnecting; pending local tasks are
        // dropped with the LocalSet.
        engine.request_shutdown();
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sse_block_payload_reads_data_lines_and_skips_keep_alives() {
        assert_eq!(
            sse_block_payload("data: {\"seq\":7}\n").as_deref(),
            Some("{\"seq\":7}")
        );
        // Multi-line data concatenates; comments and field lines are dropped.
        assert_eq!(
            sse_block_payload(": keep-alive\nevent: frame\ndata: {\"a\":\ndata: 1}\n").as_deref(),
            Some("{\"a\":1}")
        );
        assert_eq!(sse_block_payload(": keep-alive\n"), None);
        assert_eq!(sse_block_payload(""), None);
    }

    #[tokio::test]
    async fn sse_payloads_splits_event_blocks_across_chunk_boundaries() {
        let chunks: Vec<Result<&'static [u8], std::convert::Infallible>> = vec![
            Ok(b"data: one".as_slice()),
            Ok(b"\n\ndata: tw".as_slice()),
            Ok(b"o\n\n: keep-alive\n\n".as_slice()),
        ];
        let payloads: Vec<String> =
            sse_payloads(futures_util::stream::iter(chunks)).collect().await;
        assert_eq!(payloads, vec!["one".to_string(), "two".to_string()]);
    }

    #[test]
    fn wire_stream_request_carries_the_resume_cursor() {
        let wire = AuthorityLinkWire::new(LinkCoverage::Complete);
        assert_eq!(wire.stream_request(None, None).url, LINK_SUBSCRIBE_PATH);
        assert_eq!(
            wire.stream_request(None, Some(42)).url,
            format!("{LINK_SUBSCRIBE_PATH}?afterSeq=42")
        );
    }

    #[test]
    fn jitter_stays_in_the_unit_interval() {
        let scheduler = TokioScheduler::new();
        for _ in 0..1000 {
            let j = scheduler.jitter();
            assert!((0.0..1.0).contains(&j), "{j}");
        }
    }
}
