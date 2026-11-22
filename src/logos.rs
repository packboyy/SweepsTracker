//! Image loader for casino logos (any http/https image) with a disk cache, so logos show
//! instantly on startup and work offline after the first download.
//!
//! Registered after egui_extras' loaders; egui tries the most recently added loader first.
//! Files live in `<app folder>/logos/`, named by a hash of the URL.

use crate::model::app_dir;
use eframe::egui::{
    self,
    load::{Bytes, BytesLoadResult, BytesLoader, BytesPoll, LoadError},
};
use std::collections::HashMap;
use std::fs;
use std::io::Read;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

/// Logos are small; refuse anything absurd.
const MAX_BYTES: u64 = 5 * 1024 * 1024;

#[derive(Clone)]
enum Entry {
    Pending,
    Ready(Arc<[u8]>),
    Failed(String),
}

pub struct LogoLoader {
    dir: PathBuf,
    cache: Arc<Mutex<HashMap<String, Entry>>>,
}

impl LogoLoader {
    pub fn install(ctx: &egui::Context) {
        ctx.add_bytes_loader(Arc::new(LogoLoader {
            dir: app_dir().join("logos"),
            cache: Arc::default(),
        }));
    }

    fn file_for(&self, uri: &str) -> PathBuf {
        self.dir.join(format!("{:016x}", fnv1a(uri)))
    }
}

/// Stable 64-bit FNV-1a hash (std's hasher may change between Rust releases, which would
/// orphan the cache).
fn fnv1a(s: &str) -> u64 {
    s.bytes().fold(0xcbf2_9ce4_8422_2325, |h, b| {
        (h ^ b as u64).wrapping_mul(0x0100_0000_01b3)
    })
}

/// The URLs have no file extension (e.g. an image proxy's query string), so tell the image
/// decoder what the bytes are by sniffing them.
fn mime(bytes: &[u8]) -> Option<String> {
    image::guess_format(bytes)
        .ok()
        .map(|f| f.to_mime_type().to_string())
}

fn ready(bytes: &Arc<[u8]>) -> BytesPoll {
    BytesPoll::Ready {
        size: None,
        mime: mime(bytes),
        bytes: Bytes::Shared(bytes.clone()),
    }
}

fn download(uri: &str) -> Result<Vec<u8>, String> {
    let response = ureq::get(uri)
        .set(
            "User-Agent",
            "SweepsTracker/0.1 (personal spending tracker)",
        )
        .call()
        .map_err(|e| e.to_string())?;
    let mut bytes = Vec::new();
    response
        .into_reader()
        .take(MAX_BYTES)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if mime(&bytes).is_none() {
        return Err("not an image".into());
    }
    Ok(bytes)
}

impl BytesLoader for LogoLoader {
    fn id(&self) -> &str {
        egui::generate_loader_id!(LogoLoader)
    }

    fn load(&self, ctx: &egui::Context, uri: &str) -> BytesLoadResult {
        if !(uri.starts_with("https://") || uri.starts_with("http://")) {
            return Err(LoadError::NotSupported);
        }
        let mut cache = self.cache.lock().unwrap();
        match cache.get(uri) {
            Some(Entry::Pending) => return Ok(BytesPoll::Pending { size: None }),
            Some(Entry::Ready(bytes)) => return Ok(ready(bytes)),
            Some(Entry::Failed(e)) => return Err(LoadError::Loading(e.clone())),
            None => {}
        }

        // Saved from an earlier run?
        let path = self.file_for(uri);
        if let Ok(bytes) = fs::read(&path) {
            if mime(&bytes).is_some() {
                let bytes: Arc<[u8]> = bytes.into();
                cache.insert(uri.to_string(), Entry::Ready(bytes.clone()));
                return Ok(ready(&bytes));
            }
        }

        // Download in the background, save to disk, then repaint so it shows up.
        cache.insert(uri.to_string(), Entry::Pending);
        let (uri, cache, dir, ctx) = (
            uri.to_string(),
            self.cache.clone(),
            self.dir.clone(),
            ctx.clone(),
        );
        std::thread::spawn(move || {
            let entry = match download(&uri) {
                Ok(bytes) => {
                    let _ = fs::create_dir_all(&dir);
                    // Temp file + rename: a crash can't leave a half-written logo behind.
                    let tmp = path.with_extension("tmp");
                    if fs::write(&tmp, &bytes).is_ok() {
                        let _ = fs::rename(&tmp, &path);
                    }
                    Entry::Ready(bytes.into())
                }
                Err(e) => Entry::Failed(e),
            };
            cache.lock().unwrap().insert(uri, entry);
            ctx.request_repaint();
        });
        Ok(BytesPoll::Pending { size: None })
    }

    fn forget(&self, uri: &str) {
        self.cache.lock().unwrap().remove(uri);
    }

    fn forget_all(&self) {
        self.cache.lock().unwrap().clear();
    }

    fn byte_size(&self) -> usize {
        self.cache
            .lock()
            .unwrap()
            .values()
            .map(|e| match e {
                Entry::Ready(b) => b.len(),
                _ => 0,
            })
            .sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hashes_are_stable_and_sniffs_image_types() {
        // Fixed value: cache file names must not change between builds.
        assert_eq!(fnv1a("https://example.com/a.png"), 0xfa37_8f65_3a39_84b6);
        assert_eq!(mime(b"\x89PNG\r\n\x1a\n0000").as_deref(), Some("image/png"));
        assert_eq!(mime(b"\xff\xd8\xff\xe0").as_deref(), Some("image/jpeg"));
        assert_eq!(mime(b"RIFF\0\0\0\0WEBPVP8 ").as_deref(), Some("image/webp"));
        assert_eq!(mime(b"<html>"), None);
    }
}
