//! 报告渲染：HTML、纯文本与 Webhook JSON。
//!
//! HTML 采用邮件客户端安全的表格布局 + 内联样式，视觉对齐 Web 端卡片设计。
//! 数据诚实性：缺失口径不显示（不显示为 0）；估算项统一标注「估算」。

use serde_json::json;

use super::aggregate::{DimCount, ReportMetrics};

/// 报告元信息。
#[derive(Debug)]
pub struct ReportMeta {
    pub title: String,
    pub period: String,
    pub timezone: String,
    pub kind: String,
    pub generated_at: String,
}

// 与 Web Chart 调色板一致
const C_INPUT: &str = "#6366f1";
const C_OUTPUT: &str = "#10b981";
const C_CACHE_R: &str = "#f59e0b";
const C_CACHE_W: &str = "#06b6d4";
const C_REASON: &str = "#8b5cf6";
const C_REPORTED: &str = "#6366f1";
const C_CALCULATED: &str = "#10b981";
const C_ESTIMATED: &str = "#f59e0b";

fn usd(micro: i64) -> String {
    format!("${:.4}", micro as f64 / 1_000_000.0)
}

fn bytes_human(b: i64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = 1024.0 * 1024.0;
    const GB: f64 = 1024.0 * 1024.0 * 1024.0;
    let v = b as f64;
    if v >= GB {
        format!("{:.2} GB", v / GB)
    } else if v >= MB {
        format!("{:.2} MB", v / MB)
    } else if v >= KB {
        format!("{:.2} KB", v / KB)
    } else {
        format!("{b} B")
    }
}

fn n(v: i64) -> String {
    let s = v.abs().to_string();
    let mut out = String::new();
    for (i, ch) in s.chars().enumerate() {
        if i > 0 && (s.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(ch);
    }
    if v < 0 {
        format!("-{out}")
    } else {
        out
    }
}

/// 费用口径：仅当大于 0 时返回（缺失不显示）。
fn cost_lines(m: &ReportMetrics) -> Vec<(&'static str, i64, &'static str)> {
    let mut v = Vec::new();
    if m.reported_cost_micro_usd > 0 {
        v.push(("上报费用", m.reported_cost_micro_usd, C_REPORTED));
    }
    if m.calculated_cost_micro_usd > 0 {
        v.push(("计算费用", m.calculated_cost_micro_usd, C_CALCULATED));
    }
    if m.estimated_cost_micro_usd > 0 {
        v.push(("估算费用", m.estimated_cost_micro_usd, C_ESTIMATED));
    }
    v
}

/// 纯文本报告。
pub fn render_text(m: &ReportMetrics, meta: &ReportMeta) -> String {
    let mut s = String::new();
    s.push_str(&format!(
        "{}\n周期：{}\n时区：{}\n\n",
        meta.title, meta.period, meta.timezone
    ));
    if !m.has_data {
        s.push_str("本周期无数据。\n");
        return s;
    }
    s.push_str(&format!(
        "调用：{} 次    会话：{}\n",
        n(m.calls),
        n(m.sessions)
    ));
    s.push_str(&format!(
        "总 Token：{}（输入 {} / 输出 {} / 缓存读 {} / 缓存写 {} / 推理 {}）\n",
        n(m.total_tokens()),
        n(m.input_tokens),
        n(m.output_tokens),
        n(m.cache_read_tokens),
        n(m.cache_write_tokens),
        n(m.reasoning_tokens)
    ));
    if m.total_tokens() == 0 {
        s.push_str("（未采集到 Token 数据）\n");
    }
    let costs = cost_lines(m);
    if costs.is_empty() {
        s.push_str("费用：无数据（未上报且未匹配定价）\n");
    } else {
        let items: Vec<String> = costs
            .iter()
            .map(|(k, v, _)| format!("{k} {}", usd(*v)))
            .collect();
        s.push_str(&format!("费用：{}\n", items.join(" / ")));
    }
    if m.estimated_total_bytes > 0 {
        s.push_str(&format!(
            "估算流量：{}（区间 {} – {}，均为估算）\n",
            bytes_human(m.estimated_total_bytes),
            bytes_human(m.estimated_lower_bound_bytes),
            bytes_human(m.estimated_upper_bound_bytes)
        ));
    }
    if !m.top_models.is_empty() {
        s.push_str("\nTop 模型：\n");
        for d in &m.top_models {
            s.push_str(&format!(
                "  - {}：{} 次 / {} tokens\n",
                d.name,
                n(d.calls),
                n(d.tokens)
            ));
        }
    }
    if !m.top_clients.is_empty() {
        s.push_str("\nTop 客户端：\n");
        for d in &m.top_clients {
            s.push_str(&format!("  - {}：{} 次\n", d.name, n(d.calls)));
        }
    }
    s.push_str(&format!("\n生成时间：{}\n", meta.generated_at));
    s
}

// ---------- HTML ----------

fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn section_title(t: &str) -> String {
    format!(
        "<div style=\"font-size:13px;font-weight:700;color:#374151;margin:22px 0 10px;\">{}</div>",
        esc(t)
    )
}

/// 单段构成条（邮件安全：单行表格 + 内联宽度）。
fn bar(pct: i64, color: &str) -> String {
    let p = pct.clamp(0, 100);
    format!(
        "<table role=\"presentation\" width=\"100%\" cellpadding=\"0\" cellspacing=\"0\" style=\"background:#f3f4f6;border-radius:4px;\"><tr>\
         <td width=\"{p}%\" style=\"background:{color};height:8px;font-size:0;line-height:0;border-radius:4px;\">&nbsp;</td>\
         <td style=\"font-size:0;line-height:0;\">&nbsp;</td></tr></table>"
    )
}

fn kpi_cards(m: &ReportMetrics) -> String {
    let card = |label: &str, value: &str, sub: &str| {
        format!(
            "<td width=\"33.33%\" height=\"96\" valign=\"top\" style=\"padding:0 8px 0 0;\">\
             <div style=\"border:1px solid #e5e7eb;border-radius:12px;padding:14px 16px;background:#ffffff;height:66px;\">\
             <div style=\"font-size:12px;color:#6b7280;\">{label}</div>\
             <div style=\"font-size:22px;font-weight:700;color:#1f2937;margin-top:6px;font-variant-numeric:tabular-nums;\">{value}</div>\
             <div style=\"font-size:11px;color:#9ca3af;margin-top:4px;\">{sub}</div></div></td>"
        )
    };
    let kpis = format!(
        "<table role=\"presentation\" width=\"100%\" cellpadding=\"0\" cellspacing=\"0\"><tr>{}{}{}</tr></table>",
        card("模型调用", &n(m.calls), "次"),
        card("会话", &n(m.sessions), "个"),
        card("总 Token", &n(m.total_tokens()), "全部类型合计"),
    );
    let detail = format!(
        "<div style=\"margin-top:10px;font-size:12px;color:#9ca3af;\">Token 明细：输入 {} · 输出 {} · 缓存读 {} · 缓存写 {} · 推理 {}</div>",
        n(m.input_tokens),
        n(m.output_tokens),
        n(m.cache_read_tokens),
        n(m.cache_write_tokens),
        n(m.reasoning_tokens)
    );
    format!("{kpis}{detail}")
}

fn token_section(m: &ReportMetrics) -> String {
    let total = m.total_tokens();
    if total == 0 {
        return format!(
            "{}<div style=\"font-size:13px;color:#9ca3af;\">未采集到 Token 数据。</div>",
            section_title("Token 构成")
        );
    }
    let seg = |v: i64, color: &str| -> String {
        if v <= 0 {
            return String::new();
        }
        let pct = (v * 100 / total).max(1);
        format!(
            "<td width=\"{pct}%\" style=\"background:{color};height:10px;font-size:0;line-height:0;\">&nbsp;</td>"
        )
    };
    let mut barh = String::from(
        "<table role=\"presentation\" width=\"100%\" cellpadding=\"0\" cellspacing=\"0\" style=\"border-radius:6px;overflow:hidden;\"><tr>",
    );
    barh.push_str(&seg(m.input_tokens, C_INPUT));
    barh.push_str(&seg(m.output_tokens, C_OUTPUT));
    barh.push_str(&seg(m.cache_read_tokens, C_CACHE_R));
    barh.push_str(&seg(m.cache_write_tokens, C_CACHE_W));
    barh.push_str(&seg(m.reasoning_tokens, C_REASON));
    barh.push_str("</tr></table>");

    let legend_row = |label: &str, v: i64, color: &str| -> String {
        if v <= 0 {
            return String::new();
        }
        let pct = v * 100 / total;
        format!(
            "<tr><td style=\"padding:3px 0;font-size:13px;color:#4b5563;\">\
             <span style=\"display:inline-block;width:8px;height:8px;border-radius:2px;background:{color};margin-right:8px;\"></span>{label}\
             </td><td align=\"right\" style=\"padding:3px 0;font-size:13px;color:#1f2937;font-weight:600;font-variant-numeric:tabular-nums;\">{v} <span style=\"color:#9ca3af;font-weight:400;\">({pct}%)</span></td></tr>",
            label = esc(label),
            v = n(v)
        )
    };
    let legend = format!(
        "<table role=\"presentation\" width=\"100%\" cellpadding=\"0\" cellspacing=\"0\" style=\"margin-top:10px;\">{}{}{}{}{}</table>",
        legend_row("输入", m.input_tokens, C_INPUT),
        legend_row("输出", m.output_tokens, C_OUTPUT),
        legend_row("缓存读取", m.cache_read_tokens, C_CACHE_R),
        legend_row("缓存写入", m.cache_write_tokens, C_CACHE_W),
        legend_row("推理", m.reasoning_tokens, C_REASON),
    );
    format!("{}{}{}", section_title("Token 构成"), barh, legend)
}

fn cost_section(m: &ReportMetrics) -> String {
    let mut s = section_title("费用");
    let costs = cost_lines(m);
    if costs.is_empty() {
        s.push_str(
            "<div style=\"font-size:13px;color:#9ca3af;\">无数据（未上报且未匹配定价）。</div>",
        );
        return s;
    }
    s.push_str("<table role=\"presentation\" width=\"100%\" cellpadding=\"0\" cellspacing=\"0\">");
    for (label, v, color) in costs {
        s.push_str(&format!(
            "<tr><td style=\"padding:5px 0;font-size:13px;color:#4b5563;\">\
             <span style=\"display:inline-block;width:8px;height:8px;border-radius:2px;background:{color};margin-right:8px;\"></span>{label}</td>\
             <td align=\"right\" style=\"padding:5px 0;font-size:13px;color:#1f2937;font-weight:600;font-variant-numeric:tabular-nums;\">{}</td></tr>",
            usd(v),
            label = esc(label)
        ));
    }
    s.push_str("</table>");
    s
}

fn traffic_section(m: &ReportMetrics) -> String {
    if m.estimated_total_bytes <= 0 {
        return String::new();
    }
    format!(
        "{}<div style=\"border:1px solid #e5e7eb;border-radius:12px;padding:14px 16px;\">\
         <div style=\"font-size:20px;font-weight:700;color:#1f2937;font-variant-numeric:tabular-nums;\">{total} \
         <span style=\"display:inline-block;font-size:11px;font-weight:600;color:#b45309;background:#fef3c7;border-radius:999px;padding:2px 8px;margin-left:6px;vertical-align:middle;\">估算</span></div>\
         <div style=\"font-size:12px;color:#9ca3af;margin-top:6px;\">区间 {lo} – {hi}（均为估算，非实际网卡流量）</div></div>",
        section_title("估算流量"),
        total = bytes_human(m.estimated_total_bytes),
        lo = bytes_human(m.estimated_lower_bound_bytes),
        hi = bytes_human(m.estimated_upper_bound_bytes),
    )
}

fn top_section(title: &str, items: &[DimCount], show_tokens: bool) -> String {
    if items.is_empty() {
        return String::new();
    }
    let max = items.iter().map(|d| d.calls).max().unwrap_or(1).max(1);
    let palette = [
        C_INPUT, C_OUTPUT, C_CACHE_R, C_CACHE_W, C_REASON, "#ec4899", "#84cc16", "#06b6d4",
    ];
    let mut s = section_title(title);
    s.push_str("<table role=\"presentation\" width=\"100%\" cellpadding=\"0\" cellspacing=\"0\">");
    for (i, d) in items.iter().enumerate() {
        let pct = (d.calls * 100 / max).max(2);
        let value = if show_tokens {
            format!("{} 次 · {} tokens", n(d.calls), n(d.tokens))
        } else {
            format!("{} 次", n(d.calls))
        };
        s.push_str(&format!(
            "<tr><td style=\"padding:7px 0 2px;font-size:13px;color:#374151;\">{name}</td>\
             <td align=\"right\" style=\"padding:7px 0 2px;font-size:12px;color:#6b7280;font-variant-numeric:tabular-nums;\">{value}</td></tr>\
             <tr><td colspan=\"2\" style=\"padding:0 0 6px;\">{bar}</td></tr>",
            name = esc(&d.name),
            value = esc(&value),
            bar = bar(pct, palette[i % palette.len()]),
        ));
    }
    s.push_str("</table>");
    s
}

/// 内联图表（PNG 以 CID 嵌入邮件）。
#[derive(Clone, Debug)]
pub struct ReportChart {
    pub cid: String,
    pub title: String,
    pub png: Vec<u8>,
    pub series: Vec<(String, String)>,
    pub days: Vec<String>,
}

const CHART_W: u32 = 1200;

fn chart_section(c: &ReportChart) -> String {
    let mut s = format!(
        "<div style=\"margin-top:18px;\"><div style=\"font-size:13px;font-weight:700;color:#374151;margin-bottom:8px;\">{}</div>\
         <img src=\"cid:{}\" width=\"{w}\" alt=\"{t}\" style=\"width:100%;max-width:{w}px;height:auto;display:block;\">",
        esc(&c.title),
        esc(&c.cid),
        w = CHART_W,
        t = esc(&c.title),
    );
    // 时间刻度已绘制在图内，这里仅保留多条曲线图例
    if !c.series.is_empty() {
        s.push_str("<div style=\"margin-top:6px;\">");
        for (name, color) in &c.series {
            s.push_str(&format!(
                "<span style=\"display:inline-block;font-size:12px;color:#6b7280;margin-right:14px;\"><span style=\"display:inline-block;width:8px;height:8px;border-radius:2px;background:{color};margin-right:6px;\"></span>{}</span>",
                esc(name)
            ));
        }
        s.push_str("</div>");
    }
    s.push_str("</div>");
    s
}

/// HTML 报告。
pub fn render_html(m: &ReportMetrics, meta: &ReportMeta, charts: &[ReportChart]) -> String {
    let mut h = String::new();
    h.push_str("<!doctype html><html><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width,initial-scale=1\"></head>");
    h.push_str("<body style=\"margin:0;padding:0;background:#f3f4f6;\">");
    h.push_str("<table role=\"presentation\" width=\"100%\" cellpadding=\"0\" cellspacing=\"0\" style=\"background:#f3f4f6;\"><tr><td align=\"center\" style=\"padding:24px 12px;\">");
    h.push_str("<table role=\"presentation\" width=\"1280\" cellpadding=\"0\" cellspacing=\"0\" style=\"width:1280px;max-width:1280px;background:#ffffff;border:1px solid #e5e7eb;border-radius:16px;overflow:hidden;\">");

    // 头部
    h.push_str(&format!(
        "<tr><td style=\"background-color:#4f46e5;background-image:linear-gradient(135deg,#6366f1,#4338ca);padding:24px 28px;\">\
         <div style=\"font-family:-apple-system,'Segoe UI',Roboto,Helvetica,Arial,'PingFang SC','Microsoft YaHei',sans-serif;font-size:12px;letter-spacing:3px;color:#c7d2fe;font-weight:700;\">METRIA</div>\
         <div style=\"font-family:-apple-system,'Segoe UI',Roboto,Helvetica,Arial,'PingFang SC','Microsoft YaHei',sans-serif;font-size:22px;font-weight:700;color:#ffffff;margin-top:8px;\">{}</div>\
         <div style=\"font-size:13px;color:#e0e7ff;margin-top:8px;\">{} · {} · {}</div></td></tr>",
        esc(&meta.title),
        esc(&meta.period),
        esc(&meta.timezone),
        esc(&meta.kind)
    ));

    // 主体
    h.push_str("<tr><td style=\"padding:22px 28px 26px;font-family:-apple-system,'Segoe UI',Roboto,Helvetica,Arial,'PingFang SC','Microsoft YaHei',sans-serif;\">");
    if !m.has_data {
        h.push_str("<div style=\"border:1px dashed #e5e7eb;border-radius:12px;padding:36px 16px;text-align:center;color:#6b7280;font-size:14px;\">本周期无数据。</div>");
    } else {
        h.push_str(&kpi_cards(m));
        for c in charts {
            h.push_str(&chart_section(c));
        }
        h.push_str(&token_section(m));
        h.push_str(&cost_section(m));
        h.push_str(&traffic_section(m));
        h.push_str(&top_section("Top 模型", &m.top_models, true));
        h.push_str(&top_section("Top 客户端", &m.top_clients, false));
    }
    h.push_str("</td></tr>");

    // 页脚
    h.push_str(&format!(
        "<tr><td style=\"padding:16px 28px;background:#f9fafb;border-top:1px solid #e5e7eb;font-family:-apple-system,'Segoe UI',Roboto,Helvetica,Arial,'PingFang SC','Microsoft YaHei',sans-serif;\">\
         <div style=\"font-size:12px;color:#9ca3af;\">生成时间：{}</div>\
         <div style=\"font-size:11px;color:#c0c4cc;margin-top:6px;\">「估算」为估算值；缺失口径不显示，不代表为 0；流量为估算而非实际网卡流量。</div></td></tr>",
        esc(&meta.generated_at)
    ));

    h.push_str("</table></td></tr></table></body></html>");
    h
}

/// Webhook JSON 载荷（与邮件同口径）。
pub fn webhook_payload(m: &ReportMetrics, meta: &ReportMeta) -> serde_json::Value {
    json!({
        "title": meta.title,
        "kind": meta.kind,
        "period": meta.period,
        "timezone": meta.timezone,
        "generated_at": meta.generated_at,
        "has_data": m.has_data,
        "calls": m.calls,
        "sessions": m.sessions,
        "tokens": {
            "input": m.input_tokens,
            "output": m.output_tokens,
            "cache_read": m.cache_read_tokens,
            "cache_write": m.cache_write_tokens,
            "reasoning": m.reasoning_tokens,
            "total": m.total_tokens(),
        },
        "cost_micro_usd": {
            "reported": if m.reported_cost_micro_usd > 0 { Some(m.reported_cost_micro_usd) } else { None },
            "calculated": if m.calculated_cost_micro_usd > 0 { Some(m.calculated_cost_micro_usd) } else { None },
            "estimated": if m.estimated_cost_micro_usd > 0 { Some(m.estimated_cost_micro_usd) } else { None },
        },
        "traffic": {
            "estimated_total_bytes": m.estimated_total_bytes,
            "estimated_lower_bound_bytes": m.estimated_lower_bound_bytes,
            "estimated_upper_bound_bytes": m.estimated_upper_bound_bytes,
            "is_estimate": true,
        },
        "top_models": m.top_models,
        "top_clients": m.top_clients,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::report::aggregate::DimCount;

    fn meta() -> ReportMeta {
        ReportMeta {
            title: "Metria 用量报告".into(),
            period: "2026-09-09（上一自然日）".into(),
            timezone: "Asia/Shanghai".into(),
            kind: "每日".into(),
            generated_at: "2026-09-10T12:00:00+08:00".into(),
        }
    }

    #[test]
    fn no_data_short_message() {
        let m = ReportMetrics::default();
        assert!(render_text(&m, &meta()).contains("本周期无数据"));
        assert!(render_html(&m, &meta(), &[]).contains("本周期无数据"));
    }

    #[test]
    fn missing_costs_not_shown_as_zero() {
        let m = ReportMetrics {
            calls: 3,
            input_tokens: 100,
            has_data: true,
            ..Default::default()
        };
        let t = render_text(&m, &meta());
        assert!(t.contains("费用：无数据"));
        assert!(!t.contains("上报费用 $0"));
    }

    #[test]
    fn estimates_are_labeled() {
        let m = ReportMetrics {
            calls: 2,
            input_tokens: 10,
            has_data: true,
            estimated_total_bytes: 2048,
            estimated_lower_bound_bytes: 1024,
            estimated_upper_bound_bytes: 4096,
            ..Default::default()
        };
        assert!(render_text(&m, &meta()).contains("估算流量"));
        assert!(render_text(&m, &meta()).contains("均为估算"));
        let w = webhook_payload(&m, &meta());
        assert_eq!(w["traffic"]["is_estimate"], true);
        assert!(w["cost_micro_usd"]["reported"].is_null());
    }

    #[test]
    fn html_has_design_and_labels() {
        let m = ReportMetrics {
            calls: 12,
            sessions: 3,
            input_tokens: 1000,
            output_tokens: 200,
            cache_read_tokens: 300,
            has_data: true,
            calculated_cost_micro_usd: 1_500_000,
            estimated_total_bytes: 2_097_152,
            estimated_lower_bound_bytes: 1_048_576,
            estimated_upper_bound_bytes: 3_145_728,
            top_models: vec![DimCount {
                name: "claude-sonnet-4.5".into(),
                calls: 8,
                tokens: 1200,
            }],
            ..Default::default()
        };
        let h = render_html(&m, &meta(), &[]);
        assert!(h.contains("claude-sonnet-4.5"));
        assert!(h.contains("总 Token"));
        assert!(h.contains("#6366f1"));
        assert!(h.contains("估算</span>"));
        assert!(h.contains("计算费用"));
        assert!(!h.contains("上报费用"));
    }
}
