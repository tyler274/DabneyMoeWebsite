//! Tool declaration for reading a product’s packaged guidance.

pub use agent_skills::{Package, SKILL_VIEW};

/// The shared skill protocol as an engine tool. Each product supplies the package
/// the index and reader may use. Reading guidance grants no other capability.
pub fn tool() -> crate::engine::Tool {
    crate::engine::Tool {
        name: SKILL_VIEW.into(),
        description: "Read one exact approved packaged skill or one supporting resource returned in its metadata. Supply the name to read SKILL.md; use an exact returned package-relative path for a supporting file. Returns the exact content and registered resource paths, or an explicit unavailable outcome. Large results have a preview; read_tool_result reads the retained content. Read guidance does not grant tools or action authority.".into(),
        parameters: serde_json::json!({
            "type": "object",
            "properties": {
                "name": {"type":"string", "minLength":1, "maxLength":64, "pattern":"^[a-z0-9]+(?:-[a-z0-9]+)*$"},
                "file_path": {"anyOf":[{"type":"string", "minLength":1, "maxLength":500}, {"type":"null"}]}
            },
            "required": ["name"],
            "additionalProperties": false
        }),
    }
}
