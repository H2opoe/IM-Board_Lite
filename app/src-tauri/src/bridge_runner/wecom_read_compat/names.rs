//! Bounded, account-scoped name cache. Never persists directory contents.
use super::{json_string, Failure, Result};
use serde_json::{Map, Value};
use std::{
    collections::HashMap,
    sync::{Mutex, OnceLock},
    time::{Duration, Instant},
};
const TTL: Duration = Duration::from_secs(300);
const MAX_BYTES: usize = 4 * 1024 * 1024;
const MAX_ACCOUNTS: usize = 32;
#[derive(Default)]
pub(super) struct Store {
    entries: HashMap<String, Entry>,
}
struct Entry {
    names: Value,
    inserted: Instant,
    bytes: usize,
}
impl Store {
    pub fn get(&mut self, key: &str, now: Instant) -> Option<Value> {
        self.entries
            .retain(|_, entry| now.saturating_duration_since(entry.inserted) < TTL);
        self.entries.get(key).map(|entry| entry.names.clone())
    }
    pub fn put(&mut self, key: String, names: Value, now: Instant) {
        self.entries.remove(&key);
        let Ok(bytes) = serde_json::to_vec(&names).map(|bytes| bytes.len()) else {
            return;
        };
        if bytes > MAX_BYTES {
            return;
        }
        while self.entries.len() >= MAX_ACCOUNTS
            || self
                .entries
                .values()
                .map(|entry| entry.bytes)
                .sum::<usize>()
                + bytes
                > MAX_BYTES
        {
            let Some(oldest) = self
                .entries
                .iter()
                .min_by_key(|(_, entry)| entry.inserted)
                .map(|(key, _)| key.clone())
            else {
                break;
            };
            self.entries.remove(&oldest);
        }
        self.entries.insert(
            key,
            Entry {
                names,
                inserted: now,
                bytes,
            },
        );
    }
}
pub(super) fn shared() -> &'static Mutex<Store> {
    static CACHE: OnceLock<Mutex<Store>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(Store::default()))
}
pub(super) fn get(cache: &Mutex<Store>, key: &str) -> Option<Value> {
    cache.lock().ok()?.get(key, Instant::now())
}
pub(super) fn put(cache: &Mutex<Store>, key: &str, names: Value) {
    if let Ok(mut cache) = cache.lock() {
        cache.put(key.into(), names, Instant::now());
    }
}
pub(super) fn from_directory(raw: &Value) -> Result<Value> {
    let users = raw["userlist"].as_array().ok_or_else(Failure::invalid)?;
    let mut names = Map::new();
    for user in users {
        let id = json_string(user, &["userid", "userId", "id"]).ok_or_else(Failure::invalid)?;
        if let Some(name) =
            json_string(user, &["name", "displayName", "alias"]).filter(|name| name != &id)
        {
            names.insert(id, Value::String(name));
        }
    }
    Ok(Value::Object(names))
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn accounts_are_isolated_and_names_expire_without_touching_disk() {
        let mut cache = Store::default();
        let now = Instant::now();
        cache.put("profile-a".into(), json!({"same-id":"甲"}), now);
        cache.put("profile-b".into(), json!({"same-id":"乙"}), now);
        assert_eq!(cache.get("profile-a", now).unwrap()["same-id"], "甲");
        assert_eq!(cache.get("profile-b", now).unwrap()["same-id"], "乙");
        assert!(cache.get("profile-a", now + TTL).is_none());
    }
    #[test]
    fn cache_is_bounded_and_malformed_members_are_not_cached_as_empty() {
        let mut cache = Store::default();
        let now = Instant::now();
        for i in 0..40 {
            cache.put(i.to_string(), json!({"id":"name"}), now);
        }
        assert_eq!(cache.entries.len(), MAX_ACCOUNTS);
        cache.put("too-big".into(), json!("x".repeat(MAX_BYTES)), now);
        assert!(cache.get("too-big", now).is_none());
        assert!(from_directory(&json!({"userlist":[{"name":"bad"}]})).is_err());
    }
}
