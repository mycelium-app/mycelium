use mycelium_node::resources::{Sampler, Watcher, keeper_conversation, parse_cores, parse_cpu_times, parse_listening_sockets, parse_loadavg, parse_meminfo, parse_process_stat};
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
