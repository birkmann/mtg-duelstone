//! Owns the engine link while a match is loading/running: drains `LinkEvent`s into `ServerMsg`
//! messages (+ `LinkStatus`) and forwards every `SendCmd` to the socket.
//!
//! The `NetLink` resource is inserted by `app_state` when a match is created (or a replay is
//! opened) and removed when it ends; everything here is a no-op without it.

use bevy::prelude::*;
use mtg_client::{EngineLink, LinkEvent};
use mtg_protocol::ClientFrame;

use crate::shared::*;

pub struct NetPlugin;

impl Plugin for NetPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<LinkStatus>()
            .add_systems(PreUpdate, drain_link.run_if(resource_exists::<NetLink>))
            .add_systems(Update, send_cmds.run_if(resource_exists::<NetLink>).after(crate::view::ViewSet));
    }
}

/// The live engine connection. Owner: net (inserted by app_state).
#[derive(Resource)]
pub struct NetLink(pub EngineLink);

fn drain_link(link: Res<NetLink>, mut status: ResMut<LinkStatus>, mut out: MessageWriter<ServerMsg>) {
    // Bound the work per frame so a flood of frames can't stall rendering, but keep up with the
    // engine: states are coalesced server-side.
    for _ in 0..512 {
        match link.0.rx.try_recv() {
            Ok(LinkEvent::Frame(f)) => {
                out.write(ServerMsg(f));
            }
            Ok(LinkEvent::Connected) => *status = LinkStatus::Connected,
            Ok(LinkEvent::Disconnected(why)) => {
                info!("engine link down: {why}");
                *status = LinkStatus::Disconnected;
            }
            Err(_) => break,
        }
    }
}

fn send_cmds(link: Res<NetLink>, mut cmds: MessageReader<SendCmd>) {
    for SendCmd(frame) in cmds.read() {
        if matches!(frame, ClientFrame::Ping) {
            continue;
        }
        link.0.send(frame.clone());
    }
}
