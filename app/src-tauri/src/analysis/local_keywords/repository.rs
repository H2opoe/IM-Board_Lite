use rusqlite::params;

use crate::ai::clean_message_content_for_ai;
use crate::analysis::message_noise;
use crate::messages::wechat_accounts::is_wechat_system_account;

use super::types::LocalKeywordMessage;

const LOCAL_KEYWORD_MESSAGE_LIMIT: usize = 2000;

pub(super) fn load_local_keyword_messages(
    conn: &rusqlite::Connection,
    day: &str,
    profile_id: &str,
) -> anyhow::Result<Vec<LocalKeywordMessage>> {
    let mut stmt = conn.prepare(
        "select chat_id, sender_id, msg_type, content, timestamp, is_group from daily_messages
         where day = ?1 and profile_id = ?2 and trim(content) <> ''
         order by timestamp desc
         limit ?3",
    )?;
    let rows = stmt.query_map(
        params![day, profile_id, LOCAL_KEYWORD_MESSAGE_LIMIT as i64],
        |row| {
            let chat_id: String = row.get(0)?;
            if is_wechat_system_account(&chat_id) {
                return Ok(None);
            }
            let sender_id: String = row.get(1)?;
            if is_wechat_system_account(&sender_id) {
                return Ok(None);
            }
            let msg_type: String = row.get(2)?;
            let raw_content: String = row.get(3)?;
            let is_group = row.get::<_, i64>(5)? == 1;
            if message_noise::is_message_noise(Some(&msg_type), &raw_content, is_group) {
                return Ok(None);
            }
            let Some(content) = clean_message_content_for_ai(&msg_type, &raw_content) else {
                return Ok(None);
            };
            Ok(Some(LocalKeywordMessage {
                chat_id,
                content,
                timestamp: row.get(4)?,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_local_keyword_messages_skips_wechat_system_accounts() {
        let conn = rusqlite::Connection::open_in_memory().expect("in-memory sqlite");
        conn.execute_batch(include_str!("../../../migrations/001_init.sql"))
            .expect("schema");
        for (id, chat_id, sender_id, content) in [
            (
                "msg-filehelper",
                "filehelper",
                "customer-1",
                "客户问报价什么时候确认？",
            ),
            (
                "msg-service",
                "notification_messages",
                "customer-1",
                "客户问报价什么时候确认？",
            ),
            (
                "msg-public",
                "gh_news",
                "customer-1",
                "客户问报价什么时候确认？",
            ),
            (
                "msg-system-sender",
                "wxid_customer_123",
                "weixin",
                "客户问报价什么时候确认？",
            ),
            (
                "msg-business",
                "wxid_customer_123",
                "customer-1",
                "客户问报价什么时候确认？",
            ),
        ] {
            conn.execute(
                "insert into daily_messages(
                   id, day, profile_id, platform, chat_id, chat_name, is_group, sender_id,
                   sender_name, timestamp, time_text, msg_type, content, content_hash
                 )
                 values(?1, '2026-05-01', 'profile-1', 'wechat', ?2, '客户', 0,
                        ?3, '客户', 1, '09:00', 'text', ?4, ?5)",
                rusqlite::params![id, chat_id, sender_id, content, format!("hash-{id}")],
            )
            .expect("message");
        }

        let messages =
            load_local_keyword_messages(&conn, "2026-05-01", "profile-1").expect("messages");

        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].chat_id, "wxid_customer_123");
    }
}
