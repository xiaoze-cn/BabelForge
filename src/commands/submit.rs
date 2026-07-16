use crate::app::App;
use crate::commands::{output, worker};
use crate::config::{self, Preset};
use crate::error::{BfxError, Result};
use crate::storage::{NewTask, TaskPlan};
use clap::Args;
use serde_json::json;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

#[derive(Args, Clone)]
pub struct SubmitArgs {
    #[arg(required = true, num_args = 1..)]
    files: Vec<PathBuf>,
    #[arg(long)]
    model: String,
    #[arg(long, default_value = "Default")]
    preset: String,
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

pub fn command(app: &App, args: SubmitArgs, priority: bool, json: bool) -> Result<()> {
    let files = files(&args.files)?;
    let config = config::read_config(&app.paths.config)?;
    let (model_name, provider) = config::find_provider(&config, &args.model)?;
    let (preset_name, preset) = config::find_preset(&config, &args.preset)?;
    let preset = merge_preset(preset, &args);
    config::check_preset(&preset_name, &preset)?;
    let mut tasks = Vec::with_capacity(files.len());
    for file in files {
        let plan = TaskPlan {
            pages: preset.pages.clone(),
            language: preset.language.clone(),
            format: preset.format.clone(),
            destination: target_path(&file, &preset.destination)?
                .display()
                .to_string(),
            watermark: preset.watermark,
            engine_model: provider.model.clone(),
            engine_url: config::openai_base_url(&provider.url)?,
        };
        tasks.push(NewTask {
            input: file,
            model: model_name.clone(),
            preset: preset_name.clone(),
            plan,
            priority,
        });
    }
    let ids = app.store.enqueue_many(&tasks)?;
    worker::spawn()?;
    if priority {
        let tasks = worker::wait(app, &ids)?;
        print_run(&tasks, json)?;
        run_result(&tasks)?;
    } else {
        print_submit(&ids, json)?;
    }
    Ok(())
}

fn print_submit(ids: &[String], json: bool) -> Result<()> {
    if json {
        return output::json(json!({
            "tasks": ids.iter().map(|id| json!({ "id": id, "state": "QUE" })).collect::<Vec<_>>()
        }));
    }
    for id in ids {
        println!("{id} QUE");
    }
    Ok(())
}

fn print_run(tasks: &[crate::storage::Task], json: bool) -> Result<()> {
    if json {
        let tasks = tasks.iter().map(run_task).collect::<Result<Vec<_>>>()?;
        return output::json(json!({ "tasks": tasks }));
    }
    for task in tasks {
        let destination = output::path(&task_plan(task)?.destination);
        println!(
            "{} {destination} {} {}",
            task.id,
            task.state,
            output::time(task.duration_ms)
        );
    }
    Ok(())
}

fn run_task(task: &crate::storage::Task) -> Result<serde_json::Value> {
    Ok(json!({
        "id": task.id,
        "output": output::path_value(&task_plan(task)?.destination),
        "state": task.state,
        "time_ms": task.duration_ms,
        "time": output::time(task.duration_ms),
    }))
}

fn task_plan(task: &crate::storage::Task) -> Result<TaskPlan> {
    toml::from_str(&task.plan_text)
        .map_err(|error| BfxError::storage(format!("Cannot read the translation plan ({error})")))
}

fn run_result(tasks: &[crate::storage::Task]) -> Result<()> {
    for task in tasks {
        if task.state == "ERR" {
            let code = task.error_code.as_deref().unwrap_or("BFX-ENG");
            let detail = task.error_detail.as_deref().unwrap_or("Translation failed");
            return Err(BfxError::new(code, detail));
        }
        if task.state == "STP" {
            return Err(BfxError::queue(format!("Task \"{}\" was stopped", task.id)));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn returns_task_error() {
        let task = crate::storage::Task {
            id: "20260715-153042".to_owned(),
            input: "paper.pdf".to_owned(),
            model: "Model".to_owned(),
            state: "ERR".to_owned(),
            plan_text: String::new(),
            input_hash: None,
            error_code: Some("BFX-ENG".to_owned()),
            error_detail: Some("BabelDOC failed".to_owned()),
            pair: None,
            pair_hash: None,
            mono: None,
            duration_ms: None,
        };
        let error = run_result(&[task]).unwrap_err();
        assert_eq!(error.code, "BFX-ENG");
        assert_eq!(error.message, "BabelDOC failed");
    }
}

fn files(inputs: &[PathBuf]) -> Result<Vec<PathBuf>> {
    let mut result = BTreeSet::new();
    for input in inputs {
        if input.is_dir() {
            let entries = std::fs::read_dir(input).map_err(|error| {
                BfxError::input(format!("Cannot read \"{}\" ({error})", input.display()))
            })?;
            for entry in entries {
                let path = entry
                    .map_err(|error| {
                        BfxError::input(format!("Cannot read an input entry ({error})"))
                    })?
                    .path();
                if path.is_file() && source_pdf(&path) {
                    result.insert(resolve_file(&path)?);
                }
            }
        } else {
            result.insert(resolve_file(input)?);
        }
    }
    if result.is_empty() {
        return Err(BfxError::input("No source PDFs were found"));
    }
    Ok(result.into_iter().collect())
}

fn resolve_file(path: &Path) -> Result<PathBuf> {
    if !path.is_file() {
        return Err(BfxError::input(format!(
            "Input \"{}\" does not exist",
            path.display()
        )));
    }
    if !source_pdf(path) {
        return Err(BfxError::input(format!(
            "Input \"{}\" is not a source PDF",
            path.display()
        )));
    }
    path.canonicalize().map_err(|error| {
        BfxError::input(format!("Cannot resolve \"{}\" ({error})", path.display()))
    })
}

fn source_pdf(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|value| value.to_str()) else {
        return false;
    };
    let lower = name.to_ascii_lowercase();
    lower.ends_with(".pdf")
        && !lower.ends_with(".dual.pdf")
        && !lower.ends_with(".mono.pdf")
        && !lower.ends_with(".bfx-original.pdf")
}

fn merge_preset(mut preset: Preset, args: &SubmitArgs) -> Preset {
    if let Some(value) = &args.pages {
        preset.pages = value.clone();
    }
    if let Some(value) = &args.language {
        preset.language = value.clone();
    }
    if let Some(value) = &args.format {
        preset.format = value.clone();
    }
    if let Some(value) = &args.destination {
        preset.destination = value.clone();
    }
    if let Some(value) = args.watermark {
        preset.watermark = value;
    }
    preset
}

fn target_path(input: &Path, value: &str) -> Result<PathBuf> {
    if value.eq_ignore_ascii_case("Same") {
        return input
            .parent()
            .map(Path::to_path_buf)
            .ok_or_else(|| BfxError::input("The input PDF has no parent directory"));
    }
    let path = PathBuf::from(value);
    if path.is_absolute() {
        Ok(path)
    } else {
        Err(BfxError::input(
            "Destination must be Same or an absolute path",
        ))
    }
}
