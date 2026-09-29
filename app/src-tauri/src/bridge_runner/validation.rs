use std::collections::HashSet;

use super::BridgeRequest;

pub(super) fn validate_bridge_request(request: &BridgeRequest) -> Result<(), String> {
    let connector = crate::connectors::find(&request.platform)
        .ok_or_else(|| "不支持的消息平台。".to_owned())?;
    let allowed_commands = connector.capabilities.commands;
    if !allowed_commands.contains(&request.command.as_str()) {
        return Err(format!(
            "{}不支持命令{}。",
            request.platform, request.command
        ));
    }
    let profile = request
        .profile
        .as_ref()
        .ok_or_else(|| "当前命令缺少账号配置。".to_owned())?;
    if profile.platform != request.platform {
        return Err("账号平台与执行平台不一致。".to_owned());
    }
    if profile.id.trim().is_empty() || profile.id.len() > 160 {
        return Err("账号标识无效。".to_owned());
    }

    let allowed_args = allowed_args(&request.command);
    for (key, value) in &request.args {
        if !allowed_args.contains(key.as_str()) {
            return Err(format!("命令{}不接受参数{}。", request.command, key));
        }
        if value.contains('\0') || value.len() > 8_192 {
            return Err(format!("参数{}内容无效。", key));
        }
    }
    validate_required_args(request)
}

fn allowed_args(command: &str) -> HashSet<&'static str> {
    match command {
        "auth-status" | "account-identity" | "list-contacts" | "get-self" => HashSet::new(),
        "list-chats" | "search-messages" => ["limit", "start_time", "end_time", "cursor"]
            .into_iter()
            .collect(),
        "search-groups" => ["query"].into_iter().collect(),
        "fetch-messages" => [
            "chat",
            "chat_name",
            "chat_type",
            "limit",
            "start_time",
            "end_time",
            "cursor",
            "forward",
        ]
        .into_iter()
        .collect(),
        _ => HashSet::new(),
    }
}

fn validate_required_args(request: &BridgeRequest) -> Result<(), String> {
    let require_non_empty = |key: &str| {
        request
            .args
            .get(key)
            .is_some_and(|value| !value.trim().is_empty())
    };
    if request.command == "fetch-messages" && !require_non_empty("chat") {
        return Err("读取消息缺少会话标识。".to_owned());
    }
    if request.command == "search-groups" && !require_non_empty("query") {
        return Err("搜索群聊缺少关键词。".to_owned());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn rejects_unknown_platform_and_arguments() {
        let unknown = BridgeRequest {
            platform: "unknown".to_owned(),
            command: "auth-status".to_owned(),
            profile: None,
            args: HashMap::new(),
            stdin_secret: None,
        };
        assert!(validate_bridge_request(&unknown).is_err());
    }
}

#[cfg(test)]
mod contract_tests {
    use super::*;
    #[test]
    fn every_registered_connector_command_passes_validation() {
        for descriptor in crate::connectors::capability_descriptors() {
            let profile = crate::storage::models::ImProfile {
                id: "test".into(),
                platform: descriptor.platform.into(),
                label: "test".into(),
                enabled: true,
                config_json: serde_json::json!({}),
                status: "normal".into(),
                sort_order: 0,
                created_at: String::new(),
                updated_at: String::new(),
            };
            for command in descriptor.commands {
                let mut args = std::collections::HashMap::new();
                if *command == "fetch-messages" {
                    args.insert("chat".into(), "test-chat".into());
                }
                if *command == "search-groups" {
                    args.insert("query".into(), "test-query".into());
                }
                let request = BridgeRequest {
                    platform: profile.platform.clone(),
                    command: (*command).into(),
                    profile: Some(profile.clone()),
                    args,
                    stdin_secret: None,
                };
                assert!(
                    validate_bridge_request(&request).is_ok(),
                    "{} {}",
                    descriptor.platform,
                    command
                );
            }
        }
    }
}
