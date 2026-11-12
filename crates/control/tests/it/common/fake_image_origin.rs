use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio_rustls::TlsAcceptor;
use tokio_rustls::rustls::ServerConfig;
use tokio_rustls::rustls::pki_types::{CertificateDer, PrivateKeyDer, pem::PemObject};

pub fn certificate_path() -> String {
    format!("{}/tests/it/common/fake_image_cert.pem", env!("CARGO_MANIFEST_DIR"))
}

#[derive(Clone, Debug)]
pub struct Request {
    pub path: String,
    pub headers: Vec<(String, String)>,
}

impl Request {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(key, _)| key.eq_ignore_ascii_case(name)).map(|(_, value)| value.as_str())
    }
}

#[derive(Clone)]
pub struct Reply {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
    pub announced_length: Option<usize>,
}

impl Reply {
    pub fn png(width: u32, height: u32) -> Reply {
        Reply::bytes("image/png", png(width, height, 0))
    }

    pub fn bytes(content_type: &str, body: Vec<u8>) -> Reply {
        Reply { status: 200, headers: vec![("content-type".into(), content_type.into())], body, announced_length: None }
    }

    pub fn redirect(location: &str) -> Reply {
        Reply { status: 302, headers: vec![("location".into(), location.into())], body: Vec::new(), announced_length: None }
    }

    pub fn status(status: u16) -> Reply {
        Reply { status, headers: Vec::new(), body: Vec::new(), announced_length: None }
    }

    pub fn announcing(mut self, length: usize) -> Reply {
        self.announced_length = Some(length);

        self
    }
}

pub fn png(width: u32, height: u32, padding: usize) -> Vec<u8> {
    let mut bytes = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec();

    bytes.extend(width.to_be_bytes());
    bytes.extend(height.to_be_bytes());
    bytes.extend([8, 6, 0, 0, 0]);
    bytes.extend(vec![0u8; padding]);

    bytes
}

#[derive(Clone)]
pub struct Origin {
    pub port: u16,
    routes: Arc<Mutex<HashMap<String, Reply>>>,
    log: Arc<Mutex<Vec<Request>>>,
}

impl Origin {
    pub fn route(&self, path: &str, reply: Reply) {
        self.routes.lock().unwrap().insert(path.to_string(), reply);
    }

    pub fn requests(&self) -> Vec<Request> {
        self.log.lock().unwrap().clone()
    }

    pub fn count(&self) -> usize {
        self.log.lock().unwrap().len()
    }

    pub fn count_of(&self, path: &str) -> usize {
        self.log.lock().unwrap().iter().filter(|request| request.path == path).count()
    }
}

fn acceptor() -> TlsAcceptor {
    let certificates: Vec<CertificateDer<'static>> = CertificateDer::pem_file_iter(certificate_path()).unwrap().map(Result::unwrap).collect();
    let key = PrivateKeyDer::from_pem_file(format!("{}/tests/it/common/fake_image_key.pem", env!("CARGO_MANIFEST_DIR"))).unwrap();
    let provider = Arc::new(tokio_rustls::rustls::crypto::aws_lc_rs::default_provider());
    let config = ServerConfig::builder_with_provider(provider).with_safe_default_protocol_versions().unwrap().with_no_client_auth().with_single_cert(certificates, key).unwrap();

    TlsAcceptor::from(Arc::new(config))
}

pub async fn start() -> Origin {
    let acceptor = acceptor();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = Origin { port: listener.local_addr().unwrap().port(), routes: Arc::default(), log: Arc::default() };
    let serving = origin.clone();

    tokio::spawn(async move {
        loop {
            let (stream, _) = listener.accept().await.unwrap();
            let acceptor = acceptor.clone();
            let origin = serving.clone();

            tokio::spawn(async move {
                let Ok(mut secured) = acceptor.accept(stream).await else { return };
                let mut received = Vec::new();
                let mut buffer = [0u8; 2048];

                while !received.windows(4).any(|window| window == b"\r\n\r\n") {
                    let Ok(read) = secured.read(&mut buffer).await else { return };

                    if read == 0 {
                        return;
                    }

                    received.extend_from_slice(&buffer[..read]);
                }

                let text = String::from_utf8_lossy(&received).to_string();
                let mut lines = text.lines();
                let path = lines.next().unwrap_or_default().split(' ').nth(1).unwrap_or_default().to_string();
                let headers = lines.take_while(|line| !line.is_empty()).filter_map(|line| line.split_once(':')).map(|(key, value)| (key.trim().to_string(), value.trim().to_string())).collect();

                origin.log.lock().unwrap().push(Request { path: path.clone(), headers });

                let reply = origin.routes.lock().unwrap().get(&path).cloned().unwrap_or_else(|| Reply::status(404));
                let mut head = format!("HTTP/1.1 {} Reply\r\ncontent-length: {}\r\nconnection: close\r\n", reply.status, reply.announced_length.unwrap_or(reply.body.len()));

                for (key, value) in &reply.headers {
                    head.push_str(&format!("{key}: {value}\r\n"));
                }

                head.push_str("\r\n");
                let _ = secured.write_all(head.as_bytes()).await;
                let _ = secured.write_all(&reply.body).await;
                let _ = secured.shutdown().await;
            });
        }
    });

    origin
}
