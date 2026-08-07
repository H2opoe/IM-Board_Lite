#[path = "normalizers/dingtalk.rs"]
mod dingtalk;
#[path = "normalizers/feishu.rs"]
mod feishu;
#[path = "normalizers/shared.rs"]
mod shared;
#[path = "normalizers/wecom.rs"]
mod wecom;

pub(super) use dingtalk::*;
pub(super) use feishu::*;
pub(super) use shared::*;
pub(super) use wecom::*;
