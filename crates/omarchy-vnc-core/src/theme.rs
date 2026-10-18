#[derive(Clone)]
pub struct Palette {
    pub background: [u8; 3],
    pub foreground: [u8; 3],
    pub accent: [u8; 3],
}
impl Default for Palette {
    fn default() -> Self {
        Self {
            background: [26, 27, 38],
            foreground: [192, 202, 245],
            accent: [122, 162, 247],
        }
    }
}
impl Palette {
    pub fn load() -> Self {
        let mut palette = Self::default();
        let Some(home) = std::env::var_os("HOME") else {
            return palette;
        };
        let base = std::env::var_os("XDG_STATE_HOME")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| std::path::PathBuf::from(home).join(".local/state"));
        if let Ok(text) = std::fs::read_to_string(base.join("omarchy/current/theme/colors.toml")) {
            for line in text.lines() {
                if let Some((name, value)) = line.split_once('=') {
                    let value = value
                        .trim()
                        .trim_matches('"')
                        .trim_matches('\'')
                        .trim_start_matches('#');
                    if value.len() == 6
                        && let Ok(rgb) = u32::from_str_radix(value, 16)
                    {
                        let c = [(rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8];
                        match name.trim() {
                            "background" => palette.background = c,
                            "foreground" => palette.foreground = c,
                            "accent" => palette.accent = c,
                            _ => {}
                        }
                    }
                }
            }
        }
        palette
    }
}
