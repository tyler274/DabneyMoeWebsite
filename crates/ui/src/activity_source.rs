//! Server-only configured local files. No request parameter chooses a path or URL.
//! Files must be outside the public site root and atomically replaced by a trusted
//! publisher. No cache or last-good fallback: every read rechecks authorization.
use crate::activity_snapshot::*;
use crate::community_activity::{Chart, Dashboard, Row};
use serde::de::DeserializeOwned;
use std::collections::HashSet;
use std::io::Read;

const MAX_DOCUMENT_BYTES: u64 = 1_048_576;

pub fn load() -> Option<Dashboard> {
    let snapshot = read_configured("COMMUNITY_ACTIVITY_SNAPSHOT_FILE")?;
    let authorization = read_configured("COMMUNITY_ACTIVITY_AUTHORIZATION_FILE")?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_secs()
        .try_into()
        .ok()?;
    project(snapshot, authorization, now)
}

fn read_configured<T: DeserializeOwned>(key: &str) -> Option<T> {
    let path = std::path::PathBuf::from(std::env::var_os(key)?);
    if !path.is_absolute() {
        return None;
    }
    if !std::fs::metadata(&path).ok()?.is_file() {
        return None;
    }
    let file = std::fs::File::open(path).ok()?;
    if !file.metadata().ok()?.is_file() {
        return None;
    }
    let mut bytes = Vec::new();
    file.take(MAX_DOCUMENT_BYTES + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    if bytes.len() as u64 > MAX_DOCUMENT_BYTES {
        return None;
    }
    serde_json::from_slice(&bytes).ok()
}

fn fresh(at: i64, now: i64, max_age: i64) -> bool {
    at > 0 && at <= now && now - at < max_age
}

pub fn project(snapshot: Snapshot, authorization: Authorization, now: i64) -> Option<Dashboard> {
    let Snapshot::V1(snapshot) = snapshot;
    let Authorization::V1 { checked_at, grants } = authorization;
    if !fresh(snapshot.generated_at, now, SNAPSHOT_MAX_AGE)
        || !fresh(checked_at, now, AUTHORIZATION_MAX_AGE)
    {
        return None;
    }
    let earliest = now.checked_sub(RETENTION)?;
    let latest = now.checked_sub(PUBLICATION_DELAY)?;
    let allowed = |subject: &str, scope: Scope, at: i64| {
        grants.iter().any(|grant| {
            grant.subject == subject
                && grant.scope == scope
                && grant.effective_at <= at
                && grant.effective_at > 0
                && grant.expires_at > now
        })
    };
    let in_window = |at: i64| at > earliest && at <= latest;
    let healthy = |start: i64, end: i64| {
        snapshot.health.iter().any(|h| {
            h.start <= start
                && h.end >= end
                && h.process == Health::Healthy
                && h.gateway == Health::Healthy
                && h.collection == Health::Healthy
        })
    };
    // A malformed, overlapping timeline must not accidentally authorize observations.
    let mut previous_end = 0;
    for h in &snapshot.health {
        if h.start <= 0
            || h.start < previous_end
            || h.end <= h.start
            || h.end > snapshot.generated_at
        {
            return None;
        }
        previous_end = h.end;
    }
    let mut charts = Vec::new();
    let health_rows = snapshot.health.iter().filter_map(|h| {
        let start = h.start.max(earliest + 1);
        let end = h.end.min(latest);
        (start < end).then(|| Row { at: start,
            description: format!("Until {}: process {}; gateway {}; collection {}. Uncovered intervals are unknown.", crate::community_activity::utc_time(end), h.process.label(), h.gateway.label(), h.collection.label()), count: None })
    }).collect();
    charts.push(Chart {
        title: "Listener health (delayed observations, not current uptime)".into(),
        rows: health_rows,
    });
    let mut rows = Vec::new();
    let mut hours = HashSet::new();
    if snapshot.operator_subject.is_empty()
        || snapshot
            .operator
            .as_ref()
            .is_some_and(|operator| operator.subject != snapshot.operator_subject)
    {
        return None;
    }
    let operator_subject = snapshot.operator_subject.as_str();
    for hour in snapshot.hourly_messages {
        if hour.start <= 0 || hour.start % 3600 != 0 || !hours.insert(hour.start) {
            return None;
        }
        let end = hour.start.checked_add(3600)?;
        if !in_window(hour.start) || end > latest || !healthy(hour.start, end) {
            continue;
        }
        let mut subjects = HashSet::new();
        let mut count = 0_u64;
        let mut cohort = 0;
        let mut withdrawn = false;
        for contribution in hour.contributions {
            if !subjects.insert(contribution.subject.clone()) {
                return None;
            }
            if contribution.count == 0 || operator_subject == contribution.subject.as_str() {
                continue;
            }
            if !allowed(&contribution.subject, Scope::AggregateMessages, hour.start) {
                withdrawn = true;
                continue;
            }
            cohort += 1;
            count = count.checked_add(u64::from(contribution.count))?;
        }
        // Never publish a recomputed total by subtracting a withdrawn subject.
        // Producer must also invalidate previously published affected hours rather
        // than emitting a revised contributor list (see the snapshot contract).
        if !withdrawn && cohort >= MIN_COHORT {
            rows.push(Row {
                at: hour.start,
                description: format!("{count} messages from consenting contributors"),
                count: Some(count),
            });
        }
    }
    rows.sort_by_key(|r| r.at);
    charts.push(Chart {
        title: "Aggregate message counts (cohorts below five suppressed)".into(),
        rows,
    });
    if let Some(operator) = snapshot.operator {
        let mut rows = Vec::new();
        let mut hours = HashSet::new();
        for hour in operator.hours {
            if hour.start <= 0 || hour.start % 3600 != 0 || !hours.insert(hour.start) {
                return None;
            }
            let end = hour.start.checked_add(3600)?;
            if in_window(hour.start)
                && end <= latest
                && healthy(hour.start, end)
                && allowed(&operator.subject, Scope::IdentifiedMessages, hour.start)
            {
                rows.push(Row {
                    at: hour.start,
                    description: format!("{} messages", hour.count),
                    count: Some(u64::from(hour.count)),
                });
            }
        }
        rows.sort_by_key(|r| r.at);
        if !rows.is_empty() {
            charts.push(Chart {
                title: format!("Operator messages: {}", operator.label),
                rows,
            });
        }
    }
    let mut subjects = HashSet::new();
    for member in snapshot.presence {
        if !subjects.insert(member.subject.clone()) {
            return None;
        }
        let mut rows = Vec::new();
        let mut times = HashSet::new();
        for transition in member.transitions {
            if !times.insert(transition.at) {
                return None;
            }
            if in_window(transition.at)
                && allowed(&member.subject, Scope::PublicPresence, transition.at)
                && allowed(&member.subject, Scope::PresenceRetention, transition.at)
            {
                let status = if healthy(transition.at, transition.at.checked_add(1)?) {
                    transition.status
                } else {
                    DiscordStatus::Unknown
                };
                rows.push(Row {
                    at: transition.at,
                    description: format!(
                        "Discord-reported {}; a transition, not continuous presence",
                        status.label()
                    ),
                    count: None,
                });
            }
        }
        rows.sort_by_key(|r| r.at);
        if !rows.is_empty() {
            charts.push(Chart {
                title: format!("Discord-reported presence transitions: {}", member.label),
                rows,
            });
        }
    }
    // Also expire the rendered view when any grant expires, even before the authority lease.
    let valid_until = grants
        .iter()
        .filter(|g| g.expires_at > now)
        .map(|g| g.expires_at)
        .chain([
            checked_at + AUTHORIZATION_MAX_AGE,
            snapshot.generated_at + SNAPSHOT_MAX_AGE,
        ])
        .min()?;
    Some(Dashboard {
        valid_for_seconds: (valid_until - now).try_into().ok()?,
        charts,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    const NOW: i64 = 1_800_000_000;
    fn inputs() -> (Snapshot, Authorization) {
        let snapshot =
            serde_json::from_str(include_str!("../tests/fixtures/activity-v1.json")).unwrap();
        let grants = ["a", "b", "c", "d", "e", "f"]
            .into_iter()
            .flat_map(|subject| {
                [
                    Scope::AggregateMessages,
                    Scope::IdentifiedMessages,
                    Scope::PresenceRetention,
                    Scope::PublicPresence,
                ]
                .into_iter()
                .map(move |scope| Grant {
                    subject: subject.into(),
                    scope,
                    effective_at: NOW - RETENTION - 1,
                    expires_at: NOW + 1000,
                })
            })
            .collect();
        (
            snapshot,
            Authorization::V1 {
                checked_at: NOW,
                grants,
            },
        )
    }
    #[test]
    fn synthetic_boundaries_and_unknown_gap() {
        let (snapshot, auth) = inputs();
        let output = project(snapshot, auth, NOW).unwrap();
        let presence = output.charts.last().unwrap();
        assert_eq!(presence.rows.len(), 3);
        assert_eq!(presence.rows[0].at, NOW - RETENTION + 1);
        assert!(presence.rows[0].description.contains("unknown"));
        assert_eq!(presence.rows[2].at, NOW - PUBLICATION_DELAY);
        assert!(!presence.rows[2].description.contains("unknown"));
        assert_eq!(output.charts[1].rows[0].count, Some(20));
    }
    #[test]
    fn withdrawal_rechecks_authority_not_snapshot_membership() {
        let (
            snapshot,
            Authorization::V1 {
                checked_at,
                mut grants,
            },
        ) = inputs();
        grants.retain(|g| g.subject != "a" && g.subject != "b");
        let output = project(snapshot, Authorization::V1 { checked_at, grants }, NOW).unwrap();
        assert_eq!(output.charts.len(), 2);
        assert!(output.charts[1].rows.is_empty());
        assert!(!serde_json::to_string(&output)
            .unwrap()
            .contains("Synthetic Alice"));
    }
    #[test]
    fn stale_future_and_expired_authority_fail_closed() {
        let (snapshot, auth) = inputs();
        assert!(project(snapshot.clone(), auth.clone(), NOW + AUTHORIZATION_MAX_AGE).is_none());
        assert!(project(snapshot.clone(), auth, NOW - 1).is_none());
        let Authorization::V1 { grants, .. } = inputs().1;
        assert!(project(
            snapshot,
            Authorization::V1 {
                checked_at: NOW + 300,
                grants
            },
            NOW + 300
        )
        .is_none());
    }
    #[test]
    fn unknown_versions_and_content_fields_are_rejected() {
        assert!(serde_json::from_str::<Snapshot>("{\"version\":\"2\"}").is_err());
        let text = include_str!("../tests/fixtures/activity-v1.json").replace(
            "\"generated_at\":",
            "\"message_body\":\"private\",\"generated_at\":",
        );
        assert!(serde_json::from_str::<Snapshot>(&text).is_err());
    }
    #[test]
    fn operator_series_withdrawal_does_not_change_published_aggregate() {
        let (Snapshot::V1(mut snapshot), auth) = inputs();
        let before = project(Snapshot::V1(snapshot.clone()), auth.clone(), NOW).unwrap();
        snapshot.operator = None;
        let after = project(Snapshot::V1(snapshot), auth, NOW).unwrap();
        assert_eq!(before.charts[1].rows[0].count, Some(20));
        assert_eq!(
            before.charts[1].rows[0].count,
            after.charts[1].rows[0].count
        );
    }

    #[test]
    fn aggregate_excludes_identified_operator_and_zero_padding() {
        let (Snapshot::V1(mut snapshot), auth) = inputs();
        snapshot.hourly_messages[0]
            .contributions
            .last_mut()
            .unwrap()
            .count = 0;
        let output = project(Snapshot::V1(snapshot), auth, NOW).unwrap();
        assert!(output.charts[1].rows.is_empty());
    }

    #[test]
    fn withdrawal_suppresses_whole_hour_even_above_threshold() {
        let (
            Snapshot::V1(mut snapshot),
            Authorization::V1 {
                checked_at,
                mut grants,
            },
        ) = inputs();
        snapshot.operator = None;
        snapshot.hourly_messages[0]
            .contributions
            .push(Contribution {
                subject: "g".into(),
                count: 7,
            });
        grants.push(Grant {
            subject: "g".into(),
            scope: Scope::AggregateMessages,
            effective_at: NOW - RETENTION,
            expires_at: NOW + 500,
        });
        grants.retain(|grant| grant.subject != "b");
        let output = project(
            Snapshot::V1(snapshot),
            Authorization::V1 { checked_at, grants },
            NOW,
        )
        .unwrap();
        assert!(output.charts[1].rows.is_empty());
    }

    #[test]
    fn future_authority_and_missing_retention_scope_fail_closed() {
        let (snapshot, Authorization::V1 { mut grants, .. }) = inputs();
        assert!(project(
            snapshot.clone(),
            Authorization::V1 {
                checked_at: NOW + 1,
                grants: grants.clone()
            },
            NOW
        )
        .is_none());
        grants.retain(|grant| grant.scope != Scope::PresenceRetention);
        let output = project(
            snapshot,
            Authorization::V1 {
                checked_at: NOW,
                grants,
            },
            NOW,
        )
        .unwrap();
        assert!(!output
            .charts
            .iter()
            .any(|chart| chart.title.contains("presence transitions")));
    }

    #[test]
    fn operator_withdrawal_sequence_keeps_aggregate_stable() {
        let (
            Snapshot::V1(mut snapshot),
            Authorization::V1 {
                checked_at,
                mut grants,
            },
        ) = inputs();
        let before = project(
            Snapshot::V1(snapshot.clone()),
            Authorization::V1 {
                checked_at,
                grants: grants.clone(),
            },
            NOW,
        )
        .unwrap();
        assert_eq!(before.charts[1].rows[0].count, Some(20));
        grants.retain(|grant| grant.subject != "a");
        snapshot.operator = None;
        snapshot.generated_at += 1;
        let after = project(
            Snapshot::V1(snapshot),
            Authorization::V1 {
                checked_at: checked_at + 1,
                grants,
            },
            NOW + 1,
        )
        .unwrap();
        assert_eq!(
            after.charts[1].rows[0].count,
            before.charts[1].rows[0].count
        );
        assert!(!serde_json::to_string(&after)
            .unwrap()
            .contains("Synthetic Alice"));
    }

    #[test]
    fn withdrawal_sequence_removes_hour_then_stale_authority_closes_all_output() {
        let (
            snapshot,
            Authorization::V1 {
                checked_at,
                mut grants,
            },
        ) = inputs();
        let before = project(
            snapshot.clone(),
            Authorization::V1 {
                checked_at,
                grants: grants.clone(),
            },
            NOW,
        )
        .unwrap();
        assert!(!before.charts[1].rows.is_empty());
        grants.retain(|grant| grant.subject != "b");
        let withdrawn = Authorization::V1 {
            checked_at: NOW + 1,
            grants,
        };
        let after = project(snapshot.clone(), withdrawn.clone(), NOW + 1).unwrap();
        assert!(after.charts[1].rows.is_empty());
        assert!(project(snapshot, withdrawn, NOW + 1 + AUTHORIZATION_MAX_AGE).is_none());
    }

    #[test]
    fn mismatched_operator_identity_rejects_document() {
        let (Snapshot::V1(mut snapshot), auth) = inputs();
        snapshot.operator.as_mut().unwrap().subject = "different".into();
        assert!(project(Snapshot::V1(snapshot), auth, NOW).is_none());
    }

    #[test]
    fn omission_and_expiry_remove_individuals() {
        let (Snapshot::V1(mut snapshot), mut auth) = inputs();
        snapshot.presence.clear();
        snapshot.operator = None;
        assert_eq!(
            project(Snapshot::V1(snapshot), auth.clone(), NOW)
                .unwrap()
                .charts
                .len(),
            2
        );
        let Authorization::V1 { grants, .. } = &mut auth;
        for grant in grants {
            grant.expires_at = NOW;
        }
        assert_eq!(project(inputs().0, auth, NOW).unwrap().charts.len(), 2);
    }
}
