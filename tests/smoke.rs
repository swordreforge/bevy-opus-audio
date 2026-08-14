//! End-to-end smoke test for the Bevy integration: plugin registration,
//! `.opus` asset loading, decoding, and `AudioPlayer` spawning.
//!
//! Runs headless — `AudioOutput` degrades to a warning when no audio device
//! is present, so this works in CI without a sound card.

use std::time::{Duration, Instant};

use bevy::asset::AssetPlugin;
use bevy::audio::{AudioPlayer, AudioPlugin, Decodable, PlaybackSettings};
use bevy::prelude::*;
use bevy_opus_audio::{OpusAudio, OpusAudioPlugin};

#[test]
fn end_to_end_playback_smoke() {
    let fixtures = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    std::env::set_var("BEVY_ASSET_ROOT", fixtures);

    let mut app = App::new();
    app.add_plugins((
        MinimalPlugins,
        AssetPlugin {
            file_path: String::new(),
            ..default()
        },
        AudioPlugin::default(),
        OpusAudioPlugin,
    ));

    let handle = app
        .world()
        .resource::<AssetServer>()
        .load::<OpusAudio>("voice_mono.opus");
    app.world_mut()
        .spawn((AudioPlayer::<OpusAudio>(handle.clone()), PlaybackSettings::DESPAWN));

    let deadline = Instant::now() + Duration::from_secs(5);
    let mut loaded = None;
    while Instant::now() < deadline {
        app.update();
        if let Some(asset) = app.world().resource::<Assets<OpusAudio>>().get(&handle) {
            loaded = Some(asset.clone());
            break;
        }
    }

    let asset = loaded.expect("OpusAudio asset failed to load within timeout");
    let samples = asset.decoder().count();
    assert!(samples > 0, "OpusSource produced no samples");
}
