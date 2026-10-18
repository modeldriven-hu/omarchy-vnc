#[derive(Clone, Debug)]
pub struct Cursor {
    pub width: usize,
    pub height: usize,
    pub hotspot: [usize; 2],
    pub rgba: Vec<u8>,
    pub generation: u64,
}
use anyhow::{Result, bail};
#[derive(Clone, Debug)]
pub struct Frame {
    pub width: usize,
    pub height: usize,
    pub rgba: Vec<u8>,
    pub generation: u64,
}
impl Frame {
    pub fn new(width: usize, height: usize) -> Result<Self> {
        let size = width
            .checked_mul(height)
            .and_then(|n| n.checked_mul(4))
            .filter(|n| *n <= 128 * 1024 * 1024 && *n > 0);
        let Some(size) = size else {
            bail!("Remote framebuffer exceeds the 128 MiB limit")
        };
        Ok(Self {
            width,
            height,
            rgba: vec![0; size],
            generation: 0,
        })
    }
    fn bounds(&self, x: usize, y: usize, w: usize, h: usize) -> Result<()> {
        if x.checked_add(w).is_none_or(|n| n > self.width)
            || y.checked_add(h).is_none_or(|n| n > self.height)
        {
            bail!("Invalid framebuffer rectangle")
        }
        Ok(())
    }
    pub fn update(&mut self, x: usize, y: usize, w: usize, h: usize, bgra: &[u8]) -> Result<()> {
        self.bounds(x, y, w, h)?;
        if bgra.len() != w * h * 4 {
            bail!("Invalid pixel data length")
        }
        for row in 0..h {
            for col in 0..w {
                let src = (row * w + col) * 4;
                let dst = ((y + row) * self.width + x + col) * 4;
                self.rgba[dst..dst + 4].copy_from_slice(&[
                    bgra[src + 2],
                    bgra[src + 1],
                    bgra[src],
                    255,
                ]);
            }
        }
        self.generation += 1;
        Ok(())
    }
    pub fn copy(
        &mut self,
        dx: usize,
        dy: usize,
        sx: usize,
        sy: usize,
        w: usize,
        h: usize,
    ) -> Result<()> {
        self.bounds(dx, dy, w, h)?;
        self.bounds(sx, sy, w, h)?;
        // memmove semantics, including vertical overlap, without an extra full framebuffer.
        for n in 0..h {
            let row = if dy > sy { h - 1 - n } else { n };
            let a = ((sy + row) * self.width + sx) * 4;
            let b = ((dy + row) * self.width + dx) * 4;
            self.rgba.copy_within(a..a + w * 4, b);
        }
        self.generation += 1;
        Ok(())
    }
}
#[derive(Clone, Copy, Debug)]
pub struct Viewport {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub remote_width: usize,
    pub remote_height: usize,
}
impl Viewport {
    pub fn fit(available: [f32; 2], remote: [usize; 2], fit: bool) -> Self {
        let scale = if fit {
            (available[0] / remote[0] as f32).min(available[1] / remote[1] as f32)
        } else {
            1.0
        };
        let width = remote[0] as f32 * scale;
        let height = remote[1] as f32 * scale;
        Self {
            x: ((available[0] - width) / 2.0).max(0.0),
            y: ((available[1] - height) / 2.0).max(0.0),
            width,
            height,
            remote_width: remote[0],
            remote_height: remote[1],
        }
    }
    pub fn point(&self, x: f32, y: f32) -> Option<(u16, u16)> {
        if x < self.x || y < self.y || x >= self.x + self.width || y >= self.y + self.height {
            return None;
        }
        Some((
            ((x - self.x) / self.width * self.remote_width as f32) as u16,
            ((y - self.y) / self.height * self.remote_height as f32) as u16,
        ))
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bounds_and_pixels() {
        let mut f = Frame::new(2, 2).unwrap();
        assert!(f.update(1, 1, 2, 2, &[0; 16]).is_err());
        f.update(0, 0, 1, 1, &[1, 2, 3, 0]).unwrap();
        assert_eq!(&f.rgba[..4], &[3, 2, 1, 255]);
        assert!(Frame::new(usize::MAX, 2).is_err());
    }
    #[test]
    fn overlapping_copy() {
        let mut f = Frame::new(1, 3).unwrap();
        f.update(0, 0, 1, 3, &[1, 0, 0, 0, 2, 0, 0, 0, 3, 0, 0, 0])
            .unwrap();
        f.copy(0, 1, 0, 0, 1, 2).unwrap();
        assert_eq!(f.rgba[6], 1);
        assert_eq!(f.rgba[10], 2);
    }
    #[test]
    fn letterbox() {
        let v = Viewport::fit([100., 100.], [200, 100], true);
        assert_eq!(v.point(50., 50.), Some((100, 50)));
        assert_eq!(v.point(50., 10.), None);
    }
}
