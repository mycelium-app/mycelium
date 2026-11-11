use super::git;
use std::path::Path;
use std::process::{Command, Stdio};

pub(super) fn configured(repository: &Path) -> Vec<String> {
    let listed = git(repository, &["config", "--get-regexp", r"^remote\..*\.url$"], &[1]).unwrap_or_default();

    String::from_utf8_lossy(&listed).lines().filter_map(|line| line.split_once(' ').map(|(_, url)| url.trim().to_string())).collect()
}

pub(super) fn resolved(repository: &Path, remotes: &[String]) -> Vec<String> {
    let listed = git(repository, &["config", "--get-regexp", r"^url\..*\.insteadof$"], &[1]).unwrap_or_default();
    let rewrites = rewrites(&String::from_utf8_lossy(&listed));

    remotes.iter().map(|remote| resolve_alias(&rewrite(remote, &rewrites), None)).collect()
}

fn rewrites(listed: &str) -> Vec<(String, String)> {
    listed
        .lines()
        .filter_map(|line| {
            let (key, prefix) = line.split_once(' ')?;
            let base = key.strip_prefix("url.")?.strip_suffix(".insteadof")?;

            Some((prefix.to_string(), base.to_string()))
        })
        .collect()
}

fn rewrite(url: &str, rewrites: &[(String, String)]) -> String {
    let longest = rewrites.iter().filter(|(prefix, _)| url.starts_with(prefix.as_str())).max_by_key(|(prefix, _)| prefix.len());

    match longest {
        Some((prefix, base)) => format!("{base}{}", &url[prefix.len()..]),
        None => url.to_string(),
    }
}

fn split_ssh(url: &str) -> Option<(Option<&str>, &str, Option<&str>, &str)> {
    let (authority, path, separator) = match url.strip_prefix("ssh://") {
        Some(rest) => {
            let (authority, path) = rest.split_once('/').unwrap_or((rest, ""));

            (authority, path, "/")
        }
        None => {
            if url.contains("://") {
                return None;
            }

            let (authority, path) = url.split_once(':')?;

            if authority.contains('/') {
                return None;
            }

            (authority, path, ":")
        }
    };
    let (user, hostport) = match authority.rsplit_once('@') {
        Some((user, hostport)) => (Some(user), hostport),
        None => (None, authority),
    };
    let (host, port) = match (separator, hostport.split_once(':')) {
        ("/", Some((host, port))) => (host, Some(port)),
        _ => (hostport, None),
    };

    (!host.is_empty() && !host.starts_with('-')).then_some((user, host, port, path))
}

fn resolve_alias(url: &str, ssh_config: Option<&Path>) -> String {
    let Some((user, host, port, path)) = split_ssh(url) else {
        return url.to_string();
    };
    let mut command = Command::new("ssh");

    if let Some(config) = ssh_config {
        command.arg("-F").arg(config);
    }

    let output = command.arg("-G").arg(host).stdin(Stdio::null()).stderr(Stdio::null()).output();
    let Ok(output) = output.map_err(|_| ()).and_then(|output| if output.status.success() { Ok(output) } else { Err(()) }) else {
        return url.to_string();
    };
    let printed = String::from_utf8_lossy(&output.stdout);
    let setting = |name: &str| printed.lines().find_map(|line| line.strip_prefix(name).and_then(|rest| rest.strip_prefix(' '))).map(|value| value.trim().to_string());
    let Some(hostname) = setting("hostname").filter(|hostname| !hostname.is_empty()) else {
        return url.to_string();
    };
    let port = port.map(str::to_string).or_else(|| setting("port").filter(|port| port != "22"));
    let user = user.map(|user| format!("{user}@")).unwrap_or_default();
    let port = port.map(|port| format!(":{port}")).unwrap_or_default();

    format!("ssh://{user}{hostname}{port}/{}", path.trim_start_matches('/'))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::test_support::init;

    fn ssh_config(dir: &Path) -> std::path::PathBuf {
        let file = dir.join("ssh_config");
        std::fs::write(&file, "Host forge-alias\n  HostName git.example.com\n  Port 2222\nHost plain-alias\n  HostName git.example.com\n").unwrap();

        file
    }

    #[test]
    fn an_ssh_config_host_alias_resolves_to_its_host_name() {
        let dir = tempfile::tempdir().unwrap();
        let config = ssh_config(dir.path());

        assert_eq!(resolve_alias("git@plain-alias:team/app.git", Some(&config)), "ssh://git@git.example.com/team/app.git");
        assert_eq!(resolve_alias("ssh://git@forge-alias/team/app.git", Some(&config)), "ssh://git@git.example.com:2222/team/app.git");
        assert_eq!(resolve_alias("git@other.example.com:team/app.git", Some(&config)), "ssh://git@other.example.com/team/app.git");
    }

    #[test]
    fn an_address_that_is_not_ssh_is_left_as_it_is() {
        let dir = tempfile::tempdir().unwrap();
        let config = ssh_config(dir.path());

        assert_eq!(resolve_alias("https://forge-alias/team/app.git", Some(&config)), "https://forge-alias/team/app.git");
        assert_eq!(resolve_alias("/srv/git/app.git", Some(&config)), "/srv/git/app.git");
    }

    #[test]
    fn an_unreadable_ssh_config_leaves_the_literal_host() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("missing");

        assert_eq!(resolve_alias("git@forge-alias:team/app.git", Some(&missing)), "git@forge-alias:team/app.git");
    }

    #[test]
    fn the_longest_instead_of_prefix_rewrites_a_remote() {
        let repository = tempfile::tempdir().unwrap();
        init(repository.path());
        git(repository.path(), &["config", "url.https://git.example.com/.insteadOf", "fa:"], &[]).unwrap();
        git(repository.path(), &["config", "url.https://git.example.com/team-mirror/.insteadOf", "fa:team/"], &[]).unwrap();

        let resolved = resolved(repository.path(), &["fa:team/app.git".to_string(), "fa:other/app.git".to_string()]);

        assert_eq!(resolved, ["https://git.example.com/team-mirror/app.git", "https://git.example.com/other/app.git"]);
    }
}
