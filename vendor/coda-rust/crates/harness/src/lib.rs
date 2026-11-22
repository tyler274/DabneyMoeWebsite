//! Product-neutral bounded execution and the existing skill catalog surface.
//!
//! [`engine`] owns the provider/tool loop and ordered turn. [`engine::Product`] binds it
//! to accepted work. [`responses`] owns the API wire protocol. Product hosts retain authority, effects,
//! and persistence. The harness reconstructs recovery from their admitted records and messages.
//! [`skills::Package`] holds the admitted skill catalog. A skill
//! never authorizes a tool or a patient read. Spec:
//! `intent/harness.md`.
#![deny(missing_docs)]
#![allow(
    clippy::map_err_ignore,
    reason = "converting to errors that keep their cause"
)]

#[cfg(test)]
extern crate self as harness;

pub mod engine;

pub mod provider;
pub mod responses;
pub mod settings;
pub mod skills;
pub mod transport;

/// Opaque provider cache partition for a product-owned conversation or job.
/// Length-prefixing each component keeps distinct scope tuples distinct.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct PromptCacheKey([u8; 32]);

impl PromptCacheKey {
    /// Hashes only stable scope identifiers. Never include messages or evidence.
    pub fn scoped(namespace: &str, identity: &[&str]) -> Self {
        use sha2::{Digest, Sha256};
        let mut hash = Sha256::new();
        for part in std::iter::once(namespace).chain(identity.iter().copied()) {
            hash.update((part.len() as u64).to_be_bytes());
            hash.update(part.as_bytes());
        }
        Self(hash.finalize().into())
    }
    fn wire(&self) -> String {
        self.0.iter().map(|byte| format!("{byte:02x}")).collect()
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;

    use super::skills::Package;
    use agent_skills::SkillView;

    fn skills_dir() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../skills")
    }

    #[test]
    fn skill_view_returns_the_body_or_a_named_gap() {
        let root = skills_dir();
        let harness = Package::load("chords", &root).unwrap();
        assert!(
            harness
                .catalog_instructions()
                .contains("\"kind\":\"approved_skill_catalog\"")
        );

        let body = fs::read_to_string(root.join("patient-record-retrieval/SKILL.md")).unwrap();
        match harness
            .execute_skill_view(&serde_json::json!({"name":"patient-record-retrieval"}))
            .unwrap()
        {
            SkillView::Available {
                content, resources, ..
            } => {
                assert_eq!(content, body);
                assert!(resources.is_some());
            }
            other => panic!("expected the skill body, got {other:?}"),
        }

        match harness
            .execute_skill_view(&serde_json::json!({"name":"not-a-skill"}))
            .unwrap()
        {
            SkillView::UnknownSkill { name, .. } => assert_eq!(name, "not-a-skill"),
            other => panic!("expected unavailable, got {other:?}"),
        }
    }
}
