//! 前端静态资源：编译期嵌入 `web/dist`，运行时无 Node.js。

use rust_embed::RustEmbed;

#[derive(Debug, RustEmbed)]
#[folder = "../../web/dist/"]
pub struct Assets;

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

/// 定位静态资源：`/static/<path>` 优先精确匹配，其次 SPA fallback 到 index.html。
pub fn lookup(static_path: &str) -> Option<(&'static str, Vec<u8>)> {
    if let Some(f) = Assets::get(static_path) {
        return Some((guess_mime(static_path), f.data.into_owned()));
    }
    // SPA fallback：非资源路径均返回 index.html，由前端路由接管。
    if let Some(idx) = Assets::get("index.html") {
        return Some(("text/html; charset=utf-8", idx.data.into_owned()));
    }
    None
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
}
