use crate::app::App;
use crate::commands::output;
use crate::config;
use crate::engine;
use crate::error::{BfxError, Result};
use clap::Args;
use serde_json::json;

#[derive(Args)]
pub struct UpdateArgs {
    #[arg(long)]
    check: bool,
}

pub fn info(app: &App, json: bool) -> Result<()> {
    let babeldoc = engine::version(&app.root)?;
    let version = babeldoc.strip_prefix("babeldoc ").unwrap_or(&babeldoc);
    if json {
        return output::json(json!({
            "version": env!("CARGO_PKG_VERSION"),
            "babeldoc": version,
            "config": app.paths.config_dir,
            "data": app.paths.data_dir,
        }));
    }
    println!("[Version] {}", env!("CARGO_PKG_VERSION"));
    println!("[BabelDOC] {version}");
    println!("[Config] {}", app.paths.config_dir.display());
    println!("[Data] {}", app.paths.data_dir.display());
    Ok(())
}

pub fn doctor(app: &App, json: bool) -> Result<()> {
    let config = config::read_config(&app.paths.config)?;
    for (name, preset) in &config.presets {
        config::check_preset(name, preset)?;
    }
    for (name, provider) in &config.models {
        config::openai_base_url(&provider.url).map_err(|error| {
            BfxError::config(format!(
                "Model \"{name}\" has an invalid URL ({})",
                error.message
            ))
        })?;
        config::read_key(&provider.key)
            .map_err(|_| BfxError::config(format!("Model \"{name}\" has no available API key")))?;
    }
    engine::version(&app.root)?;
    if json {
        return output::json(json!({ "config": "OK", "babeldoc": "OK" }));
    }
    println!("[Config] OK");
    println!("[BabelDOC] OK");
    Ok(())
}

pub fn update(_args: UpdateArgs) -> Result<()> {
    Err(BfxError::update("No update source is configured"))
}
