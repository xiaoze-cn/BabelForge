use crate::app::App;
use crate::commands::{output, worker};
use crate::error::{BfxError, Result};
use crate::storage::Task;
use clap::Args;
use serde_json::json;

#[derive(Args)]
pub struct GetArgs {
    #[arg(long = "state", num_args = 1.., value_name = "STATE")]
    states: Vec<String>,
    #[arg(long, value_name = "ID")]
    from: Option<String>,
    #[arg(long, value_name = "ID")]
    to: Option<String>,
    #[arg(long, default_value_t = 20)]
    limit: u32,
    #[arg(long)]
    asc: bool,
}

pub fn command(app: &App, args: GetArgs, json: bool) -> Result<()> {
    worker::spawn()?;
    let states = states(&args.states)?;
    task_id(args.from.as_deref())?;
    task_id(args.to.as_deref())?;
    let tasks = app.store.list(
        &states,
        args.from.as_deref(),
        args.to.as_deref(),
        args.limit,
        args.asc,
    )?;
    print_tasks(&tasks, json)?;
    Ok(())
}

fn states(values: &[String]) -> Result<Vec<String>> {
    values
        .iter()
        .map(|value| {
            let state = value.to_ascii_uppercase();
            match state.as_str() {
                "QUE" | "RUN" | "FIN" | "ERR" | "STP" => Ok(state),
                _ => Err(BfxError::input(format!(
                    "State \"{value}\" must be QUE, RUN, FIN, ERR, or STP"
                ))),
            }
        })
        .collect()
}

fn task_id(value: Option<&str>) -> Result<()> {
    let Some(value) = value else {
        return Ok(());
    };
    let mut parts = value.split('-');
    let date = parts.next();
    let time = parts.next();
    let suffix = parts.next();
    let valid_date = date
        .is_some_and(|value| value.len() == 8 && value.bytes().all(|byte| byte.is_ascii_digit()));
    let valid_time = time
        .is_some_and(|value| value.len() == 6 && value.bytes().all(|byte| byte.is_ascii_digit()));
    let valid_suffix = suffix
        .is_none_or(|value| value.len() == 2 && value.bytes().all(|byte| byte.is_ascii_digit()));
    if valid_date && valid_time && valid_suffix && parts.next().is_none() {
        return Ok(());
    }
    Err(BfxError::input(format!("Task ID \"{value}\" is not valid")))
}

fn print_tasks(tasks: &[Task], json: bool) -> Result<()> {
    if json {
        return output::json(json!({
            "tasks": tasks.iter().map(|task| json!({ "id": task.id, "state": task.state })).collect::<Vec<_>>()
        }));
    }
    if tasks.is_empty() {
        println!("None");
        return Ok(());
    }
    for task in tasks {
        println!("{} {}", task.id, task.state);
    }
    Ok(())
}
