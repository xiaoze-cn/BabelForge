use crate::app::App;
use crate::commands::{output, worker};
use crate::error::Result;
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
