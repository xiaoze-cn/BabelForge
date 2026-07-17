pub(crate) mod worker {
    use crate::app::App;
    use crate::config;
    use crate::engine::{self, EngineInput};
    use crate::error::{BfxError, Result};
    use crate::storage::Task;
    use fs4::fs_std::FileExt;
    use sha2::{Digest, Sha256};
    use std::fs::{File, OpenOptions};
    use std::io::Read;
    use std::process::{Command, Stdio};
    use std::time::Instant;

    pub fn spawn() -> Result<()> {
        let executable = std::env::current_exe().map_err(|error| {
            BfxError::queue(format!("Cannot resolve the BFX executable ({error})"))
        })?;
        let mut command = Command::new(executable);
        command
            .arg("worker")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x0800_0000);
        }
        command.spawn().map_err(|error| {
            BfxError::queue(format!("Cannot start the background worker ({error})"))
        })?;
        Ok(())
    }

    pub fn work(app: &App) -> Result<()> {
        let Some(_lock) = lock(app)? else {
            return Ok(());
        };
        app.store.recover()?;
        loop {
            if let Some(task) = app.store.claim()? {
                let started = Instant::now();
                if let Err(error) = process(app, task, &started) {
                    app.store
                        .fail(&error.0, &error.1.code, &error.1.message, elapsed(&started))?;
                }
                continue;
            }
            std::thread::sleep(std::time::Duration::from_millis(350));
        }
    }

    pub fn wait(app: &App, ids: &[String]) -> Result<Vec<Task>> {
        loop {
            let mut tasks = Vec::with_capacity(ids.len());
            let mut finished = true;
            for id in ids {
                let task = app
                    .store
                    .get(id)?
                    .ok_or_else(|| BfxError::queue(format!("Task \"{id}\" is missing")))?;
                if matches!(task.state.as_str(), "QUE" | "RUN") {
                    finished = false;
                }
                tasks.push(task);
            }
            if finished {
                return Ok(tasks);
            }
            std::thread::sleep(std::time::Duration::from_millis(350));
        }
    }

    fn lock(app: &App) -> Result<Option<File>> {
        let path = app.paths.data_dir.join("worker.lock");
        let file = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .truncate(false)
            .open(path)
            .map_err(|error| BfxError::queue(format!("Cannot open the worker lock ({error})")))?;
        match file.try_lock_exclusive() {
            Ok(()) => Ok(Some(file)),
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => Ok(None),
            Err(error) => Err(BfxError::queue(format!("Cannot lock the worker ({error})"))),
        }
    }

    fn process(
        app: &App,
        task: crate::storage::WorkItem,
        started: &Instant,
    ) -> std::result::Result<(), (String, BfxError)> {
        if app
            .store
            .stopped(&task.id)
            .map_err(|error| (task.id.clone(), error))?
        {
            app.store
                .finish_stop(&task.id, elapsed(started))
                .map_err(|error| (task.id.clone(), error))?;
            return Ok(());
        }
        let input_hash = hash(&task.input).map_err(|error| (task.id.clone(), error))?;
        let config =
            config::read_config(&app.paths.config).map_err(|error| (task.id.clone(), error))?;
        let (_, provider) = config::find_provider(&config, &task.model)
            .map_err(|error| (task.id.clone(), error))?;
        let key = config::read_key(&provider.key).map_err(|error| (task.id.clone(), error))?;
        let result = engine::run(
            &app.root,
            &EngineInput {
                input: task.input,
                plan: task.plan,
                key,
            },
            || app.store.stopped(&task.id),
        )
        .map_err(|error| (task.id.clone(), error))?;
        let pair_hash = result
            .pair
            .as_deref()
            .map(hash)
            .transpose()
            .map_err(|error| (task.id.clone(), error))?;
        let mono_hash = result
            .mono
            .as_deref()
            .map(hash)
            .transpose()
            .map_err(|error| (task.id.clone(), error))?;
        let duration = elapsed(started);
        app.store
            .finish(
                &task.id,
                &input_hash,
                result.pair.as_deref().zip(pair_hash.as_deref()),
                result.mono.as_deref().zip(mono_hash.as_deref()),
                duration,
            )
            .map_err(|error| (task.id.clone(), error))?;
        if app
            .store
            .stopped(&task.id)
            .map_err(|error| (task.id.clone(), error))?
        {
            app.store
                .finish_stop(&task.id, elapsed(started))
                .map_err(|error| (task.id.clone(), error))?;
        }
        Ok(())
    }

    fn elapsed(started: &Instant) -> i64 {
        i64::try_from(started.elapsed().as_millis()).unwrap_or(i64::MAX)
    }

    pub fn hash(path: &std::path::Path) -> Result<String> {
        let mut file = File::open(path).map_err(|error| {
            BfxError::input(format!("Cannot read \"{}\" ({error})", path.display()))
        })?;
        let mut digest = Sha256::new();
        let mut buffer = [0_u8; 64 * 1024];
        loop {
            let count = file.read(&mut buffer).map_err(|error| {
                BfxError::input(format!("Cannot read \"{}\" ({error})", path.display()))
            })?;
            if count == 0 {
                break;
            }
            digest.update(&buffer[..count]);
        }
        Ok(format!("{:x}", digest.finalize()))
    }
}

pub(crate) mod submit {
    use super::worker;
    use crate::app::App;
    use crate::config::{self, Preset};
    use crate::error::{BfxError, Result};
    use crate::output;
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
        toml::from_str(&task.plan_text).map_err(|error| {
            BfxError::storage(format!("Cannot read the translation plan ({error})"))
        })
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
}

pub(crate) mod get {
    use super::worker;
    use crate::app::App;
    use crate::error::{BfxError, Result};
    use crate::output;
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
        let valid_date = date.is_some_and(|value| {
            value.len() == 8 && value.bytes().all(|byte| byte.is_ascii_digit())
        });
        let valid_time = time.is_some_and(|value| {
            value.len() == 6 && value.bytes().all(|byte| byte.is_ascii_digit())
        });
        let valid_suffix = suffix.is_none_or(|value| {
            value.len() == 2 && value.bytes().all(|byte| byte.is_ascii_digit())
        });
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
}

pub(crate) mod check {
    use crate::app::App;
    use crate::error::{BfxError, Result, format_error};
    use crate::output;
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
        toml::from_str(&task.plan_text).map_err(|error| {
            BfxError::storage(format!("Cannot read the translation plan ({error})"))
        })
    }

    fn file_name(value: &str) -> &str {
        Path::new(value)
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or(value)
    }

    fn error_text(task: &Task) -> Result<String> {
        let code = task.error_code.as_deref().ok_or_else(|| {
            BfxError::input(format!("Task \"{}\" has no recorded error", task.id))
        })?;
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
}

pub(crate) mod stop {
    use crate::app::App;
    use crate::error::{BfxError, Result};
    use crate::output;
    use clap::Args;
    use serde_json::json;

    #[derive(Args)]
    pub struct StopArgs {
        id: String,
    }

    pub fn command(app: &App, args: StopArgs, json: bool) -> Result<()> {
        app.store.stop(&args.id)?;
        let task = app
            .store
            .get(&args.id)?
            .ok_or_else(|| BfxError::storage(format!("Stopped task \"{}\" is missing", args.id)))?;
        if json {
            output::json(json!({ "id": task.id, "state": task.state }))?;
        } else {
            println!("{} {}", task.id, task.state);
        }
        Ok(())
    }
}

pub(crate) mod retry {
    use super::worker;
    use crate::app::App;
    use crate::error::Result;
    use crate::output;
    use clap::Args;
    use serde_json::json;

    #[derive(Args)]
    pub struct RetryArgs {
        id: String,
    }

    pub fn command(app: &App, args: RetryArgs, json: bool) -> Result<()> {
        app.store.retry(&args.id)?;
        worker::spawn()?;
        if json {
            output::json(json!({ "id": args.id, "state": "QUE" }))?;
        } else {
            println!("{} QUE", args.id);
        }
        Ok(())
    }
}

pub(crate) mod replace {
    use super::worker::hash;
    use crate::app::App;
    use crate::error::{BfxError, Result};
    use crate::output;
    use clap::Args;
    use serde_json::json;
    use std::fs;
    use std::path::{Path, PathBuf};

    pub enum ReplaceMode {
        Keep,
        Remove,
        Undo,
    }

    #[derive(Args)]
    pub struct ReplaceArgs {
        file: PathBuf,
        #[arg(long, group = "mode")]
        keep: bool,
        #[arg(long, group = "mode")]
        remove: bool,
        #[arg(long, group = "mode")]
        undo: bool,
    }

    pub fn command(app: &App, args: ReplaceArgs, json: bool) -> Result<()> {
        let mode = match (args.keep, args.remove, args.undo) {
            (true, false, false) => ReplaceMode::Keep,
            (false, true, false) => ReplaceMode::Remove,
            (false, false, true) => ReplaceMode::Undo,
            _ => {
                return Err(BfxError::input(
                    "Specify one of --keep, --remove, or --undo",
                ));
            }
        };
        run(app, &args.file, mode, json)
    }

    fn run(app: &App, source: &Path, mode: ReplaceMode, json: bool) -> Result<()> {
        let source = source.canonicalize().map_err(|error| {
            BfxError::replace(format!("Cannot resolve \"{}\" ({error})", source.display()))
        })?;
        match mode {
            ReplaceMode::Undo => undo(app, &source, json),
            ReplaceMode::Keep | ReplaceMode::Remove => replace(app, &source, mode, json),
        }
    }

    fn replace(app: &App, source: &Path, mode: ReplaceMode, json: bool) -> Result<()> {
        let task = app.store.find_pair(source)?.ok_or_else(|| {
            BfxError::replace(format!(
                "No Pair translation is available for \"{}\"",
                source.display()
            ))
        })?;
        let pair = PathBuf::from(
            task.pair
                .ok_or_else(|| BfxError::replace("The recorded Pair translation is missing"))?,
        );
        if !pair.is_file() {
            return Err(BfxError::replace(format!(
                "Pair translation \"{}\" is missing",
                pair.display()
            )));
        }
        let source_hash = hash(source)?;
        let expected_source_hash = task
            .input_hash
            .as_deref()
            .ok_or_else(|| BfxError::replace("The recorded source PDF has no hash"))?;
        if source_hash != expected_source_hash {
            return Err(BfxError::replace(format!(
                "Replacement was cancelled because \"{}\" changed after translation",
                source.display()
            )));
        }
        let pair_hash = hash(&pair)?;
        let expected_pair_hash = task
            .pair_hash
            .as_deref()
            .ok_or_else(|| BfxError::replace("The recorded Pair translation has no hash"))?;
        if pair_hash != expected_pair_hash {
            return Err(BfxError::replace("The recorded Pair translation changed"));
        }
        if matches!(mode, ReplaceMode::Keep) {
            let backup = backup_path(source)?;
            fs::rename(source, &backup).map_err(|error| {
                BfxError::replace(format!("Cannot create the backup ({error})"))
            })?;
            if let Err(error) = fs::copy(&pair, source) {
                restore_backup(source, &backup);
                return Err(BfxError::replace(format!(
                    "Cannot replace the source PDF ({error})"
                )));
            }
            if let Err(error) =
                app.store
                    .save_replace(source, &backup, &task.id, &source_hash, &pair_hash)
            {
                restore_backup(source, &backup);
                return Err(BfxError::replace(format!(
                    "Cannot record the replacement ({})",
                    error.message
                )));
            }
            if json {
                return output::json(json!({
                    "action": "replaced",
                    "file": source,
                    "backup": backup,
                }));
            }
            println!(
                "Done [BFX-REP]: Replaced \"{}\" and saved \"{}\"",
                source.display(),
                backup.display()
            );
            return Ok(());
        }
        let temporary = temporary_path(source, "replace")?;
        fs::rename(source, &temporary).map_err(|error| {
            BfxError::replace(format!("Cannot prepare the source PDF ({error})"))
        })?;
        if let Err(error) = fs::copy(&pair, source) {
            restore_backup(source, &temporary);
            return Err(BfxError::replace(format!(
                "Cannot replace the source PDF ({error})"
            )));
        }
        if let Err(error) = fs::remove_file(&temporary) {
            restore_backup(source, &temporary);
            return Err(BfxError::replace(format!(
                "Cannot remove the source backup ({error})"
            )));
        }
        if json {
            return output::json(json!({ "action": "replaced", "file": source }));
        }
        println!("Done [BFX-REP]: Replaced \"{}\"", source.display());
        Ok(())
    }

    fn undo(app: &App, source: &Path, json: bool) -> Result<()> {
        let replacement = app.store.replacement(source)?.ok_or_else(|| {
            BfxError::replace(format!(
                "No reversible replacement is available for \"{}\"",
                source.display()
            ))
        })?;
        if !replacement.backup.is_file() {
            return Err(BfxError::replace(format!(
                "Backup \"{}\" is missing",
                replacement.backup.display()
            )));
        }
        let current_hash = hash(source)?;
        if current_hash != replacement.pair_hash {
            return Err(BfxError::replace(format!(
                "\"{}\" changed after replacement",
                source.display()
            )));
        }
        let backup_hash = hash(&replacement.backup)?;
        if backup_hash != replacement.source_hash {
            return Err(BfxError::replace(format!(
                "Backup \"{}\" changed",
                replacement.backup.display()
            )));
        }
        let temporary = temporary_path(source, "undo")?;
        fs::rename(source, &temporary).map_err(|error| {
            BfxError::replace(format!("Cannot prepare the current PDF ({error})"))
        })?;
        if let Err(error) = fs::rename(&replacement.backup, source) {
            let _ = fs::rename(&temporary, source);
            return Err(BfxError::replace(format!(
                "Cannot restore the backup ({error})"
            )));
        }
        if let Err(error) = app.store.undo_replace(replacement.id) {
            let _ = fs::rename(source, &replacement.backup);
            let _ = fs::rename(&temporary, source);
            return Err(BfxError::replace(format!(
                "Cannot record the restored backup ({})",
                error.message
            )));
        }
        let _ = fs::remove_file(temporary);
        if json {
            return output::json(json!({ "action": "restored", "file": source }));
        }
        println!("Done [BFX-REP]: Restored \"{}\"", source.display());
        Ok(())
    }

    fn backup_path(source: &Path) -> Result<PathBuf> {
        let stem = source
            .file_stem()
            .and_then(|value| value.to_str())
            .ok_or_else(|| BfxError::replace("The source PDF has an invalid file name"))?;
        let backup = source.with_file_name(format!("{stem}.bfx-original.pdf"));
        if backup.exists() {
            return Err(BfxError::replace(format!(
                "Backup \"{}\" already exists",
                backup.display()
            )));
        }
        Ok(backup)
    }

    fn temporary_path(source: &Path, label: &str) -> Result<PathBuf> {
        let stem = source
            .file_stem()
            .and_then(|value| value.to_str())
            .ok_or_else(|| BfxError::replace("The source PDF has an invalid file name"))?;
        let path = source.with_file_name(format!("{stem}.bfx-{label}.tmp"));
        if path.exists() {
            return Err(BfxError::replace(format!(
                "Temporary file \"{}\" already exists",
                path.display()
            )));
        }
        Ok(path)
    }

    fn restore_backup(source: &Path, backup: &Path) {
        let _ = fs::remove_file(source);
        let _ = fs::rename(backup, source);
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use crate::app::Paths;
        use crate::storage::{NewTask, Store, TaskPlan};
        use std::time::{SystemTime, UNIX_EPOCH};

        #[test]
        fn rejects_changed_source() {
            let stamp = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let root = std::env::temp_dir().join(format!("bfx-replace-{stamp}"));
            let config = root.join("config");
            let data = root.join("data");
            fs::create_dir_all(&config).unwrap();
            fs::create_dir_all(&data).unwrap();
            let source = root.join("paper.pdf");
            let pair = root.join("paper.ZH.dual.pdf");
            fs::write(&source, b"source").unwrap();
            fs::write(&pair, b"pair").unwrap();
            let store = Store::open(data.join("bfx.sqlite3")).unwrap();
            let plan = TaskPlan {
                pages: "All".to_owned(),
                language: "EN->ZH".to_owned(),
                format: "Pair".to_owned(),
                destination: root.display().to_string(),
                watermark: false,
                engine_model: "model".to_owned(),
                engine_url: "https://example.test/v1".to_owned(),
            };
            let ids = store
                .enqueue_many(&[NewTask {
                    input: source.clone(),
                    model: "Model".to_owned(),
                    preset: "Default".to_owned(),
                    plan,
                    priority: false,
                }])
                .unwrap();
            let item = store.claim().unwrap().unwrap();
            let source_hash = hash(&source).unwrap();
            let pair_hash = hash(&pair).unwrap();
            store
                .finish(&item.id, &source_hash, Some((&pair, &pair_hash)), None, 0)
                .unwrap();
            let app = App {
                root: root.clone(),
                paths: Paths {
                    config_dir: config.clone(),
                    data_dir: data,
                    config: config.join("config.toml"),
                },
                store,
            };
            fs::write(&source, b"changed").unwrap();
            let error = replace(&app, &source, ReplaceMode::Keep, false).unwrap_err();
            assert_eq!(error.code, "BFX-REP");
            assert_eq!(fs::read(&source).unwrap(), b"changed");
            assert_eq!(ids.len(), 1);
            let _ = fs::remove_dir_all(root);
        }
    }
}
