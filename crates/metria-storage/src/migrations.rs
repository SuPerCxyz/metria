//! 版本化数据库迁移框架。
//!
//! SQL 文件位于仓库 `migrations/` 目录，命名 `N_name.sql`（N 为递增整数）。
//! 通过 `rust-embed` 在编译期嵌入，随二进制分发，无需运行时挂载。

use rusqlite::Connection;

use crate::error::{Result, StorageError};

/// 单条迁移。
#[derive(Debug, Clone)]
pub struct Migration {
    pub version: i64,
    pub name: String,
    pub sql: String,
}

/// 从编译期嵌入的 `migrations/` 目录加载全部迁移（按版本升序）。
pub fn load_embedded() -> Result<Vec<Migration>> {
    let mut migrations = Vec::new();
    for file in Embedded::iter() {
        let name = file.as_ref();
        if !name.ends_with(".sql") {
            continue;
        }
        let version = parse_version(name).ok_or_else(|| {
            StorageError::Migrate(format!("迁移文件名不符合 N_name.sql 规范: {name}"))
        })?;
        let sql = Embedded::get(&file)
            .ok_or_else(|| StorageError::Migrate(format!("嵌入迁移缺失: {name}")))?
            .data;
        let sql = String::from_utf8(sql.into_owned())
            .map_err(|_| StorageError::Migrate(format!("迁移非 UTF-8: {name}")))?;
        migrations.push(Migration {
            version,
            name: name.to_string(),
            sql,
        });
    }
    migrations.sort_by_key(|m| m.version);
    Ok(migrations)
}

/// 解析迁移文件名的版本号，如 `001_init.sql` -> 1。
fn parse_version(name: &str) -> Option<i64> {
    let stem = name.strip_suffix(".sql")?;
    let num = stem.split('_').next()?;
    num.parse::<i64>().ok()
}

/// 确保 `schema_migrations` 表存在。
pub fn ensure_migrations_table(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS schema_migrations (
            version   INTEGER PRIMARY KEY,
            name      TEXT NOT NULL,
            applied_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now'))
        );",
    )
    .map_err(|e| StorageError::Migrate(e.to_string()))?;
    Ok(())
}

/// 当前已应用的最高迁移版本。
pub fn current_version(conn: &Connection) -> Result<i64> {
    ensure_migrations_table(conn)?;
    let v: Option<i64> = conn
        .query_row("SELECT MAX(version) FROM schema_migrations", [], |r| {
            r.get(0)
        })
        .map_err(|e| StorageError::Migrate(e.to_string()))?;
    Ok(v.unwrap_or(0))
}

/// 应用迁移到目标版本（默认全部）。
///
/// 每条迁移在独立事务中执行；失败时该条回滚，之前成功的保留。
pub fn migrate(
    conn: &mut Connection,
    migrations: &[Migration],
    target: Option<i64>,
) -> Result<Vec<i64>> {
    ensure_migrations_table(conn)?;
    let current = current_version(conn)?;
    let mut applied = Vec::new();
    for m in migrations {
        if m.version <= current {
            continue;
        }
        if let Some(t) = target {
            if m.version > t {
                break;
            }
        }
        let tx = conn
            .transaction()
            .map_err(|e| StorageError::Migrate(e.to_string()))?;
        {
            tx.execute_batch(&m.sql).map_err(|e| {
                StorageError::Migrate(format!("版本 {} ({}) 失败: {e}", m.version, m.name))
            })?;
            tx.execute(
                "INSERT INTO schema_migrations (version, name) VALUES (?1, ?2)",
                rusqlite::params![m.version, m.name],
            )
            .map_err(|e| StorageError::Migrate(e.to_string()))?;
        }
        tx.commit()
            .map_err(|e| StorageError::Migrate(e.to_string()))?;
        applied.push(m.version);
    }
    Ok(applied)
}

/// 便捷方法：加载嵌入迁移并应用到最新版本。
pub fn migrate_embedded(conn: &mut Connection, target: Option<i64>) -> Result<Vec<i64>> {
    let migrations = load_embedded()?;
    migrate(conn, &migrations, target)
}

#[derive(rust_embed::RustEmbed)]
#[folder = "../../migrations/"]
struct Embedded;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{open, DbOptions};
    use std::path::PathBuf;

    fn temp_path(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("metria-mig-{name}-{}", std::process::id()))
    }

    #[test]
    fn parse_version_ok() {
        assert_eq!(parse_version("001_init.sql"), Some(1));
        assert_eq!(parse_version("123_foo.sql"), Some(123));
        assert_eq!(parse_version("init.sql"), None);
        assert_eq!(parse_version("01.sql"), Some(1));
    }

    #[test]
    fn embedded_load_sorted() {
        let ms = load_embedded().unwrap();
        assert!(!ms.is_empty());
        let versions: Vec<i64> = ms.iter().map(|m| m.version).collect();
        let mut sorted = versions.clone();
        sorted.sort();
        assert_eq!(versions, sorted);
    }

    #[test]
    fn migrate_to_latest_and_idempotent() {
        let path = temp_path("latest");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let mut conn = open(&path, &DbOptions::default()).unwrap();
        let applied = migrate_embedded(&mut conn, None).unwrap();
        assert!(!applied.is_empty(), "至少一个迁移");
        assert!(applied.windows(2).all(|w| w[0] < w[1]), "版本应升序");
        // 重复迁移应无操作
        let applied2 = migrate_embedded(&mut conn, None).unwrap();
        assert!(applied2.is_empty());
        assert_eq!(current_version(&conn).unwrap(), *applied.last().unwrap());
        for suffix in ["", "-wal", "-shm"] {
            let _ = std::fs::remove_file(format!("{}{suffix}", path.display()));
        }
    }

    #[test]
    fn migrate_target_and_bad_sql_rollback() {
        let path = temp_path("bad");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let mut conn = open(&path, &DbOptions::default()).unwrap();
        // 目标版本 0：不应用任何迁移
        let applied = migrate_embedded(&mut conn, Some(0)).unwrap();
        assert!(applied.is_empty());
        assert_eq!(current_version(&conn).unwrap(), 0);

        // 坏 SQL：迁移必须回滚，且版本不记录
        let bad = Migration {
            version: 99,
            name: "bad.sql".into(),
            sql: "CREATE TABLE t(".into(),
        };
        let err = migrate(&mut conn, &[bad], None).unwrap_err();
        assert!(err.to_string().contains("99"));
        assert_eq!(current_version(&conn).unwrap(), 0);
        for suffix in ["", "-wal", "-shm"] {
            let _ = std::fs::remove_file(format!("{}{suffix}", path.display()));
        }
    }

    /// 018 只把 Codex 的 output_tokens 扣减为不含推理的生成 Token，
    /// 其它客户端与 reasoning 保持原值，且总 Token 恒等。
    #[test]
    fn migration_018_normalizes_codex_output_only() {
        let path = temp_path("reasoning-output");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let mut conn = open(&path, &DbOptions::default()).unwrap();
        migrate_embedded(&mut conn, Some(17)).unwrap();

        let insert_usage = "INSERT INTO usage_events (
                event_id, schema_version, node_id, collector_id, source_id, client_id,
                adapter_id, adapter_version, timestamp, input_tokens, output_tokens,
                reasoning_tokens, usage_source, usage_granularity
            ) VALUES (?1, 1, 'n', 'c', 's', ?2, 'a', '1', '2026-09-01T00:00:00Z', ?3, ?4, ?5,
                      'reported', 'call')";
        let rows: [(&str, &str, i64, i64, Option<i64>); 4] = [
            // Codex：输出含推理
            ("e1", "codex", 100, 370, Some(107)),
            // Codex 异常行：推理大于输出
            ("e2", "codex", 100, 50, Some(80)),
            // 输出本就不含推理的客户端
            ("e3", "opencode", 100, 370, Some(107)),
            // 无推理的 Codex 行
            ("e4", "codex", 100, 200, None),
        ];
        for (id, client, input, output, reasoning) in rows {
            conn.execute(
                insert_usage,
                rusqlite::params![id, client, input, output, reasoning],
            )
            .unwrap();
        }

        let insert_call = "INSERT INTO model_calls (
                id, node_id, collector_id, client_id, source_id, session_id, started_at,
                status, call_granularity, output_tokens, reasoning_tokens, created_at, updated_at
            ) VALUES (?1, 'n', 'c', ?2, 's', 'sess', '2026-09-01T00:00:00Z', 'success', 'call', ?3, ?4,
                      '2026-09-01T00:00:00Z', '2026-09-01T00:00:00Z')";
        conn.execute(
            insert_call,
            rusqlite::params!["c1", "codex", 370i64, 107i64],
        )
        .unwrap();
        conn.execute(
            insert_call,
            rusqlite::params!["c2", "opencode", 370i64, 107i64],
        )
        .unwrap();

        let applied = migrate_embedded(&mut conn, Some(18)).unwrap();
        assert_eq!(applied, vec![18]);

        let read_usage = |id: &str| -> (i64, Option<i64>) {
            conn.query_row(
                "SELECT output_tokens, reasoning_tokens FROM usage_events WHERE event_id = ?1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap()
        };
        // 370 - 107；异常行钳到 0；非 Codex 不变；无推理不变
        assert_eq!(read_usage("e1"), (263, Some(107)));
        assert_eq!(read_usage("e2"), (0, Some(80)));
        assert_eq!(read_usage("e3"), (370, Some(107)));
        assert_eq!(read_usage("e4"), (200, None));

        let read_call = |id: &str| -> (i64, Option<i64>) {
            conn.query_row(
                "SELECT output_tokens, reasoning_tokens FROM model_calls WHERE id = ?1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap()
        };
        assert_eq!(read_call("c1"), (263, Some(107)));
        assert_eq!(read_call("c2"), (370, Some(107)));

        // 归一化后 output + reasoning 等于归一化前的 output（推理只计一次）
        assert_eq!(100 + 263 + 107, 100 + 370);

        for suffix in ["", "-wal", "-shm"] {
            let _ = std::fs::remove_file(format!("{}{suffix}", path.display()));
        }
    }

    /// 019 只回填「017/018 应用后、Agent 更新前」入库的 Codex 行：
    /// input 补扣缓存、output 补扣推理；窗口外与已归一化的行不动。
    #[test]
    fn migration_019_fills_only_the_late_ingest_window() {
        let path = temp_path("late-ingest");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let mut conn = open(&path, &DbOptions::default()).unwrap();
        migrate_embedded(&mut conn, Some(18)).unwrap();
        // 固定 017/018 的应用时刻，避免依赖测试运行时间
        conn.execute(
            "UPDATE schema_migrations SET applied_at = '2026-09-15T16:16:00Z' WHERE version = 17",
            [],
        )
        .unwrap();
        conn.execute(
            "UPDATE schema_migrations SET applied_at = '2026-09-16T03:03:00Z' WHERE version = 18",
            [],
        )
        .unwrap();

        let insert_usage = "INSERT INTO usage_events (
                event_id, schema_version, node_id, collector_id, source_id, client_id,
                adapter_id, adapter_version, timestamp, input_tokens, output_tokens,
                reasoning_tokens, cache_read_tokens, cache_write_tokens,
                usage_source, usage_granularity
            ) VALUES (?1, 1, 'n', 'c', 's', ?2, 'a', '1', '2026-09-16T00:00:00Z', ?3, ?4, ?5, ?6, ?7,
                      'reported', 'call')";
        let insert_call = "INSERT INTO model_calls (
                id, node_id, collector_id, client_id, source_id, session_id, started_at,
                status, call_granularity, input_tokens, output_tokens, reasoning_tokens,
                cache_read_tokens, cache_write_tokens, usage_event_id, created_at, updated_at
            ) VALUES (?1, 'n', 'c', ?2, 's', 'sess', '2026-09-16T00:00:00Z', 'success', 'call',
                      ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?9)";

        // (name, client, created_at, input, output, reasoning, cache_read, cache_write)
        type WindowRow = (
            &'static str,
            &'static str,
            &'static str,
            i64,
            i64,
            i64,
            i64,
            i64,
        );
        let rows: [WindowRow; 7] = [
            // A: 017 之后、018 之前入库 —— 只修 input
            ("a", "codex", "2026-09-15T20:00:00Z", 1000, 200, 150, 900, 0),
            // B: 018 之后、Agent 更新前入库 —— input 与 output 都修
            (
                "b",
                "codex",
                "2026-09-16T04:00:00Z",
                2000,
                300,
                100,
                1500,
                0,
            ),
            // C: Agent 更新后入库 —— 不动
            ("c", "codex", "2026-09-16T06:00:00Z", 3000, 400, 200, 100, 0),
            // D: 017 之前入库 —— 不动
            (
                "d",
                "codex",
                "2026-09-15T10:00:00Z",
                4000,
                500,
                250,
                3000,
                0,
            ),
            // E: 窗口内但非 Codex —— 不动
            (
                "e",
                "opencode",
                "2026-09-16T04:00:00Z",
                5000,
                600,
                300,
                4000,
                0,
            ),
            // F: 窗口内但 input 已小于缓存 —— input 守卫拦住；output 仍未归一化，正常扣减
            ("f", "codex", "2026-09-16T04:00:00Z", 10, 700, 100, 500, 0),
            // G: 窗口内两个维度都已小于被扣项 —— 两个守卫都拦住，不动
            ("g", "codex", "2026-09-16T04:00:00Z", 20, 50, 100, 800, 0),
        ];
        for (id, client, created, input, output, reasoning, cr, cw) in rows {
            let event_id = format!("ev-{id}");
            conn.execute(
                insert_usage,
                rusqlite::params![event_id, client, input, output, reasoning, cr, cw],
            )
            .unwrap();
            conn.execute(
                insert_call,
                rusqlite::params![
                    format!("call-{id}"),
                    client,
                    input,
                    output,
                    reasoning,
                    cr,
                    cw,
                    event_id,
                    created
                ],
            )
            .unwrap();
        }

        let applied = migrate_embedded(&mut conn, Some(19)).unwrap();
        assert_eq!(applied, vec![19]);

        let read = |table: &str, key: &str| -> (i64, i64, i64) {
            let (col, id) = if table == "usage_events" {
                ("event_id", format!("ev-{key}"))
            } else {
                ("id", format!("call-{key}"))
            };
            conn.query_row(
                &format!(
                    "SELECT input_tokens, output_tokens, reasoning_tokens FROM {table} WHERE {col} = ?1"
                ),
                [id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap()
        };

        for (table, key) in [("usage_events", "a"), ("model_calls", "a")] {
            // 只扣 input：1000-900；output 200 在 018 之前入库，不应再扣推理
            assert_eq!(read(table, key), (100, 200, 150), "{table} row a");
        }
        for (table, key) in [("usage_events", "b"), ("model_calls", "b")] {
            // input 2000-1500；output 300-100
            assert_eq!(read(table, key), (500, 200, 100), "{table} row b");
        }
        for (table, key) in [("usage_events", "c"), ("model_calls", "c")] {
            assert_eq!(read(table, key), (3000, 400, 200), "{table} row c");
        }
        for (table, key) in [("usage_events", "d"), ("model_calls", "d")] {
            assert_eq!(read(table, key), (4000, 500, 250), "{table} row d");
        }
        for (table, key) in [("usage_events", "e"), ("model_calls", "e")] {
            assert_eq!(read(table, key), (5000, 600, 300), "{table} row e");
        }
        for (table, key) in [("usage_events", "f"), ("model_calls", "f")] {
            // input 守卫生效（10 < 500+0）；output 700-100
            assert_eq!(read(table, key), (10, 600, 100), "{table} row f");
        }
        for (table, key) in [("usage_events", "g"), ("model_calls", "g")] {
            // input 20 < 800、output 50 < 100，两个守卫都生效
            assert_eq!(read(table, key), (20, 50, 100), "{table} row g");
        }

        for suffix in ["", "-wal", "-shm"] {
            let _ = std::fs::remove_file(format!("{}{suffix}", path.display()));
        }
    }

    /// 024：opencode 存量时长改用 first_response_at 重算；claude-code 的 turn 跨度
    /// 时长诚实置空；codex 正常行一个字段都不动；>24h 走兜底降级。
    #[test]
    fn migration_024_repairs_duration_origin() {
        let path = temp_path("dur024");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let mut conn = open(&path, &DbOptions::default()).unwrap();
        let up_to_23 = migrate_embedded(&mut conn, Some(23)).unwrap();
        assert_eq!(up_to_23.last().copied(), Some(23), "应先应用到 023");

        let insert = "INSERT INTO model_calls (
                id, node_id, collector_id, client_id, source_id, session_id, started_at,
                first_response_at, completed_at, duration_ms, timing_source, timing_quality,
                status, call_granularity, output_tokens, created_at, updated_at
            ) VALUES (?1, 'n', 'c', ?2, 's', 'sess', ?3, ?4, ?5, ?6, ?7, ?8,
                      'success', 'message', 700, '2026-09-01T00:00:00Z', '2026-09-01T00:00:00Z')";
        type Row = (
            &'static str,
            &'static str,
            &'static str,
            Option<&'static str>,
            Option<&'static str>,
            Option<i64>,
            Option<&'static str>,
            Option<&'static str>,
        );
        let rows: [Row; 6] = [
            // g1 可重算：started 是 4 小时前的 turn 起点（错值），fr 才是请求起点
            (
                "g1",
                "opencode",
                "2026-08-31T20:00:00Z",
                Some("2026-09-01T00:00:00Z"),
                Some("2026-09-01T00:10:00Z"),
                Some(14_400_000),
                Some("opencode_message_timestamps"),
                Some("observed"),
            ),
            // g2 opencode 缺 fr：无法重算 -> 诚实置空
            (
                "g2",
                "opencode",
                "2026-09-01T01:00:00Z",
                None,
                Some("2026-09-01T01:05:00Z"),
                Some(300_000),
                Some("opencode_message_timestamps"),
                Some("observed"),
            ),
            // c1 claude-code：turn 跨度口径不可用 -> 置空并统一 source 词汇
            (
                "c1",
                "claude-code",
                "2026-09-01T02:00:00Z",
                None,
                Some("2026-09-01T02:05:00Z"),
                Some(3_000_000),
                Some("claude_message_timestamps"),
                Some("bounded"),
            ),
            // x1 codex 正常行：起点本就正确，必须完全不变
            (
                "x1",
                "codex",
                "2026-09-01T03:00:00Z",
                Some("2026-09-01T03:00:30Z"),
                Some("2026-09-01T03:04:00Z"),
                Some(60_000),
                Some("codex_turn_to_token_count"),
                Some("observed"),
            ),
            // x2 codex >24h：兜底降级
            (
                "x2",
                "codex",
                "2026-09-01T04:00:00Z",
                None,
                Some("2026-09-01T05:00:00Z"),
                Some(90_000_000),
                Some("codex_turn_to_token_count"),
                Some("observed"),
            ),
            // g3 opencode：fr→completed 跨度 7h（>6h 残差，回填污染/挂死流）→ 置空
            (
                "g3",
                "opencode",
                "2026-09-01T05:00:00Z",
                Some("2026-09-01T06:00:00Z"),
                Some("2026-09-01T13:00:00Z"),
                Some(28_800_000),
                Some("opencode_message_timestamps"),
                Some("observed"),
            ),
        ];
        for (id, client, started, first, completed, duration, source, quality) in rows {
            conn.execute(
                insert,
                rusqlite::params![id, client, started, first, completed, duration, source, quality],
            )
            .unwrap();
        }

        let applied = migrate_embedded(&mut conn, Some(24)).unwrap();
        assert_eq!(applied, vec![24]);

        let read = |id: &str| -> (
            String,
            Option<String>,
            Option<i64>,
            Option<String>,
            Option<String>,
        ) {
            conn.query_row(
                "SELECT started_at, first_response_at, duration_ms, timing_source, timing_quality
                 FROM model_calls WHERE id = ?1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
            )
            .unwrap()
        };

        // g1：started 改为 fr，时长重算为 10 分钟
        let (started, first, duration, _source, quality) = read("g1");
        assert_eq!(
            started, "2026-09-01T00:00:00Z",
            "started 应改为请求起点代理"
        );
        assert_eq!(first, Some("2026-09-01T00:00:00Z".to_string()), "fr 应保留");
        assert_eq!(duration, Some(600_000), "时长应重算为 completed - fr");
        assert_eq!(quality.as_deref(), Some("observed"));

        // g2：无法重算必须置空，started 不动
        let (started, _, duration, _, quality) = read("g2");
        assert_eq!(started, "2026-09-01T01:00:00Z");
        assert_eq!(duration, None, "无法重算必须置空，禁止保留 turn 跨度");
        assert_eq!(quality.as_deref(), Some("unavailable"));

        // c1：turn 跨度置空 + source 统一为 legacy_unavailable
        let (started, _, duration, source, quality) = read("c1");
        assert_eq!(
            started, "2026-09-01T02:00:00Z",
            "claude started_at 不在本次范围"
        );
        assert_eq!(duration, None);
        assert_eq!(source.as_deref(), Some("legacy_unavailable"));
        assert_eq!(quality.as_deref(), Some("unavailable"));

        // x1：codex 正常行完全不变（含 Token）
        let (started, first, duration, source, quality) = read("x1");
        assert_eq!(started, "2026-09-01T03:00:00Z");
        assert_eq!(first, Some("2026-09-01T03:00:30Z".to_string()));
        assert_eq!(duration, Some(60_000), "codex 正常行时长不得被改写");
        assert_eq!(source.as_deref(), Some("codex_turn_to_token_count"));
        assert_eq!(quality.as_deref(), Some("observed"));
        let tokens: i64 = conn
            .query_row(
                "SELECT output_tokens FROM model_calls WHERE id = 'x1'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(tokens, 700, "Token 不受时长修复影响");

        // x2：>24h 兜底置空，source 保持原值
        let (started, _, duration, source, quality) = read("x2");
        assert_eq!(started, "2026-09-01T04:00:00Z");
        assert_eq!(duration, None, ">24h 必须降级");
        assert_eq!(source.as_deref(), Some("codex_turn_to_token_count"));
        assert_eq!(quality.as_deref(), Some("unavailable"));

        // g3：重算后仍 >6h 的 opencode 行 → 置空，started 采用 fr（只影响计数分桶）
        let (started, _, duration, _, quality) = read("g3");
        assert_eq!(started, "2026-09-01T06:00:00Z");
        assert_eq!(
            duration, None,
            ">6h 残差必须降级，修复后全库 duration>6h 应为 0"
        );
        assert_eq!(quality.as_deref(), Some("unavailable"));

        for suffix in ["", "-wal", "-shm"] {
            let _ = std::fs::remove_file(format!("{}{suffix}", path.display()));
        }
    }
}
