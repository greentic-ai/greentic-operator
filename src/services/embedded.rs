use std::{
    env,
    future::Future,
    path::{Path, PathBuf},
    thread,
};

use anyhow::{Context, Error, Result};
use gsm_core;
use gsm_egress::run as run_egress;
use gsm_gateway::{config::GatewayConfig, run as run_gateway};
use gsm_subscriptions_teams::{WorkerConfig, run_worker as run_subscriptions};
use tokio::runtime::Runtime;

/// Handle for an embedded service running inside the operator process.
pub struct EmbeddedServiceHandle {
    name: String,
    join: thread::JoinHandle<Result<()>>,
}

impl EmbeddedServiceHandle {
    /// Returns the canonical service name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Blocks until the embedded service exits.
    pub fn wait(self) -> Result<()> {
        let EmbeddedServiceHandle { name, join } = self;
        join.join()
            .map_err(|err| anyhow::anyhow!("embedded service {name} panicked: {err:?}"))?
    }
}

fn spawn_async<F, Fut>(name: &str, task: F) -> Result<EmbeddedServiceHandle>
where
    F: FnOnce() -> Fut + Send + 'static,
    Fut: Future<Output = Result<()>> + Send + 'static,
{
    let name = name.to_string();
    let thread_name = format!("embedded-{name}");
    let builder = thread::Builder::new().name(thread_name);
    let join = builder
        .spawn(move || {
            let runtime =
                Runtime::new().context("failed to create tokio runtime for embedded service")?;
            runtime.block_on(task())
        })
        .context("failed to spawn embedded service thread")?;
    Ok(EmbeddedServiceHandle { name, join })
}

/// Starts `gsm-gateway` inside the operator process using `GatewayConfig::load`.
pub fn spawn_gateway() -> Result<EmbeddedServiceHandle> {
    let config = GatewayConfig::load()?;
    let env = config.env.clone();
    spawn_async("gsm-gateway", move || async move {
        gsm_core::set_current_env(env);
        run_gateway(config).await
    })
}

/// Starts `gsm-egress` inside the operator process using its default loader.
pub fn spawn_egress() -> Result<EmbeddedServiceHandle> {
    spawn_async("gsm-egress", || async move { run_egress().await })
}

/// Starts `gsm-subscriptions-teams` inside the operator process using `WorkerConfig::load`.
pub fn spawn_subscriptions() -> Result<EmbeddedServiceHandle> {
    let config = WorkerConfig::load()?;
    spawn_async("gsm-subscriptions-teams", move || async move {
        run_subscriptions(config).await
    })
}

/// Starts all embedded GSM services and blocks until a signal is received.
pub fn run_services(root: &Path) -> Result<()> {
    let _guard = WorkingDirGuard::enter(root)?;
    let handles = vec![spawn_gateway()?, spawn_egress()?, spawn_subscriptions()?];

    println!("embedded GSM services running (gateway/egress/subscriptions); press Ctrl+C to stop.");
    let runtime = Runtime::new()?;
    runtime.block_on(async {
        tokio::signal::ctrl_c().await?;
        Ok::<(), Error>(())
    })?;
    println!("shutting down embedded GSM services...");
    for handle in handles {
        handle.wait()?;
    }
    Ok(())
}

struct WorkingDirGuard {
    original: PathBuf,
}

impl WorkingDirGuard {
    fn enter<P: AsRef<Path>>(root: P) -> Result<Self> {
        let original = env::current_dir()?;
        env::set_current_dir(root.as_ref())?;
        Ok(Self { original })
    }
}

impl Drop for WorkingDirGuard {
    fn drop(&mut self) {
        let _ = env::set_current_dir(&self.original);
    }
}
