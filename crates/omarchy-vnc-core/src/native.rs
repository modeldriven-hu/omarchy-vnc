//! All LibVNCClient ownership and unsafe operations stay behind this worker-local adapter.
use crate::{Cursor, Frame};
use anyhow::{Context, Result, bail};
use std::{
    ffi::{c_char, c_int, c_void},
    os::fd::RawFd,
    ptr::NonNull,
};
unsafe extern "C" {
    fn ovnc_global_init();
    fn ovnc_new(fd: c_int, user: *const c_char, password: *const c_char) -> *mut c_void;
    fn ovnc_init(client: *mut c_void) -> c_int;
    fn ovnc_step(client: *mut c_void) -> c_int;
    fn ovnc_frame(
        client: *mut c_void,
        width: *mut c_int,
        height: *mut c_int,
        generation: *mut u64,
    ) -> *const u8;
    fn ovnc_cursor(
        client: *mut c_void,
        w: *mut c_int,
        h: *mut c_int,
        x: *mut c_int,
        y: *mut c_int,
        generation: *mut u64,
    ) -> *const u8;
    fn ovnc_key(client: *mut c_void, key: u32, down: c_int) -> c_int;
    fn ovnc_pointer(client: *mut c_void, x: c_int, y: c_int, buttons: c_int) -> c_int;
    fn ovnc_refresh(client: *mut c_void) -> c_int;
    fn ovnc_clipboard(client: *mut c_void, text: *mut c_char, len: c_int) -> c_int;
    fn ovnc_take_clipboard(client: *mut c_void, len: *mut c_int) -> *const c_char;
    fn ovnc_clear_clipboard(client: *mut c_void);
    fn ovnc_free(client: *mut c_void);
}
pub(crate) struct Client(NonNull<c_void>);
impl Client {
    /// NUL-terminated buffers stay alive until ovnc_init forgets the borrowed pointers.
    pub(crate) fn init(fd: RawFd, user: &[u8], password: &[u8]) -> Result<Self> {
        static INIT: std::sync::Once = std::sync::Once::new();
        // SAFETY: process-global log configuration is initialized once before any client starts.
        INIT.call_once(|| unsafe { ovnc_global_init() });
        // SAFETY: caller owns the connected fd; C duplicates it. Buffers are NUL terminated and
        // remain borrowed through initialization. The opaque allocation is owned only here.
        let ptr = unsafe { ovnc_new(fd, user.as_ptr().cast(), password.as_ptr().cast()) };
        let client = Self(NonNull::new(ptr).context("Cannot allocate VNC client")?);
        // SAFETY: valid unique client, used on this thread only.
        if unsafe { ovnc_init(client.0.as_ptr()) } == 0 {
            bail!(
                "Authentication or VNC initialization failed. For Apple login, enter the Mac account’s short username and password; verify that account is allowed in Screen Sharing."
            )
        }
        Ok(client)
    }
    pub(crate) fn step(&mut self) -> Result<()> {
        // SAFETY: unique live client confined to worker thread.
        if unsafe { ovnc_step(self.0.as_ptr()) } == 0 {
            bail!("Remote connection closed or returned invalid VNC data")
        };
        Ok(())
    }
    pub(crate) fn frame(&self, last: u64) -> Result<Option<Frame>> {
        let (mut w, mut h, mut generation) = (0, 0, 0);
        // SAFETY: out parameters point to valid locals. C buffer is not mutated until this worker's next step.
        let data = unsafe { ovnc_frame(self.0.as_ptr(), &mut w, &mut h, &mut generation) };
        if generation == last {
            return Ok(None);
        }
        if w <= 0
            || h <= 0
            || w > 8192
            || h > 8192
            || (w as u64 * h as u64) > 8_294_400
            || data.is_null()
        {
            bail!("Invalid remote framebuffer")
        }
        let mut frame = Frame::new(w as usize, h as usize)?;
        // SAFETY: shim's allocation callback enforces dimensions and allocates width*height*4 bytes.
        let bytes = unsafe { std::slice::from_raw_parts(data, frame.rgba.len()) };
        frame.update(0, 0, w as usize, h as usize, bytes)?;
        frame.generation = generation;
        Ok(Some(frame))
    }
    pub(crate) fn cursor(&self, last: u64) -> Result<Option<Cursor>> {
        let (mut w, mut h, mut x, mut y, mut generation) = (0, 0, 0, 0, 0);
        // SAFETY: valid client and output pointers; buffer remains stable until the next worker step.
        let data = unsafe {
            ovnc_cursor(
                self.0.as_ptr(),
                &mut w,
                &mut h,
                &mut x,
                &mut y,
                &mut generation,
            )
        };
        if generation == last || data.is_null() {
            return Ok(None);
        }
        if !(1..=512).contains(&w) || !(1..=512).contains(&h) || x < 0 || y < 0 || x >= w || y >= h
        {
            bail!("Invalid cursor dimensions")
        }
        // SAFETY: shim allocates width*height*4 with the same checked limits; owned copy outlives it.
        let rgba =
            unsafe { std::slice::from_raw_parts(data, w as usize * h as usize * 4) }.to_vec();
        Ok(Some(Cursor {
            width: w as usize,
            height: h as usize,
            hotspot: [x as usize, y as usize],
            rgba,
            generation,
        }))
    }
    pub(crate) fn key(&mut self, key: u32, down: bool) -> Result<()> {
        // SAFETY: unique live client and scalar arguments.
        check(unsafe { ovnc_key(self.0.as_ptr(), key, down.into()) })
    }
    pub(crate) fn pointer(&mut self, x: u16, y: u16, b: u8) -> Result<()> {
        // SAFETY: unique live client and scalar arguments; library bounds-checks coordinates.
        check(unsafe { ovnc_pointer(self.0.as_ptr(), x.into(), y.into(), b.into()) })
    }
    pub(crate) fn refresh(&mut self) -> Result<()> {
        // SAFETY: unique live client.
        check(unsafe { ovnc_refresh(self.0.as_ptr()) })
    }
    pub(crate) fn clipboard(&mut self, text: &str) -> Result<()> {
        if text.len() > 65536 || !text.chars().all(|c| c as u32 <= 255) {
            bail!("Clipboard supports Latin-1 text up to 64 KiB")
        }
        let mut bytes: Vec<u8> = text.chars().map(|c| c as u8).collect();
        // SAFETY: buffer is valid for its supplied length and is only borrowed during the call.
        check(unsafe {
            ovnc_clipboard(
                self.0.as_ptr(),
                bytes.as_mut_ptr().cast(),
                bytes.len() as c_int,
            )
        })
    }
    pub(crate) fn take_clipboard(&mut self) -> Option<String> {
        let mut n = 0;
        // SAFETY: C buffer remains owned by client until explicit clear below; length is checked.
        let ptr = unsafe { ovnc_take_clipboard(self.0.as_ptr(), &mut n) };
        if ptr.is_null() {
            return None;
        }
        let text = if (0..=65536).contains(&n) {
            // SAFETY: shim allocates exactly length+1 and never exceeds the clipboard limit.
            Some(
                unsafe { std::slice::from_raw_parts(ptr.cast::<u8>(), n as usize) }
                    .iter()
                    .map(|b| char::from(*b))
                    .collect(),
            )
        } else {
            None
        };
        // SAFETY: no borrowed references survive; text above owns its contents.
        unsafe { ovnc_clear_clipboard(self.0.as_ptr()) };
        text
    }
}
fn check(result: c_int) -> Result<()> {
    if result == 0 {
        bail!("Cannot send input to the remote host")
    };
    Ok(())
}
impl Drop for Client {
    fn drop(&mut self) {
        // SAFETY: this uniquely owned opaque pointer is freed exactly once on its worker thread.
        unsafe { ovnc_free(self.0.as_ptr()) }
    }
}
