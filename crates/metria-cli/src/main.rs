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
    Hub,
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
    Export,
    /// 备份 Hub 数据库
    Backup,
    /// 恢复 Hub 数据库
    Restore,
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
        Command::Hub => match run_hub() {
            Ok(code) => code,
            Err(e) => {
                tracing::error!(%e, "hub 启动失败");
                ExitCode::FAILURE
            }
        },
        other => {
            eprintln!("{NOT_IMPLEMENTED}: {other:?}");
            ExitCode::FAILURE
        }
    }
}

fn run_healthcheck() -> Result<(), String> {
    let cfg = metria_hub::HubConfig::from_env().map_err(|e| e.to_string())?;
    metria_hub::healthcheck(&cfg).map_err(|e| e.to_string())?;
    println!("ok");
    Ok(())
}

fn run_hub() -> Result<ExitCode, metria_hub::HubError> {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|e| metria_hub::HubError::Io(std::io::Error::other(e)))?;
    rt.block_on(async {
        let cfg = metria_hub::HubConfig::from_env()?;
        metria_hub::serve(cfg).await?;
        Ok(ExitCode::SUCCESS)
    })
}
