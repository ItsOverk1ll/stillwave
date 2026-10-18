// SPDX-License-Identifier: GPL-3.0-only
//! File decoder on Symphonia.
//!
//! Integer PCM (WAV, AIFF, FLAC, ALAC) is shifted into the top bits of an `i32`
//! with plain shifts: no dither, no gain, no rounding, so decoding is exactly
//! reversible and can be compared bit-for-bit with a reference. Lossy codecs
//! decode to floating point and are converted at 24-bit scale; they are flagged
//! as lossy and are never bit-perfect.

use std::fs::File;
use std::path::Path;

use symphonia::core::audio::GenericAudioBufferRef;
use symphonia::core::codecs::audio::well_known::{
    CODEC_ID_ALAC, CODEC_ID_FLAC, CODEC_ID_MONKEYS_AUDIO, CODEC_ID_TTA, CODEC_ID_WAVPACK,
};
use symphonia::core::codecs::audio::{AudioDecoder, AudioDecoderOptions};
use symphonia::core::errors::Error as SymError;
use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::{FormatOptions, FormatReader, SeekMode, SeekTo, TrackType};
use symphonia::core::io::{MediaSourceStream, MediaSourceStreamOptions};
use symphonia::core::meta::MetadataOptions;
use symphonia::core::units::Timestamp;

use crate::format::StreamFormat;
use crate::traits::{AudioError, Decoder};

/// Decodes one audio file to interleaved, left-aligned `i32` samples.
pub struct FileDecoder {
    format: Box<dyn FormatReader>,
    decoder: Box<dyn AudioDecoder>,
    track_id: u32,
    stream: StreamFormat,
    total_frames: Option<u64>,
    lossy: bool,
    /// Decoded samples not yet handed out.
    pending: Vec<i32>,
    pending_pos: usize,
    /// Packets that failed to decode and were skipped (corrupt data).
    skipped_packets: u32,
    finished: bool,
}

impl FileDecoder {
    pub fn open(path: &Path) -> Result<Self, AudioError> {
        let file =
            File::open(path).map_err(|e| AudioError::Decode(format!("{}: {e}", path.display())))?;
        let mss = MediaSourceStream::new(Box::new(file), MediaSourceStreamOptions::default());
        let mut hint = Hint::new();
        if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
            hint.with_extension(ext);
        }
        let format = symphonia::default::get_probe()
            .probe(
                &hint,
                mss,
                FormatOptions::default(),
                MetadataOptions::default(),
            )
            .map_err(|e| AudioError::Unsupported(format!("{}: {e}", path.display())))?;

        let track = format
            .default_track(TrackType::Audio)
            .ok_or_else(|| AudioError::Unsupported("no audio track".into()))?;
        let params = track
            .codec_params
            .as_ref()
            .and_then(|p| p.audio())
            .ok_or_else(|| AudioError::Unsupported("missing audio parameters".into()))?;
        let sample_rate = params
            .sample_rate
            .ok_or_else(|| AudioError::Unsupported("unknown sample rate".into()))?;
        let channels = params
            .channels
            .as_ref()
            .map(symphonia::core::audio::Channels::count)
            .ok_or_else(|| AudioError::Unsupported("unknown channel layout".into()))?;
        let reported_bits = params
            .bits_per_sample
            .or(params.bits_per_coded_sample)
            .or_else(|| alac_bit_depth(params));
        // PCM-like containers report a depth. Compressed lossless codecs may not
        // (ALAC does not), so they are recognised by codec instead.
        let lossless_codec = [
            CODEC_ID_FLAC,
            CODEC_ID_ALAC,
            CODEC_ID_WAVPACK,
            CODEC_ID_MONKEYS_AUDIO,
            CODEC_ID_TTA,
        ]
        .contains(&params.codec);
        let lossy = reported_bits.is_none() && !lossless_codec;
        // 0 = learn it from the first decoded buffer (done below).
        let bit_depth = if lossy {
            24
        } else {
            reported_bits.map_or(0, |b| u8::try_from(b).unwrap_or(32))
        };
        let stream = StreamFormat::new(
            sample_rate,
            bit_depth,
            u8::try_from(channels)
                .map_err(|_| AudioError::Unsupported("too many channels".into()))?,
        );
        let total_frames = track.num_frames;
        let track_id = track.id;

        let decoder = symphonia::default::get_codecs()
            .make_audio_decoder(params, &AudioDecoderOptions::default())
            .map_err(|e| AudioError::Unsupported(format!("codec: {e}")))?;

        let mut this = Self {
            format,
            decoder,
            track_id,
            stream,
            total_frames,
            lossy,
            pending: Vec::new(),
            pending_pos: 0,
            skipped_packets: 0,
            finished: false,
        };
        if this.stream.bit_depth == 0 {
            // Decode the first packet now; its sample type tells us the depth.
            // The samples stay queued, so nothing is lost.
            if !this.fill()? {
                this.finished = true;
            }
            if this.stream.bit_depth == 0 {
                this.stream.bit_depth = 16;
            }
        }
        Ok(this)
    }

    /// Packets skipped because they were corrupt. Zero for a healthy file.
    #[must_use]
    pub fn skipped_packets(&self) -> u32 {
        self.skipped_packets
    }

    /// Decodes the next packet of our track into `pending`. Returns false at end of stream.
    fn fill(&mut self) -> Result<bool, AudioError> {
        self.pending.clear();
        self.pending_pos = 0;
        loop {
            let packet = match self.format.next_packet() {
                Ok(Some(p)) => p,
                // End of stream, or a chained stream we do not follow (treated as the end).
                Ok(None) | Err(SymError::ResetRequired) => return Ok(false),
                Err(e) => return Err(AudioError::Decode(e.to_string())),
            };
            if packet.track_id != self.track_id {
                continue;
            }
            match self.decoder.decode(&packet) {
                Ok(buf) => {
                    let native = append_left_aligned(&buf, &mut self.pending);
                    if self.stream.bit_depth == 0 {
                        self.stream.bit_depth = native;
                    }
                    if self.pending.is_empty() {
                        continue;
                    }
                    return Ok(true);
                }
                Err(SymError::DecodeError(_) | SymError::IoError(_)) => {
                    self.skipped_packets += 1;
                }
                Err(e) => return Err(AudioError::Decode(e.to_string())),
            }
        }
    }
}

/// ALAC keeps its bit depth in the 24-byte `ALACSpecificConfig` (byte 5), not in
/// the generic codec parameters, and its decoder always yields 32-bit buffers.
fn alac_bit_depth(params: &symphonia::core::codecs::audio::AudioCodecParameters) -> Option<u32> {
    if params.codec != CODEC_ID_ALAC {
        return None;
    }
    let extra = params.extra_data.as_deref()?;
    let config = extra.get(extra.len().checked_sub(24)?..)?;
    Some(u32::from(config[5])).filter(|d| matches!(d, 16 | 20 | 24 | 32))
}

/// Appends `buf` as interleaved left-aligned `i32` and returns the buffer's
/// native sample width in bits.
fn append_left_aligned(buf: &GenericAudioBufferRef<'_>, out: &mut Vec<i32>) -> u8 {
    match buf {
        GenericAudioBufferRef::S16(_) => {
            let mut v: Vec<i16> = Vec::new();
            buf.copy_to_vec_interleaved(&mut v);
            out.extend(v.into_iter().map(|s| i32::from(s) << 16));
            16
        }
        GenericAudioBufferRef::S24(_) => {
            let mut v: Vec<symphonia::core::audio::sample::i24> = Vec::new();
            buf.copy_to_vec_interleaved(&mut v);
            out.extend(v.into_iter().map(|s| s.inner() << 8));
            24
        }
        GenericAudioBufferRef::S32(_) => {
            let mut v: Vec<i32> = Vec::new();
            buf.copy_to_vec_interleaved(&mut v);
            out.extend(v);
            32
        }
        GenericAudioBufferRef::S8(_) => {
            let mut v: Vec<i8> = Vec::new();
            buf.copy_to_vec_interleaved(&mut v);
            out.extend(v.into_iter().map(|s| i32::from(s) << 24));
            8
        }
        GenericAudioBufferRef::U8(_) => {
            let mut v: Vec<u8> = Vec::new();
            buf.copy_to_vec_interleaved(&mut v);
            out.extend(v.into_iter().map(|s| (i32::from(s) - 128) << 24));
            8
        }
        // Everything else (floats from lossy codecs, exotic widths) goes through f32.
        _ => {
            let mut v: Vec<f32> = Vec::new();
            buf.copy_to_vec_interleaved(&mut v);
            #[allow(clippy::cast_possible_truncation)]
            out.extend(v.into_iter().map(|s| {
                // 24-bit resolution, then left-align. Clamp so a hot master cannot wrap.
                let q = (f64::from(s) * 8_388_608.0)
                    .round()
                    .clamp(-8_388_608.0, 8_388_607.0);
                (q as i32) << 8
            }));
            24
        }
    }
}

impl Decoder for FileDecoder {
    fn format(&self) -> StreamFormat {
        self.stream
    }

    fn total_frames(&self) -> Option<u64> {
        self.total_frames
    }

    fn is_lossy(&self) -> bool {
        self.lossy
    }

    fn read(&mut self, out: &mut [i32]) -> Result<usize, AudioError> {
        let mut written = 0;
        while written < out.len() {
            if self.pending_pos >= self.pending.len() && (self.finished || !self.fill()?) {
                self.finished = true;
                break;
            }
            let n = (out.len() - written).min(self.pending.len() - self.pending_pos);
            out[written..written + n]
                .copy_from_slice(&self.pending[self.pending_pos..self.pending_pos + n]);
            self.pending_pos += n;
            written += n;
        }
        Ok(written)
    }

    fn seek(&mut self, frame: u64) -> Result<(), AudioError> {
        let ts =
            i64::try_from(frame).map_err(|_| AudioError::Decode("seek out of range".into()))?;
        self.format
            .seek(
                SeekMode::Accurate,
                SeekTo::Timestamp {
                    ts: Timestamp::new(ts),
                    track_id: self.track_id,
                },
            )
            .map_err(|e| AudioError::Decode(format!("seek: {e}")))?;
        self.decoder.reset();
        self.pending.clear();
        self.pending_pos = 0;
        self.finished = false;
        Ok(())
    }
}
