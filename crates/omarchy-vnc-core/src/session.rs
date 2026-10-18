use crate::{Frame, Profile, native::Client};
use anyhow::{Context, Result, bail};
use std::{
    collections::BTreeSet,
    net::{Shutdown, TcpStream},
    os::fd::AsRawFd,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc::{self, SyncSender},
    },
    thread,
    time::{Duration, Instant},
};
use zeroize::Zeroizing;
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Status {
    Connecting,
    Connected,
    Disconnected,
    Failed(String),
}
#[derive(Clone)]
pub struct Snapshot {
    pub status: Status,
    pub frame: Option<Arc<Frame>>,
    pub clipboard: Option<String>,
    pub cursor: Option<Arc<crate::Cursor>>,
}
pub enum Input {
    Key(u32, bool),
    Pointer(u16, u16, u8),
    Release,
    Clipboard(String),
    Refresh,
}
type Wake = Arc<dyn Fn() + Send + Sync>;

pub struct Session {
    waker: Arc<Mutex<Wake>>,
    id: u64,
    state: Arc<Mutex<Snapshot>>,
    input: SyncSender<Input>,
    cancel: Arc<AtomicBool>,
    socket: Arc<Mutex<Option<TcpStream>>>,
    worker: Option<thread::JoinHandle<()>>,
}
impl Session {
    pub fn connect(
        profile: Profile,
        password: String,
        wake: Arc<dyn Fn() + Send + Sync>,
    ) -> Result<Self> {
        let password = Zeroizing::new(password);
        profile.validate()?;
        if password.as_bytes().contains(&0) || password.len() > 127 {
            bail!("Password must be at most 127 bytes and contain no NUL")
        }
        let state = Arc::new(Mutex::new(Snapshot {
            status: Status::Connecting,
            frame: None,
            clipboard: None,
            cursor: None,
        }));
        let (input, rx) = mpsc::sync_channel(256);
        let cancel = Arc::new(AtomicBool::new(false));
        let socket = Arc::new(Mutex::new(None));
        let waker = Arc::new(Mutex::new(wake));
        let handler = waker.clone();
        let wake: Wake = Arc::new(move || {
            let callback = handler.lock().unwrap().clone();
            callback();
        });
        let (shared, stop, fd) = (state.clone(), cancel.clone(), socket.clone());
        let worker = thread::Builder::new()
            .name("vnc-session".into())
            .spawn(move || {
                let result = run(
                    profile,
                    password,
                    rx,
                    shared.clone(),
                    stop.clone(),
                    fd.clone(),
                    wake.clone(),
                );
                *fd.lock().unwrap() = None;
                let mut s = shared.lock().unwrap();
                s.status = if stop.load(Ordering::Acquire) {
                    Status::Disconnected
                } else {
                    match result {
                        Ok(()) => Status::Disconnected,
                        Err(e) => Status::Failed(format!("{e:#}")),
                    }
                };
                s.frame = None;
                s.clipboard = None;
                s.cursor = None;
                drop(s);
                wake();
            })?;
        static IDS: AtomicU64 = AtomicU64::new(1);
        Ok(Self {
            waker,
            id: IDS.fetch_add(1, Ordering::Relaxed),
            state,
            input,
            cancel,
            socket,
            worker: Some(worker),
        })
    }
    /// Attach a different UI without touching the socket, credentials, or session identity.
    /// Invoke once after replacing the callback so pre-existing snapshots are never missed.
    pub fn set_waker(&mut self, wake: Wake) {
        *self.waker.lock().unwrap() = wake.clone();
        wake();
    }
    pub fn id(&self) -> u64 {
        self.id
    }
    pub fn snapshot(&self) -> Snapshot {
        self.state.lock().unwrap().clone()
    }
    pub fn take_clipboard(&self) -> Option<String> {
        self.state.lock().unwrap().clipboard.take()
    }
    pub fn send(&mut self, e: Input) {
        if self.input.try_send(e).is_err() {
            self.disconnect();
        }
    }
    pub fn disconnect(&mut self) {
        self.cancel.store(true, Ordering::Release);
        if let Some(socket) = self.socket.lock().unwrap().as_ref() {
            let _ = socket.shutdown(Shutdown::Both);
        }
    }
}
impl Drop for Session {
    fn drop(&mut self) {
        self.disconnect();
        if let Some(t) = self.worker.take() {
            let _ = t.join();
        }
    }
}
fn run(
    profile: Profile,
    password: Zeroizing<String>,
    input: mpsc::Receiver<Input>,
    state: Arc<Mutex<Snapshot>>,
    stop: Arc<AtomicBool>,
    socket: Arc<Mutex<Option<TcpStream>>>,
    wake: Arc<dyn Fn() + Send + Sync>,
) -> Result<()> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    let connection_result=runtime.block_on(async {
        let connection=tokio::net::TcpStream::connect((profile.host.as_str(),profile.port));
        tokio::pin!(connection);
        let deadline=tokio::time::sleep(Duration::from_secs(10));tokio::pin!(deadline);
        loop {tokio::select! {
            s=&mut connection=>break s.context("Cannot reach host; check address and Screen Sharing"),
            _=&mut deadline=>bail!("Connection timed out after 10 seconds"),
            _=tokio::time::sleep(Duration::from_millis(20))=>if stop.load(Ordering::Acquire){bail!("Cancelled")},
        }}
    });
    let connection_result = connection_result.and_then(|stream| Ok(stream.into_std()?));
    runtime.shutdown_background();
    let stream = connection_result?;
    stream.set_nonblocking(false)?;
    stream.set_nodelay(true)?;
    *socket.lock().unwrap() = Some(stream.try_clone()?);
    if stop.load(Ordering::Acquire) {
        return Ok(());
    }
    let mut user = Zeroizing::new(profile.username.as_bytes().to_vec());
    user.push(0);
    let mut secret = Zeroizing::new(password.as_bytes().to_vec());
    secret.push(0);
    let mut client = Client::init(stream.as_raw_fd(), &user, &secret)?;
    drop(user);
    drop(secret);
    drop(password);
    state.lock().unwrap().status = Status::Connected;
    wake();
    let mut generation = 0;
    let mut cursor_generation = 0;
    let mut published = Instant::now() - Duration::from_millis(34);
    let mut keys = BTreeSet::new();
    let mut pointer = (0, 0);
    while !stop.load(Ordering::Acquire) {
        for _ in 0..64 {
            let Ok(e) = input.try_recv() else { break };
            match e {
                Input::Key(k, down) => {
                    if down {
                        keys.insert(k);
                    } else {
                        keys.remove(&k);
                    }
                    client.key(k, down)?;
                }
                Input::Pointer(x, y, b) => {
                    pointer = (x, y);
                    client.pointer(x, y, b)?;
                }
                Input::Release => {
                    for k in std::mem::take(&mut keys) {
                        client.key(k, false)?;
                    }
                    client.pointer(pointer.0, pointer.1, 0)?;
                }
                Input::Clipboard(text) if profile.clipboard => {
                    client.clipboard(&text)?;
                }
                Input::Refresh => client.refresh()?,
                _ => {}
            }
        }
        client.step()?;
        if let Some(cursor) = client.cursor(cursor_generation)? {
            cursor_generation = cursor.generation;
            state.lock().unwrap().cursor = Some(Arc::new(cursor));
            wake();
        }
        if published.elapsed() >= Duration::from_millis(33) {
            if let Some(frame) = client.frame(generation)? {
                generation = frame.generation;
                state.lock().unwrap().frame = Some(Arc::new(frame));
                wake();
            }
            published = Instant::now();
        }
        if let Some(text) = client.take_clipboard()
            && profile.clipboard
        {
            state.lock().unwrap().clipboard = Some(text);
            wake();
        }
    }
    Ok(())
}
