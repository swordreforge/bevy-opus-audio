//! Pure-Rust Ogg-Opus audio decoding for [Bevy], backed by [`rusty-opus`].
//!
//! Bevy 0.19 has no Opus support out of the box — `bevy_audio`'s feature flags
//! stop at vorbis/flac/mp3/aac/wav, and rodio's `symphonia-libopus` links the
//! C `libopus`. This crate registers a custom [`Decodable`] asset type so the
//! whole pipeline stays pure Rust (no C, no FFI) on native *and* wasm.
//!
//! # Usage
//!
//! ```no_run
//! use bevy::prelude::*;
//! use bevy_opus_audio::{OpusAudio, OpusAudioPlugin};
//!
//! App::new()
//!     .add_plugins((DefaultPlugins, OpusAudioPlugin))
//!     .add_systems(Startup, |mut commands: Commands, server: Res<AssetServer>| {
//!         let handle = server.load::<OpusAudio>("audio/bgm/title.opus");
//!         commands.spawn((AudioPlayer::<OpusAudio>(handle), PlaybackSettings::LOOP));
//!     })
//!     .run();
//! ```
//!
//! [Bevy]: https://bevyengine.org

use std::io::Cursor;
use std::sync::Arc;
use std::time::Duration;

use bevy::asset::io::Reader;
use bevy::asset::{AssetLoader, LoadContext};
use bevy::audio::{AddAudioSource, ChannelCount, Decodable, SampleRate, Source};
use bevy::prelude::*;
use ogg::PacketReader;
use rusty_opus::repacketizer;
use rusty_opus::OpusDecoder;

const VALID_RATES: [u32; 5] = [8000, 12000, 16000, 24000, 48000];

/// A decoded Opus asset: the raw `.opus` bytes plus the parsed identification
/// header fields needed to build a decoder.
#[derive(Asset, TypePath, Debug, Clone)]
pub struct OpusAudio {
    bytes: Arc<[u8]>,
    channels: u16,
    sample_rate: u32,
    pre_skip: u16,
}

fn parse_opus_head(bytes: &[u8]) -> Result<(u16, u32, u16), &'static str> {
    let mut reader = PacketReader::new(Cursor::new(bytes));
    let head = reader
        .read_packet()
        .map_err(|_| "failed to read Ogg page")?
        .ok_or("empty Ogg stream")?;
    let h = &head.data;
    if h.len() < 19 || &h[..8] != b"OpusHead" {
        return Err("not an Ogg-Opus stream (missing OpusHead)");
    }
    let channels = h[9] as u16;
    let sample_rate = u32::from_le_bytes([h[12], h[13], h[14], h[15]]);
    // The OpusHead spec calls pre-skip big-endian, but libopus/ffmpeg and
    // rusty-opus all write it little-endian (verified: 0x38 0x01 = 312).
    let pre_skip = u16::from_le_bytes([h[10], h[11]]);
    if !(1..=2).contains(&channels) || !VALID_RATES.contains(&sample_rate) {
        return Err("unsupported Opus channels or sample rate");
    }
    Ok((channels, sample_rate, pre_skip))
}

/// Streaming [`Source`] that lazily demuxes Ogg packets and decodes them with
/// `rusty-opus`, yielding interleaved `f32` samples.
pub struct OpusSource {
    packets: PacketReader<Cursor<Arc<[u8]>>>,
    decoder: OpusDecoder,
    sample_rate: u32,
    channels: u16,
    pre_skip: usize,
    buf: Vec<f32>,
    pos: usize,
    done: bool,
}

impl OpusSource {
    fn new(audio: &OpusAudio) -> Self {
        let mut packets = PacketReader::new(Cursor::new(audio.bytes.clone()));
        let _ = packets.read_packet();
        let _ = packets.read_packet();
        Self {
            decoder: OpusDecoder::new(audio.sample_rate as i32, audio.channels as usize)
                .expect("channels/sample rate validated at load"),
            packets,
            sample_rate: audio.sample_rate,
            channels: audio.channels,
            pre_skip: audio.pre_skip as usize,
            buf: Vec::new(),
            pos: 0,
            done: false,
        }
    }
}

impl Iterator for OpusSource {
    type Item = f32;

    fn next(&mut self) -> Option<f32> {
        loop {
            if self.pos < self.buf.len() {
                let s = self.buf[self.pos];
                self.pos += 1;
                return Some(s);
            }
            if self.done {
                return None;
            }

            self.buf.clear();
            self.pos = 0;

            let packet = match self.packets.read_packet() {
                Ok(Some(p)) => p,
                _ => {
                    self.done = true;
                    continue;
                }
            };

            let frame_size = match opus_frame_samples(&packet.data, self.sample_rate) {
                Some(n) => n,
                None => continue,
            };
            let ch = self.channels as usize;
            let mut out = vec![0.0f32; frame_size * ch];
            match self.decoder.decode(&packet.data, frame_size, &mut out) {
                Ok(n) => {
                    let samples = &out[..n * ch];
                    if self.pre_skip > 0 {
                        let skip = self.pre_skip.min(n);
                        let start = skip * ch;
                        if start < samples.len() {
                            self.buf.extend_from_slice(&samples[start..]);
                        }
                        self.pre_skip -= skip;
                    } else {
                        self.buf.extend_from_slice(samples);
                    }
                }
                Err(_) => {}
            }
        }
    }
}

fn opus_frame_samples(packet: &[u8], sample_rate: u32) -> Option<usize> {
    if packet.is_empty() {
        return None;
    }
    let toc = packet[0];
    let frames = repacketizer::nb_frames(packet).ok()? as usize;
    let per_frame = repacketizer::samples_per_frame(toc, sample_rate as i32) as usize;
    Some(frames * per_frame)
}

impl Source for OpusSource {
    fn current_span_len(&self) -> Option<usize> {
        if self.done {
            Some(0)
        } else {
            None
        }
    }

    fn channels(&self) -> ChannelCount {
        ChannelCount::new(self.channels).expect("validated at load")
    }

    fn sample_rate(&self) -> SampleRate {
        SampleRate::new(self.sample_rate).expect("validated at load")
    }

    fn total_duration(&self) -> Option<Duration> {
        None
    }
}

impl Decodable for OpusAudio {
    type Decoder = OpusSource;

    fn decoder(&self) -> Self::Decoder {
        OpusSource::new(self)
    }
}

/// [`AssetLoader`] for `.opus` files — parses the OpusHead header up front so
/// the decoder is built with the correct channel count / sample rate / pre-skip.
#[derive(TypePath)]
pub struct OpusAudioLoader;

impl AssetLoader for OpusAudioLoader {
    type Asset = OpusAudio;
    type Settings = ();
    type Error = std::io::Error;

    async fn load(
        &self,
        reader: &mut dyn Reader,
        _settings: &Self::Settings,
        _load_context: &mut LoadContext<'_>,
    ) -> Result<Self::Asset, Self::Error> {
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes).await?;
        let (channels, sample_rate, pre_skip) = parse_opus_head(&bytes)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
        Ok(OpusAudio {
            bytes: bytes.into(),
            channels,
            sample_rate,
            pre_skip,
        })
    }

    fn extensions(&self) -> &[&str] {
        &["opus"]
    }
}

/// Registers [`OpusAudio`] as a decodable audio source and its `.opus` loader.
pub struct OpusAudioPlugin;

impl Plugin for OpusAudioPlugin {
    fn build(&self, app: &mut App) {
        app.add_audio_source::<OpusAudio>()
            .register_asset_loader(OpusAudioLoader);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> Vec<u8> {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(name);
        std::fs::read(path).expect("test fixture present")
    }

    fn decode_all(bytes: Vec<u8>) -> (u16, u32, usize) {
        let (channels, sample_rate, pre_skip) = parse_opus_head(&bytes).unwrap();
        let audio = OpusAudio {
            bytes: bytes.into(),
            channels,
            sample_rate,
            pre_skip,
        };
        let source = OpusSource::new(&audio);
        let total = source.count();
        (channels, sample_rate, total / channels as usize)
    }

    #[test]
    fn decodes_mono_voice() {
        let (channels, sample_rate, per_channel) = decode_all(fixture("voice_mono.opus"));
        assert_eq!(channels, 1);
        assert_eq!(sample_rate, 48000);
        assert!((6000..=7000).contains(&per_channel), "got {per_channel} samples/ch");
    }

    #[test]
    fn decodes_stereo_se() {
        let (channels, sample_rate, per_channel) = decode_all(fixture("se_stereo.opus"));
        assert_eq!(channels, 2);
        assert_eq!(sample_rate, 48000);
        assert!((10000..=11000).contains(&per_channel), "got {per_channel} samples/ch");
    }

    #[test]
    fn loops_stereo_bgm_without_silence() {
        let bytes = fixture("se_stereo.opus");
        let (channels, sample_rate, pre_skip) = parse_opus_head(&bytes).unwrap();
        let audio = OpusAudio {
            bytes: bytes.into(),
            channels,
            sample_rate,
            pre_skip,
        };
        let source = OpusSource::new(&audio);
        let mut repeating = source.repeat_infinite();

        let target = sample_rate as usize * 2;
        let mut sum = 0.0f32;
        let mut count = 0usize;
        for _ in 0..target {
            match repeating.next() {
                Some(s) => {
                    sum += s.abs();
                    count += 1;
                }
                None => break,
            }
        }

        assert_eq!(count, target, "repeat_infinite stopped early at {count} samples");
        assert!(sum > 0.0, "repeat_infinite produced only silence");
    }
}
