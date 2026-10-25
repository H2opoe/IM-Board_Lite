//! Process-group ownership anchored by a transferred, unreaped Child.
//!
//! The host calls `check_supported` before spawning and establishes
//! `process_group(0)` on the command before exec. It transfers the resulting
//! Child exclusively to this owner; no other code may wait/reap it. Group
//! signals remain tied to that held Child and are disarmed before final reap.
//! Administrator helpers that escape the original group require a separate
//! host completion barrier; a group exit is not such a proof.
#[cfg(unix)]
#[path = "process_group_owner.rs"]
mod owner;
#[cfg(unix)]
pub use owner::{check_supported, ProcessOwner};
