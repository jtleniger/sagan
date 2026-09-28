use async_trait::async_trait;
use loco_rs::{
    app::{AppContext, Hooks, Initializer},
    bgworker::{BackgroundWorker, Queue},
    boot::{create_app, BootResult, StartMode},
    config::Config,
    controller::AppRoutes,
    db::truncate_table,
    environment::Environment,
    task::Tasks,
    Result,
};
use migration::Migrator;
use std::{path::Path, sync::Arc};

#[allow(unused_imports)]
use crate::{
    controllers,
    hardware::{Hardware, HardwareConfig},
    initializers,
    models::_entities::{app_settings, runtime_heartbeats, users},
    monitor::SystemMonitor,
    queue, storage, tasks, workers,
};

pub struct App;
#[async_trait]
impl Hooks for App {
    fn app_name() -> &'static str {
        env!("CARGO_CRATE_NAME")
    }

    fn app_version() -> String {
        format!(
            "{} ({})",
            env!("CARGO_PKG_VERSION"),
            option_env!("BUILD_SHA")
                .or(option_env!("GITHUB_SHA"))
                .unwrap_or("dev")
        )
    }

    async fn boot(
        mode: StartMode,
        environment: &Environment,
        config: Config,
    ) -> Result<BootResult> {
        create_app::<Self, Migrator>(mode, environment, config).await
    }

    /// Builds the per-process singletons once: the app's file store (Loco's boot default is
    /// the null driver, which fails every write), the `/system` page's monitor (sysinfo's CPU
    /// usage is a delta between two reads, so every request must share one `System`) and the
    /// hardware bundle (a fan, I2C bus and camera are one-per-process resources).
    async fn after_context(ctx: AppContext) -> Result<AppContext> {
        // `into_builder`, not `AppContext::builder`: the mailer, queue provider, cache and
        // shared store the boot sequence already built must survive the one component this
        // hook replaces.
        let storage = storage::store(&ctx.config)?;
        let ctx = ctx.into_builder().storage(storage).build();

        // The `/jobs` page's view of the queue: absent when this mode keeps no queue, which
        // is what makes the page say so instead of showing an empty table.
        if let Some(inspector) = queue::Inspector::from_config(&ctx.config).await? {
            ctx.shared_store.insert(Arc::new(inspector));
        }

        ctx.shared_store.insert(Arc::new(SystemMonitor::new()));
        ctx.shared_store.insert(Arc::new(Hardware::from_config(
            &HardwareConfig::from_context(&ctx.config)?,
        )?));
        Ok(ctx)
    }

    async fn initializers(_ctx: &AppContext) -> Result<Vec<Box<dyn Initializer>>> {
        Ok(vec![Box::new(
            initializers::view_engine::ViewEngineInitializer,
        )])
    }

    fn routes(_ctx: &AppContext) -> AppRoutes {
        AppRoutes::with_default_routes() // controller routes below
            .add_route(controllers::system::routes())
            .add_route(controllers::logs::routes())
            .add_route(controllers::jobs::routes())
            .add_route(controllers::dashboard::routes())
            .add_route(controllers::auth::routes())
            .add_route(controllers::configuration::routes())
    }
    async fn connect_workers(ctx: &AppContext, queue: &Queue) -> Result<()> {
        // Every worker registered here must also appear in `workers::configured()`, which is
        // what the `/jobs` page lists.
        queue
            .register(workers::capture::CaptureWorker::build(ctx))
            .await?;
        queue
            .register(workers::heartbeat::HeartbeatWorker::build(ctx))
            .await?;
        Ok(())
    }

    fn register_tasks(tasks: &mut Tasks) {
        tasks.register(tasks::enqueue_capture::EnqueueCapture);
        tasks.register(tasks::hardware_check::HardwareCheck);
        tasks.register(tasks::heartbeat::Heartbeat);
        // tasks-inject (do not remove)
    }
    async fn truncate(ctx: &AppContext) -> Result<()> {
        truncate_table(&ctx.db, users::Entity).await?;
        truncate_table(&ctx.db, app_settings::Entity).await?;
        truncate_table(&ctx.db, runtime_heartbeats::Entity).await?;
        Ok(())
    }
    async fn seed(_ctx: &AppContext, _base: &Path) -> Result<()> {
        // seed data lives in migration/src/m20260926_000002_seed_default_user.rs
        Ok(())
    }
}
