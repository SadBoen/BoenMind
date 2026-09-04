use std::path::PathBuf;

/// 插件运行时配置
#[derive(Debug, Clone)]
pub struct Config {
    /// 数据目录路径 (context-log.jsonl 所在根目录)
    pub data_dir: PathBuf,
}

impl Config {
    /// 从 CLI 参数或环境变量探测解析
    pub fn from_args() -> Self {
        let args: Vec<String> = std::env::args().collect();
        let mut data_dir: Option<PathBuf> = None;

        let mut i = 1;
        while i < args.len() {
            match args[i].as_str() {
                "--data-dir" if i + 1 < args.len() => {
                    data_dir = Some(PathBuf::from(&args[i + 1]));
                    i += 1;
                }
                "--config" if i + 1 < args.len() => {
                    let cfg_path = PathBuf::from(&args[i + 1]);
                    if let Some(parent) = cfg_path.parent() {
                        // 若传入 config/mcp-context-inspector.json，其上一级为数据目录
                        if parent.ends_with("config") {
                            data_dir = parent.parent().map(|p| p.to_path_buf());
                        } else {
                            data_dir = Some(parent.to_path_buf());
                        }
                    }
                    i += 1;
                }
                _ => {}
            }
            i += 1;
        }

        // 环境变量兜底 BOEN_DATA_DIR / BOEN_WORKSPACE_DIR
        let data_dir = data_dir
            .or_else(|| std::env::var("BOEN_DATA_DIR").ok().map(PathBuf::from))
            .or_else(|| {
                // Windows %APPDATA%/boenmind 默认路径
                #[cfg(target_os = "windows")]
                {
                    std::env::var("APPDATA")
                        .ok()
                        .map(|a| PathBuf::from(a).join("boenmind"))
                }
                #[cfg(not(target_os = "windows"))]
                {
                    std::env::var("HOME")
                        .ok()
                        .map(|h| PathBuf::from(h).join(".boenmind"))
                }
            })
            .unwrap_or_else(|| PathBuf::from("."));

        Self { data_dir }
    }

    pub fn context_log_path(&self) -> PathBuf {
        self.data_dir.join("context-log.jsonl")
    }

    pub fn model_config_path(&self) -> PathBuf {
        self.data_dir.join("config").join("model.json")
    }
}
