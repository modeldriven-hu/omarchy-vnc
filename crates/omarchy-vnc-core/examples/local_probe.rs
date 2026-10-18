//! Verify a local-only, passwordless development VNC session without a graphical frontend.
use omarchy_vnc_core::{Input, Profile, Session, Status};
use std::{
    sync::Arc,
    thread,
    time::{Duration, Instant},
};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let port = std::env::args()
        .nth(1)
        .map(|v| v.parse())
        .transpose()?
        .unwrap_or(5909);
    let profile = Profile {
        host: "127.0.0.1".into(),
        port,
        map_ctrl_to_command: false,
        ..Profile::default()
    };
    let mut session = Session::connect(profile, String::new(), Arc::new(|| {}))?;
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        let snapshot = session.snapshot();
        if let Status::Failed(error) = snapshot.status {
            return Err(error.into());
        }
        if snapshot.status == Status::Connected {
            session.send(Input::Pointer(100, 100, 0));
        }
        if let (Some(frame), Some(cursor)) = (snapshot.frame, snapshot.cursor) {
            println!(
                "Connected: {}×{} framebuffer; {}×{} cursor, hotspot {:?}",
                frame.width, frame.height, cursor.width, cursor.height, cursor.hotspot
            );
            return Ok(());
        }
        thread::sleep(Duration::from_millis(25));
    }
    Err("Timed out waiting for framebuffer and remote cursor".into())
}
