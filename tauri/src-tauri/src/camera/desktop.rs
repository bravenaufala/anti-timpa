//! Desktop camera backend backed by `nokhwa`.
//! Replaces `cv2.VideoCapture` from `main_desktop.py`. `nokhwa` is used
//! rather than the `opencv` crate because it links against the system V4L2 /
//! AVFoundation / MediaFoundation APIs directly, which keeps the build free of
//! a full OpenCV dependency — a large part of the old app's ~100 MB APK.
//!
//! Threading note
//! --------------
//! `nokhwa::Camera` is **not** `Send`: it owns a `Box<dyn CaptureBackendTrait>`
//! whose trait object carries no `Send` bound. That rules out storing it in
//! Tauri's `State`, which requires `Send + Sync`.
//!
//! Rather than reach for `unsafe impl Send` (which would be a lie — the driver
//! handle genuinely is not thread-safe) or swap crates, the camera is owned by
//! a dedicated thread. Commands send a request and block on the reply channel.
//! This costs one thread and one channel round-trip per capture, and in return
//! the capture path is provably serialised — which is what a camera device
//! requires anyway.
//!
//! Reopenability
//! -------------
//! `release` is reversible: a released backend can be re-opened via
//! [`DesktopCameraBackend::ensure_open`]. This matters because release is not
//! always final — React StrictMode in development unmounts and remounts every
//! component once, firing the cleanup effect on the throwaway unmount. A
//! backend that latches into a released state would leave the camera
//! permanently dead, which is exactly the bug this guards against.

use super::log::{cam_debug, cam_error, cam_info, cam_warn};
use super::{CameraBackend, CameraError, Frame};

use nokhwa::pixel_format::RgbFormat;
use nokhwa::utils::{ApiBackend, CameraIndex, RequestedFormat, RequestedFormatType};
use nokhwa::Camera;

use std::sync::mpsc::{self, Receiver, Sender};

/// Requests the camera thread understands.
enum Request {
    Capture,
    Release,
}

/// Replies the camera thread sends back.
enum Reply {
    Captured(Result<Frame, CameraError>),
}

/// Handle held by the app. Sends work to the camera thread.
pub struct DesktopCameraBackend {
    tx: Sender<Request>,
    rx: Receiver<Reply>,
    handle: Option<std::thread::JoinHandle<()>>,
    index: u32,
    ready: bool,
}

impl DesktopCameraBackend {
    /// Opens the camera at `index` on a dedicated thread.
    ///
    /// Returns `Err` if the device cannot be opened, letting the caller fall
    /// back to the synthetic backend.
    pub fn open(index: u32) -> Result<Self, CameraError> {
        cam_info(&format!("mencoba membuka kamera desktop index={index}"));

        let (req_tx, req_rx) = mpsc::channel::<Request>();
        let (res_tx, res_rx) = mpsc::channel::<Reply>();

        // Handshake: the thread reports whether opening the device succeeded
        // before we return, so `open` can honour its Result contract instead
        // of failing later at the first capture.
        let (init_tx, init_rx) = mpsc::channel::<Result<(), CameraError>>();

        let handle = std::thread::Builder::new()
            .name("anti-timpa-camera".to_string())
            .spawn(move || camera_thread(index, req_rx, res_tx, init_tx))
            .map_err(|e| {
                cam_error(&format!("gagal membuat thread kamera: {e}"));
                CameraError::CaptureFailed {
                    reason: format!("gagal membuat thread kamera: {e}"),
                }
            })?;

        match init_rx.recv() {
            Ok(Ok(())) => {
                cam_info(&format!("kamera index={index} terbuka dan siap"));
                Ok(Self {
                    tx: req_tx,
                    rx: res_rx,
                    handle: Some(handle),
                    index,
                    ready: true,
                })
            }
            Ok(Err(e)) => {
                cam_error(&format!("gagal membuka kamera index={index}: {e}"));
                let _ = handle.join();
                Err(e)
            }
            Err(_) => {
                cam_error("thread kamera berhenti saat inisialisasi");
                Err(CameraError::CaptureFailed {
                    reason: "thread kamera berhenti saat inisialisasi".into(),
                })
            }
        }
    }

    /// Re-opens the device if it was previously released.
    ///
    /// Called before every capture so a spurious `release` (a StrictMode
    /// remount, a window that was hidden and reshown) self-heals instead of
    /// leaving the user with a permanently dead camera.
    fn ensure_open(&mut self) -> Result<(), CameraError> {
        if self.ready {
            return Ok(());
        }

        cam_warn(&format!(
            "kamera index={} dalam keadaan released; mencoba membuka ulang",
            self.index
        ));

        // The previous thread has exited on Release, so the channel must be
        // rebuilt too — the old receiver was dropped with it.
        let (req_tx, req_rx) = mpsc::channel::<Request>();
        let (res_tx, res_rx) = mpsc::channel::<Reply>();
        let (init_tx, init_rx) = mpsc::channel::<Result<(), CameraError>>();

        let index = self.index;
        let handle = std::thread::Builder::new()
            .name("anti-timpa-camera".to_string())
            .spawn(move || camera_thread(index, req_rx, res_tx, init_tx))
            .map_err(|e| CameraError::CaptureFailed {
                reason: format!("gagal membuat ulang thread kamera: {e}"),
            })?;

        match init_rx.recv() {
            Ok(Ok(())) => {
                self.tx = req_tx;
                self.rx = res_rx;
                self.handle = Some(handle);
                self.ready = true;
                cam_info("kamera berhasil dibuka ulang");
                Ok(())
            }
            Ok(Err(e)) => {
                let _ = handle.join();
                cam_error(&format!("gagal membuka ulang kamera: {e}"));
                Err(e)
            }
            Err(_) => Err(CameraError::CaptureFailed {
                reason: "thread kamera berhenti saat membuka ulang".into(),
            }),
        }
    }
}

/// Enumerates camera device indices available on this machine.
///
/// Hardcoding index 0 is wrong on many Linux machines: the kernel assigns
/// `/dev/videoN` in probe order, and a laptop's built-in webcam is frequently
/// **not** `video0`. UVC devices commonly expose two nodes per camera (a
/// capture node and a metadata node), so a naive "just try the first one"
/// approach can also latch onto a node that opens but never produces frames.
///
/// `nokhwa`'s query returns the real device list, so we use it rather than
/// globbing `/dev/video*` ourselves — that also keeps this correct on macOS and
/// Windows, where the concept of `/dev/videoN` does not exist.
pub fn list_device_indices() -> Vec<u32> {
    use nokhwa::query;

    match query(ApiBackend::Auto) {
        Ok(devices) => {
            let indices: Vec<u32> = devices
                .iter()
                .filter_map(|info| match info.index() {
                    CameraIndex::Index(i) => Some(*i),
                    // A device identified by path cannot be reopened by
                    // index, so it is not usable through the index API.
                    CameraIndex::String(_) => None,
                })
                .collect();

            // Log the friendly names too: on a machine with several nodes for
            // one physical camera, the names make it obvious which are real.
            for info in &devices {
                cam_info(&format!(
                    "device terdeteksi: index={:?} nama='{}' deskripsi='{}'",
                    info.index(),
                    info.human_name(),
                    info.description()
                ));
            }
            indices
        }
        Err(e) => {
            cam_warn(&format!("gagal menanyakan daftar kamera: {e}"));
            Vec::new()
        }
    }
}
/// Owns the `Camera` for its entire life. Everything here runs on one thread,
/// so the non-`Send` driver handle never crosses a thread boundary.
fn camera_thread(
    index: u32,
    req_rx: Receiver<Request>,
    res_tx: Sender<Reply>,
    init_tx: Sender<Result<(), CameraError>>,
) {
    // Highest frame rate at any resolution; requesting a specific resolution
    // is unreliable across webcams, a problem the old app also hit.
    let format = RequestedFormat::new::<RgbFormat>(RequestedFormatType::AbsoluteHighestFrameRate);

    cam_debug(&format!(
        "thread kamera mulai; format diminta={format:?}"
    ));

    let mut camera = match Camera::new(CameraIndex::Index(index), format) {
        Ok(c) => {
            cam_info(&format!(
                "Camera::new berhasil (index={index}); resolusi={:?}",
                c.resolution()
            ));
            c
        }
        Err(e) => {
            let detail = e.to_string();
            cam_error(&format!("Camera::new gagal (index={index}): {detail}"));
            cam_error(
                "  kemungkinan penyebab: tidak ada /dev/video0, izin ditolak, \
                 device dipakai proses lain, atau paket v4l2-utils belum ada",
            );
            let _ = init_tx.send(Err(map_nokhwa_error(detail)));
            return;
        }
    };

    // Start the stream explicitly rather than relying on `frame()`'s implicit
    // lazy open. That laziness is what broke a reopened camera: `Camera::new`
    // succeeded and reported a resolution, but the first `frame()` then failed
    // with "Stream Not Started" because the device had not been reopened
    // properly after a release. Doing it here makes the failure surface at
    // setup time, where it can still be reported as an open error.
    if let Err(e) = camera.open_stream() {
        let detail = e.to_string();
        cam_error(&format!("open_stream gagal untuk index={index}: {detail}"));
        cam_error(
            "  device terdeteksi tapi stream tidak bisa dibuka — biasanya device \
             sedang dipakai proses lain (mis. aplikasi video call) atau \
             pengaturan format tidak didukung",
        );
        let _ = init_tx.send(Err(map_nokhwa_error(detail)));
        return;
    }

    cam_info(&format!(
        "stream terbuka: is_stream_open={} resolusi={:?}",
        camera.is_stream_open(),
        camera.resolution()
    ));

    // Warm-up: prime the driver's buffer queue before reporting ready.
    //
    // The V4L2 backend's `open_stream` builds an `MmapStream` and each
    // `frame()` call blocks in DQBUF waiting for the driver to fill a buffer.
    // A freshly opened webcam typically needs a few frame intervals before it
    // starts delivering, so the very first calls can stall or fail. Discarding
    // a few frames here means the user's first capture is instantly served
    // from an already-running queue instead of waiting on device startup.
    //
    // Failures are tolerated and logged: a slow device should not stop the app
    // from starting, it just means the first capture may be slower.
    const WARMUP_FRAMES: usize = 3;
    for attempt in 0..WARMUP_FRAMES {
        match camera.frame() {
            Ok(_) => cam_debug(&format!("warm-up frame {}/{WARMUP_FRAMES} ok", attempt + 1)),
            Err(e) => {
                cam_warn(&format!(
                    "warm-up frame {}/{WARMUP_FRAMES} gagal: {e}",
                    attempt + 1
                ));
                break;
            }
        }
    }

    let _ = init_tx.send(Ok(()));

    while let Ok(request) = req_rx.recv() {
        match request {
            Request::Capture => {
                cam_debug("perintah Capture diterima; mengambil frame");
                let result = capture_once(&mut camera);
                match &result {
                    Ok(f) => cam_debug(&format!(
                        "frame diambil: {}x{} ({} byte)",
                        f.width,
                        f.height,
                        f.rgb.len()
                    )),
                    Err(e) => cam_error(&format!("capture gagal: {e}")),
                }
                // If the consumer is gone the app is shutting down.
                if res_tx.send(Reply::Captured(result)).is_err() {
                    cam_debug("konsumen hilang saat mengirim frame; thread berhenti");
                    break;
                }
            }
            Request::Release => {
                cam_debug("perintah Release diterima; menutup stream");
                if let Err(e) = camera.stop_stream() {
                    // Not fatal: we are tearing down either way.
                    cam_warn(&format!("stop_stream gagal saat release: {e}"));
                }
                cam_info(&format!(
                    "stream ditutup: is_stream_open={}",
                    camera.is_stream_open()
                ));
                break;
            }
        }
    }

    cam_debug("thread kamera keluar");
}

fn capture_once(camera: &mut Camera) -> Result<Frame, CameraError> {
    // Reopen if something closed the stream underneath us (another process
    // grabbing the device, or a driver hiccup). Cheaper than failing the
    // capture outright, and self-healing matches `ensure_open`'s intent.
    //
    // Note: on the V4L2 backend `open_stream` replaces the stream handle
    // rather than being a no-op, so this must only run when the stream is
    // genuinely closed — calling it on a live stream would drop the buffer
    // queue and stall the next few frames.
    if !camera.is_stream_open() {
        cam_warn("stream tertutup saat akan capture; mencoba membuka kembali");
        camera.open_stream().map_err(|e| {
            let detail = e.to_string();
            cam_error(&format!("open_stream ulang gagal: {detail}"));
            map_nokhwa_error(detail)
        })?;
        cam_info("stream berhasil dibuka kembali");
    }

    // `frame()` blocks in the driver until a buffer is ready. That is the
    // desired behaviour for a one-shot capture: the caller gets the next real
    // frame rather than a stale one. It does mean a stalled device surfaces as
    // a delay rather than an immediate error, which is why the driver timeout
    // path below is reported distinctly.
    let buffer = camera.frame().map_err(|e| {
        let detail = e.to_string();
        cam_error(&format!("camera.frame() gagal: {detail}"));
        if detail.contains("Stream Not Started") {
            cam_error(
                "  stream tidak terbuka — ini bug internal, karena is_stream_open \
                 dicek sebelum pemanggilan",
            );
        } else {
            cam_error(
                "  ini biasanya berarti device terputus, dipakai proses lain, \
                 atau timeout driver (webcam sedang tidak mengirim data)",
            );
        }
        map_nokhwa_error(detail)
    })?;

    cam_debug(&format!(
        "buffer kamera diterima: {}x{} ({})",
        buffer.resolution().width_x,
        buffer.resolution().height_y,
        buffer.source_frame_format()
    ));

    let decoded = buffer.decode_image::<RgbFormat>().map_err(|e| {
        let detail = e.to_string();
        cam_error(&format!("decode_image::<RgbFormat> gagal: {detail}"));
        cam_error("  format frame dari driver tidak bisa dikonversi ke RGB");
        map_nokhwa_error(detail)
    })?;

    let (width, height) = (decoded.width(), decoded.height());
    Frame::new(width, height, decoded.into_raw())
}

fn map_nokhwa_error(reason: String) -> CameraError {
    let lower = reason.to_lowercase();
    if lower.contains("permission") || lower.contains("denied") || lower.contains("busy") {
        CameraError::PermissionDenied
    } else if lower.contains("no such")
        || lower.contains("not found")
        || lower.contains("no device")
    {
        CameraError::NotFound
    } else {
        CameraError::CaptureFailed { reason }
    }
}

impl CameraBackend for DesktopCameraBackend {
    fn name(&self) -> &'static str {
        "nokhwa-desktop"
    }

    fn is_ready(&self) -> bool {
        self.ready
    }

    fn capture(&mut self) -> Result<Frame, CameraError> {
        // Self-heal: a previous release may have been spurious (StrictMode
        // remount), so try to reopen rather than reporting a dead camera.
        self.ensure_open()?;

        self.tx
            .send(Request::Capture)
            .map_err(|_| CameraError::CaptureFailed {
                reason: "thread kamera tidak lagi berjalan".into(),
            })?;

        match self.rx.recv() {
            Ok(Reply::Captured(result)) => result,
            Err(_) => Err(CameraError::CaptureFailed {
                reason: "thread kamera berhenti sebelum mengirim frame".into(),
            }),
        }
    }

    fn release(&mut self) {
        if !self.ready {
            cam_debug("release dipanggil tapi kamera sudah released; diabaikan");
            return;
        }
        cam_info("release: menutup kamera desktop");
        self.ready = false;
        // Ignore send failure: the thread may already have exited, which is
        // the desired end state regardless.
        let _ = self.tx.send(Request::Release);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
        cam_info("release: kamera desktop ditutup");
    }
}

impl Drop for DesktopCameraBackend {
    fn drop(&mut self) {
        // Unlike the UI-facing `release`, dropping really is final, so this
        // must not attempt a reopen.
        if self.ready {
            cam_debug("Drop: menutup kamera desktop");
            self.ready = false;
            let _ = self.tx.send(Request::Release);
            if let Some(handle) = self.handle.take() {
                let _ = handle.join();
            }
        }
    }
}

impl std::fmt::Debug for DesktopCameraBackend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "DesktopCameraBackend(index={}, ready={})", self.index, self.ready)
    }
}
