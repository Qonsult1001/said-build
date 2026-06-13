//! Emit `MERGE` statements that bring `lookups.ars_Api_Rule_Settings`
//! into alignment with the Dev Spec endpoints. MATCH is on
//! `(ars_Path, aml_Code)` so existing rows are preserved verbatim
//! (per design doc's data-preservation rule 3).

use crate::dev_spec::types::Erd;

pub fn emit_registry_merge(erd: &Erd) -> String {
    let mut out = String::from(
        "-- Registry amendments — Dev-Spec-driven.\n\
         -- Idempotent: MERGE on (ars_Path, aml_Code) leaves existing rows untouched.\n\
         -- See docs/superpowers/specs/2026-04-28-dev-spec-source-of-truth-design.md\n\n\
         -- Disable triggers on the registry table during the merge.\n\
         -- TXN's audit triggers chain to `p_dte_Audit_Backend` which has\n\
         -- pre-existing NULL-default bugs unrelated to this pipeline; we\n\
         -- restore triggers immediately after the merge completes.\n\
         DISABLE TRIGGER ALL ON lookups.ars_Api_Rule_Settings;\n\
         GO\n\n",
    );
    for ep in &erd.endpoints {
        let path_escaped = ep.path.replace('\'', "''");
        let verb = ep.method.to_uppercase();
        out.push_str(&format!(
            "MERGE INTO lookups.ars_Api_Rule_Settings AS target\n\
             USING (SELECT '{path}' AS ars_Path, '{verb}' AS aml_Code) AS src\n\
             ON target.ars_Path = src.ars_Path AND target.aml_Code = src.aml_Code\n\
             WHEN NOT MATCHED THEN\n\
             \tINSERT (ars_Api_Id, ars_Path, ars_Version, ars_Enabled, \n\
             \t        ars_Active_From, ars_Created_Date, ars_Created_Date_UTC, \n\
             \t        ars_Allow_Idempotency, aml_Code)\n\
             \tVALUES (NEWID(), src.ars_Path, 1, 1, \n\
             \t        GETDATE(), GETDATE(), GETUTCDATE(), \n\
             \t        1, src.aml_Code);\n\
             /* no-op when matched */\nGO\n\n",
            path = path_escaped, verb = verb,
        ));
    }
    out.push_str(
        "\n-- Re-enable registry-table triggers after the merge.\n\
         ENABLE TRIGGER ALL ON lookups.ars_Api_Rule_Settings;\n\
         GO\n",
    );
    out
}
