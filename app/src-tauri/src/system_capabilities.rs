use std::path::Path;

use serde::{Deserialize, Serialize};
use sysinfo::{Disks, System};

const GIB: u64 = 1024 * 1024 * 1024;
const LOCAL_MODEL_RECOMMENDED_MEMORY_BYTES: u64 = 8 * GIB;
const LOCAL_MODEL_REQUIRED_DISK_BYTES: u64 = 7 * GIB;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SystemCapabilities {
    pub os: String,
    pub architecture: String,
    pub logical_cpu_cores: usize,
    pub physical_cpu_cores: usize,
    pub total_memory_bytes: u64,
    pub available_memory_bytes: u64,
    pub available_disk_bytes: u64,
    pub recommended_analysis_batch_size: i64,
    pub local_model_supported: bool,
    pub warnings: Vec<String>,
}

impl SystemCapabilities {
    pub fn detect(app_dir: &Path) -> Self {
        let system = System::new_all();
        let total_memory_bytes = system.total_memory();
        let available_memory_bytes = system.available_memory();
        let logical_cpu_cores = system.cpus().len().max(
            std::thread::available_parallelism()
                .map(usize::from)
                .unwrap_or(1),
        );
        let physical_cpu_cores = System::physical_core_count().unwrap_or(logical_cpu_cores);
        let available_disk_bytes = available_space_for(app_dir);
        let recommended_analysis_batch_size =
            recommended_batch_size(total_memory_bytes, logical_cpu_cores);
        let mut warnings = Vec::new();
        if total_memory_bytes > 0 && total_memory_bytes < LOCAL_MODEL_RECOMMENDED_MEMORY_BYTES {
            warnings.push("系统内存低于8GB，已自动降低本地AI单批消息数量。".to_owned());
        }
        if available_disk_bytes > 0 && available_disk_bytes < LOCAL_MODEL_REQUIRED_DISK_BYTES {
            warnings.push("应用数据盘剩余空间低于7GB，暂不建议下载本地模型。".to_owned());
        }
        let local_model_supported = (total_memory_bytes == 0
            || total_memory_bytes >= LOCAL_MODEL_RECOMMENDED_MEMORY_BYTES)
            && (available_disk_bytes == 0
                || available_disk_bytes >= LOCAL_MODEL_REQUIRED_DISK_BYTES);

        Self {
            os: std::env::consts::OS.to_owned(),
            architecture: System::cpu_arch(),
            logical_cpu_cores,
            physical_cpu_cores,
            total_memory_bytes,
            available_memory_bytes,
            available_disk_bytes,
            recommended_analysis_batch_size,
            local_model_supported,
            warnings,
        }
    }

    pub fn ensure_local_model_disk_space(&self) -> Result<(), String> {
        if self.available_disk_bytes > 0
            && self.available_disk_bytes < LOCAL_MODEL_REQUIRED_DISK_BYTES
        {
            return Err(format!(
                "本地模型下载和解压至少需要7GB可用空间，当前约{:.1}GB。请先清理应用数据盘。",
                self.available_disk_bytes as f64 / GIB as f64
            ));
        }
        Ok(())
    }
}

fn available_space_for(path: &Path) -> u64 {
    let disks = Disks::new_with_refreshed_list();
    disks
        .list()
        .iter()
        .filter(|disk| path.starts_with(disk.mount_point()))
        .max_by_key(|disk| disk.mount_point().components().count())
        .map(|disk| disk.available_space())
        .unwrap_or(0)
}

fn recommended_batch_size(total_memory_bytes: u64, logical_cpu_cores: usize) -> i64 {
    if (total_memory_bytes > 0 && total_memory_bytes < 8 * GIB) || logical_cpu_cores <= 4 {
        10
    } else if total_memory_bytes > 0 && total_memory_bytes < 16 * GIB {
        15
    } else {
        20
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn low_resource_devices_get_smaller_batches() {
        assert_eq!(recommended_batch_size(4 * GIB, 8), 10);
        assert_eq!(recommended_batch_size(12 * GIB, 8), 15);
        assert_eq!(recommended_batch_size(32 * GIB, 8), 20);
        assert_eq!(recommended_batch_size(32 * GIB, 4), 10);
    }
}
