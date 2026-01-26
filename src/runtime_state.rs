use std::path::{Path, PathBuf};

use serde::Serialize;
use serde::de::DeserializeOwned;

pub struct RuntimePaths {
    state_dir: PathBuf,
    tenant: String,
    team: String,
}

impl RuntimePaths {
    pub fn new(
        state_dir: impl Into<PathBuf>,
        tenant: impl Into<String>,
        team: impl Into<String>,
    ) -> Self {
        Self {
            state_dir: state_dir.into(),
            tenant: tenant.into(),
            team: team.into(),
        }
    }

    pub fn key(&self) -> String {
        format!("{}.{}", self.tenant, self.team)
    }

    pub fn runtime_root(&self) -> PathBuf {
        self.state_dir.join("runtime").join(self.key())
    }

    pub fn pids_dir(&self) -> PathBuf {
        self.state_dir.join("pids").join(self.key())
    }

    pub fn logs_dir(&self) -> PathBuf {
        self.state_dir.join("logs").join(self.key())
    }

    pub fn resolved_dir(&self) -> PathBuf {
        self.runtime_root().join("resolved")
    }

    pub fn pid_path(&self, service_id: &str) -> PathBuf {
        self.pids_dir().join(format!("{service_id}.pid"))
    }

    pub fn log_path(&self, service_id: &str) -> PathBuf {
        self.logs_dir().join(format!("{service_id}.log"))
    }

    pub fn resolved_path(&self, service_id: &str) -> PathBuf {
        self.resolved_dir().join(format!("{service_id}.json"))
    }

    pub fn logs_root(&self) -> PathBuf {
        self.state_dir.join("logs")
    }
}

pub fn write_json<T: Serialize>(path: &Path, value: &T) -> anyhow::Result<()> {
    let bytes = serde_json::to_vec_pretty(value)?;
    atomic_write(path, &bytes)
}

pub fn read_json<T: DeserializeOwned>(path: &Path) -> anyhow::Result<Option<T>> {
    if !path.exists() {
        return Ok(None);
    }
    let data = std::fs::read(path)?;
    let value = serde_json::from_slice(&data)?;
    Ok(Some(value))
}

pub fn atomic_write(path: &Path, bytes: &[u8]) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut tmp = path.to_path_buf();
    tmp.set_extension("tmp");
    std::fs::write(&tmp, bytes)?;
    if path.exists() {
        let _ = std::fs::remove_file(path);
    }
    std::fs::rename(&tmp, path)?;
    Ok(())
}
