mod app {
    use crate::error::{BfxError, Result};
    use crate::storage::Store;
    use directories::BaseDirs;
    use std::path::PathBuf;

    pub struct Paths {
        pub config_dir: PathBuf,
        pub data_dir: PathBuf,
        pub config: PathBuf,
    }

    pub struct App {
        pub root: PathBuf,
        pub paths: Paths,
        pub store: Store,
    }

    impl App {
        pub fn open() -> Result<Self> {
            let root = runtime_root()?;
            let base = match std::env::var_os("BFX_HOME") {
                Some(value) => PathBuf::from(value),
                None => BaseDirs::new()
                    .map(|dirs| dirs.data_local_dir().join("BabelForge").join("eXecutor"))
                    .ok_or_else(|| BfxError::storage("Cannot resolve the BFX data directory"))?,
            };
            let config_dir = base.clone();
            let data_dir = base.join("data");
            std::fs::create_dir_all(&config_dir).map_err(|error| {
                BfxError::storage(format!("Cannot create the BFX config directory ({error})"))
            })?;
            std::fs::create_dir_all(&data_dir).map_err(|error| {
                BfxError::storage(format!("Cannot create the BFX data directory ({error})"))
            })?;
            let config = std::env::var_os("BFX_CONFIG_PATH")
                .map(PathBuf::from)
                .unwrap_or_else(|| config_dir.join("config.toml"));
            let store = Store::open(data_dir.join("bfx.sqlite3"))?;
            Ok(Self {
                root,
                paths: Paths {
                    config_dir,
                    data_dir,
                    config,
                },
                store,
            })
        }
    }

    fn runtime_root() -> Result<PathBuf> {
        if let Some(value) = std::env::var_os("BFX_ROOT") {
            let root = PathBuf::from(value);
            return valid_root(root);
        }
        let executable = std::env::current_exe().map_err(|error| {
            BfxError::engine(format!("Cannot resolve the BFX executable ({error})"))
        })?;
        let root = executable
            .ancestors()
            .find(|path| has_runtime(path))
            .map(PathBuf::from)
            .ok_or_else(|| BfxError::engine("Cannot locate the BFX runtime"))?;
        valid_root(root)
    }

    fn valid_root(root: PathBuf) -> Result<PathBuf> {
        if has_runtime(&root) || has_override() {
            return Ok(root);
        }
        Err(BfxError::engine(format!(
            "BFX runtime \"{}\" is incomplete",
            root.display()
        )))
    }

    fn has_runtime(root: &std::path::Path) -> bool {
        #[cfg(windows)]
        let packaged = root.join("runtime").join("Scripts").join("babeldoc.exe");
        #[cfg(not(windows))]
        let packaged = root.join("runtime").join("bin").join("babeldoc");

        packaged.is_file()
    }

    fn has_override() -> bool {
        std::env::var_os("BFX_BABELDOC")
            .map(std::path::PathBuf::from)
            .is_some_and(|path| path.is_file())
    }
}

mod config;
mod engine;
mod error;
mod execution;
mod storage;

pub(crate) mod output {
    use crate::error::{BfxError, Result};
    use serde_json::Value;

    pub fn json(value: Value) -> Result<()> {
        let text = serde_json::to_string(&value)
            .map_err(|error| BfxError::storage(format!("Cannot format JSON output ({error})")))?;
        println!("{text}");
        Ok(())
    }

    pub fn time(duration_ms: Option<i64>) -> String {
        let seconds = duration_ms.unwrap_or(0).max(0) as u64 / 1_000;
        let hours = seconds / 3_600;
        let minutes = seconds % 3_600 / 60;
        let seconds = seconds % 60;
        format!("{hours:02}:{minutes:02}:{seconds:02}")
    }

    pub fn value(value: &str) -> String {
        let value = value.replace(['\r', '\n'], " ");
        if value.chars().any(char::is_whitespace) {
            format!("\"{}\"", value.replace('"', "\\\""))
        } else {
            value
        }
    }

    pub fn path(path: &str) -> String {
        value(&path_value(path))
    }

    pub fn path_value(value: &str) -> String {
        #[cfg(windows)]
        {
            if let Some(value) = value.strip_prefix("\\\\?\\UNC\\") {
                return format!("\\\\{value}");
            }
            if let Some(value) = value.strip_prefix("\\\\?\\") {
                return value.to_owned();
            }
        }
        value.to_owned()
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[cfg(windows)]
        #[test]
        fn removes_windows_verbatim_prefix() {
            assert_eq!(path_value("\\\\?\\C:\\Users\\BFX"), "C:\\Users\\BFX");
            assert_eq!(
                path_value("\\\\?\\UNC\\server\\share\\BFX"),
                "\\\\server\\share\\BFX"
            );
        }
    }
}

mod system {
    use crate::app::App;
    use crate::config;
    use crate::engine;
    use crate::error::{BfxError, Result};
    use crate::output;
    use clap::Args;
    use serde::Deserialize;
    use serde_json::json;
    use std::time::Duration;

    const LATEST_RELEASE: &str =
        "https://api.github.com/repos/xiaoze-cn/BabelForge/releases/latest";

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
            config::read_key(&provider.key).map_err(|_| {
                BfxError::config(format!("Model \"{name}\" has no available API key"))
            })?;
        }
        engine::version(&app.root)?;
        if json {
            return output::json(json!({ "config": "OK", "babeldoc": "OK" }));
        }
        println!("[Config] OK");
        println!("[BabelDOC] OK");
        Ok(())
    }

    pub fn update(_args: UpdateArgs, json: bool) -> Result<()> {
        let release = latest_release()?;
        let latest = release_version(&release.tag_name)?;
        let current = env!("CARGO_PKG_VERSION");
        let available = version_parts(&latest)? > version_parts(current)?;
        let asset_name = format!("BabelForge-eXecutor-{latest}-win-Setup.exe");
        let asset = release.assets.iter().find(|asset| asset.name == asset_name);
        if available && asset.is_none() {
            return Err(BfxError::update(format!(
                "Release {latest} has no Windows installer asset"
            )));
        }
        let asset_url = asset.map(|asset| asset.browser_download_url.as_str());
        if json {
            return output::json(json!({
                "current": current,
                "latest": latest,
                "update": available,
                "url": asset_url,
                "release": release.html_url,
            }));
        }
        println!("[Current] {current}");
        println!("[Latest] {latest}");
        println!(
            "[Update] {}",
            if available { "Available" } else { "Current" }
        );
        if let Some(url) = asset_url {
            println!("[URL] {url}");
        }
        Ok(())
    }

    fn latest_release() -> Result<Release> {
        let agent = ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(10)))
            .build()
            .new_agent();
        let body = agent
            .get(LATEST_RELEASE)
            .header("Accept", "application/vnd.github+json")
            .header("User-Agent", concat!("bfx/", env!("CARGO_PKG_VERSION")))
            .call()
            .map_err(|error| BfxError::update(format!("Cannot check for updates ({error})")))?
            .body_mut()
            .read_to_string()
            .map_err(|error| {
                BfxError::update(format!("Cannot read update information ({error})"))
            })?;
        serde_json::from_str(&body)
            .map_err(|error| BfxError::update(format!("Cannot parse update information ({error})")))
    }

    fn release_version(tag: &str) -> Result<String> {
        let version = tag.trim().strip_prefix('v').unwrap_or(tag.trim());
        version_parts(version)?;
        Ok(version.to_owned())
    }

    fn version_parts(version: &str) -> Result<(u64, u64, u64)> {
        let mut parts = version.split('.');
        let parse = |part: Option<&str>| {
            part.ok_or_else(|| BfxError::update(format!("Version \"{version}\" is invalid")))?
                .parse::<u64>()
                .map_err(|_| BfxError::update(format!("Version \"{version}\" is invalid")))
        };
        let parsed = (
            parse(parts.next())?,
            parse(parts.next())?,
            parse(parts.next())?,
        );
        if parts.next().is_some() {
            return Err(BfxError::update(format!(
                "Version \"{version}\" is invalid"
            )));
        }
        Ok(parsed)
    }

    #[derive(Deserialize)]
    struct Release {
        tag_name: String,
        html_url: String,
        assets: Vec<ReleaseAsset>,
    }

    #[derive(Deserialize)]
    struct ReleaseAsset {
        name: String,
        browser_download_url: String,
    }
}

use crate::app::App;
use crate::config as files;
use crate::error::{BfxError, Result};
use clap::{Parser, Subcommand, error::ErrorKind};

#[derive(Parser)]
#[command(name = "bfx", version, about = "BabelForge eXecutor")]
struct Cli {
    #[arg(long, global = true)]
    json: bool,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    Config(config::command::ConfigArgs),
    Submit(execution::submit::SubmitArgs),
    Get(execution::get::GetArgs),
    Check(execution::check::CheckArgs),
    Run(execution::submit::SubmitArgs),
    Stop(execution::stop::StopArgs),
    Retry(execution::retry::RetryArgs),
    Replace(execution::replace::ReplaceArgs),
    Info,
    Doctor,
    Update(system::UpdateArgs),
    #[command(hide = true)]
    Worker,
}

pub fn start() -> Result<()> {
    if let Some(help) = help_text() {
        print!("{help}");
        return Ok(());
    }
    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(error)
            if matches!(
                error.kind(),
                ErrorKind::DisplayHelp | ErrorKind::DisplayVersion
            ) =>
        {
            error
                .print()
                .map_err(|error| BfxError::input(format!("Cannot print command help ({error})")))?;
            return Ok(());
        }
        Err(error) => return Err(BfxError::input(input_message(&error))),
    };
    let app = App::open()?;
    files::ensure_files(&app.paths)?;
    let json = cli.json;
    match cli.command {
        Command::Config(args) => config::command::command(&app, args, json),
        Command::Submit(args) => execution::submit::command(&app, args, false, json),
        Command::Get(args) => execution::get::command(&app, args, json),
        Command::Check(args) => execution::check::command(&app, args, json),
        Command::Run(args) => execution::submit::command(&app, args, true, json),
        Command::Stop(args) => execution::stop::command(&app, args, json),
        Command::Retry(args) => execution::retry::command(&app, args, json),
        Command::Replace(args) => execution::replace::command(&app, args, json),
        Command::Info => system::info(&app, json),
        Command::Doctor => system::doctor(&app, json),
        Command::Update(args) => system::update(args, json),
        Command::Worker => execution::worker::work(&app),
    }
}

fn help_text() -> Option<&'static str> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    let requested = args.is_empty()
        || args[0] == "help"
        || args.iter().any(|value| value == "--help" || value == "-h");
    if !requested {
        return None;
    }
    let command = args.first().map(String::as_str).unwrap_or("");
    Some(match command {
        "config" => {
            "bfx config\nbfx config providers\nbfx config presets\nbfx config model set <name> --model <model> --url <url> --key <key>\nbfx config model remove <name>\nbfx config preset set <name> [--pages <pages>] [--language <language>] [--format <format>] [--destination <path>] [--watermark <true|false>]\nbfx config preset remove <name>\n"
        }
        "submit" => {
            "bfx submit <file...> --model <model>\nbfx submit <file...> --model <model> --preset <preset>\nbfx submit <file...> --model <model> --pages <pages>\nbfx submit <file...> --model <model> --language <language>\nbfx submit <file...> --model <model> --format <format>\nbfx submit <file...> --model <model> --watermark <true|false>\nbfx submit <file...> --model <model> --destination <path>\nbfx submit <file...> --model <model> --json\n"
        }
        "get" => {
            "bfx get\nbfx get --limit <limit>\nbfx get --asc\nbfx get --state <state...>\nbfx get --from <id> --to <id>\nbfx get --json\n"
        }
        "check" => {
            "bfx check <id>\nbfx check <id> --file\nbfx check <id> --model\nbfx check <id> --output\nbfx check <id> --state\nbfx check <id> --error\nbfx check <id> --json\n"
        }
        "run" => {
            "bfx run <file...> --model <model>\nbfx run <file...> --model <model> --preset <preset>\nbfx run <file...> --model <model> --pages <pages>\nbfx run <file...> --model <model> --language <language>\nbfx run <file...> --model <model> --format <format>\nbfx run <file...> --model <model> --watermark <true|false>\nbfx run <file...> --model <model> --destination <path>\nbfx run <file...> --model <model> --json\n"
        }
        "stop" => "bfx stop <id>\n",
        "retry" => "bfx retry <id>\n",
        "replace" => {
            "bfx replace <file> --keep\nbfx replace <file> --remove\nbfx replace <file> --undo\n"
        }
        "update" => "bfx update --check\nbfx update\n",
        _ => {
            "bfx info\nbfx doctor\nbfx update --check\nbfx update\nbfx config\nbfx submit <file...> --model <model>\nbfx get\nbfx check <id>\nbfx run <file...> --model <model>\nbfx stop <id>\nbfx retry <id>\nbfx replace <file> --keep\nbfx replace <file> --remove\nbfx replace <file> --undo\n"
        }
    })
}

fn input_message(error: &clap::Error) -> String {
    let text = error.to_string();
    let lines = text.lines().collect::<Vec<_>>();
    let Some(index) = lines.iter().position(|line| line.starts_with("error: ")) else {
        return "Invalid command arguments".to_owned();
    };
    let mut message = lines[index].trim_start_matches("error: ").to_owned();
    if message.ends_with(':')
        && let Some(value) = lines[index + 1..]
            .iter()
            .map(|line| line.trim())
            .find(|line| !line.is_empty() && !line.starts_with("Usage:"))
    {
        message.push(' ');
        message.push_str(value);
    }
    message
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_input_error() {
        let error = match Cli::try_parse_from(["bfx", "stop"]) {
            Ok(_) => panic!("missing task ID was accepted"),
            Err(error) => error,
        };
        assert_eq!(
            input_message(&error),
            "the following required arguments were not provided: <ID>"
        );
    }
}

fn main() {
    if let Err(error) = start() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
