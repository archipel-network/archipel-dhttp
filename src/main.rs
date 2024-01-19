extern crate libc;

use std::{net::SocketAddr, path::PathBuf, os::unix::net::UnixStream};

use clap::Parser;
use libc::getuid;
use ud3tn_aap::Agent;
use async_std::task;

mod dhttp;
use dhttp::Proxy;
use dhttp::Server;

#[derive(Debug, Parser)]
#[command(author, version, about, long_about = None)]
struct CLI {
    #[arg(short, long, default_value = "127.0.0.1:8555")]
    bind: SocketAddr,

    #[arg(short = 'a', long, default_value = "dhttp/client")]
    send_agent_id: String,

    #[arg(short = 'A', long, default_value = "www")]
    agent_id: String,

    #[arg(short = 'F', long = "files", default_value = "./www")]
    files_root: PathBuf,

    #[arg(short = 'S', long)]
    socket_path: Option<PathBuf>
}

#[async_std::main]
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

    let agent_id = cli.send_agent_id.clone();

    println!("Using Archipel Core socket {}", socket_path.to_string_lossy());

    let send_socket_path = socket_path.clone();

    let send_stream = task::spawn_blocking(||
        UnixStream::connect(send_socket_path) ).await.unwrap();

    let send_agent = task::spawn_blocking(move ||
        Agent::connect(send_stream, agent_id) ).await.unwrap();


    let receive_socket_path = socket_path.clone();
    let www_agent_id = cli.agent_id.clone();

    let receive_stream = task::spawn_blocking(||
        UnixStream::connect(receive_socket_path) ).await.unwrap();
        
    let receive_agent = task::spawn_blocking(move || {
        Agent::connect(receive_stream, www_agent_id)
    }).await.unwrap();

    let server = Server::new(receive_agent, cli.files_root.clone())
        .unwrap();
    task::spawn(server.bind());

    let proxy = Proxy::new(send_agent, cli.agent_id.clone());
    println!("Starting proxy on http://{}/", cli.bind);
    proxy.bind(cli.bind).await.unwrap();

}