use serde::Serialize;
use sysinfo::Disks;
use sysinfo::System;

#[derive(Serialize, Default)]
pub struct ServerMetrics {
    pub ram_used_mb: f64,
    pub ram_total_mb: f64,
    pub ram_percent: f64,
    pub cpu_percent: f64,
    pub cpu_available: bool,
    pub disk_total_gb: u64,
    pub disk_available_gb: u64,
    pub disk_percent: f64,
}

/// Collect a snapshot of server resource usage.
pub fn collect() -> ServerMetrics {
    let mut sys = System::new();
    sys.refresh_memory();
    sys.refresh_cpu_usage();

    let ram_total = sys.total_memory();
    let ram_used = sys.used_memory();
    let ram_used_mb = ram_used as f64 / 1_048_576.0;
    let ram_total_mb = ram_total as f64 / 1_048_576.0;
    let ram_percent = if ram_total > 0 {
        ram_used as f64 / ram_total as f64 * 100.0
    } else {
        0.0
    };

    let cpus = sys.cpus();
    let cpu_available = !cpus.is_empty();
    let cpu_percent = if cpu_available {
        let sum: f32 = cpus.iter().map(|c| c.cpu_usage()).sum();
        (sum / cpus.len() as f32) as f64
    } else {
        0.0
    };

    let disks = Disks::new_with_refreshed_list();
    let (mut disk_total, mut disk_available) = (0u64, 0u64);
    if let Some(disk) = disks.list().first() {
        disk_total = disk.total_space();
        disk_available = disk.available_space();
    }
    let disk_total_gb = disk_total / 1_073_741_824;
    let disk_available_gb = disk_available / 1_073_741_824;
    let disk_percent = if disk_total > 0 {
        (disk_total - disk_available) as f64 / disk_total as f64 * 100.0
    } else {
        0.0
    };

    ServerMetrics {
        ram_used_mb,
        ram_total_mb,
        ram_percent,
        cpu_percent,
        cpu_available,
        disk_total_gb,
        disk_available_gb,
        disk_percent,
    }
}
