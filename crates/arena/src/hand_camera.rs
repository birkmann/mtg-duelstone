//! Orthographic overlay camera for the hand (and the dragged card). It renders after the table
//! camera without clearing, so hand cards are always on top and screen-aligned, never clipping
//! into the 3D table. Mesh picking sees both cameras and prefers the higher `order` one.

use bevy::camera::visibility::RenderLayers;
use bevy::camera::{ClearColorConfig, OrthographicProjection, ScalingMode};
use bevy::core_pipeline::tonemapping::Tonemapping;
use bevy::light::DirectionalLight;
use bevy::prelude::*;

use crate::layout::HAND_VIEW_H;
use crate::shared::*;

pub struct HandCameraPlugin;

impl Plugin for HandCameraPlugin {
    fn build(&self, app: &mut App) {
        if !app.is_plugin_added::<MeshPickingPlugin>() {
            app.add_plugins(MeshPickingPlugin);
        }
        app.add_systems(Startup, spawn_hand_camera);
    }
}

fn spawn_hand_camera(mut commands: Commands) {
    if std::env::var("ARENA_NO_HAND").is_ok() {
        return;
    }
    let layers = RenderLayers::layer(HAND_LAYER);
    commands.spawn((
        HandCamera,
        Camera3d::default(),
        Camera { order: 10, clear_color: ClearColorConfig::None, ..default() },
        Projection::Orthographic(OrthographicProjection {
            scaling_mode: ScalingMode::FixedVertical { viewport_height: HAND_VIEW_H },
            near: 0.0,
            far: 100.0,
            ..OrthographicProjection::default_3d()
        }),
        Transform::from_xyz(0.0, 0.0, 20.0).looking_to(Vec3::NEG_Z, Vec3::Y),
        Tonemapping::TonyMcMapface,
        Msaa::Sample4,
        layers.clone(),
        Name::new("hand camera"),
    ));
    // Hand cards are lit by their own light so the table's key light/shadows don't touch them.
    commands.spawn((
        DirectionalLight { color: Color::srgb(1.0, 0.97, 0.92), illuminance: 4200.0, shadow_maps_enabled: false, ..default() },
        Transform::default().looking_to(Vec3::new(0.2, -0.3, -1.0), Vec3::Y),
        layers,
        Name::new("hand light"),
    ));
}
