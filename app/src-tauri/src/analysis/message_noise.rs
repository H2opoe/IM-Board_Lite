use crate::messages::message_type::is_system_message_type;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MessageNoiseKind {
    SystemEvent,
    GroupMembership,
    ClientCompatibility,
    BotCheckin,
    GroupAnnouncement,
    Jielong,
}

pub(crate) fn classify_message_noise(
    msg_type: Option<&str>,
    content: &str,
    is_group: bool,
) -> Option<MessageNoiseKind> {
    let trimmed = content.trim();
    if trimmed.is_empty() {
        return None;
    }

    if is_system_message_type(msg_type.unwrap_or_default()) {
        if contains_group_announcement_notice(msg_type, trimmed) {
            return Some(MessageNoiseKind::GroupAnnouncement);
        }
        return Some(MessageNoiseKind::SystemEvent);
    }

    if contains_group_membership_notice(trimmed) {
        return Some(MessageNoiseKind::GroupMembership);
    }
    if contains_client_compatibility_notice(trimmed) {
        return Some(MessageNoiseKind::ClientCompatibility);
    }
    if contains_group_announcement_notice(msg_type, trimmed) {
        return Some(MessageNoiseKind::GroupAnnouncement);
    }
    if contains_jielong_notice(trimmed) {
        return Some(MessageNoiseKind::Jielong);
    }
    if contains_bot_checkin_notice(trimmed, is_group) {
        return Some(MessageNoiseKind::BotCheckin);
    }

    None
}

pub(crate) fn is_message_noise(msg_type: Option<&str>, content: &str, is_group: bool) -> bool {
    classify_message_noise(msg_type, content, is_group).is_some()
}

pub(crate) fn contains_client_compatibility_notice(content: &str) -> bool {
    let compact = compact_text(content);
    let notice = compact.trim_end_matches(['。', '.', '!', '！']);
    // Known complete client placeholders; surrounding business discussion stays readable.
    for client in ["当前", "飞书", "钉钉", "企业微信"] {
        for body in ["版本不支持显示内容", "版本不支持展示内容"] {
            let prefix = format!("{client}{body}");
            if let Some(tail) = notice.strip_prefix(&prefix) {
                if matches!(
                    tail,
                    "" | "，请升级后查看"
                        | ",请升级后查看"
                        | "，请升级至最新版本查看"
                        | ",请升级至最新版本查看"
                ) {
                    return true;
                }
            }
        }
        if client != "当前" {
            if let Some(tail) = notice.strip_prefix(&format!("请升级{client}查看")) {
                if matches!(tail, "" | "此消息" | "该消息" | "消息" | "内容") {
                    return true;
                }
            }
        }
    }
    false
}

pub(crate) fn contains_group_membership_notice(content: &str) -> bool {
    let compact = compact_text(content);
    let notice = compact.trim_end_matches(['。', '.', '!', '！']);
    // Legacy producer templates with identifiable shape, not generic welcome/invite words.
    let qr_join = notice.ends_with("加入群聊")
        && (notice.contains("通过扫描二维码")
            || (notice.contains("通过") && notice.contains("分享的二维码")));
    let dingtalk_welcome_card = compact.contains("让我们一起欢迎新人")
        && compact.contains("群小钉")
        && compact.contains("dingtalk://");
    qr_join || dingtalk_welcome_card || looks_like_group_welcome_template(&compact)
}

fn contains_group_announcement_notice(msg_type: Option<&str>, content: &str) -> bool {
    is_system_message_type(msg_type.unwrap_or_default()) && compact_text(content).contains("公告")
}

fn contains_bot_checkin_notice(content: &str, is_group: bool) -> bool {
    let compact = compact_text(content);
    // The bare group command and complete automated receipt are intentionally low signal.
    (is_group && compact == "签到")
        || (compact.starts_with('@')
            && compact.contains("今日第")
            && compact.contains("个签到")
            && compact.contains("当前会员总积分")
            && compact.contains("已连续签到"))
        || (compact.starts_with('@')
            && compact.contains("🕹今日已签到")
            && compact.contains("连续签到解锁")
            && compact.contains("点击进入积分商城")
            && compact.contains("https://"))
}

fn contains_jielong_notice(content: &str) -> bool {
    let trimmed = content.trim();
    let compact = compact_text(trimmed);
    if compact.is_empty() {
        return false;
    }

    let normalized_prefix = compact
        .trim_start_matches("[链接/文件]")
        .trim_start_matches("【链接/文件】")
        .trim_start_matches("[链接]")
        .trim_start_matches("【链接】")
        .trim_start_matches("[文件]")
        .trim_start_matches("【文件】");
    if normalized_prefix.starts_with("#接龙") {
        return true;
    }

    if !compact.contains("接龙") {
        return false;
    }
    numbered_list_line_count(trimmed) >= 3
}

fn looks_like_group_welcome_template(compact: &str) -> bool {
    compact.starts_with("🖥欢迎")
        && compact.contains("加入")
        && compact.contains("公测体验群")
        && compact.contains("🔹如何参与公测")
}

fn numbered_list_line_count(content: &str) -> usize {
    content
        .lines()
        .filter(|line| {
            let line = line.trim_start();
            let digit_count = line.chars().take_while(|ch| ch.is_ascii_digit()).count();
            if digit_count == 0 || digit_count > 3 {
                return false;
            }
            line.chars()
                .nth(digit_count)
                .is_some_and(|ch| matches!(ch, '.' | '、' | ')' | '）' | ' '))
        })
        .count()
}

fn compact_text(content: &str) -> String {
    content
        .chars()
        .filter(|ch| !ch.is_whitespace())
        .collect::<String>()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_representative_group_noise() {
        assert_eq!(
            classify_message_noise(
                Some("text"),
                r#"[系统] "Mr.成"通过扫描"管家芽芽"分享的二维码加入群聊"#,
                true
            ),
            Some(MessageNoiseKind::GroupMembership)
        );
        assert_eq!(
            classify_message_noise(
                Some("text"),
                "🖥 欢迎 孙文康、皮蛋瘦肉周 加入应用宝Mac公测体验群！\n🔹 如何参与公测？",
                true
            ),
            Some(MessageNoiseKind::GroupMembership)
        );
        assert_eq!(
            classify_message_noise(Some("text"), "签到", true),
            Some(MessageNoiseKind::BotCheckin)
        );
        assert_eq!(
            classify_message_noise(
                Some("text"),
                "@荣少\n今日第 58 个签到，加 5.0 积分 当前会员总积分：470.0 已连续签到5天",
                true
            ),
            Some(MessageNoiseKind::BotCheckin)
        );
        assert_eq!(
            classify_message_noise(
                Some("text"),
                "#接龙\n百香果团购\n1. 王霞 1箱\n2. 钟予馨2箱\n3. 昕彤1箱",
                true
            ),
            Some(MessageNoiseKind::Jielong)
        );
        assert_eq!(
            classify_message_noise(Some("system"), "[系统] 群公告 已更新", true),
            Some(MessageNoiseKind::GroupAnnouncement)
        );
    }

    #[test]
    fn keeps_real_discussion_from_noisy_groups() {
        for content in [
            "Steam官网被墙了，直接其他渠道找一个Steam的安装包，也一样的",
            "请设计欢迎新人引导流程",
            "明天邀请客户加入群聊讨论交付",
            "当前版本不支持显示内容，我们下周修复",
            "签到奖励积分规则需要产品确认",
            "softwareupdate --install-rosetta 在【终端】里执行下这个命令试试",
            "@通宝妈手工美食 早！不好意思，昨天有点事搞忘了，还在不，我一会过来拿",
        ] {
            assert_eq!(classify_message_noise(Some("text"), content, true), None);
        }
    }
}
