//! Browser-based SQLite store and build-time fixture seeder for the static Chords demo.
#![deny(missing_docs)]

pub mod cgm;
pub mod driver;
pub mod follow_ups;
pub mod lingo;
pub mod reviews_memory;
pub(crate) mod schema;
#[cfg(not(target_arch = "wasm32"))]
pub mod seed;
pub mod sources_admin;
pub mod spruce;
pub mod store;
pub mod updates_briefings;

pub use cgm::{
    AgpHourBin, CgmClinicalSummary, CgmParseError, GlucoseMonitor, GlucoseReading,
    GlucoseReadingBound, GlucoseUnit, GriZone, MMOL_L_TO_MG_DL, MeasurementMethod,
    ParsedGlucoseImport, parse_commercial_glucose_csv,
};
pub use driver::{DbError, SqliteDriver};
pub use lingo::{
    Gad7Severity, LingoCgmSummary, LingoParseError, LingoReading, MentalHealthCheckIn,
    MentalHealthValidationError, Phq9Severity, parse_lingo_csv,
};
#[cfg(not(target_arch = "wasm32"))]
pub use store::ChatWorkLocation;
pub use store::{DemoSourceRecord, DemoStore, DraftSaveOutcome};

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    use physician_wire::{
        AnswerState, CitationTargetKind, ExchangeSteps, MessageSegment, RecordRead, Step,
        StepState, WorkResumeTarget,
    };
    use std::path::{Path, PathBuf};
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_test_dir(tag: &str) -> PathBuf {
        let ns = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("coda-demo-{tag}-{}-{ns}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn seeded_active_store(tag: &str, limit: usize) -> (PathBuf, DemoStore) {
        let dir = temp_test_dir(tag);
        let (seed_path, active_path) = (dir.join("seed.sqlite"), dir.join("active.sqlite"));
        let seed_store = DemoStore::new(SqliteDriver::open_file(&seed_path).unwrap());
        let units = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../patients/units");
        let count = seed::seed_from_units_dir(&seed_store, &units, limit).unwrap();
        assert_eq!(count, limit);
        drop(seed_store);
        std::fs::copy(&seed_path, &active_path).unwrap();
        let driver = SqliteDriver::open_file(&active_path).unwrap();
        driver.bind_reset_files(seed_path, active_path);
        (dir, DemoStore::new(driver))
    }

    fn assert_segments_verbatim_passages(
        label: &str,
        segments: &[MessageSegment],
        sources: &std::collections::BTreeMap<String, physician_wire::CitedSourceWire>,
    ) {
        let mut saw_citation = false;
        for seg in segments {
            if let MessageSegment::Citation {
                source_key,
                quote,
                target,
                ..
            } = seg
            {
                saw_citation = true;
                assert_eq!(
                    *target,
                    CitationTargetKind::Passage,
                    "{label}: citation {source_key} must use CitationTargetKind::Passage"
                );
                assert!(
                    !quote.is_empty(),
                    "{label}: citation quote must not be empty"
                );
                let cited = sources
                    .get(source_key)
                    .unwrap_or_else(|| panic!("{label}: missing source {source_key}"));
                assert!(
                    cited.body.contains(quote),
                    "{label}: body must contain verbatim quote {quote:?}\nbody:\n{}",
                    cited.body
                );
                assert!(
                    cited
                        .readable_parts
                        .iter()
                        .any(|part| part.text.contains(quote)),
                    "{label}: readable_parts must contain verbatim quote {quote:?}"
                );
            }
        }
        assert!(
            saw_citation,
            "{label}: expected at least one inline citation"
        );
    }

    #[tokio::test]
    async fn seeds_queries_mutates_and_resets() {
        let (dir, store) = seeded_active_store("rw", 6);

        assert!(!store.today().await.unwrap().rows.is_empty());
        let search = store.search("Kowalczyk").await.unwrap();
        assert_eq!(search.patients[0].patient_workspace_id, "unit-2013");

        let ws = store.workspace("unit-2013").await.unwrap();
        assert_eq!(ws.display_name, "Gary Kowalczyk");
        let rec_id = ws.undecided_recommendations[0].recommendation_id.clone();
        let before_plans = ws.active_plan_items.len();

        let msg_cmd = serde_json::json!({
            "type": "message.submit", "idempotency_key": "k-1",
            "payload": { "patient_workspace_id": "unit-2013", "chat_id": "chat-unit-2013-1", "text": "What is his atorvastatin dose and LDL?" }
        });
        assert_eq!(store.command(&msg_cmd).await.unwrap()["ok"], true);
        let snap = store
            .chat_snapshot("unit-2013", "chat-unit-2013-1")
            .await
            .unwrap();
        assert_eq!(snap.snapshot.transcript.len(), 2);
        let ExchangeSteps::Read(steps) = &snap.snapshot.transcript.last().unwrap().steps else {
            panic!("expected read steps");
        };
        assert!(matches!(steps[0], Step::SearchedChart { .. }));
        let Step::ReadRecords {
            state: StepState::Returned { result },
            ..
        } = &steps[1]
        else {
            panic!("expected records");
        };
        let RecordRead::Whole { version, .. } = &result[0] else {
            panic!("expected whole");
        };
        assert!(
            !store
                .source_record("unit-2013", &version.candidate_id)
                .await
                .unwrap()
                .body
                .is_empty()
        );

        let decide_cmd = serde_json::json!({
            "type": "plan.decide_recommendation",
            "payload": { "patient_workspace_id": "unit-2013", "recommendation_id": rec_id }
        });
        store.command(&decide_cmd).await.unwrap();
        let ws_after = store.workspace("unit-2013").await.unwrap();
        assert!(ws_after.undecided_recommendations.is_empty());
        assert_eq!(ws_after.active_plan_items.len(), before_plans + 1);

        assert!(matches!(
            store
                .save_draft("unit-2013", "chat-unit-2013-1", 0, "Check A1c")
                .await
                .unwrap(),
            DraftSaveOutcome::Saved(_)
        ));
        assert!(matches!(
            store
                .save_draft("unit-2013", "chat-unit-2013-1", 0, "Stale edit")
                .await
                .unwrap(),
            DraftSaveOutcome::Conflict(_)
        ));

        store.reset().await.unwrap();
        let ws_reset = store.workspace("unit-2013").await.unwrap();
        assert_eq!(ws_reset.active_plan_items.len(), before_plans);
        assert!(!ws_reset.undecided_recommendations.is_empty());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn passage_citations_structured_sources_and_basis_are_verbatim() {
        let (dir, store) = seeded_active_store("passages", 10);

        // Verify pre-seeded featured conversation has Passage citations, structured sources, and cited basis.
        let seeded_snap = store
            .chat_snapshot("unit-2013", "chat-unit-2013-1")
            .await
            .unwrap()
            .snapshot;
        let AnswerState::Finished {
            segments: seeded_segs,
            basis: Some(seeded_basis),
            ..
        } = &seeded_snap.transcript[0].answer
        else {
            panic!("expected seeded finished answer with basis");
        };
        assert_segments_verbatim_passages("seeded segments", seeded_segs, &seeded_snap.sources);
        assert_segments_verbatim_passages("seeded basis", seeded_basis, &seeded_snap.sources);

        // Inspect structured source records (labs/vitals/medications/unit-1672 range+text labs).
        let records_2013 = store.load_patient_records("unit-2013").unwrap();
        let lab_2013 = records_2013
            .iter()
            .find(|r| r.group == "results" && !r.fields.is_empty())
            .expect("expected structured lab record in unit-2013");
        assert_eq!(lab_2013.record_status, "final");
        assert!(!lab_2013.readable_parts.is_empty());
        let raw_lab = lab_2013
            .raw_content
            .as_deref()
            .expect("expected pretty raw_content");
        assert!(serde_json::from_str::<serde_json::Value>(raw_lab).is_ok());
        assert!(
            !raw_lab.contains('\n'),
            "raw_content stored in SourceRecords must remain compact single-line JSON"
        );

        let vit_2013 = records_2013
            .iter()
            .find(|r| r.group == "vitals")
            .expect("expected vitals record in unit-2013");
        assert!(
            vit_2013
                .fields
                .iter()
                .any(|f| f.name == "Blood pressure" && f.unit.as_deref() == Some("mmHg"))
        );
        assert!(vit_2013.raw_content.is_some());

        let records_1672 = store.load_patient_records("unit-1672").unwrap();
        assert!(!records_1672.is_empty());
        for rec in &records_1672 {
            assert!(
                !rec.body.contains(": null"),
                "unexpected null lab value in {}",
                rec.id
            );
        }
        assert!(
            records_1672
                .iter()
                .flat_map(|r| &r.fields)
                .any(|f| f.unit.is_some() && f.range.is_some() && f.status.is_some()),
            "expected unit-1672 lab fields with unit, range, and abnormality status"
        );

        // Synthesize a live SQLite chat answer and verify Passage quotes match body & readable_parts verbatim.
        store
            .command(&serde_json::json!({
                "type": "message.submit",
                "idempotency_key": "k-passage",
                "payload": {
                    "patient_workspace_id": "unit-2013",
                    "chat_id": "chat-unit-2013-1",
                    "text": "What is his atorvastatin dose and LDL?"
                }
            }))
            .await
            .unwrap();
        let updated = store
            .chat_snapshot("unit-2013", "chat-unit-2013-1")
            .await
            .unwrap()
            .snapshot;
        let AnswerState::Finished {
            segments,
            basis: Some(basis),
            ..
        } = &updated.transcript.last().unwrap().answer
        else {
            panic!("expected finished answer with basis");
        };
        assert_segments_verbatim_passages("query segments", segments, &updated.sources);
        assert_segments_verbatim_passages("query basis", basis, &updated.sources);
        for key in updated.sources.keys() {
            let fetched = store
                .cited_source("unit-2013", "chat-unit-2013-1", key)
                .await
                .unwrap()
                .source;
            assert!(!fetched.quote.is_empty());
            assert!(fetched.body.contains(&fetched.quote));
            let pretty_raw = fetched.raw_content.as_deref().expect("pretty raw_content");
            assert!(
                pretty_raw.contains('\n'),
                "cited_source must pretty-format raw_content on read"
            );
            assert!(serde_json::from_str::<serde_json::Value>(pretty_raw).is_ok());
        }
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn follow_up_memory_note_and_work_stop_resume_transitions() {
        let (dir, store) = seeded_active_store("stop-resume", 3);
        let chat_id = "chat-stop-resume";

        // Open new discussion and send first turn (is_new == true -> no ReadConversation).
        assert_eq!(
            store
                .command(&serde_json::json!({
                    "type": "discussion.open",
                    "idempotency_key": "disc-1",
                    "payload": { "patient_workspace_id": "unit-2013", "chat_id": chat_id }
                }))
                .await
                .unwrap()["outcome"],
            "accepted"
        );
        store
            .command(&serde_json::json!({
                "type": "message.submit",
                "idempotency_key": "turn-1",
                "payload": {
                    "patient_workspace_id": "unit-2013",
                    "chat_id": chat_id,
                    "text": "Summarize current diabetes medications."
                }
            }))
            .await
            .unwrap();
        let snap1 = store
            .chat_snapshot("unit-2013", chat_id)
            .await
            .unwrap()
            .snapshot;
        let ExchangeSteps::Read(steps1) = &snap1.transcript[0].steps else {
            panic!("expected steps on turn 1");
        };
        assert!(
            !steps1
                .iter()
                .any(|s| matches!(s, Step::ReadConversation { .. }))
        );

        // Follow-up turn requesting memory note (!is_new -> ReadConversation + SavedMemoryNote).
        store
            .command(&serde_json::json!({
                "type": "message.submit",
                "idempotency_key": "turn-2",
                "payload": {
                    "patient_workspace_id": "unit-2013",
                    "chat_id": chat_id,
                    "text": "Remember to recheck fasting lipid panel in 4 weeks."
                }
            }))
            .await
            .unwrap();
        // Queue one message to test work.stop returning queued messages.
        store
            .command(&serde_json::json!({
                "type": "message.submit",
                "idempotency_key": "queued-1",
                "payload": {
                    "patient_workspace_id": "unit-2013",
                    "chat_id": chat_id,
                    "text": "Check creatinine too",
                    "delivery": "send_next"
                }
            }))
            .await
            .unwrap();

        let snap2 = store
            .chat_snapshot("unit-2013", chat_id)
            .await
            .unwrap()
            .snapshot;
        let ExchangeSteps::Read(steps2) = &snap2.transcript[1].steps else {
            panic!("expected steps on turn 2");
        };
        assert!(steps2.iter().any(|s| matches!(
            s,
            Step::ReadConversation {
                state: StepState::Returned { result: () }
            }
        )));
        assert!(steps2.iter().any(|s| matches!(
            s,
            Step::SavedMemoryNote {
                state: StepState::Returned { result: () }
            }
        )));
        let memory = store.patient_memory("unit-2013").await.unwrap();
        assert!(
            memory
                .direction
                .unwrap()
                .markdown
                .contains("Remember to recheck fasting lipid panel in 4 weeks.")
        );

        // Stop exchange 2 -> transitions to AnswerState::Stopped and drains queued messages.
        let work_item_id = snap2.transcript[1].work_item_id.0.clone();
        let stopped = store
            .command(&serde_json::json!({
                "type": "work.stop",
                "idempotency_key": "stop-1",
                "payload": {
                    "patient_workspace_id": "unit-2013",
                    "chat_id": chat_id,
                    "work_item_id": work_item_id
                }
            }))
            .await
            .unwrap();
        assert_eq!(stopped["outcome"], "accepted");
        assert_eq!(
            stopped["returned_messages"],
            serde_json::json!(["Check creatinine too"])
        );
        let snap_stopped = store
            .chat_snapshot("unit-2013", chat_id)
            .await
            .unwrap()
            .snapshot;
        assert!(snap_stopped.queued.is_empty());
        let AnswerState::Stopped {
            resume: Some(WorkResumeTarget {
                expected_run_id, ..
            }),
        } = &snap_stopped.transcript[1].answer
        else {
            panic!("expected AnswerState::Stopped with WorkResumeTarget");
        };

        // Mismatched expected_run_id must be rejected cleanly.
        let stale_resume = store
            .command(&serde_json::json!({
                "type": "work.resume",
                "idempotency_key": "res-stale",
                "payload": {
                    "patient_workspace_id": "unit-2013",
                    "chat_id": chat_id,
                    "work_item_id": work_item_id,
                    "expected_run_id": "wrong-run"
                }
            }))
            .await
            .unwrap();
        assert_eq!(stale_resume["outcome"], "rejected");

        // Matching expected_run_id completes the Stopped exchange into AnswerState::Finished.
        let resumed = store
            .command(&serde_json::json!({
                "type": "work.resume",
                "idempotency_key": "res-1",
                "payload": {
                    "patient_workspace_id": "unit-2013",
                    "chat_id": chat_id,
                    "work_item_id": work_item_id,
                    "expected_run_id": expected_run_id
                }
            }))
            .await
            .unwrap();
        assert_eq!(resumed["outcome"], "accepted");
        let snap_resumed = store
            .chat_snapshot("unit-2013", chat_id)
            .await
            .unwrap()
            .snapshot;
        let AnswerState::Finished {
            segments,
            basis: Some(basis),
            ..
        } = &snap_resumed.transcript[1].answer
        else {
            panic!("expected AnswerState::Finished after work.resume");
        };
        assert_segments_verbatim_passages("resumed segments", segments, &snap_resumed.sources);
        assert_segments_verbatim_passages("resumed basis", basis, &snap_resumed.sources);
        let ExchangeSteps::Read(resumed_steps) = &snap_resumed.transcript[1].steps else {
            panic!("expected steps after resume");
        };
        assert!(resumed_steps.iter().any(|s| matches!(
            s,
            Step::ReadConversation {
                state: StepState::Returned { result: () }
            }
        )));

        // Verify AnswerState::Working -> work.stop and CutOff / CouldntFinish -> work.resume.
        for (tag, state_ctor) in [
            (
                "working",
                AnswerState::Working {
                    stop: physician_wire::WorkStopTarget {
                        work_item_id: physician_wire::WorkItemId("wi-working".into()),
                    },
                    writing: true,
                },
            ),
            (
                "cutoff",
                AnswerState::CutOff {
                    try_again: Some(WorkResumeTarget {
                        work_item_id: physician_wire::WorkItemId("wi-cutoff".into()),
                        expected_run_id: "run-cutoff-1".into(),
                    }),
                },
            ),
            (
                "couldnt-finish",
                AnswerState::CouldntFinish {
                    try_again: Some(WorkResumeTarget {
                        work_item_id: physician_wire::WorkItemId("wi-fail".into()),
                        expected_run_id: "run-fail-1".into(),
                    }),
                },
            ),
        ] {
            let cid = format!("chat-state-{tag}");
            let custom_snap = physician_wire::ChatSnapshot {
                sources: Default::default(),
                transcript: vec![physician_wire::Exchange {
                    work_item_id: physician_wire::WorkItemId(
                        if tag == "working" {
                            "wi-working"
                        } else if tag == "cutoff" {
                            "wi-cutoff"
                        } else {
                            "wi-fail"
                        }
                        .into(),
                    ),
                    messages: vec![physician_wire::PhysicianMessage {
                        id: physician_wire::MessageKey(format!("m-{tag}")),
                        text: "What is his latest LDL?".into(),
                        saved_at: physician_wire::SavedAt(store::DEMO_BASE_UNIX + 50),
                    }],
                    steps: ExchangeSteps::Read(vec![Step::SearchedChart {
                        query: "ldl".into(),
                        group: None,
                        page: physician_wire::SearchPage::First,
                        state: StepState::Running,
                    }]),
                    kept: Vec::new(),
                    answer: state_ctor,
                }],
                feedback: physician_wire::SnapshotFeedback::Loaded,
                queued: Vec::new(),
                archived: false,
            };
            store
                .upsert_chat_snapshot(
                    "unit-2013",
                    &cid,
                    "State recovery",
                    store::DEMO_NOW,
                    &custom_snap,
                )
                .unwrap();

            if tag == "working" {
                let stop_res = store
                    .command(&serde_json::json!({
                        "type": "work.stop",
                        "idempotency_key": "stop-working",
                        "payload": {
                            "patient_workspace_id": "unit-2013",
                            "chat_id": cid
                        }
                    }))
                    .await
                    .unwrap();
                assert_eq!(stop_res["outcome"], "accepted");
                let stopped = store
                    .chat_snapshot("unit-2013", &cid)
                    .await
                    .unwrap()
                    .snapshot;
                assert!(matches!(
                    stopped.transcript[0].steps,
                    ExchangeSteps::Read(ref s) if matches!(s[0], Step::SearchedChart { state: StepState::Stopped, .. })
                ));
                let AnswerState::Stopped {
                    resume: Some(target),
                } = &stopped.transcript[0].answer
                else {
                    panic!("expected Stopped");
                };
                let res = store
                    .command(&serde_json::json!({
                        "type": "work.resume",
                        "idempotency_key": "res-working",
                        "payload": {
                            "patient_workspace_id": "unit-2013",
                            "chat_id": cid,
                            "work_item_id": target.work_item_id,
                            "expected_run_id": target.expected_run_id
                        }
                    }))
                    .await
                    .unwrap();
                assert_eq!(res["outcome"], "accepted");
            } else {
                let wid = if tag == "cutoff" {
                    "wi-cutoff"
                } else {
                    "wi-fail"
                };
                let run = if tag == "cutoff" {
                    "run-cutoff-1"
                } else {
                    "run-fail-1"
                };
                let res = store
                    .command(&serde_json::json!({
                        "type": "work.resume",
                        "idempotency_key": format!("res-{tag}"),
                        "payload": {
                            "patient_workspace_id": "unit-2013",
                            "work_item_id": wid,
                            "expected_run_id": run
                        }
                    }))
                    .await
                    .unwrap();
                assert_eq!(res["outcome"], "accepted");
            }
            let finished = store
                .chat_snapshot("unit-2013", &cid)
                .await
                .unwrap()
                .snapshot;
            assert!(matches!(
                finished.transcript[0].answer,
                AnswerState::Finished { ref segments, basis: Some(_), .. }
                    if segments.iter().any(|s| matches!(s, MessageSegment::Citation { target: CitationTargetKind::Passage, .. }))
            ));
        }

        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn maya_lin_lingo_cgm_and_mental_health_tracking_roundtrip() {
        let (dir, store) = seeded_active_store("lingo-mh", 6);

        let search = store.search("Maya Lin").await.unwrap();
        assert_eq!(search.patients[0].patient_workspace_id, "unit-lingo-mh-1");

        let ws = store.workspace("unit-lingo-mh-1").await.unwrap();
        assert_eq!(ws.display_name, "Maya Lin");
        assert!(!ws.active_follow_ups.is_empty());

        let today = store.today().await.unwrap();
        assert!(
            today
                .rows
                .iter()
                .any(|r| r.patient_workspace_id == "unit-lingo-mh-1"),
            "Maya Lin must appear prominently in Today"
        );

        let briefings = store.workspace_briefings("unit-lingo-mh-1").await.unwrap();
        assert!(
            briefings
                .briefings
                .iter()
                .any(|b| b.visit_label == "Mental health & Lingo CGM review")
        );

        let mem = store.patient_memory("unit-lingo-mh-1").await.unwrap();
        assert!(
            mem.summary
                .as_ref()
                .is_some_and(|d| d.markdown.contains("Abbott Lingo"))
        );

        let readings = parse_lingo_csv(
            "timestamp,glucose_mg_dl,lingo_count,note\n\
             2026-08-27T08:00:00Z,88,0,Fasting\n\
             2026-08-27T13:00:00Z,158,10,Post-lunch spike\n\
             2026-08-27T15:00:00Z,74,0,Afternoon dip\n",
        )
        .unwrap();
        let (cgm_rec, summary) = store
            .ingest_lingo_cgm(
                "unit-lingo-mh-1",
                "Day-15 Lingo Biosensor Upload",
                &readings,
            )
            .await
            .unwrap();
        assert_eq!(summary.count, 3);
        assert_eq!(summary.spike_count_over_140, 1);
        let fetched_cgm = store
            .source_record("unit-lingo-mh-1", &cgm_rec.id)
            .await
            .unwrap();
        assert!(
            fetched_cgm
                .fields
                .iter()
                .any(|f| f.key == "lingo_peak_glucose" && f.value == "158.0")
        );

        let checkin = MentalHealthCheckIn {
            recorded_at: "2026-08-27T15:30:00Z".into(),
            mood_1_to_10: 7,
            anxiety_1_to_10: 4,
            energy_1_to_10: 7,
            sleep_hours: 7.5,
            phq9_score: Some(6),
            gad7_score: Some(9),
            note: "Steady afternoon energy after protein-first lunch.".into(),
        };
        let mh_rec = store
            .log_mental_health_checkin("unit-lingo-mh-1", &checkin)
            .await
            .unwrap();
        let fetched_mh = store
            .source_record("unit-lingo-mh-1", &mh_rec.id)
            .await
            .unwrap();
        assert!(
            fetched_mh
                .fields
                .iter()
                .any(|f| f.key == "gad7_score" && f.name == "GAD-7 (Mild)")
        );

        let dexcom_import = parse_commercial_glucose_csv(
            "Index,Timestamp (YYYY-MM-DDThh:mm:ss),Event Type,Source Device ID,Glucose Value (mg/dL),Glucose Rate of Change (mg/dL/min)\n\
             1,2026-08-27T06:00:00,EGV,DX-G7,52,-1.2\n\
             2,2026-08-27T09:00:00,EGV,DX-G7,114,0.4\n\
             3,2026-08-27T13:00:00,EGV,DX-G7,218,2.1\n\
             4,2026-08-27T18:00:00,EGV,DX-G7,102,-0.3\n",
            None,
        )
        .unwrap();
        assert_eq!(dexcom_import.monitor, GlucoseMonitor::DexcomClarity);
        let (dex_rec, dex_summary) = store
            .ingest_commercial_cgm(
                "unit-lingo-mh-1",
                "Dexcom G7 Clarity Consultation Upload",
                &dexcom_import,
            )
            .await
            .unwrap();
        assert_eq!(dex_summary.count, 4);
        assert_eq!(dex_summary.agp_hourly.len(), 4);
        let fetched_dex = store
            .source_record("unit-lingo-mh-1", &dex_rec.id)
            .await
            .unwrap();
        assert_eq!(fetched_dex.source_label, "Dexcom Clarity CGM");
        assert!(
            fetched_dex
                .fields
                .iter()
                .any(|f| f.key == "cgm_gmi_percent")
        );
        assert!(fetched_dex.fields.iter().any(|f| f.key == "cgm_gri"));

        let _ = std::fs::remove_dir_all(dir);
    }
}
