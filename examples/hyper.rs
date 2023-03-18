use std::convert::Infallible;
use std::error::Error;
use std::net::SocketAddr;
use std::time::{SystemTime, UNIX_EPOCH};

use hyper::{Body, Request, Response, Server};
use hyper::body::to_bytes;
use hyper::http::HeaderValue;
use hyper::service::{make_service_fn, service_fn};
use serde_json::to_string;
use tokio::fs::read_to_string;

use json_rpc_v2::{json_rpc, Registry};

static mut APP: Option<App> = None;

fn app() -> &'static App {
    unsafe {
        debug_assert!(APP.is_some());
        APP.as_ref().unwrap()
    }
}

struct App {
    registry: Registry,
}

fn init() {
    env_logger::init();

    let mut registry = Registry::new();
    registry.register_method("greet", greet);
    registry.register::<System>();
    unsafe { APP = Some(App { registry }); }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    init();

    let addr = SocketAddr::from(([127, 0, 0, 1], 8888));
    println!("server started at http://{}", addr);
    println!("try:");
    println!(r#"curl http://{} -d '{{"jsonrpc":"2.0","method":"greet","params":["foo"],"id":1}}'"#, addr);
    println!(r#"curl http://{} -d '{{"jsonrpc":"2.0","method":"system.time","params":[],"id":1}}'"#, addr);
    println!(r#"curl http://{} -d '{{"jsonrpc":"2.0","method":"system.issue","params":[],"id":1}}'"#, addr);

    let make_svc = make_service_fn(|_conn| async {
        Ok::<_, Infallible>(service_fn(json_rpc))
    });
    let server = Server::bind(&addr).serve(make_svc);
    if let Err(e) = server.await {
        eprintln!("server error: {}", e);
    }
    Ok(())
}

async fn json_rpc(req: Request<Body>) -> Result<Response<Body>, hyper::Error> {
    match app().registry.call(&to_bytes(req.into_body()).await?).await {
        Some(response) => {
            let mut response = Response::new(to_string(&response).unwrap().into());
            response.headers_mut().insert("content-type", HeaderValue::from_static("application/json"));
            Ok(response)
        }
        None => Ok(Response::new("".into())),
    }
}

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