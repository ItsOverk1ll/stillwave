// SPDX-License-Identifier: GPL-3.0-only
//! WASAPI output for Windows: exclusive mode (bit-perfect, bypasses the Windows
//! mixer) and shared mode (goes through the mixer, so never bit-perfect).
//!
//! Design:
//! - A render thread owns every COM object (they are not `Send`) and is woken by
//!   the device's event. It pulls `i32` samples from a lock-free ring buffer,
//!   packs them with [`crate::pack`] (no other processing) and fills the device
//!   buffer. If the ring runs dry it writes silence and counts an underrun.
//! - The caller's `write()` only pushes into the ring, so decoding never runs on
//!   the render thread and the render thread never blocks on decoding.
//! - Exclusive mode tries the narrowest lossless container first, then wider ones
//!   (16-bit audio can travel in a 24- or 32-bit container with zero padding and
//!   stays bit-perfect). It never reduces bit depth and never resamples.
//!
//! This module is the only place in the crate that uses `unsafe` (COM calls).
#![allow(unsafe_code)]

use std::sync::atomic::{AtomicBool, AtomicIsize, AtomicU32, AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use ringbuf::traits::{Consumer, Producer, Split};
use ringbuf::{HeapCons, HeapProd, HeapRb};
use windows::core::PCWSTR;
use windows::Win32::Foundation::{HANDLE, PROPERTYKEY, WAIT_OBJECT_0};
use windows::Win32::Media::Audio::{
    eConsole, eRender, IAudioClient, IAudioRenderClient, IMMDevice, IMMDeviceEnumerator,
    MMDeviceEnumerator, AUDCLNT_E_BUFFER_SIZE_NOT_ALIGNED, AUDCLNT_SHAREMODE,
    AUDCLNT_SHAREMODE_EXCLUSIVE, AUDCLNT_SHAREMODE_SHARED, AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM,
    AUDCLNT_STREAMFLAGS_EVENTCALLBACK, AUDCLNT_STREAMFLAGS_SRC_DEFAULT_QUALITY,
    DEVICE_STATE_ACTIVE, WAVEFORMATEX, WAVEFORMATEXTENSIBLE, WAVEFORMATEXTENSIBLE_0,
};
use windows::Win32::Media::KernelStreaming::{KSDATAFORMAT_SUBTYPE_PCM, WAVE_FORMAT_EXTENSIBLE};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoTaskMemFree, CoUninitialize, CLSCTX_ALL,
    COINIT_MULTITHREADED, STGM_READ,
};
use windows::Win32::System::Threading::{
    AvRevertMmThreadCharacteristics, AvSetMmThreadCharacteristicsW, CreateEventW, SetEvent,
    WaitForSingleObject,
};

use crate::format::StreamFormat;
use crate::pack::{pack, to_f32, Container};
use crate::signal_path::SignalPath;
use crate::traits::{AudioError, OutputDevice, OutputMode, OutputStream};

pub const NAME: &str = "WASAPI";

/// Sample rates probed when listing what a device supports.
const PROBE_RATES: [u32; 8] = [
    44_100, 48_000, 88_200, 96_000, 176_400, 192_000, 352_800, 384_000,
];
const PROBE_DEPTHS: [u8; 3] = [16, 24, 32];
/// 100 ns units per millisecond.
const HNS_PER_MS: i64 = 10_000;
/// Smallest device buffer we ask for; tiny buffers underrun on busy machines.
const MIN_BUFFER_MS: i64 = 10;
/// How much audio the ring can hold, in milliseconds.
const RING_MS: usize = 300;

fn dev_err(what: &str, e: &windows::core::Error) -> AudioError {
    AudioError::Device(format!("{what}: {e}"))
}

/// Initialises COM for the current thread for as long as it lives.
struct Com;

impl Com {
    fn init() -> Self {
        // S_FALSE (already initialised) is fine; we balance every call.
        // SAFETY: plain COM initialisation, balanced by `Drop`.
        let _ = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
        Self
    }
}

impl Drop for Com {
    fn drop(&mut self) {
        // SAFETY: pairs with the `CoInitializeEx` in `init`.
        unsafe { CoUninitialize() };
    }
}

/// A Windows playback endpoint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WasapiDevice {
    /// Stable endpoint ID.
    pub id: String,
    /// Friendly name, e.g. "Speakers (USB DAC)".
    pub friendly_name: String,
    pub is_default: bool,
}

fn enumerator() -> Result<IMMDeviceEnumerator, AudioError> {
    // SAFETY: standard COM activation of the system device enumerator.
    unsafe { CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL) }
        .map_err(|e| dev_err("device enumerator", &e))
}

fn pwstr_to_string(p: windows::core::PWSTR) -> String {
    // SAFETY: `p` is a valid NUL-terminated string allocated by COM; freed below.
    let s = unsafe { p.to_string() }.unwrap_or_default();
    // SAFETY: COM allocated this string with CoTaskMemAlloc.
    unsafe { CoTaskMemFree(Some(p.as_ptr().cast())) };
    s
}

fn friendly_name(device: &IMMDevice) -> String {
    // PKEY_Device_FriendlyName {a45c254e-df1c-4efd-8020-67d146a850e0}, 14
    const KEY: PROPERTYKEY = PROPERTYKEY {
        fmtid: windows::core::GUID::from_u128(0xa45c254e_df1c_4efd_8020_67d146a850e0),
        pid: 14,
    };
    // SAFETY: property store access on a live endpoint.
    unsafe {
        let Ok(store) = device.OpenPropertyStore(STGM_READ) else {
            return "Unknown device".into();
        };
        let Ok(value) = store.GetValue(&KEY) else {
            return "Unknown device".into();
        };
        value.to_string()
    }
}

/// Lists active playback devices. The default device is flagged.
pub fn list_devices() -> Result<Vec<WasapiDevice>, AudioError> {
    let _com = Com::init();
    let e = enumerator()?;
    // SAFETY: COM calls on live interfaces; strings freed by `pwstr_to_string`.
    unsafe {
        let default_id = e
            .GetDefaultAudioEndpoint(eRender, eConsole)
            .and_then(|d| d.GetId())
            .map(pwstr_to_string)
            .ok();
        let collection = e
            .EnumAudioEndpoints(eRender, DEVICE_STATE_ACTIVE)
            .map_err(|er| dev_err("enumerate devices", &er))?;
        let count = collection
            .GetCount()
            .map_err(|er| dev_err("count devices", &er))?;
        let mut out = Vec::new();
        for i in 0..count {
            let Ok(d) = collection.Item(i) else { continue };
            let Ok(id) = d.GetId().map(pwstr_to_string) else {
                continue;
            };
            out.push(WasapiDevice {
                friendly_name: friendly_name(&d),
                is_default: default_id.as_deref() == Some(id.as_str()),
                id,
            });
        }
        Ok(out)
    }
}

/// The system default playback device.
pub fn default_device() -> Result<WasapiDevice, AudioError> {
    list_devices()?
        .into_iter()
        .find(|d| d.is_default)
        .ok_or_else(|| AudioError::Device("no default playback device".into()))
}

fn get_device(enumerator: &IMMDeviceEnumerator, id: &str) -> Result<IMMDevice, AudioError> {
    let wide: Vec<u16> = id.encode_utf16().chain(std::iter::once(0)).collect();
    // SAFETY: `wide` is NUL-terminated and outlives the call.
    unsafe { enumerator.GetDevice(PCWSTR(wide.as_ptr())) }.map_err(|e| dev_err("open device", &e))
}

fn activate(device: &IMMDevice) -> Result<IAudioClient, AudioError> {
    // SAFETY: activating the audio client interface on a live endpoint.
    unsafe { device.Activate::<IAudioClient>(CLSCTX_ALL, None) }
        .map_err(|e| dev_err("activate audio client", &e))
}

/// Builds a PCM `WAVEFORMATEXTENSIBLE`. `valid_bits` is the audio's depth, the
/// container is how wide each sample slot is; samples are MSB-justified.
fn make_format(
    rate: u32,
    channels: u8,
    container: Container,
    valid_bits: u8,
) -> WAVEFORMATEXTENSIBLE {
    let block_align = u16::from(channels) * u16::try_from(container.bytes()).unwrap_or(4);
    // WAVE_FORMAT_EXTENSIBLE is 0xFFFE, so the narrowing is exact.
    #[allow(clippy::cast_possible_truncation)]
    let tag = WAVE_FORMAT_EXTENSIBLE as u16;
    WAVEFORMATEXTENSIBLE {
        Format: WAVEFORMATEX {
            wFormatTag: tag,
            nChannels: u16::from(channels),
            nSamplesPerSec: rate,
            nAvgBytesPerSec: rate * u32::from(block_align),
            nBlockAlign: block_align,
            wBitsPerSample: u16::from(container.bits()),
            cbSize: 22,
        },
        Samples: WAVEFORMATEXTENSIBLE_0 {
            wValidBitsPerSample: u16::from(valid_bits),
        },
        dwChannelMask: if channels == 2 { 0x3 } else { 0 },
        SubFormat: KSDATAFORMAT_SUBTYPE_PCM,
    }
}

/// Containers to try for audio of `valid_bits`, narrowest lossless first.
fn container_candidates(valid_bits: u8) -> &'static [Container] {
    match valid_bits {
        0..=16 => &[
            Container::Bits16,
            Container::Bits24Packed,
            Container::Bits32,
        ],
        17..=24 => &[Container::Bits32, Container::Bits24Packed],
        _ => &[Container::Bits32],
    }
}

fn exclusive_supported(client: &IAudioClient, fmt: &WAVEFORMATEXTENSIBLE) -> bool {
    // SAFETY: `fmt` is a valid WAVEFORMATEXTENSIBLE; the cast is how the API takes it.
    let hr = unsafe {
        client.IsFormatSupported(
            AUDCLNT_SHAREMODE_EXCLUSIVE,
            std::ptr::from_ref(fmt).cast::<WAVEFORMATEX>(),
            None,
        )
    };
    hr.0 == 0 // S_OK exactly; S_FALSE ("closest match") means not supported as asked
}

/// Finds the first lossless container the device accepts for this format.
fn negotiate_exclusive(
    client: &IAudioClient,
    format: StreamFormat,
) -> Option<(Container, WAVEFORMATEXTENSIBLE)> {
    container_candidates(format.bit_depth)
        .iter()
        .find_map(|&c| {
            let wfx = make_format(
                format.sample_rate_hz,
                format.channels,
                c,
                format.bit_depth.min(c.bits()),
            );
            exclusive_supported(client, &wfx).then_some((c, wfx))
        })
}

impl OutputDevice for WasapiDevice {
    fn name(&self) -> String {
        self.friendly_name.clone()
    }

    fn supported_formats(&self, mode: OutputMode) -> Result<Vec<StreamFormat>, AudioError> {
        let _com = Com::init();
        let e = enumerator()?;
        let client = activate(&get_device(&e, &self.id)?)?;
        match mode {
            OutputMode::Exclusive => {
                let mut out = Vec::new();
                for rate in PROBE_RATES {
                    for depth in PROBE_DEPTHS {
                        let f = StreamFormat::new(rate, depth, 2);
                        if negotiate_exclusive(&client, f).is_some() {
                            out.push(f);
                        }
                    }
                }
                Ok(out)
            }
            OutputMode::Shared => {
                // SAFETY: GetMixFormat returns a CoTaskMem block we free below.
                unsafe {
                    let p = client
                        .GetMixFormat()
                        .map_err(|e| dev_err("mix format", &e))?;
                    let (rate, ch) = ((*p).nSamplesPerSec, (*p).nChannels);
                    CoTaskMemFree(Some(p.cast()));
                    Ok(vec![StreamFormat::new(
                        rate,
                        24,
                        u8::try_from(ch).unwrap_or(2),
                    )])
                }
            }
        }
    }

    fn open(
        &self,
        format: StreamFormat,
        mode: OutputMode,
    ) -> Result<Box<dyn OutputStream>, AudioError> {
        WasapiStream::open(self.clone(), format, mode).map(|s| Box::new(s) as Box<dyn OutputStream>)
    }
}

/// State shared between the caller's thread and the render thread.
struct Shared {
    stop: AtomicBool,
    paused: AtomicBool,
    underruns: AtomicU32,
    /// Audio frames handed to the device (silence not counted).
    frames_played: AtomicU64,
    /// Samples (not frames) the render thread has taken from the ring, played or discarded.
    consumed: AtomicU64,
    /// Samples up to this running total are discarded instead of played (flush).
    discard_until: AtomicU64,
    error: Mutex<Option<String>>,
    /// The render thread's wake-up event, so `Drop` can interrupt the wait.
    event: AtomicIsize,
}

/// What the render thread tells us once the device is open.
struct Opened {
    container: Container,
    exclusive: bool,
    device_buffer_frames: u32,
}

/// An open WASAPI stream.
pub struct WasapiStream {
    producer: HeapProd<i32>,
    /// Samples pushed into the ring so far (running total).
    pushed: u64,
    shared: Arc<Shared>,
    thread: Option<JoinHandle<()>>,
    format: StreamFormat,
    opened: Opened,
    device_name: String,
}

impl WasapiStream {
    fn open(
        device: WasapiDevice,
        format: StreamFormat,
        mode: OutputMode,
    ) -> Result<Self, AudioError> {
        let channels = usize::from(format.channels);
        let ring_samples = (format.sample_rate_hz as usize * channels * RING_MS / 1000).max(4096);
        let (producer, consumer) = HeapRb::<i32>::new(ring_samples).split();
        let shared = Arc::new(Shared {
            stop: AtomicBool::new(false),
            paused: AtomicBool::new(false),
            underruns: AtomicU32::new(0),
            frames_played: AtomicU64::new(0),
            consumed: AtomicU64::new(0),
            discard_until: AtomicU64::new(0),
            error: Mutex::new(None),
            event: AtomicIsize::new(0),
        });
        let (tx, rx) = mpsc::channel::<Result<Opened, AudioError>>();
        let (s2, id) = (Arc::clone(&shared), device.id.clone());
        let thread = std::thread::Builder::new()
            .name("stillwave-wasapi".into())
            .spawn(move || render_thread(&id, format, mode, consumer, &s2, &tx))
            .map_err(|e| AudioError::Device(format!("render thread: {e}")))?;
        let opened = rx
            .recv_timeout(Duration::from_secs(10))
            .map_err(|_| AudioError::Device("device did not open in time".into()))?
            .map_err(|e| match e {
                // Say which device refused, so the user knows what to change.
                AudioError::Unsupported(m) => {
                    AudioError::Unsupported(format!("{}: {m}", device.friendly_name))
                }
                other => other,
            })?;
        Ok(Self {
            producer,
            pushed: 0,
            shared,
            thread: Some(thread),
            format,
            opened,
            device_name: device.friendly_name,
        })
    }

    /// Times the device buffer ran out of audio (a glitch). Zero when healthy.
    #[must_use]
    pub fn underruns(&self) -> u32 {
        self.shared.underruns.load(Ordering::Relaxed)
    }

    /// Audio frames the device has consumed so far.
    #[must_use]
    pub fn frames_played(&self) -> u64 {
        self.shared.frames_played.load(Ordering::Relaxed)
    }

    /// Container the device was opened with.
    #[must_use]
    pub fn container(&self) -> Container {
        self.opened.container
    }

    #[must_use]
    pub fn device_name(&self) -> &str {
        &self.device_name
    }

    fn check_error(&self) -> Result<(), AudioError> {
        match self.shared.error.lock() {
            Ok(g) => g
                .as_ref()
                .map_or(Ok(()), |m| Err(AudioError::Device(m.clone()))),
            Err(_) => Err(AudioError::Device("render thread panicked".into())),
        }
    }
}

impl OutputStream for WasapiStream {
    fn signal_path(&self, source: StreamFormat, lossy_source: bool) -> SignalPath {
        let exclusive = self.opened.exclusive;
        let lossless_container = self.opened.container.holds_exactly(source.bit_depth);
        SignalPath {
            source,
            output: self.format,
            resampled: source.sample_rate_hz != self.format.sample_rate_hz,
            dsp_active: false,
            dithered: false,
            // The Windows mixer applies its own volume and may convert: not bit-perfect.
            software_volume: !exclusive || !lossless_container,
            exclusive,
            lossy_source,
        }
    }

    fn try_write(&mut self, samples: &[i32]) -> Result<usize, AudioError> {
        self.check_error()?;
        let n = self.producer.push_slice(samples);
        self.pushed += n as u64;
        Ok(n)
    }

    fn drain(&mut self) -> Result<(), AudioError> {
        let deadline = Instant::now() + Duration::from_secs(30);
        while self.shared.consumed.load(Ordering::Acquire) < self.pushed {
            self.check_error()?;
            if Instant::now() > deadline {
                return Err(AudioError::Device("drain timed out".into()));
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        // Let the audio already in the device buffer play out.
        let ms = i64::from(self.opened.device_buffer_frames) * 1000
            / i64::from(self.format.sample_rate_hz);
        std::thread::sleep(Duration::from_millis(
            u64::try_from(ms.max(1)).unwrap_or(10) * 2,
        ));
        self.check_error()
    }

    fn flush(&mut self) -> Result<(), AudioError> {
        // Everything pushed so far is stale; the render thread skips up to here.
        self.shared
            .discard_until
            .store(self.pushed, Ordering::Release);
        Ok(())
    }

    fn frames_played(&self) -> u64 {
        self.shared.frames_played.load(Ordering::Relaxed)
    }

    fn pause(&mut self) -> Result<(), AudioError> {
        self.shared.paused.store(true, Ordering::Relaxed);
        Ok(())
    }

    fn resume(&mut self) -> Result<(), AudioError> {
        self.shared.paused.store(false, Ordering::Relaxed);
        Ok(())
    }
}

impl Drop for WasapiStream {
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::Relaxed);
        let ev = self.shared.event.load(Ordering::Relaxed);
        if ev != 0 {
            // SAFETY: the handle is owned by the render thread, which is still joined below.
            let _ = unsafe { SetEvent(HANDLE(ev as *mut _)) };
        }
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

// ---- render thread ---------------------------------------------------------

struct Render {
    client: IAudioClient,
    out: IAudioRenderClient,
    event: HANDLE,
    buffer_frames: u32,
    container: Container,
    exclusive: bool,
    channels: usize,
}

fn set_error(shared: &Shared, msg: String) {
    if let Ok(mut g) = shared.error.lock() {
        *g = Some(msg);
    }
}

fn render_thread(
    device_id: &str,
    format: StreamFormat,
    mode: OutputMode,
    mut consumer: HeapCons<i32>,
    shared: &Arc<Shared>,
    tx: &mpsc::Sender<Result<Opened, AudioError>>,
) {
    let _com = Com::init();
    let mut r = match setup(device_id, format, mode, shared) {
        Ok(r) => r,
        Err(e) => {
            let _ = tx.send(Err(e));
            return;
        }
    };
    let _ = tx.send(Ok(Opened {
        container: r.container,
        exclusive: r.exclusive,
        device_buffer_frames: r.buffer_frames,
    }));

    // Ask Windows to treat this thread as pro-audio so it is scheduled on time.
    let mut task = 0u32;
    // SAFETY: valid NUL-terminated name and out-pointer.
    let mm =
        unsafe { AvSetMmThreadCharacteristicsW(windows::core::w!("Pro Audio"), &raw mut task) }
            .ok();

    // SAFETY: client is initialised; Start/Stop bracket the loop.
    if let Err(e) = unsafe { r.client.Start() } {
        set_error(shared, format!("start: {e}"));
    } else {
        run(&mut r, &mut consumer, shared);
        // SAFETY: stopping a started client.
        let _ = unsafe { r.client.Stop() };
    }
    if let Some(h) = mm {
        // SAFETY: handle came from AvSetMmThreadCharacteristicsW above.
        let _ = unsafe { AvRevertMmThreadCharacteristics(h) };
    }
}

fn setup(
    device_id: &str,
    format: StreamFormat,
    mode: OutputMode,
    shared: &Shared,
) -> Result<Render, AudioError> {
    let e = enumerator()?;
    let device = get_device(&e, device_id)?;
    let mut client = activate(&device)?;
    let channels = usize::from(format.channels);

    let (sharemode, container, exclusive): (AUDCLNT_SHAREMODE, Container, bool);
    let mut wfx: WAVEFORMATEXTENSIBLE;
    let mut flags = AUDCLNT_STREAMFLAGS_EVENTCALLBACK;
    // SAFETY: COM calls on live interfaces; CoTaskMem blocks freed after use.
    let (default_period, min_period) = unsafe {
        let (mut d, mut m) = (0i64, 0i64);
        client
            .GetDevicePeriod(Some(&raw mut d), Some(&raw mut m))
            .map_err(|er| dev_err("device period", &er))?;
        (d, m)
    };
    match mode {
        OutputMode::Exclusive => {
            let (c, w) = negotiate_exclusive(&client, format).ok_or_else(|| {
                AudioError::Unsupported(format!(
                    "this device does not accept {format} in exclusive mode, and Stillwave never converts audio without asking"
                ))
            })?;
            (sharemode, container, exclusive, wfx) = (AUDCLNT_SHAREMODE_EXCLUSIVE, c, true, w);
        }
        OutputMode::Shared => {
            // The Windows mixer works in float32. We feed it float32 and let it
            // convert the rate if needed. Not bit-perfect, and reported as such.
            wfx = make_format(
                format.sample_rate_hz,
                format.channels,
                Container::Bits32,
                32,
            );
            wfx.Format.wBitsPerSample = 32;
            wfx.SubFormat = windows::core::GUID::from_u128(0x00000003_0000_0010_8000_00aa00389b71); // KSDATAFORMAT_SUBTYPE_IEEE_FLOAT
            flags |= AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM | AUDCLNT_STREAMFLAGS_SRC_DEFAULT_QUALITY;
            (sharemode, container, exclusive) =
                (AUDCLNT_SHAREMODE_SHARED, Container::Bits32, false);
        }
    }

    let mut duration = default_period
        .max(min_period)
        .max(MIN_BUFFER_MS * HNS_PER_MS);
    let wfx_ptr = std::ptr::from_ref(&wfx).cast::<WAVEFORMATEX>();
    let (hns, period) = if exclusive {
        (duration, duration)
    } else {
        (0, 0)
    };
    // SAFETY: wfx_ptr points to a valid format that outlives the call.
    let mut init = unsafe { client.Initialize(sharemode, flags, hns, period, wfx_ptr, None) };

    // Exclusive + event mode needs the buffer to be a whole number of device
    // frames; the first attempt may be rejected with the size we should use.
    if exclusive {
        if let Err(err) = &init {
            if err.code() == AUDCLNT_E_BUFFER_SIZE_NOT_ALIGNED {
                // SAFETY: querying the aligned size the failed Initialize reported.
                let frames =
                    unsafe { client.GetBufferSize() }.map_err(|er| dev_err("aligned size", &er))?;
                duration = 10_000_000 * i64::from(frames) / i64::from(format.sample_rate_hz);
                client = activate(&device)?;
                // SAFETY: as above, with the aligned duration.
                init = unsafe {
                    client.Initialize(sharemode, flags, duration, duration, wfx_ptr, None)
                };
            }
        }
    }
    init.map_err(|er| dev_err("initialise stream", &er))?;

    // SAFETY: client is initialised; event handle and render client are owned by `Render`.
    unsafe {
        let event =
            CreateEventW(None, false, false, None).map_err(|er| dev_err("create event", &er))?;
        client
            .SetEventHandle(event)
            .map_err(|er| dev_err("set event", &er))?;
        let buffer_frames = client
            .GetBufferSize()
            .map_err(|er| dev_err("buffer size", &er))?;
        let out = client
            .GetService::<IAudioRenderClient>()
            .map_err(|er| dev_err("render client", &er))?;
        shared.event.store(event.0 as isize, Ordering::Relaxed);
        Ok(Render {
            client,
            out,
            event,
            buffer_frames,
            container,
            exclusive,
            channels,
        })
    }
}

fn run(r: &mut Render, consumer: &mut HeapCons<i32>, shared: &Shared) {
    let mut scratch: Vec<i32> = vec![0; r.buffer_frames as usize * r.channels];
    // Prime the device buffer with silence so the first event has room.
    if let Err(e) = fill(r, consumer, shared, r.buffer_frames, &mut scratch, true) {
        set_error(shared, e);
        return;
    }
    while !shared.stop.load(Ordering::Relaxed) {
        // SAFETY: waiting on the event owned by this render context.
        let wait = unsafe { WaitForSingleObject(r.event, 2000) };
        if shared.stop.load(Ordering::Relaxed) {
            break;
        }
        if wait != WAIT_OBJECT_0 {
            set_error(
                shared,
                "device stopped delivering events (unplugged?)".into(),
            );
            return;
        }
        let frames = if r.exclusive {
            r.buffer_frames
        } else {
            // SAFETY: shared mode: space = buffer size minus what is still queued.
            match unsafe { r.client.GetCurrentPadding() } {
                Ok(p) => r.buffer_frames - p,
                Err(e) => {
                    set_error(shared, format!("padding: {e}"));
                    return;
                }
            }
        };
        if frames == 0 {
            continue;
        }
        let silent = shared.paused.load(Ordering::Relaxed);
        if let Err(e) = fill(r, consumer, shared, frames, &mut scratch, silent) {
            set_error(shared, e);
            return;
        }
    }
}

/// Takes up to `out.len()` samples from the ring for the device.
///
/// Anything the caller flushed (everything up to `discard_until`, a running total)
/// is thrown away first, then real audio is taken. Every sample removed from the
/// ring, played or discarded, is added to `consumed`, which `drain()` compares with
/// what was pushed. When `silent` (paused) nothing is taken. Returns how many
/// samples of audio were written to `out`.
fn take_samples(
    ring: &mut impl Consumer<Item = i32>,
    consumed: &AtomicU64,
    discard_until: &AtomicU64,
    out: &mut [i32],
    silent: bool,
) -> usize {
    loop {
        let target = discard_until.load(Ordering::Acquire);
        let done = consumed.load(Ordering::Relaxed);
        if done >= target {
            break;
        }
        let want = usize::try_from(target - done)
            .unwrap_or(usize::MAX)
            .min(out.len());
        let dropped = ring.pop_slice(&mut out[..want]);
        if dropped == 0 {
            break;
        }
        consumed.fetch_add(dropped as u64, Ordering::Release);
    }
    if silent {
        return 0;
    }
    let got = ring.pop_slice(out);
    consumed.fetch_add(got as u64, Ordering::Release);
    got
}

/// Fills `frames` frames of the device buffer from the ring (or silence).
fn fill(
    r: &Render,
    consumer: &mut HeapCons<i32>,
    shared: &Shared,
    frames: u32,
    scratch: &mut [i32],
    silent: bool,
) -> Result<(), String> {
    let samples = frames as usize * r.channels;
    let scratch = &mut scratch[..samples];
    let got = take_samples(
        consumer,
        &shared.consumed,
        &shared.discard_until,
        scratch,
        silent,
    );
    if got < samples {
        scratch[got..].fill(0);
        if !silent {
            shared.underruns.fetch_add(1, Ordering::Relaxed);
        }
    }
    let width = if r.exclusive { r.container.bytes() } else { 4 };
    // SAFETY: GetBuffer returns a writable region of exactly `frames` frames, valid
    // until ReleaseBuffer; we write exactly samples*width bytes into it.
    unsafe {
        let ptr = r
            .out
            .GetBuffer(frames)
            .map_err(|e| format!("get buffer: {e}"))?;
        let out = std::slice::from_raw_parts_mut(ptr, samples * width);
        if r.exclusive {
            pack(scratch, r.container, out);
        } else {
            for (s, o) in scratch.iter().zip(out.chunks_exact_mut(4)) {
                o.copy_from_slice(&to_f32(*s).to_le_bytes());
            }
        }
        r.out
            .ReleaseBuffer(frames, 0)
            .map_err(|e| format!("release buffer: {e}"))?;
    }
    shared
        .frames_played
        .fetch_add((got / r.channels) as u64, Ordering::Relaxed);
    Ok(())
}

#[cfg(test)]
mod tests {
    use ringbuf::traits::{Observer, Producer, Split};
    use ringbuf::HeapRb;

    use super::*;

    fn ring(n: i32) -> (ringbuf::HeapProd<i32>, ringbuf::HeapCons<i32>) {
        let (mut p, c) = HeapRb::<i32>::new(1024).split();
        let all: Vec<i32> = (0..n).collect();
        assert_eq!(p.push_slice(&all), all.len());
        (p, c)
    }

    #[test]
    fn taking_samples_counts_them_so_drain_can_finish() {
        let (_p, mut c) = ring(100);
        let (consumed, discard) = (AtomicU64::new(0), AtomicU64::new(0));
        let mut out = [0i32; 64];
        assert_eq!(
            take_samples(&mut c, &consumed, &discard, &mut out, false),
            64
        );
        assert_eq!(&out[..3], &[0, 1, 2]);
        assert_eq!(
            take_samples(&mut c, &consumed, &discard, &mut out, false),
            36
        );
        // Everything pushed has now been consumed: this is what drain() waits for.
        assert_eq!(consumed.load(Ordering::Relaxed), 100);
    }

    #[test]
    fn a_flush_discards_stale_audio_and_still_counts_it() {
        let (_p, mut c) = ring(100);
        let (consumed, discard) = (AtomicU64::new(0), AtomicU64::new(40));
        let mut out = [0i32; 30];
        assert_eq!(
            take_samples(&mut c, &consumed, &discard, &mut out, false),
            30
        );
        assert_eq!(out[0], 40, "the first 40 samples were flushed, not played");
        assert_eq!(consumed.load(Ordering::Relaxed), 70);
    }

    #[test]
    fn a_flush_larger_than_one_buffer_is_discarded_across_calls() {
        let (_p, mut c) = ring(100);
        let (consumed, discard) = (AtomicU64::new(0), AtomicU64::new(90));
        let mut out = [0i32; 16];
        let got = take_samples(&mut c, &consumed, &discard, &mut out, false);
        assert_eq!(got, 10);
        assert_eq!(out[0], 90);
        assert_eq!(consumed.load(Ordering::Relaxed), 100);
    }

    #[test]
    fn audio_pushed_after_a_flush_is_played() {
        let (mut p, mut c) = ring(10);
        let (consumed, discard) = (AtomicU64::new(0), AtomicU64::new(10));
        let mut out = [0i32; 8];
        assert_eq!(
            take_samples(&mut c, &consumed, &discard, &mut out, false),
            0
        );
        p.push_slice(&[7, 8, 9]);
        assert_eq!(
            take_samples(&mut c, &consumed, &discard, &mut out, false),
            3
        );
        assert_eq!(&out[..3], &[7, 8, 9]);
    }

    #[test]
    fn paused_takes_nothing_from_the_ring() {
        let (_p, mut c) = ring(10);
        let (consumed, discard) = (AtomicU64::new(0), AtomicU64::new(0));
        let mut out = [0i32; 8];
        assert_eq!(take_samples(&mut c, &consumed, &discard, &mut out, true), 0);
        assert_eq!(consumed.load(Ordering::Relaxed), 0);
        assert_eq!(c.occupied_len(), 10);
    }
}
