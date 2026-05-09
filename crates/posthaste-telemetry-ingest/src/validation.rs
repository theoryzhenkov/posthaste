use serde_json::Value;
use uuid::Uuid;

use crate::{
    registry::{event_schema, EventSchema, FieldKind},
    schema::{TelemetryBatch, TelemetryMode},
};

#[derive(Debug, thiserror::Error)]
pub enum ValidationError {
    #[error("invalid telemetry payload")]
    InvalidPayload,
}

pub fn validate_batch(batch: &TelemetryBatch, max_events: usize) -> Result<(), ValidationError> {
    if batch.schema_version != 1 {
        return Err(ValidationError::InvalidPayload);
    }
    validate_ascii_token(&batch.app_version, 64)?;
    validate_client_day(&batch.client_day)?;

    match batch.telemetry_mode {
        TelemetryMode::Aggregate if batch.subject_id.is_some() => {
            return Err(ValidationError::InvalidPayload)
        }
        TelemetryMode::Product => match &batch.subject_id {
            Some(subject_id) => validate_ascii_token(subject_id, 64)?,
            None => return Err(ValidationError::InvalidPayload),
        },
        TelemetryMode::Aggregate => {}
    }

    if batch.events.is_empty() || batch.events.len() > max_events {
        return Err(ValidationError::InvalidPayload);
    }

    for event in &batch.events {
        let schema = event_schema(&event.name).ok_or(ValidationError::InvalidPayload)?;
        if batch.telemetry_mode == TelemetryMode::Aggregate && schema.product_only {
            return Err(ValidationError::InvalidPayload);
        }
        validate_event(schema, event.version, &event.event_id, &event.fields)?;
    }

    Ok(())
}

fn validate_event(
    schema: &EventSchema,
    version: u32,
    event_id: &str,
    fields: &std::collections::BTreeMap<String, Value>,
) -> Result<(), ValidationError> {
    if version != schema.version || Uuid::parse_str(event_id).is_err() || fields.len() > 16 {
        return Err(ValidationError::InvalidPayload);
    }

    for field in schema.fields.iter().filter(|field| field.required) {
        if !fields.contains_key(field.name) {
            return Err(ValidationError::InvalidPayload);
        }
    }

    for (name, value) in fields {
        let field = schema
            .fields
            .iter()
            .find(|field| field.name == name)
            .ok_or(ValidationError::InvalidPayload)?;
        validate_field_value(field.kind, value)?;
    }

    Ok(())
}

fn validate_field_value(kind: FieldKind, value: &Value) -> Result<(), ValidationError> {
    match kind {
        FieldKind::Enum(allowed) => {
            let Some(text) = value.as_str() else {
                return Err(ValidationError::InvalidPayload);
            };
            validate_ascii_token(text, 64)?;
            validate_no_banned_value(text)?;
            if allowed.contains(&text) {
                Ok(())
            } else {
                Err(ValidationError::InvalidPayload)
            }
        }
    }
}

fn validate_ascii_token(value: &str, max_len: usize) -> Result<(), ValidationError> {
    if value.is_empty()
        || value.len() > max_len
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_' | b'+'))
    {
        return Err(ValidationError::InvalidPayload);
    }
    validate_no_banned_value(value)
}

fn validate_client_day(value: &str) -> Result<(), ValidationError> {
    if value.len() != 10 {
        return Err(ValidationError::InvalidPayload);
    }
    let bytes = value.as_bytes();
    let date_shape = bytes[0..4].iter().all(u8::is_ascii_digit)
        && bytes[4] == b'-'
        && bytes[5..7].iter().all(u8::is_ascii_digit)
        && bytes[7] == b'-'
        && bytes[8..10].iter().all(u8::is_ascii_digit);
    if date_shape {
        Ok(())
    } else {
        Err(ValidationError::InvalidPayload)
    }
}

fn validate_no_banned_value(value: &str) -> Result<(), ValidationError> {
    let lower = value.to_ascii_lowercase();
    if lower.contains("@")
        || lower.contains("://")
        || lower.contains("token")
        || lower.contains("password")
        || lower.contains("secret")
        || lower.contains("bearer")
        || lower.contains("/home/")
        || lower.contains("/users/")
        || lower.contains("\\users\\")
    {
        Err(ValidationError::InvalidPayload)
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::schema::TelemetryBatch;

    #[test]
    fn valid_aggregate_payload_passes() {
        let batch = batch(json!({
            "schemaVersion": 1,
            "appVersion": "0.1.0-beta.1",
            "appChannel": "beta",
            "osFamily": "linux",
            "arch": "x86_64",
            "telemetryMode": "aggregate",
            "clientDay": "2026-05-09",
            "events": [{
                "name": "app.startup.completed",
                "version": 1,
                "eventId": "9fb18840-1a4b-4f0a-b94d-9c5e4a8c40c2",
                "fields": {
                    "duration_bucket": "s1_5",
                    "result": "ok",
                    "reason_bucket": "none"
                }
            }]
        }));

        validate_batch(&batch, 100).expect("valid payload should pass");
    }

    #[test]
    fn aggregate_payload_rejects_subject_id() {
        let batch = batch(json!({
            "schemaVersion": 1,
            "appVersion": "0.1.0-beta.1",
            "appChannel": "beta",
            "osFamily": "linux",
            "arch": "x86_64",
            "telemetryMode": "aggregate",
            "clientDay": "2026-05-09",
            "subjectId": "monthly_subject",
            "events": [{
                "name": "app.startup.completed",
                "version": 1,
                "eventId": "9fb18840-1a4b-4f0a-b94d-9c5e4a8c40c2",
                "fields": {
                    "duration_bucket": "s1_5",
                    "result": "ok",
                    "reason_bucket": "none"
                }
            }]
        }));

        assert!(validate_batch(&batch, 100).is_err());
    }

    #[test]
    fn payload_rejects_unknown_fields_before_storage() {
        let payload = json!({
            "schemaVersion": 1,
            "appVersion": "0.1.0-beta.1",
            "appChannel": "beta",
            "osFamily": "linux",
            "arch": "x86_64",
            "telemetryMode": "aggregate",
            "clientDay": "2026-05-09",
            "events": [{
                "name": "app.startup.completed",
                "version": 1,
                "eventId": "9fb18840-1a4b-4f0a-b94d-9c5e4a8c40c2",
                "fields": {
                    "duration_bucket": "s1_5",
                    "result": "ok",
                    "reason_bucket": "none",
                    "subject": "hello"
                }
            }]
        });
        let batch = batch(payload);

        assert!(validate_batch(&batch, 100).is_err());
    }

    fn batch(value: serde_json::Value) -> TelemetryBatch {
        serde_json::from_value(value).expect("fixture should deserialize")
    }
}
