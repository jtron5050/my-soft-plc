# plc-compiler

Host-side compiler for the Soft PLC **Appendix B** ST-subset → IR v0.1 → `.spkg`.

This crate is **non-RT**. Controllers receive closed packages only; they never parse ST.

## Language surface

Acceptance is against [`docs/architecture.md`](../../docs/architecture.md) Appendix B.
Anything outside that allowlist is a hard compile error with a stable error code.

### Project binding (required for I/O fixtures)

I/O and retain tags are declared in ST with `AT %I` / `%Q` / `%M` / `%R` and listed in
`project.toml` so the compiler can emit `LD_I` / `ST_Q` / `LD_RETAIN` and the package
tag dictionary. Quality gates use the built-in `Q_GOOD(input_tag)`.

See sample projects under `samples/programs/*/project.toml`.

## CLI

```bash
cargo run -p plc-compiler -- compile samples/programs/demo-conveyor/project.toml \
  -o /tmp/demo-conveyor.spkg --emit-spasm /tmp/demo-conveyor.spasm
```

Optional `--sign <ed25519-seed.hex>` produces a signed package; default is unsigned
(all-zero signature sentinel for `require_signature=false` profiles).

## Library API

`compile_project(path, &CompileOptions) -> Result<CompileOutput, CompileError>`
builds a verified `IrModule`, manifest, and `.spkg` bytes.
