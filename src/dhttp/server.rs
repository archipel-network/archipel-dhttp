use std::{path::PathBuf, sync::{Arc, Mutex}, io};

use async_std::{task, io::ReadExt};
use futures_lite::{AsyncRead, AsyncWrite};
use tide::{http::Request, Response};
use ud3tn_aap::UnixAgent;

/// Static files DHTTP server
pub struct Server {
    pub root: PathBuf,
    pub server: tide::Server<()>,
    agent: Arc<Mutex<UnixAgent>>
}

impl Server {
    pub fn new(agent: UnixAgent, root: PathBuf) -> Result<Self, io::Error> {
        let mut server = tide::new();

        server.at("").serve_dir(root.clone())?;

        Ok(Self { server, agent: Arc::new(Mutex::new(agent)), root })
    }

    pub async fn bind(self) {
        let server = Arc::new(self.server);

        loop {
            let agent = self.agent.clone();
            let bundle = match task::spawn_blocking(move || {
                agent.lock().unwrap().recv_bundle()
            }).await {
                Ok(b) => b,
                Err(e) => {
                    eprintln!("Error receiving bundle {}", e);
                    continue;
                },
            };
            let this_server = server.clone();
            task::spawn(Self::handle_bundle_reception(this_server, bundle, self.agent.clone()));
        }
    }

    async fn handle_bundle_reception(
        server: Arc<tide::Server<()>>,
        bundle: (String, Vec<u8>),
        send_agent: Arc<Mutex<UnixAgent>>) {

        let (source, content) = bundle;
        let decode_result = async_h1::server::decode(
            RequestContent(content)
        ).await;

        let request = match decode_result {
            Ok(Some((mut req, mut body_reader))) => {
                let mut body = Vec::new();
                body_reader.read_to_end(&mut body).await.unwrap();
                req.set_body(body);
                req
            },
            Ok(None) => return,
            Err(e) => {
                eprintln!("Error handling request : {}", e);
                return;
            },
        };

        let method = request.method();
        let url = request.url().clone();

        let response = match server.respond::<Request, Response>(request).await {
            Ok(r) => r,
            Err(e) => {
                eprintln!("Error handling request in server : {}", e);
                return
            },
        };

        println!("[{}] {} {}", source, url, response.status());

        let mut response_encoder = async_h1::server::Encoder::new(response.into(), method);
        let mut response_bytes = Vec::new();
        let encode_result = response_encoder.read_to_end(&mut response_bytes).await;

        match encode_result {
            Ok(r) => r,
            Err(e) => {
                eprintln!("Fail to encode resulting response : {}", e);
                return
            },
        };

        task::spawn_blocking(move || {
            send_agent.lock().unwrap()
                .send_bundle(source, &response_bytes)
        });
    }
}

#[derive(Debug, Clone)]
struct RequestContent(pub Vec<u8>);

impl AsyncRead for RequestContent {
    fn poll_read(
        self: std::pin::Pin<&mut Self>,
        _cx: &mut task::Context<'_>,
        buf: &mut [u8],
    ) -> task::Poll<std::io::Result<usize>> {
        let range = 0..buf.len().min(self.0.len());
        let result = self.get_mut().0.drain(range.clone()).collect::<Vec<_>>();
        buf[range].copy_from_slice(&result);
        task::Poll::Ready(Ok(result.len()))
    }
}

impl AsyncWrite for RequestContent {
    fn poll_write(
        self: std::pin::Pin<&mut Self>,
        _cx: &mut task::Context<'_>,
        buf: &[u8],
    ) -> task::Poll<std::io::Result<usize>> {
        task::Poll::Ready(Ok(buf.len()))
    }

    fn poll_flush(self: std::pin::Pin<&mut Self>, _cx: &mut task::Context<'_>) -> task::Poll<std::io::Result<()>> {
        task::Poll::Ready(Ok(()))
    }

    fn poll_close(self: std::pin::Pin<&mut Self>, _cx: &mut task::Context<'_>) -> task::Poll<std::io::Result<()>> {
        task::Poll::Ready(Ok(()))
    }
}