# json rpc

JSON-RPC 2.0 server implementation in Rust

# Example

[full example](examples/hyper.rs)

```rust

#[json_rpc]
fn greet(name: String) -> Result<String, Infallible> {
    Ok(format!("Hello {}", name))
}

struct System;

#[json_rpc]
impl System {
    fn time() -> Result<u64, json_rpc_v2::Error> {
        SystemTime::now().duration_since(UNIX_EPOCH).map(|v| v.as_secs()).map_err(|_| json_rpc_v2::Error::server_error())
    }

    async fn issue() -> Result<String, json_rpc_v2::Error> {
        read_to_string("/etc/issue").await.map_err(|_| json_rpc_v2::Error::server_error())
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let mut registry = Registry::new();
    registry.register_method("greet", greet);
    registry.register::<System>();
    
    let _ = registry.call(br#"{"jsonrpc":"2.0","method":"greet","params":["foo"],"id":1}"#).await;
    let _ = registry.call(br#"{"jsonrpc":"2.0","method":"system.time","params":[],"id":1}"#).await;
    let _ = registry.call(br#"{"jsonrpc":"2.0","method":"system.issue","params":[],"id":1}"#).await;
    Ok(())
}
```