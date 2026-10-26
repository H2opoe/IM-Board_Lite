//! Failure checkpoints for every message reader, scoped by account and day.
pub(super) fn has_failed_profile_checkpoint(
    conn: &rusqlite::Connection,
    profile_id: &str,
    day: &str,
) -> anyhow::Result<bool> {
    Ok(conn.query_row(
        "select exists(select 1 from sync_state where profile_id=?1 and day=?2 and last_error is not null)",
        rusqlite::params![profile_id, day],
        |row| row.get(0),
    )?)
}

// Preserve a failed read before retaining any partial records.
// Successful final outcomes clear it using the existing account transaction.
pub(super) fn record_history_read_failures(
    conn: &rusqlite::Connection,
    profile: &crate::storage::models::ImProfile,
    day: &str,
    failures: &[String],
) -> Result<(), String> {
    if failures.is_empty() {
        return Ok(());
    }
    record_profile_sync_outcomes(
        conn,
        day,
        &[crate::sync::outcome::ProfileSyncOutcome::failed(
            profile,
            failures.join("；"),
        )],
    )
}

pub(super) fn record_profile_sync_outcomes(
    conn: &rusqlite::Connection,
    day: &str,
    profile_outcomes: &[crate::sync::outcome::ProfileSyncOutcome],
) -> Result<(), String> {
    crate::daily_cache::ensure_current_day(conn, day).map_err(|err| err.to_string())?;
    let tx = conn
        .unchecked_transaction()
        .map_err(|err| err.to_string())?;

    for outcome in profile_outcomes {
        if outcome.succeeded {
            tx.execute(
                "insert into sync_state(
                     profile_id, day, last_sync_at, last_attempt_at, last_success_at,
                     last_error, cursor_json, updated_at
                 ) values(?1, ?2, datetime('now'), datetime('now'), datetime('now'), null, '{}', datetime('now'))
                 on conflict(profile_id, day) do update set
                   last_sync_at = excluded.last_sync_at,
                   last_attempt_at = excluded.last_attempt_at,
                   last_success_at = excluded.last_success_at,
                   last_error = null,
                   updated_at = excluded.updated_at",
                rusqlite::params![outcome.profile_id, day],
            )
            .map_err(|err| err.to_string())?;
        } else {
            tx.execute(
                "insert into sync_state(
                     profile_id, day, last_attempt_at, last_error, cursor_json, updated_at
                 ) values(?1, ?2, datetime('now'), ?3, '{}', datetime('now'))
                 on conflict(profile_id, day) do update set
                   last_attempt_at = excluded.last_attempt_at,
                   last_error = excluded.last_error,
                   updated_at = excluded.updated_at",
                rusqlite::params![outcome.profile_id, day, outcome.error],
            )
            .map_err(|err| err.to_string())?;
        }
    }

    tx.commit().map_err(|err| err.to_string())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::models::ImProfile;

    #[test]
    fn every_connector_preserves_read_failures_and_accepts_complete_empty_retries() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch(include_str!("../../migrations/001_init.sql"))
            .unwrap();
        conn.execute_batch(
            "alter table sync_state add column last_attempt_at text;
            alter table sync_state add column last_success_at text;
            alter table sync_state add column last_error text;",
        )
        .unwrap();
        let day = "2026-10-26";
        conn.execute(
            "update app_meta set value=?1 where key='current_day'",
            [day],
        )
        .unwrap();
        for connector in crate::connectors::capability_descriptors() {
            let profile = ImProfile {
                id: connector.platform.into(),
                platform: connector.platform.into(),
                label: "fixture".into(),
                enabled: true,
                config_json: serde_json::json!({}),
                status: "normal".into(),
                sort_order: 0,
                created_at: String::new(),
                updated_at: String::new(),
            };
            record_history_read_failures(&conn, &profile, day, &[]).unwrap();
            assert!(!has_failed_profile_checkpoint(&conn, &profile.id, day).unwrap());
            record_history_read_failures(&conn, &profile, day, &["partial read".into()]).unwrap();
            assert!(has_failed_profile_checkpoint(&conn, &profile.id, day).unwrap());
            let complete_empty =
                crate::sync::outcome::ProfileSyncOutcome::from_reads(&profile, 0, vec![], vec![]);
            record_profile_sync_outcomes(&conn, day, &[complete_empty]).unwrap();
            assert!(!has_failed_profile_checkpoint(&conn, &profile.id, day).unwrap());
        }
    }
}
