//! Telvax runtime: a tree-walking interpreter over the typed HIR plus the std implementations.

// wgpu's types nest deeply enough that proving `Send` for the GPU handle overflows the default
// limit (a warning today, a hard error in a future compiler).
#![cfg_attr(feature = "gpu", recursion_limit = "256")]

pub mod alloc_count;
mod caps;
pub mod debug;
#[cfg(feature = "flight")]
mod flight;
#[cfg(feature = "gpu")]
pub mod gpu;
mod interp;
pub mod profile;
pub mod rich;
#[cfg(feature = "live")]
mod services;
mod show;
mod std_impl;
mod table;
mod tensor;
mod value;

pub use caps::{Capability, Caps};
pub use interp::{Device, EndpointError, EndpointReply, Executor, Interp, RuntimeError};
pub use std_impl::{counters, embed_text, set_embedded_files};
pub use table::{infer_schema, schema_source};
pub use value::{
    Db, EnumVal, MapKey, MapVal, Native, Query, RequestInfo, ResponseVal, StructVal, TableVal,
    TaskVal, TensorVal, Value, VecVal,
};
