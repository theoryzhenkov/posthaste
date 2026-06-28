use crate::LogEvent;

pub const API_REQUEST_COMPLETED: LogEvent = LogEvent::new("api.request.completed");
pub const API_REQUEST_FAILED: LogEvent = LogEvent::new("api.request.failed");
pub const API_REQUEST_STARTED: LogEvent = LogEvent::new("api.request.started");

pub const ACCOUNT_CREATE_COMPENSATION_FAILED: LogEvent =
    LogEvent::new("account.create.compensation_failed");
pub const ACCOUNT_SECRET_DELETE_FAILED: LogEvent = LogEvent::new("account.secret.delete_failed");

pub const CACHE_BODY_ADMISSION_EVALUATED: LogEvent =
    LogEvent::new("cache.body.admission_evaluated");
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
pub const CONFIG_INITIALIZED: LogEvent = LogEvent::new("config.initialized");
pub const DATABASE_OPENED: LogEvent = LogEvent::new("database.opened");
pub const DATABASE_CORRUPT_REPAIRED: LogEvent = LogEvent::new("database.corrupt.repaired");
pub const DAEMON_EVENT_MALFORMED: LogEvent = LogEvent::new("daemon.event.malformed");
pub const DESKTOP_BACKEND_STARTED: LogEvent = LogEvent::new("desktop.backend.started");
pub const DESKTOP_RELEASE_CHANNEL: LogEvent = LogEvent::new("desktop.release_channel");
pub const DESKTOP_FACTORY_RESET: LogEvent = LogEvent::new("desktop.factory_reset");
pub const FRONTEND_CONSOLE_OUTPUT: LogEvent = LogEvent::new("frontend.console.output");
pub const FRONTEND_ERROR_UNCAUGHT: LogEvent = LogEvent::new("frontend.error.uncaught");
pub const FRONTEND_ERROR_UNHANDLED_REJECTION: LogEvent =
    LogEvent::new("frontend.error.unhandled_rejection");
pub const HTTP_REQUEST_COMPLETED: LogEvent = LogEvent::new("http.request.completed");
pub const LOGGING_INITIALIZED: LogEvent = LogEvent::new("logging.initialized");
pub const SERVER_LISTENING: LogEvent = LogEvent::new("server.listening");
pub const LINK_SURFACE_SERVED: LogEvent = LogEvent::new("link.surface_served");
pub const SEND_FOLLOWUP_SYNC_TRIGGER_FAILED: LogEvent =
    LogEvent::new("send.followup_sync_trigger_failed");
pub const SEND_SENDER_CACHE_UPDATE_FAILED: LogEvent =
    LogEvent::new("send.sender_cache_update_failed");
pub const DRAFT_FOLLOWUP_SYNC_TRIGGER_FAILED: LogEvent =
    LogEvent::new("draft.followup_sync_trigger_failed");
pub const OUTBOX_FOLLOWUP_SYNC_TRIGGER_FAILED: LogEvent =
    LogEvent::new("outbox.followup_sync_trigger_failed");

pub const JMAP_EMAIL_DELTA_COMPLETED: LogEvent = LogEvent::new("jmap.email.delta.completed");
pub const JMAP_EMAIL_DELTA_STARTED: LogEvent = LogEvent::new("jmap.email.delta.started");
pub const JMAP_EMAIL_DELTA_UNAVAILABLE: LogEvent = LogEvent::new("jmap.email.delta.unavailable");
pub const JMAP_EMAIL_FULL_IDS_FETCHED: LogEvent = LogEvent::new("jmap.email.full_ids_fetched");
pub const JMAP_EMAIL_FULL_METADATA_PROGRESS: LogEvent =
    LogEvent::new("jmap.email.full_metadata_progress");
pub const JMAP_EMAIL_FULL_SNAPSHOT_FETCHED: LogEvent =
    LogEvent::new("jmap.email.full_snapshot_fetched");
pub const JMAP_MAILBOX_DELTA_COMPLETED: LogEvent = LogEvent::new("jmap.mailbox.delta.completed");
pub const JMAP_MAILBOX_DELTA_STARTED: LogEvent = LogEvent::new("jmap.mailbox.delta.started");
pub const JMAP_MAILBOX_DELTA_UNAVAILABLE: LogEvent =
    LogEvent::new("jmap.mailbox.delta.unavailable");
pub const JMAP_MAILBOX_FULL_IDS_FETCHED: LogEvent = LogEvent::new("jmap.mailbox.full_ids_fetched");
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
pub const IMAP_DISCOVERY_COMPLETED: LogEvent = LogEvent::new("imap.discovery_completed");
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
pub const STORE_SYNC_BATCH_APPLIED: LogEvent = LogEvent::new("store.sync.batch_applied");
pub const STORE_SYNC_BATCH_APPLYING: LogEvent = LogEvent::new("store.sync.batch_applying");

pub const SUPERVISOR_ACCOUNT_DISABLED: LogEvent = LogEvent::new("supervisor.account.disabled");
pub const SUPERVISOR_ACCOUNT_REMOVED: LogEvent = LogEvent::new("supervisor.account.removed");
pub const SUPERVISOR_ACCOUNT_RUNTIME_STARTED: LogEvent =
    LogEvent::new("supervisor.account.runtime_started");
pub const SUPERVISOR_ACCOUNT_RUNTIME_STOPPED: LogEvent =
    LogEvent::new("supervisor.account.runtime_stopped");
pub const SUPERVISOR_ACCOUNT_STATUS_PERSIST_FAILED: LogEvent =
    LogEvent::new("supervisor.account.status_persist_failed");
pub const SUPERVISOR_CONNECTION_ESTABLISHED: LogEvent =
    LogEvent::new("supervisor.connection.established");
pub const SUPERVISOR_CONNECTION_ESTABLISHING: LogEvent =
    LogEvent::new("supervisor.connection.establishing");
pub const SUPERVISOR_GATEWAY_CONNECTING: LogEvent = LogEvent::new("supervisor.gateway.connecting");
pub const SUPERVISOR_AUTOMATION_BACKFILL_COMPLETED: LogEvent =
    LogEvent::new("supervisor.automation_backfill.completed");
pub const SUPERVISOR_AUTOMATION_BACKFILL_FAILED: LogEvent =
    LogEvent::new("supervisor.automation_backfill.failed");
pub const SUPERVISOR_OUTBOX_FLUSH_FAILED: LogEvent =
    LogEvent::new("supervisor.outbox.flush_failed");
pub const SUPERVISOR_SYNC_COMPLETED: LogEvent = LogEvent::new("supervisor.sync.completed");
pub const SUPERVISOR_SYNC_FAILED: LogEvent = LogEvent::new("supervisor.sync.failed");
pub const SUPERVISOR_SYNC_STARTED: LogEvent = LogEvent::new("supervisor.sync.started");
pub const SUPERVISOR_SYNC_TRIGGER_IGNORED: LogEvent =
    LogEvent::new("supervisor.sync.trigger_ignored");
pub const SUPERVISOR_SYNC_TRIGGER_COALESCED: LogEvent =
    LogEvent::new("supervisor.sync.trigger_coalesced");
pub const SUPERVISOR_OAUTH_REFRESH_FAILED: LogEvent =
    LogEvent::new("supervisor.oauth.refresh_failed");
pub const SUPERVISOR_OAUTH_TOKEN_REFRESHED: LogEvent =
    LogEvent::new("supervisor.oauth.token_refreshed");

pub const DOMAIN_AUTOMATION_POST_SYNC_FAILED: LogEvent =
    LogEvent::new("domain.automation.post_sync_failed");
pub const DOMAIN_CACHE_CANDIDATE_POST_SYNC_FAILED: LogEvent =
    LogEvent::new("domain.cache_candidate.post_sync_failed");
pub const REV_LOG_APPEND_FAILED: LogEvent = LogEvent::new("rev_log.append_failed");
