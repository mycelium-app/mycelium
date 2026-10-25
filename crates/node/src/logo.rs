use base64::Engine;
use serde_json::{Value, json};
use std::path::Path;

const MOST_LOGO_BYTES: u64 = 256 * 1024;
const README_LINES: usize = 40;
const SVG_PIXELS: f32 = 256.0;
const LOGO_DIRECTORIES: [&str; 8] = ["", "assets", "public", "static", "site", "docs", ".github", "images"];
const LOGO_NAMES: [&str; 5] = ["logo", "icon", "favicon", "logo-icon", "app-icon"];
const LOGO_EXTENSIONS: [&str; 5] = ["png", "webp", "jpg", "jpeg", "svg"];
const LAUNCHER_ICONS: [&str; 6] = [
    "app/src/main/res/mipmap-xxxhdpi/ic_launcher.png",
    "app/src/main/res/mipmap-xxhdpi/ic_launcher.png",
    "app/src/main/res/mipmap-xhdpi/ic_launcher.png",
    "src/main/res/mipmap-xxxhdpi/ic_launcher.png",
    "android/app/src/main/res/mipmap-xxxhdpi/ic_launcher.png",
    "android/app/src/main/res/mipmap-xxhdpi/ic_launcher.png",
];

pub fn candidates(folder: &Path) -> Vec<String> {
    let mut found = Vec::new();

    for directory in LOGO_DIRECTORIES {
        for name in LOGO_NAMES {
            for extension in LOGO_EXTENSIONS {
                found.push(if directory.is_empty() { format!("{name}.{extension}") } else { format!("{directory}/{name}.{extension}") });
            }
        }
    }

    found.extend(LAUNCHER_ICONS.iter().map(|path| path.to_string()));
    found.extend(readme_image(folder));

    found
}

pub fn find(folder: &Path) -> Value {
    for path in candidates(folder) {
        let Some((media_type, bytes)) = load(folder, &path) else {
            continue;
        };

        return json!({"path": path, "media_type": media_type, "data": base64::engine::general_purpose::STANDARD.encode(bytes)});
    }

    Value::Null
}

fn readme_image(folder: &Path) -> Option<String> {
    let readme = std::fs::read_to_string(folder.join("README.md")).ok()?;

    readme.lines().take(README_LINES).find_map(|line| {
        let reference = match line.find("![") {
            Some(start) => line[start..].split_once("](")?.1.split([')', ' ']).next()?,
            None => line.split_once("<img")?.1.split_once("src=")?.1.trim_start_matches(['"', '\'']).split(['"', '\'', ' ', '>']).next()?,
        };

        (!reference.contains("://") && !reference.starts_with("//") && !reference.is_empty()).then(|| reference.trim_start_matches("./").to_string())
    })
}

fn load(folder: &Path, path: &str) -> Option<(&'static str, Vec<u8>)> {
    let root = folder.canonicalize().ok()?;
    let file = root.join(path).canonicalize().ok()?;

    if !file.starts_with(&root) || !file.is_file() || std::fs::metadata(&file).ok()?.len() > MOST_LOGO_BYTES {
        return None;
    }

    let bytes = std::fs::read(&file).ok()?;

    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Some(("image/png", bytes));
    }

    if bytes.starts_with(b"\xff\xd8\xff") {
        return Some(("image/jpeg", bytes));
    }

    if bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WEBP") {
        return Some(("image/webp", bytes));
    }

    render_svg(&bytes).map(|png| ("image/png", png))
}

fn render_svg(bytes: &[u8]) -> Option<Vec<u8>> {
    let tree = resvg::usvg::Tree::from_data(bytes, &resvg::usvg::Options::default()).ok()?;
    let size = tree.size();
    let scale = SVG_PIXELS / size.width().max(size.height());
    let mut pixmap = resvg::tiny_skia::Pixmap::new((size.width() * scale).ceil().max(1.0) as u32, (size.height() * scale).ceil().max(1.0) as u32)?;

    resvg::render(&tree, resvg::tiny_skia::Transform::from_scale(scale, scale), &mut pixmap.as_mut());

    let png = pixmap.encode_png().ok()?;

    (png.len() as u64 <= MOST_LOGO_BYTES).then_some(png)
}

#[cfg(test)]
mod tests {
    use super::*;

    const PNG: &[u8] = b"\x89PNG\r\n\x1a\nbody";
    const SVG: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" width="40" height="20"><rect width="40" height="20" fill="red"/></svg>"#;

    fn folder_with(files: &[(&str, &[u8])]) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();

        for (path, bytes) in files {
            let file = dir.path().join(path);
            std::fs::create_dir_all(file.parent().unwrap()).unwrap();
            std::fs::write(file, bytes).unwrap();
        }

        dir
    }

    #[test]
    fn a_logo_in_the_root_is_found_before_one_in_assets() {
        let dir = folder_with(&[("assets/logo.png", PNG), ("logo.png", PNG)]);

        assert_eq!(find(dir.path())["path"], "logo.png");
    }

    #[test]
    fn a_logo_is_found_before_a_favicon() {
        let dir = folder_with(&[("public/favicon.png", PNG), ("public/logo.png", PNG)]);

        assert_eq!(find(dir.path())["path"], "public/logo.png");
    }

    #[test]
    fn the_first_image_in_the_readme_is_the_last_resort() {
        let dir = folder_with(&[("README.md", b"# App\n\n<p><img src=\"art/banner.png\" width=\"80\"></p>\n"), ("art/banner.png", PNG)]);

        assert_eq!(find(dir.path())["path"], "art/banner.png");
    }

    #[test]
    fn a_remote_readme_image_is_ignored() {
        let dir = folder_with(&[("README.md", b"![x](https://example.com/a.png)\n")]);

        assert_eq!(find(dir.path()), Value::Null);
    }

    #[test]
    fn an_image_over_256_kb_is_skipped_for_the_next_candidate() {
        let mut big = PNG.to_vec();
        big.resize(256 * 1024 + 1, 0);
        let dir = folder_with(&[("logo.png", &big), ("icon.png", PNG)]);

        assert_eq!(find(dir.path())["path"], "icon.png");
    }

    #[test]
    fn a_file_that_is_not_an_image_is_skipped() {
        let dir = folder_with(&[("logo.png", b"not an image"), ("icon.png", PNG)]);

        assert_eq!(find(dir.path())["path"], "icon.png");
    }

    #[test]
    fn an_svg_logo_is_sent_as_a_png() {
        let dir = folder_with(&[("logo.svg", SVG.as_bytes())]);

        let found = find(dir.path());
        let png = base64::engine::general_purpose::STANDARD.decode(found["data"].as_str().unwrap()).unwrap();

        assert_eq!((found["media_type"].clone(), found["path"].clone()), (json!("image/png"), json!("logo.svg")));
        assert!(png.starts_with(b"\x89PNG\r\n\x1a\n"));
    }

    #[test]
    fn a_readme_image_outside_the_folder_is_not_read() {
        let dir = folder_with(&[("README.md", b"![x](../../etc/passwd)\n")]);

        assert_eq!(find(dir.path()), Value::Null);
    }
}
