//! 用量报告 PDF 渲染（printpdf + 内嵌 CJK 子集字体）。
//!
//! 直接绘制 A4 页面：头部、KPI、趋势图（复用已渲染的 PNG）、Token 构成、费用、
//! 估算流量、Top 模型/客户端与页脚，内容超出自动分页。字体为编译期子集，无系统
//! 字体依赖；数据诚实性规则与 HTML/纯文本一致（缺失口径不显示、估算项标注「估算」）。
//!
//! 字体子集由 `assets/report-chars.txt` 中的用字经 pyftsubset 生成，修改文案后需
//! 按同样用字重新生成 `report-font.ttf` / `report-font-bold.ttf`；`font_covers_labels`
//! 测试会守护这一点。

use printpdf::path::PaintMode;
use printpdf::*;

use super::aggregate::{DimCount, ReportMetrics};
use super::render::{bytes_human, n, usd, ReportChart, ReportMeta};

const A4_W: f32 = 210.0;
const A4_H: f32 = 297.0;
const MARGIN: f32 = 15.0;
const CONTENT_W: f32 = A4_W - 2.0 * MARGIN;
const BOTTOM: f32 = 16.0;
/// 1pt = 0.352778mm。
const PT_MM: f32 = 0.352_777_8;

static FONT_REGULAR: &[u8] = include_bytes!("../../assets/report-font.ttf");
static FONT_BOLD: &[u8] = include_bytes!("../../assets/report-font-bold.ttf");

const INK: &str = "#1f2937";
const MUTED: &str = "#6b7280";
const FAINT: &str = "#9ca3af";
const BORDER: &str = "#e5e7eb";
const BG: &str = "#f9fafb";
const BRAND: &str = "#4f46e5";

const C_INPUT: &str = "#6366f1";
const C_OUTPUT: &str = "#10b981";
const C_CACHE_R: &str = "#f59e0b";
const C_CACHE_W: &str = "#06b6d4";
const C_REASON: &str = "#8b5cf6";
const DIM_COLORS: [&str; 8] = [
    "#6366f1", "#10b981", "#f59e0b", "#ef4444", "#06b6d4", "#8b5cf6", "#ec4899", "#84cc16",
];

fn rgb(hex: &str) -> Color {
    let h = hex.trim_start_matches('#');
    let c = |i: usize| {
        u8::from_str_radix(h.get(i..i + 2).unwrap_or("00"), 16).unwrap_or(0) as f32 / 255.0
    };
    Color::Rgb(Rgb::new(c(0), c(2), c(4), None))
}

/// 近似字宽（mm）：CJK 记 1em，ASCII 约 0.56em，空格 0.5em。
fn text_width_mm(s: &str, size_pt: f32) -> f32 {
    let ems: f32 = s
        .chars()
        .map(|c| match c {
            ' ' => 0.5,
            c if c.is_ascii() => 0.56,
            _ => 1.0,
        })
        .sum();
    ems * size_pt * PT_MM
}

fn kind_label(kind: &str) -> &str {
    match kind {
        "daily" => "每日",
        "weekly" => "每周",
        "monthly" => "每月",
        "test" => "测试",
        other => other,
    }
}

struct Pdf {
    doc: PdfDocumentReference,
    regular: IndirectFontRef,
    bold: IndirectFontRef,
    page: PdfPageIndex,
    layer: PdfLayerIndex,
    /// 距页底距离（mm），指向下一个元素顶边。
    y: f32,
}

impl Pdf {
    fn new() -> Result<Self, String> {
        let (doc, page, layer) = PdfDocument::new("Metria 用量报告", Mm(A4_W), Mm(A4_H), "报告");
        let regular = doc
            .add_external_font(FONT_REGULAR)
            .map_err(|e| format!("报告字体加载失败: {e}"))?;
        let bold = doc
            .add_external_font(FONT_BOLD)
            .map_err(|e| format!("报告字体加载失败: {e}"))?;
        Ok(Self {
            doc,
            regular,
            bold,
            page,
            layer,
            y: A4_H - MARGIN,
        })
    }

    fn layer(&self) -> PdfLayerReference {
        self.doc.get_page(self.page).get_layer(self.layer)
    }

    fn new_page(&mut self) {
        let (page, layer) = self.doc.add_page(Mm(A4_W), Mm(A4_H), "报告");
        self.page = page;
        self.layer = layer;
        self.y = A4_H - MARGIN;
    }

    /// 若剩余高度不足则换页。
    fn ensure(&mut self, need: f32) {
        if self.y - need < BOTTOM {
            self.new_page();
        }
    }

    fn gap(&mut self, g: f32) {
        self.y -= g;
    }

    /// 在绝对基线位置绘制文本，不影响光标。
    fn text_at(&self, s: &str, size: f32, x: f32, baseline: f32, color: &str, bold: bool) {
        let layer = self.layer();
        layer.set_fill_color(rgb(color));
        let font = if bold { &self.bold } else { &self.regular };
        layer.use_text(s, size, Mm(x), Mm(baseline), font);
    }

    /// 绘制一行左对齐文本并下移光标。
    fn text(&mut self, s: &str, size: f32, color: &str, bold: bool) {
        let mm = size * PT_MM;
        self.text_at(s, size, MARGIN, self.y - mm * 0.82, color, bold);
        self.y -= mm * 1.55;
    }

    fn divider(&mut self, color: &str) {
        self.gap(1.5);
        let layer = self.layer();
        layer.set_fill_color(rgb(color));
        layer.add_rect(
            Rect::new(
                Mm(MARGIN),
                Mm(self.y - 0.3),
                Mm(MARGIN + CONTENT_W),
                Mm(self.y),
            )
            .with_mode(PaintMode::Fill),
        );
        self.y -= 0.3;
    }

    fn section(&mut self, title: &str) {
        self.ensure(14.0);
        self.gap(4.0);
        self.text(title, 12.0, INK, true);
    }

    /// 带色点的数量行：标签左对齐，数值右对齐。
    fn legend_row(&mut self, label: &str, value: &str, color: Option<&str>) {
        self.ensure(6.5);
        let baseline = self.y - 3.0;
        if let Some(c) = color {
            let layer = self.layer();
            layer.set_fill_color(rgb(c));
            layer.add_rect(
                Rect::new(
                    Mm(MARGIN),
                    Mm(baseline - 0.2),
                    Mm(MARGIN + 2.4),
                    Mm(baseline + 2.2),
                )
                .with_mode(PaintMode::Fill),
            );
        }
        let lx = if color.is_some() {
            MARGIN + 4.5
        } else {
            MARGIN
        };
        self.text_at(label, 10.0, lx, baseline, MUTED, false);
        let w = text_width_mm(value, 10.0);
        self.text_at(value, 10.0, A4_W - MARGIN - w, baseline, INK, true);
        self.y -= 6.5;
    }

    fn kpi_cards(&mut self, items: &[(String, String)]) {
        let gap = 6.0;
        let ncols = items.len().max(1) as f32;
        let w = (CONTENT_W - gap * (ncols - 1.0)) / ncols;
        let h = 22.0;
        self.ensure(h + 2.0);
        let top = self.y;
        for (i, (label, value)) in items.iter().enumerate() {
            let x = MARGIN + i as f32 * (w + gap);
            let layer = self.layer();
            layer.set_fill_color(rgb(BG));
            layer.add_rect(
                Rect::new(Mm(x), Mm(top - h), Mm(x + w), Mm(top)).with_mode(PaintMode::Fill),
            );
            layer.set_outline_color(rgb(BORDER));
            layer.set_outline_thickness(0.6);
            layer.add_rect(
                Rect::new(Mm(x), Mm(top - h), Mm(x + w), Mm(top)).with_mode(PaintMode::Stroke),
            );
            self.text_at(label, 9.5, x + 4.0, top - 6.5, MUTED, false);
            self.text_at(value, 17.0, x + 4.0, top - 16.0, INK, true);
        }
        self.y = top - h;
    }

    fn image(&mut self, png: &[u8], target_w: f32) -> Result<f32, String> {
        let dynimg = ::image::load_from_memory(png).map_err(|e| format!("图表解析失败: {e}"))?;
        let px_w = dynimg.width() as f32;
        let px_h = dynimg.height() as f32;
        if px_w <= 0.0 {
            return Err("图表尺寸无效".into());
        }
        let dpi = px_w / (target_w / 25.4);
        let h = px_h / dpi * 25.4;
        let img = Image::from_dynamic_image(&dynimg);
        img.add_to_layer(
            self.layer(),
            ImageTransform {
                translate_x: Some(Mm(MARGIN)),
                translate_y: Some(Mm(self.y - h)),
                dpi: Some(dpi),
                ..Default::default()
            },
        );
        self.y -= h;
        Ok(h)
    }

    fn chart(&mut self, c: &ReportChart) -> Result<(), String> {
        // 标题 + 图 + 图例
        self.ensure(60.0);
        self.gap(4.0);
        self.text_at(&c.title, 11.0, MARGIN, self.y - 3.0, INK, true);
        self.y -= 6.0;
        self.image(&c.png, CONTENT_W)?;
        self.y -= 1.0;
        let baseline = self.y - 3.0;
        let mut x = MARGIN;
        for (name, color) in &c.series {
            let layer = self.layer();
            layer.set_fill_color(rgb(color));
            layer.add_rect(
                Rect::new(Mm(x), Mm(baseline - 0.2), Mm(x + 2.4), Mm(baseline + 2.2))
                    .with_mode(PaintMode::Fill),
            );
            self.text_at(name, 9.0, x + 4.2, baseline, MUTED, false);
            x += 4.2 + text_width_mm(name, 9.0) + 6.0;
        }
        self.y -= 6.0;
        Ok(())
    }
}

/// 费用口径（仅当大于 0 时返回，缺失不显示）。
fn cost_lines(m: &ReportMetrics) -> Vec<(&'static str, i64, &'static str)> {
    let mut v = Vec::new();
    if m.reported_cost_micro_usd > 0 {
        v.push(("上报费用", m.reported_cost_micro_usd, C_INPUT));
    }
    if m.calculated_cost_micro_usd > 0 {
        v.push(("计算费用", m.calculated_cost_micro_usd, C_OUTPUT));
    }
    if m.estimated_cost_micro_usd > 0 {
        v.push(("估算费用", m.estimated_cost_micro_usd, C_CACHE_R));
    }
    v
}

fn token_sections(p: &mut Pdf, m: &ReportMetrics) {
    p.section("Token 构成");
    let total = m.total_tokens();
    if total == 0 {
        p.text("未采集到 Token 数据。", 10.0, FAINT, false);
        return;
    }
    let h = 4.0;
    let top = p.y;
    let layer = p.layer();
    layer.set_fill_color(rgb("#f3f4f6"));
    layer.add_rect(
        Rect::new(Mm(MARGIN), Mm(top - h), Mm(MARGIN + CONTENT_W), Mm(top))
            .with_mode(PaintMode::Fill),
    );
    let segs = [
        (m.input_tokens, C_INPUT),
        (m.output_tokens, C_OUTPUT),
        (m.cache_read_tokens, C_CACHE_R),
        (m.cache_write_tokens, C_CACHE_W),
        (m.reasoning_tokens, C_REASON),
    ];
    let mut x = MARGIN;
    for (v, color) in segs {
        if v <= 0 {
            continue;
        }
        let w = CONTENT_W * v as f32 / total as f32;
        let layer = p.layer();
        layer.set_fill_color(rgb(color));
        layer
            .add_rect(Rect::new(Mm(x), Mm(top - h), Mm(x + w), Mm(top)).with_mode(PaintMode::Fill));
        x += w;
    }
    p.y = top - h;
    p.gap(3.0);
    for (label, v, color) in [
        ("输入", m.input_tokens, C_INPUT),
        ("输出", m.output_tokens, C_OUTPUT),
        ("缓存读取", m.cache_read_tokens, C_CACHE_R),
        ("缓存写入", m.cache_write_tokens, C_CACHE_W),
        ("推理", m.reasoning_tokens, C_REASON),
    ] {
        if v <= 0 {
            continue;
        }
        let pct = v * 100 / total;
        p.legend_row(label, &format!("{} ({}%)", n(v), pct), Some(color));
    }
}

fn cost_section(p: &mut Pdf, m: &ReportMetrics) {
    p.section("费用");
    let costs = cost_lines(m);
    if costs.is_empty() {
        p.text("无数据（未上报且未匹配定价）。", 10.0, FAINT, false);
        return;
    }
    for (label, v, color) in costs {
        p.legend_row(label, &usd(v), Some(color));
    }
}

fn traffic_section(p: &mut Pdf, m: &ReportMetrics) {
    if m.estimated_total_bytes <= 0 {
        return;
    }
    p.section("估算流量");
    let h = 20.0;
    p.ensure(h + 2.0);
    let top = p.y;
    let layer = p.layer();
    layer.set_outline_color(rgb(BORDER));
    layer.set_outline_thickness(0.6);
    layer.add_rect(
        Rect::new(Mm(MARGIN), Mm(top - h), Mm(MARGIN + CONTENT_W), Mm(top))
            .with_mode(PaintMode::Stroke),
    );
    p.text_at(
        &bytes_human(m.estimated_total_bytes),
        20.0,
        MARGIN + 5.0,
        top - 11.0,
        INK,
        true,
    );
    let total_w = text_width_mm(&bytes_human(m.estimated_total_bytes), 20.0);
    p.text_at(
        "估算",
        9.5,
        MARGIN + 7.0 + total_w,
        top - 10.5,
        C_CACHE_R,
        true,
    );
    p.text_at(
        &format!(
            "区间 {} – {}（均为估算，非实际网卡流量）",
            bytes_human(m.estimated_lower_bound_bytes),
            bytes_human(m.estimated_upper_bound_bytes)
        ),
        9.5,
        MARGIN + 5.0,
        top - 16.5,
        FAINT,
        false,
    );
    p.y = top - h;
}

fn top_section(p: &mut Pdf, title: &str, items: &[DimCount], show_tokens: bool) {
    if items.is_empty() {
        return;
    }
    p.section(title);
    let max = items.iter().map(|d| d.calls).max().unwrap_or(1).max(1);
    for (i, d) in items.iter().enumerate() {
        p.ensure(11.0);
        let value = if show_tokens {
            format!("{} 次 · {} tokens", n(d.calls), n(d.tokens))
        } else {
            format!("{} 次", n(d.calls))
        };
        let baseline = p.y - 3.0;
        p.text_at(&d.name, 10.0, MARGIN, baseline, INK, false);
        let w = text_width_mm(&value, 9.5);
        p.text_at(&value, 9.5, A4_W - MARGIN - w, baseline, MUTED, false);
        p.y -= 4.6;
        let color = DIM_COLORS[i % DIM_COLORS.len()];
        let ratio = d.calls as f32 / max as f32;
        let layer = p.layer();
        layer.set_fill_color(rgb(color));
        layer.add_rect(
            Rect::new(
                Mm(MARGIN),
                Mm(p.y - 1.6),
                Mm(MARGIN + CONTENT_W * ratio.max(0.01)),
                Mm(p.y),
            )
            .with_mode(PaintMode::Fill),
        );
        p.y -= 3.4;
    }
}

fn header(p: &mut Pdf, meta: &ReportMeta) {
    p.text_at("METRIA", 9.0, MARGIN, p.y - 3.0, BRAND, true);
    p.y -= 7.0;
    p.text(&meta.title, 21.0, INK, true);
    p.text(
        &format!(
            "{} · {} · {}",
            meta.period,
            meta.timezone,
            kind_label(&meta.kind)
        ),
        10.5,
        MUTED,
        false,
    );
    p.divider(BORDER);
    p.gap(3.0);
}

fn footer(p: &mut Pdf, meta: &ReportMeta) {
    p.divider(BORDER);
    p.gap(2.0);
    p.ensure(14.0);
    p.text(
        &format!("生成时间：{}", meta.generated_at),
        9.0,
        FAINT,
        false,
    );
    p.text(
        "「估算」为估算值；缺失口径不显示，不代表为 0；流量为估算而非实际网卡流量。",
        8.5,
        FAINT,
        false,
    );
}

/// 渲染报告 PDF。
pub fn render_pdf(
    m: &ReportMetrics,
    meta: &ReportMeta,
    charts: &[ReportChart],
) -> Result<Vec<u8>, String> {
    let mut p = Pdf::new()?;
    header(&mut p, meta);

    if !m.has_data {
        p.gap(6.0);
        p.text("本周期无数据。", 12.0, MUTED, false);
        footer(&mut p, meta);
        return p.doc.save_to_bytes().map_err(|e| e.to_string());
    }

    p.kpi_cards(&[
        ("模型调用".into(), n(m.calls)),
        ("会话".into(), n(m.sessions)),
        ("总 Token".into(), n(m.total_tokens())),
    ]);
    p.gap(2.0);
    p.text(
        &format!(
            "Token 明细：输入 {} · 输出 {} · 缓存读 {} · 缓存写 {} · 推理 {}",
            n(m.input_tokens),
            n(m.output_tokens),
            n(m.cache_read_tokens),
            n(m.cache_write_tokens),
            n(m.reasoning_tokens)
        ),
        9.0,
        FAINT,
        false,
    );

    for c in charts {
        if let Err(e) = p.chart(c) {
            tracing::warn!(error = %e, cid = %c.cid, "PDF 图表嵌入失败");
        }
    }

    token_sections(&mut p, m);
    cost_section(&mut p, m);
    traffic_section(&mut p, m);
    top_section(&mut p, "Top 模型", &m.top_models, true);
    top_section(&mut p, "Top 客户端", &m.top_clients, false);

    footer(&mut p, meta);
    p.doc.save_to_bytes().map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn meta() -> ReportMeta {
        ReportMeta {
            title: "Metria 用量报告".into(),
            period: "2026-09-09（上一自然日）".into(),
            timezone: "Asia/Shanghai".into(),
            kind: "daily".into(),
            generated_at: "2026-09-10T12:00:00+08:00".into(),
        }
    }

    fn metrics() -> ReportMetrics {
        ReportMetrics {
            calls: 128,
            sessions: 7,
            input_tokens: 1_200_000,
            output_tokens: 300_000,
            cache_read_tokens: 5_000_000,
            cache_write_tokens: 40_000,
            reasoning_tokens: 12_000,
            calculated_cost_micro_usd: 1_500_000,
            estimated_total_bytes: 2_097_152,
            estimated_lower_bound_bytes: 1_048_576,
            estimated_upper_bound_bytes: 3_145_728,
            top_models: vec![DimCount {
                name: "claude-sonnet-4.5".into(),
                calls: 80,
                tokens: 900_000,
            }],
            top_clients: vec![DimCount {
                name: "Claude Code".into(),
                calls: 80,
                tokens: 0,
            }],
            has_data: true,
            ..Default::default()
        }
    }

    #[test]
    fn renders_pdf_header_and_content() {
        let pdf = render_pdf(&metrics(), &meta(), &[]).expect("pdf");
        assert_eq!(&pdf[..4], b"%PDF");
        assert!(pdf.len() > 3000, "pdf too small: {}", pdf.len());
    }

    #[test]
    fn no_data_pdf() {
        let pdf = render_pdf(&ReportMetrics::default(), &meta(), &[]).expect("pdf");
        assert_eq!(&pdf[..4], b"%PDF");
        assert!(pdf.len() > 1500);
    }

    #[test]
    fn font_covers_fixed_labels() {
        let charset: std::collections::HashSet<char> =
            include_str!("../../assets/report-chars.txt")
                .chars()
                .collect();
        let labels = [
            "Metria 用量报告",
            "模型调用",
            "本周期无数据。",
            "「估算」为估算值；缺失口径不显示，不代表为 0；流量为估算而非实际网卡流量。",
            "Token 明细：输入 · 输出 · 缓存读 · 缓存写 · 推理",
            "Top 客户端",
            "上报费用 / 计算费用 / 估算费用",
        ];
        for l in labels {
            for c in l.chars() {
                assert!(charset.contains(&c), "字体子集缺少字符: {c:?} (in {l})");
            }
        }
    }
}
