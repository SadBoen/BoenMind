use crate::config::Config;
use serde_json::{json, Value};
use std::{
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};
use tokio::{
    io::{AsyncRead, AsyncReadExt},
    process::Command,
    time::timeout,
};

pub async fn execute_code(config: &Config, args: &Value) -> Result<Value, String> {
    let language = args
        .get("language")
        .and_then(Value::as_str)
        .ok_or("language 不能为空")?;
    let code = args
        .get("code")
        .and_then(Value::as_str)
        .ok_or("code 不能为空")?;
    let argv = string_array(args.get("argv"))?;
    let cwd = resolve_cwd(config, args.get("cwd").and_then(Value::as_str))?;
    let timeout_ms = args
        .get("timeout_ms")
        .and_then(Value::as_u64)
        .unwrap_or(config.default_timeout_ms)
        .clamp(100, 600_000);
    let max_output_bytes = args
        .get("max_output_bytes")
        .and_then(Value::as_u64)
        .unwrap_or(config.max_output_bytes as u64)
        .clamp(1_024, 2_097_152) as usize;
    let (program, mut command_args): (&str, Vec<String>) = match language {
        "shell" => ("sh", vec!["-c".into(), code.into()]),
        "python" => ("python3", vec!["-c".into(), code.into()]),
        "node" => ("node", vec!["-e".into(), code.into()]),
        _ => return Err("仅支持 shell、python、node".into()),
    };
    command_args.extend(argv);
    run(program, &command_args, cwd, timeout_ms, max_output_bytes).await
}

pub async fn execute_file(config: &Config, args: &Value) -> Result<Value, String> {
    let path = PathBuf::from(
        args.get("path")
            .and_then(Value::as_str)
            .ok_or("path 不能为空")?,
    );
    let path = canonical_existing(&path)?;
    if !config.allows(&path) {
        return Err("path 不在 allowed_roots 内".into());
    }
    let language = args
        .get("language")
        .and_then(Value::as_str)
        .unwrap_or_else(|| match path.extension().and_then(|v| v.to_str()) {
            Some("py") => "python",
            Some("js") => "node",
            _ => "shell",
        });
    let program = match language {
        "python" => "python3",
        "node" => "node",
        "shell" => "sh",
        _ => return Err("仅支持 shell、python、node".into()),
    };
    let args_vec = string_array(args.get("argv"))?;
    let mut command_args = vec![path.to_string_lossy().into_owned()];
    command_args.extend(args_vec);
    run(
        program,
        &command_args,
        resolve_cwd(config, args.get("cwd").and_then(Value::as_str))?,
        config.default_timeout_ms,
        config.max_output_bytes,
    )
    .await
}

pub async fn execute_batch(config: &Config, args: &Value) -> Result<Value, String> {
    let items = args
        .get("items")
        .and_then(Value::as_array)
        .ok_or("items 必须是数组")?;
    if items.is_empty() || items.len() > 16 {
        return Err("items 数量必须在 1..=16".into());
    }
    let mut results = Vec::with_capacity(items.len());
    for item in items {
        results.push(match execute_code(config, item).await {
            Ok(v) => json!({"success":true,"result":v}),
            Err(e) => json!({"success":false,"error":e}),
        });
    }
    Ok(json!({"success":true,"items":results}))
}

async fn run(
    program: &str,
    args: &[String],
    cwd: PathBuf,
    timeout_ms: u64,
    max_output_bytes: usize,
) -> Result<Value, String> {
    let max = max_output_bytes;
    let mut child = Command::new(program);
    child
        .args(args)
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env_clear();
    for key in [
        "PATH",
        "HOME",
        "USER",
        "TMP",
        "TEMP",
        "TMPDIR",
        "SYSTEMROOT",
        "SystemRoot",
    ] {
        if let Ok(value) = std::env::var(key) {
            child.env(key, value);
        }
    }
    let mut child = child.spawn().map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            format!("runtime_unavailable:{program}")
        } else {
            e.to_string()
        }
    })?;
    let stdout = child.stdout.take().ok_or("stdout 不可用")?;
    let stderr = child.stderr.take().ok_or("stderr 不可用")?;
    let read_out = tokio::spawn(read_limited(stdout, max));
    let read_err = tokio::spawn(read_limited(stderr, max));
    let status = match timeout(Duration::from_millis(timeout_ms), child.wait()).await {
        Ok(result) => result.map_err(|e| e.to_string())?,
        Err(_) => {
            let _ = child.kill().await;
            return Err("execution_timeout".into());
        }
    };
    let out = read_out.await.map_err(|e| e.to_string())??;
    let err = read_err.await.map_err(|e| e.to_string())??;
    Ok(
        json!({"success":status.success(),"code":status.code(),"stdout":String::from_utf8_lossy(&out.bytes),"stderr":String::from_utf8_lossy(&err.bytes),"stdout_truncated":out.truncated,"stderr_truncated":err.truncated}),
    )
}

struct Limited {
    bytes: Vec<u8>,
    truncated: bool,
}
async fn read_limited<R: AsyncRead + Unpin>(mut reader: R, max: usize) -> Result<Limited, String> {
    let mut bytes = Vec::with_capacity(max.min(8192));
    let mut chunk = [0_u8; 8192];
    let mut truncated = false;
    loop {
        let count = reader.read(&mut chunk).await.map_err(|e| e.to_string())?;
        if count == 0 {
            break;
        }
        if bytes.len() < max {
            let keep = (max - bytes.len()).min(count);
            bytes.extend_from_slice(&chunk[..keep]);
            if keep < count {
                truncated = true;
            }
        } else {
            truncated = true;
        }
    }
    Ok(Limited { bytes, truncated })
}
fn string_array(value: Option<&Value>) -> Result<Vec<String>, String> {
    value
        .map(|v| {
            v.as_array()
                .ok_or_else(|| "argv/items 必须是数组".to_string())
                .and_then(|a| {
                    a.iter()
                        .map(|v| {
                            v.as_str()
                                .map(ToOwned::to_owned)
                                .ok_or_else(|| "参数必须是字符串".to_string())
                        })
                        .collect()
                })
        })
        .unwrap_or_else(|| Ok(Vec::new()))
}
fn resolve_cwd(config: &Config, cwd: Option<&str>) -> Result<PathBuf, String> {
    let path = cwd
        .map(PathBuf::from)
        .unwrap_or_else(|| config.allowed_roots[0].clone());
    let path = canonical_existing(&path)?;
    if !config.allows(&path) {
        return Err("cwd 不在 allowed_roots 内".into());
    }
    Ok(path)
}
fn canonical_existing(path: &Path) -> Result<PathBuf, String> {
    std::fs::canonicalize(path).map_err(|e| format!("路径不可用:{e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn rejects_outside_cwd_and_limits_batch() {
        let dir = TempDir::new().expect("temp");
        let other = TempDir::new().expect("other");
        let cfg = Config::for_test(dir.path().to_path_buf());
        let error = execute_code(
            &cfg,
            &json!({"language":"shell","code":"printf ok","cwd":other.path()}),
        )
        .await
        .expect_err("cwd");
        assert!(error.contains("allowed_roots"));
        let error = execute_batch(&cfg, &json!({"items":[]}))
            .await
            .expect_err("empty batch");
        assert!(error.contains("1..=16"));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn captures_and_truncates_output() {
        let dir = TempDir::new().expect("temp");
        let mut cfg = Config::for_test(dir.path().to_path_buf());
        cfg.max_output_bytes = 1_024;
        let result = execute_code(
            &cfg,
            &json!({
                "language":"shell",
                "code":"head -c 2048 /dev/zero",
                "max_output_bytes":1024
            }),
        )
        .await
        .expect("execute");
        assert_eq!(result["stdout"].as_str().unwrap().len(), 1024);
        assert_eq!(result["stdout_truncated"], true);
    }

    #[tokio::test]
    async fn missing_runtime_is_structured_error() {
        let dir = TempDir::new().expect("temp");
        let cfg = Config::for_test(dir.path().to_path_buf());
        let result = run(
            "context-mode-nonexistent-runtime",
            &[],
            dir.path().to_path_buf(),
            cfg.default_timeout_ms,
            cfg.max_output_bytes,
        )
        .await
        .expect_err("runtime");
        assert!(result.contains("runtime_unavailable"));
    }
}
