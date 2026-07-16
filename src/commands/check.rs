use crate::app::App;
use crate::commands::output;
use crate::error::{BfxError, Result, format_error};
use crate::storage::{Task, TaskPlan};
use clap::Args;
use serde_json::{Map, Value, json};
use std::path::Path;

#[derive(Args)]
pub struct CheckArgs {
    id: String,
    #[arg(long)]
    file: bool,
    #[arg(long)]
    model: bool,
    #[arg(long)]
    output: bool,
    #[arg(long)]
    state: bool,
    #[arg(long)]
    error: bool,
}

pub fn command(app: &App, args: CheckArgs, json: bool) -> Result<()> {
    let task = app
        .store
        .get(&args.id)?
        .ok_or_else(|| BfxError::input(format!("Task \"{}\" is not available", args.id)))?;
    print_task(&task, &args, json)
}

fn print_task(task: &Task, args: &CheckArgs, json: bool) -> Result<()> {
    if args.error {
        if args.file || args.model || args.output || args.state {
            return Err(BfxError::input(
                "--error cannot be combined with summary fields",
            ));
        }
        if json {
            let code = task.error_code.as_deref().ok_or_else(|| {
                BfxError::input(format!("Task \"{}\" has no recorded error", task.id))
            })?;
            let detail = task.error_detail.as_deref().unwrap_or("Unknown error");
            return output::json(json!({
                "id": task.id,
                "state": task.state,
                "error": { "code": code, "detail": detail },
            }));
        }
        println!("{}", error_text(task)?);
        return Ok(());
    }
    let all = !args.file && !args.model && !args.output && !args.state && !args.error;
    if json {
        return json_task(task, args, all);
    }
    let mut values = Vec::new();
    if all || args.file {
        values.push(output::value(file_name(&task.input)));
    }
    if all || args.model {
        values.push(output::value(&task.model));
    }
    if all || args.output {
        values.push(output::path(&plan(task)?.destination));
    }
    if all || args.state {
        values.push(output::value(&task.state));
    }
    println!("{}", values.join(" "));
    Ok(())
}

fn json_task(task: &Task, args: &CheckArgs, all: bool) -> Result<()> {
    let mut values = Map::new();
    values.insert("id".to_owned(), Value::String(task.id.clone()));
    if all || args.file {
        values.insert(
            "file".to_owned(),
            Value::String(file_name(&task.input).to_owned()),
        );
    }
    if all || args.model {
        values.insert("model".to_owned(), Value::String(task.model.clone()));
    }
    if all || args.output {
        values.insert(
            "output".to_owned(),
            Value::String(output::path_value(&plan(task)?.destination)),
        );
    }
    if all || args.state {
        values.insert("state".to_owned(), Value::String(task.state.clone()));
    }
    if all {
        values.insert("time_ms".to_owned(), Value::from(task.duration_ms));
        values.insert(
            "time".to_owned(),
            Value::String(output::time(task.duration_ms)),
        );
        values.insert("pair".to_owned(), Value::from(task.pair.clone()));
        values.insert("mono".to_owned(), Value::from(task.mono.clone()));
    }
    output::json(Value::Object(values))
}

fn plan(task: &Task) -> Result<TaskPlan> {
    toml::from_str(&task.plan_text)
        .map_err(|error| BfxError::storage(format!("Cannot read the translation plan ({error})")))
}

fn file_name(value: &str) -> &str {
    Path::new(value)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(value)
}

fn error_text(task: &Task) -> Result<String> {
    let code = task
        .error_code
        .as_deref()
        .ok_or_else(|| BfxError::input(format!("Task \"{}\" has no recorded error", task.id)))?;
    let detail = task.error_detail.as_deref().unwrap_or("Unknown error");
    Ok(format_error(code, detail))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn errors_when_missing() {
        let task = Task {
            id: "20260715-153042".to_owned(),
            input: "paper.pdf".to_owned(),
            model: "Model".to_owned(),
            state: "FIN".to_owned(),
            plan_text: String::new(),
            input_hash: None,
            error_code: None,
            error_detail: None,
            pair: None,
            pair_hash: None,
            mono: None,
            duration_ms: None,
        };
        let error = error_text(&task).unwrap_err();
        assert_eq!(error.code, "BFX-INP");
    }
}
