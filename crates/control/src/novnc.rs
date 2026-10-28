use axum::extract::Path;
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};

const FILES: &[(&str, &[u8])] = &[
    ("AUTHORS", include_bytes!("../assets/novnc/AUTHORS")),
    ("LICENSE.MPL-2.0", include_bytes!("../assets/novnc/LICENSE.MPL-2.0")),
    ("LICENSE.txt", include_bytes!("../assets/novnc/LICENSE.txt")),
    ("NOTICE.md", include_bytes!("../assets/novnc/NOTICE.md")),
    ("core/base64.js", include_bytes!("../assets/novnc/core/base64.js")),
    ("core/crypto/aes.js", include_bytes!("../assets/novnc/core/crypto/aes.js")),
    ("core/crypto/bigint.js", include_bytes!("../assets/novnc/core/crypto/bigint.js")),
    ("core/crypto/crypto.js", include_bytes!("../assets/novnc/core/crypto/crypto.js")),
    ("core/crypto/des.js", include_bytes!("../assets/novnc/core/crypto/des.js")),
    ("core/crypto/dh.js", include_bytes!("../assets/novnc/core/crypto/dh.js")),
    ("core/crypto/md5.js", include_bytes!("../assets/novnc/core/crypto/md5.js")),
    ("core/crypto/rsa.js", include_bytes!("../assets/novnc/core/crypto/rsa.js")),
    ("core/decoders/copyrect.js", include_bytes!("../assets/novnc/core/decoders/copyrect.js")),
    ("core/decoders/h264.js", include_bytes!("../assets/novnc/core/decoders/h264.js")),
    ("core/decoders/hextile.js", include_bytes!("../assets/novnc/core/decoders/hextile.js")),
    ("core/decoders/jpeg.js", include_bytes!("../assets/novnc/core/decoders/jpeg.js")),
    ("core/decoders/raw.js", include_bytes!("../assets/novnc/core/decoders/raw.js")),
    ("core/decoders/rre.js", include_bytes!("../assets/novnc/core/decoders/rre.js")),
    ("core/decoders/tight.js", include_bytes!("../assets/novnc/core/decoders/tight.js")),
    ("core/decoders/tightpng.js", include_bytes!("../assets/novnc/core/decoders/tightpng.js")),
    ("core/decoders/zlib.js", include_bytes!("../assets/novnc/core/decoders/zlib.js")),
    ("core/decoders/zrle.js", include_bytes!("../assets/novnc/core/decoders/zrle.js")),
    ("core/deflator.js", include_bytes!("../assets/novnc/core/deflator.js")),
    ("core/display.js", include_bytes!("../assets/novnc/core/display.js")),
    ("core/encodings.js", include_bytes!("../assets/novnc/core/encodings.js")),
    ("core/inflator.js", include_bytes!("../assets/novnc/core/inflator.js")),
    ("core/input/domkeytable.js", include_bytes!("../assets/novnc/core/input/domkeytable.js")),
    ("core/input/fixedkeys.js", include_bytes!("../assets/novnc/core/input/fixedkeys.js")),
    ("core/input/gesturehandler.js", include_bytes!("../assets/novnc/core/input/gesturehandler.js")),
    ("core/input/keyboard.js", include_bytes!("../assets/novnc/core/input/keyboard.js")),
    ("core/input/keysym.js", include_bytes!("../assets/novnc/core/input/keysym.js")),
    ("core/input/keysymdef.js", include_bytes!("../assets/novnc/core/input/keysymdef.js")),
    ("core/input/util.js", include_bytes!("../assets/novnc/core/input/util.js")),
    ("core/input/vkeys.js", include_bytes!("../assets/novnc/core/input/vkeys.js")),
    ("core/input/xtscancodes.js", include_bytes!("../assets/novnc/core/input/xtscancodes.js")),
    ("core/ra2.js", include_bytes!("../assets/novnc/core/ra2.js")),
    ("core/rfb.js", include_bytes!("../assets/novnc/core/rfb.js")),
    ("core/util/browser.js", include_bytes!("../assets/novnc/core/util/browser.js")),
    ("core/util/cursor.js", include_bytes!("../assets/novnc/core/util/cursor.js")),
    ("core/util/element.js", include_bytes!("../assets/novnc/core/util/element.js")),
    ("core/util/events.js", include_bytes!("../assets/novnc/core/util/events.js")),
    ("core/util/eventtarget.js", include_bytes!("../assets/novnc/core/util/eventtarget.js")),
    ("core/util/int.js", include_bytes!("../assets/novnc/core/util/int.js")),
    ("core/util/logging.js", include_bytes!("../assets/novnc/core/util/logging.js")),
    ("core/util/strings.js", include_bytes!("../assets/novnc/core/util/strings.js")),
    ("core/websock.js", include_bytes!("../assets/novnc/core/websock.js")),
    ("embed.html", include_bytes!("../assets/novnc/embed.html")),
    ("vendor/pako/LICENSE", include_bytes!("../assets/novnc/vendor/pako/LICENSE")),
    ("vendor/pako/lib/utils/common.js", include_bytes!("../assets/novnc/vendor/pako/lib/utils/common.js")),
    ("vendor/pako/lib/zlib/adler32.js", include_bytes!("../assets/novnc/vendor/pako/lib/zlib/adler32.js")),
    ("vendor/pako/lib/zlib/constants.js", include_bytes!("../assets/novnc/vendor/pako/lib/zlib/constants.js")),
    ("vendor/pako/lib/zlib/crc32.js", include_bytes!("../assets/novnc/vendor/pako/lib/zlib/crc32.js")),
    ("vendor/pako/lib/zlib/deflate.js", include_bytes!("../assets/novnc/vendor/pako/lib/zlib/deflate.js")),
    ("vendor/pako/lib/zlib/gzheader.js", include_bytes!("../assets/novnc/vendor/pako/lib/zlib/gzheader.js")),
    ("vendor/pako/lib/zlib/inffast.js", include_bytes!("../assets/novnc/vendor/pako/lib/zlib/inffast.js")),
    ("vendor/pako/lib/zlib/inflate.js", include_bytes!("../assets/novnc/vendor/pako/lib/zlib/inflate.js")),
    ("vendor/pako/lib/zlib/inftrees.js", include_bytes!("../assets/novnc/vendor/pako/lib/zlib/inftrees.js")),
    ("vendor/pako/lib/zlib/messages.js", include_bytes!("../assets/novnc/vendor/pako/lib/zlib/messages.js")),
    ("vendor/pako/lib/zlib/trees.js", include_bytes!("../assets/novnc/vendor/pako/lib/zlib/trees.js")),
    ("vendor/pako/lib/zlib/zstream.js", include_bytes!("../assets/novnc/vendor/pako/lib/zlib/zstream.js")),
];

fn content_type(path: &str) -> &'static str {
    match path.rsplit_once('.').map(|(_, extension)| extension) {
        Some("js") => "text/javascript",
        Some("html") => "text/html; charset=utf-8",
        _ => "text/plain; charset=utf-8",
    }
}

pub async fn serve(Path(path): Path<String>) -> Response {
    match FILES.iter().find(|(name, _)| *name == path) {
        Some((name, body)) => ([(header::CONTENT_TYPE, content_type(name)), (header::CACHE_CONTROL, "public, max-age=3600")], *body).into_response(),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}
