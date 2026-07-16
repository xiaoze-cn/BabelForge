use crate::config;
use crate::error::{BfxError, Result};
use crate::storage::TaskPlan;
use serde::Serialize;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub struct EngineInput {
    pub input: PathBuf,
    pub plan: TaskPlan,
    pub key: String,
}

pub struct EngineOutput {
    pub pair: Option<PathBuf>,
    pub mono: Option<PathBuf>,
}

pub fn version(root: &Path) -> Result<String> {
    let output = command(root)?
        .arg("--version")
        .output()
        .map_err(|error| BfxError::engine(format!("Cannot start BabelDOC ({error})")))?;
    if !output.status.success() {
        return Err(BfxError::engine("BabelDOC version check failed"));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

pub fn run<F>(root: &Path, input: &EngineInput, should_stop: F) -> Result<EngineOutput>
where
    F: Fn() -> Result<bool>,
{
    let (source, target) = language(&input.plan.language)?;
    let destination = destination(&input.input, &input.plan.destination)?;
    fs::create_dir_all(&destination).map_err(|error| {
        BfxError::engine(format!("Cannot create the output directory ({error})"))
    })?;
    let config = write_config(&input.plan, &input.key)?;
    let result = run_command(
        root,
        input,
        &source,
        &target,
        &destination,
        &config,
        &should_stop,
    );
    let _ = fs::remove_file(config);
    result
}

fn run_command<F>(
    root: &Path,
    input: &EngineInput,
    source: &str,
    target: &str,
    destination: &Path,
    config: &Path,
    should_stop: &F,
) -> Result<EngineOutput>
where
    F: Fn() -> Result<bool>,
{
    let started = SystemTime::now();
    let mut process = command(root)?;
    process
        .arg("--config")
        .arg(config)
        .arg("--files")
        .arg(&input.input)
        .arg("--lang-in")
        .arg(source)
        .arg("--lang-out")
        .arg(target)
        .arg("--output")
        .arg(destination)
        .arg("--openai")
        .arg("--watermark-output-mode")
        .arg(if input.plan.watermark {
            "watermarked"
        } else {
            "no_watermark"
        });
    if !input.plan.pages.eq_ignore_ascii_case("All") {
        process.arg("--pages").arg(&input.plan.pages);
    }
    match input.plan.format.to_ascii_lowercase().as_str() {
        "pair" => {
            process.arg("--no-mono");
        }
        "mono" => {
            process.arg("--no-dual");
        }
        "both" => {}
        _ => {
            return Err(BfxError::engine(
                "The selected preset has an invalid format",
            ));
        }
    }
    let (log, path) = log_file()?;
    let stderr = log
        .try_clone()
        .map_err(|error| BfxError::engine(format!("Cannot capture BabelDOC output ({error})")))?;
    let mut child = process
        .stdout(Stdio::from(log))
        .stderr(Stdio::from(stderr))
        .spawn()
        .map_err(|error| BfxError::engine(format!("Cannot run BabelDOC ({error})")))?;
    let result = wait_child(&mut child, &path, should_stop);
    let _ = fs::remove_file(&path);
    let (status, text) = result?;
    if !status.success() {
        let status = status
            .code()
            .map_or_else(|| "unknown".to_owned(), |code| code.to_string());
        let detail = diagnostic(&text);
        let message = match detail {
            Some(detail) => format!("BabelDOC failed with exit status {status} ({detail})"),
            _ => format!("BabelDOC failed with exit status {status}"),
        };
        return Err(BfxError::engine(message));
    }
    let output = paths(
        &input.input,
        destination,
        target,
        &input.plan.format,
        input.plan.watermark,
    );
    check(&output, started)?;
    Ok(output)
}

fn log_file() -> Result<(fs::File, PathBuf)> {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| BfxError::engine(format!("Cannot read the system time ({error})")))?
        .as_nanos();
    let path = std::env::temp_dir().join(format!("bfx-{stamp}.log"));
    let file = fs::File::create(&path)
        .map_err(|error| BfxError::engine(format!("Cannot capture BabelDOC output ({error})")))?;
    Ok((file, path))
}

fn wait_child<F>(
    child: &mut Child,
    log: &Path,
    should_stop: &F,
) -> Result<(std::process::ExitStatus, String)>
where
    F: Fn() -> Result<bool>,
{
    loop {
        if should_stop()? {
            stop_child(child)?;
            let _ = child.wait();
            return Err(BfxError::queue("Translation was stopped"));
        }
        if let Some(status) = child
            .try_wait()
            .map_err(|error| BfxError::engine(format!("Cannot monitor BabelDOC ({error})")))?
        {
            let text = fs::read_to_string(log).unwrap_or_default();
            return Ok((status, text));
        }
        thread::sleep(Duration::from_millis(250));
    }
}

#[cfg(windows)]
fn stop_child(child: &mut Child) -> Result<()> {
    let output = Command::new("taskkill")
        .args(["/PID", &child.id().to_string(), "/T", "/F"])
        .output()
        .map_err(|error| BfxError::queue(format!("Cannot stop BabelDOC ({error})")))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(BfxError::queue("Cannot stop BabelDOC"))
    }
}

#[cfg(not(windows))]
fn stop_child(child: &mut Child) -> Result<()> {
    child
        .kill()
        .map_err(|error| BfxError::queue(format!("Cannot stop BabelDOC ({error})")))
}

fn command(root: &Path) -> Result<Command> {
    if let Some(value) = std::env::var_os("BFX_BABELDOC") {
        return Ok(Command::new(value));
    }
    let runtime = babeldoc_path(root);
    if runtime.is_file() {
        return Ok(Command::new(runtime));
    }
    Err(BfxError::engine(format!(
        "Cannot locate BabelDOC in \"{}\"",
        root.display()
    )))
}

fn babeldoc_path(root: &Path) -> PathBuf {
    #[cfg(windows)]
    {
        root.join("runtime").join("Scripts").join("babeldoc.exe")
    }
    #[cfg(not(windows))]
    {
        root.join("runtime").join("bin").join("babeldoc")
    }
}

fn language(value: &str) -> Result<(String, String)> {
    let Some((source, target)) = value.split_once("->") else {
        return Err(BfxError::engine(
            "The selected preset has an invalid language",
        ));
    };
    if source.trim().is_empty() || target.trim().is_empty() {
        return Err(BfxError::engine(
            "The selected preset has an invalid language",
        ));
    }
    Ok((
        source.trim().to_ascii_lowercase(),
        target.trim().to_ascii_lowercase(),
    ))
}

fn destination(input: &Path, value: &str) -> Result<PathBuf> {
    if value.eq_ignore_ascii_case("Same") {
        return input
            .parent()
            .map(Path::to_path_buf)
            .ok_or_else(|| BfxError::engine("The input PDF has no parent directory"));
    }
    let path = PathBuf::from(value);
    if path.is_absolute() {
        Ok(path)
    } else {
        Err(BfxError::engine(
            "The selected preset has an invalid destination",
        ))
    }
}

fn paths(
    input: &Path,
    destination: &Path,
    target: &str,
    format: &str,
    watermark: bool,
) -> EngineOutput {
    let stem = input
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("translated");
    let watermark = if watermark {
        "watermarked"
    } else {
        "no_watermark"
    };
    let pair = destination.join(format!("{stem}.{watermark}.{target}.dual.pdf"));
    let mono = destination.join(format!("{stem}.{watermark}.{target}.mono.pdf"));
    EngineOutput {
        pair: matches!(format.to_ascii_lowercase().as_str(), "pair" | "both").then_some(pair),
        mono: matches!(format.to_ascii_lowercase().as_str(), "mono" | "both").then_some(mono),
    }
}

fn check(output: &EngineOutput, started: SystemTime) -> Result<()> {
    let cutoff = started
        .checked_sub(Duration::from_secs(2))
        .unwrap_or(UNIX_EPOCH);
    for path in [&output.pair, &output.mono].into_iter().flatten() {
        let metadata = fs::metadata(path).map_err(|error| {
            BfxError::engine(format!(
                "BabelDOC did not create \"{}\" ({error})",
                path.display()
            ))
        })?;
        if metadata.len() == 0 {
            return Err(BfxError::engine(format!(
                "BabelDOC created an empty output \"{}\"",
                path.display()
            )));
        }
        let changed = metadata.modified().map_err(|error| {
            BfxError::engine(format!("Cannot inspect BabelDOC output ({error})"))
        })?;
        if changed < cutoff {
            return Err(BfxError::engine(format!(
                "BabelDOC did not refresh \"{}\"",
                path.display()
            )));
        }
    }
    Ok(())
}

#[derive(Serialize)]
struct Config<'a> {
    babeldoc: Service<'a>,
}

#[derive(Serialize)]
struct Service<'a> {
    openai: bool,
    #[serde(rename = "openai-model")]
    model: &'a str,
    #[serde(rename = "openai-base-url")]
    url: &'a str,
    #[serde(rename = "openai-api-key")]
    key: &'a str,
}

fn write_config(plan: &TaskPlan, key: &str) -> Result<PathBuf> {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| BfxError::engine(format!("Cannot read the system time ({error})")))?
        .as_nanos();
    let url = config::openai_base_url(&plan.engine_url)?;
    let text = toml::to_string(&Config {
        babeldoc: Service {
            openai: true,
            model: &plan.engine_model,
            url: &url,
            key,
        },
    })
    .map_err(|error| BfxError::engine(format!("Cannot create the BabelDOC config ({error})")))?;
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    for attempt in 0..100 {
        let path =
            std::env::temp_dir().join(format!("bfx-{}-{stamp}-{attempt}.toml", std::process::id()));
        let mut file = match options.open(&path) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => {
                return Err(BfxError::engine(format!(
                    "Cannot create the BabelDOC config ({error})"
                )));
            }
        };
        if let Err(error) = file
            .write_all(text.as_bytes())
            .and_then(|()| file.sync_all())
        {
            let _ = fs::remove_file(&path);
            return Err(BfxError::engine(format!(
                "Cannot write the BabelDOC config ({error})"
            )));
        }
        return Ok(path);
    }
    Err(BfxError::engine("Cannot allocate a BabelDOC config file"))
}

fn diagnostic(raw: &str) -> Option<String> {
    raw.lines()
        .find(|line| {
            let lower = line.to_ascii_lowercase();
            !line.trim().is_empty()
                && !lower.contains("api-key")
                && !lower.contains("api_key")
                && !lower.contains("authorization")
        })
        .map(|line| line.trim().chars().take(300).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stops_child() {
        let mut command = if cfg!(windows) {
            let mut command = Command::new("cmd");
            command.args(["/C", "ping -n 30 127.0.0.1 > nul"]);
            command
        } else {
            let mut command = Command::new("sh");
            command.args(["-c", "sleep 30"]);
            command
        };
        let mut child = command.spawn().unwrap();
        let error = wait_child(&mut child, Path::new("missing.log"), &|| Ok(true)).unwrap_err();
        assert_eq!(error.code, "BFX-QUE");
    }

    #[test]
    fn uses_babeldoc_watermark_output_names() {
        let input = Path::new("paper.pdf");
        let destination = Path::new("output");
        let plain = paths(input, destination, "ZH", "Pair", false);
        let marked = paths(input, destination, "ZH", "Mono", true);
        assert_eq!(
            plain.pair,
            Some(destination.join("paper.no_watermark.ZH.dual.pdf"))
        );
        assert_eq!(
            marked.mono,
            Some(destination.join("paper.watermarked.ZH.mono.pdf"))
        );
    }

    #[test]
    fn normalizes_language_codes() {
        assert_eq!(
            language("EN -> ZH").unwrap(),
            ("en".to_owned(), "zh".to_owned())
        );
    }
}
