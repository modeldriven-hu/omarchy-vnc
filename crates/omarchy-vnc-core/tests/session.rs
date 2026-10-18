//! Real TCP/RFB integration fixture: no external VNC server or GUI required.
use omarchy_vnc_core::{Input, Profile, Session, Status};
use std::{
    io::{Read, Write},
    net::TcpListener,
    sync::{Arc, mpsc},
    thread,
    time::{Duration, Instant},
};
fn exact(s: &mut std::net::TcpStream, n: usize) -> Vec<u8> {
    let mut b = vec![0; n];
    s.read_exact(&mut b).unwrap();
    b
}
fn until(mut f: impl FnMut() -> bool) {
    let end = Instant::now() + Duration::from_secs(5);
    while !f() {
        assert!(Instant::now() < end, "timed out");
        thread::sleep(Duration::from_millis(10));
    }
}
#[test]
fn framebuffer_input_clipboard_and_disconnect() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let (tx, rx) = mpsc::channel();
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        stream.write_all(b"RFB 003.008\n").unwrap();
        assert_eq!(exact(&mut stream, 12), b"RFB 003.008\n");
        stream.write_all(&[1, 2]).unwrap();
        assert_eq!(exact(&mut stream, 1), [2]);
        stream.write_all(&[0; 16]).unwrap();
        assert_eq!(
            exact(&mut stream, 16),
            [
                0x77, 0xdf, 0xa8, 0x1c, 0x9f, 0xd7, 0xb4, 0x07, 0x77, 0xdf, 0xa8, 0x1c, 0x9f, 0xd7,
                0xb4, 0x07
            ]
        );
        stream.write_all(&[0, 0, 0, 0]).unwrap();
        assert_eq!(exact(&mut stream, 1), [1]);
        let mut init = vec![];
        init.extend(2u16.to_be_bytes());
        init.extend(2u16.to_be_bytes());
        init.extend([32, 24, 0, 1, 0, 255, 0, 255, 0, 255, 16, 8, 0, 0, 0, 0]);
        init.extend(4u32.to_be_bytes());
        init.extend(b"test");
        stream.write_all(&init).unwrap();
        let mut pixels_sent = false;
        let mut observed_key = false;
        loop {
            let mut kind = [0];
            if stream.read_exact(&mut kind).is_err() {
                break;
            }
            match kind[0] {
                0 => {
                    exact(&mut stream, 19);
                }
                2 => {
                    let h = exact(&mut stream, 3);
                    let n = u16::from_be_bytes([h[1], h[2]]) as usize;
                    let encodings = exact(&mut stream, n * 4);
                    assert!(
                        encodings
                            .as_chunks::<4>()
                            .0
                            .iter()
                            .any(|e| i32::from_be_bytes(*e) == -239),
                        "RichCursor must be negotiated"
                    );
                }
                3 => {
                    exact(&mut stream, 9);
                    if !pixels_sent {
                        let mut update = vec![0, 0, 0, 2];
                        update.extend([0, 0, 0, 0, 0, 2, 0, 2, 0, 0, 0, 0]);
                        update.extend([1, 2, 3, 0].repeat(4));
                        update.extend([0, 1, 0, 1, 0, 2, 0, 2]);
                        update.extend((-239i32).to_be_bytes());
                        update.extend([4, 5, 6, 0].repeat(4));
                        update.extend([0xc0, 0x80]);
                        stream.write_all(&update).unwrap();
                        stream
                            .write_all(&[3, 0, 0, 0, 0, 0, 0, 2, b'h', b'i'])
                            .unwrap();
                        pixels_sent = true;
                    }
                }
                4 => {
                    let e = exact(&mut stream, 7);
                    if e[0] == 1 && e[3..7] == [0, 0, 0, 97] {
                        observed_key = true;
                        tx.send(()).unwrap();
                    }
                }
                5 => {
                    exact(&mut stream, 5);
                }
                6 => {
                    let h = exact(&mut stream, 7);
                    let n = u32::from_be_bytes(h[3..7].try_into().unwrap()) as usize;
                    let text = exact(&mut stream, n);
                    assert_eq!(text, b"local");
                    assert!(observed_key);
                    tx.send(()).unwrap();
                }
                n => panic!("unexpected message {n}"),
            }
        }
    });
    let profile = Profile {
        host: "127.0.0.1".into(),
        port,
        clipboard: true,
        ..Profile::default()
    };
    let mut session = Session::connect(profile, "test".into(), Arc::new(|| {})).unwrap();
    until(|| session.snapshot().frame.is_some());
    let snapshot = session.snapshot();
    assert_eq!(snapshot.status, Status::Connected);
    assert_eq!(&snapshot.frame.unwrap().rgba[..4], &[3, 2, 1, 255]);
    let session_id = session.id();
    let before = session.snapshot().frame.unwrap();
    let wakes = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let count = wakes.clone();
    session.set_waker(Arc::new(move || {
        count.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }));
    assert_eq!(session.id(), session_id);
    assert!(Arc::ptr_eq(&before, &session.snapshot().frame.unwrap()));
    assert!(wakes.load(std::sync::atomic::Ordering::Relaxed) > 0);
    until(|| session.snapshot().cursor.is_some());
    let cursor = session.snapshot().cursor.unwrap();
    assert_eq!(cursor.hotspot, [1, 1]);
    assert_eq!(&cursor.rgba[..4], &[6, 5, 4, 255]);
    assert_eq!(cursor.rgba[15], 0);
    until(|| session.snapshot().clipboard.is_some());
    assert_eq!(session.take_clipboard(), Some("hi".into()));
    session.send(Input::Key(97, true));
    rx.recv_timeout(Duration::from_secs(3)).unwrap();
    session.send(Input::Clipboard("local".into()));
    rx.recv_timeout(Duration::from_secs(3)).unwrap();
    drop(session);
    server.join().unwrap();
}
#[test]
fn cancel_during_stalled_handshake() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = thread::spawn(move || {
        let (mut s, _) = listener.accept().unwrap();
        s.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
        let mut b = [0];
        let _ = s.read(&mut b);
    });
    let session = Session::connect(
        Profile {
            host: "127.0.0.1".into(),
            port,
            ..Profile::default()
        },
        String::new(),
        Arc::new(|| {}),
    )
    .unwrap();
    thread::sleep(Duration::from_millis(100));
    let start = Instant::now();
    drop(session);
    assert!(start.elapsed() < Duration::from_secs(2));
    server.join().unwrap();
}
#[test]
fn authentication_failure_is_not_connected() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = thread::spawn(move || {
        let (mut s, _) = listener.accept().unwrap();
        s.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
        s.write_all(b"RFB 003.008\n").unwrap();
        exact(&mut s, 12);
        s.write_all(&[1, 2]).unwrap();
        exact(&mut s, 1);
        s.write_all(&[0; 16]).unwrap();
        exact(&mut s, 16);
        s.write_all(&[0, 0, 0, 1, 0, 0, 0, 6, b'd', b'e', b'n', b'i', b'e', b'd'])
            .unwrap();
    });
    let session = Session::connect(
        Profile {
            host: "127.0.0.1".into(),
            port,
            ..Profile::default()
        },
        "wrong".into(),
        Arc::new(|| {}),
    )
    .unwrap();
    until(|| matches!(session.snapshot().status, Status::Failed(_)));
    assert!(session.snapshot().frame.is_none());
    drop(session);
    server.join().unwrap();
}

#[test]
fn apple_authentication_negotiation() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = thread::spawn(move || {
        let (mut s, _) = listener.accept().unwrap();
        s.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        s.write_all(b"RFB 003.889\n").unwrap();
        let banner = exact(&mut s, 12);
        assert!(banner.starts_with(b"RFB 003."));
        s.write_all(&[4, 30, 33, 36, 35]).unwrap();
        assert_eq!(exact(&mut s, 1), [30]);
        let hex = "FFFFFFFFFFFFFFFFC90FDAA22168C234C4C6628B80DC1CD129024E088A67CC74020BBEA63B139B22514A08798E3404DDEF9519B3CD3A431B302B0A6DF25F14374FE1356D6D51C245E485B576625E7EC6F44C42E9A637ED6B0BFF5CB6F406B7EDEE386BFB5A899FA5AE9F24117C4B1FE649286651ECE65381FFFFFFFFFFFFFFFF";
        let prime: Vec<u8> = (0..hex.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap())
            .collect();
        s.write_all(&[0, 2, 0, 128]).unwrap();
        s.write_all(&prime).unwrap();
        let mut public = vec![0; 128];
        public[127] = 128;
        s.write_all(&public).unwrap();
        let response = exact(&mut s, 256);
        assert!(response.iter().any(|b| *b != 0));
        assert!(!response.windows(11).any(|w| w == b"sample-user"));
        // Reject deliberately: tests negotiation and encrypted response, not actual Mac credentials.
        s.write_all(&[0, 0, 0, 1, 0, 0, 0, 6, b'd', b'e', b'n', b'i', b'e', b'd'])
            .unwrap();
    });
    let session = Session::connect(
        Profile {
            host: "127.0.0.1".into(),
            port,
            username: "sample-user".into(),
            ..Profile::default()
        },
        "sample-pass".into(),
        Arc::new(|| {}),
    )
    .unwrap();
    until(|| matches!(session.snapshot().status, Status::Failed(_)));
    assert!(session.snapshot().frame.is_none());
    drop(session);
    server.join().unwrap();
}
