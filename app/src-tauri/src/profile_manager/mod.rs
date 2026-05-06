use chrono::Local;
use rusqlite::{params, Connection};

use crate::storage::models::ImProfile;

pub fn list_profiles(conn: &Connection) -> anyhow::Result<Vec<ImProfile>> {
    let mut stmt = conn.prepare(
        "select id, platform, label, enabled, config_json, status, sort_order, created_at, updated_at
         from profiles order by sort_order, platform, label",
    )?;
    let rows = stmt.query_map([], |row| {
        let config: String = row.get(4)?;
        Ok(ImProfile {
            id: row.get(0)?,
            platform: row.get(1)?,
            label: row.get(2)?,
            enabled: row.get::<_, i64>(3)? == 1,
            config_json: serde_json::from_str(&config).unwrap_or_else(|_| serde_json::json!({})),
            status: row.get(5)?,
            sort_order: row.get(6)?,
            created_at: row.get(7)?,
            updated_at: row.get(8)?,
        })
    })?;
    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

pub fn upsert_profile(conn: &Connection, profile: ImProfile) -> anyhow::Result<ImProfile> {
    let now = Local::now().to_rfc3339();
    let created_at = if profile.created_at.is_empty() {
        now.clone()
    } else {
        profile.created_at.clone()
    };
    conn.execute(
        "insert into profiles(id, platform, label, enabled, config_json, status, sort_order, created_at, updated_at)
         values (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
         on conflict(id) do update set
           platform = excluded.platform,
           label = excluded.label,
           enabled = excluded.enabled,
           config_json = excluded.config_json,
           status = excluded.status,
           sort_order = excluded.sort_order,
           updated_at = excluded.updated_at",
        params![
            profile.id,
            profile.platform,
            profile.label,
            if profile.enabled { 1 } else { 0 },
            serde_json::to_string(&profile.config_json)?,
            profile.status,
            profile.sort_order,
            created_at,
            now
        ],
    )?;
    Ok(profile)
}

pub fn delete_profile(conn: &Connection, profile_id: &str) -> anyhow::Result<()> {
    conn.execute("delete from profiles where id = ?1", params![profile_id])?;
    conn.execute(
        "delete from sync_state where profile_id = ?1",
        params![profile_id],
    )?;
    Ok(())
}
