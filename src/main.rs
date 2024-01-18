extern crate libc;

use std::{net::SocketAddr, sync::{Arc, Mutex}, path::PathBuf, os::unix::net::UnixStream};

use anyhow::anyhow;
use clap::Parser;
use dhttp::get_target_eid;
use http_types::StatusCode;
use libc::getuid;
use tide::Response;
use ud3tn_aap::{Agent, UnixAgent};
use tokio::task;

mod dhttp;

#[derive(Debug, Parser)]
#[command(author, version, about, long_about = None)]
struct CLI {
    #[arg(short, long, default_value = "127.0.0.1:8555")]
    bind: SocketAddr,

    #[arg(short = 'a', long, default_value = "dhttp/client")]
    send_agent_id: String,

    #[arg(short = 'A', long, default_value = "www")]
    agent_id: String,

    #[arg(short = 'S', long)]
    socket_path: Option<PathBuf>
}

#[tokio::main]
async fn main() {
    let cli = CLI::parse();

    let socket_path = match cli.socket_path {
        Some(val) => val,
        None => {
            match unsafe { getuid() } {
                0 => PathBuf::from("/run/archipel-core/archipel-core.socket"),
                uid => PathBuf::from(format!("/run/user/{}/archipel-core/archipel-core.socket", uid))
            }
        },
    };

    let agent_id = cli.send_agent_id;

    println!("Using Archipel Core socket {}", socket_path.to_string_lossy());

    let stream = task::spawn_blocking(||
        UnixStream::connect(socket_path) ).await.unwrap().unwrap();
    let send_agent = task::spawn_blocking(||
        Agent::connect(stream, agent_id) ).await.unwrap().unwrap();

    let state = Arc::new(State {
        destination_agent_id: cli.agent_id,
        send_agent: Mutex::new(send_agent)
    });

    let mut app = tide::with_state(state);
    
    app
        .at("/").all(handle_dtn_request)
        .at("*").all(handle_dtn_request)
        ;

    println!("Starting HTTP server on {}", cli.bind);
    app.listen(cli.bind).await.unwrap();
}

async fn handle_dtn_request(req: tide::Request<StateHandle>) -> Result<Response, http_types::Error>{
    let state = req.state().clone();
    
    let host = get_target_eid(&req.as_ref())
                .map(|it| {
                    // Redirect to our node if host is local
                    if it == "localhost" || it == "127.0.0.1" {
                        let agent = state.send_agent.lock();
                        let current_eid = agent.unwrap().node_eid.clone();
                        current_eid[6..current_eid.len()-1].to_owned()
                    } else { it }
                })
                .ok_or(Rejection::MissingHostHeader)?;

    let bundle_content = dhttp::from_http(req.into()).await
        .map_err(|it| {
            eprintln!("Failed to create a bundle for the provided request : {}", it);
            Rejection::InternalServerError
        })?;

    {
        state.send_agent.lock().unwrap().send_bundle(
            format!("dtn://{}/{}", host, state.destination_agent_id),
            &bundle_content
        )
        .map_err(|it| {
            eprintln!("Failed to send bundle for the provided request : {}", it);
            Rejection::InternalServerError
        })?;
    }

    Ok(Response::from("Bundle sent"))
}

enum Rejection {
    InternalServerError,
    MissingHostHeader
}

impl From<Rejection> for tide::Error {
    fn from(value: Rejection) -> Self {
        match value {
            
            Rejection::InternalServerError => tide::Error::new(
                StatusCode::InternalServerError,
                anyhow!("Internal server error")),

            Rejection::MissingHostHeader => tide::Error::new(
                StatusCode::BadRequest,
                anyhow!("Missing host header; Host header is required to route on DTN")
            ),

        }
    }
}


type StateHandle = Arc<State>;

struct State {
    pub destination_agent_id: String,
    pub send_agent: Mutex<UnixAgent>
}