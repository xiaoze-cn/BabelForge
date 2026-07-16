use crate::app::App;
use crate::commands::{output, worker::hash};
use crate::error::{BfxError, Result};
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
        fs::rename(source, &backup)
            .map_err(|error| BfxError::replace(format!("Cannot create the backup ({error})")))?;
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
    fs::rename(source, &temporary)
        .map_err(|error| BfxError::replace(format!("Cannot prepare the source PDF ({error})")))?;
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
    fs::rename(source, &temporary)
        .map_err(|error| BfxError::replace(format!("Cannot prepare the current PDF ({error})")))?;
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
