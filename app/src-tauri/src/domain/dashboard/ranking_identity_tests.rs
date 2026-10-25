use super::*;
use serde_json::json;

fn database() -> rusqlite::Connection {
    let conn = rusqlite::Connection::open_in_memory().unwrap();
    conn.execute_batch("create table profiles(id text,platform text,config_json text);
        create table daily_messages(day text,profile_id text,platform text,chat_id text,chat_name text,is_group integer,sender_id text,sender_name text,msg_type text,content text);").unwrap();
    for (profile, id) in [("one", "ou_me"), ("two", "ou_other_me")] {
        conn.execute(
            "insert into profiles values(?1,'feishu',?2)",
            params![
                profile,
                json!({"accountIdentity":{"userId":id},"remark":profile}).to_string()
            ],
        )
        .unwrap();
    }
    conn
}
fn message(
    conn: &rusqlite::Connection,
    profile: &str,
    chat: &str,
    sender: &str,
    name: &str,
    group: bool,
) {
    conn.execute("insert into daily_messages values('2026-10-25',?1,(select platform from profiles where id=?1),?2,?2,?3,?4,?5,'text','项目进度已确认')",params![profile,chat,group,sender,name]).unwrap();
}
#[test]
fn actual_open_id_recognizes_both_sides_without_renaming_sender_ids() {
    let conn = database();
    message(&conn, "one", "p2p", "ou_me", "真实昵称", false);
    message(&conn, "one", "p2p", "ou_peer", "同事", false);
    assert_eq!(
        chat_rank(&conn, "2026-10-25", "one").unwrap()[0]["count"],
        2
    );
    let top = speaker_top(&conn, "2026-10-25", "one").unwrap();
    assert_eq!(top.len(), 1);
    assert_eq!(top[0]["speaker"], "同事");
}
#[test]
fn one_sided_chats_remain_filtered_and_self_never_counts_as_other_side() {
    let conn = database();
    message(&conn, "one", "incoming", "ou_peer", "同事", false);
    message(&conn, "one", "outgoing", "ou_me", "真实昵称", false);
    assert!(chat_rank(&conn, "2026-10-25", "one").unwrap().is_empty());
    assert!(speaker_top(&conn, "2026-10-25", "one").unwrap().is_empty());
}
#[test]
fn aggregate_does_not_combine_sides_across_accounts_and_user_ids_are_profile_scoped() {
    let conn = database();
    message(&conn, "one", "same-chat", "ou_me", "自己", false);
    message(&conn, "two", "same-chat", "ou_peer", "同事", false);
    assert!(chat_rank(&conn, "2026-10-25", "aggregate")
        .unwrap()
        .is_empty());
    message(&conn, "two", "same-chat", "ou_other_me", "自己", false);
    message(
        &conn,
        "two",
        "same-chat",
        "ou_me",
        "另一个账号的联系人",
        false,
    );
    let top = speaker_top(&conn, "2026-10-25", "two").unwrap();
    assert_eq!(top.len(), 2);
    assert!(top.iter().any(|r| r["speaker"] == "另一个账号的联系人"));
}
#[test]
fn refreshed_cli_identity_repairs_existing_rows_and_legacy_self_aliases_still_work() {
    let conn = database();
    conn.execute(
        "update profiles set config_json=?1 where id='one'",
        [json!({"accountIdentity":{"userId":"wrong-old-id"}}).to_string()],
    )
    .unwrap();
    message(&conn, "one", "p2p", "ou_me", "真实昵称", false);
    message(&conn, "one", "p2p", "ou_peer", "同事", false);
    assert!(chat_rank(&conn, "2026-10-25", "one").unwrap().is_empty());
    crate::messages::account_identity::record_feishu_self_identity(&conn, "one", "ou_me").unwrap();
    assert_eq!(chat_rank(&conn, "2026-10-25", "one").unwrap().len(), 1);
    message(&conn, "one", "legacy", "me", "我", false);
    message(&conn, "one", "legacy", "peer", "联系人", false);
    assert_eq!(chat_rank(&conn, "2026-10-25", "one").unwrap().len(), 2);
}
#[test]
fn group_speaker_statistics_still_exclude_the_actual_account_owner() {
    let conn = database();
    message(&conn, "one", "group", "ou_me", "真实昵称", true);
    message(&conn, "one", "group", "ou_peer", "群成员", true);
    assert!(chat_rank(&conn, "2026-10-25", "one").unwrap().is_empty());
    let top = speaker_top(&conn, "2026-10-25", "one").unwrap();
    assert_eq!(top.len(), 1);
    assert_eq!(top[0]["speaker"], "群成员");
}

#[test]
fn dingtalk_open_ids_and_staff_ids_recognize_self_but_keep_two_sided_requirement() {
    let conn = database();
    conn.execute("update profiles set platform='dingtalk',config_json=?1 where id='one'",[json!({"accountIdentity":{"userId":"staff-me"},"dingtalkSelfIds":["D_me","staff-me","123"]}).to_string()]).unwrap();
    message(&conn, "one", "p2p", "D_peer", "同事", false);
    assert!(chat_rank(&conn, "2026-10-25", "one").unwrap().is_empty());
    assert!(speaker_top(&conn, "2026-10-25", "one").unwrap().is_empty());
    message(&conn, "one", "p2p", "D_me", "真实昵称", false);
    message(&conn, "one", "p2p", "123", "真实昵称", false);
    assert_eq!(
        chat_rank(&conn, "2026-10-25", "one").unwrap()[0]["count"],
        3
    );
    let top = speaker_top(&conn, "2026-10-25", "one").unwrap();
    assert_eq!(top.len(), 1);
    assert_eq!(top[0]["speaker"], "同事");
}
#[test]
fn dingtalk_group_speakers_and_aliases_are_scoped_to_the_current_profile() {
    let conn = database();
    conn.execute(
        "update profiles set platform='dingtalk',config_json=?1 where id='one'",
        [json!({"dingtalkSelfIds":["D_me"]}).to_string()],
    )
    .unwrap();
    conn.execute(
        "update profiles set platform='dingtalk',config_json=?1 where id='two'",
        [json!({"dingtalkSelfIds":["D_second"]}).to_string()],
    )
    .unwrap();
    message(&conn, "one", "group", "D_me", "本人", true);
    message(&conn, "one", "group", "D_peer", "群成员", true);
    assert!(chat_rank(&conn, "2026-10-25", "one").unwrap().is_empty());
    assert_eq!(
        speaker_top(&conn, "2026-10-25", "one").unwrap()[0]["speaker"],
        "群成员"
    );
    message(&conn, "two", "group", "D_me", "另一个账号的联系人", true);
    assert_eq!(
        speaker_top(&conn, "2026-10-25", "two").unwrap()[0]["speaker"],
        "另一个账号的联系人"
    );
}
