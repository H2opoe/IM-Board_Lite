use super::{ConnectorAdapter, ConnectorKind};

pub const ADAPTER: ConnectorAdapter = ConnectorAdapter {
    platform: "wechat",
    kind: ConnectorKind::WechatLocal,
    is_available,
};

#[cfg(windows)]
fn is_available() -> bool {
    false
}

#[cfg(not(windows))]
fn is_available() -> bool {
    true
}
