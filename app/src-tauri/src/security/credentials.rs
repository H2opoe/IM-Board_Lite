#[cfg(not(test))]
use anyhow::Context;
use rusqlite::{params, Connection, OptionalExtension};

#[cfg(not(test))]
const AI_KEYRING_SERVICE: &str = "com.local.im-board.ai";
#[cfg(not(test))]
const AI_KEYRING_ACCOUNT: &str = "default-api-key";

#[cfg(not(test))]
fn entry() -> anyhow::Result<keyring::v1::Entry> {
    keyring::v1::Entry::new(AI_KEYRING_SERVICE, AI_KEYRING_ACCOUNT)
        .context("系统安全凭据服务不可用")
}

#[cfg(not(test))]
pub fn load_ai_api_key() -> anyhow::Result<Option<String>> {
    match entry()?.get_password() {
        Ok(value) if !value.trim().is_empty() => Ok(Some(value)),
        Ok(_) | Err(keyring::v1::Error::NoEntry) => Ok(None),
        Err(error) => Err(error).context("读取系统安全凭据失败"),
    }
}

#[cfg(not(test))]
pub fn store_ai_api_key(value: &str) -> anyhow::Result<()> {
    let value = value.trim();
    if value.is_empty() {
        return delete_ai_api_key();
    }
    entry()?.set_password(value).context("写入系统安全凭据失败")
}

#[cfg(not(test))]
pub fn delete_ai_api_key() -> anyhow::Result<()> {
    match entry()?.delete_credential() {
        Ok(()) | Err(keyring::v1::Error::NoEntry) => Ok(()),
        Err(error) => Err(error).context("删除系统安全凭据失败"),
    }
}

#[cfg(test)]
static TEST_AI_API_KEY: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);

#[cfg(test)]
pub fn load_ai_api_key() -> anyhow::Result<Option<String>> {
    Ok(TEST_AI_API_KEY.lock().expect("test keyring lock").clone())
}

#[cfg(test)]
pub fn store_ai_api_key(value: &str) -> anyhow::Result<()> {
    *TEST_AI_API_KEY.lock().expect("test keyring lock") = Some(value.trim().to_owned());
    Ok(())
}

#[cfg(test)]
pub fn delete_ai_api_key() -> anyhow::Result<()> {
    *TEST_AI_API_KEY.lock().expect("test keyring lock") = None;
    Ok(())
}

/// Migrates a legacy plaintext AI key without risking data loss. The database is
/// cleared only after the operating-system credential store accepts the secret.
pub fn migrate_legacy_ai_api_key(conn: &Connection) -> anyhow::Result<bool> {
    let legacy = conn
        .query_row("select api_key from ai_config where id = 1", [], |row| {
            row.get::<_, String>(0)
        })
        .optional()?;
    let Some(legacy) = legacy.filter(|value| !value.trim().is_empty()) else {
        return Ok(false);
    };

    store_ai_api_key(&legacy)?;
    conn.execute(
        "update ai_config set api_key = '', updated_at = datetime('now') where id = 1",
        params![],
    )?;
    Ok(true)
}

pub fn resolve_ai_api_key(legacy_value: String) -> anyhow::Result<String> {
    if !legacy_value.trim().is_empty() {
        return Ok(legacy_value);
    }
    Ok(load_ai_api_key()?.unwrap_or_default())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_key_is_cleared_only_after_secure_storage_succeeds() {
        delete_ai_api_key().expect("clear test keyring");
        let conn = Connection::open_in_memory().expect("database");
        conn.execute_batch(
            "create table ai_config(id integer primary key, api_key text not null, updated_at text);
             insert into ai_config(id, api_key) values(1, 'legacy-secret');",
        )
        .expect("legacy schema");

        assert!(migrate_legacy_ai_api_key(&conn).expect("migrate"));
        let stored: String = conn
            .query_row("select api_key from ai_config where id = 1", [], |row| {
                row.get(0)
            })
            .expect("database key");
        assert!(stored.is_empty());
        assert_eq!(
            load_ai_api_key().expect("keyring").as_deref(),
            Some("legacy-secret")
        );
        delete_ai_api_key().expect("clear test keyring");
    }
}
