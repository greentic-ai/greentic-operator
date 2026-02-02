use anyhow::{Result as AnyhowResult, anyhow};
use async_trait::async_trait;
use greentic_secrets_lib::{
    Result as SecretResult, SecretError, SecretsManager, SecretsStore,
    core::{Error as CoreError, seed::DevStore},
};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

pub struct SecretsClient {
    store: Arc<DevStore>,
    store_path: Option<PathBuf>,
}

impl SecretsClient {
    pub fn open(bundle_root: &Path) -> AnyhowResult<Self> {
        if let Some(path) =
            override_dev_store_path().or_else(|| find_default_dev_store_path(bundle_root))
            && path.exists()
        {
            return Self::open_with_path(path);
        }
        let store = DevStore::open_default()
            .map_err(|err| anyhow!("failed to open dev secrets store: {err}"))?;
        Ok(Self {
            store: Arc::new(store),
            store_path: None,
        })
    }

    pub fn open_with_path(path: PathBuf) -> AnyhowResult<Self> {
        let store = DevStore::with_path(path.clone())
            .map_err(|err| anyhow!("failed to open dev secrets store: {err}"))?;
        Ok(Self {
            store: Arc::new(store),
            store_path: Some(path),
        })
    }

    pub fn store_path(&self) -> Option<&Path> {
        self.store_path.as_deref()
    }
}

#[async_trait]
impl SecretsManager for SecretsClient {
    async fn read(&self, path: &str) -> SecretResult<Vec<u8>> {
        let result = self.store.get(path).await;
        match result {
            Ok(value) => Ok(value),
            Err(CoreError::NotFound { entity }) => Err(SecretError::NotFound(entity)),
            Err(err) => Err(SecretError::Backend(err.to_string().into())),
        }
    }

    async fn write(&self, _: &str, _: &[u8]) -> SecretResult<()> {
        Err(SecretError::Permission(
            "dev secrets store is read-only".into(),
        ))
    }

    async fn delete(&self, _: &str) -> SecretResult<()> {
        Err(SecretError::Permission(
            "dev secrets store is read-only".into(),
        ))
    }
}

fn override_dev_store_path() -> Option<PathBuf> {
    std::env::var("GREENTIC_DEV_SECRETS_PATH")
        .ok()
        .map(PathBuf::from)
}

fn find_default_dev_store_path(bundle_root: &Path) -> Option<PathBuf> {
    let candidates = [
        bundle_root.join(".greentic/dev/.dev.secrets.env"),
        bundle_root.join(".greentic/state/dev/.dev.secrets.env"),
    ];
    candidates.into_iter().find(|path| path.exists())
}

#[cfg(test)]
mod tests {
    use super::*;
    use greentic_secrets_lib::{
        SecretFormat, SeedDoc, SeedEntry, SeedValue,
        core::seed::{ApplyOptions, DevStore, apply_seed},
    };
    use tempfile::tempdir;
    use tokio::runtime::Runtime;

    #[test]
    fn reads_seeded_secret_from_dev_store() -> anyhow::Result<()> {
        let dir = tempdir()?;
        let store_path = dir.path().join("secrets.env");
        let store = DevStore::with_path(store_path.clone())?;
        let seed = SeedDoc {
            entries: vec![SeedEntry {
                uri: "secrets://demo/acme/_/mypack/my_secret".to_string(),
                format: SecretFormat::Text,
                value: SeedValue::Text {
                    text: "hello world".to_string(),
                },
                description: None,
            }],
        };
        let runtime = Runtime::new()?;
        let report =
            runtime.block_on(async { apply_seed(&store, &seed, ApplyOptions::default()).await });
        assert_eq!(report.ok, 1);
        let client = SecretsClient::open_with_path(store_path.clone())?;
        let value = runtime
            .block_on(async { client.read("secrets://demo/acme/_/mypack/my_secret").await })?;
        assert_eq!(value, b"hello world".to_vec());
        Ok(())
    }
}
