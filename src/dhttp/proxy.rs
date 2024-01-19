use std::{sync::Arc, net::SocketAddr};

use anyhow::anyhow;
use async_std::{sync::Mutex, io};
use http_types::StatusCode;
use tide::Response;
use ud3tn_aap::UnixAgent;

use super::{get_target_eid, from_http};

/// HTTP Proxy routing http request as bundle
pub struct Proxy {
    server: tide::Server<StateHandle>
}

struct State {
    pub destination_agent_id: String,
    pub send_agent: Mutex<UnixAgent>
}

type StateHandle = Arc<State>;

impl Proxy {

    /// Create a new Proxy using the defined agent
    pub fn new(send_agent: UnixAgent, destination_agent_id: String) -> Self {
        let state = Arc::new(State {
            destination_agent_id,
            send_agent: Mutex::new(send_agent),
        });

        let mut server = tide::with_state(state);
    
        server.at("/").all(Proxy::handle_dtn_request)
              .at("*").all(Proxy::handle_dtn_request)
              ;

        Self {
            server
        }
    }

    /// Bind proxy and wait for HTTP connection
    pub async fn bind(self, addr: SocketAddr) -> io::Result<()> {
        self.server.listen(addr).await
    }

    async fn handle_dtn_request(req: tide::Request<StateHandle>) -> Result<Response, http_types::Error>{
        let state = req.state().clone();
        
        let host = get_target_eid(&req.as_ref())
                    .map(|it| async {
                        // Redirect to our node if host is local
                        if it == "localhost" || it == "127.0.0.1" {
                            let agent = state.send_agent.lock().await;
                            let current_eid = agent.node_eid.clone();
                            current_eid[6..current_eid.len()-1].to_owned()
                        } else { it }
                    })
                    .ok_or(Rejection::MissingHostHeader)?.await;
    
        let bundle_content = from_http(req.into()).await
            .map_err(|it| {
                eprintln!("Failed to create a bundle for the provided request : {}", it);
                Rejection::InternalServerError
            })?;
    
        {
            state.send_agent.lock().await.send_bundle(
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