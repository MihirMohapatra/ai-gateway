use crate::GatewayConfig;

pub fn load(path: &str) -> anyhow::Result<GatewayConfig> {
    let contents = std::fs::read_to_string(path)?;
    let config: GatewayConfig = toml::from_str(&contents)?;
    Ok(config)
}
