use super::*;

impl TagReadStore for DatabaseStore {
    fn list_tags(&self, account_id: &AccountId) -> Result<Vec<TagSummary>, StoreError> {
        let connection = self.read_connection()?;
        let mut statement = connection
            .prepare_cached(
                "SELECT TRIM(mk.keyword) AS keyword,
                        COUNT(DISTINCT CASE WHEN m.is_read = 0 THEN m.id END) AS unread_messages,
                        COUNT(DISTINCT m.id) AS total_messages
                 FROM message_keyword_effective mk
                 JOIN message_effective m
                   ON m.account_id = mk.account_id
                  AND m.id = mk.message_id
                 WHERE mk.account_id = ?1
                   AND TRIM(mk.keyword) <> ''
                   AND TRIM(mk.keyword) NOT LIKE '$%'
                 GROUP BY TRIM(mk.keyword)
                 ORDER BY LOWER(TRIM(mk.keyword)), TRIM(mk.keyword)",
            )
            .map_err(sql_to_store_error)?;
        let rows = statement
            .query_map(params![account_id.as_str()], |row| {
                Ok(TagSummary {
                    name: row.get(0)?,
                    unread_messages: row.get(1)?,
                    total_messages: row.get(2)?,
                })
            })
            .map_err(sql_to_store_error)?;

        let mut tags = Vec::new();
        for row in rows {
            tags.push(row.map_err(sql_to_store_error)?);
        }
        Ok(tags)
    }
}
