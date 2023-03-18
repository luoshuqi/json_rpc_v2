pub use log::error;
pub use serde;
pub use serde_json;

pub use json_rpc_v2_macro::json_rpc;
pub use protocol::*;
pub use registry::*;

mod protocol;
mod registry;