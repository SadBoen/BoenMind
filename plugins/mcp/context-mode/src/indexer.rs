use crate::{config::Config, store::Store};
use serde_json::{json, Value};
use std::{
    fs,
    path::{Path, PathBuf},
};

const TEXT_EXTENSIONS: &[&str] = &[
    "txt", "md", "markdown", "json", "yaml", "yml", "toml", "rs", "ts", "tsx", "js", "jsx", "py",
    "sh", "html", "css", "xml", "csv", "log",
];

pub fn index_path(
    store: &Store,
    config: &Config,
    requested: Option<&Path>,
) -> Result<Value, String> {
    let root = requested.unwrap_or_else(|| {
        config
            .allowed_roots
            .first()
            .map(PathBuf::as_path)
            .unwrap_or(config.data_dir.as_path())
    });
    if !config.allows(root) {
        return Err("path 不在 allowed_roots 内".into());
    }
    let mut files = Vec::new();
    collect(root, config, &mut files)?;
    let mut indexed = 0usize;
    let mut skipped = 0usize;
    for path in files.into_iter().take(config.max_files) {
        let metadata = fs::metadata(&path).map_err(|e| e.to_string())?;
        if metadata.len() as usize > config.max_file_bytes {
            skipped += 1;
            continue;
        }
        let content = match fs::read_to_string(&path) {
            Ok(v) => v,
            Err(_) => {
                skipped += 1;
                continue;
            }
        };
        let modified = metadata
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0);
        store.upsert_document(&path.to_string_lossy(), &content, content.len(), modified)?;
        indexed += 1;
    }
    Ok(json!({"success":true,"indexed":indexed,"skipped":skipped,"limit":config.max_files}))
}

pub fn search(store: &Store, query: &str, limit: usize) -> Result<Value, String> {
    Ok(json!({"success":true,"query":query,"results":store.search(query, limit)?}))
}

fn collect(path: &Path, config: &Config, out: &mut Vec<PathBuf>) -> Result<(), String> {
    if out.len() >= config.max_files {
        return Ok(());
    }
    if !config.allows(path) {
        return Ok(());
    }
    let path = fs::canonicalize(path).map_err(|e| e.to_string())?;
    if !config.allows(&path) {
        return Ok(());
    }
    let metadata = fs::metadata(&path).map_err(|e| e.to_string())?;
    if metadata.is_file() {
        if path
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| TEXT_EXTENSIONS.contains(&e.to_ascii_lowercase().as_str()))
            .unwrap_or(false)
        {
            out.push(path.to_path_buf());
        }
        return Ok(());
    }
    if metadata.is_dir() {
        for entry in fs::read_dir(path).map_err(|e| e.to_string())? {
            let entry = entry.map_err(|e| e.to_string())?;
            collect(&entry.path(), config, out)?;
            if out.len() >= config.max_files {
                break;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn indexes_text_and_searches_fts5() {
        let dir = TempDir::new().expect("temp");
        let file = dir.path().join("note.md");
        fs::write(&file, "rust context mode searchable").expect("write");
        let cfg = Config::for_test(dir.path().to_path_buf());
        let store = Store::open(&dir.path().join("db.sqlite3")).expect("db");
        let indexed = index_path(&store, &cfg, Some(dir.path())).expect("index");
        assert_eq!(indexed["indexed"], 1);
        let found = search(&store, "searchable", 10).expect("search");
        assert_eq!(
            found["results"][0]["path"],
            fs::canonicalize(&file)
                .expect("canonical file")
                .to_string_lossy()
                .to_string()
        );
        assert_eq!(found["results"][0]["trust"], "untrusted");
    }

    #[test]
    fn rejects_path_outside_allowed_root() {
        let dir = TempDir::new().expect("temp");
        let other = TempDir::new().expect("other");
        let cfg = Config::for_test(dir.path().to_path_buf());
        let store = Store::open(&dir.path().join("db.sqlite3")).expect("db");
        let error = index_path(&store, &cfg, Some(other.path())).expect_err("must reject");
        assert!(error.contains("allowed_roots"));
    }
}
