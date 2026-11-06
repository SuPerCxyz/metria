//! 前端静态资源：编译期嵌入 `web/dist`，运行时无 Node.js。

use rust_embed::RustEmbed;

#[derive(Debug, RustEmbed)]
#[folder = "../../web/dist/"]
pub struct Assets;

/// 已解析的静态资源。
#[derive(Debug)]
pub struct StaticAsset {
    pub mime: &'static str,
    pub bytes: Vec<u8>,
    /// 实际命中的嵌入路径，用于决定缓存策略。
    pub path: String,
    /// SPA fallback 返回的 index.html。
    pub is_index: bool,
}

/// 根据扩展名猜测 MIME 类型（资源有限，仅覆盖常见类型）。
pub fn guess_mime(path: &str) -> &'static str {
    let ext = path.rsplit('.').next().unwrap_or("");
    match ext {
        "html" => "text/html; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "js" | "mjs" => "application/javascript; charset=utf-8",
        "json" => "application/json; charset=utf-8",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "ico" => "image/x-icon",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        "ttf" => "font/ttf",
        "webmanifest" => "application/manifest+json",
        "map" => "application/json",
        _ => "application/octet-stream",
    }
}

/// 旧版 CSS 可能仍请求 `/static/static/fonts/MapleMono-Medium.woff2`。
/// 保留一个版本的兼容映射，避免旧缓存 CSS 在发布后 404。
const LEGACY_FONT_PATHS: &[&str] = &[
    "static/fonts/MapleMono-Medium.woff2",
    "static/fonts/MapleMono-Medium-subset.woff2",
    "fonts/MapleMono-Medium.woff2",
];

const SUBSET_FONT_PATH: &str = "fonts/MapleMono-Medium-subset.woff2";

fn embedded(path: &str) -> Option<StaticAsset> {
    let file = Assets::get(path)?;
    Some(StaticAsset {
        mime: guess_mime(path),
        bytes: file.data.into_owned(),
        path: path.to_string(),
        is_index: false,
    })
}

/// 定位静态资源：`/static/<path>` 优先精确匹配，其次 SPA fallback 到 index.html。
pub fn lookup(static_path: &str) -> Option<StaticAsset> {
    if let Some(asset) = embedded(static_path) {
        return Some(asset);
    }
    if LEGACY_FONT_PATHS.contains(&static_path) {
        if let Some(asset) = embedded(SUBSET_FONT_PATH) {
            return Some(asset);
        }
    }
    // 兼容旧的 public/static 输出（历史版本曾生成 dist/static/*）。
    let nested_path = format!("static/{static_path}");
    if let Some(asset) = embedded(&nested_path) {
        return Some(asset);
    }
    // SPA fallback：非资源路径均返回 index.html，由前端路由接管。
    let file = Assets::get("index.html")?;
    Some(StaticAsset {
        mime: "text/html; charset=utf-8",
        bytes: file.data.into_owned(),
        path: "index.html".to_string(),
        is_index: true,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mime_guess() {
        assert!(guess_mime("index.html").starts_with("text/html"));
        assert!(guess_mime("app.js").contains("javascript"));
        assert_eq!(guess_mime("x.unknown"), "application/octet-stream");
    }

    #[test]
    fn embed_has_index() {
        // web/dist 必须在 cargo build 之前构建，否则该测试失败。
        assert!(Assets::get("index.html").is_some());
    }

    #[test]
    fn legacy_font_path_maps_to_subset() {
        let asset = lookup("static/fonts/MapleMono-Medium.woff2").expect("legacy font");
        assert_eq!(asset.path, SUBSET_FONT_PATH);
        assert_eq!(asset.mime, "font/woff2");
    }
}
