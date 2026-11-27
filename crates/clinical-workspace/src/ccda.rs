//! Pure-Rust HL7 C-CDA R2.1 XML clinical document parser (zero external XML
//! dependencies; runs identically in SSR, browser WebAssembly, and Tauri).
//!
//! Supports CCD (`34133-9`), Discharge Summary (`18842-5`), Progress Note
//! (`11506-3`), Consultation Note (`11488-4`), and Referral Note (`57133-1`)
//! documents, extracting `recordTarget/patientRole` demographics and LOINC-coded
//! sections (Problems `11450-4`, Medications `10160-0`, Allergies `48765-2`,
//! Results `30954-2`, Vital Signs `8716-3`, Immunizations `11369-6`, Procedures
//! `47519-4`, Encounters `46240-8`, Plan of Treatment `18776-5`, Social History
//! `29762-2`, and Notes `11506-3`/`18842-5`/`11488-4`/`34109-9` + template
//! `2.16.840.1.113883.10.20.22.2.65`).

use crate::ehr_portal::{sanitize_id_slug, AllergyEntry, FhirImportResult, FhirResourceWarning};
use crate::{ChartRecord, ConfidentialityClass, Medication, PatientChart, Problem, RecordKind};

const MAX_XML_DEPTH: usize = 64;
const MAX_XML_NODES: usize = 25_000;
const MAX_XML_BYTES: usize = 2 * 1024 * 1024;

#[derive(Debug, Clone)]
struct XmlNode {
    name: String,
    attrs: Vec<(String, String)>,
    children: Vec<XmlNode>,
    text: String,
}

impl XmlNode {
    fn attr(&self, key: &str) -> Option<&str> {
        self.attrs
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }

    fn child(&self, name: &str) -> Option<&XmlNode> {
        self.children.iter().find(|c| c.name == name)
    }

    fn children_named<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a XmlNode> {
        self.children.iter().filter(move |c| c.name == name)
    }

    fn find_descendant(&self, name: &str) -> Option<&XmlNode> {
        for child in &self.children {
            if child.name == name {
                return Some(child);
            }
            if let Some(found) = child.find_descendant(name) {
                return Some(found);
            }
        }
        None
    }

    fn collect_descendants<'a>(&'a self, name: &str, out: &mut Vec<&'a XmlNode>) {
        for child in &self.children {
            if child.name == name {
                out.push(child);
            }
            child.collect_descendants(name, out);
        }
    }

    fn text_content(&self) -> String {
        let mut out = String::new();
        self.push_text_content(&mut out);
        collapse_whitespace(&out)
    }

    fn push_text_content(&self, out: &mut String) {
        if !self.text.is_empty() {
            if !out.is_empty() && !out.ends_with(' ') {
                out.push(' ');
            }
            out.push_str(&self.text);
        }
        for child in &self.children {
            child.push_text_content(out);
        }
    }
}

/// Parses an HL7 C-CDA R2.1 XML document into a normalized [`FhirImportResult`]
/// with per-section warnings so malformed or empty sections never block valid
/// sibling sections.
pub fn ingest_ccda_xml(
    source_label: &str,
    portal_id: &str,
    raw_xml: &str,
) -> Result<FhirImportResult, String> {
    if raw_xml.len() > MAX_XML_BYTES {
        return Err("C-CDA XML document exceeds the 2 MiB local parsing safety cap.".into());
    }
    let root = parse_xml_document(raw_xml)?;
    if root.name != "ClinicalDocument" {
        return Err(format!(
            "Expected C-CDA root element `<ClinicalDocument>`, found `<{}>`.",
            root.name
        ));
    }

    let doc_confidentiality = detect_ccda_confidentiality(&root, raw_xml);
    let prefix = sanitize_id_slug(portal_id);

    let mut result = FhirImportResult {
        source_label: source_label.trim().to_string(),
        portal_id: portal_id.trim().to_string(),
        patient_name: None,
        patient_birth_date: None,
        records: Vec::new(),
        medications: Vec::new(),
        problems: Vec::new(),
        allergies: Vec::new(),
        total_entries_seen: 0,
        warnings: Vec::new(),
        confidentiality: doc_confidentiality,
    };

    // 1. Header patient demographics: recordTarget/patientRole
    if let Some(patient_role) = root
        .child("recordTarget")
        .and_then(|rt| rt.child("patientRole"))
    {
        if let Some(patient) = patient_role.child("patient") {
            if let Some(name_el) = patient.child("name") {
                let given: Vec<String> = name_el
                    .children_named("given")
                    .map(XmlNode::text_content)
                    .filter(|s| !s.is_empty())
                    .collect();
                let family = name_el
                    .child("family")
                    .map(XmlNode::text_content)
                    .unwrap_or_default();
                let full = if !given.is_empty() || !family.is_empty() {
                    format!("{} {}", given.join(" "), family).trim().to_string()
                } else {
                    name_el.text_content()
                };
                if !full.is_empty() {
                    result.patient_name = Some(full);
                }
            }
            if let Some(btime) = patient
                .child("birthTime")
                .and_then(|b| b.attr("value"))
                .map(format_hl7_date)
                .filter(|s| !s.is_empty())
            {
                result.patient_birth_date = Some(btime);
            }
        }
    }

    let doc_date = root
        .child("effectiveTime")
        .and_then(|e| e.attr("value"))
        .map(format_hl7_date)
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "Unknown date".into());
    let doc_title = root
        .child("title")
        .map(XmlNode::text_content)
        .filter(|s| !s.is_empty())
        .or_else(|| {
            root.child("code")
                .and_then(|c| c.attr("displayName"))
                .map(str::to_string)
        })
        .unwrap_or_else(|| "C-CDA Clinical Document".into());

    // 2. Structured body sections
    let mut sections = Vec::new();
    if let Some(body) = root
        .child("component")
        .and_then(|c| c.child("structuredBody"))
    {
        body.collect_descendants("section", &mut sections);
    }

    for (sec_idx, section) in sections.iter().enumerate() {
        result.total_entries_seen += 1;
        let loinc = section
            .child("code")
            .and_then(|c| c.attr("code"))
            .unwrap_or("");
        let section_title = section
            .child("title")
            .map(XmlNode::text_content)
            .filter(|s| !s.is_empty())
            .or_else(|| {
                section
                    .child("code")
                    .and_then(|c| c.attr("displayName"))
                    .map(str::to_string)
            })
            .unwrap_or_else(|| format!("C-CDA Section {}", sec_idx + 1));

        let templates: Vec<&str> = section
            .children_named("templateId")
            .filter_map(|t| t.attr("root"))
            .collect();

        if section.attr("nullFlavor").is_some() {
            result.warnings.push(FhirResourceWarning {
                index: sec_idx,
                resource_type: format!("CCDA:{loinc}"),
                resource_id: Some(section_title),
                reason: "Section marked with nullFlavor; skipped structured extraction.".into(),
            });
            continue;
        }

        match classify_section(loinc, &templates) {
            CcdaSectionKind::Problems => {
                let added = extract_problems(section, doc_confidentiality, &mut result.problems);
                if added == 0 {
                    fallback_narrative_or_warn(
                        section,
                        sec_idx,
                        &prefix,
                        loinc,
                        &section_title,
                        &doc_date,
                        source_label,
                        doc_confidentiality,
                        &mut result,
                    );
                }
            }
            CcdaSectionKind::Medications => {
                let added =
                    extract_medications(section, doc_confidentiality, &mut result.medications);
                if added == 0 {
                    fallback_narrative_or_warn(
                        section,
                        sec_idx,
                        &prefix,
                        loinc,
                        &section_title,
                        &doc_date,
                        source_label,
                        doc_confidentiality,
                        &mut result,
                    );
                }
            }
            CcdaSectionKind::Allergies => {
                let added = extract_allergies(
                    section,
                    sec_idx,
                    &prefix,
                    &doc_date,
                    source_label,
                    doc_confidentiality,
                    &mut result.allergies,
                    &mut result.records,
                );
                if added == 0 {
                    fallback_narrative_or_warn(
                        section,
                        sec_idx,
                        &prefix,
                        loinc,
                        &section_title,
                        &doc_date,
                        source_label,
                        doc_confidentiality,
                        &mut result,
                    );
                }
            }
            CcdaSectionKind::Results => {
                let added = extract_observations_section(
                    section,
                    sec_idx,
                    &prefix,
                    "lab",
                    &section_title,
                    &doc_date,
                    source_label,
                    doc_confidentiality,
                    &mut result.records,
                );
                if added == 0 {
                    fallback_narrative_or_warn(
                        section,
                        sec_idx,
                        &prefix,
                        loinc,
                        &section_title,
                        &doc_date,
                        source_label,
                        doc_confidentiality,
                        &mut result,
                    );
                }
            }
            CcdaSectionKind::Vitals => {
                let added = extract_observations_section(
                    section,
                    sec_idx,
                    &prefix,
                    "vital",
                    &section_title,
                    &doc_date,
                    source_label,
                    doc_confidentiality,
                    &mut result.records,
                );
                if added == 0 {
                    fallback_narrative_or_warn(
                        section,
                        sec_idx,
                        &prefix,
                        loinc,
                        &section_title,
                        &doc_date,
                        source_label,
                        doc_confidentiality,
                        &mut result,
                    );
                }
            }
            CcdaSectionKind::Immunizations
            | CcdaSectionKind::Procedures
            | CcdaSectionKind::Encounters
            | CcdaSectionKind::PlanOfTreatment
            | CcdaSectionKind::SocialHistory
            | CcdaSectionKind::Notes => {
                let summary = extract_section_summary(section);
                if summary.is_empty() {
                    result.warnings.push(FhirResourceWarning {
                        index: sec_idx,
                        resource_type: format!("CCDA:{loinc}"),
                        resource_id: Some(section_title),
                        reason: "Section contains neither structured entries nor narrative text."
                            .into(),
                    });
                } else {
                    result.records.push(ChartRecord {
                        id: format!(
                            "{prefix}-ccda-{}-{sec_idx}",
                            sanitize_id_slug(if loinc.is_empty() { "sec" } else { loinc })
                        ),
                        kind: RecordKind::ProgressNote,
                        title: section_title,
                        recorded_at: section_effective_date(section)
                            .unwrap_or_else(|| doc_date.clone()),
                        source_label: source_label.to_string(),
                        body: summary,
                        key_values: if loinc.is_empty() {
                            Vec::new()
                        } else {
                            vec![("LOINC".into(), loinc.to_string())]
                        },
                        confidentiality: doc_confidentiality,
                    });
                }
            }
            CcdaSectionKind::Other => {
                let narrative = section
                    .child("text")
                    .map(XmlNode::text_content)
                    .unwrap_or_default();
                if !narrative.is_empty() {
                    result.records.push(ChartRecord {
                        id: format!("{prefix}-ccda-other-{sec_idx}"),
                        kind: RecordKind::ProgressNote,
                        title: format!("{doc_title} — {section_title}"),
                        recorded_at: doc_date.clone(),
                        source_label: source_label.to_string(),
                        body: narrative,
                        key_values: Vec::new(),
                        confidentiality: doc_confidentiality,
                    });
                } else {
                    result.warnings.push(FhirResourceWarning {
                        index: sec_idx,
                        resource_type: format!(
                            "CCDA:{}",
                            if loinc.is_empty() { "unknown" } else { loinc }
                        ),
                        resource_id: Some(section_title),
                        reason: "Unrecognized C-CDA section without narrative text.".into(),
                    });
                }
            }
        }
    }

    // 3. Support nonXMLBody fallback if present
    if sections.is_empty() {
        if let Some(non_xml) = root
            .child("component")
            .and_then(|c| c.child("nonXMLBody"))
            .and_then(|b| b.child("text"))
        {
            let text = non_xml.text_content();
            if !text.is_empty() {
                result.total_entries_seen += 1;
                result.records.push(ChartRecord {
                    id: format!("{prefix}-ccda-doc-0"),
                    kind: RecordKind::ProgressNote,
                    title: doc_title,
                    recorded_at: doc_date,
                    source_label: source_label.to_string(),
                    body: text,
                    key_values: Vec::new(),
                    confidentiality: doc_confidentiality,
                });
            }
        }
    }

    if result.records.is_empty()
        && result.medications.is_empty()
        && result.problems.is_empty()
        && result.allergies.is_empty()
        && result.patient_name.is_none()
    {
        return Err("No supported C-CDA R2.1 clinical content found in XML document.".into());
    }

    Ok(result)
}

/// Generates a standards-compliant, zero-PHI fictional C-CDA R2.1 Continuity of
/// Care Document (CCD) XML sample for interactive demo and unit testing.
#[must_use]
pub fn sandbox_ccda_xml_for_patient(patient: &PatientChart) -> String {
    let mut name_parts = patient.display_name.split_whitespace();
    let given = name_parts.next().unwrap_or("Sample");
    let family = name_parts.collect::<Vec<_>>().join(" ");
    let family = if family.is_empty() {
        "Patient"
    } else {
        family.as_str()
    };
    let dob_compact = patient.birth_date.replace('-', "");

    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<ClinicalDocument xmlns="urn:hl7-org:v3" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance">
  <templateId root="2.16.840.1.113883.10.20.22.1.2" extension="2015-08-01"/>
  <code code="34133-9" codeSystem="2.16.840.1.113883.6.1" displayName="Summarization of Episode Note"/>
  <title>Continuity of Care Document (Zero-PHI C-CDA R2.1 Sample)</title>
  <effectiveTime value="20261124"/>
  <confidentialityCode code="N" codeSystem="2.16.840.1.113883.5.25"/>
  <recordTarget>
    <patientRole>
      <id root="2.16.840.1.113883.19.5" extension="{mrn}"/>
      <patient>
        <name><given>{given}</given><family>{family}</family></name>
        <birthTime value="{dob}"/>
      </patient>
    </patientRole>
  </recordTarget>
  <component>
    <structuredBody>
      <component>
        <section>
          <templateId root="2.16.840.1.113883.10.20.22.2.5.1" extension="2015-08-01"/>
          <code code="11450-4" codeSystem="2.16.840.1.113883.6.1" displayName="Problem List"/>
          <title>Problems</title>
          <entry>
            <act classCode="ACT" moodCode="EVN">
              <entryRelationship typeCode="SUBJ">
                <observation classCode="OBS" moodCode="EVN">
                  <statusCode code="completed"/>
                  <value xsi:type="CD" code="E55.9" displayName="Vitamin D deficiency, unspecified"/>
                </observation>
              </entryRelationship>
            </act>
          </entry>
        </section>
      </component>
      <component>
        <section>
          <templateId root="2.16.840.1.113883.10.20.22.2.1.1" extension="2014-06-09"/>
          <code code="10160-0" codeSystem="2.16.840.1.113883.6.1" displayName="History of Medication Use"/>
          <title>Medications</title>
          <entry>
            <substanceAdministration classCode="SBADM" moodCode="EVN">
              <statusCode code="active"/>
              <doseQuantity value="2000" unit="IU"/>
              <consumable>
                <manufacturedProduct>
                  <manufacturedMaterial>
                    <code code="316965" displayName="Cholecalciferol (Vitamin D3)"/>
                  </manufacturedMaterial>
                </manufacturedProduct>
              </consumable>
            </substanceAdministration>
          </entry>
        </section>
      </component>
      <component>
        <section>
          <templateId root="2.16.840.1.113883.10.20.22.2.6.1" extension="2015-08-01"/>
          <code code="48765-2" codeSystem="2.16.840.1.113883.6.1" displayName="Allergies and Adverse Reactions"/>
          <title>Allergies</title>
          <entry>
            <act classCode="ACT" moodCode="EVN">
              <entryRelationship typeCode="SUBJ">
                <observation classCode="OBS" moodCode="EVN">
                  <effectiveTime value="20250410"/>
                  <participant typeCode="CSM">
                    <participantRole classCode="MANU">
                      <playingEntity classCode="MMAT">
                        <code code="7980" displayName="Amoxicillin"/>
                      </playingEntity>
                    </participantRole>
                  </participant>
                  <entryRelationship typeCode="MFST">
                    <observation classCode="OBS" moodCode="EVN">
                      <value xsi:type="CD" displayName="Mild urticaria"/>
                    </observation>
                  </entryRelationship>
                </observation>
              </entryRelationship>
            </act>
          </entry>
        </section>
      </component>
      <component>
        <section>
          <templateId root="2.16.840.1.113883.10.20.22.2.3.1" extension="2015-08-01"/>
          <code code="30954-2" codeSystem="2.16.840.1.113883.6.1" displayName="Relevant diagnostic tests and/or laboratory data"/>
          <title>Results</title>
          <entry>
            <organizer classCode="BATTERY" moodCode="EVN">
              <code code="24323-8" displayName="Comprehensive Metabolic &amp; Vitamin D Panel"/>
              <component>
                <observation classCode="OBS" moodCode="EVN">
                  <code code="1989-3" displayName="25-Hydroxyvitamin D"/>
                  <effectiveTime value="20261116"/>
                  <value xsi:type="PQ" value="38.5" unit="ng/mL"/>
                </observation>
              </component>
            </organizer>
          </entry>
        </section>
      </component>
      <component>
        <section>
          <templateId root="2.16.840.1.113883.10.20.22.2.4.1" extension="2015-08-01"/>
          <code code="8716-3" codeSystem="2.16.840.1.113883.6.1" displayName="Vital Signs"/>
          <title>Vital Signs</title>
          <entry>
            <organizer classCode="CLUSTER" moodCode="EVN">
              <component>
                <observation classCode="OBS" moodCode="EVN">
                  <code code="8480-6" displayName="Systolic Blood Pressure"/>
                  <effectiveTime value="20261116"/>
                  <value xsi:type="PQ" value="122" unit="mmHg"/>
                </observation>
              </component>
              <component>
                <observation classCode="OBS" moodCode="EVN">
                  <code code="8462-4" displayName="Diastolic Blood Pressure"/>
                  <effectiveTime value="20261116"/>
                  <value xsi:type="PQ" value="76" unit="mmHg"/>
                </observation>
              </component>
            </organizer>
          </entry>
        </section>
      </component>
      <component>
        <section>
          <templateId root="2.16.840.1.113883.10.20.22.2.10" extension="2014-06-09"/>
          <code code="18776-5" codeSystem="2.16.840.1.113883.6.1" displayName="Plan of Care"/>
          <title>Plan of Treatment</title>
          <text>Continue Vitamin D3 2000 IU daily and recheck 25-OH Vitamin D in 6 months. Bring portable summary to next primary care visit.</text>
        </section>
      </component>
    </structuredBody>
  </component>
</ClinicalDocument>"#,
        mrn = crate::ehr_portal::html_escape(&patient.mrn),
        given = crate::ehr_portal::html_escape(given),
        family = crate::ehr_portal::html_escape(family),
        dob = crate::ehr_portal::html_escape(&dob_compact),
    )
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CcdaSectionKind {
    Problems,
    Medications,
    Allergies,
    Results,
    Vitals,
    Immunizations,
    Procedures,
    Encounters,
    PlanOfTreatment,
    SocialHistory,
    Notes,
    Other,
}

fn classify_section(loinc: &str, templates: &[&str]) -> CcdaSectionKind {
    match loinc {
        "11450-4" => return CcdaSectionKind::Problems,
        "10160-0" => return CcdaSectionKind::Medications,
        "48765-2" => return CcdaSectionKind::Allergies,
        "30954-2" => return CcdaSectionKind::Results,
        "8716-3" => return CcdaSectionKind::Vitals,
        "11369-6" => return CcdaSectionKind::Immunizations,
        "47519-4" => return CcdaSectionKind::Procedures,
        "46240-8" => return CcdaSectionKind::Encounters,
        "18776-5" => return CcdaSectionKind::PlanOfTreatment,
        "29762-2" => return CcdaSectionKind::SocialHistory,
        "11506-3" | "18842-5" | "11488-4" | "34109-9" | "57133-1" => return CcdaSectionKind::Notes,
        _ => {}
    }
    for oid in templates {
        match *oid {
            "2.16.840.1.113883.10.20.22.2.5.1" | "2.16.840.1.113883.10.20.22.2.5" => {
                return CcdaSectionKind::Problems
            }
            "2.16.840.1.113883.10.20.22.2.1.1" | "2.16.840.1.113883.10.20.22.2.1" => {
                return CcdaSectionKind::Medications
            }
            "2.16.840.1.113883.10.20.22.2.6.1" | "2.16.840.1.113883.10.20.22.2.6" => {
                return CcdaSectionKind::Allergies
            }
            "2.16.840.1.113883.10.20.22.2.3.1" | "2.16.840.1.113883.10.20.22.2.3" => {
                return CcdaSectionKind::Results
            }
            "2.16.840.1.113883.10.20.22.2.4.1" | "2.16.840.1.113883.10.20.22.2.4" => {
                return CcdaSectionKind::Vitals
            }
            "2.16.840.1.113883.10.20.22.2.2.1" | "2.16.840.1.113883.10.20.22.2.2" => {
                return CcdaSectionKind::Immunizations
            }
            "2.16.840.1.113883.10.20.22.2.7.1" | "2.16.840.1.113883.10.20.22.2.7" => {
                return CcdaSectionKind::Procedures
            }
            "2.16.840.1.113883.10.20.22.2.22.1" | "2.16.840.1.113883.10.20.22.2.22" => {
                return CcdaSectionKind::Encounters
            }
            "2.16.840.1.113883.10.20.22.2.10" => return CcdaSectionKind::PlanOfTreatment,
            "2.16.840.1.113883.10.20.22.2.17" => return CcdaSectionKind::SocialHistory,
            "2.16.840.1.113883.10.20.22.2.65" => return CcdaSectionKind::Notes,
            _ => {}
        }
    }
    CcdaSectionKind::Other
}

fn detect_ccda_confidentiality(root: &XmlNode, raw_xml: &str) -> ConfidentialityClass {
    let conf_code = root
        .child("confidentialityCode")
        .and_then(|c| c.attr("code"))
        .unwrap_or("");
    let lower = raw_xml.to_ascii_lowercase();
    // Fail-closed: treat restricted (`R`) and very-restricted (`V`) C-CDA
    // documents, explicit 42 CFR Part 2 notices, and HL7 `ETHUD` / SUD markers
    // as Part 2 protected so restricted records never silently leak into
    // general exports.
    if matches!(conf_code, "R" | "V")
        || lower.contains("42 cfr part 2")
        || lower.contains("42 cfr 2.")
        || lower.contains("ethud")
        || lower.contains("substance use disorder")
        || lower.contains("alcohol use disorder")
        || lower.contains("opioid use disorder")
    {
        ConfidentialityClass::Part2Sud
    } else {
        ConfidentialityClass::StandardHipaa
    }
}

fn extract_problems(
    section: &XmlNode,
    confidentiality: ConfidentialityClass,
    out: &mut Vec<Problem>,
) -> usize {
    let mut observations = Vec::new();
    section.collect_descendants("observation", &mut observations);
    let mut added = 0;
    for obs in observations {
        if obs.attr("nullFlavor").is_some() {
            continue;
        }
        let Some(val) = obs.child("value") else {
            continue;
        };
        if val.attr("nullFlavor").is_some() {
            continue;
        }
        let label = val
            .attr("displayName")
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .or_else(|| {
                val.child("originalText")
                    .map(XmlNode::text_content)
                    .filter(|s| !s.is_empty())
            })
            .or_else(|| val.attr("code").map(str::to_string));
        let Some(label) = label else {
            continue;
        };
        let code = val.attr("code").unwrap_or("CCDA").to_string();
        let status = obs
            .child("statusCode")
            .and_then(|s| s.attr("code"))
            .unwrap_or("active")
            .to_string();
        out.push(Problem {
            code,
            label,
            status,
            confidentiality,
        });
        added += 1;
    }
    added
}

fn extract_medications(
    section: &XmlNode,
    confidentiality: ConfidentialityClass,
    out: &mut Vec<Medication>,
) -> usize {
    let mut admins = Vec::new();
    section.collect_descendants("substanceAdministration", &mut admins);
    let mut added = 0;
    for admin in admins {
        if admin.attr("nullFlavor").is_some() {
            continue;
        }
        let Some(mat) = admin.find_descendant("manufacturedMaterial") else {
            continue;
        };
        let Some(code_el) = mat.child("code") else {
            continue;
        };
        let name = code_el
            .attr("displayName")
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .or_else(|| {
                code_el
                    .child("originalText")
                    .map(XmlNode::text_content)
                    .filter(|s| !s.is_empty())
            })
            .or_else(|| code_el.attr("code").map(str::to_string));
        let Some(name) = name else {
            continue;
        };

        let dose = admin
            .child("doseQuantity")
            .and_then(|dq| {
                let val = dq.attr("value")?;
                match dq.attr("unit") {
                    Some(u) if !u.is_empty() => Some(format!("{val} {u}")),
                    _ => Some(val.to_string()),
                }
            })
            .unwrap_or_else(|| "See C-CDA directions".into());
        let status = admin
            .child("statusCode")
            .and_then(|s| s.attr("code"))
            .unwrap_or("active");
        let route = admin
            .child("routeCode")
            .and_then(|r| r.attr("displayName"))
            .unwrap_or("");
        let schedule = if route.is_empty() {
            format!("Status: {status}")
        } else {
            format!("{route} · Status: {status}")
        };
        out.push(Medication {
            name,
            dose,
            schedule,
            indication: "Imported from C-CDA R2.1 Medications section".into(),
            confidentiality,
        });
        added += 1;
    }
    added
}

#[allow(clippy::too_many_arguments)]
fn extract_allergies(
    section: &XmlNode,
    sec_idx: usize,
    prefix: &str,
    doc_date: &str,
    source_label: &str,
    confidentiality: ConfidentialityClass,
    allergies_out: &mut Vec<AllergyEntry>,
    records_out: &mut Vec<ChartRecord>,
) -> usize {
    let mut added = 0;

    // Walk allergy entries (each `<entry>` usually wraps one allergy observation).
    for (idx, entry) in section.children_named("entry").enumerate() {
        let Some(playing) = entry.find_descendant("playingEntity") else {
            continue;
        };
        let Some(code_el) = playing.child("code") else {
            continue;
        };
        let substance = code_el
            .attr("displayName")
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .or_else(|| {
                playing
                    .child("name")
                    .map(XmlNode::text_content)
                    .filter(|s| !s.is_empty())
            })
            .or_else(|| code_el.attr("code").map(str::to_string));
        let Some(substance) = substance else {
            continue;
        };

        // Find manifestation reaction inside entryRelationship[@typeCode='MFST']
        let mut rels = Vec::new();
        entry.collect_descendants("entryRelationship", &mut rels);
        let reaction = rels
            .iter()
            .find(|r| r.attr("typeCode") == Some("MFST"))
            .and_then(|r| r.find_descendant("value"))
            .and_then(|v| v.attr("displayName"))
            .map(str::to_string)
            .unwrap_or_else(|| "Documented allergy/intolerance".into());
        let recorded_at = section_effective_date(entry).unwrap_or_else(|| doc_date.to_string());
        let id = format!("{prefix}-ccda-alg-{sec_idx}-{idx}");

        allergies_out.push(AllergyEntry {
            id: id.clone(),
            substance: substance.clone(),
            reaction: reaction.clone(),
            criticality: "Documented".into(),
            recorded_at: recorded_at.clone(),
            source_label: source_label.to_string(),
            confidentiality,
        });
        records_out.push(ChartRecord {
            id,
            kind: RecordKind::ProgressNote,
            title: format!("Allergy / Intolerance: {substance}"),
            recorded_at,
            source_label: source_label.to_string(),
            body: format!("Documented reaction: {reaction} (C-CDA R2.1)."),
            key_values: vec![("Allergy".into(), substance)],
            confidentiality,
        });
        added += 1;
    }

    added
}

#[allow(clippy::too_many_arguments)]
fn extract_observations_section(
    section: &XmlNode,
    sec_idx: usize,
    prefix: &str,
    tag: &str,
    section_title: &str,
    doc_date: &str,
    source_label: &str,
    confidentiality: ConfidentialityClass,
    out: &mut Vec<ChartRecord>,
) -> usize {
    let mut observations = Vec::new();
    section.collect_descendants("observation", &mut observations);

    let mut kvs = Vec::new();
    let mut parts = Vec::new();
    let mut best_date: Option<String> = None;

    for obs in observations {
        if obs.attr("nullFlavor").is_some() {
            continue;
        }
        let Some(code_el) = obs.child("code") else {
            continue;
        };
        let label = code_el
            .attr("displayName")
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .or_else(|| code_el.attr("code"))
            .unwrap_or("");
        if label.is_empty() {
            continue;
        }
        let Some(val_el) = obs.child("value") else {
            continue;
        };
        if val_el.attr("nullFlavor").is_some() {
            continue;
        }
        let val_str = if let Some(v) = val_el.attr("value") {
            match val_el.attr("unit") {
                Some(u) if !u.is_empty() => format!("{v} {u}"),
                _ => v.to_string(),
            }
        } else if let Some(d) = val_el.attr("displayName") {
            d.to_string()
        } else {
            val_el.text_content()
        };
        if val_str.is_empty() {
            continue;
        }
        if best_date.is_none() {
            best_date = section_effective_date(obs);
        }
        kvs.push((label.to_string(), val_str.clone()));
        parts.push(format!("{label}: {val_str}"));
    }

    if parts.is_empty() {
        return 0;
    }

    out.push(ChartRecord {
        id: format!("{prefix}-ccda-{tag}-{sec_idx}"),
        kind: RecordKind::LabResult,
        title: section_title.to_string(),
        recorded_at: best_date.unwrap_or_else(|| doc_date.to_string()),
        source_label: source_label.to_string(),
        body: format!("{} (C-CDA R2.1).", parts.join("; ")),
        key_values: kvs,
        confidentiality,
    });
    1
}

fn extract_section_summary(section: &XmlNode) -> String {
    if let Some(text) = section.child("text") {
        let t = text.text_content();
        if !t.is_empty() {
            return t;
        }
    }
    let mut codes = Vec::new();
    section.collect_descendants("code", &mut codes);
    let displays: Vec<&str> = codes
        .into_iter()
        .skip(1) // skip section header code
        .filter_map(|c| c.attr("displayName"))
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect();
    displays.join("; ")
}

#[allow(clippy::too_many_arguments)]
fn fallback_narrative_or_warn(
    section: &XmlNode,
    sec_idx: usize,
    prefix: &str,
    loinc: &str,
    section_title: &str,
    doc_date: &str,
    source_label: &str,
    confidentiality: ConfidentialityClass,
    result: &mut FhirImportResult,
) {
    let narrative = section
        .child("text")
        .map(XmlNode::text_content)
        .unwrap_or_default();
    if !narrative.is_empty() {
        result.records.push(ChartRecord {
            id: format!("{prefix}-ccda-narr-{sec_idx}"),
            kind: RecordKind::ProgressNote,
            title: section_title.to_string(),
            recorded_at: section_effective_date(section).unwrap_or_else(|| doc_date.to_string()),
            source_label: source_label.to_string(),
            body: narrative,
            key_values: if loinc.is_empty() {
                Vec::new()
            } else {
                vec![("LOINC".into(), loinc.to_string())]
            },
            confidentiality,
        });
    } else {
        result.warnings.push(FhirResourceWarning {
            index: sec_idx,
            resource_type: format!("CCDA:{loinc}"),
            resource_id: Some(section_title.to_string()),
            reason: "Section had no structured entries or narrative text.".into(),
        });
    }
}

fn section_effective_date(node: &XmlNode) -> Option<String> {
    let eff = node.find_descendant("effectiveTime")?;
    eff.attr("value")
        .or_else(|| eff.child("low").and_then(|l| l.attr("value")))
        .map(format_hl7_date)
        .filter(|s| !s.is_empty())
}

pub(crate) fn format_hl7_date(raw: &str) -> String {
    let digits: String = raw.chars().take_while(char::is_ascii_digit).collect();
    if digits.len() >= 8 {
        format!("{}-{}-{}", &digits[0..4], &digits[4..6], &digits[6..8])
    } else if digits.len() >= 4 {
        digits[0..4].to_string()
    } else {
        raw.trim().to_string()
    }
}

fn collapse_whitespace(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn strip_xml_prefix(name: &str) -> &str {
    name.rsplit_once(':')
        .map(|(_, local)| local)
        .unwrap_or(name)
}

fn decode_xml_entities(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut rest = input;
    while let Some(amp) = rest.find('&') {
        out.push_str(&rest[..amp]);
        rest = &rest[amp + 1..];
        if let Some(semi) = rest.find(';') {
            let entity = &rest[..semi];
            match entity {
                "amp" => out.push('&'),
                "lt" => out.push('<'),
                "gt" => out.push('>'),
                "quot" => out.push('"'),
                "apos" => out.push('\''),
                _ if entity.starts_with("#x") || entity.starts_with("#X") => {
                    if let Ok(code) = u32::from_str_radix(&entity[2..], 16) {
                        if let Some(ch) = char::from_u32(code) {
                            out.push(ch);
                        }
                    }
                }
                _ if entity.starts_with('#') => {
                    if let Ok(code) = entity[1..].parse::<u32>() {
                        if let Some(ch) = char::from_u32(code) {
                            out.push(ch);
                        }
                    }
                }
                _ => {
                    out.push('&');
                    out.push_str(entity);
                    out.push(';');
                }
            }
            rest = &rest[semi + 1..];
        } else {
            out.push('&');
        }
    }
    out.push_str(rest);
    out
}

/// Bounded pure-Rust XML DOM parser for C-CDA R2.1 documents.
fn parse_xml_document(input: &str) -> Result<XmlNode, String> {
    let bytes = input.as_bytes();
    let mut pos = 0usize;
    let mut stack: Vec<XmlNode> = Vec::new();
    let mut root: Option<XmlNode> = None;
    let mut node_count = 0usize;

    // Skip optional UTF-8 BOM
    if input.starts_with('\u{feff}') {
        pos = 3;
    }

    while pos < bytes.len() {
        if bytes[pos] == b'<' {
            if input[pos..].starts_with("<!--") {
                let Some(end) = input[pos + 4..].find("-->") else {
                    return Err("Unterminated XML comment in C-CDA document.".into());
                };
                pos += 4 + end + 3;
            } else if input[pos..].starts_with("<![CDATA[") {
                let Some(end) = input[pos + 9..].find("]]>") else {
                    return Err("Unterminated CDATA block in C-CDA document.".into());
                };
                let cdata = &input[pos + 9..pos + 9 + end];
                if let Some(top) = stack.last_mut() {
                    top.text.push_str(cdata);
                }
                pos += 9 + end + 3;
            } else if input[pos..].starts_with("<?") {
                let Some(end) = input[pos + 2..].find("?>") else {
                    return Err("Unterminated XML processing instruction.".into());
                };
                pos += 2 + end + 2;
            } else if input[pos..].starts_with("<!") {
                // Fail-closed: reject ALL DTD / DOCTYPE / ENTITY declarations (XXE / external DTD defense).
                return Err(
                    "XML DOCTYPE and DTD declarations (`<!...`) are forbidden for security (XXE defense)."
                        .into(),
                );
            } else if input[pos..].starts_with("</") {
                let Some(end) = input[pos + 2..].find('>') else {
                    return Err("Unterminated XML closing tag.".into());
                };
                let close_name = strip_xml_prefix(input[pos + 2..pos + 2 + end].trim());
                let Some(node) = stack.pop() else {
                    return Err(format!("Unexpected closing XML tag `</{close_name}>`."));
                };
                if node.name != close_name {
                    return Err(format!(
                        "Mismatched XML closing tag: expected `</{}>`, found `</{close_name}>`.",
                        node.name
                    ));
                }
                if let Some(parent) = stack.last_mut() {
                    parent.children.push(node);
                } else {
                    root = Some(node);
                }
                pos += 2 + end + 1;
            } else {
                // Opening or self-closing tag
                let tag_end = find_tag_end(input, pos + 1)
                    .ok_or_else(|| "Unterminated XML opening tag.".to_string())?;
                let inner = &input[pos + 1..tag_end];
                let self_closing = inner.trim_end().ends_with('/');
                let clean = if self_closing {
                    inner.trim_end().trim_end_matches('/')
                } else {
                    inner
                };
                let (raw_name, attrs) = parse_tag_header(clean)?;
                let local_name = strip_xml_prefix(raw_name).to_string();

                node_count += 1;
                if node_count > MAX_XML_NODES {
                    return Err("C-CDA XML exceeded maximum element count safety cap.".into());
                }
                if stack.len() >= MAX_XML_DEPTH {
                    return Err("C-CDA XML exceeded maximum nesting depth safety cap.".into());
                }

                let node = XmlNode {
                    name: local_name,
                    attrs,
                    children: Vec::new(),
                    text: String::new(),
                };
                if self_closing {
                    if let Some(parent) = stack.last_mut() {
                        parent.children.push(node);
                    } else {
                        root = Some(node);
                    }
                } else {
                    stack.push(node);
                }
                pos = tag_end + 1;
            }
        } else {
            let next_lt = input[pos..]
                .find('<')
                .map(|i| pos + i)
                .unwrap_or(bytes.len());
            let chunk = &input[pos..next_lt];
            if let Some(top) = stack.last_mut() {
                if !chunk.trim().is_empty() {
                    if !top.text.is_empty() {
                        top.text.push(' ');
                    }
                    top.text.push_str(&decode_xml_entities(chunk));
                }
            }
            pos = next_lt;
        }
    }

    if !stack.is_empty() {
        return Err(format!(
            "Unclosed XML element `<{}>` in C-CDA document.",
            stack.last().map(|n| n.name.as_str()).unwrap_or("unknown")
        ));
    }

    root.ok_or_else(|| "Empty XML document.".to_string())
}

fn find_tag_end(input: &str, start: usize) -> Option<usize> {
    let mut quote: Option<u8> = None;
    for (rel, &b) in input.as_bytes()[start..].iter().enumerate() {
        match quote {
            Some(q) if b == q => quote = None,
            Some(_) => {}
            None if b == b'"' || b == b'\'' => quote = Some(b),
            None if b == b'>' => return Some(start + rel),
            None => {}
        }
    }
    None
}

type XmlAttrs = Vec<(String, String)>;

fn parse_tag_header(header: &str) -> Result<(&str, XmlAttrs), String> {
    let bytes = header.as_bytes();
    let mut i = 0;
    while i < bytes.len() && bytes[i].is_ascii_whitespace() {
        i += 1;
    }
    let name_start = i;
    while i < bytes.len() && !bytes[i].is_ascii_whitespace() {
        i += 1;
    }
    let name = &header[name_start..i];
    if name.is_empty() {
        return Err("Empty XML tag name.".into());
    }

    let mut attrs = Vec::new();
    while i < bytes.len() {
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        if i >= bytes.len() {
            break;
        }
        let key_start = i;
        while i < bytes.len() && bytes[i] != b'=' && !bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        let key = strip_xml_prefix(&header[key_start..i]).to_string();
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        if i >= bytes.len() || bytes[i] != b'=' {
            return Err(format!("Malformed XML attribute `{key}` in `<{name}>`."));
        }
        i += 1;
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        if i >= bytes.len() || (bytes[i] != b'"' && bytes[i] != b'\'') {
            return Err(format!("Expected quoted XML attribute value for `{key}`."));
        }
        let q = bytes[i];
        i += 1;
        let val_start = i;
        while i < bytes.len() && bytes[i] != q {
            i += 1;
        }
        if i >= bytes.len() {
            return Err(format!("Unterminated attribute value for `{key}`."));
        }
        let val = decode_xml_entities(&header[val_start..i]);
        i += 1;
        attrs.push((key, val));
    }

    Ok((name, attrs))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ehr_portal::merge_fhir_import_into_chart;
    use crate::find_patient;

    #[test]
    fn ccda_r21_sandbox_document_parses_all_sections_and_merges_with_dedup() {
        let nadia = find_patient("pt-nadia-kulkarni").expect("nadia");
        let xml = sandbox_ccda_xml_for_patient(&nadia);

        let import = ingest_ccda_xml("SimplePractice C-CDA", "org-simplepractice", &xml)
            .expect("valid C-CDA R2.1");
        assert_eq!(import.patient_name.as_deref(), Some("Nadia Kulkarni"));
        assert_eq!(import.patient_birth_date.as_deref(), Some("1988-04-14"));
        assert_eq!(import.confidentiality, ConfidentialityClass::StandardHipaa);
        assert_eq!(import.problems.len(), 1);
        assert_eq!(import.problems[0].code, "E55.9");
        assert_eq!(import.medications.len(), 1);
        assert!(import.medications[0].name.contains("Cholecalciferol"));
        assert_eq!(import.allergies.len(), 1);
        assert_eq!(import.allergies[0].substance, "Amoxicillin");
        // allergy record + results + vitals + plan of treatment = 4 records
        assert_eq!(import.records.len(), 4);
        assert!(import.warnings.is_empty());

        let mut chart = nadia.clone();
        let mut allergies = Vec::new();
        let first = merge_fhir_import_into_chart(&mut chart, &mut allergies, &import);
        assert_eq!(first.added_records, 4);
        assert_eq!(first.added_medications, 1);
        assert_eq!(first.added_problems, 1);
        assert_eq!(first.added_allergies, 1);

        let second = merge_fhir_import_into_chart(&mut chart, &mut allergies, &import);
        assert_eq!(second.added_records, 0);
        assert!(second.skipped_duplicates >= 7);
    }

    #[test]
    fn ccda_rejects_xxe_entities_and_warns_on_nullflavor_sections() {
        let xxe =
            r#"<!DOCTYPE foo [<!ENTITY xxe SYSTEM "file:///etc/passwd">]><ClinicalDocument/>"#;
        assert!(ingest_ccda_xml("Evil", "evil", xxe).is_err());

        // Plain external DOCTYPE SYSTEM/PUBLIC without ENTITY is also rejected fail-closed.
        let doctype_sys =
            r#"<!DOCTYPE ClinicalDocument SYSTEM "http://evil.example/dtd"><ClinicalDocument/>"#;
        assert!(ingest_ccda_xml("Evil", "evil", doctype_sys).is_err());

        // Restricted confidentialityCode="R" fails closed to Part2Sud.
        let restricted = r#"<?xml version="1.0"?>
        <ClinicalDocument xmlns="urn:hl7-org:v3">
          <confidentialityCode code="R" codeSystem="2.16.840.1.113883.5.25"/>
          <title>Behavioral Summary</title>
          <effectiveTime value="20261120"/>
          <component><structuredBody>
            <component><section>
              <code code="11506-3" displayName="Progress Note"/>
              <title>Progress Note</title>
              <text>Restricted clinical note.</text>
            </section></component>
          </structuredBody></component>
        </ClinicalDocument>"#;
        let restricted_import = ingest_ccda_xml("Clinic", "clinic-r", restricted).unwrap();
        assert_eq!(
            restricted_import.confidentiality,
            ConfidentialityClass::Part2Sud
        );

        let partial = r#"<?xml version="1.0"?>
        <ClinicalDocument xmlns="urn:hl7-org:v3">
          <title>Progress Note</title>
          <effectiveTime value="20261120"/>
          <component><structuredBody>
            <component><section nullFlavor="NI"><code code="11450-4"/></section></component>
            <component><section>
              <code code="11506-3" displayName="Progress Note"/>
              <title>Progress Note</title>
              <text>Patient doing well on current plan.</text>
            </section></component>
          </structuredBody></component>
        </ClinicalDocument>"#;
        let import = ingest_ccda_xml("Clinic", "clinic-1", partial).expect("partial ok");
        assert_eq!(import.records.len(), 1);
        assert_eq!(import.warnings.len(), 1);
    }
}
