//! Appendix B ST AST.

use crate::error::Span;

/// Compilation unit: zero or more top-level PROGRAM / FUNCTION_BLOCK decls.
#[derive(Debug, Clone, PartialEq)]
pub struct CompilationUnit {
    /// Top-level declarations in source order.
    pub decls: Vec<Decl>,
}

/// Top-level declaration.
#[derive(Debug, Clone, PartialEq)]
pub enum Decl {
    /// `PROGRAM name … END_PROGRAM`
    Program(Program),
    /// `FUNCTION_BLOCK name … END_FUNCTION_BLOCK`
    FunctionBlock(FunctionBlock),
}

/// PROGRAM body (task entry).
#[derive(Debug, Clone, PartialEq)]
pub struct Program {
    /// Program name.
    pub name: String,
    /// Source span of the name.
    pub name_span: Span,
    /// Variable sections.
    pub vars: Vec<VarSection>,
    /// Statements.
    pub body: Vec<Stmt>,
    /// Full span.
    pub span: Span,
}

/// FUNCTION_BLOCK type definition.
#[derive(Debug, Clone, PartialEq)]
pub struct FunctionBlock {
    /// Type name.
    pub name: String,
    /// Name span.
    pub name_span: Span,
    /// Variable sections.
    pub vars: Vec<VarSection>,
    /// Statements.
    pub body: Vec<Stmt>,
    /// Full span.
    pub span: Span,
}

/// Variable section kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VarKind {
    /// `VAR`
    Var,
    /// `VAR_INPUT`
    Input,
    /// `VAR_OUTPUT`
    Output,
    /// `VAR_RETAIN`
    Retain,
    /// `VAR CONSTANT` / `VAR_GLOBAL CONSTANT`
    Constant,
    /// `VAR_GLOBAL` (non-constant)
    Global,
}

/// One `VAR*` … `END_VAR` section.
#[derive(Debug, Clone, PartialEq)]
pub struct VarSection {
    /// Section kind.
    pub kind: VarKind,
    /// Declarations.
    pub vars: Vec<VarDecl>,
    /// Span.
    pub span: Span,
}

/// Direct address plane.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DirectPlane {
    /// `%I`
    I,
    /// `%Q`
    Q,
    /// `%M`
    M,
    /// `%R`
    R,
}

/// `AT %I0` style binding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirectAddr {
    /// Plane.
    pub plane: DirectPlane,
    /// Slot / offset index.
    pub index: u32,
    /// Span.
    pub span: Span,
}

/// Type expression in a declaration.
#[derive(Debug, Clone, PartialEq)]
pub enum TypeExpr {
    /// Elementary or FB type name (`BOOL`, `TON`, `ConveyorDrive`).
    Named {
        /// Type name.
        name: String,
        /// Span.
        span: Span,
    },
    /// `ARRAY [0..N] OF T`
    Array {
        /// Upper bound N (inclusive); lower must be 0.
        upper: u32,
        /// Element type.
        elem: Box<TypeExpr>,
        /// Span.
        span: Span,
    },
}

/// One variable declaration.
#[derive(Debug, Clone, PartialEq)]
pub struct VarDecl {
    /// Name.
    pub name: String,
    /// Name span.
    pub name_span: Span,
    /// Optional `AT %…`.
    pub at: Option<DirectAddr>,
    /// Type.
    pub ty: TypeExpr,
    /// Optional init expression (`:= …`).
    pub init: Option<Expr>,
    /// Full span.
    pub span: Span,
}

/// Statement.
#[derive(Debug, Clone, PartialEq)]
pub enum Stmt {
    /// `lhs := rhs;`
    Assign {
        /// Left-hand side.
        lhs: Expr,
        /// Right-hand side.
        rhs: Expr,
        /// Span.
        span: Span,
    },
    /// FB / primitive call as a statement (optional output associations).
    FbCall {
        /// Instance or type call.
        call: FbCallExpr,
        /// Span.
        span: Span,
    },
    /// `IF … END_IF`
    If {
        /// Branches: (cond, body)* ; else is last Option body.
        branches: Vec<(Expr, Vec<Stmt>)>,
        /// Else body.
        else_body: Option<Vec<Stmt>>,
        /// Span.
        span: Span,
    },
    /// `CASE … END_CASE`
    Case {
        /// Selector.
        selector: Expr,
        /// Arms: labels + body.
        arms: Vec<CaseArm>,
        /// Else body.
        else_body: Option<Vec<Stmt>>,
        /// Span.
        span: Span,
    },
    /// `WHILE … END_WHILE`
    While {
        /// Condition.
        cond: Expr,
        /// Optional `{ max_iter := N }`
        max_iter: Option<u32>,
        /// Body.
        body: Vec<Stmt>,
        /// Span.
        span: Span,
    },
    /// `FOR i := a TO b BY c DO … END_FOR`
    For {
        /// Loop variable name.
        var: String,
        /// Var span.
        var_span: Span,
        /// Start.
        from: Expr,
        /// End (inclusive).
        to: Expr,
        /// Optional BY.
        by: Option<Expr>,
        /// Optional max_iter attribute.
        max_iter: Option<u32>,
        /// Body.
        body: Vec<Stmt>,
        /// Span.
        span: Span,
    },
    /// `RETURN;`
    Return {
        /// Span.
        span: Span,
    },
    /// Empty `;`
    Empty {
        /// Span.
        span: Span,
    },
}

/// CASE arm labels + body.
#[derive(Debug, Clone, PartialEq)]
pub struct CaseArm {
    /// One or more integer labels (ranges lowered by parser to flat list).
    pub labels: Vec<i64>,
    /// Body.
    pub body: Vec<Stmt>,
}

/// Expression.
#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    /// Boolean / numeric / time literal.
    Literal {
        /// Value.
        value: Literal,
        /// Span.
        span: Span,
    },
    /// Name or dotted path (`ton.Q`, `arr[i]`).
    Name {
        /// Path segments (name or index).
        path: Vec<PathSeg>,
        /// Span.
        span: Span,
    },
    /// Unary op.
    Unary {
        /// Operator.
        op: UnaryOp,
        /// Operand.
        expr: Box<Expr>,
        /// Span.
        span: Span,
    },
    /// Binary op.
    Binary {
        /// Operator.
        op: BinaryOp,
        /// Left.
        left: Box<Expr>,
        /// Right.
        right: Box<Expr>,
        /// Span.
        span: Span,
    },
    /// `Q_GOOD(name)`
    QGood {
        /// Input tag expression (must resolve to `%I`).
        expr: Box<Expr>,
        /// Span.
        span: Span,
    },
    /// Inline FB call expression (rare; usually a statement).
    FbCall {
        /// Call.
        call: FbCallExpr,
        /// Span.
        span: Span,
    },
}

/// Path segment.
#[derive(Debug, Clone, PartialEq)]
pub enum PathSeg {
    /// Identifier.
    Ident(String),
    /// `[index]` — index is constant or name for v1 (const preferred).
    Index(Expr),
}

/// Literal values.
#[derive(Debug, Clone, PartialEq)]
pub enum Literal {
    /// BOOL.
    Bool(bool),
    /// INT/DINT integer.
    Int(i64),
    /// REAL.
    Real(f32),
    /// TIME milliseconds.
    TimeMs(i32),
}

/// Unary operators.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnaryOp {
    /// `-`
    Neg,
    /// `NOT`
    Not,
}

/// Binary operators.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinaryOp {
    /// `+`
    Add,
    /// `-`
    Sub,
    /// `*`
    Mul,
    /// `/`
    Div,
    /// `MOD`
    Mod,
    /// `AND`
    And,
    /// `OR`
    Or,
    /// `XOR`
    Xor,
    /// `=`
    Eq,
    /// `<>`
    Ne,
    /// `<`
    Lt,
    /// `<=`
    Le,
    /// `>`
    Gt,
    /// `>=`
    Ge,
}

/// FB call: `inst(IN := …, Q => out)` or `inst(a, b)`.
#[derive(Debug, Clone, PartialEq)]
pub struct FbCallExpr {
    /// Callee path (instance name, possibly dotted).
    pub callee: Vec<PathSeg>,
    /// Callee span.
    pub callee_span: Span,
    /// Input associations / positional args.
    pub inputs: Vec<CallArg>,
    /// Output associations `Q => dest`.
    pub outputs: Vec<CallOut>,
}

/// Call input argument.
#[derive(Debug, Clone, PartialEq)]
pub enum CallArg {
    /// Positional.
    Positional(Expr),
    /// Named `IN := expr`
    Named {
        /// Parameter name.
        name: String,
        /// Value.
        value: Expr,
    },
}

/// Call output association.
#[derive(Debug, Clone, PartialEq)]
pub struct CallOut {
    /// Output parameter name.
    pub name: String,
    /// Destination expression.
    pub dest: Expr,
}

impl Expr {
    /// Span of this expression.
    #[must_use]
    pub fn span(&self) -> Span {
        match self {
            Self::Literal { span, .. }
            | Self::Name { span, .. }
            | Self::Unary { span, .. }
            | Self::Binary { span, .. }
            | Self::QGood { span, .. }
            | Self::FbCall { span, .. } => *span,
        }
    }
}
