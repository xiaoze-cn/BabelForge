use crate::error::{BfxError, Result};
use crate::storage::Store;
use directories::BaseDirs;
use std::path::PathBuf;

pub struct Paths {
    pub config_dir: PathBuf,
    pub data_dir: PathBuf,
    pub config: PathBuf,
}

pub struct App {
    pub root: PathBuf,
    pub paths: Paths,
    pub store: Store,
}

impl App {
    pub fn open() -> Result<Self> {
        let root = runtime_root()?;
        let base = match std::env::var_os("BFX_HOME") {
            Some(value) => PathBuf::from(value),
            None => BaseDirs::new()
                .map(|dirs| dirs.data_local_dir().join("BabelForge").join("eXecutor"))
                .ok_or_else(|| BfxError::storage("Cannot resolve the BFX data directory"))?,
        };
        let config_dir = base.clone();
        let data_dir = base.join("data");
        std::fs::create_dir_all(&config_dir).map_err(|error| {
            BfxError::storage(format!("Cannot create the BFX config directory ({error})"))
        })?;
        std::fs::create_dir_all(&data_dir).map_err(|error| {
            BfxError::storage(format!("Cannot create the BFX data directory ({error})"))
        })?;
        let config = std::env::var_os("BFX_CONFIG_PATH")
            .map(PathBuf::from)
            .unwrap_or_else(|| config_dir.join("config.toml"));
        let store = Store::open(data_dir.join("bfx.sqlite3"))?;
        Ok(Self {
            root,
            paths: Paths {
                config_dir,
                data_dir,
                config,
            },
            store,
        })
    }
}

fn runtime_root() -> Result<PathBuf> {
    if let Some(value) = std::env::var_os("BFX_ROOT") {
        let root = PathBuf::from(value);
        return valid_root(root);
    }
    let executable = std::env::current_exe().map_err(|error| {
        BfxError::engine(format!("Cannot resolve the BFX executable ({error})"))
    })?;
    let root = executable
        .ancestors()
        .find(|path| has_runtime(path))
        .map(PathBuf::from)
        .ok_or_else(|| BfxError::engine("Cannot locate the BFX runtime"))?;
    valid_root(root)
}

fn valid_root(root: PathBuf) -> Result<PathBuf> {
    if has_runtime(&root) || has_override() {
        return Ok(root);
    }
    Err(BfxError::engine(format!(
        "BFX runtime \"{}\" is incomplete",
        root.display()
    )))
}

fn has_runtime(root: &std::path::Path) -> bool {
    #[cfg(windows)]
    let packaged = root.join("runtime").join("Scripts").join("babeldoc.exe");
    #[cfg(not(windows))]
    let packaged = root.join("runtime").join("bin").join("babeldoc");

    packaged.is_file()
}

fn has_override() -> bool {
    std::env::var_os("BFX_BABELDOC")
        .map(std::path::PathBuf::from)
        .is_some_and(|path| path.is_file())
}
