use notify::{EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use std::path::Path;
use std::sync::Arc;

use crate::GatewayConfig;

fn load_raw(path: &str) -> anyhow::Result<GatewayConfig> {
    let contents = std::fs::read_to_string(path)?;
    let config: GatewayConfig = toml::from_str(&contents)?;
    Ok(config)
}

fn apply_env_overrides(config: &mut GatewayConfig) {
    macro_rules! override_if {
        ($var:literal, $field:expr) => {
            if let Ok(val) = std::env::var($var) {
                $field = val.into();
            }
        };
        ($var:literal, $field:expr, parse) => {
            if let Ok(val) = std::env::var($var) {
                if let Ok(parsed) = val.parse() {
                    $field = parsed;
                }
            }
        };
    }

    override_if!("GATEWAY__SERVER__HOST", config.server.host);
    override_if!("GATEWAY__SERVER__PORT", config.server.port, parse);
    override_if!("GATEWAY__AUTH__JWT_SECRET", config.auth.jwt_secret);
    override_if!("GATEWAY__CACHE__REDIS_URL", config.cache.redis_url);

    if let Some(ref mut openai) = config.providers.openai {
        override_if!("GATEWAY__PROVIDERS__OPENAI__API_KEY", openai.api_key);
        override_if!("GATEWAY__PROVIDERS__OPENAI__BASE_URL", openai.base_url);
    }
    if let Some(ref mut anthropic) = config.providers.anthropic {
        override_if!("GATEWAY__PROVIDERS__ANTHROPIC__API_KEY", anthropic.api_key);
        override_if!("GATEWAY__PROVIDERS__ANTHROPIC__BASE_URL", anthropic.base_url);
    }

    if let Some(ref mut metering) = config.metering {
        override_if!("GATEWAY__METERING__DATABASE_URL", metering.database_url);
    }
}

pub fn load(path: &str) -> anyhow::Result<GatewayConfig> {
    let mut config = load_raw(path)?;
    apply_env_overrides(&mut config);
    Ok(config)
}

pub struct ConfigWatcher {
    _watcher: Arc<RecommendedWatcher>,
    pub rx: tokio::sync::watch::Receiver<GatewayConfig>,
}

impl ConfigWatcher {
    pub fn latest(&self) -> GatewayConfig {
        self.rx.borrow().clone()
    }
}

pub fn watch(path: &str) -> anyhow::Result<ConfigWatcher> {
    let config_path = path.to_string();
    let config = load(&config_path)?;
    let (tx, rx) = tokio::sync::watch::channel(config);

    let tx_clone = tx.clone();
    let watch_path = config_path.clone();
    let watcher = notify::recommended_watcher(move |event: Result<notify::Event, notify::Error>| {
        if let Ok(event) = event {
            if matches!(event.kind, EventKind::Modify(_) | EventKind::Create(_)) {
                if let Ok(new_config) = load(&watch_path) {
                    let _ = tx_clone.send(new_config);
                }
            }
        }
    })?;

    let mut w = watcher;
    w.watch(Path::new(&config_path), RecursiveMode::NonRecursive)?;

    Ok(ConfigWatcher {
        _watcher: Arc::new(w),
        rx,
    })
}
