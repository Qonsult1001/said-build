//! AST-based audit for C# Request / Response DTOs.
//!
//! Reads a deployed `.cs` DTO file via the shared tree-sitter grammar
//! registry in `sca-core::grammars` (same C# parser the brain uses for
//! `said sym`) and extracts a typed `DtoFile` describing every property.
//!
//! The audit then diffs the deployed `DtoFile` against the **expected**
//! shape derived from a Dev Spec body schema, reporting drift
//! semantically (property `CurrencyCode` of type `string` with
//! `[Required]`) rather than textually (whitespace, attribute order,
//! comment churn).
//!
//! This is the .NET-side audit pattern the user pointed to: .said already
//! ships a C# tree-sitter grammar; no Roslyn dependency is needed.

use std::path::Path;

use crate::dev_spec::schema::{cs_type_for, pascal_case, BodySchema};

#[derive(Debug, Clone, Default)]
pub struct DtoProperty {
    /// PascalCase property name as authored in the `.cs` file.
    pub name: String,
    /// Type text exactly as it appears (`Guid`, `Guid?`, `List<Guid>`,
    /// `decimal?`, `dynamic`, `string`, …).
    pub ty: String,
    /// Each `[Attribute]` text, in source order, including brackets:
    /// `["[Required]", "[JsonRequired]"]`.
    pub attributes: Vec<String>,
}

#[derive(Debug, Clone, Default)]
pub struct DtoFile {
    /// First `class XYZ` name found in the file. DTOs are one class
    /// per file by convention.
    pub class_name: String,
    /// Namespace declaration (file-scoped or block form, trimmed of
    /// `;` and braces).
    pub namespace: String,
    /// `[Attribute]` text on the class declaration (`[ExcludeFromCodeCoverage]`).
    pub class_attributes: Vec<String>,
    /// Property list in source order.
    pub properties: Vec<DtoProperty>,
}

/// Parse a `.cs` file into a `DtoFile` using the shared tree-sitter
/// C# grammar. Errors only on I/O or grammar failure — a file that
/// doesn't contain a class returns `DtoFile::default()`.
pub fn parse_dto_file(path: &Path) -> Result<DtoFile, String> {
    let source = std::fs::read_to_string(path)
        .map_err(|e| format!("read {}: {}", path.display(), e))?;
    parse_dto_text(&source)
}

/// Same as `parse_dto_file` but takes the source text directly. Useful
/// for tests and for diffing rendered-vs-deployed without writing the
/// rendered output to a temp file.
pub fn parse_dto_text(source: &str) -> Result<DtoFile, String> {
    use tree_sitter::Parser;

    let spec = sca_core::grammars::lookup_by_extension("cs")
        .ok_or_else(|| "C# tree-sitter grammar not registered".to_string())?;
    let language = (spec.language.0)();

    let mut parser = Parser::new();
    parser
        .set_language(&language)
        .map_err(|e| format!("set_language(c_sharp): {}", e))?;
    let tree = parser
        .parse(source, None)
        .ok_or_else(|| "tree-sitter parse returned None".to_string())?;

    let bytes = source.as_bytes();
    let mut out = DtoFile::default();
    walk(tree.root_node(), bytes, &mut out, /*in_class=*/ false);
    Ok(out)
}

fn walk(node: tree_sitter::Node, bytes: &[u8], out: &mut DtoFile, in_class: bool) {
    let kind = node.kind();

    match kind {
        "file_scoped_namespace_declaration" | "namespace_declaration" => {
            // Both forms: `namespace X.Y.Z;` and `namespace X.Y.Z { ... }`.
            if out.namespace.is_empty() {
                if let Some(name) = first_child_kinded(node, bytes, "qualified_name") {
                    out.namespace = name;
                } else if let Some(name) = first_child_kinded(node, bytes, "identifier") {
                    out.namespace = name;
                }
            }
            // Recurse so we still pick up the class inside a block-form
            // namespace.
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                walk(child, bytes, out, in_class);
            }
        }
        "class_declaration" => {
            if out.class_name.is_empty() {
                if let Some(name) = first_child_kinded(node, bytes, "identifier") {
                    out.class_name = name;
                }
                out.class_attributes = collect_attribute_lists(node, bytes);
            }
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                walk(child, bytes, out, /*in_class=*/ true);
            }
        }
        "property_declaration" if in_class => {
            if let Some(prop) = property_from_node(node, bytes) {
                out.properties.push(prop);
            }
        }
        _ => {
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                walk(child, bytes, out, in_class);
            }
        }
    }
}

fn property_from_node(node: tree_sitter::Node, bytes: &[u8]) -> Option<DtoProperty> {
    // C# tree-sitter property_declaration shape (observed):
    //   property_declaration
    //     attribute_list*           [Required], [JsonRequired]
    //     modifier                  public
    //     <type-node>               predefined_type | identifier | generic_name | nullable_type | array_type
    //     identifier                <name>      (always last identifier-shaped child before accessor_list)
    //     accessor_list             { get; set; }
    //
    // Custom user types (`Guid`, `MyEnum`, `dynamic`) come through as an
    // `identifier` child — so we cannot exclude `identifier` outright;
    // we have to pick the FIRST identifier as the type and the LAST as
    // the name. Predefined types (`string`, `int`) appear as
    // `predefined_type`, generics as `generic_name`, nullable value
    // types as `nullable_type`.

    // First pass: locate the type child and the name child.
    let mut type_node: Option<tree_sitter::Node> = None;
    let mut name_node: Option<tree_sitter::Node> = None;
    let mut cursor = node.walk();
    let children: Vec<_> = node.children(&mut cursor).collect();
    // Find accessor_list — the name is the identifier immediately
    // preceding it.
    let accessor_idx = children
        .iter()
        .position(|c| c.kind() == "accessor_list" || c.kind() == "arrow_expression_clause");
    let name_idx = if let Some(acc) = accessor_idx {
        // Walk backwards from accessor_list to find the last identifier.
        (0..acc).rev().find(|&i| children[i].kind() == "identifier")
    } else {
        // No accessor — last identifier wins.
        children
            .iter()
            .enumerate()
            .filter(|(_, c)| c.kind() == "identifier")
            .map(|(i, _)| i)
            .last()
    };
    if let Some(i) = name_idx {
        name_node = Some(children[i]);
    }
    // Type is the FIRST non-trivial type-shaped child between the
    // attribute/modifier preamble and the name.
    let stop = name_idx.unwrap_or(children.len());
    for (i, child) in children.iter().enumerate().take(stop) {
        let k = child.kind();
        if matches!(
            k,
            "attribute_list"
                | "modifier"
                | "static"
                | "public"
                | "private"
                | "protected"
                | "internal"
                | "abstract"
                | "virtual"
                | "override"
                | "readonly"
                | "{"
                | "}"
                | ";"
        ) {
            continue;
        }
        // Anything else here is the type.
        let _ = i;
        type_node = Some(*child);
        break;
    }

    let name = name_node
        .and_then(|n| std::str::from_utf8(&bytes[n.byte_range()]).ok())
        .map(|s| s.trim().to_string())?;
    let ty = type_node
        .and_then(|n| std::str::from_utf8(&bytes[n.byte_range()]).ok())
        .map(|s| s.trim().to_string())?;

    let attributes = collect_attribute_lists(node, bytes);

    Some(DtoProperty {
        name,
        ty,
        attributes,
    })
}

fn first_child_kinded(node: tree_sitter::Node, bytes: &[u8], kind: &str) -> Option<String> {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == kind {
            return std::str::from_utf8(&bytes[child.byte_range()])
                .ok()
                .map(|s| s.trim().to_string());
        }
    }
    None
}

fn collect_attribute_lists(node: tree_sitter::Node, bytes: &[u8]) -> Vec<String> {
    let mut out = Vec::new();
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "attribute_list" {
            if let Ok(s) = std::str::from_utf8(&bytes[child.byte_range()]) {
                out.push(s.trim().to_string());
            }
        }
    }
    out
}

// =========================================================================
// Diff
// =========================================================================

#[derive(Debug, Clone)]
pub enum DtoDrift {
    /// Property is in the deployed file but not in the Dev Spec.
    Extra { name: String, ty: String },
    /// Property is in the Dev Spec but not in the deployed file.
    Missing { name: String, expected_ty: String },
    /// Property exists on both sides but the C# type differs.
    TypeMismatch {
        name: String,
        deployed: String,
        expected: String,
    },
    /// Required field is missing the `[Required]` attribute (or vice
    /// versa). Optional in spec is silently allowed even when deployed
    /// adds `[Required]` — being more strict than the contract isn't
    /// drift, it's tightening.
    RequiredMismatch {
        name: String,
        deployed_has_required: bool,
        spec_required: bool,
    },
}

/// Which DTO kind we're auditing — controls the type-equivalence rules
/// (response DTOs use `ExpandoObject` instead of `dynamic`, allow
/// nullable value types even without `(required)`, never require
/// `[Required]`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DtoKind {
    Request,
    Response,
}

/// Diff a `DtoFile` (parsed from the deployed `.cs`) against the
/// canonical schema (parsed from the Dev Spec markdown), using
/// `DtoKind::Request` rules. Wrapper preserved for API compatibility.
pub fn diff(dto: &DtoFile, schema: &BodySchema) -> Vec<DtoDrift> {
    diff_kind(dto, schema, DtoKind::Request)
}

/// Diff variant that takes the DTO kind. Response DTOs:
///   - `dynamic` from request-side mapping is accepted as
///     `ExpandoObject` on deployed
///   - All-nullable value types — `Guid?` is OK where spec says `Guid`
///   - No `[Required]` enforcement — response fields are nullable by
///     convention
pub fn diff_kind(dto: &DtoFile, schema: &BodySchema, kind: DtoKind) -> Vec<DtoDrift> {
    let mut out = Vec::new();

    // Build name → (expected_ty, expected_required) from spec. For
    // response DTOs use the response type mapper (`ExpandoObject`,
    // nullable value types).
    let mut spec_map = std::collections::BTreeMap::<String, (String, bool)>::new();
    for f in &schema.fields {
        let ty = match kind {
            DtoKind::Request => cs_type_for(f),
            DtoKind::Response => response_cs_type_for(f),
        };
        spec_map.insert(pascal_case(&f.name), (ty, f.required));
    }

    // Deployed → spec.
    let mut deployed_names = std::collections::BTreeSet::<String>::new();
    for p in &dto.properties {
        deployed_names.insert(p.name.clone());
        let Some((expected_ty, spec_required)) = spec_map.get(&p.name) else {
            out.push(DtoDrift::Extra {
                name: p.name.clone(),
                ty: p.ty.clone(),
            });
            continue;
        };
        if !types_equivalent(&p.ty, expected_ty) {
            out.push(DtoDrift::TypeMismatch {
                name: p.name.clone(),
                deployed: p.ty.clone(),
                expected: expected_ty.clone(),
            });
        }
        if kind == DtoKind::Request {
            let has_required = p
                .attributes
                .iter()
                .any(|a| a.contains("Required") && !a.contains("JsonRequired"));
            if *spec_required && !has_required {
                out.push(DtoDrift::RequiredMismatch {
                    name: p.name.clone(),
                    deployed_has_required: has_required,
                    spec_required: *spec_required,
                });
            }
        }
    }

    // Spec → deployed (missing properties).
    for (name, (expected_ty, _)) in &spec_map {
        if !deployed_names.contains(name) {
            out.push(DtoDrift::Missing {
                name: name.clone(),
                expected_ty: expected_ty.clone(),
            });
        }
    }

    out
}

/// Type equivalence for audit purposes. Treats nullable-value-type
/// suffixes (`?`) as cosmetic — `Guid` and `Guid?` are equivalent when
/// the only difference is the trailing `?`. Reference types (`string`,
/// `dynamic`, `List<…>`) compare as-is. `dynamic` and `ExpandoObject`
/// are treated as equivalent — both mean "open-shape JSON object" and
/// the request vs response sides spell it differently per convention.
fn types_equivalent(a: &str, b: &str) -> bool {
    let na = a.trim_end_matches('?').trim();
    let nb = b.trim_end_matches('?').trim();
    if na == nb {
        return true;
    }
    let open_object = |t: &str| t == "dynamic" || t == "ExpandoObject" || t == "object";
    open_object(na) && open_object(nb)
}

/// Response-flavour C# type mapping — matches `cs_render::cs_type_for_response`
/// (`Type: object` → `ExpandoObject`, all value types nullable).
fn response_cs_type_for(field: &crate::dev_spec::schema::BodyField) -> String {
    let t = field.yaml_type.to_ascii_lowercase();
    let fmt = field.format.to_ascii_lowercase();
    let items_t = field.items_type.to_ascii_lowercase();
    let items_fmt = field.items_format.to_ascii_lowercase();

    if t == "array" {
        if items_fmt == "uuid" || items_t == "uuid" {
            return "List<Guid>".to_string();
        }
        if items_t == "string" {
            return "List<string>".to_string();
        }
        if items_t == "integer" {
            return "List<int>".to_string();
        }
        return "List<object>".to_string();
    }
    if t == "object" {
        return "ExpandoObject".to_string();
    }
    if t == "string" {
        if fmt == "uuid" {
            return "Guid?".to_string();
        }
        return "string".to_string();
    }
    if t == "integer" {
        return "int?".to_string();
    }
    if t == "number" {
        return "decimal?".to_string();
    }
    if t == "boolean" {
        return "bool?".to_string();
    }
    "string".to_string()
}

// =========================================================================
// Tests
// =========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_DTO: &str = r#"
namespace DirectTransact.TxnGlobal.API.Models.Request.Account;

[ExcludeFromCodeCoverage]
public class CreateAccountRequest
{
    /// <summary>Doc.</summary>
    public Guid? Id { get; set; }

    [Required]
    public string Name { get; set; }

    [Required]
    [JsonRequired]
    public Guid AccountOwnerId { get; set; }

    public List<Guid> AccountMembers { get; set; }

    public dynamic Attributes { get; set; }
}
"#;

    #[test]
    fn parses_namespace_class_and_properties() {
        let dto = parse_dto_text(SAMPLE_DTO).expect("parse");
        assert_eq!(dto.class_name, "CreateAccountRequest");
        assert!(dto
            .namespace
            .ends_with("Models.Request.Account"));
        assert_eq!(dto.class_attributes, vec!["[ExcludeFromCodeCoverage]"]);
        let names: Vec<&str> = dto.properties.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(
            names,
            vec!["Id", "Name", "AccountOwnerId", "AccountMembers", "Attributes"]
        );
        let tys: Vec<&str> = dto.properties.iter().map(|p| p.ty.as_str()).collect();
        assert_eq!(tys, vec!["Guid?", "string", "Guid", "List<Guid>", "dynamic"]);
        // Name has [Required], AccountOwnerId has [Required] + [JsonRequired].
        assert_eq!(dto.properties[1].attributes, vec!["[Required]"]);
        assert_eq!(
            dto.properties[2].attributes,
            vec!["[Required]", "[JsonRequired]"]
        );
    }

    #[test]
    fn diff_reports_missing_extra_and_type_mismatch() {
        use crate::dev_spec::schema::BodyField;
        let dto = parse_dto_text(SAMPLE_DTO).expect("parse");

        // Spec has: Id, Name (required), CurrencyCode (required), AccountOwnerId (required Guid).
        // → Extra: AccountMembers (in deployed, not spec)
        // → Extra: Attributes
        // → Missing: CurrencyCode
        // → TypeMismatch: nothing — types we share line up.
        let mut schema = BodySchema::default();
        schema.fields = vec![
            BodyField { name: "id".into(), yaml_type: "string".into(), format: "uuid".into(), ..Default::default() },
            BodyField { name: "name".into(), required: true, yaml_type: "string".into(), ..Default::default() },
            BodyField { name: "currencyCode".into(), required: true, yaml_type: "string".into(), ..Default::default() },
            BodyField { name: "accountOwnerId".into(), required: true, yaml_type: "string".into(), format: "uuid".into(), ..Default::default() },
        ];
        let drifts = diff(&dto, &schema);
        // Find each drift kind.
        let mut has_missing_currency = false;
        let mut has_extra_members = false;
        let mut has_extra_attributes = false;
        for d in &drifts {
            match d {
                DtoDrift::Missing { name, .. } if name == "CurrencyCode" => has_missing_currency = true,
                DtoDrift::Extra { name, .. } if name == "AccountMembers" => has_extra_members = true,
                DtoDrift::Extra { name, .. } if name == "Attributes" => has_extra_attributes = true,
                _ => {}
            }
        }
        assert!(has_missing_currency, "expected CurrencyCode missing");
        assert!(has_extra_members, "expected AccountMembers extra");
        assert!(has_extra_attributes, "expected Attributes extra");
    }
}
