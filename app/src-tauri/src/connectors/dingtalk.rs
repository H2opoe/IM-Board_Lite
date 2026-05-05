use super::{ConnectorAdapter, ConnectorKind};

pub const ADAPTER: ConnectorAdapter = ConnectorAdapter {
    platform: "dingtalk",
    kind: ConnectorKind::Dingtalk,
    is_available,
};

fn is_available() -> bool {
    true
}
