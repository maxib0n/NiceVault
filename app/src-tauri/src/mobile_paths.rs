use std::path::{Component, Path, PathBuf};

pub fn uri_name(uri: &str) -> String {
    let last = uri.split(['?', '#']).next().unwrap_or(uri).rsplit('/').next().unwrap_or(uri);
    let b = last.as_bytes();
    let mut bytes = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        let hex = |c: u8| (c as char).to_digit(16);
        if b[i] == b'%'
            && i + 2 < b.len()
            && let (Some(h), Some(l)) = (hex(b[i + 1]), hex(b[i + 2]))
        {
            bytes.push((h * 16 + l) as u8);
            i += 3;
        } else {
            bytes.push(b[i]);
            i += 1;
        }
    }
    let decoded = String::from_utf8_lossy(&bytes).into_owned();
    clean_name(decoded.rsplit(['/', ':']).next().unwrap_or(""))
}

pub fn clean_name(n: &str) -> String {
    let n: String = n.trim().chars().map(|c| if c == '/' || c == '\\' || c.is_control() { '_' } else { c }).collect();
    if n.is_empty() || n == "." || n == ".." { "file".to_owned() } else { n }
}

pub fn staging_of(root: &Path, p: &Path) -> Option<PathBuf> {
    let rest = p.strip_prefix(root).ok()?;
    match rest.components().next()? {
        Component::Normal(first) => Some(root.join(first)),
        _ => None,
    }
}

pub fn volume_name(name: &str) -> String {
    if name.to_ascii_lowercase().ends_with(".nvault") { name.to_owned() } else { format!("{name}.nvault") }
}

pub fn unique_in(dir: &Path, name: &str) -> PathBuf {
    let (stem, ext) = match name.rfind('.') {
        Some(i) if i > 0 => (&name[..i], &name[i..]),
        _ => (name, ""),
    };
    (1..)
        .map(|n| dir.join(if n == 1 { name.to_owned() } else { format!("{stem} ({n}){ext}") }))
        .find(|p| !p.exists())
        .expect("an unused name")
}

pub fn rebase_container(p: &Path, home: &Path) -> Option<PathBuf> {
    const MARK: &str = "/Data/Application/";
    let split = |s: &str| -> Option<(usize, usize)> {
        let at = s.find(MARK)? + MARK.len();
        let uuid_end = at + s[at..].find('/').unwrap_or(s.len() - at);
        (uuid_end > at).then_some((at, uuid_end))
    };
    let (ps, hs) = (p.to_str()?, home.to_str()?);
    let (p_at, p_end) = split(ps)?;
    let (h_at, h_end) = split(hs)?;
    let rest = &ps[p_end..];
    if rest.is_empty() || ps[p_at..p_end] == hs[h_at..h_end] {
        return None;
    }
    Some(PathBuf::from(format!("{}{}", &hs[..h_end], rest)))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Media {
    Image,
    Video,
}

pub fn gallery_kind(name: &str, ios: bool) -> Option<Media> {
    let ext = name.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()).unwrap_or_default();
    match ext.as_str() {
        "jpg" | "jpeg" | "jfif" | "png" | "gif" | "heic" | "heif" | "webp" | "tif" | "tiff" | "dng" | "bmp" => Some(Media::Image),
        "avif" if !ios => Some(Media::Image),
        "mp4" | "m4v" | "mov" | "3gp" => Some(Media::Video),
        "webm" | "mkv" if !ios => Some(Media::Video),
        _ => None,
    }
}

pub fn doc_mime(name: &str) -> String {
    let ext = name.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()).unwrap_or_default();
    match ext.as_str() {
        "html" | "htm" => "text/html".into(),
        "heif" => "image/heif".into(),
        "dng" => "image/x-adobe-dng".into(),
        "3gp" => "video/3gpp".into(),
        _ => crate::preview::mime_for(name).split(';').next().unwrap_or("application/octet-stream").trim().to_owned(),
    }
}

pub fn doc_id(vault: &str, node: u64) -> String {
    format!("{vault}/{node}")
}

pub fn parse_doc_id(doc: &str) -> Option<(&str, u64)> {
    let (v, n) = doc.rsplit_once('/')?;
    (!v.is_empty() && !v.contains('/')).then_some(())?;
    Some((v, n.parse().ok()?))
}

pub fn open_mode(mode: &str) -> Option<(bool, bool)> {
    match mode {
        "r" => Some((false, false)),
        "w" | "wt" | "rwt" => Some((true, true)),
        "wa" | "rw" => Some((true, false)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_from_uris() {
        assert_eq!(uri_name("content://com.android.externalstorage.documents/document/primary%3ADownload%2Fphoto.jpg"), "photo.jpg");
        assert_eq!(uri_name("content://media/document/image%3A1000"), "1000");
        assert_eq!(uri_name("file:///private/var/mobile/a%20b.pdf"), "a b.pdf");
        assert_eq!(uri_name("content://x/"), "file");
        assert_eq!(clean_name(" a/b\u{0}.txt "), "a_b_.txt");
        assert_eq!(clean_name(".."), "file");
    }

    #[test]
    fn staging_folders() {
        let r = Path::new("/cache/tmp/nv-import");
        assert_eq!(staging_of(r, &r.join("18a/0/IMG_0001.HEIC")), Some(r.join("18a")));
        assert_eq!(staging_of(r, &r.join("18a")), Some(r.join("18a")));
        assert_eq!(staging_of(r, r), None);
        assert_eq!(staging_of(r, Path::new("/cache/tmp/nv-import-x/a")), None);
        assert_eq!(staging_of(r, Path::new("/var/mobile/Documents/a.pdf")), None);
    }

    #[test]
    fn container_rebase() {
        let home = Path::new("/var/mobile/Containers/Data/Application/NEW-UUID/Library/Application Support/app.nicevault.desktop");
        let old = Path::new("/private/var/mobile/Containers/Data/Application/OLD-UUID/Library/Application Support/app.nicevault.desktop/Volumes/Mio.nvault");
        assert_eq!(
            rebase_container(old, home),
            Some(PathBuf::from("/var/mobile/Containers/Data/Application/NEW-UUID/Library/Application Support/app.nicevault.desktop/Volumes/Mio.nvault"))
        );
        assert_eq!(rebase_container(Path::new("/var/mobile/Containers/Data/Application/NEW-UUID/a.nvault"), home), None);
        assert_eq!(rebase_container(Path::new("/data/user/0/app.nicevault.desktop/Volumes/a.nvault"), home), None);
        assert_eq!(rebase_container(Path::new("/home/user/a.nvault"), home), None);
        assert_eq!(rebase_container(Path::new("/var/mobile/Containers/Data/Application/OLD"), home), None);
        assert_eq!(rebase_container(old, Path::new("/data/user/0/x")), None);
    }

    #[test]
    fn volume_names() {
        assert_eq!(volume_name("Fisco"), "Fisco.nvault");
        assert_eq!(volume_name("Fisco.NVAULT"), "Fisco.NVAULT");
        let d = std::env::temp_dir().join(format!("nv-unique-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("A.nvault"), b"").unwrap();
        assert_eq!(unique_in(&d, "A.nvault"), d.join("A (2).nvault"));
        assert_eq!(unique_in(&d, "B.nvault"), d.join("B.nvault"));
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn gallery_kinds() {
        assert_eq!(gallery_kind("IMG_1.HEIC", true), Some(Media::Image));
        assert_eq!(gallery_kind("a.jpeg", false), Some(Media::Image));
        assert_eq!(gallery_kind("v.MOV", true), Some(Media::Video));
        assert_eq!(gallery_kind("v.webm", true), None, "Photos does not import WebM");
        assert_eq!(gallery_kind("v.webm", false), Some(Media::Video));
        assert_eq!(gallery_kind("x.avif", true), None);
        assert_eq!(gallery_kind("doc.pdf", false), None);
        assert_eq!(gallery_kind("noext", false), None);
    }

    #[test]
    fn provider_ids_and_modes() {
        assert_eq!(doc_id("a1b2", 7), "a1b2/7");
        assert_eq!(parse_doc_id("a1b2/7"), Some(("a1b2", 7)));
        assert_eq!(parse_doc_id("a1b2/x"), None);
        assert_eq!(parse_doc_id("/7"), None);
        assert_eq!(parse_doc_id("a/b/7"), None);
        assert_eq!(open_mode("r"), Some((false, false)));
        assert_eq!(open_mode("w"), Some((true, true)));
        assert_eq!(open_mode("rw"), Some((true, false)));
        assert_eq!(open_mode("x"), None);
        assert_eq!(doc_mime("a.txt"), "text/plain");
        assert_eq!(doc_mime("a.html"), "text/html");
        assert_eq!(doc_mime("a.JPG"), "image/jpeg");
        assert_eq!(doc_mime("a.bin"), "application/octet-stream");
    }
}
