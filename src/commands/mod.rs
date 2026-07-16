mod check;
mod config;
mod get;
mod output;
mod replace;
mod retry;
mod stop;
mod submit;
mod system;
pub mod worker;

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
    Config(config::ConfigArgs),
    Submit(submit::SubmitArgs),
    Get(get::GetArgs),
    Check(check::CheckArgs),
    Run(submit::SubmitArgs),
    Stop(stop::StopArgs),
    Retry(retry::RetryArgs),
    Replace(replace::ReplaceArgs),
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
        Command::Config(args) => config::command(&app, args, json),
        Command::Submit(args) => submit::command(&app, args, false, json),
        Command::Get(args) => get::command(&app, args, json),
        Command::Check(args) => check::command(&app, args, json),
        Command::Run(args) => submit::command(&app, args, true, json),
        Command::Stop(args) => stop::command(&app, args, json),
        Command::Retry(args) => retry::command(&app, args, json),
        Command::Replace(args) => replace::command(&app, args, json),
        Command::Info => system::info(&app, json),
        Command::Doctor => system::doctor(&app, json),
        Command::Update(args) => system::update(args),
        Command::Worker => worker::work(&app),
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
