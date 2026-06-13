//! Shared types for the Dev Spec pipeline. The data flow is:
//!
//!   markdown files  --parse-->  Vec<DevSpecEndpoint>
//!                   --erd----->  Erd { entities, endpoints }
//!                   --borrow-->  Vec<BorrowDecision>
//!                   --emit---->  CREATE TABLE SQL + registry MERGE SQL
//!
//! Each stage's output is canonical (sorted, deterministic) so re-runs
//! produce byte-identical artifacts when inputs are unchanged.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// One endpoint as parsed from a Dev Spec markdown file.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DevSpecEndpoint {
    /// HTTP verb in uppercase: GET / POST / PUT / PATCH / DELETE.
    pub method: String,
    /// Path template with `{paramName}` placeholders, e.g.
    /// `/cardholders/{cardholder_id}/transitions`.
    pub path: String,
    /// Top-level summary harvested from the H1 or first prose line.
    pub summary: String,
    /// Source markdown filename relative to `4-expectations/Dev Planning/`.
    /// Used for traceability in derived artifacts.
    pub source_file: String,
    /// Path-template parameters (e.g. `cardholder_id`).
    pub path_params: Vec<DevSpecParam>,
    /// Request body schema, parsed from the YAML "Schema" block.
    /// `None` for verbs that conventionally lack a body (GET, DELETE).
    pub request_body: Option<DevSpecSchema>,
    /// 200/201 success response body schema, when present.
    pub response_body: Option<DevSpecSchema>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(default)]
pub struct DevSpecParam {
    pub name: String,
    pub ty: String,                         // canonical type token: "uuid", "string", "int", "date-time"
    pub required: bool,
    pub description: String,
    /// Location: `path`, `query`, `header`. Defaults to `path` for
    /// path-template params (legacy callers).
    pub location: String,
    /// Default value, when the Dev Spec's parameter `Schema` declares
    /// one (`Default: 1`). Carried through to OpenAPI as `default:`.
    pub default: Option<String>,
    /// Inclusive lower bound, e.g. `Minimum: 1`. Carried as `minimum:`.
    pub minimum: Option<String>,
    /// Inclusive upper bound, e.g. `Maximum: 100`. Carried as `maximum:`.
    pub maximum: Option<String>,
    /// Optional example value (e.g. `Example: 1`).
    pub example: Option<String>,
}

/// Recursive schema node. Mirrors the YAML "Schema" blocks in the
/// markdown — `Type: object` with nested `Properties:`, or a leaf
/// scalar with `Type: string` + `Format`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(default)]
pub struct DevSpecSchema {
    pub ty: String,                // "object", "string", "integer", "array", ...
    pub format: Option<String>,    // "uuid", "date-time", ...
    pub description: Option<String>,
    /// Example value harvested from the markdown's `Example:` line.
    /// Carried through to OpenAPI as the `example:` schema annotation
    /// so consumers see realistic placeholder values.
    pub example: Option<String>,
    /// For `Type: object`: ordered field name → child schema. Insertion
    /// order is preserved in the markdown so we use BTreeMap for
    /// canonical sorted output.
    pub properties: BTreeMap<String, DevSpecSchema>,
    /// For `Type: array`: schema of elements.
    pub items: Option<Box<DevSpecSchema>>,
}

/// Derived ERD — what the contract implies must exist.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct Erd {
    pub entities: BTreeMap<String, Entity>,
    pub endpoints: Vec<DevSpecEndpoint>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Entity {
    pub name: String,                 // PascalCase: "Cardholder", "Address"
    pub columns: Vec<Column>,
    pub primary_key: String,          // column name
    pub foreign_keys: Vec<ForeignKey>,
    /// Source endpoint(s) that introduced this entity. Diagnostic only.
    pub introduced_by: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Column {
    pub name: String,                 // PascalCase as in Dev Spec: "FirstName"
    pub ty: String,                   // canonical: "uuid", "string(N)", "datetime", "int", "json"
    pub nullable: bool,
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ForeignKey {
    pub column: String,
    pub references_entity: String,
    pub references_column: String,
}

/// Outcome of TXN-borrow lookup for one entity.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BorrowDecision {
    pub entity: String,
    pub borrowed_from: Option<String>, // "cardholder.cpf_Client_Profile" if matched
    pub schema: String,                // target SQL schema, e.g. "cardholder"
    pub table_name: String,            // target table, e.g. "cpf_Cardholder"
    pub prefix: String,                // 3-letter prefix, e.g. "cpf"
    /// Score the borrow heuristic gave — 0 means "no borrow, fresh table".
    pub score: u32,
    /// Human-readable reason, written to erd-borrows.md.
    pub reason: String,
}
