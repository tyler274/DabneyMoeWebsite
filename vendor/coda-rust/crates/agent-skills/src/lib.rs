//! Loads the clinical skill catalog and answers `skill_view`.
//!
//! Each product supplies its own package root. Each skill directory contains a
//! `SKILL.md` with YAML front matter. The loader refuses symlinks and identifies
//! files by their SHA-256 digest. Reading guidance grants no tool or patient access.
#![deny(missing_docs)]
#![allow(
    clippy::map_err_ignore,
    reason = "converting to errors that keep their cause"
)]

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

/// The tool that opens guidance from a product's admitted package.
pub const SKILL_VIEW: &str = "skill_view";

/// A correctable request outside the advertised skill tool schema.
#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct SkillArgumentError(&'static str);

impl SkillArgumentError {
    /// Model feedback for the field that failed validation.
    pub fn message(&self) -> &'static str {
        self.0
    }
}

/// One skill name and, for a supporting resource, its registered path.
#[derive(Debug, Clone, PartialEq, Eq)]
struct SkillViewRequest {
    /// Skill name.
    pub name: String,
    /// Package-relative resource path, or `None` for `SKILL.md`.
    pub file_path: Option<String>,
}

impl SkillViewRequest {
    /// Validates the exact argument object. Unknown or missing keys, or an
    /// invalid resource path receive corrective feedback.
    pub fn parse(arguments: &Value) -> Result<Self, SkillArgumentError> {
        let object = arguments.as_object().ok_or(SkillArgumentError(
            "Invalid skill_view arguments: arguments must be an object",
        ))?;
        if object
            .keys()
            .any(|key| !matches!(key.as_str(), "name" | "file_path"))
        {
            return Err(SkillArgumentError(
                "Invalid skill_view arguments: unexpected field; use name and an optional file_path",
            ));
        }
        let name = object
            .get("name")
            .and_then(Value::as_str)
            .ok_or(SkillArgumentError(
                "Invalid skill_view arguments: name must be a string",
            ))?;
        if !valid_skill_name(name) {
            return Err(SkillArgumentError(
                "Invalid skill_view arguments: name must be 1-64 lowercase letters, digits and single hyphens",
            ));
        }
        let file_path = match object.get("file_path") {
            None | Some(Value::Null) => None,
            Some(Value::String(path)) if (1..=500).contains(&path.chars().count()) => {
                Some(path.clone())
            }
            Some(_) => {
                return Err(SkillArgumentError(
                    "Invalid skill_view arguments: file_path must be null or a 1-500 character path",
                ));
            }
        };
        Ok(Self {
            name: name.to_owned(),
            file_path,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
/// The `SkillResource` value exchanged by this crate.
pub struct SkillResource {
    /// The `file_path` value.
    pub file_path: String,
    /// SHA-256 of the exact file text: a short stable name, so saved runs can be
    /// grouped or compared by skill version without comparing whole texts.
    pub version_id: String,
    /// The `byte_length` value.
    pub byte_length: usize,
    /// The `content` value.
    pub content: String,
}

/// Immutable guidance selected by a product for its accepted work. This
/// snapshot supplies both the resident index and every later resource read.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Package {
    /// Product whose approved folder supplied the package.
    pub product: String,
    /// Digest of the sorted skills and their exact resource contents: a short
    /// stable name, so saved runs can be grouped or compared by package without
    /// comparing whole packages.
    pub version: String,
    skills: Vec<AgentSkill>,
}

impl Package {
    /// Creates an empty admitted skill package for `product` when no on-disk skills folder is present.
    pub fn empty(product: &str) -> Self {
        let skills = Vec::new();
        let version = version_id(&serde_json::to_vec(&skills).expect("skill data serializes"));
        Self {
            product: product.into(),
            version,
            skills,
        }
    }

    /// Load and validate the selected product folder.
    pub fn load(product: &str, root: &Path) -> Result<Self, String> {
        let skills = load_agent_skills(root)?;
        let version = version_id(&serde_json::to_vec(&skills).expect("skill data serializes"));
        Ok(Self {
            product: product.into(),
            version,
            skills,
        })
    }
    /// Metadata from this admitted package, excluding all bodies.
    pub fn catalog_instructions(&self) -> String {
        catalog_instructions(&self.skills)
    }
    /// The section that loads `name` at the start of a job: the line
    /// `Loaded skill: <name>`, then the whole `SKILL.md`, the text
    /// `skill_view` returns for that name with no `file_path`. `None` when this
    /// package has no skill of that name.
    pub fn loaded_skill_section(&self, name: &str) -> Option<String> {
        named_skill(&self.skills, name)
            .map(|skill| format!("Loaded skill: {}\n{}", skill.name, skill.content))
    }
    /// Open only a name or path present in this admitted package.
    pub fn execute_skill_view(&self, arguments: &Value) -> Result<SkillView, SkillArgumentError> {
        execute_skill_view(&self.skills, arguments)
    }
}

/// One visible skill, either loaded from a product package or frozen with a run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentSkill {
    /// Indexed name and the only key for opening this skill.
    pub name: String,
    /// Resident index description.
    pub description: String,
    /// SHA-256 of the exact skill text: a short stable name, so saved runs can be
    /// grouped or compared by skill version without comparing whole texts.
    #[serde(rename = "version")]
    pub version_id: String,
    /// Exact text available to the run.
    pub content: String,
    /// Registered files for products offering resource reads.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub resources: Vec<SkillResource>,
}

/// Loads each skill directory under `root`.
pub fn load_agent_skills(root: &Path) -> Result<Vec<AgentSkill>, String> {
    let root = fs::canonicalize(root).map_err(|error| error.to_string())?;
    let mut skills = Vec::new();
    let entries = fs::read_dir(&root).map_err(|error| error.to_string())?;
    for entry in entries {
        let entry = entry.map_err(|error| error.to_string())?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') {
            continue;
        }
        let file_type = entry.file_type().map_err(|error| error.to_string())?;
        if file_type.is_symlink() {
            return Err(format!(
                "Packaged skill entry {name} cannot be a symbolic link"
            ));
        }
        if !file_type.is_dir() {
            continue;
        }
        skills.push(load_skill(&root, &name)?);
    }
    if skills.is_empty() {
        return Err("No packaged skills were found".to_owned());
    }
    skills.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(skills)
}

/// Renders the model-facing metadata index, without opening a skill body.
pub fn catalog_instructions(skills: &[AgentSkill]) -> String {
    format!(
        "Approved skill catalog (metadata only; read bodies with skill_view):\n{}",
        catalog_json(skills)
    )
}

/// Parses one common invocation and opens only guidance in this product's supplied package.
pub fn execute_skill_view(
    skills: &[AgentSkill],
    arguments: &Value,
) -> Result<SkillView, SkillArgumentError> {
    let request = SkillViewRequest::parse(arguments)?;
    view_skill(skills, &request.name, request.file_path.as_deref())
}

/// Publishes the compact catalog. Resource metadata only; no file bodies.
fn catalog_json(skills: &[AgentSkill]) -> String {
    let catalog = Catalog {
        kind: "approved_skill_catalog",
        skills: skills.iter().map(SkillMetadata::from).collect(),
    };
    serde_json::to_string(&catalog).expect("skill catalog is json")
}

/// Opens a name only within the skills visible in this product or run's index.
fn named_skill<'a>(skills: &'a [AgentSkill], name: &str) -> Option<&'a AgentSkill> {
    skills.iter().find(|skill| skill.name == name)
}

/// Reads one skill body or one registered resource.
/// The product retains the exact body; the harness owns its presentation.
fn view_skill(
    skills: &[AgentSkill],
    name: &str,
    file_path: Option<&str>,
) -> Result<SkillView, SkillArgumentError> {
    // An unknown name returns the names of the skills this package has, as
    // Hermes does, so the model can correct its request.
    let Some(skill) = named_skill(skills, name) else {
        return Ok(SkillView::UnknownSkill {
            name: name.to_owned(),
            file_path: file_path.map(str::to_owned),
            available_skills: skills.iter().map(|skill| skill.name.clone()).collect(),
        });
    };
    // `SKILL.md` by name is the skill body, as in the Hermes reference.
    let file_path = file_path.filter(|path| *path != "SKILL.md");
    let resource = match file_path {
        None => Some(SkillResource {
            file_path: "SKILL.md".to_owned(),
            version_id: skill.version_id.clone(),
            byte_length: skill.content.len(),
            content: skill.content.clone(),
        }),
        Some(path) if is_registered_path(path) => skill
            .resources
            .iter()
            .find(|candidate| candidate.file_path == path)
            .cloned(),
        Some(_) => None,
    };
    let paths = || {
        skill
            .resources
            .iter()
            .map(|item| item.file_path.clone())
            .collect()
    };
    // A path the skill does not have returns the paths it does have, so the
    // model can correct its request.
    let Some(resource) = resource else {
        return Ok(SkillView::Unavailable {
            name: name.to_owned(),
            file_path: file_path.map(str::to_owned),
            resources: paths(),
        });
    };
    Ok(SkillView::Available {
        name: skill.name.clone(),
        file_path: file_path.map(str::to_owned),
        content: resource.content,
        resources: file_path.is_none().then(paths),
    })
}

fn is_registered_path(path: &str) -> bool {
    !path.is_empty()
        && path.len() <= 500
        && !path.starts_with('/')
        && !path.contains('\\')
        && path
            .split('/')
            .all(|component| !component.is_empty() && component != "." && component != "..")
}

/// One `skill_view` outcome.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SkillView {
    /// The whole body, plus registered resource paths when `file_path` was null
    /// or `SKILL.md`.
    Available {
        /// Skill name.
        name: String,
        /// The requested path. Null is the skill body.
        file_path: Option<String>,
        /// The whole file. Never a prefix.
        content: String,
        /// Present only for the skill body.
        resources: Option<Vec<String>>,
    },
    /// A path the named skill does not have.
    Unavailable {
        /// Requested skill name.
        name: String,
        /// Requested path.
        file_path: Option<String>,
        /// The skill's registered resource paths.
        resources: Vec<String>,
    },
    /// A name the package does not have.
    UnknownSkill {
        /// Requested skill name.
        name: String,
        /// Requested path.
        file_path: Option<String>,
        /// Names of the skills in the package the job was given.
        available_skills: Vec<String>,
    },
}

impl Serialize for SkillView {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Available {
                name,
                file_path,
                content,
                resources,
            } => AvailableView {
                status: "available",
                name,
                file_path: file_path.as_deref(),
                content,
                resources: resources.as_deref(),
            }
            .serialize(serializer),
            Self::Unavailable {
                name,
                file_path,
                resources,
            } => UnavailableView {
                status: "unavailable",
                name,
                file_path: file_path.as_deref(),
                resources: Some(resources),
                available_skills: None,
            }
            .serialize(serializer),
            Self::UnknownSkill {
                name,
                file_path,
                available_skills,
            } => UnavailableView {
                status: "unavailable",
                name,
                file_path: file_path.as_deref(),
                resources: None,
                available_skills: Some(available_skills),
            }
            .serialize(serializer),
        }
    }
}

impl SkillView {
    /// The exact JSON result, before harness presentation.
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).expect("skill view is json")
    }
}

#[derive(Serialize)]
struct Catalog<'a> {
    kind: &'a str,
    skills: Vec<SkillMetadata<'a>>,
}

#[derive(Serialize)]
struct SkillMetadata<'a> {
    name: &'a str,
    description: &'a str,
    resources: Vec<&'a str>,
}

#[derive(Serialize)]
struct AvailableView<'a> {
    status: &'a str,
    name: &'a str,
    file_path: Option<&'a str>,
    content: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    resources: Option<&'a [String]>,
}

#[derive(Serialize)]
struct UnavailableView<'a> {
    status: &'a str,
    name: &'a str,
    file_path: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    resources: Option<&'a [String]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    available_skills: Option<&'a [String]>,
}

impl<'a> From<&'a AgentSkill> for SkillMetadata<'a> {
    fn from(skill: &'a AgentSkill) -> Self {
        Self {
            name: &skill.name,
            description: &skill.description,
            resources: skill
                .resources
                .iter()
                .map(|resource| resource.file_path.as_str())
                .collect(),
        }
    }
}

fn load_skill(root: &Path, folder: &str) -> Result<AgentSkill, String> {
    let folder_path = resolve_inside(root, folder)?;
    let meta = fs::symlink_metadata(&folder_path).map_err(|error| error.to_string())?;
    if meta.file_type().is_symlink() || !meta.is_dir() {
        return Err(format!("Packaged skill {folder} is not a directory"));
    }
    let main = load_resource(&folder_path, "SKILL.md")?;
    let (yaml, body) = split_frontmatter(&main.content)
        .ok_or_else(|| format!("Packaged skill {folder} has no YAML frontmatter"))?;
    if body.trim().is_empty() {
        return Err(format!("Packaged skill {folder} has no instructions"));
    }
    let fields = parse_frontmatter(yaml).map_err(|error| {
        format!("Packaged skill {folder} has invalid YAML frontmatter: {error}")
    })?;
    let name = yaml_string(&fields, "name")
        .filter(|value| !value.is_empty())
        .ok_or_else(|| format!("Packaged skill {folder} has no name"))?;
    if !valid_skill_name(&name) {
        return Err(format!(
            "Packaged skill {folder} has an invalid standard skill name"
        ));
    }
    if name != folder {
        return Err(format!(
            "Packaged skill folder and name must both be {folder}"
        ));
    }
    let description = yaml_string(&fields, "description")
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| format!("Packaged skill {folder} has no description"))?;
    if description.len() > 1024 {
        return Err(format!(
            "Packaged skill {folder} description exceeds 1024 characters"
        ));
    }
    let description = description.trim().to_owned();
    Ok(AgentSkill {
        name,
        description,
        version_id: main.version_id,
        content: main.content,
        resources: collect_resources(&folder_path)?,
    })
}

fn collect_resources(folder: &Path) -> Result<Vec<SkillResource>, String> {
    let mut resources = Vec::new();
    visit(folder, folder, &mut resources)?;
    resources.sort_by(|left, right| left.file_path.cmp(&right.file_path));
    Ok(resources)
}

fn visit(
    folder: &Path,
    directory: &Path,
    resources: &mut Vec<SkillResource>,
) -> Result<(), String> {
    let mut entries = fs::read_dir(directory)
        .map_err(|error| error.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') {
            continue;
        }
        let file_type = entry.file_type().map_err(|error| error.to_string())?;
        let relative = directory
            .strip_prefix(folder)
            .unwrap_or(Path::new(""))
            .join(&name);
        let relative = relative.to_string_lossy().replace('\\', "/");
        if file_type.is_symlink() {
            return Err(format!(
                "Packaged skill resource {relative} cannot be a symbolic link"
            ));
        }
        if file_type.is_dir() {
            visit(folder, &entry.path(), resources)?;
        } else if file_type.is_file() && relative != "SKILL.md" {
            resources.push(load_resource(folder, &relative)?);
        } else if !file_type.is_file() {
            return Err(format!(
                "Packaged skill resource {relative} is not a regular file"
            ));
        }
    }
    Ok(())
}

fn load_resource(folder: &Path, file_path: &str) -> Result<SkillResource, String> {
    let absolute = resolve_inside(folder, file_path)?;
    let meta = fs::symlink_metadata(&absolute).map_err(|error| error.to_string())?;
    if meta.file_type().is_symlink() || !meta.is_file() {
        return Err(format!(
            "Packaged skill resource {file_path} is not a regular file"
        ));
    }
    let bytes = fs::read(&absolute).map_err(|error| error.to_string())?;
    let content = String::from_utf8(bytes.clone())
        .map_err(|_| format!("Packaged skill resource {file_path} is not UTF-8 text"))?;
    Ok(SkillResource {
        file_path: file_path.to_owned(),
        version_id: version_id(&bytes),
        byte_length: bytes.len(),
        content,
    })
}

/// Returns `sha256:` and the hex SHA-256 of `bytes`, the short stable name of an
/// exact skill text or package.
pub fn version_id(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut out = String::with_capacity("sha256:".len() + digest.len() * 2);
    out.push_str("sha256:");
    for byte in digest {
        use std::fmt::Write as _;
        let _ = write!(&mut out, "{byte:02x}");
    }
    out
}

fn split_frontmatter(content: &str) -> Option<(&str, &str)> {
    let rest = content
        .strip_prefix("---\n")
        .or_else(|| content.strip_prefix("---\r\n"))?;
    let (yaml, after) = rest.split_once("\n---")?;
    let body = after
        .strip_prefix('\n')
        .or_else(|| after.strip_prefix("\r\n"))
        .unwrap_or(after);
    Some((yaml, body))
}

fn parse_frontmatter(yaml: &str) -> Result<serde_yaml::Mapping, String> {
    let value: serde_yaml::Value = serde_yaml::from_str(yaml).map_err(|error| error.to_string())?;
    match value {
        serde_yaml::Value::Mapping(mapping) => Ok(mapping),
        _ => Err("frontmatter must be a mapping".to_owned()),
    }
}

fn yaml_string(fields: &serde_yaml::Mapping, key: &str) -> Option<String> {
    fields
        .get(serde_yaml::Value::String(key.to_owned()))
        .and_then(serde_yaml::Value::as_str)
        .map(str::to_owned)
}

fn valid_skill_name(name: &str) -> bool {
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if name.len() > 64 || !first.is_ascii_lowercase() && !first.is_ascii_digit() {
        return false;
    }
    let mut dash = false;
    for character in chars {
        if character.is_ascii_lowercase() || character.is_ascii_digit() {
            dash = false;
        } else if character == '-' && !dash {
            dash = true;
        } else {
            return false;
        }
    }
    !name.ends_with('-')
}

fn resolve_inside(root: &Path, child: &str) -> Result<PathBuf, String> {
    if child.is_empty()
        || child.starts_with('/')
        || child.contains('\\')
        || child.split('/').any(|part| part == "..")
    {
        return Err(format!("Invalid packaged skill path {child}"));
    }
    Ok(root.join(child))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arguments_follow_the_reference_schema() {
        let ok = serde_json::json!({"name":"pre-visit-preparation"});
        assert_eq!(
            SkillViewRequest::parse(&ok).unwrap(),
            SkillViewRequest {
                name: "pre-visit-preparation".into(),
                file_path: None,
            }
        );
        let path = serde_json::json!({"name":"a1-b2","file_path":"references/x.md"});
        assert_eq!(
            SkillViewRequest::parse(&path).unwrap().file_path.as_deref(),
            Some("references/x.md")
        );
        for bad in [
            serde_json::json!([]),
            serde_json::json!({"name":"pre-visit-preparation","max_result_bytes":4096}),
            serde_json::json!({"name":"pre-visit-preparation","file_path":""}),
            serde_json::json!({"name":"pre-visit-preparation","file_path":7}),
            serde_json::json!({"name":"pre-visit-preparation","file_path":"x".repeat(501)}),
            serde_json::json!({"name":"Pre-Visit"}),
            serde_json::json!({"name":"pre--visit"}),
            serde_json::json!({"name":"-pre"}),
            serde_json::json!({"name":""}),
            serde_json::json!({"name":"a".repeat(65)}),
            serde_json::json!({"name":"pre-visit-preparation","extra":1}),
        ] {
            let reason = SkillViewRequest::parse(&bad).unwrap_err();
            assert!(
                reason.message().starts_with("Invalid skill_view arguments"),
                "{bad}"
            );
        }
    }

    #[test]
    fn the_supplied_package_controls_listing_and_opening() {
        let package = record_retrieval_only("listing");
        let index: Value = serde_json::from_str(&catalog_json(&package.skills)).unwrap();
        assert_eq!(index["skills"].as_array().unwrap().len(), 1);
        assert_eq!(index["skills"][0]["name"], "patient-record-retrieval");
        assert!(
            !package
                .catalog_instructions()
                .contains("patient-communication")
        );
        for (name, expected) in [
            ("patient-record-retrieval", "available"),
            ("patient-communication", "unavailable"),
        ] {
            let result = package
                .execute_skill_view(&serde_json::json!({"name":name}))
                .unwrap();
            let opened = serde_json::to_value(result).unwrap();
            assert_eq!(opened["status"], expected);
        }
    }

    fn catalog_root() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../skills")
    }

    /// A package loaded from a folder that holds only patient-record-retrieval,
    /// while the workspace catalog also has patient-communication.
    fn record_retrieval_only(test: &str) -> Package {
        let root = std::env::temp_dir().join(format!("coda-skills-{test}-{}", std::process::id()));
        let folder = root.join("patient-record-retrieval");
        fs::create_dir_all(&folder).unwrap();
        fs::copy(
            catalog_root().join("patient-record-retrieval/SKILL.md"),
            folder.join("SKILL.md"),
        )
        .unwrap();
        let package = Package::load("chords", &root).unwrap();
        let _ = fs::remove_dir_all(&root);
        package
    }

    #[test]
    fn catalog_matches_the_skill_directory_bytes() {
        let root = catalog_root();
        let skills = load_agent_skills(&root).unwrap();
        let mut folders = fs::read_dir(&root)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .filter(|name| !name.starts_with('.'))
            .collect::<Vec<_>>();
        folders.sort();
        let names = skills
            .iter()
            .map(|skill| skill.name.clone())
            .collect::<Vec<_>>();
        assert_eq!(names, folders);
        let catalog = catalog_json(&skills);
        assert!(catalog.contains("\"kind\":\"approved_skill_catalog\""));
        assert!(!catalog.contains("\"content\""));
        for skill in skills {
            let bytes = fs::read(root.join(&skill.name).join("SKILL.md")).unwrap();
            assert_eq!(skill.version_id, version_id(&bytes));
            assert_eq!(skill.content.as_bytes(), bytes);
            assert!(!skill.description.is_empty());
        }
    }

    #[test]
    fn view_json_preserves_the_exact_body_and_names_its_outcome() {
        let skills = skills_with_guide();
        let guide = "Read the registered guide exactly.";
        let body = view_skill(&skills, "patient-record-retrieval", None).unwrap();
        let encoded = body.to_json();
        let SkillView::Available { content, .. } = &body else {
            panic!("expected the body, got {body:?}");
        };
        let value: serde_json::Value = serde_json::from_str(&encoded).unwrap();
        assert_eq!(value["status"], "available");
        assert!(
            value.get("version_id").is_none(),
            "skill output exposes a digest to the model"
        );
        assert!(
            value.get("byte_length").is_none(),
            "skill output exposes byte accounting to the model"
        );
        assert_eq!(value["file_path"], serde_json::Value::Null);
        assert_eq!(value["content"].as_str(), Some(content.as_str()));
        assert_eq!(
            value["resources"],
            serde_json::json!(["references/authoring-guide.md"])
        );
        let resource = view_skill(
            &skills,
            "patient-record-retrieval",
            Some("references/authoring-guide.md"),
        )
        .unwrap()
        .to_json();
        let value: serde_json::Value = serde_json::from_str(&resource).unwrap();
        assert_eq!(value["file_path"], "references/authoring-guide.md");
        assert_eq!(value["content"], guide);
        assert!(value.get("version_id").is_none());
        assert!(value.get("byte_length").is_none());
        assert!(value.get("resources").is_none());
        let mut large = skills.clone();
        let skill = large
            .iter_mut()
            .find(|skill| skill.name == "patient-record-retrieval")
            .unwrap();
        skill.content = "Guidance. ".repeat(8_000);
        skill.version_id = version_id(skill.content.as_bytes());
        let opened = execute_skill_view(
            &large,
            &serde_json::json!({"name":"patient-record-retrieval"}),
        )
        .unwrap();
        assert!(
            matches!(opened, SkillView::Available { content, .. } if content.len() == 80_000),
            "a loaded skill body must remain available for harness paging"
        );
    }

    /// The packaged catalog, with one registered resource on patient-record-retrieval.
    fn skills_with_guide() -> Vec<AgentSkill> {
        let mut skills = load_agent_skills(&catalog_root()).unwrap();
        let guide = "Read the registered guide exactly.";
        skills
            .iter_mut()
            .find(|skill| skill.name == "patient-record-retrieval")
            .unwrap()
            .resources
            .push(SkillResource {
                file_path: "references/authoring-guide.md".into(),
                version_id: version_id(guide.as_bytes()),
                byte_length: guide.len(),
                content: guide.into(),
            });
        skills
    }

    #[test]
    fn skill_md_by_name_opens_the_body() {
        let skills = skills_with_guide();
        let named = execute_skill_view(
            &skills,
            &serde_json::json!({"name":"patient-record-retrieval","file_path":"SKILL.md"}),
        )
        .unwrap();
        let absent = execute_skill_view(
            &skills,
            &serde_json::json!({"name":"patient-record-retrieval"}),
        )
        .unwrap();
        assert_eq!(named.to_json(), absent.to_json());
        let value: Value = serde_json::from_str(&named.to_json()).unwrap();
        assert_eq!(value["status"], "available");
        assert_eq!(
            value["resources"],
            serde_json::json!(["references/authoring-guide.md"])
        );
    }

    #[test]
    fn missing_file_on_a_known_skill_lists_its_files() {
        let skills = skills_with_guide();
        let missing: Value = serde_json::from_str(
            &view_skill(
                &skills,
                "patient-record-retrieval",
                Some("references/absent.md"),
            )
            .unwrap()
            .to_json(),
        )
        .unwrap();
        assert_eq!(
            missing,
            serde_json::json!({"status":"unavailable","name":"patient-record-retrieval","file_path":"references/absent.md","resources":["references/authoring-guide.md"]})
        );
        let bare: Value = serde_json::from_str(
            &view_skill(
                &skills,
                "patient-communication",
                Some("references/absent.md"),
            )
            .unwrap()
            .to_json(),
        )
        .unwrap();
        assert_eq!(bare["status"], "unavailable");
        assert_eq!(bare["resources"], serde_json::json!([]));
    }

    #[test]
    fn unknown_skill_lists_the_skills_of_the_package() {
        let package = record_retrieval_only("unknown");
        for path in [None, Some("SKILL.md")] {
            let value = serde_json::to_value(
                package
                    .execute_skill_view(&serde_json::json!({"name":"not-a-skill","file_path":path}))
                    .unwrap(),
            )
            .unwrap();
            assert_eq!(
                value,
                serde_json::json!({"status":"unavailable","name":"not-a-skill","file_path":path,
                    "available_skills":["patient-record-retrieval"]})
            );
        }
    }

    #[test]
    fn frontmatter_keeps_a_colon_inside_a_yaml_string() {
        let root = std::env::temp_dir().join(format!("coda-skills-{}", std::process::id()));
        let folder = root.join("visit-note");
        fs::create_dir_all(&folder).unwrap();
        fs::write(
            folder.join("SKILL.md"),
            "---\nname: visit-note\ndescription: \"Use when: the note has a colon\"\n---\n\nKeep the chart.\n",
        )
        .unwrap();
        let skills = load_agent_skills(&root).unwrap();
        assert_eq!(skills[0].description, "Use when: the note has a colon");
        fs::write(
            folder.join("SKILL.md"),
            "---\n- not-a-mapping\n---\n\nBody.\n",
        )
        .unwrap();
        assert!(load_agent_skills(&root).is_err());
        let _ = fs::remove_dir_all(root);
    }
}
