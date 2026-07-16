use crate::error::{BfxError, Result};
use serde_json::Value;

pub fn json(value: Value) -> Result<()> {
    let text = serde_json::to_string(&value)
        .map_err(|error| BfxError::storage(format!("Cannot format JSON output ({error})")))?;
    println!("{text}");
    Ok(())
}

pub fn time(duration_ms: Option<i64>) -> String {
    let seconds = duration_ms.unwrap_or(0).max(0) as u64 / 1_000;
    let hours = seconds / 3_600;
    let minutes = seconds % 3_600 / 60;
    let seconds = seconds % 60;
    format!("{hours:02}:{minutes:02}:{seconds:02}")
}

pub fn value(value: &str) -> String {
    let value = value.replace(['\r', '\n'], " ");
    if value.chars().any(char::is_whitespace) {
        format!("\"{}\"", value.replace('"', "\\\""))
    } else {
        value
    }
}

pub fn path(path: &str) -> String {
    value(&path_value(path))
}

pub fn path_value(value: &str) -> String {
    #[cfg(windows)]
    {
        if let Some(value) = value.strip_prefix("\\\\?\\UNC\\") {
            return format!("\\\\{value}");
        }
        if let Some(value) = value.strip_prefix("\\\\?\\") {
            return value.to_owned();
        }
    }
    value.to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(windows)]
    #[test]
    fn removes_windows_verbatim_prefix() {
        assert_eq!(path_value("\\\\?\\C:\\Users\\BFX"), "C:\\Users\\BFX");
        assert_eq!(
            path_value("\\\\?\\UNC\\server\\share\\BFX"),
            "\\\\server\\share\\BFX"
        );
    }
}
