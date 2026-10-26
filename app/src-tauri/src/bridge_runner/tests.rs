use super::*;

#[cfg(unix)]
#[tokio::test]
async fn official_cli_timeout_unregisters_and_stops_waiting() {
    let active_bridges = BridgeRegistry::default();
    let mut command = Command::new("sh");
    command::configure_process_supervision(&mut command);
    command.arg("-c").arg("sleep 2");
    let child = command.spawn().expect("spawn timeout fixture");

    let error =
        wait_for_official_cli_output(child, Some(&active_bridges), Duration::from_millis(20))
            .await
            .expect_err("command should time out");

    assert!(error.to_string().contains("已请求终止无响应进程"));
    assert!(active_bridges.is_empty());
}

#[test]
fn classifies_feishu_b2c_app_history_error() {
    let stdout = r#"{
          "ok": false,
          "identity": "user",
          "error": {
            "type": "api_error",
            "code": 231204,
            "message": "HTTP 400: The app type is not supported, ext=b2c app not support",
            "detail": null
          }
        }"#;

    let error = classify_feishu_cli_error(stdout, "").expect("classified");
    assert_eq!(error.code, "FEISHU_B2C_APP_UNSUPPORTED");
    assert!(error.recoverable);
    assert!(error.message.contains("已跳过"));
}

#[test]
fn accepts_verified_feishu_auth_status_without_token_status() {
    let raw = serde_json::json!({
        "appId": "cli_xxx",
        "brand": "feishu",
        "verified": true,
        "scope": "auth:user.id:read contact:user.base:readonly contact:user.basic_profile:readonly im:chat:read im:message.group_msg:get_as_user im:message.p2p_msg:get_as_user im:message.reactions:read im:message:readonly search:message offline_access",
        "identities": {
            "user": {
                "status": "ready",
                "available": true,
                "verified": true
            }
        }
    });

    assert!(validate_feishu_auth_status(&raw).is_none());
}

#[test]
fn accepts_nested_feishu_auth_status_fields() {
    let raw = serde_json::json!({
        "ok": true,
        "data": {
            "app": {
                "app_id": "cli_xxx",
                "brand": "feishu"
            },
            "auth": {
                "verified": true,
                "token_status": "valid",
                "scopes": [
                    "search:message",
                    "im:chat:read",
                    "im:message:readonly",
                    "im:message.reactions:read",
                    "im:message.p2p_msg:get_as_user",
                    "im:message.group_msg:get_as_user",
                    "contact:user.base:readonly",
                    "contact:user.basic_profile:readonly"
                ]
            }
        }
    });

    assert!(validate_feishu_auth_status(&raw).is_none());
}

#[test]
fn accepts_feishu_auth_status_with_named_user_identity() {
    let raw = serde_json::json!({
        "appId": "cli_xxx",
        "brand": "feishu",
        "defaultAs": "auto",
        "identities": {
            "bot": {
                "status": "ready",
                "available": true
            },
            "user": {
                "status": "ready",
                "available": true,
                "openId": "ou_xxx",
                "userName": "Chase",
                "tokenStatus": "valid",
                "scope": "auth:user.id:read contact:user.base:readonly contact:user.basic_profile:readonly im:chat:read im:message.group_msg:get_as_user im:message.p2p_msg:get_as_user im:message.reactions:read im:message:readonly search:message offline_access"
            }
        },
        "identity": "user"
    });

    assert!(validate_feishu_auth_status(&raw).is_none());
}

#[test]
fn accepts_feishu_auth_status_that_needs_refresh() {
    let raw = serde_json::json!({
        "appId": "cli_xxx",
        "brand": "feishu",
        "defaultAs": "auto",
        "identities": {
            "bot": {
                "status": "ready",
                "available": true
            },
            "user": {
                "status": "needs_refresh",
                "available": true,
                "message": "User identity: needs refresh (will auto-refresh on next user API call)",
                "openId": "ou_xxx",
                "userName": "Chase",
                "tokenStatus": "needs_refresh",
                "scope": "auth:user.id:read contact:user.base:readonly contact:user.basic_profile:readonly im:chat:read im:message.group_msg:get_as_user im:message.p2p_msg:get_as_user im:message.reactions:read im:message:readonly search:message offline_access"
            }
        },
        "identity": "user",
        "note": "User identity needs refresh and will be refreshed automatically on the next user API call."
    });

    assert!(validate_feishu_auth_status(&raw).is_none());
}

#[test]
fn classifies_feishu_missing_scope_as_reauth_hint() {
    let stdout = r#"{
          "ok": false,
          "identity": "user",
          "error": {
            "subtype": "missing_scope",
            "message": "missing required scope(s): im:message.reactions:read",
            "missing_scopes": ["im:message.reactions:read"],
            "identity": "user"
          }
        }"#;

    let error = classify_feishu_cli_error(stdout, "").expect("classified");
    assert_eq!(error.code, "FEISHU_MISSING_SCOPE");
    assert!(error.message.contains("im:message.reactions:read"));
    assert!(error.message.contains("重新复制并运行飞书绑定命令"));
}

#[test]
fn classifies_feishu_network_transport_as_retryable_cli_issue() {
    let stdout = r#"{
          "ok": false,
          "identity": "user",
          "error": {
            "type": "network",
            "subtype": "transport",
            "message": "request failed"
          },
          "_notice": {
            "update": {
              "current": "1.0.47",
              "latest": "1.0.48"
            }
          }
        }"#;

    let error = classify_feishu_cli_error(stdout, "").expect("classified");
    assert_eq!(error.code, "FEISHU_NETWORK_TRANSPORT");
    assert!(error.recoverable);
    assert!(error.message.contains("网络传输失败"));
    assert!(error.message.contains("当前 1.0.47，最新 1.0.48"));
    assert!(!error.message.contains("授权不完整"));
}

#[test]
fn classifies_feishu_search_not_authenticated_as_user_auth_incomplete() {
    let stdout = r#"{
          "ok": false,
          "error": {
            "reason": "not_authenticated",
            "message": "not_authenticated"
          }
        }"#;

    let error = classify_feishu_cli_error(stdout, "").expect("classified");
    assert_eq!(error.code, "FEISHU_NOT_AUTHENTICATED");
    assert!(error.message.contains("授权不完整"));
    assert!(!error.message.contains("请复制飞书绑定命令"));
}

#[test]
fn ignores_feishu_auth_words_inside_successful_message_payload() {
    let raw = serde_json::json!({
        "items": [
            {
                "message_id": "om_xxx",
                "content": "今天排查过 not_authenticated、not configured 和未登录，但这只是聊天正文。"
            }
        ]
    });

    assert!(classify_feishu_cli_structured_error(&raw, "").is_none());
}

#[test]
fn classifies_dingtalk_developer_settings_permission_error() {
    let stdout = r#"{
          "error": {
            "action_url": "https://open-dev.dingtalk.com/fe/old#/developerSettings",
            "category": "api",
            "code": 1,
            "friendly_hint": "该组织尚未开启 CLI数据访问权限，请联系组织主管理员开启。",
            "message": "business error: success=false",
            "reason": "business_error",
            "server_error_code": "TOKEN_VERIFIED_FAILED",
            "server_key": "group-chat"
          }
        }"#;

    let error = classify_dingtalk_cli_error(stdout, "").expect("classified");
    assert_eq!(error.code, "DINGTALK_MESSAGE_PERMISSION_MISSING");
    assert!(error.recoverable);
    assert!(error.message.contains("CLI数据访问权限"));
}

#[test]
fn classifies_sanitized_dingtalk_permission_error() {
    let stdout = r#"{
          "error": {
            "action_url": "https://open-dev.dingtalk.com/fe/old#/developerSettings",
            "category": "api",
            "code": 1,
            "friendly_hint": "该组织尚未开启 CLI数据访问权限，请联系组织主管理员开启。",
            "message": "business error: success=false",
token=***
            "reason": "business_error",
            "server_key": "group-chat"
          }
        }"#;

    let error = classify_dingtalk_cli_error(stdout, "").expect("classified");
    assert_eq!(error.code, "DINGTALK_MESSAGE_PERMISSION_MISSING");
    assert!(error.recoverable);
    assert!(error.message.contains("CLI数据访问权限"));
}

#[test]
fn classifies_sanitized_dingtalk_permission_error_with_spaced_hint() {
    let stdout = r#"{
          "error": {
            "action_url": "https://open-dev.dingtalk.com/fe/old#/developerSettings",
            "category": "api",
            "code": 1,
            "friendly_hint": "该组织尚未开启 CLI 数据访问权限，请联系组织主管理员开启。",
            "hint": "The API returned a business-level error. Check required parameters and values.",
            "message": "business error: success=false",
token=***
            "operation": "tools/call",
            "reason": "business_error",
            "server_key": "group-chat",
token=***
            "trace_id": "0bab027317781704908111717e096e"
          }
        }"#;

    let error = classify_dingtalk_cli_error(stdout, "").expect("classified");
    assert_eq!(error.code, "DINGTALK_MESSAGE_PERMISSION_MISSING");
    assert!(error.recoverable);
    assert!(error.message.contains("CLI数据访问权限"));
}

#[test]
fn classifies_dingtalk_pat_permission_error() {
    let stdout = r#"{
          "code": "PAT_MEDIUM_RISK_NO_PERMISSION",
          "data": {
            "requiredScopes": [
              {
                "scope": "chat.message:list"
              }
            ]
          },
          "success": false
        }"#;

    let error = classify_dingtalk_cli_error(stdout, "").expect("classified");
    assert_eq!(error.code, "DINGTALK_MESSAGE_PERMISSION_MISSING");
    assert!(error.recoverable);
    assert!(error.message.contains("消息读取权限"));
}

#[test]
fn classifies_dingtalk_group_chat_forbidden_error() {
    let stdout = r#"{
          "error": {
            "category": "api",
            "code": 1,
            "hint": "The API returned a business-level error. Check required parameters and values.",
            "message": "forbidden request",
            "operation": "tools/call",
            "reason": "business_error",
            "server_error_code": "1001",
            "server_key": "group-chat",
            "trace_id": "21030ead17780641308991861e0a34"
          }
        }"#;

    let error = classify_dingtalk_cli_error(stdout, "").expect("classified");
    assert_eq!(error.code, "DINGTALK_MESSAGE_PERMISSION_MISSING");
    assert!(error.recoverable);
    assert!(error.message.contains("消息读取权限"));
}

#[test]
fn filters_feishu_cli_page_progress() {
    let output = "[page 1] fetching...\n[page 1] fetched 50 items\n真实警告";

    assert_eq!(sanitize_feishu_cli_output(output), "真实警告");
}

#[test]
fn parses_feishu_message_search_json_with_page_progress() {
    let stdout =
            "[page 1] fetching...\n{\"items\":[{\"chat_id\":\"oc_1\",\"chat_name\":\"产品群\",\"chat_type\":\"group\",\"create_time\":\"1779250000000\"}]}\n";

    let raw = parse_feishu_cli_json(stdout).expect("json parsed");
    let chats = normalize_feishu_message_sessions(&raw);

    assert_eq!(
        chats
            .as_array()
            .and_then(|items| items.first())
            .and_then(|item| item.get("chatName"))
            .and_then(|value| value.as_str()),
        Some("产品群")
    );
}

#[test]
fn normalizes_feishu_chat_list_json() {
    let raw = serde_json::json!({
        "items": [
            {
                "chat_id": "oc_1",
                "name": "产品群",
                "chat_type": "group",
                "last_active_time": "2026-05-26T10:20:00+08:00"
            }
        ]
    });

    let chats = normalize_feishu_chats(&raw);

    assert_eq!(
        chats
            .as_array()
            .and_then(|items| items.first())
            .and_then(|item| item.get("chatId"))
            .and_then(|value| value.as_str()),
        Some("oc_1")
    );
    assert_eq!(
        chats
            .as_array()
            .and_then(|items| items.first())
            .and_then(|item| item.get("chatName"))
            .and_then(|value| value.as_str()),
        Some("产品群")
    );
}

#[test]
fn message_type_boundary_actual_enterprise_adapters_keep_declared_types() {
    use crate::analysis::message_noise;
    use crate::messages::{import_window::MessageImportWindow, normalizer::normalize_message};
    use chrono::TimeZone;
    let window = MessageImportWindow::natural_day("2026-05-01").unwrap();
    let timestamp = window.start_timestamp + 12 * 3600;
    let time = chrono::Local
        .timestamp_opt(timestamp, 0)
        .single()
        .unwrap()
        .format("%Y-%m-%d %H:%M:%S")
        .to_string();
    let args = std::collections::HashMap::from([
        ("chat".to_owned(), "business-chat".to_owned()),
        ("chat_name".to_owned(), "项目群".to_owned()),
        ("chat_type".to_owned(), "group".to_owned()),
    ]);
    for platform in ["feishu", "dingtalk", "wecom"] {
        let profile = crate::storage::models::ImProfile {
            id: platform.to_owned(),
            platform: platform.to_owned(),
            label: "fixture".to_owned(),
            enabled: true,
            config_json: serde_json::json!({}),
            status: "normal".to_owned(),
            sort_order: 0,
            created_at: "".to_owned(),
            updated_at: "".to_owned(),
        };
        for (kind, content) in [
            ("text", "群公告发布时间需要确认"),
            ("text", "拍一拍提醒功能需要交付"),
            ("text", "[系统] 用户手写的会议安排需要确认"),
            ("system", "后台系统维护需要确认invoice.pdf"),
            ("notice", "<sysmsg type=\"event\">后台配置已调整</sysmsg>"),
        ] {
            let raw = match platform {
                "feishu" => {
                    serde_json::json!({"items":[{"create_time":timestamp.to_string(),"msg_type":kind,"content":serde_json::json!({"text":content}).to_string(),"sender":{"id":"customer"}}]})
                }
                "dingtalk" => {
                    serde_json::json!({"messages":[{"timestamp":timestamp,"msgType":kind,"content":content,"senderId":"customer"}]})
                }
                _ => {
                    serde_json::json!({"messages":[{"send_time":time,"msgtype":kind,"text":{"content":content},kind:{"name":content,"content":content},"userid":"customer"}]})
                }
            };
            let adapted = match platform {
                "feishu" => super::normalizers::normalize_feishu_messages(&raw, &args),
                "dingtalk" => super::normalizers::normalize_dingtalk_messages(&raw, &args),
                _ => super::normalizers::normalize_wecom_messages(
                    &raw,
                    &args,
                    &serde_json::json!({}),
                ),
            };
            let message = normalize_message(
                &profile,
                &adapted[0],
                &window,
                "business-chat",
                "项目群",
                true,
            )
            .expect("normalized message");
            assert_eq!(message.raw_type.as_deref(), Some(kind));
            assert_eq!(
                message.msg_type,
                if kind == "text" { "text" } else { "system" },
                "{platform}: {content}"
            );
            assert_eq!(
                message_noise::is_message_noise(Some(&message.msg_type), &message.content, true),
                kind != "text",
                "{platform}: {content}"
            );
        }
    }
}

#[test]
fn feishu_empty_search_and_missing_search_scope_are_distinct() {
    let empty = serde_json::json!({"ok":true,"data":{"items":[]}});
    assert!(classify_feishu_cli_structured_error(&empty, "").is_none());
    assert_eq!(
        normalize_feishu_message_sessions(&empty),
        serde_json::json!([])
    );
    let denied = serde_json::json!({"ok":false,"error":{
        "subtype":"missing_scope","missing_scopes":["search:message"]
    }});
    let error = classify_feishu_cli_structured_error(&denied, "").unwrap();
    assert_eq!(error.code, "FEISHU_MISSING_SCOPE");
    assert!(error.message.contains("search:message"));
}

#[test]
fn feishu_projection_rejects_unknown_and_discarded_rows_but_accepts_empty_reads() {
    for command in ["list-chats", "fetch-messages", "search-messages"] {
        for raw in [
            serde_json::json!([]),
            serde_json::json!({"data":{"items":[],"has_more":false}}),
        ] {
            assert_eq!(
                checked_feishu_read(&raw, &HashMap::new(), command).unwrap(),
                serde_json::json!([])
            );
        }
        for raw in [
            serde_json::json!({}),
            serde_json::json!({"items":[] ,"has_more":true}),
            serde_json::json!({"items":[{"unexpected":"private body"}]}),
        ] {
            assert!(checked_feishu_read(&raw, &HashMap::new(), command).is_err());
        }
    }
}

#[test]
fn validated_nested_search_projection_preserves_nonempty_results() {
    let raw = serde_json::json!({"data":{"result":{"items":[{
        "chat_id":"chat", "message_id":"message", "create_time":"1792972800000"
    }]}}});
    let projected = checked_feishu_read(&raw, &HashMap::new(), "search-messages").unwrap();
    assert_eq!(projected.as_array().unwrap().len(), 1);
    assert_eq!(projected[0]["chatId"], "chat");
}
