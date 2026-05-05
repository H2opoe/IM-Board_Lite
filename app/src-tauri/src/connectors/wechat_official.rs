use super::{ConnectorAdapter, ConnectorKind};

pub const ADAPTER: ConnectorAdapter = ConnectorAdapter {
    platform: "wechat",
    kind: ConnectorKind::WechatOfficial,
    is_available,
};

#[cfg(windows)]
fn is_available() -> bool {
    true
}

#[cfg(not(windows))]
fn is_available() -> bool {
    false
}
