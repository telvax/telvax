//! High-Level IR: name resolution, type checking and tensor-shape checking.

mod check;
pub mod coverage;
pub mod hir;
mod named;
pub mod stdlib;
pub mod stdlib_docs;
pub mod ty;

pub use ty::{Dim, Prim, Ty};

/// Numeric precision used for `std.tensor.matmul` on `f32` tensors.
///
/// `Int8` is symmetric per-tensor dynamic quantization: both operands are scaled by
/// `max|x| / 127`, rounded to `i8` (ties away from zero), multiplied with `i32` accumulation and
/// scaled back. The interpreter and the native backend implement exactly the same arithmetic, so
/// their results agree bit for bit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Quantization {
    #[default]
    None,
    Int8,
    /// Int8 with one scale per row of the left operand and one per column of the right one
    /// (`--quantize int8-channel`). More accurate than [`Quantization::Int8`] when columns differ
    /// in magnitude. Interpreter only.
    Int8Channel,
    /// Like `Int8Channel`, but the weights (the right operand) use 4 bits: values in `-7..=7`
    /// (`--quantize int4-channel`). The activations stay 8-bit. Interpreter only.
    Int4Channel,
}

use telvax_diagnostics::Diagnostic;

/// The syntax edition a program is read in (`edition` in `telvax.toml`). See `docs/guide/stability.md`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Edition {
    /// The stable edition, and the one a program without a manifest is read in.
    #[default]
    E2026,
    /// A preview of the next edition, only when a package asks for it: it is not stable and may change or go.
    /// Differences from 2026: `std.string` is gone (its functions are in `std.text`).
    E2027Preview,
}

impl Edition {
    /// The edition of an `edition = "..."` line, or `None` for one this compiler does not read.
    pub fn from_name(name: &str) -> Option<Edition> {
        match name {
            "2026" => Some(Edition::E2026),
            "2027-preview" => Some(Edition::E2027Preview),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Edition::E2026 => "2026",
            Edition::E2027Preview => "2027-preview",
        }
    }
}

/// Checks a parsed program, returning its typed HIR and every diagnostic found.
/// The HIR is always produced so later tooling can work on partially valid code.
pub fn lower(program: &telvax_ast::Program) -> (hir::Program, Vec<Diagnostic>) {
    lower_in(program, Edition::default())
}

/// [`lower`] for a program of `edition`.
pub fn lower_in(
    program: &telvax_ast::Program,
    edition: Edition,
) -> (hir::Program, Vec<Diagnostic>) {
    telvax_ast::stack::with_big_stack(|| {
        // Named arguments and defaults are settled first, so the checker sees plain calls.
        let mut program = program.clone();
        let mut diags = named::resolve(&mut program);
        let (hir, more) = check::lower(&program, edition);
        diags.extend(more);
        (hir, diags)
    })
}
