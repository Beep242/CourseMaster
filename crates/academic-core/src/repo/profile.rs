use sqlx::{Row, SqlitePool};

use crate::error::CoreError;
use crate::models::UserProfile;

fn json_or(s: &str, fallback: serde_json::Value) -> serde_json::Value {
    serde_json::from_str(s).unwrap_or(fallback)
}

pub async fn get(pool: &SqlitePool) -> Result<Option<UserProfile>, CoreError> {
    let row = sqlx::query(
        "SELECT name, university, major, weekly_availability_json, preferred_study_times_json, \
         sleep_schedule_json, goals, onboarding_complete FROM user_profile WHERE id = 1",
    )
    .fetch_optional(pool)
    .await?;

    Ok(match row {
        None => None,
        Some(r) => Some(UserProfile {
            name: r.try_get("name")?,
            university: r.try_get("university")?,
            major: r.try_get("major")?,
            weekly_availability: json_or(&r.try_get::<String, _>("weekly_availability_json")?, serde_json::json!({})),
            preferred_study_times: json_or(&r.try_get::<String, _>("preferred_study_times_json")?, serde_json::json!([])),
            sleep_schedule: json_or(&r.try_get::<String, _>("sleep_schedule_json")?, serde_json::json!({})),
            goals: r.try_get("goals")?,
            onboarding_complete: r.try_get::<i64, _>("onboarding_complete")? != 0,
        }),
    })
}

pub async fn upsert(pool: &SqlitePool, profile: &UserProfile) -> Result<(), CoreError> {
    sqlx::query(
        "INSERT INTO user_profile (id, name, university, major, weekly_availability_json, \
         preferred_study_times_json, sleep_schedule_json, goals, onboarding_complete, updated_at) \
         VALUES (1, ?, ?, ?, ?, ?, ?, ?, ?, strftime('%Y-%m-%dT%H:%M:%fZ','now')) \
         ON CONFLICT(id) DO UPDATE SET name=excluded.name, university=excluded.university, \
         major=excluded.major, weekly_availability_json=excluded.weekly_availability_json, \
         preferred_study_times_json=excluded.preferred_study_times_json, \
         sleep_schedule_json=excluded.sleep_schedule_json, goals=excluded.goals, \
         onboarding_complete=excluded.onboarding_complete, updated_at=strftime('%Y-%m-%dT%H:%M:%fZ','now')",
    )
    .bind(&profile.name)
    .bind(&profile.university)
    .bind(&profile.major)
    .bind(serde_json::to_string(&profile.weekly_availability).unwrap_or_else(|_| "{}".into()))
    .bind(serde_json::to_string(&profile.preferred_study_times).unwrap_or_else(|_| "[]".into()))
    .bind(serde_json::to_string(&profile.sleep_schedule).unwrap_or_else(|_| "{}".into()))
    .bind(&profile.goals)
    .bind(profile.onboarding_complete as i64)
    .execute(pool)
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::connect_in_memory;

    #[tokio::test]
    async fn upsert_then_get_round_trips() {
        let pool = connect_in_memory().await.unwrap();
        assert!(get(&pool).await.unwrap().is_none());

        let profile = UserProfile {
            name: "Alex".into(),
            university: Some("SRU".into()),
            major: Some("CS".into()),
            weekly_availability: serde_json::json!({"mon": ["18:00-21:00"]}),
            preferred_study_times: serde_json::json!(["evening"]),
            sleep_schedule: serde_json::json!({"wake": "08:00", "sleep": "00:00"}),
            goals: Some("Graduate with honors".into()),
            onboarding_complete: true,
        };
        upsert(&pool, &profile).await.unwrap();

        let loaded = get(&pool).await.unwrap().unwrap();
        assert_eq!(loaded.name, "Alex");
        assert_eq!(loaded.onboarding_complete, true);
        assert_eq!(loaded.preferred_study_times, serde_json::json!(["evening"]));
    }
}
