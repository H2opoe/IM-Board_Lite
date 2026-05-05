pub mod dingtalk;
pub mod feishu;
pub mod wechat_local;
pub mod wechat_official;
pub mod wecom;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConnectorKind {
    WechatLocal,
    WechatOfficial,
    Wecom,
    Feishu,
    Dingtalk,
}

#[derive(Clone, Copy)]
pub struct ConnectorAdapter {
    pub platform: &'static str,
    pub kind: ConnectorKind,
    pub is_available: fn() -> bool,
}

impl ConnectorAdapter {
    fn matches(self, platform: &str) -> bool {
        (self.is_available)() && self.platform == platform
    }
}

const CONNECTOR_REGISTRY: &[ConnectorAdapter] = &[
    wechat_official::ADAPTER,
    wechat_local::ADAPTER,
    wecom::ADAPTER,
    feishu::ADAPTER,
    dingtalk::ADAPTER,
];

pub fn find(platform: &str) -> Option<ConnectorAdapter> {
    CONNECTOR_REGISTRY
        .iter()
        .copied()
        .find(|adapter| adapter.matches(platform))
}
