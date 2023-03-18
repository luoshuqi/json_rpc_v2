use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;

use log::error;
use serde::Serialize;
use serde_json::{from_slice, Value};
use tokio::spawn;
use tokio::sync::mpsc::unbounded_channel;

use crate::{Error, Id, Request, Response};

pub type Method = fn(Value) -> Pin<Box<dyn Future<Output=Result<Value, Error>> + Send>>;

pub trait Provider {
    fn methods() -> &'static [(&'static str, Method)];
}

pub struct Registry {
    methods: HashMap<&'static str, Method>,
}

impl Registry {
    pub fn new() -> Registry {
        Self { methods: HashMap::new() }
    }

    pub fn register<T: Provider>(&mut self) {
        for &(name, method) in T::methods() {
            self.methods.insert(name, method);
        }
    }

    pub fn register_method(&mut self, name: &'static str, method: Method) {
        self.methods.insert(name, method);
    }

    pub async fn call(&self, req: &[u8]) -> Option<Value> {
        match req {
            [b'{', ..] => match from_slice(req) {
                Ok(req) => self.call_one(req).await.map(to_value),
                Err(err) => {
                    error!("parse request error: {}", err);
                    Some(parse_error())
                }
            }
            [b'[', ..] => match from_slice(req) {
                Ok(req) => {
                    let response = self.call_batch(req).await;
                    match response.is_empty() {
                        false => Some(to_value(response)),
                        true => None,
                    }
                }
                Err(err) => {
                    error!("parse batch request error: {}", err);
                    Some(parse_error())
                }
            }
            _ => {
                error!("invalid request, expected '{{' or '['");
                Some(invalid_request())
            }
        }
    }

    fn get_method(&self, method: &str) -> Option<Method> {
        match self.methods.get(method) {
            Some(method) => Some(*method),
            None => {
                error!("method {} not found", method);
                None
            }
        }
    }

    async fn call_one(&self, req: Request) -> Option<Response> {
        match self.get_method(&req.method) {
            Some(method) => match method(req.params).await {
                Ok(result) if !req.id.is_notification() => Some(Response::ok(req.id, result)),
                Err(err) if !req.id.is_notification() => Some(Response::error(req.id, err)),
                _ => None,
            },
            None if !req.id.is_notification() => Some(Response::error(req.id, Error::method_not_found())),
            None => None,
        }
    }

    async fn call_batch(&self, batch_req: Vec<Request>) -> Vec<Response> {
        let mut response = Vec::with_capacity(batch_req.len());
        let mut wait = 0;
        let (tx, mut rx) = unbounded_channel();

        for req in batch_req {
            match self.get_method(&req.method) {
                Some(method) if !req.id.is_notification() => {
                    wait += 1;
                    let tx = tx.clone();
                    let _ = spawn(async move {
                        tx.send(match method(req.params).await {
                            Ok(result) => Response::ok(req.id, result),
                            Err(err) => Response::error(req.id, err),
                        })
                    });
                }
                Some(method) => {
                    let _ = spawn(async move { method(req.params); });
                }
                None if !req.id.is_notification() => response.push(Response::error(req.id, Error::method_not_found())),
                None => {}
            }
        }

        while wait > 0 {
            match rx.recv().await {
                Some(v) => response.push(v),
                None => break,
            }
            wait -= 1
        }

        response
    }
}

fn invalid_request() -> Value {
    to_value(Response::error(Id::Null, Error::invalid_request()))
}

fn parse_error() -> Value {
    to_value(Response::error(Id::Null, Error::parse_error()))
}

fn to_value(v: impl Serialize) -> Value {
    serde_json::to_value(v).unwrap()
}

#[cfg(test)]
mod tests {
    use std::future::Future;
    use std::pin::Pin;
    use std::sync::Once;

    use serde_json::{to_string, Value};
    use tokio::runtime::{Builder, Runtime};

    use crate::{Error, Registry};

    static mut ENV: Option<Env> = None;
    static ONCE: Once = Once::new();

    struct Env {
        runtime: Runtime,
        registry: Registry,
    }

    fn env() -> &'static Env {
        ONCE.call_once(|| {
            env_logger::init();
            let runtime = Builder::new_current_thread().build().unwrap();
            let mut registry = Registry::new();
            registry.register_method("sum", sum);
            unsafe { ENV = Some(Env { runtime, registry }); }
        });
        unsafe { ENV.as_ref().unwrap() }
    }

    fn sum(args: Value) -> Pin<Box<dyn Future<Output=Result<Value, Error>> + Send>> {
        Box::pin(async move {
            let (a, b) = match args {
                Value::Array(args) if args.len() >= 2 => {
                    let a = args[0].as_i64();
                    let b = args[1].as_i64();
                    (a, b)
                }
                Value::Object(args) => {
                    let a = args.get("a").and_then(Value::as_i64);
                    let b = args.get("b").and_then(Value::as_i64);
                    (a, b)
                }
                _ => return Err(Error::invalid_params()),
            };
            if a.is_some() || b.is_some() {
                Ok(Value::from(a.unwrap() + b.unwrap()))
            } else {
                Err(Error::invalid_params())
            }
        })
    }


    #[test]
    fn test_by_position_parameter() {
        let req = br#"{"jsonrpc":"2.0","method":"sum","params":[3,4],"id":1}"#;
        let result = env().runtime.block_on(env().registry.call(req));
        assert_eq!(to_string(result.as_ref().unwrap()).unwrap(), r#"{"id":1,"jsonrpc":"2.0","result":7}"#);
    }

    #[test]
    fn test_by_name_parameter() {
        let req = br#"{"jsonrpc":"2.0","method":"sum","params":{"a":3,"b":4},"id":1}"#;
        let result = env().runtime.block_on(env().registry.call(req));
        assert_eq!(to_string(result.as_ref().unwrap()).unwrap(), r#"{"id":1,"jsonrpc":"2.0","result":7}"#);
    }

    #[test]
    fn test_batch_call() {
        let req = br#"[{"jsonrpc":"2.0","method":"sum","params":[3,4],"id":1},{"jsonrpc":"2.0","method":"sum","params":[3,4],"id":2}]
"#;
        let result = env().runtime.block_on(env().registry.call(req));
        assert_eq!(to_string(result.as_ref().unwrap()).unwrap(), r#"[{"id":1,"jsonrpc":"2.0","result":7},{"id":2,"jsonrpc":"2.0","result":7}]"#);
    }

    #[test]
    fn test_method_not_found() {
        let req = br#"{"jsonrpc":"2.0","method":"sum1","params":[3,4],"id":1}"#;
        let result = env().runtime.block_on(env().registry.call(req));
        assert_eq!(to_string(result.as_ref().unwrap()).unwrap(), r#"{"error":{"code":-32601,"message":"Method not found"},"id":1,"jsonrpc":"2.0"}"#);
    }

    #[test]
    fn test_invalid_json() {
        let req = br#"{1"jsonrpc":"1.0","method":"sum1","params":[3,4],"id":1}"#;
        let result = env().runtime.block_on(env().registry.call(req));
        assert_eq!(to_string(result.as_ref().unwrap()).unwrap(), r#"{"error":{"code":-32700,"message":"Parse error"},"id":null,"jsonrpc":"2.0"}"#);
    }

    #[test]
    fn test_invalid_id() {
        let req = br#"{"jsonrpc":"2.0","method":"sum1","params":[3,4],"id":1.1}"#;
        let result = env().runtime.block_on(env().registry.call(req));
        assert_eq!(to_string(result.as_ref().unwrap()).unwrap(), r#"{"error":{"code":-32700,"message":"Parse error"},"id":null,"jsonrpc":"2.0"}"#);
    }

    #[test]
    fn test_invalid_version() {
        let req = br#"{"jsonrpc":"2.1","method":"sum1","params":[3,4],"id":1.1}"#;
        let result = env().runtime.block_on(env().registry.call(req));
        assert_eq!(to_string(result.as_ref().unwrap()).unwrap(), r#"{"error":{"code":-32700,"message":"Parse error"},"id":null,"jsonrpc":"2.0"}"#);
    }
}