use std::fmt::{Display, Formatter};

pub type Result<T> = std::result::Result<T, BfxError>;

pub fn format_error(code: &str, message: &str) -> String {
    format!("Error [{code}]: {message}")
}

#[derive(Debug)]
pub struct BfxError {
    pub code: String,
    pub message: String,
}

impl BfxError {
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
        }
    }

    pub fn config(message: impl Into<String>) -> Self {
        Self::new("BFX-CFG", message)
    }

    pub fn input(message: impl Into<String>) -> Self {
        Self::new("BFX-INP", message)
    }

    pub fn queue(message: impl Into<String>) -> Self {
        Self::new("BFX-QUE", message)
    }

    pub fn engine(message: impl Into<String>) -> Self {
        Self::new("BFX-ENG", message)
    }

    pub fn storage(message: impl Into<String>) -> Self {
        Self::new("BFX-DB", message)
    }

    pub fn replace(message: impl Into<String>) -> Self {
        Self::new("BFX-REP", message)
    }

    pub fn update(message: impl Into<String>) -> Self {
        Self::new("BFX-UPD", message)
    }
}

impl Display for BfxError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}", format_error(&self.code, &self.message))
    }
}

impl std::error::Error for BfxError {}
