//! Background preview rendering.
//!
//! Requests are debounced and coalesced: while the user types, only the
//! latest document is rendered. Results are posted back to the UI thread.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::time::Duration;

use slint::{ComponentHandle, Rgba8Pixel, SharedPixelBuffer};

use menu_core::domain::MenuDocument;
use menu_core::engine::MenuEngine;

use crate::{MainWindow, PreviewState, Strings};

const DEBOUNCE: Duration = Duration::from_millis(120);

struct Request {
    doc: MenuDocument,
    /// Physical pixels per PDF point (on-screen size × HiDPI factor).
    pixels_per_point: f32,
    generation: u64,
}

pub struct PreviewWorker {
    tx: Sender<Request>,
    latest: Arc<AtomicU64>,
}

impl PreviewWorker {
    pub fn spawn(engine: Arc<MenuEngine>, window: slint::Weak<MainWindow>) -> Self {
        let (tx, rx) = mpsc::channel::<Request>();
        let latest = Arc::new(AtomicU64::new(0));
        let latest_in_thread = latest.clone();

        std::thread::Builder::new()
            .name("preview".into())
            .spawn(move || {
                while let Ok(mut request) = rx.recv() {
                    // Debounce: keep taking newer requests until things settle.
                    loop {
                        match rx.recv_timeout(DEBOUNCE) {
                            Ok(newer) => request = newer,
                            Err(RecvTimeoutError::Timeout) => break,
                            Err(RecvTimeoutError::Disconnected) => return,
                        }
                    }
                    let outcome = engine
                        .render_preview(&request.doc, request.pixels_per_point)
                        .map(|(rendered, image)| {
                            let buffer = SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(
                                &image.pixels,
                                image.width,
                                image.height,
                            );
                            let (w, h) = rendered.page_size_pt;
                            (buffer, h / w, rendered.scale, rendered.fits)
                        });
                    if let Err(error) = &outcome {
                        tracing::error!(error = %format!("{error:#}"), "preview render failed");
                    }
                    let is_latest = latest_in_thread.load(Ordering::SeqCst) == request.generation;
                    let window = window.clone();
                    let _ = slint::invoke_from_event_loop(move || {
                        let Some(ui) = window.upgrade() else { return };
                        let preview = ui.global::<PreviewState>();
                        match outcome {
                            Ok((buffer, aspect, scale, fits)) => {
                                preview.set_page(slint::Image::from_rgba8(buffer));
                                preview.set_aspect(aspect);
                                preview.set_scale_percent((scale * 100.0).round() as i32);
                                preview.set_fits(fits);
                                preview.set_error("".into());
                            }
                            Err(error) => {
                                let message = ui
                                    .global::<Strings>()
                                    .invoke_render_failed(format!("{error:#}").into());
                                preview.set_error(message);
                            }
                        }
                        if is_latest {
                            preview.set_busy(false);
                        }
                    });
                }
            })
            .expect("failed to start preview thread");

        Self { tx, latest }
    }

    pub fn request(&self, doc: MenuDocument, pixels_per_point: f32) {
        let generation = self.latest.fetch_add(1, Ordering::SeqCst) + 1;
        let pixels_per_point = pixels_per_point.clamp(0.4, 5.0);
        let _ = self.tx.send(Request {
            doc,
            pixels_per_point,
            generation,
        });
    }
}
