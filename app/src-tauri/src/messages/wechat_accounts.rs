const WECHAT_SYSTEM_ACCOUNTS: &[&str] = &[
    "newsapp",
    "fmessage",
    "filehelper",
    "weibo",
    "qqmail",
    "tmessage",
    "qmessage",
    "qqsync",
    "floatbottle",
    "lbsapp",
    "shakeapp",
    "medianote",
    "qqfriend",
    "readerapp",
    "blogapp",
    "facebookapp",
    "masssendapp",
    "meishiapp",
    "feedsapp",
    "voip",
    "blogappweixin",
    "weixin",
    "brandsessionholder",
    "weixinreminder",
    "officialaccounts",
    "notification_messages",
    "wxitil",
    "userexperience_alarm",
    "@placeholder_foldgroup",
];

pub(crate) fn is_wechat_system_account(value: &str) -> bool {
    let normalized = value.trim().to_ascii_lowercase();
    normalized.starts_with("gh_") || WECHAT_SYSTEM_ACCOUNTS.contains(&normalized.as_str())
}

#[cfg(test)]
mod tests {
    use super::is_wechat_system_account;

    #[test]
    fn detects_wechat_system_accounts_and_public_account_prefix() {
        for username in [
            "newsapp",
            "fmessage",
            "filehelper",
            "weibo",
            "qqmail",
            "tmessage",
            "qmessage",
            "qqsync",
            "floatbottle",
            "lbsapp",
            "shakeapp",
            "medianote",
            "qqfriend",
            "readerapp",
            "blogapp",
            "facebookapp",
            "masssendapp",
            "meishiapp",
            "feedsapp",
            "voip",
            "blogappweixin",
            "weixin",
            "brandsessionholder",
            "weixinreminder",
            "officialaccounts",
            "notification_messages",
            "wxitil",
            "userexperience_alarm",
            "@placeholder_foldgroup",
            "gh_1234567890abcdef",
            " GH_NEWS ",
        ] {
            assert!(
                is_wechat_system_account(username),
                "{username} should be treated as a WeChat system account"
            );
        }
    }

    #[test]
    fn keeps_normal_contacts_and_groups() {
        for username in [
            "wxid_customer_123",
            "客户项目群",
            "chatroom_abc",
            "work_group",
        ] {
            assert!(
                !is_wechat_system_account(username),
                "{username} should stay eligible for sync and analysis"
            );
        }
    }
}
