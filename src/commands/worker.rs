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
    let executable = std::env::current_exe()
        .map_err(|error| BfxError::queue(format!("Cannot resolve the BFX executable ({error})")))?;
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
    let (_, provider) =
        config::find_provider(&config, &task.model).map_err(|error| (task.id.clone(), error))?;
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
