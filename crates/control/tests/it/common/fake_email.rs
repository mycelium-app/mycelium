use base64::Engine;
use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};
use tokio_rustls::TlsAcceptor;
use tokio_rustls::rustls::pki_types::{CertificateDer, PrivateKeyDer, pem::PemObject};
use tokio_rustls::rustls::ServerConfig;

pub const USERNAME: &str = "ada@example.com";
pub const PASSWORD: &str = "app-password-0123";

const FIRST_UID: u32 = 5;
const SECOND_UID: u32 = 7;

#[derive(Default)]
pub struct FakeMail {
    pub imap_commands: Mutex<Vec<String>>,
    pub sent: Mutex<Vec<String>>,
    pub attachment_bytes: Mutex<usize>,
    pub capabilities: Mutex<Option<String>>,
    pub appended: Mutex<Vec<(String, Vec<u8>)>>,
    pub removed: Mutex<Vec<u32>>,
    pub moved: Mutex<Vec<(u32, String)>>,
    pub marked: Mutex<Vec<u32>>,
    pub reject_append: AtomicBool,
    pub reject_delete: AtomicBool,
    pub hang_delete: AtomicBool,
    pub delete_reached: tokio::sync::Notify,
    pub reported_size: Mutex<Option<u32>>,
    pub transfer_overrun: AtomicBool,
}

pub fn first_message() -> String {
    [
        "From: Grace Hopper <grace@example.com>",
        "To: Ada <ada@example.com>",
        "Cc: Alan <alan@example.com>",
        "Subject: Lunch plans",
        "Date: Mon, 20 Oct 2025 12:00:00 +0000",
        "Message-ID: <m5@example.com>",
        "References: <m1@example.com>",
        "MIME-Version: 1.0",
        "Content-Type: multipart/mixed; boundary=\"b\"",
        "",
        "--b",
        "Content-Type: text/plain; charset=utf-8",
        "",
        "Noon at the usual place?",
        "--b",
        "Content-Type: application/pdf; name=\"menu.pdf\"",
        "Content-Disposition: attachment; filename=\"menu.pdf\"",
        "Content-Transfer-Encoding: base64",
        "",
        "aGVsbG8gd29ybGQh",
        "--b--",
        "",
    ]
    .join("\r\n")
}

fn second_message() -> String {
    ["From: Bob <bob@example.com>", "Subject: Invoice", "Date: Tue, 21 Oct 2025 09:30:00 +0000", "Message-ID: <m7@example.com>", "Content-Type: text/html; charset=utf-8", "Content-Transfer-Encoding: base64", "", "PHA+UGxlYXNlIHBheS48L3A+", ""].join("\r\n")
}

fn message(uid: u32) -> String {
    if uid == FIRST_UID { first_message() } else { second_message() }
}

fn headers_of(message: &str) -> String {
    let end = message.find("\r\n\r\n").unwrap() + 4;

    message[..end].to_string()
}

async fn reply<S: AsyncWrite + Unpin>(stream: &mut S, text: &str) {
    stream.write_all(text.as_bytes()).await.unwrap();
}

fn fetched(sequence: usize, uid: u32, query: &str) -> String {
    let message = message(uid);
    let mut fields = format!("UID {uid} INTERNALDATE \"20-Oct-2025 12:00:00 +0000\"");

    if query.contains("BODYSTRUCTURE") {
        let structure = if uid == FIRST_UID {
            r#"(("TEXT" "PLAIN" ("CHARSET" "UTF-8") NIL NIL "7BIT" 23 1)("APPLICATION" "PDF" ("NAME" "menu.pdf") NIL NIL "BASE64" 16 NIL ("ATTACHMENT" ("FILENAME" "menu.pdf"))) "MIXED")"#
        } else {
            r#"("TEXT" "HTML" ("CHARSET" "UTF-8") NIL NIL "BASE64" 24 1)"#
        };
        fields.push_str(&format!(" BODYSTRUCTURE {structure}"));
    }

    if query.contains("RFC822.SIZE") {
        fields.push_str(&format!(" RFC822.SIZE {}", message.len()));
    }

    if query.contains("FLAGS") {
        fields.push_str(" FLAGS (\\Seen \\Flagged \\Recent custom)");
    }

    let mut literals = Vec::new();

    if query.contains("BODY.PEEK[]") {
        literals.push((if query.contains("<0.") { "BODY[]<0>" } else { "BODY[]" }, message.clone()));
    }

    if query.contains("HEADER") {
        literals.push(("BODY[HEADER]", headers_of(&message)));
    }

    if query.contains("BODY.PEEK[1.MIME]") {
        let mime = if uid == FIRST_UID { "Content-Type: text/plain; charset=utf-8\r\n\r\n" } else { "Content-Type: text/html; charset=utf-8\r\nContent-Transfer-Encoding: base64\r\n\r\n" };
        literals.push(("BODY[1.MIME]", mime.to_string()));
    }

    if query.contains("BODY.PEEK[1]") {
        let text = if uid == FIRST_UID { "Noon at the usual place?" } else { "PHA+UGxlYXNlIHBheS48L3A+" };
        literals.push(("BODY[1]<0>", text.to_string()));
    }

    for (label, data) in literals {
        fields.push_str(&format!(" {label} {{{}}}\r\n{data}", data.len()));
    }

    format!("* {sequence} FETCH ({fields})\r\n")
}

async fn imap_connection<S: AsyncRead + AsyncWrite + Unpin>(stream: S, mail: Arc<FakeMail>) {
    let mut stream = stream;

    reply(&mut stream, "* OK fake ready\r\n").await;

    let (read, mut write) = tokio::io::split(stream);
    let mut reader = BufReader::new(read);
    let mut line = String::new();

    loop {
        line.clear();

        if reader.read_line(&mut line).await.unwrap_or(0) == 0 {
            return;
        }

        let line = line.trim_end_matches(['\r', '\n']).to_string();
        mail.imap_commands.lock().unwrap().push(line.clone());

        let mut parts = line.splitn(3, ' ');
        let tag = parts.next().unwrap_or_default().to_string();
        let command = parts.next().unwrap_or_default().to_uppercase();
        let rest = parts.next().unwrap_or_default().to_string();
        let response = match command.as_str() {
            "LOGIN" if rest == format!("{USERNAME:?} {PASSWORD:?}") => format!("{tag} OK LOGIN completed\r\n"),
            "LOGIN" => format!("{tag} NO [AUTHENTICATIONFAILED] Authentication failed\r\n"),
            "CAPABILITY" => {
                let capabilities = mail.capabilities.lock().unwrap().clone().unwrap_or_else(|| "IMAP4rev1 MOVE UIDPLUS".to_string());

                format!("* CAPABILITY {capabilities}\r\n{tag} OK done\r\n")
            }
            "LIST" => format!("* LIST (\\HasNoChildren) \"/\" \"INBOX\"\r\n* LIST (\\HasNoChildren) \"/\" \"Sent\"\r\n{tag} OK done\r\n"),
            "EXAMINE" | "SELECT" if rest == "\"INBOX\"" || rest == "\"Sent\"" => format!("* 2 EXISTS\r\n* 0 RECENT\r\n* OK [UIDVALIDITY 1] ok\r\n{tag} OK [READ-ONLY] done\r\n"),
            "EXAMINE" | "SELECT" => format!("{tag} NO no such folder\r\n"),
            "FETCH" => format!("{}{}{tag} OK done\r\n", fetched(1, FIRST_UID, "HEADER"), fetched(2, SECOND_UID, "HEADER")),
            "APPEND" => {
                if mail.reject_append.load(Ordering::SeqCst) {
                    format!("{tag} NO destination full\r\n")
                } else {
                    let length = rest.rsplit_once('{').unwrap().1.trim_end_matches('}').parse::<usize>().unwrap();
                    write.write_all(b"+ ready\r\n").await.unwrap();
                    let mut bytes = vec![0; length];
                    reader.read_exact(&mut bytes).await.unwrap();
                    let mut newline = [0; 2];
                    reader.read_exact(&mut newline).await.unwrap();
                    mail.appended.lock().unwrap().push((rest.clone(), bytes));

                    format!("{tag} OK [APPENDUID 1 20] appended\r\n")
                }
            }
            "UID" if rest.starts_with("STORE") => {
                let uid = rest.split(' ').nth(1).unwrap().parse::<u32>().unwrap();
                let mut marked = mail.marked.lock().unwrap();

                if rest.contains("+FLAGS") {
                    marked.push(uid);
                } else {
                    marked.retain(|value| *value != uid);
                }

                format!("{tag} OK stored\r\n")
            }
            "UID" if rest.starts_with("EXPUNGE") => {
                if mail.hang_delete.load(Ordering::SeqCst) {
                    mail.delete_reached.notify_one();
                    std::future::pending::<()>().await;
                }

                if mail.reject_delete.load(Ordering::SeqCst) {
                    format!("{tag} NO deletion refused\r\n")
                } else {
                    let uid = rest.split(' ').nth(1).unwrap().parse::<u32>().unwrap();
                    mail.removed.lock().unwrap().push(uid);
                    mail.marked.lock().unwrap().retain(|value| *value != uid);

                    format!("* 1 EXPUNGE\r\n{tag} OK removed\r\n")
                }
            }
            "UID" if rest.starts_with("MOVE") => {
                let mut fields = rest.splitn(3, ' ');
                fields.next();
                let uid = fields.next().unwrap().parse::<u32>().unwrap();
                let destination = fields.next().unwrap().to_string();
                mail.moved.lock().unwrap().push((uid, destination));
                mail.removed.lock().unwrap().push(uid);

                format!("* 1 EXPUNGE\r\n{tag} OK moved\r\n")
            }
            "EXPUNGE" => {
                mail.removed.lock().unwrap().extend(mail.marked.lock().unwrap().drain(..));

                format!("* 1 EXPUNGE\r\n{tag} OK removed\r\n")
            }
            "UID" if rest.starts_with("FETCH") => {
                if (rest.contains("BODY.PEEK[]") || rest.contains("BODY.PEEK[2]")) && rest.split(' ').nth(1) == Some("5") {
                    *mail.attachment_bytes.lock().unwrap() += 12;
                }

                let set = rest.split(' ').nth(1).unwrap_or_default();
                let mut body: String = set.split(',').filter_map(|uid| uid.parse::<u32>().ok()).filter(|uid| (*uid == FIRST_UID || *uid == SECOND_UID) && !mail.removed.lock().unwrap().contains(uid)).map(|uid| fetched(if uid == FIRST_UID { 1 } else { 2 }, uid, &rest)).collect();

                if mail.transfer_overrun.load(Ordering::SeqCst) && rest.contains("BODY.PEEK[]") {
                    let length = 20 * 1024 * 1024 + 65537;
                    body = format!("* 1 FETCH (UID 5 BODY[]<0> {{{length}}}\r\n{})\r\n", "x".repeat(length));
                }

                if let Some(size) = *mail.reported_size.lock().unwrap() {
                    body = body.replace(&format!("RFC822.SIZE {}", message(FIRST_UID).len()), &format!("RFC822.SIZE {size}"));
                }

                format!("{body}{tag} OK done\r\n")
            }
            "UID" if rest.starts_with("SEARCH") => {
                let removed = mail.removed.lock().unwrap().clone();
                let found: Vec<String> = [FIRST_UID, SECOND_UID].into_iter().filter(|uid| !removed.contains(uid)).map(|uid| uid.to_string()).collect();

                format!("* SEARCH {}\r\n{tag} OK done\r\n", found.join(" "))
            }
            "LOGOUT" => {
                write.write_all(format!("* BYE bye\r\n{tag} OK done\r\n").as_bytes()).await.unwrap();

                return;
            }
            _ => format!("{tag} BAD unknown\r\n"),
        };

        if write.write_all(response.as_bytes()).await.is_err() {
            return;
        }
    }
}

async fn smtp_connection<S: AsyncRead + AsyncWrite + Unpin>(stream: S, mail: Arc<FakeMail>) {
    let mut stream = stream;

    reply(&mut stream, "220 fake ready\r\n").await;

    let (read, mut write) = tokio::io::split(stream);
    let mut reader = BufReader::new(read);
    let credentials = base64::engine::general_purpose::STANDARD.encode(format!("\0{USERNAME}\0{PASSWORD}"));
    let mut line = String::new();

    loop {
        line.clear();

        if reader.read_line(&mut line).await.unwrap_or(0) == 0 {
            return;
        }

        let command = line.trim_end().to_string();
        let upper = command.to_uppercase();
        let response = if upper.starts_with("EHLO") {
            "250-fake\r\n250 AUTH PLAIN\r\n".to_string()
        } else if upper.starts_with("AUTH PLAIN") {
            if command.split(' ').nth(2) == Some(credentials.as_str()) { "235 ok\r\n".to_string() } else { "535 bad credentials\r\n".to_string() }
        } else if upper.starts_with("DATA") {
            write.write_all(b"354 go\r\n").await.unwrap();

            let mut data = String::new();

            loop {
                line.clear();
                reader.read_line(&mut line).await.unwrap();

                if line == ".\r\n" {
                    break;
                }

                data.push_str(&line);
            }

            mail.sent.lock().unwrap().push(data);
            "250 queued\r\n".to_string()
        } else if upper.starts_with("QUIT") {
            write.write_all(b"221 bye\r\n").await.unwrap();

            return;
        } else {
            "250 ok\r\n".to_string()
        };

        write.write_all(response.as_bytes()).await.unwrap();
    }
}

pub fn certificate_path() -> String {
    format!("{}/tests/it/common/fake_mail_cert.pem", env!("CARGO_MANIFEST_DIR"))
}

fn acceptor() -> TlsAcceptor {
    let certificates: Vec<CertificateDer<'static>> = CertificateDer::pem_file_iter(certificate_path()).unwrap().map(Result::unwrap).collect();
    let key = PrivateKeyDer::from_pem_file(format!("{}/tests/it/common/fake_mail_key.pem", env!("CARGO_MANIFEST_DIR"))).unwrap();
    let provider = Arc::new(tokio_rustls::rustls::crypto::aws_lc_rs::default_provider());
    let config = ServerConfig::builder_with_provider(provider).with_safe_default_protocol_versions().unwrap().with_no_client_auth().with_single_cert(certificates, key).unwrap();

    TlsAcceptor::from(Arc::new(config))
}

async fn listen(handler: fn(tokio_rustls::server::TlsStream<TcpStream>, Arc<FakeMail>) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send>>, mail: Arc<FakeMail>) -> u16 {
    let acceptor = acceptor();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();

    tokio::spawn(async move {
        loop {
            let (stream, _) = listener.accept().await.unwrap();

            let Ok(secured) = acceptor.accept(stream).await else { continue };

            tokio::spawn(handler(secured, mail.clone()));
        }
    });

    port
}

pub async fn fake_mail() -> (u16, u16, Arc<FakeMail>) {
    unsafe { std::env::set_var("MYCELIUM_MAIL_EXTRA_CA", certificate_path()) };

    let mail = Arc::new(FakeMail::default());
    mail.marked.lock().unwrap().push(SECOND_UID);
    let imap = listen(|stream, mail| Box::pin(imap_connection(stream, mail)), mail.clone()).await;
    let smtp = listen(|stream, mail| Box::pin(smtp_connection(stream, mail)), mail.clone()).await;

    (imap, smtp, mail)
}
