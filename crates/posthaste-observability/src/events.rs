use crate::LogEvent;

pub const ACCOUNT_CREATE_COMPENSATION_FAILED: LogEvent =
    LogEvent::new("account.create.compensation_failed");
pub const ACCOUNT_SECRET_DELETE_FAILED: LogEvent = LogEvent::new("account.secret.delete_failed");

pub const CACHE_BODY_ADMISSION_EVALUATED: LogEvent =
    LogEvent::new("cache.body.admission_evaluated");
pub const CACHE_BODY_BATCH_DEADLINE: LogEvent = LogEvent::new("cache.body.batch_deadline");
pub const CACHE_BODY_CANDIDATE_GENERATION_SKIPPED: LogEvent =
    LogEvent::new("cache.body.candidate_generation_skipped");
pub const CACHE_BODY_CANDIDATE_SCORED: LogEvent = LogEvent::new("cache.body.candidate_scored");
pub const CACHE_BODY_CANDIDATES_SCORED: LogEvent = LogEvent::new("cache.body.candidates_scored");
pub const CACHE_BODY_CANDIDATES_UPSERTED: LogEvent =
    LogEvent::new("cache.body.candidates_upserted");
pub const CACHE_BODY_DEFERRED_BY_LEASE: LogEvent = LogEvent::new("cache.body.deferred_by_lease");
pub const CACHE_BODY_FETCH_FAILED: LogEvent = LogEvent::new("cache.body.fetch_failed");
pub const CACHE_BODY_FETCH_STARTED: LogEvent = LogEvent::new("cache.body.fetch_started");
pub const CACHE_BODY_NO_CANDIDATES: LogEvent = LogEvent::new("cache.body.no_candidates");
pub const CACHE_BODY_PLAN_CREATED: LogEvent = LogEvent::new("cache.body.plan.created");
pub const CACHE_BODY_SKIPPED_DISABLED: LogEvent = LogEvent::new("cache.body.skipped_disabled");
pub const CACHE_BODY_STORED: LogEvent = LogEvent::new("cache.body.stored");
pub const CACHE_FETCH_COMPLETED: LogEvent = LogEvent::new("cache.fetch.completed");
pub const CACHE_FETCH_FAILED: LogEvent = LogEvent::new("cache.fetch.failed");
pub const CACHE_FETCH_NO_WORK: LogEvent = LogEvent::new("cache.fetch.no_work");
pub const CACHE_FETCH_SKIPPED_BUDGET: LogEvent = LogEvent::new("cache.fetch.skipped_budget");
pub const CACHE_FETCH_SKIPPED_NO_GATEWAY: LogEvent =
    LogEvent::new("cache.fetch.skipped_no_gateway");
pub const CACHE_FETCH_SKIPPED_NO_LEASE: LogEvent = LogEvent::new("cache.fetch.skipped_no_lease");
pub const CACHE_MAINTENANCE_FEEDBACK_RECORDED: LogEvent =
    LogEvent::new("cache.maintenance.feedback_recorded");
pub const CACHE_MAINTENANCE_LEASE_GRANTED: LogEvent =
    LogEvent::new("cache.maintenance.lease_granted");
pub const CACHE_MAINTENANCE_TRIGGER_FAILED: LogEvent =
    LogEvent::new("cache.maintenance.trigger_failed");
pub const CACHE_RESCORE_CANDIDATE_SCORED: LogEvent =
    LogEvent::new("cache.rescore.candidate_scored");
pub const CACHE_RESCORE_COMPLETED: LogEvent = LogEvent::new("cache.rescore.completed");
pub const CACHE_RESCORE_FAILED: LogEvent = LogEvent::new("cache.rescore.failed");
pub const CACHE_RESCORE_NO_CANDIDATES: LogEvent = LogEvent::new("cache.rescore.no_candidates");
pub const CACHE_RESCORE_STALE_QUEUE_FAILED: LogEvent =
    LogEvent::new("cache.rescore.stale_queue_failed");
pub const CACHE_RESCORE_STALE_QUEUED: LogEvent = LogEvent::new("cache.rescore.stale_queued");
pub const CACHE_SEARCH_VISIBILITY_RECORDED: LogEvent =
    LogEvent::new("cache.search_visibility.recorded");
pub const CACHE_SEARCH_VISIBILITY_RECORD_FAILED: LogEvent =
    LogEvent::new("cache.search_visibility.record_failed");
pub const CACHE_SEARCH_VISIBILITY_RESULT_COUNT_FAILED: LogEvent =
    LogEvent::new("cache.search_visibility.result_count_failed");
pub const CACHE_SEARCH_VISIBILITY_SCOPE_COUNT_FAILED: LogEvent =
    LogEvent::new("cache.search_visibility.scope_count_failed");

pub const CONFIG_DEFAULT_INITIALIZED: LogEvent = LogEvent::new("config.default_initialized");
pub const CONFIG_BOOTSTRAP_IMPORTED: LogEvent = LogEvent::new("config.bootstrap_imported");
pub const DATABASE_OPENED: LogEvent = LogEvent::new("database.opened");
pub const DATABASE_CORRUPT_REPAIRED: LogEvent = LogEvent::new("database.corrupt.repaired");
pub const DESKTOP_BACKEND_STARTED: LogEvent = LogEvent::new("desktop.backend.started");
pub const DESKTOP_RELEASE_CHANNEL: LogEvent = LogEvent::new("desktop.release_channel");
pub const DESKTOP_FACTORY_RESET: LogEvent = LogEvent::new("desktop.factory_reset");
pub const HTTP_REQUEST_COMPLETED: LogEvent = LogEvent::new("http.request.completed");
/// A 5xx left the `/v1` boundary. Carries the correlation id echoed to the
/// client and the server-internal cause (io/sql/runtime detail) that is kept
/// off the wire (RFC-L2-lifecycle D72 / M30). The correlation id joins this log
/// line to the sanitized response body.
pub const HTTP_INTERNAL_ERROR: LogEvent = LogEvent::new("http.internal_error");
/// An authz caveat check denied a request at the `/v1` boundary. Carries the
/// (non-sensitive) deny reason operators need — previously discarded (D72 / M30).
pub const HTTP_AUTHZ_DENIED: LogEvent = LogEvent::new("http.authz.denied");
/// A deliberate fail-closed abort fired via [`fail_closed!`]. The reason is
/// logged here before the process panics so the abort is diagnosable in the
/// operator log (RFC-L2-lifecycle D73 / M30).
pub const FAIL_CLOSED: LogEvent = LogEvent::new("fail_closed");
pub const LOGGING_INITIALIZED: LogEvent = LogEvent::new("logging.initialized");
pub const SERVER_LISTENING: LogEvent = LogEvent::new("server.listening");
pub const LINK_SURFACE_SERVED: LogEvent = LogEvent::new("link.surface_served");
pub const SEND_SENDER_CACHE_UPDATE_FAILED: LogEvent =
    LogEvent::new("send.sender_cache_update_failed");
/// A JMAP send's submission committed but the server neither applied nor
/// rejected the `onSuccessUpdateEmail` Drafts→Sent move (a silently-ignored
/// reference). Non-fatal — the message WAS submitted — but the outgoing copy
/// stays filed in Drafts, so the anomaly must be visible in the operator log.
pub const SEND_SENT_MOVE_NOT_APPLIED: LogEvent = LogEvent::new("send.sent_move_not_applied");
pub const OUTBOX_FOLLOWUP_SYNC_TRIGGER_FAILED: LogEvent =
    LogEvent::new("outbox.followup_sync_trigger_failed");

/// BE-H2: a transient-failing outbox op crossed the skip threshold and no
/// longer halts the account's flush drain.
pub const OUTBOX_TRANSIENT_OP_SKIPPED: LogEvent = LogEvent::new("outbox.transient_op_skipped");

pub const JMAP_EMAIL_DELTA_COMPLETED: LogEvent = LogEvent::new("jmap.email.delta.completed");
pub const JMAP_EMAIL_DELTA_STARTED: LogEvent = LogEvent::new("jmap.email.delta.started");
pub const JMAP_EMAIL_DELTA_UNAVAILABLE: LogEvent = LogEvent::new("jmap.email.delta.unavailable");
pub const JMAP_EMAIL_FULL_IDS_FETCHED: LogEvent = LogEvent::new("jmap.email.full_ids_fetched");
pub const JMAP_EMAIL_FULL_METADATA_PROGRESS: LogEvent =
    LogEvent::new("jmap.email.full_metadata_progress");
pub const JMAP_EMAIL_FULL_SNAPSHOT_FETCHED: LogEvent =
    LogEvent::new("jmap.email.full_snapshot_fetched");
/// The paginated full-snapshot `Email/query` could not be proven exhaustive
/// (server capped the result and did not advance, or the reported `total` was
/// not reached). The remote id set is treated as INCOMPLETE, so this cycle
/// upserts what it retrieved but refuses prune-by-absence — never delete local
/// mail against an id set that is not provably the full remote truth.
pub const JMAP_EMAIL_FULL_QUERY_INCOMPLETE: LogEvent =
    LogEvent::new("jmap.email.full_query_incomplete");
pub const JMAP_MAILBOX_DELTA_COMPLETED: LogEvent = LogEvent::new("jmap.mailbox.delta.completed");
pub const JMAP_MAILBOX_DELTA_STARTED: LogEvent = LogEvent::new("jmap.mailbox.delta.started");
pub const JMAP_MAILBOX_DELTA_UNAVAILABLE: LogEvent =
    LogEvent::new("jmap.mailbox.delta.unavailable");
pub const JMAP_MAILBOX_FULL_IDS_FETCHED: LogEvent = LogEvent::new("jmap.mailbox.full_ids_fetched");
/// DP-C3 mail-loss guard: a full `Mailbox/query` snapshot whose id set could not
/// be proven exhaustive. The snapshot upserts what it retrieved but refuses
/// mailbox prune-by-absence — a capped/empty listing must never cascade-delete
/// every local mailbox.
pub const JMAP_MAILBOX_FULL_QUERY_INCOMPLETE: LogEvent =
    LogEvent::new("jmap.mailbox.full_query_incomplete");
pub const JMAP_MAILBOX_FULL_SNAPSHOT_FETCHED: LogEvent =
    LogEvent::new("jmap.mailbox.full_snapshot_fetched");
pub const JMAP_SESSION_CONNECTING: LogEvent = LogEvent::new("jmap.session.connecting");
pub const JMAP_SESSION_ESTABLISHED: LogEvent = LogEvent::new("jmap.session.established");
pub const JMAP_SYNC_BATCH_FETCHED: LogEvent = LogEvent::new("jmap.sync.batch_fetched");
pub const JMAP_SYNC_FETCH_STARTED: LogEvent = LogEvent::new("jmap.sync.fetch_started");
pub const JMAP_SYNC_MAILBOX_FETCHED: LogEvent = LogEvent::new("jmap.sync.mailbox_fetched");
pub const JMAP_WEBSOCKET_CAPABILITY_AVAILABLE: LogEvent =
    LogEvent::new("jmap.websocket.capability_available");
pub const JMAP_WEBSOCKET_CAPABILITY_UNAVAILABLE: LogEvent =
    LogEvent::new("jmap.websocket.capability_unavailable");
pub const JMAP_WEBSOCKET_CONNECTION_ESTABLISHED: LogEvent =
    LogEvent::new("jmap.websocket.connection_established");
pub const JMAP_WEBSOCKET_CONNECTION_FAILED: LogEvent =
    LogEvent::new("jmap.websocket.connection_failed");
pub const JMAP_WEBSOCKET_CONNECTION_OPENING: LogEvent =
    LogEvent::new("jmap.websocket.connection_opening");

pub const PUSH_CONNECTED: LogEvent = LogEvent::new("push.connected");
pub const PUSH_DISCONNECTED: LogEvent = LogEvent::new("push.disconnected");
pub const PUSH_FALLING_BACK: LogEvent = LogEvent::new("push.falling_back");
pub const PUSH_FALLBACK_CYCLED_TO_PRIMARY: LogEvent =
    LogEvent::new("push.fallback_cycled_to_primary");
pub const PUSH_FALLBACK_TRIGGERED: LogEvent = LogEvent::new("push.fallback_triggered");
pub const PUSH_NOTIFICATION_RECEIVED: LogEvent = LogEvent::new("push.notification_received");
pub const PUSH_RECONNECT_BACKOFF: LogEvent = LogEvent::new("push.reconnect_backoff");
pub const PUSH_TERMINAL: LogEvent = LogEvent::new("push.terminal");
pub const PUSH_WS_KEEPALIVE_FAILED: LogEvent = LogEvent::new("push.ws.keepalive_failed");
pub const PUSH_TRANSPORT_OPEN_FAILED: LogEvent = LogEvent::new("push.transport_open_failed");
pub const PUSH_TRANSPORT_UNSUPPORTED: LogEvent = LogEvent::new("push.transport_unsupported");
pub const PUSH_TRANSPORT_NEGOTIATED: LogEvent = LogEvent::new("push.transport_negotiated");
pub const PUSH_WS_STREAM_ENDED: LogEvent = LogEvent::new("push.ws.stream_ended");
pub const PUSH_WS_STREAM_ERROR: LogEvent = LogEvent::new("push.ws.stream_error");
pub const PUSH_WS_STREAM_OPENING: LogEvent = LogEvent::new("push.ws.stream_opening");
pub const PUSH_SSE_CONNECTION_FAILED: LogEvent = LogEvent::new("push.sse.connection_failed");
pub const PUSH_SSE_STREAM_OPENING: LogEvent = LogEvent::new("push.sse.stream_opening");

pub const IMAP_IDLE_CONNECT_FAILED: LogEvent = LogEvent::new("imap.idle.connect_failed");
pub const IMAP_IDLE_DISCONNECTED: LogEvent = LogEvent::new("imap.idle.disconnected");
pub const IMAP_IDLE_MAILBOX_MISSING: LogEvent = LogEvent::new("imap.idle.mailbox_missing");
pub const IMAP_IDLE_PERIODIC_POLL_ONLY: LogEvent = LogEvent::new("imap.idle.periodic_poll_only");
pub const IMAP_IDLE_PUSH_ENABLED: LogEvent = LogEvent::new("imap.idle.push_enabled");
pub const IMAP_IDLE_RETURNED: LogEvent = LogEvent::new("imap.idle.returned");
pub const IMAP_IDLE_REJECT_BACKOFF: LogEvent = LogEvent::new("imap.idle.reject_backoff");
pub const IMAP_SESSION_CONNECTED: LogEvent = LogEvent::new("imap.session.connected");
pub const IMAP_SESSION_CONNECT_FAILED: LogEvent = LogEvent::new("imap.session.connect_failed");
pub const IMAP_SESSION_DROPPED: LogEvent = LogEvent::new("imap.session.dropped");
pub const IMAP_SESSION_POISONED: LogEvent = LogEvent::new("imap.session.poisoned");
pub const IMAP_DISCOVERY_COMPLETED: LogEvent = LogEvent::new("imap.discovery_completed");
pub const IMAP_DRAFT_DELETE_ALREADY_GONE: LogEvent =
    LogEvent::new("imap.draft.delete_already_gone");
pub const IMAP_MAILBOX_HEADER_FETCH_COMPLETED: LogEvent =
    LogEvent::new("imap.mailbox.header_fetch_completed");
pub const IMAP_MAILBOX_HEADER_FETCH_PROGRESS: LogEvent =
    LogEvent::new("imap.mailbox.header_fetch_progress");
pub const IMAP_MAILBOX_HEADER_FETCH_SORTED: LogEvent =
    LogEvent::new("imap.mailbox.header_fetch_sorted");
pub const IMAP_MAILBOX_HEADER_FETCH_STARTED: LogEvent =
    LogEvent::new("imap.mailbox.header_fetch_started");
pub const IMAP_MAILBOX_SYNC_PLAN_DETAIL: LogEvent = LogEvent::new("imap.mailbox.sync_plan_detail");
pub const IMAP_MAILBOX_SYNC_PLANNED: LogEvent = LogEvent::new("imap.mailbox.sync_planned");
pub const IMAP_MAILBOX_UID_DELTA_SEARCH_COMPLETED: LogEvent =
    LogEvent::new("imap.mailbox.uid_delta_search_completed");
pub const IMAP_MAILBOX_UID_SEARCH_COMPLETED: LogEvent =
    LogEvent::new("imap.mailbox.uid_search_completed");
pub const IMAP_SMTP_SENT_APPEND_FAILED: LogEvent = LogEvent::new("imap.smtp.sent_append_failed");
pub const IMAP_SMTP_SENT_MAILBOX_MISSING: LogEvent =
    LogEvent::new("imap.smtp.sent_mailbox_missing");
pub const IMAP_SYNC_DISCOVERY_COMPLETED: LogEvent = LogEvent::new("imap.sync.discovery_completed");
pub const IMAP_SYNC_FETCH_COMPLETED: LogEvent = LogEvent::new("imap.sync.fetch_completed");
pub const IMAP_SYNC_FETCH_STARTED: LogEvent = LogEvent::new("imap.sync.fetch_started");

pub const STORE_CACHE_ORPHANS_PRUNED: LogEvent = LogEvent::new("store.cache.orphans_pruned");
pub const STORE_CACHE_STRUCTURAL_BODY_REPAIRED: LogEvent =
    LogEvent::new("store.cache.structural_body_repaired");
/// The full-snapshot prune-by-absence floor guard tripped: the remote id set
/// was empty or drastically smaller than the local store, so pruning was
/// refused and the local store preserved (a transiently-empty-but-`Ok` remote
/// query, or an id set that slipped past the completeness check, must never
/// silently wipe local mail). A legitimate mass deletion must arrive via an
/// explicit full-resync signal, not an unbounded absence-prune.
pub const STORE_SYNC_ABSENCE_PRUNE_REFUSED: LogEvent =
    LogEvent::new("store.sync.absence_prune_refused");
pub const STORE_SYNC_BATCH_APPLIED: LogEvent = LogEvent::new("store.sync.batch_applied");
pub const STORE_SYNC_BATCH_APPLYING: LogEvent = LogEvent::new("store.sync.batch_applying");
/// The deferred post-startup body-cache repair (RFC-L2-lifecycle N15 / M27
/// sub-unit (b)) failed. Non-fatal — the store already opened and is serving
/// real reads/writes; the repair is idempotent and simply did not run this
/// time (a future retry or the next startup catches it up).
pub const STORE_STARTUP_BODY_CACHE_REPAIR_FAILED: LogEvent =
    LogEvent::new("store.startup.body_cache_repair_failed");

/// The deferred post-startup address-book backfill failed. Non-fatal — the
/// store is already serving, the backfill is idempotent, and ingest maintains
/// the book incrementally regardless; a future retry or the next startup
/// catches it up.
pub const STORE_STARTUP_ADDRESS_BOOK_BACKFILL_FAILED: LogEvent =
    LogEvent::new("store.startup.address_book_backfill_failed");

/// The deferred post-startup full-text-index backfill (the FTS5 `rebuild`
/// that repopulates `message_fts` after the body-indexing migration dropped
/// the old header-only index) failed. Non-fatal — the store is serving and
/// the trigger-maintained index stays consistent for NEW writes; the rebuild
/// is idempotent and re-runs on the next startup, but until it succeeds text
/// search on the upgraded database misses pre-upgrade mail.
pub const STORE_STARTUP_MESSAGE_FTS_BACKFILL_FAILED: LogEvent =
    LogEvent::new("store.startup.message_fts_backfill_failed");

/// The deferred post-startup full-text-index backfill ran the one-time FTS5
/// `rebuild` (upgrade path only: messages existed while the index was empty).
pub const STORE_STARTUP_MESSAGE_FTS_BACKFILL_COMPLETED: LogEvent =
    LogEvent::new("store.startup.message_fts_backfill_completed");

pub const SUPERVISOR_ACCOUNT_DISABLED: LogEvent = LogEvent::new("supervisor.account.disabled");
pub const SUPERVISOR_ACCOUNT_REMOVED: LogEvent = LogEvent::new("supervisor.account.removed");
pub const SUPERVISOR_ACCOUNT_RUNTIME_STARTED: LogEvent =
    LogEvent::new("supervisor.account.runtime_started");
pub const SUPERVISOR_ACCOUNT_RUNTIME_STOPPED: LogEvent =
    LogEvent::new("supervisor.account.runtime_stopped");
pub const SUPERVISOR_ACCOUNT_STATUS_PERSIST_FAILED: LogEvent =
    LogEvent::new("supervisor.account.status_persist_failed");
pub const SUPERVISOR_ACCOUNT_PANICKED: LogEvent = LogEvent::new("supervisor.account.panicked");
pub const SUPERVISOR_ACCOUNT_EXITED_UNEXPECTEDLY: LogEvent =
    LogEvent::new("supervisor.account.exited_unexpectedly");
pub const SUPERVISOR_ACCOUNT_RESTARTING: LogEvent = LogEvent::new("supervisor.account.restarting");
pub const SUPERVISOR_ACCOUNT_HALTED: LogEvent = LogEvent::new("supervisor.account.halted");
pub const SUPERVISOR_ACCOUNT_STOP_ESCALATED: LogEvent =
    LogEvent::new("supervisor.account.stop_escalated");
pub const SUPERVISOR_CONNECTION_ESTABLISHED: LogEvent =
    LogEvent::new("supervisor.connection.established");
pub const SUPERVISOR_CONNECTION_ESTABLISHING: LogEvent =
    LogEvent::new("supervisor.connection.establishing");
pub const SUPERVISOR_GATEWAY_CONNECTING: LogEvent = LogEvent::new("supervisor.gateway.connecting");
pub const SUPERVISOR_AUTOMATION_BACKFILL_COMPLETED: LogEvent =
    LogEvent::new("supervisor.automation_backfill.completed");
pub const SUPERVISOR_AUTOMATION_BACKFILL_FAILED: LogEvent =
    LogEvent::new("supervisor.automation_backfill.failed");
pub const SUPERVISOR_SNOOZE_AUTO_RETURNED: LogEvent =
    LogEvent::new("supervisor.snooze.auto_returned");
pub const SUPERVISOR_SCHEDULED_SEND_DUE: LogEvent = LogEvent::new("supervisor.scheduled_send.due");
pub const SUPERVISOR_SCHEDULED_SEND_PROBE_FAILED: LogEvent =
    LogEvent::new("supervisor.scheduled_send.probe_failed");
pub const SUPERVISOR_SYNC_COMPLETED: LogEvent = LogEvent::new("supervisor.sync.completed");
pub const SUPERVISOR_SYNC_FAILED: LogEvent = LogEvent::new("supervisor.sync.failed");
pub const SUPERVISOR_SYNC_STARTED: LogEvent = LogEvent::new("supervisor.sync.started");
pub const SUPERVISOR_SYNC_TRIGGER_COALESCED: LogEvent =
    LogEvent::new("supervisor.sync.trigger_coalesced");
pub const SUPERVISOR_OAUTH_REFRESH_FAILED: LogEvent =
    LogEvent::new("supervisor.oauth.refresh_failed");
pub const SUPERVISOR_OAUTH_TOKEN_REFRESHED: LogEvent =
    LogEvent::new("supervisor.oauth.token_refreshed");
/// A select!-loop arm's bounded call (RFC-L2-lifecycle D66 / M26) exceeded
/// its `tokio::time::timeout` budget — the account is marked `Degraded` and
/// the loop moves on to its next tick/command (the M21 watchdog owns
/// lifecycle, not this per-arm backstop).
pub const SUPERVISOR_ARM_TIMEOUT: LogEvent = LogEvent::new("supervisor.arm.timeout");

pub const DOMAIN_AUTOMATION_POST_SYNC_FAILED: LogEvent =
    LogEvent::new("domain.automation.post_sync_failed");
pub const DOMAIN_CACHE_CANDIDATE_POST_SYNC_FAILED: LogEvent =
    LogEvent::new("domain.cache_candidate.post_sync_failed");
pub const REV_LOG_APPEND_FAILED: LogEvent = LogEvent::new("rev_log.append_failed");

// Automation rule engine (RFC-L2-scripting S5).
pub const RULE_ENGINE_STARTED: LogEvent = LogEvent::new("rule.engine.started");
pub const RULE_EVALUATION_FAILED: LogEvent = LogEvent::new("rule.evaluation_failed");
pub const RULE_ACTION_APPLY_FAILED: LogEvent = LogEvent::new("rule.action.apply_failed");
pub const RULE_NOTIFY: LogEvent = LogEvent::new("rule.notify");
pub const RULE_FIRED: LogEvent = LogEvent::new("rule.fired");
pub const RULE_WEBHOOK_DELIVERED: LogEvent = LogEvent::new("rule.webhook.delivered");
pub const RULE_EXEC_COMPLETED: LogEvent = LogEvent::new("rule.exec.completed");
pub const RULE_DELIVERY_FAILED: LogEvent = LogEvent::new("rule.delivery_failed");
