//! `.env` loading on top of `dotenvy`. The process environment always wins over the file
//! (`dotenvy::dotenv` only sets variables that aren't already present).

/// Loads the first `.env` found, in debug builds only (a shipped app has no project folder): the current directory (and its ancestors, via `dotenvy`),
/// then the executable's directory. Missing or unreadable files are fine.
pub fn load_env_file() {
    if !cfg!(debug_assertions) {
        return;
    }
    if dotenvy::dotenv().is_ok() {
        return;
    }
    if let Some(exe_dotenv) = std::env::current_exe()
        .ok()
        .and_then(|e| e.parent().map(|p| p.join(".env")))
    {
        let _ = dotenvy::from_path(exe_dotenv);
    }
}
