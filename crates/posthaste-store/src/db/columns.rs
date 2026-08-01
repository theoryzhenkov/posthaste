use super::*;

pub(super) fn ensure_column(
    connection: &Connection,
    table_name: &str,
    column_name: &str,
    // Borrowed rather than `&'static`: a caller adding the same column to both
    // message planes builds the statement with `format!`.
    alter_sql: &str,
) -> Result<(), StoreError> {
    let mut statement = connection
        .prepare(&format!("PRAGMA table_info({table_name})"))
        .map_err(sql_to_store_error)?;
    let columns = statement
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(sql_to_store_error)?;
    for column in columns {
        if column.map_err(sql_to_store_error)? == column_name {
            return Ok(());
        }
    }
    connection
        .execute(alter_sql, [])
        .map_err(sql_to_store_error)?;
    Ok(())
}
