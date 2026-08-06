//! Metria 命令行入口。
#![warn(missing_debug_implementations, rust_2018_idioms)]

use std::process::ExitCode;

use clap::{Parser, Subcommand};

const PKG_VERSION: &str = env!("CARGO_PKG_VERSION");
const NOT_IMPLEMENTED: &str = "not implemented in M1";

#[derive(Debug, Parser)]
#[command(
    name = "metria",
    version = PKG_VERSION,
    about = "Metria - AI coding agent 用量监控、费用分析与流量估算平台",
    disable_help_subcommand = true
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// 启动 Metria Hub 服务
    Hub {
        /// Demo 模式：生成确定性合成数据并展示
        #[arg(long, default_value_t = false)]
        demo: bool,
    },
    /// 启动 Metria Agent（Collector）
    Agent,
    /// 将客户端数据源目录导入为归一化事件
    Import {
        /// 数据源客户端：claude | codex | opencode
        #[arg(long)]
        source: String,
        /// 客户端数据目录
        #[arg(long)]
        path: String,
        /// 只解析并输出到 stdout，不写入任何数据
        #[arg(long, default_value_t = false)]
        dry_run: bool,
        /// 输出 NDJSON 文件路径（默认 stdout）
        #[arg(long)]
        out: Option<String>,
    },
    /// 环境诊断检查
    Doctor {
        /// 指定检查目标
        #[arg(long)]
        adapter: Option<String>,
        #[arg(long, default_value_t = false)]
        hub: bool,
        #[arg(long, default_value_t = false)]
        database: bool,
        #[arg(long, default_value_t = false)]
        spool: bool,
        #[arg(long, default_value_t = false)]
        traffic: bool,
    },
    /// 查看或生成配置
    Config,
    /// 导出数据
    Export {
        /// 导出内容：sessions | calls | usage
        #[arg(long)]
        kind: String,
        /// 导出格式：json | ndjson | csv
        #[arg(long, default_value = "json")]
        format: String,
        /// 起始时间（RFC3339），默认 30 天前
        #[arg(long)]
        from: Option<String>,
        /// 结束时间（RFC3339），默认当前
        #[arg(long)]
        to: Option<String>,
        /// 输出文件路径（默认 ./metria-export-<kind>-<ts>）
        #[arg(long)]
        out: Option<String>,
    },
    /// 备份 Hub 数据库
    Backup {
        /// 输出文件路径（默认 ./metria-backup-<ts>.db.zst）
        #[arg(long)]
        out: Option<String>,
    },
    /// 恢复 Hub 数据库（需先停止 Hub）
    Restore {
        /// 备份文件路径
        #[arg(long)]
        input: String,
    },
    /// 运行只读 MCP 服务
    Mcp,
    /// 容器健康检查（返回 0 表示健康）
    Healthcheck,
    /// 打印版本信息
    Version,
}

fn main() -> ExitCode {
    metria_cli::init();
    let cli = Cli::parse();
    match cli.command {
        Command::Version => {
            println!("metria {PKG_VERSION}");
            ExitCode::SUCCESS
        }
        Command::Healthcheck => match run_healthcheck() {
            Ok(()) => ExitCode::SUCCESS,
            Err(msg) => {
                eprintln!("healthcheck failed: {msg}");
                ExitCode::FAILURE
            }
        },
        Command::Agent => match run_agent() {
            Ok(code) => code,
            Err(e) => {
                tracing::error!(%e, "agent 启动失败");
                ExitCode::FAILURE
            }
        },
        Command::Hub { demo } => match run_hub(demo) {
            Ok(code) => code,
            Err(e) => {
                tracing::error!(%e, "hub 启动失败");
                ExitCode::FAILURE
            }
        },
        Command::Import {
            source,
            path,
            dry_run,
            out,
        } => match metria_cli::import::run_import(&source, &path, dry_run, out.as_deref()) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("import 失败: {e}");
                ExitCode::FAILURE
            }
        },
        Command::Doctor {
            adapter,
            hub,
            database,
            spool,
            traffic,
        } => {
            match metria_cli::doctor::run_doctor(adapter.as_deref(), hub, database, spool, traffic)
            {
                Ok(()) => ExitCode::SUCCESS,
                Err(e) => {
                    eprintln!("doctor 失败: {e}");
                    ExitCode::FAILURE
                }
            }
        }
        Command::Backup { out } => match run_backup(out.as_deref()) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("备份失败: {e}");
                ExitCode::FAILURE
            }
        },
        Command::Restore { input } => match run_restore(&input) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("恢复失败: {e}");
                ExitCode::FAILURE
            }
        },
        Command::Mcp => match run_mcp() {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("MCP 退出: {e}");
                ExitCode::FAILURE
            }
        },
        Command::Export {
            kind,
            format,
            from,
            to,
            out,
        } => match run_export(
            &kind,
            &format,
            from.as_deref(),
            to.as_deref(),
            out.as_deref(),
        ) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("导出失败: {e}");
                ExitCode::FAILURE
            }
        },
        other => {
            eprintln!("{NOT_IMPLEMENTED}: {other:?}");
            ExitCode::FAILURE
        }
    }
}

fn hub_db_url() -> Result<String, String> {
    Ok(metria_hub::HubConfig::from_env()
        .map_err(|e| e.to_string())?
        .database_url)
}

fn run_backup(out: Option<&str>) -> Result<(), String> {
    let url = hub_db_url()?;
    metria_cli::backup::backup(&url, out)
}

fn run_restore(input: &str) -> Result<(), String> {
    let url = hub_db_url()?;
    metria_cli::backup::restore(&url, input)
}

fn run_export(
    kind: &str,
    format: &str,
    from: Option<&str>,
    to: Option<&str>,
    out: Option<&str>,
) -> Result<(), String> {
    let url = hub_db_url()?;
    let kind = metria_cli::export::parse_kind(kind)
        .ok_or_else(|| format!("未知导出类型: {kind}（支持 sessions/calls/usage）"))?;
    let fmt = metria_hub::export::parse_format(format)
        .ok_or_else(|| format!("未知导出格式: {format}（支持 json/ndjson/csv）"))?;
    let from = from
        .map(|s| {
            chrono::DateTime::parse_from_rfc3339(s)
                .map(|t| t.with_timezone(&chrono::Utc))
                .map_err(|e| format!("from 解析失败: {e}"))
        })
        .transpose()?
        .unwrap_or_else(|| chrono::Utc::now() - chrono::Duration::days(30));
    let to = to
        .map(|s| {
            chrono::DateTime::parse_from_rfc3339(s)
                .map(|t| t.with_timezone(&chrono::Utc))
                .map_err(|e| format!("to 解析失败: {e}"))
        })
        .transpose()?
        .unwrap_or_else(chrono::Utc::now);
    metria_cli::export::export(&url, kind, &fmt, from, to, out)
}

fn run_mcp() -> Result<(), String> {
    metria_cli::mcp::run()
}

fn run_healthcheck() -> Result<(), String> {
    let cfg = metria_hub::HubConfig::from_env().map_err(|e| e.to_string())?;
    metria_hub::healthcheck(&cfg).map_err(|e| e.to_string())?;
    println!("ok");
    Ok(())
}

fn run_agent() -> Result<ExitCode, metria_agent::AgentError> {
    let cfg = metria_agent::AgentConfig::from_env()?;
    metria_agent::run(cfg)?;
    Ok(ExitCode::SUCCESS)
}

fn run_hub(demo: bool) -> Result<ExitCode, metria_hub::HubError> {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|e| metria_hub::HubError::Io(std::io::Error::other(e)))?;
    rt.block_on(async {
        let mut cfg = metria_hub::HubConfig::from_env()?;
        cfg.demo = demo;
        metria_hub::serve(cfg).await?;
        Ok(ExitCode::SUCCESS)
    })
}
