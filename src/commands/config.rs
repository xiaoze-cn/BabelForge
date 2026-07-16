use crate::app::App;
use crate::commands::output;
use crate::config::{self, Provider};
use crate::error::{BfxError, Result};
use clap::{Args, Subcommand};
use serde_json::json;

#[derive(Args)]
pub struct ConfigArgs {
    #[command(subcommand)]
    command: Option<ConfigCommand>,
}

#[derive(Subcommand)]
enum ConfigCommand {
    Providers,
    Presets,
    Model(ModelArgs),
    Preset(PresetArgs),
}

#[derive(Args)]
struct ModelArgs {
    #[command(subcommand)]
    command: ModelCommand,
}

#[derive(Subcommand)]
enum ModelCommand {
    Set(ModelSet),
    Remove(NameArgs),
}

#[derive(Args)]
struct ModelSet {
    name: String,
    #[arg(long)]
    model: String,
    #[arg(long)]
    url: String,
    #[arg(long)]
    key: String,
}

#[derive(Args)]
struct PresetArgs {
    #[command(subcommand)]
    command: PresetCommand,
}

#[derive(Subcommand)]
enum PresetCommand {
    Set(PresetSet),
    Remove(NameArgs),
}

#[derive(Args)]
struct PresetSet {
    name: String,
    #[arg(long)]
    pages: Option<String>,
    #[arg(long)]
    language: Option<String>,
    #[arg(long)]
    format: Option<String>,
    #[arg(long)]
    destination: Option<String>,
    #[arg(long)]
    watermark: Option<bool>,
}

#[derive(Args)]
struct NameArgs {
    name: String,
}

pub fn command(app: &App, args: ConfigArgs, json: bool) -> Result<()> {
    match args.command {
        None => {
            if json {
                return output::json(json!({
                    "config": app.paths.config,
                }));
            }
            println!("Config: {}", app.paths.config.display());
            Ok(())
        }
        Some(ConfigCommand::Providers) => list_models(app, json),
        Some(ConfigCommand::Presets) => list_presets(app, json),
        Some(ConfigCommand::Model(args)) => model_command(app, args.command, json),
        Some(ConfigCommand::Preset(args)) => preset_command(app, args.command, json),
    }
}

fn list_models(app: &App, json: bool) -> Result<()> {
    let config = config::read_config(&app.paths.config)?;
    if json {
        return output::json(json!({ "models": config.models.keys().collect::<Vec<_>>() }));
    }
    if config.models.is_empty() {
        println!("None");
    } else {
        for name in config.models.keys() {
            println!("{name}");
        }
    }
    Ok(())
}

fn list_presets(app: &App, json: bool) -> Result<()> {
    let config = config::read_config(&app.paths.config)?;
    if json {
        return output::json(json!({ "presets": config.presets.keys().collect::<Vec<_>>() }));
    }
    if config.presets.is_empty() {
        println!("None");
    } else {
        for name in config.presets.keys() {
            println!("{name}");
        }
    }
    Ok(())
}

fn model_command(app: &App, command: ModelCommand, json: bool) -> Result<()> {
    let mut config = config::read_config(&app.paths.config)?;
    match command {
        ModelCommand::Set(args) => {
            config.models.insert(
                args.name.clone(),
                Provider {
                    model: args.model,
                    url: config::openai_base_url(&args.url)?,
                    key: args.key,
                },
            );
            config::write_config(&app.paths.config, &config)?;
            if json {
                return output::json(json!({ "action": "saved", "model": args.name }));
            }
            println!("Done [BFX-CFG]: Saved model \"{}\"", args.name);
        }
        ModelCommand::Remove(args) => {
            let key = matching_key(config.models.keys(), &args.name)?;
            config.models.remove(&key);
            config::write_config(&app.paths.config, &config)?;
            if json {
                return output::json(json!({ "action": "removed", "model": key }));
            }
            println!("Done [BFX-CFG]: Removed model \"{key}\"");
        }
    }
    Ok(())
}

fn preset_command(app: &App, command: PresetCommand, json: bool) -> Result<()> {
    let mut config = config::read_config(&app.paths.config)?;
    match command {
        PresetCommand::Set(args) => {
            let key = config
                .presets
                .keys()
                .find(|saved| saved.eq_ignore_ascii_case(&args.name))
                .cloned()
                .unwrap_or_else(|| args.name.clone());
            let mut preset = config
                .presets
                .get(&key)
                .cloned()
                .unwrap_or_else(config::default_preset);
            if let Some(value) = args.pages {
                preset.pages = value;
            }
            if let Some(value) = args.language {
                preset.language = value;
            }
            if let Some(value) = args.format {
                preset.format = value;
            }
            if let Some(value) = args.destination {
                preset.destination = value;
            }
            if let Some(value) = args.watermark {
                preset.watermark = value;
            }
            config::check_preset(&key, &preset)?;
            config.presets.insert(key.clone(), preset);
            config::write_config(&app.paths.config, &config)?;
            if json {
                return output::json(json!({ "action": "saved", "preset": key }));
            }
            println!("Done [BFX-CFG]: Saved preset \"{key}\"");
        }
        PresetCommand::Remove(args) => {
            let key = matching_key(config.presets.keys(), &args.name)?;
            if key.eq_ignore_ascii_case("Default") {
                return Err(BfxError::config("Preset \"Default\" cannot be removed"));
            }
            config.presets.remove(&key);
            config::write_config(&app.paths.config, &config)?;
            if json {
                return output::json(json!({ "action": "removed", "preset": key }));
            }
            println!("Done [BFX-CFG]: Removed preset \"{key}\"");
        }
    }
    Ok(())
}

fn matching_key<'a>(mut keys: impl Iterator<Item = &'a String>, name: &str) -> Result<String> {
    keys.find(|saved| saved.eq_ignore_ascii_case(name))
        .cloned()
        .ok_or_else(|| BfxError::config(format!("\"{name}\" is not configured")))
}
