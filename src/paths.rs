use anyhow::Context;
use std::path::PathBuf;
use std::sync::OnceLock;

static APP_DIR: OnceLock<PathBuf> = OnceLock::new();

pub fn init() -> anyhow::Result<()> {
    let exe = std::env::current_exe()?;
    let exe_dir = exe
        .parent()
        .context("Could not determine executable directory")?;

    // cargo run: executable is in target/debug or target/release.
    let cargo_dir = exe_dir.parent().and_then(|p| p.parent());

    let app_dir = if let Some(project_dir) = cargo_dir {
        if project_dir.join("Cargo.toml").exists() {
            project_dir.to_path_buf()
        } else {
            exe_dir.to_path_buf()
        }
    } else {
        exe_dir.to_path_buf()
    };

    APP_DIR
        .set(app_dir)
        .map_err(|_| anyhow::anyhow!("Application paths already initialized"))
}

pub fn app_dir() -> &'static PathBuf {
    APP_DIR.get().expect("paths::init() must be called first")
}

pub fn db() -> PathBuf {
    app_dir().join("db.db")
}

pub fn jm() -> PathBuf {
    app_dir().join("jm.xml")
}

pub fn dict() -> PathBuf {
    app_dir().join("system.dic")
}

pub fn model() -> PathBuf {
    app_dir().join("models/lfm/lfmf16.gguf")
}

#[cfg(windows)]
pub fn llama() -> PathBuf {
    app_dir().join("tools/llama.cpp/cuda/llama-server.exe")
}

#[cfg(windows)]
pub fn llama_cpu() -> PathBuf {
    app_dir().join("tools/llama.cpp/cpu/llama-server.exe")
}

#[cfg(target_os = "linux")]
pub fn llama() -> PathBuf {
    app_dir().join("tools/llama.cpp/cuda/llama-server")
}

#[cfg(target_os = "linux")]
pub fn llama_cpu() -> PathBuf {
    app_dir().join("tools/llama.cpp/cpu/llama-server")
}
