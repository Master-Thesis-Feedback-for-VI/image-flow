//! Small loopback-only HTTP fixture. No external service or extra dependency is needed.
use std::{sync::Arc, time::Duration};

use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
    task::JoinHandle,
    time::timeout,
};

use crate::requesters::AuthToHost;

pub struct TestServer {
    pub url: String,
    task: JoinHandle<String>,
}

impl TestServer {
    pub async fn new(status: &str, body: &str) -> Self {
        let response = format!(
            "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        Self::raw_response(response).await
    }

    pub async fn raw_response(response: String) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind fixture");
        let url = format!(
            "http://{}",
            listener
                .local_addr()
                .expect("fixture address")
        );
        let task = tokio::spawn(async move {
            let (mut socket, _) = timeout(Duration::from_secs(5), listener.accept())
                .await
                .expect("fixture accept timeout")
                .expect("accept fixture request");
            let mut request = Vec::new();
            loop {
                let mut buffer = [0; 4096];
                let read = timeout(Duration::from_secs(5), socket.read(&mut buffer))
                    .await
                    .expect("fixture read timeout")
                    .expect("read fixture request");
                assert!(read > 0, "connection closed before complete request");
                request.extend_from_slice(&buffer[..read]);
                if let Some(end) = request
                    .windows(4)
                    .position(|w| w == b"\r\n\r\n")
                {
                    let headers = String::from_utf8_lossy(&request[..end]);
                    let length = headers
                        .lines()
                        .find_map(|line| {
                            let (name, value) = line.split_once(':')?;
                            name.eq_ignore_ascii_case("content-length")
                                .then(|| {
                                    value
                                        .trim()
                                        .parse::<usize>()
                                        .expect("request length")
                                })
                        })
                        .unwrap_or(0);
                    if request.len() >= end + 4 + length {
                        break;
                    }
                }
            }
            socket
                .write_all(response.as_bytes())
                .await
                .expect("send fixture response");
            String::from_utf8(request).expect("UTF-8 fixture request")
        });
        Self { url, task }
    }

    pub fn auth(&self) -> Arc<AuthToHost> {
        Arc::new(AuthToHost {
            overpass_url: self
                .url
                .clone(),
            nominatim_url: self
                .url
                .clone(),
            user: "test".into(),
            passwd: "secret".into(),
        })
    }

    pub async fn request(&mut self) -> String {
        timeout(Duration::from_secs(5), &mut self.task)
            .await
            .expect("fixture completion timeout")
            .expect("fixture task")
    }
}

impl Drop for TestServer {
    fn drop(&mut self) {
        self.task
            .abort();
    }
}
