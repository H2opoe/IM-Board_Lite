use super::{ConnectorAdapter, ConnectorKind};

pub const ADAPTER: ConnectorAdapter = ConnectorAdapter {
    platform: "wecom",
    kind: ConnectorKind::Wecom,
    is_available,
};

fn is_available() -> bool {
    true
}
