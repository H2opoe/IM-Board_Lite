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
    pub local_model_profile: String,
    pub local_model_context_size: u32,
    pub local_model_gpu_layers: u32,
    pub local_model_threads: usize,
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
        let runtime_arch_supported = matches!(
            (std::env::consts::OS, std::env::consts::ARCH),
            ("macos", "aarch64") | ("macos", "x86_64") | ("windows", "x86_64")
        );
        let local_model_supported = runtime_arch_supported
            && (total_memory_bytes == 0
                || total_memory_bytes >= LOCAL_MODEL_RECOMMENDED_MEMORY_BYTES)
            && (available_memory_bytes == 0 || available_memory_bytes >= 5 * GIB)
            && (available_disk_bytes == 0
                || available_disk_bytes >= LOCAL_MODEL_REQUIRED_DISK_BYTES);
        if !runtime_arch_supported {
            warnings.push(format!(
                "当前系统架构 {}/{} 暂不支持包内本地AI，可继续使用云端AI。",
                std::env::consts::OS,
                std::env::consts::ARCH
            ));
        }
        if available_memory_bytes > 0 && available_memory_bytes < 5 * GIB {
            warnings.push("当前可用内存低于5GB，暂不启动本地AI，以免系统卡顿。".to_owned());
        }
        let (local_model_profile, local_model_context_size) =
            local_model_profile(available_memory_bytes, total_memory_bytes);
        let local_model_gpu_layers = if std::env::consts::OS == "macos" {
            match local_model_profile.as_str() {
                "performance" => 99,
                "balanced" => 48,
                _ => 20,
            }
        } else {
            0
        };
        let local_model_threads = physical_cpu_cores.clamp(1, 12);

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
            local_model_profile,
            local_model_context_size,
            local_model_gpu_layers,
            local_model_threads,
            warnings,
        }
    }

    pub fn ensure_local_model_disk_space(&self) -> Result<(), String> {
        if !self.local_model_supported {
            return Err("当前可用内存或磁盘空间不足，暂不建议启动7B本地模型。可改用云端AI，或释放内存和磁盘后重试。".to_owned());
        }
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

// Keep the server context aligned with the analysis request budget on every hardware tier.
fn local_model_profile(available_memory_bytes: u64, total_memory_bytes: u64) -> (String, u32) {
    let memory = if available_memory_bytes > 0 {
        available_memory_bytes
    } else {
        total_memory_bytes
    };
    if memory > 0 && memory < 8 * GIB {
        ("eco".to_owned(), crate::ai::LOCAL_DEEPSEEK_CONTEXT_SIZE)
    } else if memory > 0 && memory < 16 * GIB {
        (
            "balanced".to_owned(),
            crate::ai::LOCAL_DEEPSEEK_CONTEXT_SIZE,
        )
    } else {
        (
            "performance".to_owned(),
            crate::ai::LOCAL_DEEPSEEK_CONTEXT_SIZE,
        )
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

    #[test]
    fn local_model_profiles_keep_the_request_context_contract() {
        assert_eq!(
            local_model_profile(6 * GIB, 32 * GIB),
            ("eco".to_owned(), crate::ai::LOCAL_DEEPSEEK_CONTEXT_SIZE)
        );
        assert_eq!(
            local_model_profile(12 * GIB, 32 * GIB),
            (
                "balanced".to_owned(),
                crate::ai::LOCAL_DEEPSEEK_CONTEXT_SIZE
            )
        );
        assert_eq!(
            local_model_profile(24 * GIB, 32 * GIB),
            (
                "performance".to_owned(),
                crate::ai::LOCAL_DEEPSEEK_CONTEXT_SIZE
            )
        );
        assert_eq!(
            local_model_profile(0, 12 * GIB),
            (
                "balanced".to_owned(),
                crate::ai::LOCAL_DEEPSEEK_CONTEXT_SIZE
            )
        );
    }
}
