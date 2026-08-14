# bevy-opus-audio

Pure-Rust Ogg-Opus audio decoding for [Bevy](https://bevyengine.org), backed by
[`rusty-opus`](https://crates.io/crates/rusty-opus). No C, no FFI, no `libopus`.

Bevy 0.19 has no built-in Opus support — `bevy_audio`'s feature flags stop at
vorbis/flac/mp3/aac/wav, and rodio's `symphonia-libopus` links the C `libopus`.
This crate registers a custom [`Decodable`] asset so the whole pipeline stays
pure Rust on **native and wasm**.

## Features

- **Zero C/FFI** — no `libopus`, no `bindgen`, no build-time toolchain.
- **Streaming decode** — Ogg packets are demuxed and decoded lazily, so memory
  stays low even for long BGM tracks.
- **Loop-compatible** — works with `PlaybackMode::Loop` (`repeat_infinite`).
- **wasm-friendly** — compiles to `wasm32-unknown-unknown` (scalar fallback,
  no SIMD required).

## Installation

```toml
[dependencies]
bevy-opus-audio = "0.1"
```

## Usage

```rust
use bevy::prelude::*;
use bevy_opus_audio::{OpusAudio, OpusAudioPlugin};

App::new()
    .add_plugins((DefaultPlugins, OpusAudioPlugin))
    .add_systems(Startup, |mut commands: Commands, server: Res<AssetServer>| {
        let handle = server.load::<OpusAudio>("audio/bgm/title.opus");
        commands.spawn((AudioPlayer::<OpusAudio>(handle), PlaybackSettings::LOOP));
    })
    .run();
```

## How it works

`OpusAudioPlugin` registers three pieces:

- **`OpusAudio`** — a decodable [`Asset`] for `.opus` files. The `OpusHead`
  identification header is parsed at load time to capture channel count,
  sample rate, and pre-skip.
- **`OpusAudioLoader`** — an [`AssetLoader`] for the `.opus` extension.
- **`OpusSource`** — a streaming [`rodio::Source`] that demuxes Ogg packets and
  decodes them to interleaved `f32` samples, skipping the encoder's pre-skip
  delay at the start.

## Compatibility

| Bevy | bevy-opus-audio |
|------|-----------------|
| 0.19 | 0.1             |

## License

MIT OR Apache-2.0 (the same dual license as Bevy itself).
