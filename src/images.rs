//! Pictures a reply points at with markdown `![alt](url)`. Rune draws one only once the app
//! hands it an `Image`, and on native it has no HTTP client, so a worker downloads each URL
//! once and Rune's `ImageCache` decodes the bytes.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use rune::{Image, ImageCache, Waker};
use tracing::warn;

const MAX_BYTES: u64 = 10 * 1024 * 1024;
const PLACEHOLDER_SIZE: [u32; 2] = [240, 360];
const TIMEOUT: Duration = Duration::from_secs(15);

pub trait ImageFetch: Send + Sync {
    fn fetch(&self, url: &str) -> Result<Vec<u8>, String>;
}

pub struct HttpImages;

impl ImageFetch for HttpImages {
    fn fetch(&self, url: &str) -> Result<Vec<u8>, String> {
        if !(url.starts_with("https://") || url.starts_with("http://")) {
            return Err("only http and https images load".into());
        }
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .timeout_global(Some(TIMEOUT))
            .build()
            .into();
        let mut response = agent.get(url).call().map_err(|e| e.to_string())?;
        response
            .body_mut()
            .with_config()
            .limit(MAX_BYTES)
            .read_to_vec()
            .map_err(|e| e.to_string())
    }
}

enum Slot {
    Loading,
    Ready { image: Image, shown: bool },
    Failed,
}

struct Inner {
    cache: ImageCache,
    fetcher: Arc<dyn ImageFetch>,
    waker: Waker,
    placeholder: Image,
    placeholder_drawn: AtomicBool,
    slots: Mutex<HashMap<String, Slot>>,
}

#[derive(Clone)]
pub struct ImageStore(Arc<Inner>);

impl ImageStore {
    pub fn new(waker: Waker, fetcher: Arc<dyn ImageFetch>) -> Self {
        let cache = ImageCache::new(waker.clone());
        let [w, h] = PLACEHOLDER_SIZE;
        let faint_grey = [128, 128, 128, 40].repeat((w * h) as usize);
        Self(Arc::new(Inner {
            placeholder: cache.rgba(w, h, faint_grey),
            placeholder_drawn: AtomicBool::new(false),
            cache,
            fetcher,
            waker,
            slots: Mutex::new(HashMap::new()),
        }))
    }

    /// The picture for `url`: a faint box the size of a poster while it downloads and decodes,
    /// and none if it never will arrive. The first ask starts the download; the waker redraws
    /// when it lands.
    pub fn get(&self, url: &str) -> Option<Image> {
        let mut slots = self.0.slots.lock().unwrap_or_else(|e| e.into_inner());
        match slots.get(url) {
            Some(Slot::Ready { image, .. }) if image.is_ready() => return Some(image.clone()),
            Some(Slot::Failed) => return None,
            Some(_) => return Some(self.0.placeholder.clone()),
            None => {}
        }
        slots.insert(url.to_owned(), Slot::Loading);
        drop(slots);
        let inner = self.0.clone();
        let url = url.to_owned();
        std::thread::spawn(move || {
            let slot = match inner.fetcher.fetch(&url) {
                Ok(bytes) => Slot::Ready {
                    image: inner.cache.load(bytes),
                    shown: false,
                },
                Err(e) => {
                    warn!("image {url}: {e}");
                    Slot::Failed
                }
            };
            inner
                .slots
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .insert(url, slot);
            (inner.waker)();
        });
        Some(self.0.placeholder.clone())
    }

    /// True once for each picture that has finished decoding since the last call, which is
    /// the cue to draw the view again.
    pub fn take_decoded(&self) -> bool {
        let mut slots = self.0.slots.lock().unwrap_or_else(|e| e.into_inner());
        let mut decoded = self.0.placeholder.is_ready()
            && !self.0.placeholder_drawn.swap(true, Ordering::Relaxed);
        for slot in slots.values_mut() {
            if let Slot::Ready { image, shown } = slot
                && !*shown
                && image.is_ready()
            {
                *shown = true;
                decoded = true;
            }
        }
        decoded
    }
}
