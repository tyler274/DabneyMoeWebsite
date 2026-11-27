//! Pure-Rust HL7 v2.x pipe-delimited message parser (zero external deps;
//! runs identically in SSR, browser WebAssembly, and Tauri).
//!
//! Supports `ADT`, `ORU`, `MDM`, and `SIU` messages covering `MSH`, `PID`,
//! `PV1`, `OBR`, `OBX`, `AL1`, `DG1`, `RXE`, `TXA`, `NTE`, and `SCH` segments,
//! honoring dynamic field/component/repetition/escape/subcomponent separators
//! parsed directly from `MSH-1` and `MSH-2` and exact HL7 v2.5.1 field indices.

use crate::ccda::format_hl7_date;
use crate::ehr_portal::{sanitize_id_slug, AllergyEntry, FhirImportResult, FhirResourceWarning};
use crate::{ChartRecord, ConfidentialityClass, Medication, PatientChart, Problem, RecordKind};

#[derive(Clone, Copy, Debug)]
struct Hl7Delimiters {
    field: char,
    component: char,
    repetition: char,
    escape: char,
    subcomponent: char,
}

impl Hl7Delimiters {
    fn from_msh(msh_line: &str) -> Result<(Self, Vec<&str>), String> {
        let mut chars = msh_line.chars();
        let m = chars.next();
        let s = chars.next();
        let h = chars.next();
        if (m, s, h) != (Some('M'), Some('S'), Some('H')) {
            return Err("HL7 v2 message must begin with an `MSH` header segment.".into());
        }
        let field = chars
            .next()
            .filter(|c| c.is_ascii_punctuation())
            .ok_or_else(|| "MSH-1 field separator is missing or invalid.".to_string())?;

        // Split MSH on field separator:
        // parts[0] = "MSH", parts[1] = MSH-2 (encoding chars), parts[8] = MSH-9, etc.
        let parts: Vec<&str> = msh_line.split(field).collect();
        let enc = parts
            .get(1)
            .copied()
            .ok_or_else(|| "MSH-2 encoding characters are missing.".to_string())?;
        let mut enc_chars = enc.chars();
        let component = enc_chars.next().unwrap_or('^');
        let repetition = enc_chars.next().unwrap_or('~');
        let escape = enc_chars.next().unwrap_or('\\');
        let subcomponent = enc_chars.next().unwrap_or('&');

        Ok((
            Self {
                field,
                component,
                repetition,
                escape,
                subcomponent,
            },
            parts,
        ))
    }

    /// Reads 1-based HL7 field `n` on non-MSH segments (`parts[n]`).
    fn field<'a>(&self, parts: &[&'a str], n: usize) -> &'a str {
        parts.get(n).copied().unwrap_or("").trim()
    }

    /// Reads 1-based component `comp` (1 = first component) from a field value.
    fn component<'a>(&self, value: &'a str, comp: usize) -> &'a str {
        if comp == 0 {
            return "";
        }
        value
            .split(self.component)
            .nth(comp - 1)
            .unwrap_or("")
            .trim()
    }

    /// Unescapes standard HL7 v2 escape sequences (`\F\`, `\S\`, `\T\`, `\R\`,
    /// `\E\`, `\.br\`).
    fn unescape(&self, raw: &str) -> String {
        if !raw.contains(self.escape) {
            return raw.to_string();
        }
        let mut out = String::with_capacity(raw.len());
        let mut rest = raw;
        while let Some(idx) = rest.find(self.escape) {
            out.push_str(&rest[..idx]);
            let after_start = &rest[idx + self.escape.len_utf8()..];
            if let Some(end) = after_start.find(self.escape) {
                let seq = &after_start[..end];
                match seq {
                    "F" => out.push(self.field),
                    "S" => out.push(self.component),
                    "T" => out.push(self.subcomponent),
                    "R" => out.push(self.repetition),
                    "E" => out.push(self.escape),
                    ".br" => out.push('\n'),
                    other => {
                        out.push(self.escape);
                        out.push_str(other);
                        out.push(self.escape);
                    }
                }
                rest = &after_start[end + self.escape.len_utf8()..];
            } else {
                out.push(self.escape);
                rest = after_start;
            }
        }
        out.push_str(rest);
        out
    }
}

struct PendingObservationGroup {
    id_suffix: String,
    title: String,
    recorded_at: String,
    kind: RecordKind,
    status: String,
    lines: Vec<String>,
    key_values: Vec<(String, String)>,
}

/// Parses an HL7 v2.x pipe-delimited message (`ADT`, `ORU`, `MDM`, `SIU`) into
/// a normalized [`FhirImportResult`] with per-segment warnings.
pub fn ingest_hl7v2_message(
    source_label: &str,
    portal_id: &str,
    raw_hl7: &str,
) -> Result<FhirImportResult, String> {
    let segments: Vec<&str> = raw_hl7
        .split(['\r', '\n'])
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect();

    let Some(&first) = segments.first() else {
        return Err("HL7 v2 payload is empty.".into());
    };

    let (delims, msh_parts) = Hl7Delimiters::from_msh(first)?;
    // On MSH, split index 8 is MSH-9 (Message Type), index 6 is MSH-7 (Timestamp),
    // index 9 is MSH-10 (Control ID), index 11 is MSH-12 (Version).
    let msg_type_raw = msh_parts.get(8).copied().unwrap_or("");
    let msg_type = delims.component(msg_type_raw, 1);
    let msh_date = msh_parts
        .get(6)
        .copied()
        .map(format_hl7_date)
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "Unknown date".into());
    let control_id = msh_parts.get(9).copied().unwrap_or("0");

    let prefix = sanitize_id_slug(portal_id);
    let confidentiality = if raw_hl7.to_ascii_lowercase().contains("42 cfr part 2") {
        ConfidentialityClass::Part2Sud
    } else {
        ConfidentialityClass::StandardHipaa
    };

    let mut result = FhirImportResult {
        source_label: source_label.trim().to_string(),
        portal_id: portal_id.trim().to_string(),
        patient_name: None,
        patient_birth_date: None,
        records: Vec::new(),
        medications: Vec::new(),
        problems: Vec::new(),
        allergies: Vec::new(),
        total_entries_seen: segments.len(),
        warnings: Vec::new(),
        confidentiality,
    };

    if !msg_type.is_empty() && !matches!(msg_type, "ADT" | "ORU" | "MDM" | "SIU") {
        result.warnings.push(FhirResourceWarning {
            index: 0,
            resource_type: format!("MSH:{msg_type}"),
            resource_id: Some(control_id.to_string()),
            reason: format!(
                "HL7 v2 message type `{msg_type}` is outside primary ADT/ORU/MDM/SIU profiles; extracting recognized segments."
            ),
        });
    }

    let mut active_group: Option<PendingObservationGroup> = None;

    let flush_group = |group: &mut Option<PendingObservationGroup>,
                       records: &mut Vec<ChartRecord>| {
        if let Some(g) = group.take() {
            if !g.lines.is_empty() {
                records.push(ChartRecord {
                    id: format!("{prefix}-hl7-{}", sanitize_id_slug(&g.id_suffix)),
                    kind: g.kind,
                    title: g.title,
                    recorded_at: g.recorded_at,
                    source_label: source_label.to_string(),
                    body: format!("{} (HL7 v2 status: {}).", g.lines.join("; "), g.status),
                    key_values: g.key_values,
                    confidentiality,
                });
            }
        }
    };

    for (seg_idx, seg) in segments.iter().enumerate().skip(1) {
        let parts: Vec<&str> = seg.split(delims.field).collect();
        let seg_id = parts.first().copied().unwrap_or("");

        match seg_id {
            "PID" => {
                // PID-5: Family^Given^Middle
                let raw_name = delims
                    .field(&parts, 5)
                    .split(delims.repetition)
                    .next()
                    .unwrap_or("");
                let family = delims.unescape(delims.component(raw_name, 1));
                let given = delims.unescape(delims.component(raw_name, 2));
                let full = format!("{given} {family}").trim().to_string();
                if !full.is_empty() {
                    result.patient_name = Some(full);
                }
                let dob = format_hl7_date(delims.field(&parts, 7));
                if !dob.is_empty() {
                    result.patient_birth_date = Some(dob);
                }
            }
            "PV1" => {
                flush_group(&mut active_group, &mut result.records);
                let class_code = delims.field(&parts, 2);
                let class_label = match class_code {
                    "I" => "Inpatient",
                    "O" => "Outpatient",
                    "E" => "Emergency",
                    "P" => "Preadmit",
                    other if !other.is_empty() => other,
                    _ => "Encounter",
                };
                let location = delims.unescape(delims.component(delims.field(&parts, 3), 1));
                let visit_num = delims.component(delims.field(&parts, 19), 1);
                let admit = format_hl7_date(delims.field(&parts, 44));
                let discharge = format_hl7_date(delims.field(&parts, 45));
                if class_code.is_empty() && location.is_empty() && visit_num.is_empty() {
                    result.warnings.push(FhirResourceWarning {
                        index: seg_idx,
                        resource_type: "PV1".into(),
                        resource_id: None,
                        reason: "PV1 visit segment has no class, location, or visit number.".into(),
                    });
                } else {
                    let recorded = if admit.is_empty() {
                        msh_date.clone()
                    } else {
                        admit.clone()
                    };
                    let mut kvs = vec![("Visit Class".into(), class_label.to_string())];
                    if !location.is_empty() {
                        kvs.push(("Location".into(), location.clone()));
                    }
                    if !visit_num.is_empty() {
                        kvs.push(("Visit #".into(), visit_num.to_string()));
                    }
                    let discharge_txt = if discharge.is_empty() {
                        String::new()
                    } else {
                        format!(", discharged {discharge}")
                    };
                    result.records.push(ChartRecord {
                        id: format!("{prefix}-hl7-pv1-{seg_idx}"),
                        kind: RecordKind::ProgressNote,
                        title: format!("{class_label} Visit ({msg_type})"),
                        recorded_at: recorded.clone(),
                        source_label: source_label.to_string(),
                        body: format!(
                            "{class_label} encounter at {} (admitted {recorded}{discharge_txt}).",
                            if location.is_empty() {
                                "facility"
                            } else {
                                &location
                            }
                        ),
                        key_values: kvs,
                        confidentiality,
                    });
                }
            }
            "OBR" => {
                flush_group(&mut active_group, &mut result.records);
                let service = delims.field(&parts, 4);
                let title = {
                    let text = delims.unescape(delims.component(service, 2));
                    if !text.is_empty() {
                        text
                    } else {
                        delims.unescape(delims.component(service, 1))
                    }
                };
                let obs_date = format_hl7_date(delims.field(&parts, 7));
                let status = delims.field(&parts, 25);
                active_group = Some(PendingObservationGroup {
                    id_suffix: format!("obr-{control_id}-{seg_idx}"),
                    title: if title.is_empty() {
                        "HL7 v2 Diagnostic Panel".into()
                    } else {
                        title
                    },
                    recorded_at: if obs_date.is_empty() {
                        msh_date.clone()
                    } else {
                        obs_date
                    },
                    kind: RecordKind::LabResult,
                    status: if status.is_empty() {
                        "F".into()
                    } else {
                        status.to_string()
                    },
                    lines: Vec::new(),
                    key_values: Vec::new(),
                });
            }
            "TXA" => {
                flush_group(&mut active_group, &mut result.records);
                let doc_type = delims.field(&parts, 2);
                let doc_label = match doc_type {
                    "DS" => "Discharge Summary",
                    "PN" => "Progress Note",
                    "CN" => "Consultation Note",
                    "OP" => "Operative Note",
                    other if !other.is_empty() => other,
                    _ => "Clinical Document",
                };
                let activity_date = format_hl7_date(delims.field(&parts, 4));
                let doc_id = delims.component(delims.field(&parts, 12), 1);
                let status = delims.field(&parts, 17);
                active_group = Some(PendingObservationGroup {
                    id_suffix: if doc_id.is_empty() {
                        format!("txa-{seg_idx}")
                    } else {
                        format!("txa-{doc_id}")
                    },
                    title: format!("{doc_label} (HL7 MDM)"),
                    recorded_at: if activity_date.is_empty() {
                        msh_date.clone()
                    } else {
                        activity_date
                    },
                    kind: RecordKind::ProgressNote,
                    status: if status.is_empty() {
                        "AU".into()
                    } else {
                        status.to_string()
                    },
                    lines: Vec::new(),
                    key_values: vec![("Document Type".into(), doc_label.to_string())],
                });
            }
            "OBX" => {
                let value_type = delims.field(&parts, 2);
                let obs_id = delims.field(&parts, 3);
                let label = {
                    let t = delims.unescape(delims.component(obs_id, 2));
                    if !t.is_empty() {
                        t
                    } else {
                        delims.unescape(delims.component(obs_id, 1))
                    }
                };
                let val_raw = delims.field(&parts, 5);
                let val = if matches!(value_type, "CE" | "CWE") {
                    let t = delims.unescape(delims.component(val_raw, 2));
                    if !t.is_empty() {
                        t
                    } else {
                        delims.unescape(delims.component(val_raw, 1))
                    }
                } else {
                    delims.unescape(val_raw)
                };
                if label.is_empty() && val.is_empty() {
                    result.warnings.push(FhirResourceWarning {
                        index: seg_idx,
                        resource_type: "OBX".into(),
                        resource_id: None,
                        reason: "OBX segment missing both OBX-3 identifier and OBX-5 value.".into(),
                    });
                    continue;
                }
                let units = delims.unescape(delims.component(delims.field(&parts, 6), 1));
                let ref_range = delims.unescape(delims.field(&parts, 7));
                let abnormal = delims.field(&parts, 8);
                let status = delims.field(&parts, 11);
                let obs_date = format_hl7_date(delims.field(&parts, 14));

                let mut formatted_val = if units.is_empty() {
                    val.clone()
                } else {
                    format!("{val} {units}")
                };
                if !abnormal.is_empty() && abnormal != "N" {
                    formatted_val.push_str(&format!(" [{abnormal}]"));
                }
                let line = if ref_range.is_empty() {
                    if label.is_empty() {
                        formatted_val.clone()
                    } else {
                        format!("{label}: {formatted_val}")
                    }
                } else {
                    format!("{label}: {formatted_val} (ref {ref_range})")
                };

                let group = active_group.get_or_insert_with(|| PendingObservationGroup {
                    id_suffix: format!("obx-{control_id}-{seg_idx}"),
                    title: if label.is_empty() {
                        "HL7 v2 Observation".into()
                    } else {
                        label.clone()
                    },
                    recorded_at: if obs_date.is_empty() {
                        msh_date.clone()
                    } else {
                        obs_date.clone()
                    },
                    kind: if matches!(value_type, "TX" | "FT") {
                        RecordKind::ProgressNote
                    } else {
                        RecordKind::LabResult
                    },
                    status: if status.is_empty() {
                        "F".into()
                    } else {
                        status.to_string()
                    },
                    lines: Vec::new(),
                    key_values: Vec::new(),
                });
                group.lines.push(line);
                if !label.is_empty() && !matches!(value_type, "TX" | "FT") {
                    group.key_values.push((label, formatted_val));
                }
            }
            "NTE" => {
                let comment = delims.unescape(delims.field(&parts, 3));
                if !comment.is_empty() {
                    if let Some(group) = active_group.as_mut() {
                        group.lines.push(format!("Note: {comment}"));
                    } else {
                        result.records.push(ChartRecord {
                            id: format!("{prefix}-hl7-nte-{seg_idx}"),
                            kind: RecordKind::ProgressNote,
                            title: "HL7 v2 Clinical Note".into(),
                            recorded_at: msh_date.clone(),
                            source_label: source_label.to_string(),
                            body: comment,
                            key_values: Vec::new(),
                            confidentiality,
                        });
                    }
                }
            }
            "AL1" => {
                // AL1-2 type, AL1-3 allergen, AL1-4 severity, AL1-5 reaction(s)
                let allergen_field = delims.field(&parts, 3);
                let substance = {
                    let t = delims.unescape(delims.component(allergen_field, 2));
                    if !t.is_empty() {
                        t
                    } else {
                        delims.unescape(delims.component(allergen_field, 1))
                    }
                };
                if substance.is_empty() {
                    result.warnings.push(FhirResourceWarning {
                        index: seg_idx,
                        resource_type: "AL1".into(),
                        resource_id: None,
                        reason: "AL1 segment missing AL1-3 allergen.".into(),
                    });
                    continue;
                }
                let sev_code = delims.component(delims.field(&parts, 4), 1);
                let severity = match sev_code {
                    "SV" => "High (Severe)",
                    "MO" => "Moderate",
                    "MI" => "Low (Mild)",
                    "U" | "" => "Unspecified",
                    other => other,
                };
                let reactions: Vec<String> = delims
                    .field(&parts, 5)
                    .split(delims.repetition)
                    .map(|r| delims.unescape(r.trim()))
                    .filter(|s| !s.is_empty())
                    .collect();
                let reaction = if reactions.is_empty() {
                    "Documented allergy".into()
                } else {
                    reactions.join(", ")
                };
                let id = format!("{prefix}-hl7-al1-{seg_idx}");
                result.allergies.push(AllergyEntry {
                    id: id.clone(),
                    substance: substance.clone(),
                    reaction: reaction.clone(),
                    criticality: severity.to_string(),
                    recorded_at: msh_date.clone(),
                    source_label: source_label.to_string(),
                    confidentiality,
                });
                result.records.push(ChartRecord {
                    id,
                    kind: RecordKind::ProgressNote,
                    title: format!("Allergy / Intolerance: {substance}"),
                    recorded_at: msh_date.clone(),
                    source_label: source_label.to_string(),
                    body: format!("Documented reaction: {reaction} (severity: {severity})."),
                    key_values: vec![
                        ("Allergy".into(), substance),
                        ("Severity".into(), severity.to_string()),
                    ],
                    confidentiality,
                });
            }
            "DG1" => {
                // DG1-3 code^text^system, DG1-4 fallback desc, DG1-6 type (A/W/F)
                let dg_field = delims.field(&parts, 3);
                let code = delims.component(dg_field, 1);
                let label = {
                    let t = delims.unescape(delims.component(dg_field, 2));
                    if !t.is_empty() {
                        t
                    } else {
                        delims.unescape(delims.field(&parts, 4))
                    }
                };
                if label.is_empty() && code.is_empty() {
                    result.warnings.push(FhirResourceWarning {
                        index: seg_idx,
                        resource_type: "DG1".into(),
                        resource_id: None,
                        reason: "DG1 segment missing diagnosis code and description.".into(),
                    });
                    continue;
                }
                let diag_type = match delims.field(&parts, 6) {
                    "A" => "Admitting",
                    "W" => "Working",
                    "F" => "Final",
                    other if !other.is_empty() => other,
                    _ => "Active",
                };
                result.problems.push(Problem {
                    code: if code.is_empty() {
                        "HL7".into()
                    } else {
                        code.to_string()
                    },
                    label: if label.is_empty() {
                        code.to_string()
                    } else {
                        label
                    },
                    status: diag_type.to_string(),
                    confidentiality,
                });
            }
            "RXE" => {
                // RXE-2 give code, RXE-3 amount, RXE-5 units, RXE-15 Rx#
                let give = delims.field(&parts, 2);
                let drug = {
                    let t = delims.unescape(delims.component(give, 2));
                    if !t.is_empty() {
                        t
                    } else {
                        delims.unescape(delims.component(give, 1))
                    }
                };
                if drug.is_empty() {
                    result.warnings.push(FhirResourceWarning {
                        index: seg_idx,
                        resource_type: "RXE".into(),
                        resource_id: None,
                        reason: "RXE pharmacy segment missing RXE-2 give code.".into(),
                    });
                    continue;
                }
                let amt = delims.field(&parts, 3);
                let units = delims.unescape(delims.component(delims.field(&parts, 5), 1));
                let rx_num = delims.field(&parts, 15);
                let dose = format!("{amt} {units}").trim().to_string();
                result.medications.push(Medication {
                    name: drug,
                    dose: if dose.is_empty() {
                        "See pharmacy order".into()
                    } else {
                        dose
                    },
                    schedule: if rx_num.is_empty() {
                        "HL7 RXE Order".into()
                    } else {
                        format!("Rx #{rx_num}")
                    },
                    indication: "Imported from HL7 v2 pharmacy order".into(),
                    confidentiality,
                });
            }
            "SCH" => {
                flush_group(&mut active_group, &mut result.records);
                // SCH-1 Placer ID, SCH-7 Reason, SCH-11 TQ (comp 4 = start, comp 5 = end), SCH-25 Status
                let appt_id = delims.component(delims.field(&parts, 1), 1);
                let reason_f = delims.field(&parts, 7);
                let reason = {
                    let t = delims.unescape(delims.component(reason_f, 2));
                    if !t.is_empty() {
                        t
                    } else {
                        delims.unescape(delims.component(reason_f, 1))
                    }
                };
                let timing = delims
                    .field(&parts, 11)
                    .split(delims.repetition)
                    .next()
                    .unwrap_or("");
                let start = format_hl7_date(delims.component(timing, 4));
                let status_f = delims.field(&parts, 25);
                let status = {
                    let t = delims.unescape(delims.component(status_f, 2));
                    if !t.is_empty() {
                        t
                    } else {
                        delims.unescape(delims.component(status_f, 1))
                    }
                };
                result.records.push(ChartRecord {
                    id: format!(
                        "{prefix}-hl7-sch-{}",
                        if appt_id.is_empty() {
                            seg_idx.to_string()
                        } else {
                            sanitize_id_slug(appt_id)
                        }
                    ),
                    kind: RecordKind::ProgressNote,
                    title: if reason.is_empty() {
                        "Scheduled Appointment (HL7 SIU)".into()
                    } else {
                        format!("Appointment: {reason}")
                    },
                    recorded_at: if start.is_empty() {
                        msh_date.clone()
                    } else {
                        start.clone()
                    },
                    source_label: source_label.to_string(),
                    body: format!(
                        "HL7 v2 scheduling event ({}) for {}.",
                        if status.is_empty() { "Booked" } else { &status },
                        if start.is_empty() {
                            "scheduled visit"
                        } else {
                            &start
                        }
                    ),
                    key_values: vec![(
                        "Status".into(),
                        if status.is_empty() {
                            "Booked".into()
                        } else {
                            status
                        },
                    )],
                    confidentiality,
                });
            }
            _ => {
                // Other structural segments (EVN, ORC, NK1, etc.) are benign metadata.
            }
        }
    }

    flush_group(&mut active_group, &mut result.records);

    if result.records.is_empty()
        && result.medications.is_empty()
        && result.problems.is_empty()
        && result.allergies.is_empty()
        && result.patient_name.is_none()
    {
        return Err("No supported HL7 v2 clinical segments found in message.".into());
    }

    Ok(result)
}

/// Generates a zero-PHI fictional HL7 v2.5.1 `ORU^R01` laboratory, allergy, and
/// diagnosis sample message for interactive demo and unit testing.
#[must_use]
pub fn sandbox_hl7v2_message_for_patient(patient: &PatientChart) -> String {
    let mut name_parts = patient.display_name.split_whitespace();
    let given = name_parts.next().unwrap_or("Sample");
    let family = name_parts.collect::<Vec<_>>().join(" ");
    let family = if family.is_empty() {
        "Patient"
    } else {
        family.as_str()
    };
    let dob = patient.birth_date.replace('-', "");

    format!(
        "MSH|^~\\&|LAB_FEED|REGIONAL_LAB|DABNEY_DEMO|PATIENT_VAULT|20261124093000||ORU^R01|MSG2026112401|P|2.5.1\r\n\
         PID|1||{mrn}^^^DABNEY^MR||{family}^{given}||{dob}|U\r\n\
         PV1|1|O|OUTPATIENT_LAB^^^REGIONAL_LAB||||||||||||||||V-88412|||||||||||||||||||||||||20261124\r\n\
         OBR|1|ORD-401|FIL-901|58410-2^Complete Blood Count & Ferritin^LN|||20261124091500||||||||||||||||||F\r\n\
         OBX|1|NM|2276-4^Ferritin^LN||64|ng/mL|15-150|N|||F|||20261124091500\r\n\
         OBX|2|NM|718-7^Hemoglobin^LN||13.8|g/dL|12.0-16.0|N|||F|||20261124091500\r\n\
         NTE|1|L|Synthetic zero-PHI HL7 v2.5.1 lab feed\\F\\verified within reference interval.\r\n\
         DG1|1|I10|D50.9^Iron deficiency anemia, resolved^I10||20261124|F\r\n\
         AL1|1|DA|1000^Erythromycin^RXNORM|MO|GI cramping\r\n\
         RXE||310429^Ferrous sulfate 325 mg^RXNORM|325||mg||||||||||RX-77210",
        mrn = patient.mrn,
        family = family,
        given = given,
        dob = dob,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::find_patient;

    #[test]
    fn hl7v2_oru_message_parses_exact_msh_pid_pv1_obr_obx_nte_dg1_al1_rxe() {
        let nadia = find_patient("pt-nadia-kulkarni").expect("nadia");
        let msg = sandbox_hl7v2_message_for_patient(&nadia);

        let import =
            ingest_hl7v2_message("Regional Lab HL7 v2", "org-hl7-lab", &msg).expect("hl7 import");
        assert_eq!(import.patient_name.as_deref(), Some("Nadia Kulkarni"));
        assert_eq!(import.patient_birth_date.as_deref(), Some("1988-04-14"));
        assert_eq!(import.problems.len(), 1);
        assert_eq!(import.problems[0].code, "D50.9");
        assert_eq!(import.allergies.len(), 1);
        assert_eq!(import.allergies[0].substance, "Erythromycin");
        assert_eq!(import.medications.len(), 1);
        assert!(import.medications[0].name.contains("Ferrous sulfate"));
        // PV1 + OBR/OBX/NTE + AL1 record = 3 records
        assert_eq!(import.records.len(), 3);
        // Verify HL7 escape sequence `\F\` decoded to `|`
        assert!(import
            .records
            .iter()
            .any(|r| r.body.contains("lab feed|verified")));
        assert!(import.warnings.is_empty());
    }

    #[test]
    fn hl7v2_supports_mdm_siu_and_custom_field_separators() {
        // Custom field separator `#` and component separator `!`
        let siu = "MSH#!~\\&#SCHED#CLINIC#APP#VAULT#20261125##SIU!S12#SIU-99#P#2.5.1\n\
                   PID#1##MRN-1##Doe!Jane##19900105#F\n\
                   SCH#APT-42######FOLLOWUP!Cardiology Follow-Up####1!!30!202612101000!202612101030##############Booked";
        let import = ingest_hl7v2_message("Clinic SIU", "clinic-siu", siu).expect("custom sep ok");
        assert_eq!(import.patient_name.as_deref(), Some("Jane Doe"));
        assert_eq!(import.patient_birth_date.as_deref(), Some("1990-01-05"));
        assert_eq!(import.records.len(), 1);
        assert!(import.records[0].title.contains("Cardiology Follow-Up"));
        assert_eq!(import.records[0].recorded_at, "2026-12-10");

        let mdm = "MSH|^~\\&|EHR|HOSP|APP|VAULT|20261125||MDM^T02|MDM-7|P|2.5.1\r\n\
                   PID|1||MRN-2||Smith^Alex||19850520|M\r\n\
                   TXA|1|DS||20261124||||||||DOC-55|||||AU\r\n\
                   OBX|1|TX|18842-5^Discharge Summary^LN||Discharged home in stable condition\\.br\\Follow up in 2 weeks.||||||F";
        let mdm_import = ingest_hl7v2_message("Hospital MDM", "hosp-mdm", mdm).expect("mdm ok");
        assert_eq!(mdm_import.records.len(), 1);
        assert!(mdm_import.records[0].title.contains("Discharge Summary"));
        assert!(mdm_import.records[0].body.contains('\n'));
    }
}
