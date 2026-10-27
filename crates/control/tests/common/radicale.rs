use std::process::{Child, Command, Stdio};

pub const USERNAME: &str = "sam";
pub const PASSWORD: &str = "app-password-1";

const STARTUP: std::time::Duration = std::time::Duration::from_secs(240);

pub struct Radicale {
    pub address: String,
    _folder: tempfile::TempDir,
    child: Child,
}

impl Drop for Radicale {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Radicale {
    pub async fn start(calendars: &[&str]) -> Radicale {
        let folder = tempfile::tempdir().unwrap();
        let users = folder.path().join("users");
        let port = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
        let binary = std::env::var("RADICALE_BIN").unwrap_or_else(|_| "radicale".to_string());

        std::fs::write(&users, format!("{USERNAME}:{PASSWORD}\n")).unwrap();

        let child = Command::new(&binary)
            .args(["--server-hosts", &format!("127.0.0.1:{port}"), "--storage-filesystem-folder"])
            .arg(folder.path().join("collections"))
            .args(["--auth-type", "htpasswd", "--auth-htpasswd-filename"])
            .arg(&users)
            .args(["--auth-htpasswd-encryption", "plain"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap_or_else(|error| panic!("start Radicale from {binary}, or set RADICALE_BIN: {error}"));
        let radicale = Radicale { address: format!("http://127.0.0.1:{port}"), _folder: folder, child };
        let http = reqwest::Client::new();

        let deadline = tokio::time::Instant::now() + STARTUP;

        while http.get(&radicale.address).basic_auth(USERNAME, Some(PASSWORD)).send().await.is_err() {
            assert!(tokio::time::Instant::now() < deadline, "timed out waiting for Radicale to listen");
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }

        for calendar in calendars {
            let response = http.request(reqwest::Method::from_bytes(b"MKCALENDAR").unwrap(), format!("{}/{USERNAME}/{}/", radicale.address, calendar.to_lowercase())).basic_auth(USERNAME, Some(PASSWORD)).body(format!(r#"<c:mkcalendar xmlns:c="urn:ietf:params:xml:ns:caldav" xmlns:d="DAV:"><d:set><d:prop><d:displayname>{calendar}</d:displayname></d:prop></d:set></c:mkcalendar>"#)).send().await.unwrap();

            assert!(response.status().is_success(), "make {calendar}: {}", response.status());
        }

        radicale
    }

    pub async fn put(&self, calendar: &str, name: &str, ics: &str) {
        let response = reqwest::Client::new().put(format!("{}/{USERNAME}/{}/{name}.ics", self.address, calendar.to_lowercase())).basic_auth(USERNAME, Some(PASSWORD)).header("content-type", "text/calendar").body(ics.to_string()).send().await.unwrap();

        assert!(response.status().is_success(), "put {name}: {}", response.status());
    }
}
