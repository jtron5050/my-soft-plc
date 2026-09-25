//! Recursive-descent parser for Appendix B ST-subset.

use crate::ast::*;
use crate::error::{CompileError, ErrorCode, Span};
use crate::lex::{lex, Token, TokenKind};

/// Parse ST source into a compilation unit.
pub fn parse(source: &str) -> Result<CompilationUnit, CompileError> {
    let tokens = lex(source)?;
    let mut p = Parser {
        tokens: &tokens,
        i: 0,
    };
    p.parse_unit()
}

struct Parser<'a> {
    tokens: &'a [Token],
    i: usize,
}

impl<'a> Parser<'a> {
    fn peek(&self) -> &Token {
        &self.tokens[self.i]
    }

    fn peek_kind(&self) -> &TokenKind {
        &self.peek().kind
    }

    fn bump(&mut self) -> &Token {
        let t = &self.tokens[self.i];
        if self.i + 1 < self.tokens.len() {
            self.i += 1;
        }
        t
    }

    fn expect_kw(&mut self, kw: &'static str) -> Result<Span, CompileError> {
        let t = self.peek().clone();
        match t.kind {
            TokenKind::Keyword(k) if k == kw => {
                self.bump();
                Ok(t.span)
            }
            _ => Err(self.err(
                ErrorCode::EParse,
                format!("expected keyword {kw}, got {}", describe(&t)),
            )),
        }
    }

    fn expect(&mut self, kind: TokenKind) -> Result<Token, CompileError> {
        let t = self.peek().clone();
        if kinds_eq(&t.kind, &kind) {
            self.bump();
            Ok(t)
        } else {
            Err(self.err(
                ErrorCode::EParse,
                format!("expected {kind:?}, got {}", describe(&t)),
            ))
        }
    }

    fn expect_ident(&mut self) -> Result<(String, Span), CompileError> {
        let t = self.peek().clone();
        match t.kind {
            TokenKind::Ident => {
                self.bump();
                Ok((t.text, t.span))
            }
            // Allow elementary type names as idents in some positions via separate helpers.
            _ => Err(self.err(
                ErrorCode::EParse,
                format!("expected identifier, got {}", describe(&t)),
            )),
        }
    }

    fn err(&self, code: ErrorCode, message: impl Into<String>) -> CompileError {
        CompileError::new(code, message).with_span(self.peek().span)
    }

    fn excluded_kw(&self, code: ErrorCode, kw: &str) -> CompileError {
        CompileError::new(code, format!("{kw} is excluded by Appendix B ST-subset"))
            .with_span(self.peek().span)
    }

    fn check_excluded_keyword(&self) -> Result<(), CompileError> {
        if let TokenKind::Keyword(kw) = self.peek_kind() {
            match *kw {
                "VAR_IN_OUT" => return Err(self.excluded_kw(ErrorCode::EExcludedVarInOut, kw)),
                "VAR_EXTERNAL" => return Err(self.excluded_kw(ErrorCode::EExcluded, kw)),
                "REF_TO" | "ADR" => return Err(self.excluded_kw(ErrorCode::EExcludedPointer, kw)),
                "STRING" | "WSTRING" => {
                    return Err(self.excluded_kw(ErrorCode::EExcludedString, kw))
                }
                "REPEAT" | "UNTIL" | "END_REPEAT" | "EXIT" | "CONTINUE" => {
                    return Err(self.excluded_kw(ErrorCode::EExcludedLoopCtrl, kw))
                }
                "METHOD" | "CLASS" | "INTERFACE" => {
                    return Err(self.excluded_kw(ErrorCode::EExcludedOop, kw))
                }
                "CONFIGURATION" | "RESOURCE" => {
                    return Err(self.excluded_kw(ErrorCode::EExcludedConfig, kw))
                }
                "LINT" | "LREAL" => return Err(self.excluded_kw(ErrorCode::EExcluded, kw)),
                _ => {}
            }
        }
        Ok(())
    }

    fn parse_unit(&mut self) -> Result<CompilationUnit, CompileError> {
        let mut decls = Vec::new();
        while !matches!(self.peek_kind(), TokenKind::Eof) {
            self.check_excluded_keyword()?;
            match self.peek_kind() {
                TokenKind::Keyword("PROGRAM") => decls.push(Decl::Program(self.parse_program()?)),
                TokenKind::Keyword("FUNCTION_BLOCK") => {
                    decls.push(Decl::FunctionBlock(self.parse_fb()?))
                }
                _ => {
                    return Err(self.err(
                        ErrorCode::EParse,
                        format!(
                            "expected PROGRAM or FUNCTION_BLOCK, got {}",
                            describe(self.peek())
                        ),
                    ));
                }
            }
        }
        Ok(CompilationUnit { decls })
    }

    fn parse_program(&mut self) -> Result<Program, CompileError> {
        let start = self.expect_kw("PROGRAM")?;
        let (name, name_span) = self.expect_ident()?;
        let mut vars = Vec::new();
        let mut body = Vec::new();
        loop {
            self.check_excluded_keyword()?;
            match self.peek_kind() {
                TokenKind::Keyword("VAR")
                | TokenKind::Keyword("VAR_INPUT")
                | TokenKind::Keyword("VAR_OUTPUT")
                | TokenKind::Keyword("VAR_RETAIN")
                | TokenKind::Keyword("VAR_GLOBAL") => vars.push(self.parse_var_section()?),
                TokenKind::Keyword("END_PROGRAM") => {
                    let end = self.expect_kw("END_PROGRAM")?;
                    return Ok(Program {
                        name,
                        name_span,
                        vars,
                        body,
                        span: Span::new(start.start, end.end),
                    });
                }
                TokenKind::Keyword("FUNCTION_BLOCK") => {
                    return Err(self.excluded_kw(
                        ErrorCode::EExcludedNestedFbType,
                        "FUNCTION_BLOCK nested inside PROGRAM/FB",
                    ));
                }
                _ => body.push(self.parse_stmt()?),
            }
        }
    }

    fn parse_fb(&mut self) -> Result<FunctionBlock, CompileError> {
        let start = self.expect_kw("FUNCTION_BLOCK")?;
        let (name, name_span) = self.expect_ident()?;
        let mut vars = Vec::new();
        let mut body = Vec::new();
        loop {
            self.check_excluded_keyword()?;
            match self.peek_kind() {
                TokenKind::Keyword("VAR")
                | TokenKind::Keyword("VAR_INPUT")
                | TokenKind::Keyword("VAR_OUTPUT")
                | TokenKind::Keyword("VAR_RETAIN")
                | TokenKind::Keyword("VAR_GLOBAL") => vars.push(self.parse_var_section()?),
                TokenKind::Keyword("END_FUNCTION_BLOCK") => {
                    let end = self.expect_kw("END_FUNCTION_BLOCK")?;
                    return Ok(FunctionBlock {
                        name,
                        name_span,
                        vars,
                        body,
                        span: Span::new(start.start, end.end),
                    });
                }
                TokenKind::Keyword("FUNCTION_BLOCK") => {
                    return Err(self.excluded_kw(
                        ErrorCode::EExcludedNestedFbType,
                        "nested FUNCTION_BLOCK type definition",
                    ));
                }
                _ => body.push(self.parse_stmt()?),
            }
        }
    }

    fn parse_var_section(&mut self) -> Result<VarSection, CompileError> {
        let t = self.peek().clone();
        let (kind, start) = match t.kind {
            TokenKind::Keyword("VAR_INPUT") => {
                self.bump();
                (VarKind::Input, t.span)
            }
            TokenKind::Keyword("VAR_OUTPUT") => {
                self.bump();
                (VarKind::Output, t.span)
            }
            TokenKind::Keyword("VAR_RETAIN") => {
                self.bump();
                (VarKind::Retain, t.span)
            }
            TokenKind::Keyword("VAR_GLOBAL") => {
                self.bump();
                let kind = if matches!(self.peek_kind(), TokenKind::Keyword("CONSTANT")) {
                    self.bump();
                    VarKind::Constant
                } else {
                    VarKind::Global
                };
                (kind, t.span)
            }
            TokenKind::Keyword("VAR") => {
                self.bump();
                let kind = if matches!(self.peek_kind(), TokenKind::Keyword("CONSTANT")) {
                    self.bump();
                    VarKind::Constant
                } else {
                    VarKind::Var
                };
                (kind, t.span)
            }
            TokenKind::Keyword("VAR_IN_OUT") => {
                return Err(self.excluded_kw(ErrorCode::EExcludedVarInOut, "VAR_IN_OUT"));
            }
            _ => {
                return Err(self.err(ErrorCode::EParse, "expected VAR section"));
            }
        };
        let mut vars = Vec::new();
        while !matches!(self.peek_kind(), TokenKind::Keyword("END_VAR")) {
            self.check_excluded_keyword()?;
            vars.push(self.parse_var_decl()?);
        }
        let end = self.expect_kw("END_VAR")?;
        Ok(VarSection {
            kind,
            vars,
            span: Span::new(start.start, end.end),
        })
    }

    fn parse_var_decl(&mut self) -> Result<VarDecl, CompileError> {
        let (name, name_span) = self.expect_ident()?;
        let mut at = None;
        if matches!(self.peek_kind(), TokenKind::Keyword("AT")) {
            self.bump();
            let addr_tok = self.peek().clone();
            if addr_tok.kind != TokenKind::DirectAddr {
                return Err(self.err(ErrorCode::EParse, "expected direct address after AT"));
            }
            self.bump();
            at = Some(parse_direct_addr(&addr_tok)?);
        }
        self.expect(TokenKind::Colon)?;
        let ty = self.parse_type()?;
        let mut init = None;
        if self.peek_kind() == &TokenKind::Assign {
            self.bump();
            init = Some(self.parse_expr()?);
        }
        let semi = self.expect(TokenKind::Semi)?;
        Ok(VarDecl {
            name,
            name_span,
            at,
            ty,
            init,
            span: Span::new(name_span.start, semi.span.end),
        })
    }

    fn parse_type(&mut self) -> Result<TypeExpr, CompileError> {
        self.check_excluded_keyword()?;
        if matches!(self.peek_kind(), TokenKind::Keyword("ARRAY")) {
            let start = self.expect_kw("ARRAY")?;
            self.expect(TokenKind::LBracket)?;
            let lo = self.expect(TokenKind::IntLit)?;
            let lo_v: i64 = lo
                .text
                .parse()
                .map_err(|_| self.err(ErrorCode::EParse, "bad array lower bound"))?;
            if lo_v != 0 {
                return Err(self.err(ErrorCode::EParse, "ARRAY lower bound must be 0 in v1"));
            }
            self.expect(TokenKind::DotDot)?;
            let hi = self.expect(TokenKind::IntLit)?;
            let hi_v: u32 = hi.text.parse().map_err(|_| {
                CompileError::new(ErrorCode::EParse, "bad array upper bound").with_span(hi.span)
            })?;
            self.expect(TokenKind::RBracket)?;
            self.expect_kw("OF")?;
            let elem = self.parse_type()?;
            let end = elem.span();
            return Ok(TypeExpr::Array {
                upper: hi_v,
                elem: Box::new(elem),
                span: Span::new(start.start, end.end),
            });
        }
        // Named type: Ident or elementary keyword used as type
        let t = self.peek().clone();
        let (name, span) = match t.kind {
            TokenKind::Ident => {
                self.bump();
                (t.text, t.span)
            }
            TokenKind::Keyword(kw)
                if matches!(
                    kw,
                    "BOOL"
                        | "INT"
                        | "DINT"
                        | "REAL"
                        | "TIME"
                        | "TON"
                        | "TOF"
                        | "TP"
                        | "CTU"
                        | "CTD"
                        | "RS"
                        | "SR"
                        | "PID"
                ) =>
            {
                self.bump();
                (kw.to_string(), t.span)
            }
            TokenKind::Keyword("STRING") | TokenKind::Keyword("WSTRING") => {
                return Err(self.excluded_kw(ErrorCode::EExcludedString, "STRING"));
            }
            TokenKind::Keyword("LINT") | TokenKind::Keyword("LREAL") => {
                return Err(self.excluded_kw(ErrorCode::EExcluded, "LINT/LREAL"));
            }
            TokenKind::Keyword("REF_TO") => {
                return Err(self.excluded_kw(ErrorCode::EExcludedPointer, "REF_TO"));
            }
            _ => {
                return Err(self.err(
                    ErrorCode::EParse,
                    format!("expected type name, got {}", describe(&t)),
                ));
            }
        };
        Ok(TypeExpr::Named { name, span })
    }

    fn parse_stmt(&mut self) -> Result<Stmt, CompileError> {
        self.check_excluded_keyword()?;
        // Optional attribute before WHILE/FOR
        let mut max_iter_attr = None;
        if self.peek_kind() == &TokenKind::LBrace {
            max_iter_attr = Some(self.parse_max_iter_attr()?);
        }
        match self.peek_kind() {
            TokenKind::Keyword("IF") => self.parse_if(),
            TokenKind::Keyword("CASE") => self.parse_case(),
            TokenKind::Keyword("WHILE") => self.parse_while(max_iter_attr),
            TokenKind::Keyword("FOR") => self.parse_for(max_iter_attr),
            TokenKind::Keyword("RETURN") => {
                let start = self.expect_kw("RETURN")?;
                let end = self.expect(TokenKind::Semi)?;
                Ok(Stmt::Return {
                    span: Span::new(start.start, end.span.end),
                })
            }
            TokenKind::Semi => {
                let t = self.bump().clone();
                Ok(Stmt::Empty { span: t.span })
            }
            _ => {
                if max_iter_attr.is_some() {
                    return Err(self.err(
                        ErrorCode::EParse,
                        "{ max_iter := … } only valid before WHILE/FOR",
                    ));
                }
                // Assign or FB call statement
                let expr = self.parse_expr()?;
                if self.peek_kind() == &TokenKind::Assign {
                    self.bump();
                    let rhs = self.parse_expr()?;
                    let end = self.expect(TokenKind::Semi)?;
                    let start = expr.span().start;
                    Ok(Stmt::Assign {
                        lhs: expr,
                        rhs,
                        span: Span::new(start, end.span.end),
                    })
                } else if let Expr::FbCall { call, span } = expr {
                    let end = self.expect(TokenKind::Semi)?;
                    Ok(Stmt::FbCall {
                        call,
                        span: Span::new(span.start, end.span.end),
                    })
                } else if self.peek_kind() == &TokenKind::LParen {
                    // name( … );  FB call
                    let call = self.finish_fb_call(expr)?;
                    let end = self.expect(TokenKind::Semi)?;
                    Ok(Stmt::FbCall {
                        call: call.clone(),
                        span: Span::new(call.callee_span.start, end.span.end),
                    })
                } else {
                    Err(self.err(
                        ErrorCode::EParse,
                        "expected assignment or FB call statement",
                    ))
                }
            }
        }
    }

    fn parse_max_iter_attr(&mut self) -> Result<u32, CompileError> {
        self.expect(TokenKind::LBrace)?;
        // max_iter := N
        let id = self.expect_ident()?;
        if !id.0.eq_ignore_ascii_case("max_iter") {
            return Err(self.err(ErrorCode::EParse, "only max_iter attribute supported"));
        }
        self.expect(TokenKind::Assign)?;
        let n = self.expect(TokenKind::IntLit)?;
        let v: u32 = n
            .text
            .parse()
            .map_err(|_| CompileError::new(ErrorCode::EParse, "bad max_iter").with_span(n.span))?;
        self.expect(TokenKind::RBrace)?;
        Ok(v)
    }

    fn parse_if(&mut self) -> Result<Stmt, CompileError> {
        let start = self.expect_kw("IF")?;
        let mut branches = Vec::new();
        let cond = self.parse_expr()?;
        self.expect_kw("THEN")?;
        let body = self.parse_stmt_list(&[
            TokenKind::Keyword("ELSIF"),
            TokenKind::Keyword("ELSE"),
            TokenKind::Keyword("END_IF"),
        ])?;
        branches.push((cond, body));
        while matches!(self.peek_kind(), TokenKind::Keyword("ELSIF")) {
            self.bump();
            let c = self.parse_expr()?;
            self.expect_kw("THEN")?;
            let b = self.parse_stmt_list(&[
                TokenKind::Keyword("ELSIF"),
                TokenKind::Keyword("ELSE"),
                TokenKind::Keyword("END_IF"),
            ])?;
            branches.push((c, b));
        }
        let else_body = if matches!(self.peek_kind(), TokenKind::Keyword("ELSE")) {
            self.bump();
            Some(self.parse_stmt_list(&[TokenKind::Keyword("END_IF")])?)
        } else {
            None
        };
        let end = self.expect_kw("END_IF")?;
        // Optional semicolon after END_IF
        if self.peek_kind() == &TokenKind::Semi {
            self.bump();
        }
        Ok(Stmt::If {
            branches,
            else_body,
            span: Span::new(start.start, end.end),
        })
    }

    fn parse_case(&mut self) -> Result<Stmt, CompileError> {
        let start = self.expect_kw("CASE")?;
        let selector = self.parse_expr()?;
        self.expect_kw("OF")?;
        let mut arms = Vec::new();
        let mut else_body = None;
        loop {
            match self.peek_kind() {
                TokenKind::Keyword("END_CASE") => break,
                TokenKind::Keyword("ELSE") => {
                    self.bump();
                    else_body = Some(self.parse_stmt_list(&[TokenKind::Keyword("END_CASE")])?);
                    break;
                }
                _ => {
                    let mut labels = Vec::new();
                    loop {
                        let lit = self.expect(TokenKind::IntLit)?;
                        let v: i64 = lit.text.parse().map_err(|_| {
                            CompileError::new(ErrorCode::EParse, "bad CASE label")
                                .with_span(lit.span)
                        })?;
                        if self.peek_kind() == &TokenKind::DotDot {
                            self.bump();
                            let hi = self.expect(TokenKind::IntLit)?;
                            let hv: i64 = hi.text.parse().map_err(|_| {
                                CompileError::new(ErrorCode::EParse, "bad CASE range")
                                    .with_span(hi.span)
                            })?;
                            if hv < v {
                                return Err(self.err(ErrorCode::EParse, "CASE range high < low"));
                            }
                            for x in v..=hv {
                                labels.push(x);
                            }
                        } else {
                            labels.push(v);
                        }
                        if self.peek_kind() == &TokenKind::Comma {
                            self.bump();
                            continue;
                        }
                        break;
                    }
                    self.expect(TokenKind::Colon)?;
                    let body = self.parse_stmt_list(&[
                        TokenKind::IntLit,
                        TokenKind::Keyword("ELSE"),
                        TokenKind::Keyword("END_CASE"),
                    ])?;
                    // Heuristic: next arm starts with IntLit or ELSE/END_CASE
                    arms.push(CaseArm { labels, body });
                }
            }
        }
        let end = self.expect_kw("END_CASE")?;
        if self.peek_kind() == &TokenKind::Semi {
            self.bump();
        }
        Ok(Stmt::Case {
            selector,
            arms,
            else_body,
            span: Span::new(start.start, end.end),
        })
    }

    fn parse_while(&mut self, max_iter: Option<u32>) -> Result<Stmt, CompileError> {
        let start = self.expect_kw("WHILE")?;
        let cond = self.parse_expr()?;
        self.expect_kw("DO")?;
        let body = self.parse_stmt_list(&[TokenKind::Keyword("END_WHILE")])?;
        let end = self.expect_kw("END_WHILE")?;
        if self.peek_kind() == &TokenKind::Semi {
            self.bump();
        }
        Ok(Stmt::While {
            cond,
            max_iter,
            body,
            span: Span::new(start.start, end.end),
        })
    }

    fn parse_for(&mut self, max_iter: Option<u32>) -> Result<Stmt, CompileError> {
        let start = self.expect_kw("FOR")?;
        let (var, var_span) = self.expect_ident()?;
        self.expect(TokenKind::Assign)?;
        let from = self.parse_expr()?;
        self.expect_kw("TO")?;
        let to = self.parse_expr()?;
        let by = if matches!(self.peek_kind(), TokenKind::Keyword("BY")) {
            self.bump();
            Some(self.parse_expr()?)
        } else {
            None
        };
        self.expect_kw("DO")?;
        let body = self.parse_stmt_list(&[TokenKind::Keyword("END_FOR")])?;
        let end = self.expect_kw("END_FOR")?;
        if self.peek_kind() == &TokenKind::Semi {
            self.bump();
        }
        Ok(Stmt::For {
            var,
            var_span,
            from,
            to,
            by,
            max_iter,
            body,
            span: Span::new(start.start, end.end),
        })
    }

    fn parse_stmt_list(&mut self, stop: &[TokenKind]) -> Result<Vec<Stmt>, CompileError> {
        let mut body = Vec::new();
        while !matches!(self.peek_kind(), TokenKind::Eof)
            && !stop.iter().any(|s| kinds_eq(self.peek_kind(), s))
        {
            // CASE arms: stop when we see an integer label at start of line-ish —
            // handled by stop containing IntLit; but statements can start with IntLit
            // only as expression — if stop has IntLit and peek is IntLit followed by
            // colon/comma/dotdot, it's a new arm.
            if stop.iter().any(|s| matches!(s, TokenKind::IntLit))
                && matches!(self.peek_kind(), TokenKind::IntLit)
            {
                // Lookahead: CASE label form
                let save = self.i;
                let _ = self.bump();
                let is_label = matches!(
                    self.peek_kind(),
                    TokenKind::Colon | TokenKind::Comma | TokenKind::DotDot
                );
                self.i = save;
                if is_label {
                    break;
                }
            }
            body.push(self.parse_stmt()?);
        }
        Ok(body)
    }

    fn finish_fb_call(&mut self, callee_expr: Expr) -> Result<FbCallExpr, CompileError> {
        let (callee, callee_span) = match callee_expr {
            Expr::Name { path, span } => (path, span),
            other => {
                return Err(
                    CompileError::new(ErrorCode::EParse, "FB call callee must be a name")
                        .with_span(other.span()),
                );
            }
        };
        self.expect(TokenKind::LParen)?;
        let mut inputs = Vec::new();
        let mut outputs = Vec::new();
        if self.peek_kind() != &TokenKind::RParen {
            loop {
                // Could be OUT => dest  or NAME := expr  or positional expr
                // Lookahead for ident :=  or ident =>
                if matches!(self.peek_kind(), TokenKind::Ident) && self.i + 1 < self.tokens.len() {
                    let next = &self.tokens[self.i + 1].kind;
                    if *next == TokenKind::Assign {
                        let name = self.bump().text.clone();
                        self.bump(); // :=
                        let value = self.parse_expr()?;
                        inputs.push(CallArg::Named { name, value });
                    } else if *next == TokenKind::Arrow {
                        let name = self.bump().text.clone();
                        self.bump(); // =>
                        let dest = self.parse_expr()?;
                        outputs.push(CallOut { name, dest });
                    } else {
                        inputs.push(CallArg::Positional(self.parse_expr()?));
                    }
                } else {
                    inputs.push(CallArg::Positional(self.parse_expr()?));
                }
                if self.peek_kind() == &TokenKind::Comma {
                    self.bump();
                    continue;
                }
                break;
            }
        }
        let end = self.expect(TokenKind::RParen)?;
        Ok(FbCallExpr {
            callee,
            callee_span: Span::new(callee_span.start, end.span.end),
            inputs,
            outputs,
        })
    }

    // --- expressions (precedence climbing) ---

    fn parse_expr(&mut self) -> Result<Expr, CompileError> {
        self.parse_or()
    }

    fn parse_or(&mut self) -> Result<Expr, CompileError> {
        let mut left = self.parse_and()?;
        while matches!(self.peek_kind(), TokenKind::Keyword("OR")) {
            self.bump();
            let right = self.parse_and()?;
            let span = Span::new(left.span().start, right.span().end);
            left = Expr::Binary {
                op: BinaryOp::Or,
                left: Box::new(left),
                right: Box::new(right),
                span,
            };
        }
        Ok(left)
    }

    fn parse_and(&mut self) -> Result<Expr, CompileError> {
        let mut left = self.parse_xor()?;
        while matches!(self.peek_kind(), TokenKind::Keyword("AND")) {
            self.bump();
            let right = self.parse_xor()?;
            let span = Span::new(left.span().start, right.span().end);
            left = Expr::Binary {
                op: BinaryOp::And,
                left: Box::new(left),
                right: Box::new(right),
                span,
            };
        }
        Ok(left)
    }

    fn parse_xor(&mut self) -> Result<Expr, CompileError> {
        let mut left = self.parse_compare()?;
        while matches!(self.peek_kind(), TokenKind::Keyword("XOR")) {
            self.bump();
            let right = self.parse_compare()?;
            let span = Span::new(left.span().start, right.span().end);
            left = Expr::Binary {
                op: BinaryOp::Xor,
                left: Box::new(left),
                right: Box::new(right),
                span,
            };
        }
        Ok(left)
    }

    fn parse_compare(&mut self) -> Result<Expr, CompileError> {
        let mut left = self.parse_add()?;
        loop {
            let op = match self.peek_kind() {
                TokenKind::Eq => BinaryOp::Eq,
                TokenKind::Ne => BinaryOp::Ne,
                TokenKind::Lt => BinaryOp::Lt,
                TokenKind::Le => BinaryOp::Le,
                TokenKind::Gt => BinaryOp::Gt,
                TokenKind::Ge => BinaryOp::Ge,
                _ => break,
            };
            self.bump();
            let right = self.parse_add()?;
            let span = Span::new(left.span().start, right.span().end);
            left = Expr::Binary {
                op,
                left: Box::new(left),
                right: Box::new(right),
                span,
            };
        }
        Ok(left)
    }

    fn parse_add(&mut self) -> Result<Expr, CompileError> {
        let mut left = self.parse_mul()?;
        loop {
            let op = match self.peek_kind() {
                TokenKind::Plus => BinaryOp::Add,
                TokenKind::Minus => BinaryOp::Sub,
                _ => break,
            };
            self.bump();
            let right = self.parse_mul()?;
            let span = Span::new(left.span().start, right.span().end);
            left = Expr::Binary {
                op,
                left: Box::new(left),
                right: Box::new(right),
                span,
            };
        }
        Ok(left)
    }

    fn parse_mul(&mut self) -> Result<Expr, CompileError> {
        let mut left = self.parse_unary()?;
        loop {
            let op = match self.peek_kind() {
                TokenKind::Star => BinaryOp::Mul,
                TokenKind::Slash => BinaryOp::Div,
                TokenKind::Keyword("MOD") => BinaryOp::Mod,
                _ => break,
            };
            self.bump();
            let right = self.parse_unary()?;
            let span = Span::new(left.span().start, right.span().end);
            left = Expr::Binary {
                op,
                left: Box::new(left),
                right: Box::new(right),
                span,
            };
        }
        Ok(left)
    }

    fn parse_unary(&mut self) -> Result<Expr, CompileError> {
        if matches!(self.peek_kind(), TokenKind::Keyword("NOT")) {
            let start = self.bump().span;
            let expr = self.parse_unary()?;
            return Ok(Expr::Unary {
                op: UnaryOp::Not,
                span: Span::new(start.start, expr.span().end),
                expr: Box::new(expr),
            });
        }
        if self.peek_kind() == &TokenKind::Minus {
            let start = self.bump().span;
            let expr = self.parse_unary()?;
            return Ok(Expr::Unary {
                op: UnaryOp::Neg,
                span: Span::new(start.start, expr.span().end),
                expr: Box::new(expr),
            });
        }
        self.parse_postfix()
    }

    fn parse_postfix(&mut self) -> Result<Expr, CompileError> {
        let mut expr = self.parse_primary()?;
        loop {
            match self.peek_kind() {
                TokenKind::Dot => {
                    self.bump();
                    let (name, span) = match self.peek().clone() {
                        Token {
                            kind: TokenKind::Ident,
                            text,
                            span,
                        } => {
                            self.bump();
                            (text, span)
                        }
                        Token {
                            kind: TokenKind::Keyword(kw),
                            span,
                            ..
                        } => {
                            self.bump();
                            (kw.to_string(), span)
                        }
                        _ => {
                            return Err(self.err(ErrorCode::EParse, "expected field name after '.'"))
                        }
                    };
                    // Extend name path
                    match &mut expr {
                        Expr::Name { path, span: s } => {
                            path.push(PathSeg::Ident(name));
                            s.end = span.end;
                        }
                        _ => {
                            return Err(CompileError::new(
                                ErrorCode::EParse,
                                "field access on non-name",
                            )
                            .with_span(span));
                        }
                    }
                }
                TokenKind::LBracket => {
                    self.bump();
                    let idx = self.parse_expr()?;
                    let end = self.expect(TokenKind::RBracket)?;
                    match &mut expr {
                        Expr::Name { path, span: s } => {
                            path.push(PathSeg::Index(idx));
                            s.end = end.span.end;
                        }
                        _ => {
                            return Err(CompileError::new(ErrorCode::EParse, "index on non-name")
                                .with_span(end.span));
                        }
                    }
                }
                TokenKind::LParen => {
                    // FB call expression
                    let call = self.finish_fb_call(expr)?;
                    expr = Expr::FbCall {
                        span: call.callee_span,
                        call,
                    };
                }
                _ => break,
            }
        }
        Ok(expr)
    }

    fn parse_primary(&mut self) -> Result<Expr, CompileError> {
        self.check_excluded_keyword()?;
        let t = self.peek().clone();
        match t.kind {
            TokenKind::Keyword("TRUE") => {
                self.bump();
                Ok(Expr::Literal {
                    value: Literal::Bool(true),
                    span: t.span,
                })
            }
            TokenKind::Keyword("FALSE") => {
                self.bump();
                Ok(Expr::Literal {
                    value: Literal::Bool(false),
                    span: t.span,
                })
            }
            TokenKind::IntLit => {
                self.bump();
                let v: i64 = t.text.parse().map_err(|_| {
                    CompileError::new(ErrorCode::EParse, "bad integer").with_span(t.span)
                })?;
                Ok(Expr::Literal {
                    value: Literal::Int(v),
                    span: t.span,
                })
            }
            TokenKind::RealLit => {
                self.bump();
                let v: f32 = t.text.parse().map_err(|_| {
                    CompileError::new(ErrorCode::EParse, "bad real").with_span(t.span)
                })?;
                Ok(Expr::Literal {
                    value: Literal::Real(v),
                    span: t.span,
                })
            }
            TokenKind::TimeLit => {
                self.bump();
                let v: i32 = t.text.parse().map_err(|_| {
                    CompileError::new(ErrorCode::EParse, "bad time").with_span(t.span)
                })?;
                Ok(Expr::Literal {
                    value: Literal::TimeMs(v),
                    span: t.span,
                })
            }
            TokenKind::Ident => {
                self.bump();
                // Built-in Q_GOOD(
                if t.text.eq_ignore_ascii_case("Q_GOOD") && self.peek_kind() == &TokenKind::LParen {
                    self.bump();
                    let inner = self.parse_expr()?;
                    let end = self.expect(TokenKind::RParen)?;
                    return Ok(Expr::QGood {
                        expr: Box::new(inner),
                        span: Span::new(t.span.start, end.span.end),
                    });
                }
                Ok(Expr::Name {
                    path: vec![PathSeg::Ident(t.text)],
                    span: t.span,
                })
            }
            TokenKind::LParen => {
                self.bump();
                let e = self.parse_expr()?;
                self.expect(TokenKind::RParen)?;
                Ok(e)
            }
            TokenKind::Keyword("EN") | TokenKind::Keyword("ENO") => {
                // Treat as identifier-like for associations; for primary, allow as name
                self.bump();
                Ok(Expr::Name {
                    path: vec![PathSeg::Ident(
                        if matches!(t.kind, TokenKind::Keyword("EN")) {
                            "EN".into()
                        } else {
                            "ENO".into()
                        },
                    )],
                    span: t.span,
                })
            }
            _ => Err(self.err(
                ErrorCode::EParse,
                format!("unexpected token in expression: {}", describe(&t)),
            )),
        }
    }
}

impl TypeExpr {
    fn span(&self) -> Span {
        match self {
            Self::Named { span, .. } | Self::Array { span, .. } => *span,
        }
    }
}

fn parse_direct_addr(tok: &Token) -> Result<DirectAddr, CompileError> {
    // text is "I:0"
    let (plane_s, idx_s) = tok
        .text
        .split_once(':')
        .ok_or_else(|| CompileError::new(ErrorCode::ELex, "bad direct addr").with_span(tok.span))?;
    let plane = match plane_s {
        "I" => DirectPlane::I,
        "Q" => DirectPlane::Q,
        "M" => DirectPlane::M,
        "R" => DirectPlane::R,
        _ => {
            return Err(CompileError::new(ErrorCode::ELex, "bad plane").with_span(tok.span));
        }
    };
    let index: u32 = idx_s
        .parse()
        .map_err(|_| CompileError::new(ErrorCode::ELex, "bad direct index").with_span(tok.span))?;
    Ok(DirectAddr {
        plane,
        index,
        span: tok.span,
    })
}

fn describe(t: &Token) -> String {
    if t.text.is_empty() {
        format!("{:?}", t.kind)
    } else {
        format!("{:?} `{}`", t.kind, t.text)
    }
}

fn kinds_eq(a: &TokenKind, b: &TokenKind) -> bool {
    match (a, b) {
        (TokenKind::Keyword(x), TokenKind::Keyword(y)) => x == y,
        _ => std::mem::discriminant(a) == std::mem::discriminant(b),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_simple_program() {
        let u = parse(
            r"
            PROGRAM Main
              VAR_GLOBAL
                i0 AT %I0 : BOOL;
                q0 AT %Q0 : BOOL;
              END_VAR
              q0 := i0 AND TRUE;
            END_PROGRAM
            ",
        )
        .unwrap();
        assert_eq!(u.decls.len(), 1);
    }

    #[test]
    fn reject_var_in_out() {
        let e = parse(
            r"
            FUNCTION_BLOCK F
              VAR_IN_OUT x : BOOL; END_VAR
            END_FUNCTION_BLOCK
            ",
        )
        .unwrap_err();
        assert_eq!(e.code, ErrorCode::EExcludedVarInOut);
    }
}
