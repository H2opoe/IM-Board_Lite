use rusqlite::params;

use crate::ai::clean_message_content_for_ai;
use crate::analysis::message_noise;

use super::types::LocalKeywordMessage;

const LOCAL_KEYWORD_MESSAGE_LIMIT: usize = 2000;

pub(super) fn load_local_keyword_messages(
    conn: &rusqlite::Connection,
    day: &str,
    profile_id: &str,
) -> anyhow::Result<Vec<LocalKeywordMessage>> {
    let mut stmt = conn.prepare(
        "select chat_id, msg_type, content, timestamp, is_group, raw_type, raw_json, platform from daily_messages
         where day = ?1 and profile_id = ?2 and trim(content) <> ''
         order by timestamp desc
         limit ?3",
    )?;
    let rows = stmt.query_map(
        params![day, profile_id, LOCAL_KEYWORD_MESSAGE_LIMIT as i64],
        |row| {
            let chat_id: String = row.get(0)?;
            let msg_type: String = row.get(1)?;
            let raw_type: Option<String> = row.get(5)?;
            let raw_json: Option<String> = row.get(6)?;
            let platform: String = row.get(7)?;
            let raw_content: String = row.get(2)?;
            let msg_type = crate::messages::message_type::resolve_stored_message_type(
                &platform,
                &msg_type,
                raw_type.as_deref(),
                raw_json.as_deref(),
                &raw_content,
            );
            let is_group = row.get::<_, i64>(4)? == 1;
            if message_noise::is_message_noise(Some(&msg_type), &raw_content, is_group) {
                return Ok(None);
            }
            let Some(content) = clean_message_content_for_ai(&msg_type, &raw_content) else {
                return Ok(None);
            };
            Ok(Some(LocalKeywordMessage {
                chat_id,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_local_keyword_messages_excludes_enterprise_client_notices() {
        let conn = rusqlite::Connection::open_in_memory().expect("in-memory sqlite");
        conn.execute_batch(include_str!("../../../migrations/001_init.sql"))
            .expect("schema");
        for (platform, app_name) in [
            ("feishu", "飞书"),
            ("dingtalk", "钉钉"),
            ("wecom", "企业微信"),
        ] {
            let profile_id = format!("profile-{platform}");
            for (index, content) in [
                format!("请升级{app_name}查看"),
                format!("{app_name}版本不支持显示内容，请升级后查看"),
                "客户确认交付方案，需要今天跟进处理".to_owned(),
            ]
            .into_iter()
            .enumerate()
            {
                let id = format!("msg-{platform}-{index}");
                conn.execute(
                    "insert into daily_messages(
                       id, day, profile_id, platform, chat_id, chat_name, is_group, sender_id,
                       sender_name, timestamp, time_text, msg_type, content, content_hash
                     ) values(?1, '2026-05-01', ?2, ?3, 'business-chat', '客户', 0,
                              'customer-1', '客户', ?4, '09:00', 'text', ?5, ?6)",
                    params![
                        id,
                        profile_id,
                        platform,
                        index as i64,
                        content,
                        format!("hash-{id}")
                    ],
                )
                .expect("message");
            }
            let messages =
                load_local_keyword_messages(&conn, "2026-05-01", &profile_id).expect("messages");
            assert_eq!(
                messages.len(),
                1,
                "{platform} compatibility notices must be excluded"
            );
            assert_eq!(messages[0].content, "客户确认交付方案，需要今天跟进处理");
        }
    }

    #[test]
    fn message_type_boundary_local_keyword_reader_repairs_old_metadata_without_reimport() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch(include_str!("../../../migrations/001_init.sql"))
            .unwrap();
        for platform in ["feishu", "dingtalk", "wecom"] {
            for (id, msg_type, raw_type, raw_json, content) in [
                (
                    "business",
                    "text",
                    Some("text"),
                    "{}",
                    "群公告与拍一拍提醒功能的交付时间需要确认",
                ),
                (
                    "raw-system",
                    "text",
                    Some("system"),
                    "{}",
                    "后台系统维护时间需要确认",
                ),
                (
                    "json-system",
                    "text",
                    None,
                    r#"{"raw":{"msgtype":"system"}}"#,
                    "后台配置时间需要确认",
                ),
                (
                    "old-file",
                    "file",
                    Some("system"),
                    "{}",
                    "后台需要提交invoice.pdf",
                ),
            ] {
                conn.execute("insert into daily_messages(id,day,profile_id,platform,chat_id,chat_name,is_group,sender_id,sender_name,timestamp,time_text,msg_type,content,raw_type,raw_json,content_hash) values(?1,'2026-05-01',?2,?2,'business-chat','项目群',1,'customer','客户',1,'09:00',?3,?4,?5,?6,?1)",params![format!("{platform}-{id}"),platform,msg_type,content,raw_type,raw_json]).unwrap();
            }
            let messages = load_local_keyword_messages(&conn, "2026-05-01", platform).unwrap();
            assert_eq!(messages.len(), 1, "{platform}");
            assert!(messages[0].content.contains("群公告与拍一拍提醒功能"));
        }
    }
}
