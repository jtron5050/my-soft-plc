//! Compile errors with stable codes (Appendix B reject oracle).

use std::fmt;

/// Stable diagnostic codes for tests and tooling.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ErrorCode {
    /// Lexical error.
    ELex,
    /// Generic parse error.
    EParse,
    /// Construct excluded by Appendix B.
    EExcluded,
    /// `VAR_IN_OUT` excluded.
    EExcludedVarInOut,
    /// Pointers / `REF_TO` / `ADR` excluded.
    EExcludedPointer,
    /// `STRING` / `WSTRING` excluded.
    EExcludedString,
    /// `REPEAT` / `EXIT` / `CONTINUE` excluded.
    EExcludedLoopCtrl,
    /// Nested FB type definition excluded.
    EExcludedNestedFbType,
    /// OOP / methods / interfaces excluded.
    EExcludedOop,
    /// `CONFIGURATION` / `RESOURCE` excluded.
    EExcludedConfig,
    /// Unbounded `WHILE`/`FOR` without proven/`max_iter` bound.
    EUnboundedLoop,
    /// Recursive user-FB call graph.
    ERecursion,
    /// Undefined symbol.
    EUndefined,
    /// Type mismatch.
    EType,
    /// Duplicate definition.
    EDuplicate,
    /// Project / binding error.
    EProject,
    /// Layout / resource limit.
    ELayout,
    /// Codegen / verify failure.
    ECodegen,
    /// Package build failure.
    EPackage,
}

impl ErrorCode {
    /// Stable string form (`E_EXCLUDED_VAR_IN_OUT`, …).
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ELex => "E_LEX",
            Self::EParse => "E_PARSE",
            Self::EExcluded => "E_EXCLUDED",
            Self::EExcludedVarInOut => "E_EXCLUDED_VAR_IN_OUT",
            Self::EExcludedPointer => "E_EXCLUDED_POINTER",
            Self::EExcludedString => "E_EXCLUDED_STRING",
            Self::EExcludedLoopCtrl => "E_EXCLUDED_LOOP_CTRL",
            Self::EExcludedNestedFbType => "E_EXCLUDED_NESTED_FB_TYPE",
            Self::EExcludedOop => "E_EXCLUDED_OOP",
            Self::EExcludedConfig => "E_EXCLUDED_CONFIG",
            Self::EUnboundedLoop => "E_UNBOUNDED_LOOP",
            Self::ERecursion => "E_RECURSION",
            Self::EUndefined => "E_UNDEFINED",
            Self::EType => "E_TYPE",
            Self::EDuplicate => "E_DUPLICATE",
            Self::EProject => "E_PROJECT",
            Self::ELayout => "E_LAYOUT",
            Self::ECodegen => "E_CODEGEN",
            Self::EPackage => "E_PACKAGE",
        }
    }
}

impl fmt::Display for ErrorCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Byte offset span in a source file (UTF-8 byte indices).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Span {
    /// Inclusive start.
    pub start: usize,
    /// Exclusive end.
    pub end: usize,
}

impl Span {
    /// Empty span at `pos`.
    #[must_use]
    pub const fn at(pos: usize) -> Self {
        Self {
            start: pos,
            end: pos,
        }
    }

    /// Span covering `[start, end)`.
    #[must_use]
    pub const fn new(start: usize, end: usize) -> Self {
        Self { start, end }
    }
}

/// One compile diagnostic.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompileError {
    /// Stable code.
    pub code: ErrorCode,
    /// Human message.
    pub message: String,
    /// Optional source path.
    pub path: Option<String>,
    /// Optional span in that file.
    pub span: Option<Span>,
}

impl CompileError {
    /// Build an error with code and message.
    #[must_use]
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            path: None,
            span: None,
        }
    }

    /// Attach a file path.
    #[must_use]
    pub fn with_path(mut self, path: impl Into<String>) -> Self {
        self.path = Some(path.into());
        self
    }

    /// Attach a span.
    #[must_use]
    pub fn with_span(mut self, span: Span) -> Self {
        self.span = Some(span);
        self
    }
}

impl fmt::Display for CompileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.code, self.message)?;
        if let Some(path) = &self.path {
            write!(f, " ({path}")?;
            if let Some(span) = self.span {
                write!(f, ":{}..{}", span.start, span.end)?;
            }
            write!(f, ")")?;
        }
        Ok(())
    }
}

impl std::error::Error for CompileError {}

impl From<plc_ir::IrError> for CompileError {
    fn from(e: plc_ir::IrError) -> Self {
        Self::new(ErrorCode::ECodegen, e.to_string())
    }
}

impl From<plc_ir::VerifyError> for CompileError {
    fn from(e: plc_ir::VerifyError) -> Self {
        Self::new(ErrorCode::ECodegen, format!("IR verify failed: {e}"))
    }
}

impl From<plc_package::PackageError> for CompileError {
    fn from(e: plc_package::PackageError) -> Self {
        Self::new(ErrorCode::EPackage, e.to_string())
    }
}
