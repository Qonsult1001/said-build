//! Rebuilder submodules.

pub mod docx;
pub mod pdf;

/// Decoupled rebuild input — does not depend on the Store.
/// The vault orchestrator (T13) translates Store reads to RebuildRef
/// before invoking format-specific rebuilders.
#[derive(Debug, Clone)]
pub struct RebuildRef {
    /// Asset kind: "paragraph" | "image" | "font" | "xml"
    pub kind: String,
    /// Original zip entry path (e.g. "word/styles.xml") or None for
    /// non-named refs (e.g. raw paragraphs that the rebuilder will
    /// emit into a freshly-generated document.xml).
    pub name: Option<String>,
    /// Raw bytes — text for paragraph kind, binary for image/font,
    /// raw XML for xml kind.
    pub bytes: Vec<u8>,
}
