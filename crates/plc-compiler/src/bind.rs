//! Symbol binding, layout, and lowering to IR.

use std::collections::{BTreeMap, HashMap, HashSet};

use plc_fb_primitives::PRIMITIVE_ABI;
use plc_ir::{
    encode_instruction, verify_module, DecodedInstr, EntryPoint, IrModule, IrType, Opcode,
    PrimitiveId, IR_MAJOR, IR_MINOR,
};
use plc_package::{IrTypeName, Manifest, ManifestRetainSymbol, RestartPolicy, TagEntry, TagKind};

use crate::ast::*;
use crate::error::{CompileError, ErrorCode, Span};
use crate::project::LoadedProject;

const MAX_ARRAY_ELEMS: u32 = 1024;
const MAX_LOOP_DEFAULT: u32 = 10_000;

#[derive(Debug, Clone)]
enum TypeDef {
    Elem(IrType),
    Array { elem: Box<TypeDef>, len: u32 },
    Prim(PrimitiveId),
    UserFb(String),
}

impl TypeDef {
    fn ir_elem(&self) -> Option<IrType> {
        match self {
            Self::Elem(t) => Some(*t),
            _ => None,
        }
    }

    fn byte_size(&self, fbs: &HashMap<String, UserFbLayout>) -> Result<u32, CompileError> {
        Ok(match self {
            Self::Elem(t) => t.byte_width() as u32,
            Self::Array { elem, len } => elem.byte_size(fbs)?.saturating_mul(*len),
            Self::Prim(p) => prim_shadow_size(*p),
            Self::UserFb(name) => fbs
                .get(&name.to_ascii_lowercase())
                .map(|l| l.total_data_size)
                .ok_or_else(|| {
                    CompileError::new(ErrorCode::EUndefined, format!("unknown FB type {name}"))
                })?,
        })
    }

    fn align(&self) -> u32 {
        match self {
            Self::Elem(IrType::Bool) => 1,
            Self::Elem(IrType::Int) => 2,
            Self::Elem(_) => 4,
            Self::Array { elem, .. } => elem.align(),
            Self::Prim(_) | Self::UserFb(_) => 4,
        }
    }
}

#[derive(Debug, Clone)]
struct FieldLayout {
    name: String,
    ty: TypeDef,
    offset: u32,
    retain: bool,
    retain_offset: Option<u32>,
}

#[derive(Debug, Clone)]
struct UserFbLayout {
    #[allow(dead_code)]
    name: String,
    inputs: Vec<String>,
    #[allow(dead_code)]
    outputs: Vec<String>,
    fields: Vec<FieldLayout>,
    total_data_size: u32,
    entry_name: String,
    body: Vec<Stmt>,
    /// Nested primitive / FB instance vars (name → type).
    instances: Vec<(String, TypeDef)>,
}

#[derive(Debug, Clone)]
enum Storage {
    /// Typed image slot.
    Input {
        slot: u32,
        ty: IrType,
    },
    Output {
        slot: u32,
        ty: IrType,
    },
    /// Data segment byte offset.
    Data {
        offset: u32,
        ty: TypeDef,
    },
    /// Retain segment.
    Retain {
        offset: u32,
        ty: IrType,
    },
    /// Primitive instance.
    Prim {
        id: PrimitiveId,
        instance: u32,
        /// Data offsets for outputs in declaration order.
        out_offsets: Vec<(String, u32, IrType)>,
    },
    /// User FB instance at data base.
    UserInst {
        fb: String,
        base: u32,
    },
    Const {
        #[allow(dead_code)]
        ty: IrType,
        value: Literal,
    },
}

#[derive(Debug, Clone)]
struct Symbol {
    storage: Storage,
    #[allow(dead_code)]
    span: Span,
}

struct Allocator {
    data: u32,
    retain: u32,
}

impl Allocator {
    fn alloc(&mut self, size: u32, align: u32, retain: bool) -> u32 {
        let cursor = if retain {
            &mut self.retain
        } else {
            &mut self.data
        };
        let a = align.max(1);
        let mis = *cursor % a;
        if mis != 0 {
            *cursor += a - mis;
        }
        let off = *cursor;
        *cursor += size;
        off
    }
}

/// Compile a loaded project into an [`IrModule`] and [`Manifest`].
pub fn compile_loaded(
    project: &LoadedProject,
    build_id: &str,
) -> Result<(IrModule, Manifest), CompileError> {
    let mut units = Vec::new();
    for (_, path) in &project.lib_sources {
        let text = std::fs::read_to_string(path).map_err(|e| {
            CompileError::new(ErrorCode::EProject, format!("read {}: {e}", path.display()))
        })?;
        let u = crate::parse::parse(&text).map_err(|e| e.with_path(path.display().to_string()))?;
        // Libraries may only contribute FUNCTION_BLOCKs
        for d in &u.decls {
            if matches!(d, Decl::Program(_)) {
                return Err(CompileError::new(
                    ErrorCode::EProject,
                    format!("library {} must not define PROGRAM", path.display()),
                ));
            }
        }
        units.push((path.display().to_string(), u));
    }
    for path in &project.sources {
        let text = std::fs::read_to_string(path).map_err(|e| {
            CompileError::new(ErrorCode::EProject, format!("read {}: {e}", path.display()))
        })?;
        let u = crate::parse::parse(&text).map_err(|e| e.with_path(path.display().to_string()))?;
        units.push((path.display().to_string(), u));
    }

    let mut fb_asts: BTreeMap<String, FunctionBlock> = BTreeMap::new();
    let mut programs: BTreeMap<String, Program> = BTreeMap::new();
    for (_path, unit) in &units {
        for d in &unit.decls {
            match d {
                Decl::FunctionBlock(fb) => {
                    let key = fb.name.to_ascii_lowercase();
                    if fb_asts.contains_key(&key) {
                        return Err(CompileError::new(
                            ErrorCode::EDuplicate,
                            format!("duplicate FUNCTION_BLOCK {}", fb.name),
                        )
                        .with_span(fb.name_span));
                    }
                    fb_asts.insert(key, fb.clone());
                }
                Decl::Program(p) => {
                    let key = p.name.to_ascii_lowercase();
                    if programs.contains_key(&key) {
                        return Err(CompileError::new(
                            ErrorCode::EDuplicate,
                            format!("duplicate PROGRAM {}", p.name),
                        )
                        .with_span(p.name_span));
                    }
                    programs.insert(key, p.clone());
                }
            }
        }
    }

    // Layout user FBs (no recursion in type refs for size — topological)
    let mut fb_layouts: HashMap<String, UserFbLayout> = HashMap::new();
    let mut pending: Vec<String> = fb_asts.keys().cloned().collect();
    let mut guard = 0;
    while !pending.is_empty() {
        guard += 1;
        if guard > 10_000 {
            return Err(CompileError::new(
                ErrorCode::ELayout,
                "FB layout did not converge (possible cycle)",
            ));
        }
        let before = pending.len();
        pending.retain(|name| {
            let fb = fb_asts.get(name).unwrap();
            match layout_user_fb(fb, &fb_layouts) {
                Ok(layout) => {
                    fb_layouts.insert(name.clone(), layout);
                    false
                }
                Err(e) if e.code == ErrorCode::EUndefined => true, // retry
                Err(_) => {
                    // push hard errors by storing a poisoned attempt — recompute below
                    true
                }
            }
        });
        if pending.len() == before {
            // Force error from first remaining
            let name = &pending[0];
            let fb = fb_asts.get(name).unwrap();
            layout_user_fb(fb, &fb_layouts)?;
            return Err(CompileError::new(
                ErrorCode::EUndefined,
                format!("cannot layout FB {name}"),
            ));
        }
    }

    // Recursion check on user FB call graph
    check_fb_recursion(&fb_asts)?;

    let mut alloc = Allocator { data: 0, retain: 0 };
    let mut globals: HashMap<String, Symbol> = HashMap::new();
    let mut retain_symbols: Vec<ManifestRetainSymbol> = Vec::new();
    let mut tag_dictionary: Vec<TagEntry> = Vec::new();
    let mut input_slots: u32 = 0;
    let mut output_slots: u32 = 0;
    let mut prim_counts: HashMap<PrimitiveId, u32> = HashMap::new();

    // Project tags first (authoritative slots)
    for tag in &project.file.tag {
        let ty = parse_ir_type(&tag.ty)?;
        let kind = match tag.kind.to_ascii_uppercase().as_str() {
            "I" => TagKind::I,
            "Q" => TagKind::Q,
            "M" => TagKind::M,
            "R" => TagKind::R,
            other => {
                return Err(CompileError::new(
                    ErrorCode::EProject,
                    format!("unknown tag kind {other}"),
                ));
            }
        };
        tag_dictionary.push(TagEntry {
            name: tag.name.clone(),
            ty: IrTypeName(ty),
            kind,
            slot: tag.slot,
        });
        match kind {
            TagKind::I => {
                let slot = tag.slot.unwrap_or(input_slots);
                input_slots = input_slots.max(slot + 1);
            }
            TagKind::Q => {
                let slot = tag.slot.unwrap_or(output_slots);
                output_slots = output_slots.max(slot + 1);
            }
            _ => {}
        }
    }

    // Collect globals from all programs (shared image)
    for p in programs.values() {
        bind_var_sections(
            &p.vars,
            true,
            &mut globals,
            &mut alloc,
            &fb_layouts,
            &mut prim_counts,
            &mut retain_symbols,
            &mut input_slots,
            &mut output_slots,
            &mut tag_dictionary,
            project.file.max_instances,
        )?;
    }

    // Ensure program tasks exist
    let mut task_entries = BTreeMap::new();
    for t in &project.file.task {
        let key = t.program.to_ascii_lowercase();
        if !programs.contains_key(&key) {
            return Err(CompileError::new(
                ErrorCode::EProject,
                format!("task {} PROGRAM {} not found", t.name, t.program),
            ));
        }
        task_entries.insert(t.name.clone(), format!("task.{}", t.name));
    }

    let mut codegen = Codegen::new(fb_layouts.clone(), globals.clone());
    // Emit user FB bodies first
    let mut fb_ids: HashMap<String, u32> = HashMap::new();
    let mut next_fb_id = 0u32;
    for (key, layout) in &fb_layouts {
        let id = next_fb_id;
        next_fb_id += 1;
        fb_ids.insert(key.clone(), id);
        let entry_name = format!("fb.{id}");
        // Patch layout entry name
        if let Some(l) = codegen.fb_layouts.get_mut(key) {
            l.entry_name = entry_name.clone();
        }
        codegen.begin_entry(&entry_name, true);
        // Local scope = FB fields relative + nested instances need separate handling
        let mut local = HashMap::new();
        for f in &layout.fields {
            if f.retain {
                local.insert(
                    f.name.to_ascii_lowercase(),
                    Symbol {
                        storage: Storage::Retain {
                            offset: f.retain_offset.unwrap(),
                            ty: f.ty.ir_elem().unwrap_or(IrType::Bool),
                        },
                        span: Span::default(),
                    },
                );
            } else {
                local.insert(
                    f.name.to_ascii_lowercase(),
                    Symbol {
                        storage: Storage::Data {
                            offset: f.offset,
                            ty: f.ty.clone(),
                        },
                        span: Span::default(),
                    },
                );
            }
        }
        // Nested instances declared in FB VAR
        for (iname, ity) in &layout.instances {
            match ity {
                TypeDef::Prim(pid) => {
                    let inst = *prim_counts.entry(*pid).or_insert(0);
                    if inst >= project.file.max_instances {
                        return Err(CompileError::new(
                            ErrorCode::ELayout,
                            "primitive instance limit exceeded",
                        ));
                    }
                    *prim_counts.get_mut(pid).unwrap() += 1;
                    let out_offsets = alloc_prim_outputs(&mut alloc, *pid);
                    local.insert(
                        iname.to_ascii_lowercase(),
                        Symbol {
                            storage: Storage::Prim {
                                id: *pid,
                                instance: inst,
                                out_offsets,
                            },
                            span: Span::default(),
                        },
                    );
                }
                TypeDef::UserFb(uname) => {
                    let ul = codegen
                        .fb_layouts
                        .get(&uname.to_ascii_lowercase())
                        .ok_or_else(|| {
                            CompileError::new(ErrorCode::EUndefined, format!("FB {uname}"))
                        })?;
                    let base = alloc.alloc(ul.total_data_size, 4, false);
                    local.insert(
                        iname.to_ascii_lowercase(),
                        Symbol {
                            storage: Storage::UserInst {
                                fb: uname.clone(),
                                base,
                            },
                            span: Span::default(),
                        },
                    );
                }
                _ => {}
            }
        }
        codegen.with_locals(local, |cg| {
            for stmt in &layout.body {
                cg.emit_stmt(stmt)?;
            }
            cg.emit_simple(Opcode::Ret, 0);
            Ok(())
        })?;
    }

    // Emit programs
    for t in &project.file.task {
        let prog = programs.get(&t.program.to_ascii_lowercase()).unwrap();
        let entry = format!("task.{}", t.name);
        codegen.begin_entry(&entry, false);
        // Program-local vars already in globals; also bind PROGRAM-private VAR into locals
        let local = HashMap::new();
        // Locals already merged into globals in bind_var_sections for programs —
        // use globals only.
        codegen.with_locals(local, |cg| {
            for stmt in &prog.body {
                cg.emit_stmt(stmt)?;
            }
            cg.emit_simple(Opcode::Halt, 0);
            Ok(())
        })?;
    }

    // Finalize sizes: bump alloc for anything codegen allocated during FB nested — already in alloc
    // Also account for loop counters etc. allocated during codegen
    alloc.data = alloc.data.max(codegen.extra_data);
    alloc.retain = alloc.retain.max(codegen.extra_retain);

    let module = codegen.finish(alloc.data, alloc.retain, input_slots, output_slots)?;
    verify_module(&module)?;

    let restart = match project.file.restart_policy.as_str() {
        "safe_reset" => RestartPolicy::SafeReset,
        "bumpless" => RestartPolicy::Bumpless,
        other => {
            return Err(CompileError::new(
                ErrorCode::EProject,
                format!("unknown restart_policy {other}"),
            ));
        }
    };

    let mut manifest = Manifest {
        id: project.file.id.clone(),
        version: project.file.version.clone(),
        build_id: build_id.to_string(),
        ir_major: IR_MAJOR,
        ir_minor: IR_MINOR,
        primitive_abi: PRIMITIVE_ABI,
        task_entries,
        retain_symbols,
        tag_dictionary,
        restart_policy: restart,
        compatibility_hash: "0".repeat(64),
        input_slots: Some(input_slots),
        output_slots: Some(output_slots),
        data_size: Some(module.data_size),
        retain_size: Some(module.retain_size),
        const_size: Some(module.const_size),
    };
    manifest.compatibility_hash = plc_package::compute_compatibility_hash(&manifest);

    Ok((module, manifest))
}

fn parse_ir_type(s: &str) -> Result<IrType, CompileError> {
    match s.to_ascii_uppercase().as_str() {
        "BOOL" => Ok(IrType::Bool),
        "INT" => Ok(IrType::Int),
        "DINT" => Ok(IrType::Dint),
        "REAL" => Ok(IrType::Real),
        "TIME" => Ok(IrType::Time),
        other => Err(CompileError::new(
            ErrorCode::EType,
            format!("unsupported type {other}"),
        )),
    }
}

fn resolve_type(
    ty: &TypeExpr,
    fb_layouts: &HashMap<String, UserFbLayout>,
) -> Result<TypeDef, CompileError> {
    match ty {
        TypeExpr::Named { name, span } => {
            let u = name.to_ascii_uppercase();
            if let Ok(ir) = parse_ir_type(&u) {
                return Ok(TypeDef::Elem(ir));
            }
            if let Some(p) = PrimitiveId::from_name(&u) {
                return Ok(TypeDef::Prim(p));
            }
            // User FB type (known or forward-ref; layout phase resolves size).
            let _ = (fb_layouts, span);
            Ok(TypeDef::UserFb(name.clone()))
        }
        TypeExpr::Array { upper, elem, span } => {
            let len = upper + 1;
            if len > MAX_ARRAY_ELEMS {
                return Err(CompileError::new(
                    ErrorCode::ELayout,
                    format!("array length {len} exceeds {MAX_ARRAY_ELEMS}"),
                )
                .with_span(*span));
            }
            let elem_ty = resolve_type(elem, fb_layouts)?;
            if !matches!(elem_ty, TypeDef::Elem(_)) {
                return Err(CompileError::new(
                    ErrorCode::EType,
                    "array elements must be elementary in v1",
                )
                .with_span(*span));
            }
            Ok(TypeDef::Array {
                elem: Box::new(elem_ty),
                len,
            })
        }
    }
}

fn prim_shadow_size(p: PrimitiveId) -> u32 {
    // outputs only stash
    match p.output_count() {
        1 => 4,
        2 => 8,
        n => u32::from(n) * 4,
    }
}

fn prim_out_types(p: PrimitiveId) -> Vec<(String, IrType)> {
    match p {
        PrimitiveId::Ton | PrimitiveId::Tof | PrimitiveId::Tp => {
            vec![("Q".into(), IrType::Bool), ("ET".into(), IrType::Time)]
        }
        PrimitiveId::Ctu | PrimitiveId::Ctd => {
            vec![("Q".into(), IrType::Bool), ("CV".into(), IrType::Dint)]
        }
        PrimitiveId::Rs | PrimitiveId::Sr | PrimitiveId::RTrig | PrimitiveId::FTrig => {
            vec![("Q".into(), IrType::Bool)]
        }
        PrimitiveId::Pid => vec![("OUT".into(), IrType::Real)],
    }
}

fn prim_in_names(p: PrimitiveId) -> &'static [&'static str] {
    match p {
        PrimitiveId::Ton | PrimitiveId::Tof | PrimitiveId::Tp => &["IN", "PT"],
        PrimitiveId::Ctu => &["CU", "R", "PV"],
        PrimitiveId::Ctd => &["CD", "LD", "PV"],
        PrimitiveId::Rs | PrimitiveId::Sr => &["S", "R"],
        PrimitiveId::RTrig | PrimitiveId::FTrig => &["CLK"],
        PrimitiveId::Pid => &["PV", "SP", "ENABLE"],
    }
}

fn alloc_prim_outputs(alloc: &mut Allocator, p: PrimitiveId) -> Vec<(String, u32, IrType)> {
    let mut out = Vec::new();
    for (name, ty) in prim_out_types(p) {
        let off = alloc.alloc(ty.byte_width() as u32, ty_align(ty), false);
        out.push((name, off, ty));
    }
    out
}

fn ty_align(ty: IrType) -> u32 {
    match ty {
        IrType::Bool => 1,
        IrType::Int => 2,
        _ => 4,
    }
}

fn layout_user_fb(
    fb: &FunctionBlock,
    known: &HashMap<String, UserFbLayout>,
) -> Result<UserFbLayout, CompileError> {
    let mut alloc = Allocator { data: 0, retain: 0 };
    let mut fields = Vec::new();
    let mut inputs = Vec::new();
    let mut outputs = Vec::new();
    let mut instances = Vec::new();

    for section in &fb.vars {
        for v in &section.vars {
            let ty = resolve_type(&v.ty, known)?;
            // If type is UserFb not yet known, fail with EUndefined for retry
            if let TypeDef::UserFb(ref n) = ty {
                if !known.contains_key(&n.to_ascii_lowercase()) {
                    return Err(CompileError::new(
                        ErrorCode::EUndefined,
                        format!("FB {n} not laid out yet"),
                    ));
                }
            }
            match section.kind {
                VarKind::Input => {
                    inputs.push(v.name.clone());
                    let size = ty.byte_size(known)?;
                    let off = alloc.alloc(size, ty.align(), false);
                    fields.push(FieldLayout {
                        name: v.name.clone(),
                        ty,
                        offset: off,
                        retain: false,
                        retain_offset: None,
                    });
                }
                VarKind::Output => {
                    outputs.push(v.name.clone());
                    let size = ty.byte_size(known)?;
                    let off = alloc.alloc(size, ty.align(), false);
                    fields.push(FieldLayout {
                        name: v.name.clone(),
                        ty,
                        offset: off,
                        retain: false,
                        retain_offset: None,
                    });
                }
                VarKind::Retain => {
                    if let TypeDef::Elem(ir) = &ty {
                        let off = alloc.alloc(ir.byte_width() as u32, ty_align(*ir), true);
                        fields.push(FieldLayout {
                            name: v.name.clone(),
                            ty,
                            offset: 0,
                            retain: true,
                            retain_offset: Some(off),
                        });
                    } else {
                        return Err(CompileError::new(
                            ErrorCode::EType,
                            "VAR_RETAIN field must be elementary",
                        )
                        .with_span(v.span));
                    }
                }
                VarKind::Var | VarKind::Constant => match &ty {
                    TypeDef::Prim(_) | TypeDef::UserFb(_) => {
                        instances.push((v.name.clone(), ty));
                    }
                    _ => {
                        let size = ty.byte_size(known)?;
                        let off = alloc.alloc(size, ty.align(), false);
                        fields.push(FieldLayout {
                            name: v.name.clone(),
                            ty,
                            offset: off,
                            retain: false,
                            retain_offset: None,
                        });
                    }
                },
                VarKind::Global => {
                    return Err(CompileError::new(
                        ErrorCode::EExcluded,
                        "VAR_GLOBAL inside FUNCTION_BLOCK is excluded",
                    )
                    .with_span(section.span));
                }
            }
        }
    }

    Ok(UserFbLayout {
        name: fb.name.clone(),
        inputs,
        outputs,
        fields,
        total_data_size: alloc.data.max(4),
        entry_name: format!("fb.{}", fb.name),
        body: fb.body.clone(),
        instances,
    })
}

fn check_fb_recursion(fbs: &BTreeMap<String, FunctionBlock>) -> Result<(), CompileError> {
    // Build edges: FB A calls instance typed as FB B
    let mut edges: HashMap<String, Vec<String>> = HashMap::new();
    for (name, fb) in fbs {
        let mut targets = Vec::new();
        for section in &fb.vars {
            for v in &section.vars {
                if let TypeExpr::Named { name: tn, .. } = &v.ty {
                    let key = tn.to_ascii_lowercase();
                    if fbs.contains_key(&key) {
                        targets.push(key);
                    }
                }
            }
        }
        // also scan call statements for type-order calls — instances cover composition
        edges.insert(name.clone(), targets);
    }
    for start in edges.keys() {
        let mut stack = vec![start.clone()];
        let mut path = HashSet::new();
        while let Some(n) = stack.pop() {
            if !path.insert(n.clone()) {
                return Err(CompileError::new(
                    ErrorCode::ERecursion,
                    format!("recursive FB composition involving {n}"),
                ));
            }
            if let Some(ts) = edges.get(&n) {
                for t in ts {
                    if t == start {
                        return Err(CompileError::new(
                            ErrorCode::ERecursion,
                            format!("recursive FB call graph at {start}"),
                        ));
                    }
                    stack.push(t.clone());
                }
            }
            path.remove(&n);
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn bind_var_sections(
    sections: &[VarSection],
    allow_global: bool,
    globals: &mut HashMap<String, Symbol>,
    alloc: &mut Allocator,
    fb_layouts: &HashMap<String, UserFbLayout>,
    prim_counts: &mut HashMap<PrimitiveId, u32>,
    retain_symbols: &mut Vec<ManifestRetainSymbol>,
    input_slots: &mut u32,
    output_slots: &mut u32,
    tag_dictionary: &mut Vec<TagEntry>,
    max_instances: u32,
) -> Result<(), CompileError> {
    for section in sections {
        let is_const = section.kind == VarKind::Constant;
        if section.kind == VarKind::Global && !allow_global {
            return Err(
                CompileError::new(ErrorCode::EExcluded, "VAR_GLOBAL not allowed here")
                    .with_span(section.span),
            );
        }
        for v in &section.vars {
            let key = v.name.to_ascii_lowercase();
            if globals.contains_key(&key) {
                // Multi-PROGRAM apps may re-declare the same VAR_GLOBAL AT binding.
                if matches!(
                    section.kind,
                    VarKind::Global | VarKind::Retain | VarKind::Constant
                ) && v.at.is_some()
                {
                    continue;
                }
                return Err(CompileError::new(
                    ErrorCode::EDuplicate,
                    format!("duplicate variable {}", v.name),
                )
                .with_span(v.name_span));
            }
            let ty = resolve_type(&v.ty, fb_layouts)?;
            let storage = if let Some(at) = &v.at {
                match at.plane {
                    DirectPlane::I => {
                        let ir = ty.ir_elem().ok_or_else(|| {
                            CompileError::new(ErrorCode::EType, "%I must be elementary")
                                .with_span(v.span)
                        })?;
                        *input_slots = (*input_slots).max(at.index + 1);
                        if !tag_dictionary
                            .iter()
                            .any(|t| t.slot == Some(at.index) && t.kind == TagKind::I)
                        {
                            tag_dictionary.push(TagEntry {
                                name: v.name.clone(),
                                ty: IrTypeName(ir),
                                kind: TagKind::I,
                                slot: Some(at.index),
                            });
                        }
                        Storage::Input {
                            slot: at.index,
                            ty: ir,
                        }
                    }
                    DirectPlane::Q => {
                        let ir = ty.ir_elem().ok_or_else(|| {
                            CompileError::new(ErrorCode::EType, "%Q must be elementary")
                                .with_span(v.span)
                        })?;
                        *output_slots = (*output_slots).max(at.index + 1);
                        if !tag_dictionary
                            .iter()
                            .any(|t| t.slot == Some(at.index) && t.kind == TagKind::Q)
                        {
                            tag_dictionary.push(TagEntry {
                                name: v.name.clone(),
                                ty: IrTypeName(ir),
                                kind: TagKind::Q,
                                slot: Some(at.index),
                            });
                        }
                        Storage::Output {
                            slot: at.index,
                            ty: ir,
                        }
                    }
                    DirectPlane::R => {
                        let ir = ty.ir_elem().ok_or_else(|| {
                            CompileError::new(ErrorCode::EType, "%R must be elementary")
                                .with_span(v.span)
                        })?;
                        let off = at.index; // treat as byte offset when provided as %R0
                                            // Prefer allocator if offset is just an index — use alloc for size
                        let real_off = if at.index == 0 && alloc.retain == 0 {
                            alloc.alloc(ir.byte_width() as u32, ty_align(ir), true)
                        } else {
                            let o = alloc.alloc(ir.byte_width() as u32, ty_align(ir), true);
                            let _ = off;
                            o
                        };
                        retain_symbols.push(ManifestRetainSymbol {
                            name: v.name.clone(),
                            ty: IrTypeName(ir),
                            offset: real_off,
                        });
                        if !tag_dictionary.iter().any(|t| t.name == v.name) {
                            tag_dictionary.push(TagEntry {
                                name: v.name.clone(),
                                ty: IrTypeName(ir),
                                kind: TagKind::R,
                                slot: None,
                            });
                        }
                        Storage::Retain {
                            offset: real_off,
                            ty: ir,
                        }
                    }
                    DirectPlane::M => {
                        let size = ty.byte_size(fb_layouts)?;
                        let off = alloc.alloc(size, ty.align(), false);
                        Storage::Data { offset: off, ty }
                    }
                }
            } else if is_const {
                let ir = ty.ir_elem().ok_or_else(|| {
                    CompileError::new(ErrorCode::EType, "CONSTANT must be elementary")
                        .with_span(v.span)
                })?;
                let lit = match &v.init {
                    Some(Expr::Literal { value, .. }) => value.clone(),
                    _ => {
                        return Err(CompileError::new(
                            ErrorCode::EType,
                            "CONSTANT requires literal initializer",
                        )
                        .with_span(v.span));
                    }
                };
                Storage::Const { ty: ir, value: lit }
            } else if section.kind == VarKind::Retain {
                let ir = ty.ir_elem().ok_or_else(|| {
                    CompileError::new(ErrorCode::EType, "VAR_RETAIN must be elementary")
                        .with_span(v.span)
                })?;
                let off = alloc.alloc(ir.byte_width() as u32, ty_align(ir), true);
                retain_symbols.push(ManifestRetainSymbol {
                    name: v.name.clone(),
                    ty: IrTypeName(ir),
                    offset: off,
                });
                Storage::Retain {
                    offset: off,
                    ty: ir,
                }
            } else {
                match &ty {
                    TypeDef::Prim(pid) => {
                        let inst = *prim_counts.entry(*pid).or_insert(0);
                        if inst >= max_instances {
                            return Err(CompileError::new(
                                ErrorCode::ELayout,
                                "primitive instance limit exceeded",
                            ));
                        }
                        *prim_counts.get_mut(pid).unwrap() += 1;
                        let out_offsets = alloc_prim_outputs(alloc, *pid);
                        Storage::Prim {
                            id: *pid,
                            instance: inst,
                            out_offsets,
                        }
                    }
                    TypeDef::UserFb(name) => {
                        let layout =
                            fb_layouts.get(&name.to_ascii_lowercase()).ok_or_else(|| {
                                CompileError::new(
                                    ErrorCode::EUndefined,
                                    format!("unknown FB type {name}"),
                                )
                                .with_span(v.span)
                            })?;
                        let base = alloc.alloc(layout.total_data_size, 4, false);
                        Storage::UserInst {
                            fb: name.clone(),
                            base,
                        }
                    }
                    _ => {
                        let size = ty.byte_size(fb_layouts)?;
                        let off = alloc.alloc(size, ty.align(), false);
                        Storage::Data {
                            offset: off,
                            ty: ty.clone(),
                        }
                    }
                }
            };
            globals.insert(
                key,
                Symbol {
                    storage,
                    span: v.name_span,
                },
            );
        }
    }
    Ok(())
}

// --- Codegen ---

struct Codegen {
    fb_layouts: HashMap<String, UserFbLayout>,
    globals: HashMap<String, Symbol>,
    locals: HashMap<String, Symbol>,
    code: Vec<u8>,
    entries: Vec<EntryPoint>,
    labels: HashMap<String, u32>,
    fixups: Vec<(usize, String)>,
    label_id: u32,
    extra_data: u32,
    extra_retain: u32,
    data_cursor: u32,
}

impl Codegen {
    fn new(fb_layouts: HashMap<String, UserFbLayout>, globals: HashMap<String, Symbol>) -> Self {
        Self {
            fb_layouts,
            globals,
            locals: HashMap::new(),
            code: Vec::new(),
            entries: Vec::new(),
            labels: HashMap::new(),
            fixups: Vec::new(),
            label_id: 0,
            extra_data: 0,
            extra_retain: 0,
            data_cursor: 0,
        }
    }

    fn begin_entry(&mut self, name: &str, is_user_fb: bool) {
        self.entries.push(EntryPoint {
            name: name.to_string(),
            pc: self.code.len() as u32,
            is_user_fb,
        });
    }

    fn with_locals<F>(&mut self, locals: HashMap<String, Symbol>, f: F) -> Result<(), CompileError>
    where
        F: FnOnce(&mut Self) -> Result<(), CompileError>,
    {
        self.locals = locals;
        let r = f(self);
        self.locals.clear();
        r
    }

    fn fresh_label(&mut self, prefix: &str) -> String {
        let id = self.label_id;
        self.label_id += 1;
        format!(".{prefix}{id}")
    }

    fn place_label(&mut self, name: &str) {
        self.labels.insert(name.to_string(), self.code.len() as u32);
    }

    fn emit(&mut self, instr: DecodedInstr) {
        self.code.extend_from_slice(&encode_instruction(&instr));
    }

    fn emit_simple(&mut self, op: Opcode, payload: u32) {
        self.emit(DecodedInstr::Simple { op, payload });
    }

    fn emit_jmp(&mut self, op: Opcode, label: &str) {
        let off = self.code.len();
        self.emit_simple(op, 0);
        self.fixups.push((off, label.to_string()));
    }

    fn lookup(&self, name: &str) -> Result<&Symbol, CompileError> {
        let key = name.to_ascii_lowercase();
        self.locals
            .get(&key)
            .or_else(|| self.globals.get(&key))
            .ok_or_else(|| CompileError::new(ErrorCode::EUndefined, format!("undefined `{name}`")))
    }

    fn alloc_temp(&mut self, ty: IrType) -> u32 {
        let a = ty_align(ty);
        let mis = self.data_cursor % a;
        if mis != 0 {
            self.data_cursor += a - mis;
        }
        // Find max from globals
        let base = self
            .globals
            .values()
            .filter_map(|s| match &s.storage {
                Storage::Data { offset, ty } => {
                    Some(offset + ty.byte_size(&self.fb_layouts).unwrap_or(4))
                }
                Storage::Prim { out_offsets, .. } => out_offsets
                    .iter()
                    .map(|(_, o, t)| o + t.byte_width() as u32)
                    .max(),
                Storage::UserInst { fb, base } => self
                    .fb_layouts
                    .get(&fb.to_ascii_lowercase())
                    .map(|l| base + l.total_data_size),
                _ => None,
            })
            .max()
            .unwrap_or(0)
            .max(self.extra_data)
            .max(self.data_cursor);
        self.data_cursor = base;
        let mis = self.data_cursor % a;
        if mis != 0 {
            self.data_cursor += a - mis;
        }
        let off = self.data_cursor;
        self.data_cursor += ty.byte_width() as u32;
        self.extra_data = self.data_cursor;
        off
    }

    fn emit_stmt(&mut self, stmt: &Stmt) -> Result<(), CompileError> {
        match stmt {
            Stmt::Empty { .. } => Ok(()),
            Stmt::Return { .. } => {
                self.emit_simple(Opcode::Ret, 0);
                Ok(())
            }
            Stmt::Assign { lhs, rhs, .. } => {
                let ty = self.emit_expr(rhs)?;
                self.emit_store(lhs, ty)?;
                Ok(())
            }
            Stmt::FbCall { call, .. } => self.emit_fb_call(call),
            Stmt::If {
                branches,
                else_body,
                ..
            } => {
                let end = self.fresh_label("if_end");
                for (i, (cond, body)) in branches.iter().enumerate() {
                    let next = self.fresh_label("if_next");
                    let t = self.emit_expr(cond)?;
                    if t != IrType::Bool {
                        return Err(CompileError::new(
                            ErrorCode::EType,
                            "IF condition must be BOOL",
                        ));
                    }
                    self.emit_jmp(Opcode::JmpIfNot, &next);
                    for s in body {
                        self.emit_stmt(s)?;
                    }
                    if i + 1 < branches.len() || else_body.is_some() {
                        self.emit_jmp(Opcode::Jmp, &end);
                    }
                    self.place_label(&next);
                }
                if let Some(eb) = else_body {
                    for s in eb {
                        self.emit_stmt(s)?;
                    }
                }
                self.place_label(&end);
                Ok(())
            }
            Stmt::Case {
                selector,
                arms,
                else_body,
                ..
            } => {
                let sel_ty = self.emit_expr(selector)?;
                if !matches!(sel_ty, IrType::Int | IrType::Dint) {
                    return Err(CompileError::new(
                        ErrorCode::EType,
                        "CASE selector must be INT/DINT",
                    ));
                }
                let tmp = self.alloc_temp(sel_ty);
                self.emit_simple(Opcode::StData, tmp);
                let end = self.fresh_label("case_end");
                for arm in arms {
                    let body_l = self.fresh_label("case_body");
                    let next_l = self.fresh_label("case_next");
                    for (i, lab) in arm.labels.iter().enumerate() {
                        self.emit_simple(Opcode::LdData, tmp);
                        self.emit(DecodedInstr::WithImm32 {
                            op: Opcode::PushIDint,
                            payload: 0,
                            imm: *lab as u32,
                        });
                        self.emit_simple(Opcode::Eq, 0);
                        if i + 1 == arm.labels.len() {
                            self.emit_jmp(Opcode::JmpIfNot, &next_l);
                        } else {
                            self.emit_jmp(Opcode::JmpIf, &body_l);
                        }
                    }
                    self.place_label(&body_l);
                    for s in &arm.body {
                        self.emit_stmt(s)?;
                    }
                    self.emit_jmp(Opcode::Jmp, &end);
                    self.place_label(&next_l);
                }
                if let Some(eb) = else_body {
                    for s in eb {
                        self.emit_stmt(s)?;
                    }
                }
                self.place_label(&end);
                Ok(())
            }
            Stmt::While {
                cond,
                max_iter,
                body,
                span,
                ..
            } => {
                let max = max_iter.unwrap_or(MAX_LOOP_DEFAULT);
                if max_iter.is_none() {
                    // Require attribute unless we can prove — v1 requires attribute or default cap
                    // Appendix B: must carry attribute OR compiler-proven bound. We accept default cap
                    // only when attribute present OR max_iter provided; else error.
                    return Err(CompileError::new(
                        ErrorCode::EUnboundedLoop,
                        "WHILE requires { max_iter := <const> } attribute",
                    )
                    .with_span(*span));
                }
                let counter = self.alloc_temp(IrType::Dint);
                // counter := max
                self.emit(DecodedInstr::WithImm32 {
                    op: Opcode::PushIDint,
                    payload: 0,
                    imm: max,
                });
                self.emit_simple(Opcode::StData, counter);
                let head = self.fresh_label("wh_head");
                let end = self.fresh_label("wh_end");
                self.place_label(&head);
                // if counter == 0 goto end
                self.emit_simple(Opcode::LdData, counter);
                self.emit(DecodedInstr::WithImm32 {
                    op: Opcode::PushIDint,
                    payload: 0,
                    imm: 0,
                });
                self.emit_simple(Opcode::Eq, 0);
                self.emit_jmp(Opcode::JmpIf, &end);
                let t = self.emit_expr(cond)?;
                if t != IrType::Bool {
                    return Err(CompileError::new(
                        ErrorCode::EType,
                        "WHILE condition must be BOOL",
                    ));
                }
                self.emit_jmp(Opcode::JmpIfNot, &end);
                for s in body {
                    self.emit_stmt(s)?;
                }
                // counter -= 1
                self.emit_simple(Opcode::LdData, counter);
                self.emit(DecodedInstr::WithImm32 {
                    op: Opcode::PushIDint,
                    payload: 0,
                    imm: 1,
                });
                self.emit_simple(Opcode::Sub, 0);
                self.emit_simple(Opcode::StData, counter);
                self.emit_jmp(Opcode::Jmp, &head);
                self.place_label(&end);
                Ok(())
            }
            Stmt::For {
                var,
                from,
                to,
                by,
                max_iter,
                body,
                span,
                ..
            } => {
                let max = match max_iter {
                    Some(m) => *m,
                    None => {
                        // Try prove from constant bounds
                        match (from, to, by) {
                            (
                                Expr::Literal {
                                    value: Literal::Int(a),
                                    ..
                                },
                                Expr::Literal {
                                    value: Literal::Int(b),
                                    ..
                                },
                                None
                                | Some(Expr::Literal {
                                    value: Literal::Int(1),
                                    ..
                                }),
                            ) if *b >= *a && (*b - *a + 1) <= i64::from(MAX_LOOP_DEFAULT) => {
                                (*b - *a + 1) as u32
                            }
                            _ => {
                                return Err(CompileError::new(
                                    ErrorCode::EUnboundedLoop,
                                    "FOR requires { max_iter := <const> } or constant bounds ≤ 10000",
                                )
                                .with_span(*span));
                            }
                        }
                    }
                };
                let _ = max;
                let vty = IrType::Dint;
                // Ensure loop var exists in scope as data
                let var_off = match self.lookup(var).map(|s| s.storage.clone()) {
                    Ok(Storage::Data { offset, .. }) => offset,
                    Ok(_) => {
                        return Err(CompileError::new(
                            ErrorCode::EType,
                            "FOR variable must be a data VAR",
                        ));
                    }
                    Err(_) => {
                        let off = self.alloc_temp(vty);
                        self.locals.insert(
                            var.to_ascii_lowercase(),
                            Symbol {
                                storage: Storage::Data {
                                    offset: off,
                                    ty: TypeDef::Elem(vty),
                                },
                                span: *span,
                            },
                        );
                        off
                    }
                };
                self.emit_expr(from)?;
                self.emit_simple(Opcode::StData, var_off);
                let to_tmp = self.alloc_temp(IrType::Dint);
                self.emit_expr(to)?;
                self.emit_simple(Opcode::StData, to_tmp);
                let by_tmp = self.alloc_temp(IrType::Dint);
                if let Some(b) = by {
                    self.emit_expr(b)?;
                } else {
                    self.emit(DecodedInstr::WithImm32 {
                        op: Opcode::PushIDint,
                        payload: 0,
                        imm: 1,
                    });
                }
                self.emit_simple(Opcode::StData, by_tmp);
                let counter = self.alloc_temp(IrType::Dint);
                self.emit(DecodedInstr::WithImm32 {
                    op: Opcode::PushIDint,
                    payload: 0,
                    imm: max,
                });
                self.emit_simple(Opcode::StData, counter);
                let head = self.fresh_label("for_head");
                let end = self.fresh_label("for_end");
                self.place_label(&head);
                self.emit_simple(Opcode::LdData, counter);
                self.emit(DecodedInstr::WithImm32 {
                    op: Opcode::PushIDint,
                    payload: 0,
                    imm: 0,
                });
                self.emit_simple(Opcode::Eq, 0);
                self.emit_jmp(Opcode::JmpIf, &end);
                // if var > to goto end (for positive by — v1 assumes by > 0)
                self.emit_simple(Opcode::LdData, var_off);
                self.emit_simple(Opcode::LdData, to_tmp);
                self.emit_simple(Opcode::Gt, 0);
                self.emit_jmp(Opcode::JmpIf, &end);
                for s in body {
                    self.emit_stmt(s)?;
                }
                self.emit_simple(Opcode::LdData, var_off);
                self.emit_simple(Opcode::LdData, by_tmp);
                self.emit_simple(Opcode::Add, 0);
                self.emit_simple(Opcode::StData, var_off);
                self.emit_simple(Opcode::LdData, counter);
                self.emit(DecodedInstr::WithImm32 {
                    op: Opcode::PushIDint,
                    payload: 0,
                    imm: 1,
                });
                self.emit_simple(Opcode::Sub, 0);
                self.emit_simple(Opcode::StData, counter);
                self.emit_jmp(Opcode::Jmp, &head);
                self.place_label(&end);
                Ok(())
            }
        }
    }

    fn emit_fb_call(&mut self, call: &FbCallExpr) -> Result<(), CompileError> {
        let inst_name = match call.callee.first() {
            Some(PathSeg::Ident(n)) if call.callee.len() == 1 => n.clone(),
            _ => {
                return Err(CompileError::new(
                    ErrorCode::EParse,
                    "FB call callee must be a simple instance name",
                )
                .with_span(call.callee_span));
            }
        };
        let sym = self.lookup(&inst_name)?.clone();
        match sym.storage {
            Storage::Prim {
                id,
                instance,
                out_offsets,
            } => {
                // Resolve inputs in declaration order
                let names = prim_in_names(id);
                let mut args: Vec<Option<&Expr>> = vec![None; names.len()];
                let mut pos = 0;
                for a in &call.inputs {
                    match a {
                        CallArg::Positional(e) => {
                            if pos >= args.len() {
                                return Err(CompileError::new(
                                    ErrorCode::EType,
                                    "too many FB inputs",
                                ));
                            }
                            args[pos] = Some(e);
                            pos += 1;
                        }
                        CallArg::Named { name, value } => {
                            if name.eq_ignore_ascii_case("EN") {
                                // only allow TRUE
                                match value {
                                    Expr::Literal {
                                        value: Literal::Bool(true),
                                        ..
                                    } => {}
                                    _ => {
                                        return Err(CompileError::new(
                                            ErrorCode::EExcluded,
                                            "EN must be constant TRUE in v1 (or omitted)",
                                        ));
                                    }
                                }
                                continue;
                            }
                            let idx = names
                                .iter()
                                .position(|n| n.eq_ignore_ascii_case(name))
                                .ok_or_else(|| {
                                    CompileError::new(
                                        ErrorCode::EUndefined,
                                        format!("unknown input {name}"),
                                    )
                                })?;
                            args[idx] = Some(value);
                        }
                    }
                }
                for (i, a) in args.iter().enumerate() {
                    let e = a.ok_or_else(|| {
                        CompileError::new(ErrorCode::EType, format!("missing input {}", names[i]))
                    })?;
                    self.emit_expr(e)?;
                }
                self.emit(DecodedInstr::CallFb {
                    fb_kind: 0,
                    fb_id: id as u32,
                    instance_base: instance,
                });
                // Store outputs: VM leaves first output on top (TON: Q then ET under it).
                for (_name, off, _ty) in &out_offsets {
                    self.emit_simple(Opcode::StData, *off);
                }
                // Output associations
                for o in &call.outputs {
                    if o.name.eq_ignore_ascii_case("ENO") {
                        continue;
                    }
                    let (_n, off, ty) = out_offsets
                        .iter()
                        .find(|(n, _, _)| n.eq_ignore_ascii_case(&o.name))
                        .ok_or_else(|| {
                            CompileError::new(
                                ErrorCode::EUndefined,
                                format!("unknown output {}", o.name),
                            )
                        })?;
                    self.emit_simple(Opcode::LdData, *off);
                    self.emit_store(&o.dest, *ty)?;
                }
                Ok(())
            }
            Storage::UserInst { fb, base } => {
                let layout = self
                    .fb_layouts
                    .get(&fb.to_ascii_lowercase())
                    .ok_or_else(|| CompileError::new(ErrorCode::EUndefined, format!("FB {fb}")))?
                    .clone();
                // Copy-in
                let mut pos = 0;
                let mut named: HashMap<String, &Expr> = HashMap::new();
                for a in &call.inputs {
                    match a {
                        CallArg::Positional(e) => {
                            if pos >= layout.inputs.len() {
                                return Err(CompileError::new(ErrorCode::EType, "too many inputs"));
                            }
                            let field = &layout.inputs[pos];
                            let f = layout
                                .fields
                                .iter()
                                .find(|x| x.name.eq_ignore_ascii_case(field))
                                .unwrap();
                            let ty = self.emit_expr(e)?;
                            self.emit_store_offset(base + f.offset, false, ty)?;
                            pos += 1;
                        }
                        CallArg::Named { name, value } => {
                            if name.eq_ignore_ascii_case("EN") {
                                continue;
                            }
                            named.insert(name.to_ascii_lowercase(), value);
                        }
                    }
                }
                for (fname, expr) in named {
                    let f = layout
                        .fields
                        .iter()
                        .find(|x| x.name.eq_ignore_ascii_case(&fname))
                        .ok_or_else(|| {
                            CompileError::new(
                                ErrorCode::EUndefined,
                                format!("unknown input {fname}"),
                            )
                        })?;
                    let ty = self.emit_expr(expr)?;
                    if f.retain {
                        self.emit_store_offset(f.retain_offset.unwrap(), true, ty)?;
                    } else {
                        self.emit_store_offset(base + f.offset, false, ty)?;
                    }
                }
                let fb_id = self
                    .entries
                    .iter()
                    .find(|e| e.name == layout.entry_name || e.name.starts_with("fb."))
                    .map(|_| {
                        // find by layout entry_name
                        self.entries
                            .iter()
                            .position(|e| e.name == layout.entry_name)
                            .unwrap_or(0) as u32
                    });
                // Prefer parse id from entry name fb.N
                let fb_id = layout
                    .entry_name
                    .strip_prefix("fb.")
                    .and_then(|s| s.parse().ok())
                    .or(fb_id)
                    .unwrap_or(0);
                self.emit(DecodedInstr::CallFb {
                    fb_kind: 1,
                    fb_id,
                    instance_base: base,
                });
                // Copy-out associations + default nothing
                for o in &call.outputs {
                    let f = layout
                        .fields
                        .iter()
                        .find(|x| x.name.eq_ignore_ascii_case(&o.name))
                        .ok_or_else(|| {
                            CompileError::new(
                                ErrorCode::EUndefined,
                                format!("unknown output {}", o.name),
                            )
                        })?;
                    if f.retain {
                        self.emit_simple(Opcode::LdRetain, f.retain_offset.unwrap());
                    } else {
                        // LD relative: need absolute = base+off but LD_DATA uses data_base during FB;
                        // after RET, data_base restored — use absolute offset
                        self.emit_simple(Opcode::LdData, base + f.offset);
                    }
                    let ty = f.ty.ir_elem().unwrap_or(IrType::Bool);
                    self.emit_store(&o.dest, ty)?;
                }
                Ok(())
            }
            _ => Err(CompileError::new(
                ErrorCode::EType,
                format!("`{inst_name}` is not an FB instance"),
            )),
        }
    }

    fn emit_store_offset(
        &mut self,
        offset: u32,
        retain: bool,
        _ty: IrType,
    ) -> Result<(), CompileError> {
        if retain {
            self.emit_simple(Opcode::StRetain, offset);
        } else {
            self.emit_simple(Opcode::StData, offset);
        }
        Ok(())
    }

    fn emit_store(&mut self, lhs: &Expr, _ty: IrType) -> Result<(), CompileError> {
        match lhs {
            Expr::Name { path, .. } => {
                let (root, rest) = path_parts(path)?;
                let sym = self.lookup(&root)?.clone();
                if rest.is_empty() {
                    match sym.storage {
                        Storage::Output { slot, .. } => self.emit_simple(Opcode::StQ, slot),
                        Storage::Data { offset, .. } => self.emit_simple(Opcode::StData, offset),
                        Storage::Retain { offset, .. } => {
                            self.emit_simple(Opcode::StRetain, offset)
                        }
                        Storage::Input { .. } => {
                            return Err(CompileError::new(
                                ErrorCode::EType,
                                "cannot assign to %I input",
                            ));
                        }
                        Storage::Const { .. } => {
                            return Err(CompileError::new(
                                ErrorCode::EType,
                                "cannot assign to CONSTANT",
                            ));
                        }
                        Storage::Prim { .. } | Storage::UserInst { .. } => {
                            return Err(CompileError::new(
                                ErrorCode::EType,
                                "cannot assign to FB instance directly",
                            ));
                        }
                    }
                    return Ok(());
                }
                // Field store: inst.Q for prim outputs / user fields
                let field = match &rest[0] {
                    PathSeg::Ident(f) => f.clone(),
                    _ => {
                        return Err(CompileError::new(ErrorCode::EType, "bad field store"));
                    }
                };
                match sym.storage {
                    Storage::Prim { out_offsets, .. } => {
                        let (_n, off, _) = out_offsets
                            .iter()
                            .find(|(n, _, _)| n.eq_ignore_ascii_case(&field))
                            .ok_or_else(|| {
                                CompileError::new(
                                    ErrorCode::EUndefined,
                                    format!("no field {field}"),
                                )
                            })?;
                        self.emit_simple(Opcode::StData, *off);
                    }
                    Storage::UserInst { fb, base } => {
                        let layout = self.fb_layouts.get(&fb.to_ascii_lowercase()).unwrap();
                        let f = layout
                            .fields
                            .iter()
                            .find(|x| x.name.eq_ignore_ascii_case(&field))
                            .ok_or_else(|| {
                                CompileError::new(
                                    ErrorCode::EUndefined,
                                    format!("no field {field}"),
                                )
                            })?;
                        if f.retain {
                            self.emit_simple(Opcode::StRetain, f.retain_offset.unwrap());
                        } else {
                            self.emit_simple(Opcode::StData, base + f.offset);
                        }
                    }
                    Storage::Data {
                        offset,
                        ty: TypeDef::Array { elem, .. },
                    } => {
                        if let PathSeg::Index(idx) = &rest[0] {
                            let elem_ty = elem.ir_elem().unwrap();
                            let elem_sz = elem_ty.byte_width() as u32;
                            // only const index v1
                            let i = match idx {
                                Expr::Literal {
                                    value: Literal::Int(i),
                                    ..
                                } => *i as u32,
                                _ => {
                                    return Err(CompileError::new(
                                        ErrorCode::EType,
                                        "array index must be constant in v1 store",
                                    ));
                                }
                            };
                            self.emit_simple(Opcode::StData, offset + i * elem_sz);
                        }
                    }
                    _ => {
                        return Err(CompileError::new(ErrorCode::EType, "invalid store target"));
                    }
                }
                Ok(())
            }
            _ => Err(CompileError::new(
                ErrorCode::EType,
                "invalid assignment target",
            )),
        }
    }

    fn emit_expr(&mut self, expr: &Expr) -> Result<IrType, CompileError> {
        match expr {
            Expr::Literal { value, .. } => match value {
                Literal::Bool(b) => {
                    self.emit_simple(Opcode::PushIBool, u32::from(*b));
                    Ok(IrType::Bool)
                }
                Literal::Int(v) => {
                    self.emit(DecodedInstr::WithImm32 {
                        op: Opcode::PushIDint,
                        payload: 0,
                        imm: *v as u32,
                    });
                    Ok(IrType::Dint)
                }
                Literal::Real(v) => {
                    self.emit(DecodedInstr::WithImm32 {
                        op: Opcode::PushIReal,
                        payload: 0,
                        imm: v.to_bits(),
                    });
                    Ok(IrType::Real)
                }
                Literal::TimeMs(v) => {
                    self.emit(DecodedInstr::WithImm32 {
                        op: Opcode::PushTime,
                        payload: 0,
                        imm: *v as u32,
                    });
                    Ok(IrType::Time)
                }
            },
            Expr::QGood { expr, .. } => {
                let (root, _) = match expr.as_ref() {
                    Expr::Name { path, .. } => path_parts(path)?,
                    _ => {
                        return Err(CompileError::new(
                            ErrorCode::EType,
                            "Q_GOOD expects an %I tag name",
                        ));
                    }
                };
                let sym = self.lookup(&root)?;
                match sym.storage {
                    Storage::Input { slot, .. } => {
                        self.emit_simple(Opcode::LdIq, slot);
                        Ok(IrType::Bool)
                    }
                    _ => Err(CompileError::new(
                        ErrorCode::EType,
                        "Q_GOOD requires an input tag",
                    )),
                }
            }
            Expr::Unary { op, expr, .. } => {
                let t = self.emit_expr(expr)?;
                match op {
                    UnaryOp::Not => {
                        self.emit_simple(Opcode::Not, 0);
                        Ok(IrType::Bool)
                    }
                    UnaryOp::Neg => {
                        self.emit_simple(Opcode::Neg, 0);
                        Ok(t)
                    }
                }
            }
            Expr::Binary {
                op, left, right, ..
            } => {
                match op {
                    BinaryOp::And | BinaryOp::Or => {
                        // short-circuit
                        let end = self.fresh_label("sc_end");
                        let t = self.emit_expr(left)?;
                        if t != IrType::Bool {
                            return Err(CompileError::new(ErrorCode::EType, "AND/OR require BOOL"));
                        }
                        if *op == BinaryOp::And {
                            // if false, skip right (leave false)
                            let dup_false = self.fresh_label("and_f");
                            // stack has left; duplicate via temp
                            let tmp = self.alloc_temp(IrType::Bool);
                            self.emit_simple(Opcode::StData, tmp);
                            self.emit_simple(Opcode::LdData, tmp);
                            self.emit_jmp(Opcode::JmpIfNot, &dup_false);
                            self.emit_expr(right)?;
                            self.emit_jmp(Opcode::Jmp, &end);
                            self.place_label(&dup_false);
                            self.emit_simple(Opcode::PushIBool, 0);
                            self.place_label(&end);
                        } else {
                            let dup_true = self.fresh_label("or_t");
                            let tmp = self.alloc_temp(IrType::Bool);
                            self.emit_simple(Opcode::StData, tmp);
                            self.emit_simple(Opcode::LdData, tmp);
                            self.emit_jmp(Opcode::JmpIf, &dup_true);
                            self.emit_expr(right)?;
                            self.emit_jmp(Opcode::Jmp, &end);
                            self.place_label(&dup_true);
                            self.emit_simple(Opcode::PushIBool, 1);
                            self.place_label(&end);
                        }
                        Ok(IrType::Bool)
                    }
                    _ => {
                        let lt = self.emit_expr(left)?;
                        let rt = self.emit_expr(right)?;
                        let op_code = match op {
                            BinaryOp::Add => Opcode::Add,
                            BinaryOp::Sub => Opcode::Sub,
                            BinaryOp::Mul => Opcode::Mul,
                            BinaryOp::Div => Opcode::Div,
                            BinaryOp::Mod => {
                                return Err(CompileError::new(
                                    ErrorCode::EExcluded,
                                    "MOD not in IR v0.1 opcode set",
                                ));
                            }
                            BinaryOp::Xor => Opcode::Xor,
                            BinaryOp::Eq => Opcode::Eq,
                            BinaryOp::Ne => Opcode::Ne,
                            BinaryOp::Lt => Opcode::Lt,
                            BinaryOp::Le => Opcode::Le,
                            BinaryOp::Gt => Opcode::Gt,
                            BinaryOp::Ge => Opcode::Ge,
                            BinaryOp::And | BinaryOp::Or => unreachable!(),
                        };
                        // Insert CONV if needed for numeric mismatch (REAL promote)
                        if matches!(
                            op,
                            BinaryOp::Add
                                | BinaryOp::Sub
                                | BinaryOp::Mul
                                | BinaryOp::Div
                                | BinaryOp::Lt
                                | BinaryOp::Le
                                | BinaryOp::Gt
                                | BinaryOp::Ge
                                | BinaryOp::Eq
                                | BinaryOp::Ne
                        ) && lt != rt
                        {
                            // Simple: convert left already on stack under right — hard.
                            // Require matching types in v1.
                            if lt != rt {
                                return Err(CompileError::new(
                                    ErrorCode::EType,
                                    format!("type mismatch {lt:?} vs {rt:?}"),
                                ));
                            }
                        }
                        self.emit_simple(op_code, 0);
                        Ok(
                            if matches!(
                                op,
                                BinaryOp::Eq
                                    | BinaryOp::Ne
                                    | BinaryOp::Lt
                                    | BinaryOp::Le
                                    | BinaryOp::Gt
                                    | BinaryOp::Ge
                            ) {
                                IrType::Bool
                            } else {
                                lt
                            },
                        )
                    }
                }
            }
            Expr::Name { path, .. } => self.emit_load_path(path),
            Expr::FbCall { call, .. } => {
                self.emit_fb_call(call)?;
                // Expression form: leave first output on stack if prim — already stored.
                // Not supported as value expression without association — return BOOL false
                Err(CompileError::new(
                    ErrorCode::EType,
                    "FB call as expression value not supported; use statement + field read",
                ))
            }
        }
    }

    fn emit_load_path(&mut self, path: &[PathSeg]) -> Result<IrType, CompileError> {
        let (root, rest) = path_parts(path)?;
        let sym = self.lookup(&root)?.clone();
        if rest.is_empty() {
            return match sym.storage {
                Storage::Input { slot, ty } => {
                    self.emit_simple(Opcode::LdI, slot);
                    Ok(ty)
                }
                Storage::Output { slot, ty } => {
                    self.emit_simple(Opcode::LdQ, slot);
                    Ok(ty)
                }
                Storage::Data {
                    offset,
                    ty: TypeDef::Elem(t),
                } => {
                    self.emit_simple(Opcode::LdData, offset);
                    Ok(t)
                }
                Storage::Retain { offset, ty } => {
                    self.emit_simple(Opcode::LdRetain, offset);
                    Ok(ty)
                }
                Storage::Const { value, .. } => {
                    let e = Expr::Literal {
                        value,
                        span: Span::default(),
                    };
                    self.emit_expr(&e)
                }
                _ => Err(CompileError::new(
                    ErrorCode::EType,
                    format!("cannot read `{root}` as a value"),
                )),
            };
        }
        let field = match &rest[0] {
            PathSeg::Ident(f) => f.clone(),
            PathSeg::Index(idx) => {
                if let Storage::Data {
                    offset,
                    ty: TypeDef::Array { elem, .. },
                } = sym.storage
                {
                    let et = elem.ir_elem().unwrap();
                    let i = match idx {
                        Expr::Literal {
                            value: Literal::Int(i),
                            ..
                        } => *i as u32,
                        _ => {
                            return Err(CompileError::new(
                                ErrorCode::EType,
                                "array index must be constant in v1",
                            ));
                        }
                    };
                    self.emit_simple(Opcode::LdData, offset + i * et.byte_width() as u32);
                    return Ok(et);
                }
                return Err(CompileError::new(ErrorCode::EType, "not an array"));
            }
        };
        match sym.storage {
            Storage::Prim { out_offsets, .. } => {
                let (_n, off, ty) = out_offsets
                    .iter()
                    .find(|(n, _, _)| n.eq_ignore_ascii_case(&field))
                    .ok_or_else(|| {
                        CompileError::new(ErrorCode::EUndefined, format!("no field {field}"))
                    })?;
                self.emit_simple(Opcode::LdData, *off);
                Ok(*ty)
            }
            Storage::UserInst { fb, base } => {
                let (retain, retain_off, data_off, ty) = {
                    let layout = self.fb_layouts.get(&fb.to_ascii_lowercase()).unwrap();
                    let f = layout
                        .fields
                        .iter()
                        .find(|x| x.name.eq_ignore_ascii_case(&field))
                        .ok_or_else(|| {
                            CompileError::new(ErrorCode::EUndefined, format!("no field {field}"))
                        })?;
                    (
                        f.retain,
                        f.retain_offset,
                        base + f.offset,
                        f.ty.ir_elem().unwrap_or(IrType::Bool),
                    )
                };
                if retain {
                    self.emit_simple(Opcode::LdRetain, retain_off.unwrap());
                } else {
                    self.emit_simple(Opcode::LdData, data_off);
                }
                Ok(ty)
            }
            _ => Err(CompileError::new(ErrorCode::EType, "bad field load")),
        }
    }

    fn finish(
        mut self,
        data_size: u32,
        retain_size: u32,
        input_slots: u32,
        output_slots: u32,
    ) -> Result<IrModule, CompileError> {
        for (off, label) in &self.fixups {
            let pc = self.labels.get(label).ok_or_else(|| {
                CompileError::new(ErrorCode::ECodegen, format!("undefined label {label}"))
            })?;
            // patch payload of simple jmp at off
            let word = u32::from_le_bytes(self.code[*off..*off + 4].try_into().unwrap());
            let op = (word >> 24) as u8;
            let new = (u32::from(op) << 24) | (pc & 0x00FF_FFFF);
            self.code[*off..*off + 4].copy_from_slice(&new.to_le_bytes());
        }
        let data_size = data_size.max(self.extra_data).max(16);
        let retain_size = retain_size.max(self.extra_retain);
        Ok(IrModule {
            ir_major: IR_MAJOR,
            ir_minor: IR_MINOR,
            const_size: 0,
            data_size,
            retain_size,
            input_slots,
            output_slots,
            entries: self.entries,
            const_data: Vec::new(),
            code: self.code,
        })
    }
}

fn path_parts(path: &[PathSeg]) -> Result<(String, &[PathSeg]), CompileError> {
    match path.first() {
        Some(PathSeg::Ident(n)) => Ok((n.clone(), &path[1..])),
        _ => Err(CompileError::new(
            ErrorCode::EParse,
            "path must start with identifier",
        )),
    }
}
