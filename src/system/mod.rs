use std::{
    collections::BTreeMap,
    ffi::OsStr,
    time::{Duration, Instant},
};

use anyhow::{Result, anyhow};
use sysinfo::{Disks, Networks, ProcessesToUpdate, System};

/// Known virtual/ram filesystems to exclude from disk monitoring.
fn is_real_filesystem(fs: &OsStr) -> bool {
    !matches!(
        fs.to_str(),
        Some("tmpfs" | "devtmpfs" | "ramfs" | "overlay" | "aufs")
    )
}

#[derive(Debug, Clone, Default)]
pub struct DiskSample {
    pub mount: String,
    pub available_bytes: u64,
    pub total_bytes: u64,
}

#[derive(Debug, Clone, Default)]
pub struct ProcessSample {
    pub memory_bytes: u64,
    pub cpu_percent: f32,
    pub command: String,
}

#[derive(Debug, Clone, Default)]
pub struct NetworkSample {
    pub name: String,
    pub received_bytes: u64,
    pub transmitted_bytes: u64,
    pub receive_rate: u64,
    pub transmit_rate: u64,
}

#[derive(Debug, Clone, Default)]
pub struct IpAddressSample {
    pub interface: String,
    pub address: String,
}

#[derive(Debug, Clone, Default)]
pub struct SystemSnapshot {
    pub os_name: String,
    pub kernel_name: String,
    pub kernel_version: String,
    pub architecture: String,
    pub hostname: String,
    pub ip_address: String,
    pub ip_address_entries: Vec<IpAddressSample>,
    pub uptime_seconds: u64,
    pub load_average: String,
    pub cpu_model: String,
    pub cpu_cores: usize,
    pub cpu_frequency_mhz: u64,
    pub cpu_percent: f32,
    pub mem_percent: f32,
    pub swap_percent: f32,
    pub mem_used: u64,
    pub mem_total: u64,
    pub mem_available: u64,
    pub swap_used: u64,
    pub mem_detail: String,
    pub swap_detail: String,
    pub net_rx: String,
    pub net_tx: String,
    pub net_rx_rate: u64,
    pub net_tx_rate: u64,
    pub network_interfaces: Vec<NetworkSample>,
    pub disks: Vec<DiskSample>,
    /// Complete mount list used by the full system information page. The
    /// compact sidebar continues to use `disks`.
    pub filesystems: Vec<DiskSample>,
    pub processes: Vec<ProcessSample>,
    pub total_swap: u64,
}

pub struct SystemSampler {
    sys: System,
    nets: Networks,
    disks: Disks,
    last_rx_total: u64,
    last_tx_total: u64,
    last_instant: Instant,
}

impl SystemSampler {
    pub fn new() -> Self {
        let mut sys = System::new_all();
        sys.refresh_all();
        let nets = Networks::new_with_refreshed_list();
        let disks = Disks::new_with_refreshed_list();
        let last_rx_total = nets.values().map(|d| d.total_received()).sum();
        let last_tx_total = nets.values().map(|d| d.total_transmitted()).sum();

        Self {
            sys,
            nets,
            disks,
            last_rx_total,
            last_tx_total,
            last_instant: Instant::now(),
        }
    }

    pub fn interval() -> Duration {
        Duration::from_millis(1000)
    }

    pub fn sample(&mut self) -> SystemSnapshot {
        self.sys.refresh_cpu_usage();
        self.sys.refresh_memory();
        self.sys.refresh_processes(ProcessesToUpdate::All, true);
        self.nets.refresh(true);
        self.disks.refresh(true);

        let cpu_percent = self.sys.global_cpu_usage() / 100.0;
        let mem_total = self.sys.total_memory();
        let mem_used = self.sys.used_memory();
        let swap_total = self.sys.total_swap();
        let swap_used = self.sys.used_swap();

        let rx_total: u64 = self.nets.values().map(|d| d.total_received()).sum();
        let tx_total: u64 = self.nets.values().map(|d| d.total_transmitted()).sum();
        let now = Instant::now();
        let elapsed = now
            .duration_since(self.last_instant)
            .as_secs_f64()
            .max(0.001);
        let rx_rate = (rx_total.saturating_sub(self.last_rx_total) as f64 / elapsed) as u64;
        let tx_rate = (tx_total.saturating_sub(self.last_tx_total) as f64 / elapsed) as u64;
        self.last_rx_total = rx_total;
        self.last_tx_total = tx_total;
        self.last_instant = now;
        let mut network_interfaces = self
            .nets
            .iter()
            .map(|(name, data)| NetworkSample {
                name: name.clone(),
                received_bytes: data.total_received(),
                transmitted_bytes: data.total_transmitted(),
                receive_rate: (data.received() as f64 / elapsed) as u64,
                transmit_rate: (data.transmitted() as f64 / elapsed) as u64,
            })
            .collect::<Vec<_>>();
        network_interfaces.sort_by(|left, right| left.name.cmp(&right.name));

        let mut filesystems: Vec<DiskSample> = self
            .disks
            .iter()
            .filter(|disk| disk.total_space() > 0)
            .map(|disk| DiskSample {
                mount: disk.mount_point().to_string_lossy().to_string(),
                available_bytes: disk.available_space(),
                total_bytes: disk.total_space(),
            })
            .collect();
        filesystems.sort_by(|a, b| a.mount.cmp(&b.mount));

        let mut disks: Vec<DiskSample> = self
            .disks
            .iter()
            .filter(|disk| disk.total_space() > 0 && is_real_filesystem(disk.file_system()))
            .map(|disk| DiskSample {
                mount: disk.mount_point().to_string_lossy().to_string(),
                available_bytes: disk.available_space(),
                total_bytes: disk.total_space(),
            })
            .collect();
        disks.sort_by(|a, b| {
            if a.mount == "/" {
                return std::cmp::Ordering::Less;
            }
            if b.mount == "/" {
                return std::cmp::Ordering::Greater;
            }
            a.mount.cmp(&b.mount)
        });

        let mut processes = self
            .sys
            .processes()
            .values()
            .filter(|process| process.memory() > 0)
            .map(|process| ProcessSample {
                memory_bytes: process.memory(),
                cpu_percent: process.cpu_usage(),
                command: process.name().to_string_lossy().into_owned(),
            })
            .collect::<Vec<_>>();
        processes.sort_by_key(|process| std::cmp::Reverse(process.memory_bytes));
        processes.truncate(64);

        SystemSnapshot {
            os_name: System::long_os_version().unwrap_or_default(),
            kernel_name: System::name().unwrap_or_default(),
            kernel_version: System::kernel_version().unwrap_or_default(),
            architecture: std::env::consts::ARCH.to_string(),
            hostname: System::host_name().unwrap_or_default(),
            ip_address: String::new(),
            ip_address_entries: Vec::new(),
            uptime_seconds: System::uptime(),
            load_average: {
                let load = System::load_average();
                format!("{:.2}, {:.2}, {:.2}", load.one, load.five, load.fifteen)
            },
            cpu_model: self
                .sys
                .cpus()
                .first()
                .map(|cpu| cpu.brand().to_string())
                .unwrap_or_default(),
            cpu_cores: self
                .sys
                .physical_core_count()
                .unwrap_or_else(|| self.sys.cpus().len()),
            cpu_frequency_mhz: self
                .sys
                .cpus()
                .first()
                .map(|cpu| cpu.frequency())
                .unwrap_or_default(),
            cpu_percent,
            mem_percent: ratio(mem_used, mem_total),
            swap_percent: ratio(swap_used, swap_total),
            mem_used,
            mem_total,
            mem_available: mem_total.saturating_sub(mem_used),
            swap_used,
            mem_detail: format!("{}/{}", format_bytes(mem_used), format_bytes(mem_total)),
            swap_detail: format!("{}/{}", format_bytes(swap_used), format_bytes(swap_total)),
            net_rx: format!("{}/s", format_bytes(rx_rate)),
            net_tx: format!("{}/s", format_bytes(tx_rate)),
            net_rx_rate: rx_rate,
            net_tx_rate: tx_rate,
            network_interfaces,
            disks,
            filesystems,
            processes,
            total_swap: swap_total,
        }
    }
}

/// A process-wide shared system sampler that caches the latest snapshot.
///
/// Multiple windows each calling `sample()` within the same interval will
/// only trigger the expensive sampling work once; subsequent callers
/// receive the cached snapshot. This avoids N× redundant reads of
/// `/proc` (and equivalents) when N windows are open.
pub struct SharedSystemSampler {
    sampler: SystemSampler,
    last_snapshot: SystemSnapshot,
    last_sample_instant: Instant,
}

impl SharedSystemSampler {
    pub fn new() -> Self {
        let mut sampler = SystemSampler::new();
        let last_snapshot = sampler.sample();
        Self {
            sampler,
            last_snapshot,
            last_sample_instant: Instant::now(),
        }
    }

    /// Returns the latest snapshot. Performs the expensive sampling only if
    /// the interval has elapsed since the last sample; otherwise returns
    /// the cached snapshot. Callers should clone the result if they need
    /// to own it beyond the borrow.
    pub fn sample(&mut self) -> &SystemSnapshot {
        if self.last_sample_instant.elapsed() >= SystemSampler::interval() {
            self.last_snapshot = self.sampler.sample();
            self.last_sample_instant = Instant::now();
        }
        &self.last_snapshot
    }
}

fn ratio(used: u64, total: u64) -> f32 {
    if total == 0 {
        0.0
    } else {
        (used as f32 / total as f32).clamp(0.0, 1.0)
    }
}

pub fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} {}", UNITS[unit])
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

pub fn remote_snapshot_from_kv(raw: &str) -> Result<SystemSnapshot> {
    let mut kv = BTreeMap::new();
    let mut disks = Vec::new();
    let mut filesystems = Vec::new();
    let mut network_interfaces = Vec::new();
    let mut ip_address_entries = Vec::new();
    let mut processes = Vec::new();

    for line in raw.lines().map(str::trim).filter(|line| !line.is_empty()) {
        if let Some(rest) = line.strip_prefix("DISK=") {
            let mut parts = rest.split('\t');
            let mount = parts.next().unwrap_or_default().to_string();
            let available_bytes = parts
                .next()
                .unwrap_or("0")
                .parse::<u64>()
                .unwrap_or_default();
            let total_bytes = parts
                .next()
                .unwrap_or("0")
                .parse::<u64>()
                .unwrap_or_default();
            disks.push(DiskSample {
                mount,
                available_bytes,
                total_bytes,
            });
            continue;
        }

        if let Some(rest) = line.strip_prefix("FILESYSTEM=") {
            let mut parts = rest.split('\t');
            filesystems.push(DiskSample {
                mount: parts.next().unwrap_or_default().to_string(),
                available_bytes: parts.next().unwrap_or("0").parse().unwrap_or_default(),
                total_bytes: parts.next().unwrap_or("0").parse().unwrap_or_default(),
            });
            continue;
        }

        if let Some(rest) = line.strip_prefix("NETIF=") {
            let mut parts = rest.split('\t');
            network_interfaces.push(NetworkSample {
                name: parts.next().unwrap_or_default().to_string(),
                received_bytes: parts.next().unwrap_or("0").parse().unwrap_or_default(),
                transmitted_bytes: parts.next().unwrap_or("0").parse().unwrap_or_default(),
                receive_rate: parts.next().unwrap_or("0").parse().unwrap_or_default(),
                transmit_rate: parts.next().unwrap_or("0").parse().unwrap_or_default(),
            });
            continue;
        }

        if let Some(rest) = line.strip_prefix("IP_ENTRY=") {
            let mut parts = rest.splitn(2, '\t');
            ip_address_entries.push(IpAddressSample {
                interface: parts.next().unwrap_or_default().to_string(),
                address: parts.next().unwrap_or_default().to_string(),
            });
            continue;
        }

        if let Some(rest) = line.strip_prefix("PROCESS=") {
            let mut parts = rest.splitn(4, '\t');
            let _pid = parts.next();
            processes.push(ProcessSample {
                memory_bytes: parts.next().unwrap_or("0").parse().unwrap_or_default(),
                cpu_percent: parts.next().unwrap_or("0").parse().unwrap_or_default(),
                command: parts.next().unwrap_or_default().to_string(),
            });
            continue;
        }

        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        kv.insert(key.to_string(), value.to_string());
    }

    let cpu_percent = kv
        .get("CPU_PERCENT")
        .ok_or_else(|| anyhow!("missing CPU_PERCENT"))?
        .parse::<f32>()
        .unwrap_or_default()
        / 100.0;

    let mem_used = parse_u64(&kv, "MEM_USED");
    let mem_total = parse_u64(&kv, "MEM_TOTAL");
    let swap_used = parse_u64(&kv, "SWAP_USED");
    let swap_total = parse_u64(&kv, "SWAP_TOTAL");
    let rx_rate = parse_u64(&kv, "NET_RX");
    let tx_rate = parse_u64(&kv, "NET_TX");

    // Safety filter: exclude entries with zero/negligible total size
    // (catches any virtual fs lines that slipped past the script filter)
    disks.retain(|d| d.total_bytes >= 1024 * 1024);

    disks.sort_by(|a, b| {
        if a.mount == "/" {
            return std::cmp::Ordering::Less;
        }
        if b.mount == "/" {
            return std::cmp::Ordering::Greater;
        }
        a.mount.cmp(&b.mount)
    });
    filesystems.retain(|filesystem| filesystem.total_bytes > 0);
    if filesystems.is_empty() {
        filesystems = disks.clone();
    }
    filesystems.sort_by(|a, b| {
        if a.mount == "/" {
            return std::cmp::Ordering::Less;
        }
        if b.mount == "/" {
            return std::cmp::Ordering::Greater;
        }
        a.mount.cmp(&b.mount)
    });
    network_interfaces.sort_by(|left, right| left.name.cmp(&right.name));

    let mut ip_addresses = kv
        .get("IP_ADDRESSES")
        .map(|addresses| {
            addresses
                .split_whitespace()
                .filter(|address| !address.is_empty())
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    if ip_addresses.is_empty()
        && let Some(primary) = kv.get("IP_ADDRESS").filter(|address| !address.is_empty())
    {
        ip_addresses.push(primary.clone());
    }
    ip_address_entries.retain(|entry| !entry.address.is_empty());
    let mut unique_entries = Vec::new();
    for entry in ip_address_entries {
        if !unique_entries.iter().any(|existing: &IpAddressSample| {
            existing.interface == entry.interface && existing.address == entry.address
        }) {
            unique_entries.push(entry);
        }
    }
    let mut unique_ip_addresses = unique_entries
        .iter()
        .map(|entry| entry.address.clone())
        .collect::<Vec<_>>();
    for address in ip_addresses {
        if !unique_ip_addresses.contains(&address) {
            unique_ip_addresses.push(address.clone());
            unique_entries.push(IpAddressSample {
                interface: "-".to_string(),
                address,
            });
        }
    }
    let primary_ip = unique_ip_addresses.first().cloned().unwrap_or_default();

    Ok(SystemSnapshot {
        os_name: kv.get("OS_NAME").cloned().unwrap_or_default(),
        kernel_name: kv.get("KERNEL_NAME").cloned().unwrap_or_default(),
        kernel_version: kv.get("KERNEL_VERSION").cloned().unwrap_or_default(),
        architecture: kv.get("ARCHITECTURE").cloned().unwrap_or_default(),
        hostname: kv.get("HOSTNAME").cloned().unwrap_or_default(),
        ip_address: primary_ip,
        ip_address_entries: unique_entries,
        uptime_seconds: parse_u64(&kv, "UPTIME_SECONDS"),
        load_average: kv.get("LOAD_AVERAGE").cloned().unwrap_or_default(),
        cpu_model: kv.get("CPU_MODEL").cloned().unwrap_or_default(),
        cpu_cores: parse_u64(&kv, "CPU_CORES") as usize,
        cpu_frequency_mhz: parse_u64(&kv, "CPU_FREQUENCY_MHZ"),
        cpu_percent: cpu_percent.clamp(0.0, 1.0),
        mem_percent: ratio(mem_used, mem_total),
        swap_percent: ratio(swap_used, swap_total),
        mem_used,
        mem_total,
        mem_available: mem_total.saturating_sub(mem_used),
        swap_used,
        mem_detail: format!("{}/{}", format_bytes(mem_used), format_bytes(mem_total)),
        swap_detail: format!("{}/{}", format_bytes(swap_used), format_bytes(swap_total)),
        net_rx: format!("{}/s", format_bytes(rx_rate)),
        net_tx: format!("{}/s", format_bytes(tx_rate)),
        net_rx_rate: rx_rate,
        net_tx_rate: tx_rate,
        network_interfaces,
        disks,
        filesystems,
        processes,
        total_swap: swap_total,
    })
}

fn parse_u64(kv: &BTreeMap<String, String>, key: &str) -> u64 {
    kv.get(key)
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_remote_system_details_and_processes() {
        let snapshot = remote_snapshot_from_kv(
            "OS_NAME=Example Linux\n\
             KERNEL_NAME=Linux\n\
             KERNEL_VERSION=6.8.0\n\
             ARCHITECTURE=x86_64\n\
             HOSTNAME=server-01\n\
             IP_ADDRESS=10.0.0.8\n\
             IP_ADDRESSES=10.0.0.8 172.17.0.1\n\
             IP_ENTRY=eth0\t10.0.0.8\n\
             IP_ENTRY=docker0\t172.17.0.1\n\
             UPTIME_SECONDS=90061\n\
             LOAD_AVERAGE=0.10 0.20 0.30\n\
             CPU_MODEL=Example CPU\n\
             CPU_CORES=8\n\
             CPU_FREQUENCY_MHZ=3200\n\
             CPU_PERCENT=25.5\n\
             MEM_TOTAL=16000\n\
             MEM_USED=6000\n\
             SWAP_TOTAL=4000\n\
             SWAP_USED=1000\n\
             NET_RX=128\n\
             NET_TX=64\n\
             NETIF=eth0\t10000\t5000\t128\t64\n\
             PROCESS=42\t2048\t12.5\tsshd\n\
             DISK=/\t3000000000\t10000000000\n\
             FILESYSTEM=/\t3000000000\t10000000000\n\
             FILESYSTEM=/run\t1000000000\t2000000000",
        )
        .expect("remote snapshot should parse");

        assert_eq!(snapshot.os_name, "Example Linux");
        assert_eq!(snapshot.kernel_name, "Linux");
        assert_eq!(snapshot.hostname, "server-01");
        assert_eq!(snapshot.ip_address_entries[0].interface, "eth0");
        assert_eq!(snapshot.ip_address_entries[1].interface, "docker0");
        assert_eq!(snapshot.uptime_seconds, 90061);
        assert_eq!(snapshot.cpu_cores, 8);
        assert_eq!(snapshot.mem_available, 10000);
        assert_eq!(snapshot.processes[0].command, "sshd");
        assert_eq!(snapshot.network_interfaces[0].name, "eth0");
        assert_eq!(snapshot.network_interfaces[0].receive_rate, 128);
        assert_eq!(snapshot.disks[0].mount, "/");
        assert_eq!(snapshot.filesystems.len(), 2);
        assert_eq!(snapshot.filesystems[1].mount, "/run");
    }
}
