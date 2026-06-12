use super::*;

pub(super) fn cache_object_id_key(object_id: Option<&str>) -> &str {
    object_id.unwrap_or("")
}

pub(super) fn u64_to_i64(value: u64) -> Result<i64, StoreError> {
    i64::try_from(value).map_err(|_| StoreError::Failure("cache byte count too large".to_string()))
}

pub(super) fn i64_to_u64(value: i64) -> Result<u64, StoreError> {
    u64::try_from(value).map_err(|_| StoreError::Failure("negative cache byte count".to_string()))
}

pub(super) fn parse_cache_layer(value: String) -> Result<CacheLayer, rusqlite::Error> {
    CacheLayer::parse(&value).ok_or_else(|| {
        rusqlite::Error::FromSqlConversionFailure(
            0,
            rusqlite::types::Type::Text,
            Box::new(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("unknown cache layer {value}"),
            )),
        )
    })
}

pub(super) fn parse_cache_fetch_unit(value: String) -> Result<CacheFetchUnit, rusqlite::Error> {
    CacheFetchUnit::parse(&value).ok_or_else(|| {
        rusqlite::Error::FromSqlConversionFailure(
            0,
            rusqlite::types::Type::Text,
            Box::new(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("unknown cache fetch unit {value}"),
            )),
        )
    })
}

pub(super) fn parse_cache_object_state(value: String) -> Result<CacheObjectState, rusqlite::Error> {
    CacheObjectState::parse(&value).ok_or_else(|| {
        rusqlite::Error::FromSqlConversionFailure(
            0,
            rusqlite::types::Type::Text,
            Box::new(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("unknown cache object state {value}"),
            )),
        )
    })
}

pub(super) fn option_u64_to_i64(value: Option<u64>) -> Result<Option<i64>, StoreError> {
    value.map(u64_to_i64).transpose()
}

pub(super) fn optional_i64_to_u64(
    value: Option<i64>,
    column: usize,
) -> Result<Option<u64>, rusqlite::Error> {
    value.map(i64_to_u64).transpose().map_err(|err| {
        rusqlite::Error::FromSqlConversionFailure(
            column,
            rusqlite::types::Type::Integer,
            Box::new(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                err.to_string(),
            )),
        )
    })
}
