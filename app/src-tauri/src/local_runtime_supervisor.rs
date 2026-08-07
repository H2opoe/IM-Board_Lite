use std::net::TcpListener;
use std::process::Command;
use std::sync::atomic::{AtomicU16, AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use tokio::sync::{Mutex as AsyncMutex, MutexGuard};

pub struct LocalRuntimeSupervisor {
    port: AtomicU16,
    process: Mutex<Option<OwnedRuntimeProcess>>,
    generation: AtomicU64,
    startup_lock: AsyncMutex<()>,
}

#[derive(Clone, Copy)]
struct OwnedRuntimeProcess {
    pid: u32,
    generation: u64,
}

impl LocalRuntimeSupervisor {
    pub fn new() -> anyhow::Result<Self> {
        Ok(Self {
            port: AtomicU16::new(available_loopback_port()?),
            process: Mutex::new(None),
            generation: AtomicU64::new(0),
            startup_lock: AsyncMutex::new(()),
        })
    }

    pub async fn startup_guard(&self) -> MutexGuard<'_, ()> {
        self.startup_lock.lock().await
    }

    pub fn renew_endpoint(&self) -> Result<String, String> {
        let port = available_loopback_port().map_err(|error| error.to_string())?;
        self.port.store(port, Ordering::SeqCst);
        Ok(self.base_url())
    }

    pub fn base_url(&self) -> String {
        format!("http://127.0.0.1:{}/v1", self.port.load(Ordering::SeqCst))
    }

    pub fn port(&self) -> u16 {
        self.port.load(Ordering::SeqCst)
    }

    pub fn register(&self, pid: u32) -> u64 {
        let generation = self.generation.fetch_add(1, Ordering::SeqCst) + 1;
        if let Ok(mut process) = self.process.lock() {
            *process = Some(OwnedRuntimeProcess { pid, generation });
        }
        generation
    }

    pub fn clear_if_owned(&self, pid: u32, generation: u64) -> bool {
        self.process
            .lock()
            .map(|mut process| {
                if process.is_some_and(|owned| owned.pid == pid && owned.generation == generation) {
                    *process = None;
                    true
                } else {
                    false
                }
            })
            .unwrap_or(false)
    }

    pub fn has_owned_process(&self) -> bool {
        self.process
            .lock()
            .map(|process| process.is_some())
            .unwrap_or(false)
    }

    pub fn stop(&self) -> Result<bool, String> {
        let process = self
            .process
            .lock()
            .map_err(|error| error.to_string())?
            .take();
        let Some(process) = process else {
            return Ok(false);
        };
        terminate_process_tree(process.pid)?;
        Ok(true)
    }

    pub async fn wait_until_ready(&self, model: &str, timeout: Duration) -> bool {
        let started_at = Instant::now();
        while started_at.elapsed() < timeout {
            if self.is_owned_runtime_ready(model).await {
                return true;
            }
            tokio::time::sleep(Duration::from_millis(700)).await;
        }
        false
    }

    pub async fn is_owned_runtime_ready(&self, model: &str) -> bool {
        if !self.has_owned_process() {
            return false;
        }
        let Ok(client) = reqwest::Client::builder()
            .timeout(Duration::from_secs(3))
            .build()
        else {
            return false;
        };
        let Ok(response) = client
            .get(format!("{}/models", self.base_url()))
            .send()
            .await
        else {
            return false;
        };
        if !response.status().is_success() {
            return false;
        }
        response
            .json::<serde_json::Value>()
            .await
            .ok()
            .and_then(|value| value.get("data").and_then(|data| data.as_array()).cloned())
            .is_some_and(|models| {
                models.iter().any(|item| {
                    item.get("id")
                        .and_then(|id| id.as_str())
                        .is_some_and(|id| id == model)
                })
            })
    }
}

impl Drop for LocalRuntimeSupervisor {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}

fn available_loopback_port() -> anyhow::Result<u16> {
    let listener = TcpListener::bind(("127.0.0.1", 0))?;
    Ok(listener.local_addr()?.port())
}

#[cfg(windows)]
fn terminate_process_tree(pid: u32) -> Result<(), String> {
    let status = Command::new("taskkill")
        .args(["/PID", &pid.to_string(), "/T", "/F"])
        .status()
        .map_err(|error| format!("终止本地推理进程失败：{error}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("终止本地推理进程失败：{status}"))
    }
}

#[cfg(not(windows))]
fn terminate_process_tree(pid: u32) -> Result<(), String> {
    let status = Command::new("kill")
        .args(["-TERM", &pid.to_string()])
        .status()
        .map_err(|error| format!("终止本地推理进程失败：{error}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("终止本地推理进程失败：{status}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allocates_a_dynamic_loopback_endpoint() {
        let supervisor = LocalRuntimeSupervisor::new().expect("supervisor");
        assert!(supervisor.port() > 0);
        assert!(supervisor.base_url().starts_with("http://127.0.0.1:"));
        assert!(supervisor.base_url().ends_with("/v1"));
    }

    #[test]
    fn clears_only_the_process_generation_it_owns() {
        let supervisor = LocalRuntimeSupervisor::new().expect("supervisor");
        let first_generation = supervisor.register(10_001);
        let second_generation = supervisor.register(10_002);
        assert!(!supervisor.clear_if_owned(10_001, first_generation));
        assert!(supervisor.has_owned_process());
        assert!(supervisor.clear_if_owned(10_002, second_generation));
        assert!(!supervisor.has_owned_process());
    }
}
