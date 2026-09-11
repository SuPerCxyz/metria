//! 服务端折线图渲染（PNG）。
//!
//! 使用完整时间轴（缺失桶补 0）、平滑曲线（Catmull-Rom 插值）与 X/Y 轴刻度文字
//! （内嵌子集字体，无系统字体依赖）。中文标题与图例由 HTML 承载，输出内存 PNG。

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::OnceLock;

use plotters::prelude::*;
use plotters::style::{register_font, FontStyle};

use super::render::{chart_theme, format_chart_number};

/// 内嵌子集字体（仅含 0-9 K M . : - 空格），无系统字体依赖。
static FONT: &[u8] = include_bytes!("../../assets/chart-font.ttf");
const FONT_FAMILY: &str = "metria-chart";

fn ensure_font() {
    static DONE: OnceLock<()> = OnceLock::new();
    DONE.get_or_init(|| {
        if register_font(FONT_FAMILY, FontStyle::Normal, FONT).is_err() {
            tracing::warn!("图表字体注册失败");
        }
    });
}

fn unique() -> u64 {
    static N: AtomicU64 = AtomicU64::new(0);
    N.fetch_add(1, Ordering::Relaxed)
}

fn compact(v: f64) -> String {
    format_chart_number(v)
}

/// Catmull-Rom 样条插值，用于把折线平滑为曲线。
fn catmull_rom(pts: &[(f64, f64)], samples: usize) -> Vec<(f64, f64)> {
    if pts.len() < 3 || samples == 0 {
        return pts.to_vec();
    }
    let get = |i: isize| -> (f64, f64) {
        let idx = i.clamp(0, pts.len() as isize - 1) as usize;
        pts[idx]
    };
    let mut out = Vec::with_capacity(pts.len() * samples);
    for i in 0..pts.len() - 1 {
        let p0 = get(i as isize - 1);
        let p1 = pts[i];
        let p2 = pts[i + 1];
        let p3 = get(i as isize + 2);
        for s in 0..samples {
            let t = s as f64 / samples as f64;
            let t2 = t * t;
            let t3 = t2 * t;
            let x = 0.5
                * ((2.0 * p1.0)
                    + (-p0.0 + p2.0) * t
                    + (2.0 * p0.0 - 5.0 * p1.0 + 4.0 * p2.0 - p3.0) * t2
                    + (-p0.0 + 3.0 * p1.0 - 3.0 * p2.0 + p3.0) * t3);
            let y = 0.5
                * ((2.0 * p1.1)
                    + (-p0.1 + p2.1) * t
                    + (2.0 * p0.1 - 5.0 * p1.1 + 4.0 * p2.1 - p3.1) * t2
                    + (-p0.1 + 3.0 * p1.1 - 3.0 * p2.1 + p3.1) * t3);
            out.push((x, y.max(0.0)));
        }
    }
    out.push(*pts.last().unwrap());
    out
}

/// 渲染一张多序列平滑曲线图为 PNG。`labels` 为完整时间轴标签。
pub fn line_chart_png(
    labels: &[String],
    series: &[(RGBColor, Vec<i64>)],
    width: u32,
    height: u32,
    fill: bool,
) -> Result<Vec<u8>, String> {
    let path = std::env::temp_dir().join(format!(
        "metria-chart-{}-{}.png",
        std::process::id(),
        unique()
    ));

    ensure_font();
    let theme = chart_theme();
    {
        let root = BitMapBackend::new(&path, (width, height)).into_drawing_area();
        root.fill(&WHITE).map_err(|e| e.to_string())?;
        let n = labels.len().max(2);
        if !series.is_empty() {
            let ymax = series
                .iter()
                .flat_map(|(_, v)| v.iter())
                .copied()
                .max()
                .unwrap_or(1)
                .max(1) as f64;
            let x_max = (n as f64 - 1.0).max(1.0);
            let mut chart = ChartBuilder::on(&root)
                .margin(8)
                .x_label_area_size(28)
                .y_label_area_size(52)
                .build_cartesian_2d(0f64..x_max, 0f64..ymax)
                .map_err(|e| e.to_string())?;
            let labs = labels.to_vec();
            chart
                .configure_mesh()
                .disable_x_mesh()
                .x_labels(theme.x_max_ticks)
                .x_label_formatter(&|x: &f64| {
                    let r = x.round();
                    if (x - r).abs() > 0.05 {
                        String::new()
                    } else {
                        labs.get(r as usize).cloned().unwrap_or_default()
                    }
                })
                .y_labels(theme.y_tick_count)
                .y_label_formatter(&|y: &f64| compact(*y))
                .label_style(
                    TextStyle::from((FONT_FAMILY, theme.font_size)).color(&RGBColor(156, 163, 175)),
                )
                .axis_style(RGBColor(156, 163, 175))
                .light_line_style(RGBColor(156, 163, 175).mix(0.12))
                .draw()
                .map_err(|e| e.to_string())?;

            for (color, vals) in series {
                let pts = vals
                    .iter()
                    .enumerate()
                    .map(|(i, v)| (i as f64, *v as f64))
                    .collect::<Vec<_>>();
                let line = catmull_rom(&pts, 12);
                if fill {
                    let mut area = line.clone();
                    if let (Some(first), Some(last)) = (line.first(), line.last()) {
                        area.push((last.0, 0.0));
                        area.push((first.0, 0.0));
                    }
                    chart
                        .draw_series(std::iter::once(Polygon::new(
                            area,
                            color.mix(theme.fill_opacity).filled(),
                        )))
                        .map_err(|e| e.to_string())?;
                }
                chart
                    .draw_series(LineSeries::new(
                        line,
                        color.stroke_width(theme.line_width as u32),
                    ))
                    .map_err(|e| e.to_string())?;
            }
        }
        root.present().map_err(|e| e.to_string())?;
    }

    let bytes = std::fs::read(&path).map_err(|e| e.to_string());
    let _ = std::fs::remove_file(&path);
    bytes
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_png_with_axis_text() {
        let labels: Vec<String> = (0..24).map(|h| format!("{h:02}:00")).collect();
        let series = vec![(
            RGBColor(99, 102, 241),
            (0..24).map(|i| (i * 3) as i64).collect(),
        )];
        let png = line_chart_png(&labels, &series, 1200, 300, false).expect("png");
        assert!(png.len() > 500, "png too small: {}", png.len());
        assert_eq!(&png[..4], b"\x89PNG");
    }

    #[test]
    fn compact_numbers() {
        assert_eq!(compact(950.0), "950");
        assert_eq!(compact(1500.0), "1.5K");
        assert_eq!(compact(2_300_000.0), "2.3M");
    }

    #[test]
    fn smooth_interpolation_densifies() {
        let pts = vec![(0.0, 0.0), (1.0, 10.0), (2.0, 0.0)];
        let out = catmull_rom(&pts, 8);
        assert!(out.len() > pts.len());
        assert!(out.iter().all(|(_, y)| *y >= 0.0));
    }
}
