//! Fixture 测试工具：golden / malformed / schema drift 的统一跑法。

use std::path::{Path, PathBuf};

use metria_core::model::{SourceCursor, SourceStatus};

use crate::error::ScanTolerance;
use crate::types::{
    DiscoveredSource, DiscoveryContext, ScanBatch, ScanIdentity, SourceAdapter, SourceHealth,
};

/// 测试结果汇总。
#[derive(Debug)]
pub struct ScanSummary {
    pub batch: ScanBatch,
    pub tolerance: ScanTolerance,
    pub new_cursor: Option<SourceCursor>,
    pub health: SourceHealth,
}

/// 对 fixture 目录中的某个子目录执行「发现 → 健康 → 扫描」完整链路。
pub fn scan_fixture(
    adapter: &dyn SourceAdapter,
    fixture_root: &Path,
    source_subpath: &str,
) -> ScanSummary {
    let root = fixture_root.join(source_subpath);
    assert!(root.exists(), "fixture 目录不存在: {}", root.display());
    let ctx = DiscoveryContext {
        node_id: "test-node".into(),
        collector_id: "test-collector".into(),
        root_paths: vec![fixture_root.to_path_buf()],
    };
    let discovered = adapter.discover(&ctx).unwrap_or_else(|e| {
        panic!("discover 失败 ({}) : {e}", adapter.id());
    });
    // 找到匹配子路径的来源
    let canonical: PathBuf = root.canonicalize().unwrap_or(root);
    let source = discovered
        .iter()
        .find(|s| s.canonical_path == canonical)
        .cloned()
        .unwrap_or_else(|| {
            panic!(
                "未发现匹配来源 {}（候选: {:?}）",
                canonical.display(),
                discovered
                    .iter()
                    .map(|s| s.canonical_path.display().to_string())
                    .collect::<Vec<_>>()
            )
        });
    scan_source(adapter, &source)
}

/// 对单个已构造来源执行健康与扫描。
pub fn scan_source(adapter: &dyn SourceAdapter, source: &DiscoveredSource) -> ScanSummary {
    let health = adapter
        .health(source)
        .unwrap_or_else(|e| panic!("health 失败 ({}): {e}", adapter.id()));
    let batch = adapter
        .scan(source, None, &ScanIdentity::test())
        .unwrap_or_else(|e| panic!("scan 失败 ({}): {e}", adapter.id()));
    ScanSummary {
        new_cursor: batch.next_cursor.clone(),
        tolerance: ScanTolerance {
            recovered: batch.usage_events.len() as u64,
            skipped: batch.warnings.len() as u64,
            warnings: batch.warnings.clone(),
        },
        batch,
        health,
    }
}

/// 断言 golden 基本健康：健康通过、有 usage 事件、无致命错误。
pub fn assert_golden_basics(summary: &ScanSummary) {
    assert!(summary.health.ok, "来源应健康: {:?}", summary.health);
    assert!(
        !summary.batch.usage_events.is_empty(),
        "golden fixture 应产生 usage 事件"
    );
    assert!(
        summary
            .batch
            .source_errors
            .iter()
            .all(|e| e.severity != "fatal"),
        "golden fixture 不应有 fatal 错误"
    );
}

/// 断言 malformed 处理符合预期：不 panic、有警告、坏记录被跳过但正常记录仍解析。
pub fn assert_malformed_tolerant(summary: &ScanSummary, expect_warnings: bool) {
    if expect_warnings {
        assert!(
            !summary.batch.warnings.is_empty(),
            "malformed fixture 应产生解析警告"
        );
    }
    // 坏记录不得中断整体解析
    assert!(
        summary
            .batch
            .source_errors
            .iter()
            .all(|e| e.severity != "fatal"),
        "malformed 不应出现 fatal 错误（应警告+continue）"
    );
}

/// 断言来源状态。
pub fn assert_source_status(summary: &ScanSummary, status: SourceStatus) {
    assert_eq!(summary.health.status, status);
}
