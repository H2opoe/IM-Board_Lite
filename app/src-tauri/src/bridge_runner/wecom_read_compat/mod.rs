//! Official 0.1.9 read-protocol compatibility, using current CLI authorization.
//! Contact IDs, group discovery and history all use the same gateway protocol.
mod credentials;
mod names;
mod transport;
use super::super::normalizers::{
    json_string, normalize_wecom_contacts, normalize_wecom_messages, parse_local_timestamp,
};
use super::super::{BridgeEnvelope, BridgeError};
use crate::storage::models::ImProfile;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::{HashMap, HashSet},
    time::{Instant, SystemTime, UNIX_EPOCH},
};
use transport::Transport;

type Result<T> = std::result::Result<T, Failure>;
#[derive(Debug)]
struct Failure {
    code: &'static str,
    message: String,
}
impl Failure {
    fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
    fn auth() -> Self {
        Self::new(
            "WECOM_NOT_AUTHENTICATED",
            "企业微信授权文件无效或账号不匹配，请在账号配置中检查授权。",
        )
    }
    fn invalid() -> Self {
        Self::new(
            "WECOM_COMPAT_INVALID_RESPONSE",
            "企业微信自动读取返回格式不完整，未将其当作空结果或完整同步。",
        )
    }
    fn service(stage: &str, code: i64) -> Self {
        Self::new(
            "WECOM_COMPAT_API_ERROR",
            format!("企业微信自动读取失败（{stage}，错误码 {code}）。"),
        )
    }
}
fn user_agent() -> String {
    format!(
        "WeComCLI/0.1.9 distribution/im-board-read-compat {}/{}",
        std::env::consts::OS,
        std::env::consts::ARCH
    )
}
fn request_id(prefix: &str) -> String {
    format!(
        "{prefix}_{}_{}",
        chrono::Utc::now().timestamp_millis(),
        &uuid::Uuid::new_v4().simple().to_string()[..8]
    )
}
fn config_request(bot: &credentials::Bot, now: u64, nonce: &str) -> Value {
    let mut hash = Sha256::new();
    hash.update(bot.secret.as_bytes());
    hash.update(bot.id.as_bytes());
    hash.update(now.to_string());
    hash.update(nonce);
    json!({"bot_id":bot.id,"time":now,"nonce":nonce,"signature":format!("{:x}",hash.finalize()),"bind_source":1,"cli_version":user_agent()})
}
const CONFIG_URL: &str = "https://qyapi.weixin.qq.com/cgi-bin/aibot/cli/get_mcp_config";
fn official_endpoint(url: &str) -> bool {
    // Derive the gateway's domain from the single authoritative API address.
    let Ok(authority) = reqwest::Url::parse(CONFIG_URL) else {
        return false;
    };
    let Some(domain) = authority
        .host_str()
        .and_then(|host| host.split_once('.'))
        .map(|(_, domain)| domain)
    else {
        return false;
    };
    reqwest::Url::parse(url).is_ok_and(|url| {
        url.scheme() == "https"
            && url.username().is_empty()
            && url.password().is_none()
            && url.fragment().is_none()
            && url.port_or_known_default() == Some(443)
            && url.host_str().is_some_and(|host| {
                host == domain
                    || host
                        .strip_suffix(domain)
                        .is_some_and(|prefix| prefix.ends_with('.'))
            })
    })
}

fn endpoint(config: &Value, category: &str) -> Result<String> {
    if let Some(code) = config
        .get("errcode")
        .and_then(Value::as_i64)
        .filter(|code| *code != 0)
    {
        return Err(Failure::service("授权接口", code));
    }
    let item = config
        .get("list")
        .and_then(Value::as_array)
        .and_then(|items| items.iter().find(|item| item["biz_type"] == category))
        .ok_or_else(|| {
            Failure::new(
                "WECOM_COMPAT_CAPABILITY_MISSING",
                "当前授权未提供完整通讯录或消息读取接口；未降级为手工关键词搜索。",
            )
        })?;
    if item.get("is_authed").and_then(Value::as_bool) == Some(false) {
        return Err(Failure::auth());
    }
    let url = item
        .get("url")
        .and_then(Value::as_str)
        .filter(|url| official_endpoint(url))
        .ok_or_else(Failure::invalid)?;
    Ok(url.into())
}
fn rpc_payload(response: Value) -> Result<Value> {
    if let Some(error) = response.get("error").filter(|v| !v.is_null()) {
        return Err(Failure::service(
            "读取接口",
            error["code"].as_i64().unwrap_or(-1),
        ));
    }
    let result = response.get("result").ok_or_else(Failure::invalid)?;
    if result["isError"].as_bool() == Some(true) {
        return Err(Failure::new(
            "WECOM_COMPAT_API_ERROR",
            "企业微信自动读取接口执行失败；未显示可能包含联系人或消息的原始响应。",
        ));
    }
    let content = result["content"]
        .as_array()
        .filter(|items| items.len() == 1)
        .ok_or_else(Failure::invalid)?;
    let text = content[0]["text"]
        .as_str()
        .filter(|_| content[0]["type"] == "text")
        .ok_or_else(Failure::invalid)?;
    let payload: Value = serde_json::from_str(text).map_err(|_| Failure::invalid())?;
    if let Some(code) = payload["errcode"].as_i64().filter(|code| *code != 0) {
        return Err(Failure::service("业务接口", code));
    }
    Ok(payload)
}
async fn all_pages(
    transport: &impl Transport,
    url: &str,
    method: &str,
    mut arguments: Value,
    field: &str,
) -> Result<Value> {
    let mut items = Vec::new();
    let mut seen = HashSet::new();
    let mut bytes = 0;
    for _ in 0..100 {
        let response=transport.post(url,json!({"jsonrpc":"2.0","id":request_id("mcp_rpc"),"method":"tools/call","params":{"name":method,"arguments":arguments}})).await?;
        let payload = rpc_payload(response)?;
        let page = payload[field].as_array().ok_or_else(Failure::invalid)?;
        bytes += serde_json::to_vec(page)
            .map_err(|_| Failure::invalid())?
            .len();
        if items.len() + page.len() > 20_000 || bytes > 32 * 1024 * 1024 {
            return Err(Failure::invalid());
        }
        items.extend(page.iter().cloned());
        if payload
            .get("next_cursor")
            .is_some_and(|v| !v.is_null() && !v.is_string())
            || payload
                .get("has_more")
                .is_some_and(|v| !v.is_null() && !v.is_boolean())
        {
            return Err(Failure::invalid());
        }
        let next = payload["next_cursor"].as_str().unwrap_or("");
        let has_more = payload["has_more"].as_bool();
        if next.is_empty() {
            if has_more == Some(true) {
                return Err(Failure::invalid());
            }
            let mut output = json!({"has_more":false,"next_cursor":""});
            output[field] = json!(items);
            return Ok(output);
        }
        if has_more == Some(false) || !seen.insert(next.to_owned()) {
            return Err(Failure::invalid());
        }
        arguments["cursor"] = json!(next);
    }
    Err(Failure::new(
        "WECOM_COMPAT_READ_INCOMPLETE",
        "企业微信自动读取达到分页上限，未将部分结果当作完整同步。",
    ))
}
fn time_arguments(args: &HashMap<String, String>) -> Result<Value> {
    let start = args
        .get("start_time")
        .filter(|v| !v.trim().is_empty())
        .ok_or_else(Failure::invalid)?;
    let end = args
        .get("end_time")
        .filter(|v| !v.trim().is_empty())
        .ok_or_else(Failure::invalid)?;
    Ok(json!({"begin_time":start,"end_time":end}))
}
async fn read(
    transport: &impl Transport,
    bot: &credentials::Bot,
    command: &str,
    args: &HashMap<String, String>,
) -> Result<Value> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| Failure::invalid())?
        .as_secs();
    let config = transport
        .post(CONFIG_URL, config_request(bot, now, &request_id("mcp")))
        .await?;
    match command {
        "list-contacts" => {
            all_pages(
                transport,
                &endpoint(&config, "contact")?,
                "get_userlist",
                json!({}),
                "userlist",
            )
            .await
        }
        "list-chats" => {
            let directory = all_pages(
                transport,
                &endpoint(&config, "contact")?,
                "get_userlist",
                json!({}),
                "userlist",
            )
            .await?;
            let mut chats = all_pages(
                transport,
                &endpoint(&config, "msg")?,
                "get_msg_chat_list",
                time_arguments(args)?,
                "chats",
            )
            .await?;
            chats["userlist"] = directory["userlist"].clone();
            Ok(chats)
        }
        "fetch-messages" => {
            let mut arguments = time_arguments(args)?;
            let id = args
                .get("chat")
                .filter(|v| !v.is_empty())
                .ok_or_else(Failure::invalid)?;
            let kind = match args.get("chat_type").map(String::as_str) {
                Some("1" | "single" | "direct") => 1,
                Some("2" | "group") => 2,
                _ => return Err(Failure::invalid()),
            };
            arguments["chatid"] = json!(id);
            arguments["chat_type"] = json!(kind);
            all_pages(
                transport,
                &endpoint(&config, "msg")?,
                "get_message",
                arguments,
                "messages",
            )
            .await
        }
        _ => Err(Failure::invalid()),
    }
}
async fn read_with_names(
    transport: &impl Transport,
    bot: &credentials::Bot,
    cache: &std::sync::Mutex<names::Store>,
    key: &str,
    command: &str,
    args: &HashMap<String, String>,
) -> Result<Value> {
    let sender_names = if command == "fetch-messages" {
        match names::get(cache, key) {
            Some(names) => Some(names),
            None => {
                let directory = read(transport, bot, "list-contacts", &HashMap::new()).await?;
                let names = names::from_directory(&directory)?;
                names::put(cache, key, names.clone());
                Some(names)
            }
        }
    } else {
        None
    };
    let mut raw = read(transport, bot, command, args).await?;
    if raw.get("userlist").is_some() {
        names::put(cache, key, names::from_directory(&raw)?);
    }
    if let Some(names) = sender_names {
        raw["sender_names"] = names;
    }
    Ok(raw)
}
fn name_cache_key(profile: &ImProfile, bot: &credentials::Bot, config: &str) -> String {
    let mut hash = Sha256::new();
    for field in [
        profile.id.as_str(),
        config,
        bot.id.as_str(),
        bot.secret.as_str(),
    ] {
        hash.update(field.len().to_le_bytes());
        hash.update(field.as_bytes());
    }
    format!("{:x}", hash.finalize())
}

fn chats_with_directory(raw: &Value) -> Value {
    let users: HashSet<String> = raw["userlist"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|v| json_string(v, &["userid", "userId"]))
        .collect();
    Value::Array(raw["chats"].as_array().into_iter().flatten().filter_map(|chat| {
        let id=json_string(chat,&["chat_id","chatid","chatId"])?;
        let group=!users.contains(&id);
        Some(json!({"chatId":id,"chatName":json_string(chat,&["chat_name","name"]).unwrap_or_else(||id.clone()),"isGroup":group,"chatType":if group {2}else{1},"lastMessageTime":chat["last_msg_time"],"lastMessageTimestamp":json_string(chat,&["last_msg_time"]).and_then(|v|parse_local_timestamp(&v)),"raw":chat}))
    }).collect())
}
pub(super) async fn bridge(
    profile: &ImProfile,
    command: &str,
    args: &HashMap<String, String>,
    started: Instant,
) -> BridgeEnvelope {
    let result = async {
        let config = profile.config_json["configDir"]
            .as_str()
            .ok_or_else(Failure::auth)?;
        let expected = profile
            .config_json
            .pointer("/accountIdentity/userId")
            .and_then(Value::as_str);
        let bot = credentials::load(&super::super::paths::expand_home(config), expected)?;
        let raw = read_with_names(
            &transport::Http::new()?,
            &bot,
            names::shared(),
            &name_cache_key(profile, &bot, config),
            command,
            args,
        )
        .await?;
        let data = match command {
            "list-contacts" => normalize_wecom_contacts(&raw),
            "list-chats" => chats_with_directory(&raw),
            "fetch-messages" => normalize_wecom_messages(&raw, args, &profile.config_json),
            _ => return Err(Failure::invalid()),
        };
        let field = match command {
            "list-contacts" => "userlist",
            "list-chats" => "chats",
            "fetch-messages" => "messages",
            _ => return Err(Failure::invalid()),
        };
        if data.as_array().map(Vec::len) != raw[field].as_array().map(Vec::len) {
            return Err(Failure::invalid());
        }
        Ok(data)
    }
    .await;
    let meta = json!({"platform":"wecom","readProtocol":"official-mcp-0.1.9","duration_ms":started.elapsed().as_millis()});
    match result {
        Ok(data) => BridgeEnvelope {
            ok: true,
            data,
            warnings: vec![],
            error: None,
            meta,
        },
        Err(error) => BridgeEnvelope {
            ok: false,
            data: Value::Null,
            warnings: vec![],
            error: Some(BridgeError {
                code: error.code.into(),
                message: error.message,
                recoverable: true,
            }),
            meta,
        },
    }
}
#[cfg(test)]
mod tests;
