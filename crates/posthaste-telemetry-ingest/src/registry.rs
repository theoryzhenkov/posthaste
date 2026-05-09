#[derive(Clone, Copy, Debug)]
pub struct EventSchema {
    pub name: &'static str,
    pub version: u32,
    pub product_only: bool,
    pub fields: &'static [FieldSchema],
}

#[derive(Clone, Copy, Debug)]
pub struct FieldSchema {
    pub name: &'static str,
    pub kind: FieldKind,
    pub required: bool,
}

#[derive(Clone, Copy, Debug)]
pub enum FieldKind {
    Enum(&'static [&'static str]),
}

pub fn event_schema(name: &str) -> Option<&'static EventSchema> {
    EVENT_SCHEMAS.iter().find(|schema| schema.name == name)
}

const RESULT: &[&str] = &["ok", "failed", "cancelled"];
const REASON: &[&str] = &[
    "none",
    "auth",
    "network",
    "provider_rejected",
    "local_store",
    "timeout",
    "schema",
    "quota",
    "consent",
    "unknown",
];
const DURATION: &[&str] = &["lt_1s", "s1_5", "s5_15", "s15_60", "m1_5", "gt_5m"];
const COUNT: &[&str] = &[
    "zero",
    "n1",
    "n2_5",
    "n6_20",
    "n21_100",
    "n101_1000",
    "gt_1000",
];
const DRIVER: &[&str] = &["jmap", "imap_smtp", "mock"];
const RECEIVE_PROTOCOL: &[&str] = &["jmap", "imap"];
const TRIGGER: &[&str] = &["startup", "manual", "push", "poll", "mutation", "unknown"];
const ROUTE_FAMILY: &[&str] = &[
    "settings",
    "accounts",
    "mailboxes",
    "messages",
    "conversations",
    "compose",
    "search",
    "events",
    "assets",
    "unknown",
];
const METHOD_FAMILY: &[&str] = &["get", "post", "patch", "delete", "other"];
const STATUS_CLASS: &[&str] = &["2xx", "3xx", "4xx", "5xx", "unknown"];
const PAYLOAD_SIZE: &[&str] = &[
    "zero",
    "lt_1kb",
    "kb1_10",
    "kb10_100",
    "kb100_256",
    "gt_256kb",
];
const QUERY_SHAPE: &[&str] = &[
    "empty",
    "simple_term",
    "field_filter",
    "boolean",
    "advanced",
];
const CACHE_LAYER: &[&str] = &["memory", "sqlite", "body_blob", "raw_message"];
const CACHE_RESULT: &[&str] = &["hit", "miss", "stale", "error"];
const DROP_REASON: &[&str] = &[
    "consent_off",
    "schema",
    "quota",
    "spool_full",
    "too_old",
    "unknown",
];
const REJECT_REASON: &[&str] = &[
    "unknown_event",
    "unknown_field",
    "invalid_value",
    "banned_value",
    "too_large",
    "unknown",
];

static EVENT_SCHEMAS: &[EventSchema] = &[
    EventSchema {
        name: "app.startup.completed",
        version: 1,
        product_only: false,
        fields: &[
            field("duration_bucket", FieldKind::Enum(DURATION), true),
            field("result", FieldKind::Enum(RESULT), true),
            field("reason_bucket", FieldKind::Enum(REASON), true),
        ],
    },
    EventSchema {
        name: "sync.cycle.completed",
        version: 1,
        product_only: false,
        fields: &[
            field("driver_family", FieldKind::Enum(DRIVER), true),
            field("receive_protocol", FieldKind::Enum(RECEIVE_PROTOCOL), true),
            field("trigger", FieldKind::Enum(TRIGGER), true),
            field("duration_bucket", FieldKind::Enum(DURATION), true),
            field("result", FieldKind::Enum(RESULT), true),
            field("reason_bucket", FieldKind::Enum(REASON), true),
            field("item_count_bucket", FieldKind::Enum(COUNT), true),
        ],
    },
    EventSchema {
        name: "api.request.completed",
        version: 1,
        product_only: false,
        fields: &[
            field("route_family", FieldKind::Enum(ROUTE_FAMILY), true),
            field("method_family", FieldKind::Enum(METHOD_FAMILY), true),
            field("duration_bucket", FieldKind::Enum(DURATION), true),
            field("status_class", FieldKind::Enum(STATUS_CLASS), true),
            field("payload_size_bucket", FieldKind::Enum(PAYLOAD_SIZE), true),
            field("result", FieldKind::Enum(RESULT), true),
        ],
    },
    EventSchema {
        name: "search.query.completed",
        version: 1,
        product_only: false,
        fields: &[
            field("duration_bucket", FieldKind::Enum(DURATION), true),
            field("result_count_bucket", FieldKind::Enum(COUNT), true),
            field("query_shape", FieldKind::Enum(QUERY_SHAPE), true),
            field("result", FieldKind::Enum(RESULT), true),
            field("reason_bucket", FieldKind::Enum(REASON), true),
        ],
    },
    EventSchema {
        name: "cache.lookup.completed",
        version: 1,
        product_only: false,
        fields: &[
            field("cache_layer", FieldKind::Enum(CACHE_LAYER), true),
            field("result", FieldKind::Enum(CACHE_RESULT), true),
            field("duration_bucket", FieldKind::Enum(DURATION), true),
            field("size_bucket", FieldKind::Enum(PAYLOAD_SIZE), true),
        ],
    },
    EventSchema {
        name: "telemetry.upload.completed",
        version: 1,
        product_only: false,
        fields: &[
            field("batch_size_bucket", FieldKind::Enum(COUNT), true),
            field("result", FieldKind::Enum(RESULT), true),
            field("reason_bucket", FieldKind::Enum(REASON), true),
        ],
    },
    EventSchema {
        name: "telemetry.event.dropped",
        version: 1,
        product_only: false,
        fields: &[
            field("drop_reason", FieldKind::Enum(DROP_REASON), true),
            field("count_bucket", FieldKind::Enum(COUNT), true),
        ],
    },
    EventSchema {
        name: "telemetry.schema.rejected",
        version: 1,
        product_only: false,
        fields: &[
            field(
                "event_family",
                FieldKind::Enum(&[
                    "app",
                    "sync",
                    "api",
                    "search",
                    "cache",
                    "telemetry",
                    "unknown",
                ]),
                true,
            ),
            field("reject_reason", FieldKind::Enum(REJECT_REASON), true),
            field("count_bucket", FieldKind::Enum(COUNT), true),
        ],
    },
    EventSchema {
        name: "profile.provider.recorded",
        version: 1,
        product_only: true,
        fields: &[
            field(
                "built_in_provider",
                FieldKind::Enum(&[
                    "fastmail",
                    "gmail",
                    "icloud",
                    "outlook",
                    "generic",
                    "development",
                ]),
                true,
            ),
            field("driver_family", FieldKind::Enum(DRIVER), true),
            field(
                "auth_family",
                FieldKind::Enum(&[
                    "oauth",
                    "app_credential",
                    "api_credential",
                    "manual_credential",
                    "development",
                ]),
                true,
            ),
            field("account_count_bucket", FieldKind::Enum(COUNT), true),
        ],
    },
];

const fn field(name: &'static str, kind: FieldKind, required: bool) -> FieldSchema {
    FieldSchema {
        name,
        kind,
        required,
    }
}
