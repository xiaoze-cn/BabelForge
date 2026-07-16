use crate::app::App;
use crate::commands::output;
use crate::error::{BfxError, Result};
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
