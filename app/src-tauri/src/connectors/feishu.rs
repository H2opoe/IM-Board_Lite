use super::{ConnectorAdapter, ConnectorKind};

pub const ADAPTER: ConnectorAdapter = ConnectorAdapter {
    platform: "feishu",
    kind: ConnectorKind::Feishu,
    is_available,
};

fn is_available() -> bool {
    true
}
