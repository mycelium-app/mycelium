use mycelium_proto::link::{DiskUsage, ListeningPort, NodeToControl, ProcessUsage, ResourceSample};
use std::collections::{HashMap, HashSet};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio::time::Instant;
use uuid::Uuid;

const CLOCK_TICKS_PER_SECOND: f32 = 100.0;
const BUSIEST_PROCESSES: usize = 8;
const AGENT_PROGRAMS: &[&str] = &["claude-agent-acp", "codex-acp", "claude", "codex"];
const LISTEN_STATE: &str = "0A";

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Memory {
    pub total_bytes: u64,
    pub available_bytes: u64,
    pub swap_total_bytes: u64,
    pub swap_free_bytes: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ProcessStat {
    pub name: String,
    pub parent: u32,
    pub ticks: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ListeningSocket {
    pub address: String,
    pub port: u16,
    pub inode: u64,
}

pub fn parse_cpu_times(stat: &str) -> Option<(u64, u64)> {
    let line = stat.lines().find(|line| line.starts_with("cpu "))?;
    let fields: Vec<u64> = line.split_whitespace().skip(1).filter_map(|field| field.parse().ok()).collect();
    let total: u64 = fields.iter().take(8).sum();
    let idle = fields.get(3).copied().unwrap_or(0) + fields.get(4).copied().unwrap_or(0);

    Some((total.saturating_sub(idle), total))
}

pub fn parse_cores(stat: &str) -> u32 {
    stat.lines().filter(|line| line.strip_prefix("cpu").and_then(|rest| rest.chars().next()).is_some_and(|next| next.is_ascii_digit())).count() as u32
}

pub fn parse_meminfo(text: &str) -> Memory {
    let kilobytes = |name: &str| -> u64 {
        text.lines().find_map(|line| line.strip_prefix(name)?.strip_prefix(':')?.split_whitespace().next()?.parse::<u64>().ok()).unwrap_or(0) * 1024
    };

    Memory { total_bytes: kilobytes("MemTotal"), available_bytes: kilobytes("MemAvailable"), swap_total_bytes: kilobytes("SwapTotal"), swap_free_bytes: kilobytes("SwapFree") }
}

pub fn parse_loadavg(text: &str) -> [f32; 3] {
    let mut load = [0.0; 3];

    for (slot, field) in load.iter_mut().zip(text.split_whitespace()) {
        *slot = field.parse().unwrap_or(0.0);
    }

    load
}

pub fn parse_process_stat(text: &str) -> Option<ProcessStat> {
    let open = text.find('(')?;
    let close = text.rfind(')')?;
    let fields: Vec<&str> = text.get(close + 1..)?.split_whitespace().collect();
    let parent = fields.get(1)?.parse().ok()?;
    let ticks = fields.get(11)?.parse::<u64>().ok()? + fields.get(12)?.parse::<u64>().ok()?;

    Some(ProcessStat { name: text.get(open + 1..close)?.to_string(), parent, ticks })
}

pub fn parse_listening_sockets(text: &str) -> Vec<ListeningSocket> {
    text.lines()
        .skip(1)
        .filter_map(|line| {
            let fields: Vec<&str> = line.split_whitespace().collect();

            if fields.get(3) != Some(&LISTEN_STATE) {
                return None;
            }

            let (address, port) = fields.get(1)?.split_once(':')?;

            Some(ListeningSocket { address: decode_address(address)?, port: u16::from_str_radix(port, 16).ok()?, inode: fields.get(9)?.parse().ok()? })
        })
        .collect()
}

fn decode_address(hex: &str) -> Option<String> {
    match hex.len() {
        8 => Some(Ipv4Addr::from(u32::from_str_radix(hex, 16).ok()?.swap_bytes()).to_string()),
        32 => {
            let mut octets = [0u8; 16];

            for (index, chunk) in hex.as_bytes().chunks(8).enumerate() {
                let word = u32::from_str_radix(std::str::from_utf8(chunk).ok()?, 16).ok()?.swap_bytes();
                octets[index * 4..index * 4 + 4].copy_from_slice(&word.to_be_bytes());
            }

            let address = Ipv6Addr::from(octets);

            Some(match address.to_ipv4_mapped() {
                Some(mapped) => IpAddr::V4(mapped).to_string(),
                None => address.to_string(),
            })
        }
        _ => None,
    }
}

pub fn keeper_conversation(arguments: &[&str]) -> Option<Uuid> {
    let keeper = arguments.iter().position(|argument| *argument == "keeper")?;
    let directory = arguments.get(keeper..)?.windows(2).find(|pair| pair[0] == "--dir")?[1];

    Uuid::parse_str(Path::new(directory).file_name()?.to_str()?).ok()
}

fn is_agent(arguments: &[&str]) -> bool {
    let program = arguments.first().and_then(|first| Path::new(first).file_name()).and_then(|name| name.to_str()).unwrap_or_default();

    AGENT_PROGRAMS.contains(&program) || arguments.iter().any(|argument| argument.contains("claude-agent-acp") || argument.contains("codex-acp"))
}

struct Seen {
    stat: ProcessStat,
    program: String,
    agent: bool,
    keeper_of: Option<Uuid>,
    cwd: Option<String>,
}

pub struct Sampler {
    proc_root: PathBuf,
    disks: Vec<PathBuf>,
    previous_cpu: Option<(u64, u64)>,
    previous_ticks: HashMap<u32, u64>,
}

impl Sampler {
    pub fn new(proc_root: PathBuf, disks: Vec<PathBuf>) -> Sampler {
        let previous_cpu = std::fs::read_to_string(proc_root.join("stat")).ok().as_deref().and_then(parse_cpu_times);

        let mut sampler = Sampler { proc_root, disks, previous_cpu, previous_ticks: HashMap::new() };
        sampler.previous_ticks = sampler.read_processes().into_iter().map(|(pid, process)| (pid, process.stat.ticks)).collect();

        sampler
    }

    pub fn available(&self) -> bool {
        self.proc_root.join("stat").is_file()
    }

    pub fn sample(&mut self, at_ms: i64, elapsed_seconds: f32) -> ResourceSample {
        if !self.available() {
            return ResourceSample {
                at_ms,
                available: false,
                cpu_percent: 0.0,
                cores: 0,
                load: [0.0; 3],
                memory_total_bytes: 0,
                memory_available_bytes: 0,
                swap_total_bytes: 0,
                swap_free_bytes: 0,
                disks: Vec::new(),
                processes: Vec::new(),
                ports: Vec::new(),
            };
        }

        let stat = self.read("stat");
        let cpu = parse_cpu_times(&stat);
        let cpu_percent = match (self.previous_cpu, cpu) {
            (Some((previous_busy, previous_total)), Some((busy, total))) if total > previous_total => (busy.saturating_sub(previous_busy)) as f32 / (total - previous_total) as f32 * 100.0,
            _ => 0.0,
        };
        self.previous_cpu = cpu;

        let memory = parse_meminfo(&self.read("meminfo"));
        let seen = self.read_processes();
        let owners = self.listening_owners(&seen);
        let conversation_of = |pid: u32| conversation_of(&seen, pid);

        let mut ticks = HashMap::new();
        let mut processes: Vec<ProcessUsage> = seen
            .iter()
            .map(|(pid, process)| {
                ticks.insert(*pid, process.stat.ticks);
                let spent = process.stat.ticks.saturating_sub(self.previous_ticks.get(pid).copied().unwrap_or(process.stat.ticks));
                let cpu_percent = if elapsed_seconds > 0.0 { spent as f32 / CLOCK_TICKS_PER_SECOND / elapsed_seconds * 100.0 } else { 0.0 };

                ProcessUsage {
                    pid: *pid,
                    name: process.stat.name.clone(),
                    command: process.program.clone(),
                    cpu_percent,
                    memory_bytes: self.resident_bytes(*pid),
                    agent: process.agent,
                    conversation_id: conversation_of(*pid),
                    cwd: process.cwd.clone(),
                }
            })
            .collect();
        self.previous_ticks = ticks;

        processes.sort_by(|left, right| right.cpu_percent.total_cmp(&left.cpu_percent).then(left.pid.cmp(&right.pid)));
        let mut kept = 0;
        processes.retain(|process| {
            kept += 1;

            process.agent || kept <= BUSIEST_PROCESSES
        });

        let mut ports: Vec<ListeningPort> = self
            .listening_sockets()
            .into_iter()
            .map(|socket| {
                let pid = owners.get(&socket.inode).copied();
                let owner = pid.and_then(|pid| seen.get(&pid));

                ListeningPort {
                    port: socket.port,
                    address: socket.address,
                    pid,
                    process: owner.map(|owner| owner.stat.name.clone()),
                    conversation_id: pid.and_then(conversation_of),
                    cwd: owner.and_then(|owner| owner.cwd.clone()),
                }
            })
            .collect();
        ports.sort_by(|left, right| left.port.cmp(&right.port).then_with(|| left.address.cmp(&right.address)));
        ports.dedup_by(|later, earlier| later.port == earlier.port && later.address == earlier.address);

        ResourceSample {
            at_ms,
            available: true,
            cpu_percent,
            cores: parse_cores(&stat),
            load: parse_loadavg(&self.read("loadavg")),
            memory_total_bytes: memory.total_bytes,
            memory_available_bytes: memory.available_bytes,
            swap_total_bytes: memory.swap_total_bytes,
            swap_free_bytes: memory.swap_free_bytes,
            disks: self.disks.iter().filter_map(|mount| disk_usage(mount)).collect(),
            processes,
            ports,
        }
    }

    fn read(&self, relative: &str) -> String {
        std::fs::read_to_string(self.proc_root.join(relative)).unwrap_or_default()
    }

    fn process_ids(&self) -> Vec<u32> {
        let Ok(entries) = std::fs::read_dir(&self.proc_root) else {
            return Vec::new();
        };

        entries.flatten().filter_map(|entry| entry.file_name().to_str()?.parse().ok()).collect()
    }

    fn read_processes(&self) -> HashMap<u32, Seen> {
        let mut seen = HashMap::new();

        for pid in self.process_ids() {
            let Some(stat) = parse_process_stat(&self.read(&format!("{pid}/stat"))) else {
                continue;
            };
            let cmdline = self.read(&format!("{pid}/cmdline"));
            let arguments: Vec<&str> = cmdline.split('\0').filter(|argument| !argument.is_empty()).collect();
            let program = arguments.first().and_then(|first| Path::new(first).file_name()).and_then(|name| name.to_str()).unwrap_or(&stat.name).to_string();
            let cwd = std::fs::read_link(self.proc_root.join(pid.to_string()).join("cwd")).ok().map(|path| path.to_string_lossy().into_owned());

            seen.insert(pid, Seen { agent: is_agent(&arguments), keeper_of: keeper_conversation(&arguments), program, cwd, stat });
        }

        seen
    }

    fn resident_bytes(&self, pid: u32) -> u64 {
        let pages = self.read(&format!("{pid}/statm")).split_whitespace().nth(1).and_then(|field| field.parse::<u64>().ok()).unwrap_or(0);

        pages * 4096
    }

    fn listening_sockets(&self) -> Vec<ListeningSocket> {
        let mut sockets = parse_listening_sockets(&self.read("net/tcp"));
        sockets.extend(parse_listening_sockets(&self.read("net/tcp6")));

        sockets
    }

    fn listening_owners(&self, seen: &HashMap<u32, Seen>) -> HashMap<u64, u32> {
        let wanted: HashSet<u64> = self.listening_sockets().into_iter().map(|socket| socket.inode).collect();
        let mut owners = HashMap::new();

        if wanted.is_empty() {
            return owners;
        }

        for pid in seen.keys() {
            let Ok(entries) = std::fs::read_dir(self.proc_root.join(pid.to_string()).join("fd")) else {
                continue;
            };

            for entry in entries.flatten() {
                let Ok(target) = std::fs::read_link(entry.path()) else {
                    continue;
                };
                let inode = target.to_str().and_then(|text| text.strip_prefix("socket:[")?.strip_suffix(']')?.parse::<u64>().ok());

                if let Some(inode) = inode.filter(|inode| wanted.contains(inode)) {
                    owners.entry(inode).or_insert(*pid);
                }
            }
        }

        owners
    }
}

fn conversation_of(seen: &HashMap<u32, Seen>, pid: u32) -> Option<Uuid> {
    let mut current = pid;

    for _ in 0..64 {
        let process = seen.get(&current)?;

        if let Some(conversation_id) = process.keeper_of {
            return Some(conversation_id);
        }

        if process.stat.parent == current || process.stat.parent == 0 {
            return None;
        }

        current = process.stat.parent;
    }

    None
}

fn disk_usage(mount: &Path) -> Option<DiskUsage> {
    let stats = nix::sys::statvfs::statvfs(mount).ok()?;
    let block = stats.fragment_size() as u64;

    Some(DiskUsage { mount: mount.to_string_lossy().into_owned(), total_bytes: stats.blocks() as u64 * block, free_bytes: stats.blocks_available() as u64 * block })
}

pub struct Watcher {
    out: mpsc::UnboundedSender<NodeToControl>,
    proc_root: PathBuf,
    disks: Vec<PathBuf>,
    interval: Duration,
    deadline: Arc<Mutex<Instant>>,
    sampling: Arc<AtomicBool>,
}

impl Watcher {
    pub fn new(out: mpsc::UnboundedSender<NodeToControl>, proc_root: PathBuf, disks: Vec<PathBuf>, interval: Duration) -> Watcher {
        Watcher { out, proc_root, disks, interval, deadline: Arc::new(Mutex::new(Instant::now())), sampling: Arc::new(AtomicBool::new(false)) }
    }

    pub fn extend(&self, lease: Duration) -> Option<JoinHandle<()>> {
        if let Ok(mut deadline) = self.deadline.lock() {
            *deadline = Instant::now() + lease;
        }

        if self.sampling.swap(true, Ordering::SeqCst) {
            return None;
        }

        let sampler = Arc::new(Mutex::new(Sampler::new(self.proc_root.clone(), self.disks.clone())));
        let out = self.out.clone();
        let deadline = self.deadline.clone();
        let sampling = self.sampling.clone();
        let interval = self.interval;

        Some(tokio::spawn(async move {
            let mut previous = Instant::now();

            loop {
                tokio::time::sleep(interval).await;

                let watched_until = deadline.lock().map(|deadline| *deadline).unwrap_or_else(|_| Instant::now());

                if Instant::now() >= watched_until {
                    break;
                }

                let elapsed = previous.elapsed().as_secs_f32();
                previous = Instant::now();
                let at_ms = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|since| since.as_millis() as i64).unwrap_or(0);
                let taken = sampler.clone();
                let Ok(sample) = tokio::task::spawn_blocking(move || taken.lock().ok().map(|mut sampler| sampler.sample(at_ms, elapsed))).await else {
                    break;
                };
                let Some(sample) = sample else {
                    break;
                };

                if out.send(NodeToControl::Resources { sample }).is_err() {
                    break;
                }
            }

            sampling.store(false, Ordering::SeqCst);
        }))
    }
}
