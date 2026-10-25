//! Repair ID placeholders without replacing names carried by actual messages.
use rusqlite::{params, Connection};
use serde_json::{Map, Value};

pub(crate) fn repair_wecom_sender_names(
    conn: &Connection,
    profile_id: &str,
    contacts: &Value,
) -> rusqlite::Result<usize> {
    let mut names = Map::new();
    for contact in contacts.as_array().into_iter().flatten() {
        let id = contact
            .get("chatId")
            .and_then(Value::as_str)
            .filter(|s| !s.trim().is_empty());
        let name = contact
            .get("chatName")
            .and_then(Value::as_str)
            .filter(|s| !s.trim().is_empty());
        if let (Some(id), Some(name)) = (id, name) {
            if id != name {
                names.insert(id.into(), Value::String(name.into()));
            }
        }
    }
    if names.is_empty() {
        return Ok(0);
    }
    conn.execute(
        "with names as materialized (select key as id, value as name from json_each(?2))
         update daily_messages set sender_name = (select name from names where names.id = daily_messages.sender_id)
         where profile_id = ?1 and platform = 'wecom'
           and (trim(sender_name) = '' or sender_name = sender_id)
           and sender_id in (select id from names)",
        params![profile_id, Value::Object(names).to_string()],
    )
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn backfill_is_profile_scoped_preserves_real_names_and_handles_opaque_ids() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("create table daily_messages(id text primary key,profile_id text,platform text,sender_id text,sender_name text);
            insert into daily_messages values ('a','one','wecom','same-id','same-id'),('b','two','wecom','same-id','same-id'),
            ('c','one','wecom','same-id','消息中的姓名'),('d','one','feishu','same-id','same-id'),('e','one','wecom','unknown','unknown');").unwrap();
        conn.execute(
            "insert into daily_messages values('quoted','one','wecom',?1,'')",
            ["id.\"opaque"],
        )
        .unwrap();
        let contacts = json!([{"chatId":"same-id","chatName":"通讯录姓名"},{"chatId":"id.\"opaque","chatName":"特殊 ID 姓名"}]);
        assert_eq!(
            repair_wecom_sender_names(&conn, "one", &contacts).unwrap(),
            2
        );
        let name = |id: &str| {
            conn.query_row(
                "select sender_name from daily_messages where id=?1",
                [id],
                |r| r.get::<_, String>(0),
            )
            .unwrap()
        };
        assert_eq!(name("a"), "通讯录姓名");
        assert_eq!(name("b"), "same-id");
        assert_eq!(name("c"), "消息中的姓名");
        assert_eq!(name("d"), "same-id");
        assert_eq!(name("e"), "unknown");
        assert_eq!(name("quoted"), "特殊 ID 姓名");
        assert_eq!(
            repair_wecom_sender_names(&conn, "one", &contacts).unwrap(),
            0
        );
    }
}
