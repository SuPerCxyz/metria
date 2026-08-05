//! JSONL 流式解析工具：按行增量读取、限长、容忍坏记录。
//!
//! 解析器不因单条坏记录中断；未写完的末尾行不消费（游标停在最后完整行末尾）。

use std::fs::File;
use std::io::{BufRead, BufReader, Seek, SeekFrom};
use std::path::Path;

use crate::error::{AdapterError, ScanTolerance};

/// 按行增量扫描 JSONL 文件。
///
/// - `offset`：游标字节偏移（从该处继续读取）。
/// - `max_line_bytes`：单行上限；超限记录跳过并告警。
/// - `on_line`：对每条完整 JSON 行执行（未知字段由调用方自行容忍）。
///
/// 返回新的字节偏移（最后完整行之后）。EOF 处的未写完行不会被消费。
pub fn scan_jsonl_file(
    path: &Path,
    offset: u64,
    max_line_bytes: usize,
    mut on_line: impl FnMut(&serde_json::Value) -> Result<(), AdapterError>,
    tolerance: &mut ScanTolerance,
) -> Result<u64, AdapterError> {
    let file = File::open(path).map_err(|e| AdapterError::NotReadable {
        path: path.display().to_string(),
        source: e,
    })?;
    let mut reader = BufReader::new(file);
    reader
        .seek(SeekFrom::Start(offset))
        .map_err(AdapterError::Io)?;

    let mut pos = offset;
    let mut buf: Vec<u8> = Vec::with_capacity(64 * 1024);
    loop {
        buf.clear();
        let n = reader.read_until(b'\n', &mut buf)?;
        if n == 0 {
            break; // EOF
        }
        if !buf.ends_with(b"\n") {
            // 未写完的末尾行：不消费，等待下次追加完成
            break;
        }
        // 去掉换行符与可能的 \r
        let mut line = &buf[..buf.len() - 1];
        if line.ends_with(b"\r") {
            line = &line[..line.len() - 1];
        }
        pos += n as u64;

        if line.is_empty() {
            continue;
        }
        if line.len() > max_line_bytes {
            tolerance.record(format!(
                "{}: 行超长（{} > {} 字节），跳过",
                path.display(),
                line.len(),
                max_line_bytes
            ));
            continue;
        }
        match serde_json::from_slice::<serde_json::Value>(line) {
            Ok(v) => {
                if let Err(e) = on_line(&v) {
                    tolerance.record(format!("{}: 处理失败: {e}", path.display()));
                }
            }
            Err(e) => {
                tolerance.record(format!(
                    "{}: JSON 解析失败（可能是非 UTF-8 或坏记录）: {e}",
                    path.display()
                ));
            }
        }
    }
    Ok(pos)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn temp_jsonl(tag: &str, lines: &[&str]) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("adapter-parse-{tag}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("test.jsonl");
        let mut f = std::fs::File::create(&p).unwrap();
        for l in lines {
            writeln!(f, "{l}").unwrap();
        }
        p
    }

    #[test]
    fn scans_complete_lines_and_skips_bad() {
        let p = temp_jsonl(
            "good",
            &[
                r#"{"id":1,"ok":true}"#,
                r#"not-json"#,
                r#"{"id":3,"ok":false}"#,
            ],
        );
        let mut tol = ScanTolerance::default();
        let mut ids = Vec::new();
        let new_off = scan_jsonl_file(
            &p,
            0,
            4096,
            |v| {
                ids.push(v["id"].as_i64().unwrap());
                Ok(())
            },
            &mut tol,
        )
        .unwrap();
        assert_eq!(ids, vec![1, 3]);
        assert_eq!(tol.skipped, 1);
        assert!(new_off > 0);
        let _ = std::fs::remove_dir_all(p.parent().unwrap());
    }

    #[test]
    fn incomplete_tail_not_consumed() {
        let p = temp_jsonl("tail", &[r#"{"id":1}"#, r#"{"id":2}"#]);
        // 追加未写完行
        let mut f = std::fs::OpenOptions::new().append(true).open(&p).unwrap();
        write!(f, "{{\"id\":3}}").unwrap();
        drop(f);
        let mut tol = ScanTolerance::default();
        let mut ids = Vec::new();
        let new_off = scan_jsonl_file(
            &p,
            0,
            4096,
            |v| {
                ids.push(v["id"].as_i64().unwrap());
                Ok(())
            },
            &mut tol,
        )
        .unwrap();
        assert_eq!(ids, vec![1, 2]);
        // 游标应停在 id=2 之后，id=3 未消费
        let mut tol2 = ScanTolerance::default();
        let mut ids2 = Vec::new();
        scan_jsonl_file(
            &p,
            new_off,
            4096,
            |v| {
                ids2.push(v["id"].as_i64().unwrap());
                Ok(())
            },
            &mut tol2,
        )
        .unwrap();
        assert_eq!(ids2, Vec::<i64>::new());
        let _ = std::fs::remove_dir_all(p.parent().unwrap());
    }

    #[test]
    fn oversized_line_skipped() {
        let big = format!(r#"{{"s":"{}"}}"#, "x".repeat(200));
        let p = temp_jsonl("big", &[&big]);
        let mut tol = ScanTolerance::default();
        let mut count = 0;
        scan_jsonl_file(
            &p,
            0,
            100,
            |_| {
                count += 1;
                Ok(())
            },
            &mut tol,
        )
        .unwrap();
        assert_eq!(count, 0);
        assert_eq!(tol.skipped, 1);
        let _ = std::fs::remove_dir_all(p.parent().unwrap());
    }
}
