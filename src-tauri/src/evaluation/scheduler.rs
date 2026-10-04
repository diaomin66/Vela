//! Schedule claiming happens before inference; missed intervals never accumulate.
use super::{storage, types::EvaluationPlan};
use crate::core::AppPaths;
use chrono::{DateTime, Duration, Utc};

pub(super) fn next_time(now: DateTime<Utc>, minutes: u32) -> String {
    (now + Duration::minutes(i64::from(minutes))).to_rfc3339()
}
pub(super) fn due(enabled: bool, next: Option<&str>, now: DateTime<Utc>) -> bool {
    enabled
        && next
            .and_then(|value| DateTime::parse_from_rfc3339(value).ok())
            .is_none_or(|value| value <= now)
}
pub(super) fn claim(
    paths: &AppPaths,
    now: DateTime<Utc>,
) -> Result<Option<EvaluationPlan>, String> {
    let _lock = paths.lock()?;
    let mut store = storage::read(paths)?;
    if !due(
        store.plan.schedule_enabled,
        store.next_run_at.as_deref(),
        now,
    ) {
        return Ok(None);
    }
    store.next_run_at = Some(next_time(
        now,
        store.plan.effective_interval_minutes().clamp(10, 10080),
    ));
    storage::write(paths, &store)?;
    Ok(Some(store.plan))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn disabled_schedules_never_run_and_missed_intervals_are_claimed_once() {
        let directory = tempfile::tempdir().unwrap();
        let paths = AppPaths {
            data: directory.path().join("data"),
            config: directory.path().join("unused"),
            helper: directory.path().join("helper"),
            locations: None,
        };
        let now = Utc::now();
        assert!(claim(&paths, now).unwrap().is_none());
        let mut store = storage::read(&paths).unwrap();
        store.plan.schedule_enabled = true;
        store.plan.interval_hours = 6;
        store.plan.interval_minutes = None;
        store.next_run_at = Some((now - Duration::days(10)).to_rfc3339());
        storage::write(&paths, &store).unwrap();
        assert!(claim(&paths, now).unwrap().is_some());
        assert!(claim(&paths, now).unwrap().is_none());
        assert_eq!(
            storage::read(&paths).unwrap().next_run_at,
            Some(next_time(now, 360))
        );
        assert!(!paths.config.exists());
    }
}
