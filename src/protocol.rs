use std::borrow::Cow;
use std::convert::Infallible;

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde::de::Unexpected;
use serde_json::Value;

#[derive(Debug, Copy, Clone)]
struct V2_0;

impl Serialize for V2_0 {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error> where S: Serializer {
        "2.0".serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for V2_0 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error> where D: Deserializer<'de> {
        let version = <&str>::deserialize(deserializer)?;
        if version == "2.0" {
            Ok(V2_0)
        } else {
            use serde::de::Error;
            Err(Error::invalid_value(Unexpected::Str(version), &"2.0"))
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Default, Eq, PartialEq, Ord, PartialOrd, Hash, Clone)]
#[serde(untagged)]
pub enum Id {
    Number(i64),
    String(String),
    Null,
    #[default]
    Notification,
}

impl Id {
    pub fn is_notification(&self) -> bool {
        matches!(self, Self::Notification)
    }
}

#[derive(Deserialize)]
pub struct Request {
    #[allow(unused)]
    jsonrpc: V2_0,
    pub method: String,
    pub params: Value,
    #[serde(default)]
    pub id: Id,
}

#[derive(Serialize)]
pub struct Response {
    jsonrpc: V2_0,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<Error>,

    pub id: Id,
}

impl Response {
    pub fn ok(id: Id, result: Value) -> Self {
        debug_assert!(!id.is_notification());
        Self { jsonrpc: V2_0, result: Some(result), error: None, id }
    }

    pub fn error(id: Id, error: Error) -> Self {
        debug_assert!(!id.is_notification());
        Self { jsonrpc: V2_0, result: None, error: Some(error), id }
    }
}

#[derive(Serialize)]
pub struct Error {
    pub code: i32,
    pub message: Cow<'static, str>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

impl From<Infallible> for Error {
    fn from(value: Infallible) -> Self {
        match value {}
    }
}

impl Error {
    pub const fn parse_error() -> Self {
        Self {
            code: -32700,
            message: Cow::Borrowed("Parse error"),
            data: None,
        }
    }

    pub const fn invalid_request() -> Self {
        Self {
            code: -32600,
            message: Cow::Borrowed("Invalid Request"),
            data: None,
        }
    }

    pub const fn method_not_found() -> Self {
        Self {
            code: -32601,
            message: Cow::Borrowed("Method not found"),
            data: None,
        }
    }

    pub const fn invalid_params() -> Self {
        Self {
            code: -32602,
            message: Cow::Borrowed("Invalid params"),
            data: None,
        }
    }

    pub const fn internal_error() -> Self {
        Self {
            code: -32603,
            message: Cow::Borrowed("Internal error"),
            data: None,
        }
    }

    pub const fn server_error() -> Self {
        Self {
            code: -32000,
            message: Cow::Borrowed("Server error"),
            data: None,
        }
    }
}
