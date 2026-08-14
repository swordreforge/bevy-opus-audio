//! Play a `.opus` file out loud (loop).
//!
//! ```sh
//! cargo run --example play --release
//! ```
//!
//! Put a file at `assets/music.opus` first, or edit the path below.

use bevy::asset::AssetPlugin;
use bevy::audio::{AudioPlayer, AudioPlugin, PlaybackSettings};
use bevy::prelude::*;
use bevy_opus_audio::{OpusAudio, OpusAudioPlugin};

fn main() {
    App::new()
        .add_plugins((
            MinimalPlugins,
            AssetPlugin::default(),
            AudioPlugin::default(),
            OpusAudioPlugin,
        ))
        .add_systems(Startup, |mut commands: Commands, server: Res<AssetServer>| {
            let handle = server.load::<OpusAudio>("music.opus");
            commands.spawn((AudioPlayer::<OpusAudio>(handle), PlaybackSettings::LOOP));
        })
        .run();
}
