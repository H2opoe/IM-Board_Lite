use super::*;
use base64::Engine;
use ring::aead;
use std::{collections::VecDeque, sync::Mutex};
use zeroize::Zeroizing;

fn bot() -> credentials::Bot {
    credentials::Bot {
        id: "fixture-bot".into(),
        secret: Zeroizing::new("fixture-secret".into()),
    }
}
fn rpc(value: Value) -> Value {
    json!({"result":{"content":[{"type":"text","text":value.to_string()}]}})
}
fn config() -> Value {
    json!({"errcode":0,"list":[{"biz_type":"contact","url":"https://qyapi.weixin.qq.com/contact?token=fixture"},{"biz_type":"msg","url":"https://qyapi.weixin.qq.com/messages?token=fixture"}]})
}
struct Mock {
    replies: Mutex<VecDeque<Value>>,
    calls: Mutex<Vec<(String, Value)>>,
}
impl Mock {
    fn new(replies: Vec<Value>) -> Self {
        Self {
            replies: Mutex::new(replies.into()),
            calls: Mutex::new(vec![]),
        }
    }
}
impl Transport for Mock {
    fn post(
        &self,
        url: &str,
        body: Value,
    ) -> impl std::future::Future<Output = Result<Value>> + Send {
        self.calls.lock().unwrap().push((url.into(), body));
        std::future::ready(Ok(self
            .replies
            .lock()
            .unwrap()
            .pop_front()
            .expect("fixture response")))
    }
}
fn time_args() -> HashMap<String, String> {
    HashMap::from([
        ("start_time".into(), "2026-10-24 00:00:00".into()),
        ("end_time".into(), "2026-10-24 23:59:59".into()),
    ])
}
fn credentials_fixture(current: bool) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let bytes = [7u8; 32];
    std::fs::write(
        dir.path().join(".encryption_key"),
        base64::engine::general_purpose::STANDARD.encode(bytes),
    )
    .unwrap();
    let stored = json!({"id":"fixture-bot","secret":"fixture-secret"});
    let mut plain = serde_json::to_vec(&if current {
        json!({"bot":stored,"token":"ignored"})
    } else {
        stored
    })
    .unwrap();
    let key = aead::LessSafeKey::new(aead::UnboundKey::new(&aead::AES_256_GCM, &bytes).unwrap());
    key.seal_in_place_append_tag(
        aead::Nonce::assume_unique_for_key([3u8; 12]),
        aead::Aad::empty(),
        &mut plain,
    )
    .unwrap();
    let mut encrypted = vec![3u8; 12];
    encrypted.extend(plain);
    std::fs::write(
        dir.path().join(if current {
            "credentials.enc"
        } else {
            "bot.enc"
        }),
        encrypted,
    )
    .unwrap();
    dir
}
#[test]
fn credentials_both_formats_are_read_only_and_account_bound() {
    for current in [true, false] {
        let dir = credentials_fixture(current);
        let file = dir.path().join(if current {
            "credentials.enc"
        } else {
            "bot.enc"
        });
        let before = std::fs::read(&file).unwrap();
        let loaded = credentials::load(dir.path(), Some("fixture-bot")).unwrap();
        assert_eq!(&*loaded.secret, "fixture-secret");
        assert_eq!(std::fs::read(file).unwrap(), before);
        assert!(credentials::load(dir.path(), Some("other-account")).is_err());
    }
}
#[test]
fn corrupt_current_credentials_never_fall_back_to_legacy() {
    let dir = credentials_fixture(false);
    std::fs::write(dir.path().join("credentials.enc"), b"corrupt").unwrap();
    assert!(credentials::load(dir.path(), None).is_err());
}
#[cfg(unix)]
#[test]
fn credential_symlinks_are_rejected() {
    let dir = credentials_fixture(true);
    std::fs::rename(
        dir.path().join("credentials.enc"),
        dir.path().join("real.enc"),
    )
    .unwrap();
    std::os::unix::fs::symlink("real.enc", dir.path().join("credentials.enc")).unwrap();
    assert!(credentials::load(dir.path(), None).is_err());
}
#[test]
fn signatures_match_official_protocol_and_endpoints_reject_token_leaks() {
    assert_eq!(
        config_request(&bot(), 123, "mcp_fixture")["signature"],
        "96073bb0fe37d50583ddde29cae8ecadce865eb21441cbda6b86338d5b8f0c13"
    );
    assert!(official_endpoint(
        "https://qyapi.weixin.qq.com/api?token=fixture"
    ));
    for url in [
        "http://qyapi.weixin.qq.com/a",
        "https://weixin.qq.com.evil.test/a",
        "https://evilweixin.qq.com/a",
        "https://user@qyapi.weixin.qq.com/a",
        "https://qyapi.weixin.qq.com:444/a",
        "https://qyapi.weixin.qq.com/a#x",
    ] {
        assert!(!official_endpoint(url));
    }
}
#[tokio::test]
async fn directory_automatically_reads_all_pages_and_more_than_ten_people() {
    let people: Vec<Value> = (0..25)
        .map(|i| json!({"userid":format!("fixture-{i}")}))
        .collect();
    let mock = Mock::new(vec![
        config(),
        rpc(json!({"userlist":people[..12],"has_more":true,"next_cursor":"page2"})),
        rpc(json!({"userlist":people[12..],"has_more":false})),
    ]);
    let result = read(&mock, &bot(), "list-contacts", &HashMap::new())
        .await
        .unwrap();
    assert_eq!(result["userlist"].as_array().unwrap().len(), 25);
    let calls = mock.calls.lock().unwrap();
    assert_eq!(calls[1].1["params"]["name"], "get_userlist");
    assert_eq!(calls[1].1["params"]["arguments"], json!({}));
    assert_eq!(calls[2].1["params"]["arguments"]["cursor"], "page2");
}
#[tokio::test]
async fn malformed_or_looping_pages_are_errors_not_empty_success() {
    for replies in [
        vec![rpc(json!({"userlist":[],"has_more":true}))],
        vec![rpc(json!({"users":[]}))],
        vec![
            rpc(json!({"userlist":[],"next_cursor":"same"})),
            rpc(json!({"userlist":[],"next_cursor":"same"})),
        ],
    ] {
        let mock = Mock::new(replies);
        assert!(all_pages(
            &mock,
            "https://qyapi.weixin.qq.com/a",
            "get_userlist",
            json!({}),
            "userlist"
        )
        .await
        .is_err());
    }
}
#[tokio::test]
async fn message_reads_use_same_protocol_ids_and_explicit_chat_type() {
    let mock = Mock::new(vec![
        config(),
        rpc(json!({"messages":[{"msgid":"one"}],"next_cursor":"next"})),
        rpc(json!({"messages":[{"msgid":"two"}]})),
    ]);
    let mut args = time_args();
    args.insert("chat".into(), "opaque-contact-id".into());
    args.insert("chat_type".into(), "1".into());
    let result = read(&mock, &bot(), "fetch-messages", &args).await.unwrap();
    assert_eq!(result["messages"].as_array().unwrap().len(), 2);
    let calls = mock.calls.lock().unwrap();
    assert_eq!(calls[1].1["params"]["name"], "get_message");
    assert_eq!(
        calls[1].1["params"]["arguments"]["chatid"],
        "opaque-contact-id"
    );
    assert_eq!(calls[1].1["params"]["arguments"]["chat_type"], 1);
    assert_eq!(calls[2].1["params"]["arguments"]["cursor"], "next");
}
#[tokio::test]
async fn chats_use_directory_for_direct_classification_and_numeric_time() {
    let mock = Mock::new(vec![
        config(),
        rpc(json!({"userlist":[{"userid":"direct-id"}]})),
        rpc(
            json!({"chats":[{"chatid":"direct-id","last_msg_time":"2026-10-24 12:00:00"},{"chatid":"opaque-group","chat_name":"Project A"}]}),
        ),
    ]);
    let raw = read(&mock, &bot(), "list-chats", &time_args())
        .await
        .unwrap();
    let chats = chats_with_directory(&raw);
    assert_eq!(chats[0]["chatType"], 1);
    assert!(chats[0]["lastMessageTimestamp"].as_i64().is_some());
    assert_eq!(chats[1]["chatType"], 2);
}
#[test]
fn service_errors_never_echo_contact_or_message_content() {
    let error = rpc_payload(rpc(
        json!({"errcode":40058,"errmsg":"PRIVATE-CONTENT token=SECRET"}),
    ))
    .unwrap_err();
    assert!(error.message.contains("40058"));
    assert!(!format!("{error:?}").contains("PRIVATE"));
    assert!(!error.message.contains("SECRET"));
}
#[tokio::test]
#[ignore = "Explicitly authorized, count-only real-account directory verification"]
async fn approved_live_directory_count_only() {
    let config =
        std::env::var_os("IMBOARD_LIVE_WECOM_CONFIG_DIR").expect("explicit config required");
    let bot = credentials::load(std::path::Path::new(&config), None).unwrap();
    let raw = read(
        &transport::Http::new().unwrap(),
        &bot,
        "list-contacts",
        &HashMap::new(),
    )
    .await
    .unwrap();
    println!(
        "authorizedVisibleContactCount={}; hasMore=false",
        raw["userlist"].as_array().unwrap().len()
    );
}

#[tokio::test]
async fn directory_names_are_reused_for_messages_without_cross_account_leakage() {
    let cache = Mutex::new(names::Store::default());
    for (account, name) in [("account-a", "甲"), ("account-b", "乙")] {
        let mock = Mock::new(vec![
            config(),
            rpc(json!({"userlist":[{"userid":"same-id","name":name}]})),
            config(),
            rpc(
                json!({"messages":[{"userid":"same-id","send_time":"2026-10-24 12:00:00","msgtype":"text","text":{"content":"fixture"}}]}),
            ),
        ]);
        read_with_names(
            &mock,
            &bot(),
            &cache,
            account,
            "list-contacts",
            &HashMap::new(),
        )
        .await
        .unwrap();
        let mut args = time_args();
        args.insert("chat".into(), "group-id".into());
        args.insert("chat_type".into(), "2".into());
        let raw = read_with_names(&mock, &bot(), &cache, account, "fetch-messages", &args)
            .await
            .unwrap();
        let normalized = normalize_wecom_messages(&raw, &args, &json!({}));
        assert_eq!(normalized[0]["senderId"], "same-id");
        assert_eq!(normalized[0]["senderName"], name);
        let calls = mock.calls.lock().unwrap();
        assert_eq!(
            calls
                .iter()
                .filter(|(_, body)| body["params"]["name"] == "get_userlist")
                .count(),
            1
        );
    }
}
#[tokio::test]
async fn direct_message_reads_load_names_on_cache_miss() {
    let cache = Mutex::new(names::Store::default());
    let mock = Mock::new(vec![
        config(),
        rpc(json!({"userlist":[{"userid":"u1","name":"成员姓名"}]})),
        config(),
        rpc(json!({"messages":[]})),
    ]);
    let mut args = time_args();
    args.insert("chat".into(), "u1".into());
    args.insert("chat_type".into(), "1".into());
    let raw = read_with_names(
        &mock,
        &bot(),
        &cache,
        "new-account",
        "fetch-messages",
        &args,
    )
    .await
    .unwrap();
    assert_eq!(raw["sender_names"]["u1"], "成员姓名");
}
