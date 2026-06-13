//! Output schema — shared by Anthropic tools + OpenAI-compat json_schema.
//!
//! Per spec §9.1. Field-level word-count constraints are documented in the
//! schema `description` strings so the LLM sees them; enforcement also happens
//! in `generator::validate` as a belt-and-braces check.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GeneratedArtifacts {
    pub spec: SpecDoc,
    pub plan: PlanDoc,
    pub tasks: Vec<TaskItem>,
    pub brain_refs: Vec<BrainRef>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SpecDoc {
    pub overview: String,
    #[serde(default)]
    pub actors: Vec<String>,
    pub acceptance_criteria: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PlanDoc {
    pub steps: Vec<PlanStep>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PlanStep {
    pub id: String,
    pub action: String,
    #[serde(default)]
    pub grounding_frame_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TaskItem {
    pub id: String,
    pub text: String,
    #[serde(default)]
    pub grounding_frame_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BrainRef {
    pub frame_id: String,
    pub why_relevant: String,
}

/// The JSON Schema the LLM must conform to.
pub fn output_schema() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "additionalProperties": false,
        "required": ["spec", "plan", "tasks", "brain_refs"],
        "properties": {
            "spec": {
                "type": "object",
                "additionalProperties": false,
                "required": ["overview", "actors", "acceptance_criteria"],
                "properties": {
                    "overview": {
                        "type": "string",
                        "description": "40-200 words, 2-4 sentences."
                    },
                    "actors": { "type": "array", "items": { "type": "string" } },
                    "acceptance_criteria": {
                        "type": "array",
                        "minItems": 1,
                        "items": {
                            "type": "string",
                            "description": "15-80 words, 1-2 sentences, self-contained, verbatim proper nouns. Up to 100 words / 3 sentences for detail-rich criteria."
                        }
                    }
                }
            },
            "plan": {
                "type": "object",
                "additionalProperties": false,
                "required": ["steps"],
                "properties": {
                    "steps": {
                        "type": "array",
                        "minItems": 1,
                        "items": {
                            "type": "object",
                            "additionalProperties": false,
                            "required": ["id", "action", "grounding_frame_ids"],
                            "properties": {
                                "id": { "type": "string" },
                                "action": {
                                    "type": "string",
                                    "description": "15-80 words, 1-2 sentences."
                                },
                                "grounding_frame_ids": {
                                    "type": "array",
                                    "items": { "type": "string" }
                                }
                            }
                        }
                    }
                }
            },
            "tasks": {
                "type": "array",
                "minItems": 1,
                "items": {
                    "type": "object",
                    "additionalProperties": false,
                    "required": ["id", "text", "grounding_frame_ids"],
                    "properties": {
                        "id": { "type": "string" },
                        "text": { "type": "string" },
                        "grounding_frame_ids": { "type": "array", "items": { "type": "string" } }
                    }
                }
            },
            "brain_refs": {
                "type": "array",
                "items": {
                    "type": "object",
                    "additionalProperties": false,
                    "required": ["frame_id", "why_relevant"],
                    "properties": {
                        "frame_id": { "type": "string" },
                        "why_relevant": { "type": "string" }
                    }
                }
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schema_has_all_four_top_level_keys() {
        let v = output_schema();
        let required = v["required"].as_array().unwrap();
        let keys: Vec<&str> = required.iter().filter_map(|r| r.as_str()).collect();
        assert!(keys.contains(&"spec"));
        assert!(keys.contains(&"plan"));
        assert!(keys.contains(&"tasks"));
        assert!(keys.contains(&"brain_refs"));
    }

    #[test]
    fn artifacts_roundtrip() {
        let a = GeneratedArtifacts {
            spec: SpecDoc {
                overview: "Creates an account.".into(),
                actors: vec!["operator".into()],
                acceptance_criteria: vec!["Valid request returns 201.".into()],
            },
            plan: PlanDoc {
                steps: vec![PlanStep {
                    id: "S1".into(),
                    action: "Implement handler.".into(),
                    grounding_frame_ids: vec!["frame-1".into()],
                }],
            },
            tasks: vec![TaskItem {
                id: "T1".into(),
                text: "Write integration test.".into(),
                grounding_frame_ids: vec!["frame-1".into()],
            }],
            brain_refs: vec![BrainRef {
                frame_id: "frame-1".into(),
                why_relevant: "Defines the proc.".into(),
            }],
        };
        let encoded = serde_json::to_string(&a).unwrap();
        let decoded: GeneratedArtifacts = serde_json::from_str(&encoded).unwrap();
        assert_eq!(a, decoded);
    }

    #[test]
    fn artifacts_reject_missing_fields() {
        let bad = serde_json::json!({ "spec": { "overview": "x" } });
        let res: Result<GeneratedArtifacts, _> = serde_json::from_value(bad);
        assert!(res.is_err());
    }
}
