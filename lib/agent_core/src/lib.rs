// Transport-independent action contracts and bounded JSON. New shared implementation;
// no corresponding module exists in the frozen assembly reference.
#![no_std]
#![forbid(unsafe_code)]

extern crate alloc;

pub mod action;
pub mod json;
pub mod rpc;
pub mod schema;
pub mod task;
mod mcp;

pub use action::{ActionContext, ActionError, ActionResult, ActionSpec, BatchCall, BatchResult,
    Dispatcher, ErrorCode, Host, Invocation, Page, Resource};
pub use json::Value;
pub use rpc::{Protocol, RpcSession};
