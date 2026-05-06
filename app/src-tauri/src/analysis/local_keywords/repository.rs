use rusqlite::params;

use crate::ai::clean_message_content_for_ai;

use super::types::LocalKeywordMessage;

const LOCAL_KEYWORD_MESSAGE_LIMIT: usize = 2000;

pub(super) fn load_local_keyword_messages(
    conn: &rusqlite::Connection,
    day: &str,
    profile_id: &str,
) -> anyhow::Result<Vec<LocalKeywordMessage>> {
    let mut stmt = conn.prepare(
        "select chat_id, msg_type, content, timestamp from daily_messages
         where day = ?1 and profile_id = ?2 and trim(content) <> ''
         order by timestamp desc
         limit ?3",
    )?;
    let rows = stmt.query_map(
        params![day, profile_id, LOCAL_KEYWORD_MESSAGE_LIMIT as i64],
        |row| {
            let msg_type: String = row.get(1)?;
            let raw_content: String = row.get(2)?;
            let Some(content) = clean_message_content_for_ai(&msg_type, &raw_content) else {
                return Ok(None);
            };
            Ok(Some(LocalKeywordMessage {
                chat_id: row.get(0)?,
                content,
                timestamp: row.get(3)?,
            }))
        },
    )?;

    let mut messages = Vec::new();
    for row in rows {
        if let Some(message) = row? {
            messages.push(message);
        }
    }
    Ok(messages)
}
