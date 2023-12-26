use futures_lite::AsyncReadExt;
use http_types::{Request, Error};
use async_h1::client::Encoder;

pub fn get_target_eid(req: &Request) -> Option<String> {
    let host = req.host()?.to_owned();

    match host.split_once(":") {
        Some((host, _port)) => Some(host.to_owned()),
        None => Some(host),
    }
}

pub async fn from_http(mut req: Request) -> Result<Vec<u8>, Error> {
    let body = req.body_bytes().await?;

    let mut dtn_req: Request = req.into();
    dtn_req.set_body(body);

    let target_eid = get_target_eid(&dtn_req);
    dtn_req.remove_header("Forwarded");
    dtn_req.remove_header("X-Forwarded-Host");
    dtn_req.remove_header("Host");

    if let Some(eid) = target_eid {
        dtn_req.insert_header("Host", eid);
    }

    let mut encoder = Encoder::new(dtn_req);
    let mut buf:Vec<u8> = Vec::new();
    encoder.read_to_end(&mut buf).await.unwrap();

    Ok(buf)
}