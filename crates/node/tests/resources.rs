use mycelium_node::resources::{Sampler, Watcher, keeper_conversation, parse_cores, parse_cpu_times, parse_listening_sockets, parse_docker_containers, parse_loadavg, parse_meminfo, parse_process_stat, parse_status_uid};
use nix::unistd::geteuid;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::process::ExitStatusExt;
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};
use uuid::Uuid;

fn fixture(relative: &str) -> String {
    std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/proc").join(relative)).unwrap()
}

fn copy_tree(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();

    for entry in std::fs::read_dir(from).unwrap().flatten() {
        let target = to.join(entry.file_name());

        if entry.path().is_dir() {
            copy_tree(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), target).unwrap();
        }
    }
}

fn proc_with_sockets() -> (tempfile::TempDir, PathBuf) {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("proc");
    copy_tree(&Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/proc"), &root);

    std::fs::create_dir_all(root.join("200/fd")).unwrap();
    symlink("socket:[5001]", root.join("200/fd/3")).unwrap();
    symlink("/work/app", root.join("200/cwd")).unwrap();
    std::fs::create_dir_all(root.join("300/fd")).unwrap();
    symlink("socket:[5004]", root.join("300/fd/4")).unwrap();

    (directory, root)
}

#[test]
fn cpu_times_split_busy_from_idle() {
    assert_eq!(parse_cpu_times(&fixture("stat")), Some((1500, 10000)));
    assert_eq!(parse_cores(&fixture("stat")), 2);
}

#[test]
fn memory_and_load_are_read_in_bytes_and_averages() {
    let memory = parse_meminfo(&fixture("meminfo"));

    assert_eq!(memory.total_bytes, 16_000_000 * 1024);
    assert_eq!(memory.available_bytes, 8_000_000 * 1024);
    assert_eq!(memory.swap_free_bytes, 1_500_000 * 1024);
    assert_eq!(parse_loadavg(&fixture("loadavg")), [0.52, 0.40, 0.31]);
}

#[test]
fn a_process_name_containing_brackets_does_not_shift_the_fields() {
    let stat = parse_process_stat("12 (a) b) S 4 12 12 0 -1 0 0 0 0 0 7 3 0 0 20 0 1 0 1 1 1 0").unwrap();

    assert_eq!(stat.name, "a) b");
    assert_eq!(stat.parent, 4);
    assert_eq!(stat.ticks, 10);
}

#[test]
fn only_listening_sockets_are_read_with_their_addresses() {
    let v4 = parse_listening_sockets(&fixture("net/tcp"));
    let v6 = parse_listening_sockets(&fixture("net/tcp6"));

    assert_eq!(v4.iter().map(|socket| (socket.address.as_str(), socket.port, socket.inode)).collect::<Vec<_>>(), [("0.0.0.0", 3000, 5001), ("127.0.0.1", 8080, 5002)]);
    assert_eq!(v6.iter().map(|socket| (socket.address.as_str(), socket.port)).collect::<Vec<_>>(), [("::", 5432), ("::1", 9000)]);
}

#[test]
fn a_keeper_command_line_names_its_conversation() {
    let conversation = Uuid::from_u128(7);

    assert_eq!(keeper_conversation(&["mycelium", "keeper", "--dir", &format!("/s/sessions/{conversation}"), "--", "claude"]), Some(conversation));
    assert_eq!(keeper_conversation(&["mycelium", "node", "run"]), None);
}

#[test]
fn a_sample_reports_resources_processes_and_ports_with_their_conversation() {
    let (_directory, root) = proc_with_sockets();
    let mut sampler = Sampler::new(root.clone(), vec![]);
    std::fs::write(root.join("stat"), "cpu  1500 0 500 8500 500 0 0 0 0 0\ncpu0 750 0 250 4250 250 0 0 0 0 0\ncpu1 750 0 250 4250 250 0 0 0 0 0\n").unwrap();
    std::fs::write(root.join("200/stat"), "200 (node) S 100 100 100 0 -1 4194560 0 0 0 0 250 150 0 0 20 0 1 0 1 1000 100 0\n").unwrap();

    let sample = sampler.sample(1000, 2.0);

    assert!(sample.available);
    assert_eq!(sample.cores, 2);
    assert_eq!(sample.cpu_percent, 50.0);
    assert_eq!(sample.memory_total_bytes, 16_000_000 * 1024);

    let node = sample.processes.iter().find(|process| process.pid == 200).unwrap();
    assert_eq!(node.cpu_percent, 50.0);
    assert_eq!(node.memory_bytes, 2560 * 4096);
    assert_eq!(node.conversation_id, Some(Uuid::from_u128(7)));
    assert_eq!(node.command, "node");

    let keeper = sample.processes.iter().find(|process| process.pid == 100).unwrap();
    assert!(keeper.agent);

    let web = sample.ports.iter().find(|port| port.port == 3000).unwrap();
    assert_eq!(web.process.as_deref(), Some("node"));
    assert_eq!(web.conversation_id, Some(Uuid::from_u128(7)));
    assert_eq!(web.cwd.as_deref(), Some("/work/app"));

    let database = sample.ports.iter().find(|port| port.port == 5432).unwrap();
    assert_eq!(database.process.as_deref(), Some("postgres"));
    assert_eq!(database.conversation_id, None);

    let unowned = sample.ports.iter().find(|port| port.port == 8080).unwrap();
    assert_eq!(unowned.process, None);
}

#[test]
fn a_command_line_is_never_reported_beyond_the_program_name() {
    let (_directory, root) = proc_with_sockets();
    let sample = Sampler::new(root, vec![]).sample(0, 1.0);
    let text = serde_json::to_string(&sample).unwrap();

    assert!(!text.contains("hidden"));
}

#[test]
fn a_machine_without_proc_reports_not_available_rather_than_failing() {
    let directory = tempfile::tempdir().unwrap();
    let mut sampler = Sampler::new(directory.path().join("missing"), vec![]);

    let sample = sampler.sample(5, 3.0);

    assert!(!sample.available);
    assert!(sample.processes.is_empty());
    assert!(sample.ports.is_empty());
}

#[tokio::test]
async fn watching_samples_while_the_lease_lasts_and_a_second_request_only_extends_it() {
    let (_directory, root) = proc_with_sockets();
    let (out, mut received) = tokio::sync::mpsc::unbounded_channel();
    let watcher = Watcher::new(out, root, vec![], std::time::Duration::from_millis(10));

    let sampling = watcher.extend(std::time::Duration::from_millis(100)).unwrap();
    let repeated = watcher.extend(std::time::Duration::from_millis(100));
    sampling.await.unwrap();

    assert!(repeated.is_none());
    assert!(received.try_recv().is_ok());
}

#[tokio::test]
async fn nobody_watching_means_no_sampling() {
    let (_directory, root) = proc_with_sockets();
    let (out, mut received) = tokio::sync::mpsc::unbounded_channel();
    let watcher = Watcher::new(out, root, vec![], std::time::Duration::from_millis(10));

    watcher.extend(std::time::Duration::ZERO).unwrap().await.unwrap();

    assert!(received.try_recv().is_err());
}

fn other_user() -> u32 {
    geteuid().as_raw() + 1
}

fn status_of(uid: u32) -> String {
    format!("Name:\tx\nUid:\t{uid}\t{uid}\t{uid}\t{uid}\nGid:\t1\t1\t1\t1\n")
}

fn docker_script(directory: &Path, body: &str) -> PathBuf {
    let script = directory.join("docker");
    std::fs::write(&script, format!("#!/bin/sh\n{body}\n")).unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();

    script
}

#[test]
fn the_effective_user_is_read_from_a_process_status() {
    assert_eq!(parse_status_uid("Name:\tbash\nUid:\t1000\t1001\t1000\t1000\n"), Some(1001));
    assert_eq!(parse_status_uid("Name:\tbash\n"), None);
}

#[test]
fn docker_output_lists_each_container_with_its_state() {
    let containers = parse_docker_containers("db|running\nweb-1|exited\n\nbroken line\n|running\n");

    assert_eq!(containers.iter().map(|container| (container.name.as_str(), container.state.as_str())).collect::<Vec<_>>(), [("db", "running"), ("web-1", "exited")]);
}

#[test]
fn a_process_is_stoppable_when_it_belongs_to_a_conversation_or_to_the_node_user() {
    let (_directory, root) = proc_with_sockets();
    std::fs::write(root.join("200/status"), status_of(other_user())).unwrap();
    std::fs::write(root.join("300/status"), status_of(other_user())).unwrap();
    std::fs::write(root.join("100/status"), status_of(geteuid().as_raw())).unwrap();

    let sample = Sampler::new(root, vec![]).sample(0, 1.0);
    let stoppable = |pid: u32| sample.processes.iter().find(|process| process.pid == pid).unwrap().stoppable;

    assert!(stoppable(200));
    assert!(stoppable(100));
    assert!(!stoppable(300));
}

#[test]
fn docker_containers_are_reported_when_the_cli_answers() {
    let (directory, root) = proc_with_sockets();
    let script = docker_script(directory.path(), "echo 'db|running'; echo 'web|exited'");

    let docker = Sampler::new(root, vec![]).with_docker_program(script).sample(0, 1.0).docker.unwrap();

    assert_eq!(docker.running, 1);
    assert_eq!(docker.containers.len(), 2);
}

#[test]
fn a_missing_or_failing_docker_is_not_available_and_the_sample_still_succeeds() {
    let (directory, root) = proc_with_sockets();
    let failing = docker_script(directory.path(), "echo 'Cannot connect to the Docker daemon' >&2; exit 1");

    let missing = Sampler::new(root.clone(), vec![]).with_docker_program(directory.path().join("absent")).sample(0, 1.0);
    let broken = Sampler::new(root, vec![]).with_docker_program(failing).sample(0, 1.0);

    assert!(missing.available && missing.docker.is_none());
    assert!(broken.available && broken.docker.is_none());
}

#[test]
fn stopping_a_process_sends_it_sigterm() {
    let mut child = std::process::Command::new("sleep").arg("60").spawn().unwrap();

    let stopped = Sampler::new(PathBuf::from("/proc"), vec![]).stop(child.id(), "sleep");

    assert_eq!(stopped, Ok(()));
    assert_eq!(child.wait().unwrap().signal(), Some(15));
}

#[test]
fn stopping_a_process_that_has_exited_or_changed_name_is_refused() {
    let (_directory, root) = proc_with_sockets();
    let sampler = Sampler::new(root, vec![]);

    assert_eq!(sampler.stop(999, "gone"), Err("gone has already exited.".into()));
    assert_eq!(sampler.stop(300, "nginx"), Err("Process 300 is no longer nginx. Reload the page and try again.".into()));
}

#[test]
fn stopping_a_process_that_another_user_owns_is_refused() {
    let (_directory, root) = proc_with_sockets();
    std::fs::write(root.join("300/status"), status_of(other_user())).unwrap();

    let stopped = Sampler::new(root, vec![]).stop(300, "postgres");

    assert_eq!(stopped, Err("postgres belongs to another user, so Mycelium cannot stop it.".into()));
}
