//! UI-independent connection, storage, framebuffer, and input logic.
mod frame;
mod native;
mod profiles;
mod session;
mod theme;
pub use frame::*;
pub use profiles::*;
pub use session::*;
pub use theme::*;

#[derive(Default)]
pub struct LaunchOptions {
    pub gui: bool,
    pub profile: Option<String>,
    pub host: Option<String>,
    pub port: Option<u16>,
}
pub fn startup_options() -> Result<LaunchOptions, String> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() == 1 && (args[0] == "--help" || args[0] == "-h") {
        println!(
            "Omarchy VNC {}\nUsage: {} [--gui] [--profile NAME | --host HOST [--port PORT]]\nCtrl+Alt+Escape releases remote input and leaves fullscreen.",
            env!("CARGO_PKG_VERSION"),
            std::env::args().next().unwrap_or_default()
        );
        std::process::exit(0);
    }
    if args == ["--version"] {
        println!("{}", env!("CARGO_PKG_VERSION"));
        std::process::exit(0);
    }
    let mut options = LaunchOptions::default();
    let mut iter = args.iter();
    while let Some(flag) = iter.next() {
        if flag == "--gui" && !options.gui {
            options.gui = true;
            continue;
        }
        let value = iter.next().ok_or("Expected a value after the option")?;
        match flag.as_str() {
            "--host" if options.host.is_none() => options.host = Some(value.clone()),
            "--profile" if options.profile.is_none() => options.profile = Some(value.clone()),
            "--port" if options.port.is_none() => {
                let port = value
                    .parse::<u16>()
                    .map_err(|_| "Port must be between 1 and 65535")?;
                if port == 0 {
                    return Err("Port must be between 1 and 65535".into());
                }
                options.port = Some(port);
            }
            _ => {
                return Err(
                    "Usage: omarchy-vnc-{egui,gtk} [--gui] [--profile NAME | --host HOST [--port PORT]]"
                        .into(),
                );
            }
        }
    }
    if options.profile.is_some() && options.host.is_some()
        || options.port.is_some() && options.host.is_none()
    {
        return Err("Use --port with --host; --profile and --host are mutually exclusive".into());
    }
    Ok(options)
}

pub fn keysym(c: char) -> u32 {
    if c as u32 <= 255 {
        c as u32
    } else {
        0x01000000 | c as u32
    }
}

/// Shared profile presentation state. Persistence remains transactional on save failures.
#[derive(Clone)]
pub struct Model {
    pub store: Store,
    pub profiles: Profiles,
    pub selected: Option<usize>,
    pub draft: Profile,
    pub error: String,
}
impl Model {
    pub fn load_launch(options: &LaunchOptions) -> anyhow::Result<Self> {
        let mut model = Self::load(options.profile.as_deref())?;
        if let Some(host) = &options.host {
            model.new_profile();
            model.draft.host = host.clone();
            if let Some(port) = options.port {
                model.draft.port = port;
            }
        }
        Ok(model)
    }
    pub fn load(name: Option<&str>) -> anyhow::Result<Self> {
        let store = Store::discover()?;
        let (profiles, error) = match store.load() {
            Ok(p) => (p, String::new()),
            Err(e) => (Profiles::default(), e.to_string()),
        };
        let selected = name
            .or(profiles.default.as_deref())
            .and_then(|n| profiles.profiles.iter().position(|p| p.name == n));
        let draft = selected
            .map(|i| profiles.profiles[i].clone())
            .unwrap_or_default();
        let mut model = Self {
            store,
            profiles,
            selected,
            draft,
            error,
        };
        if name.is_some() && selected.is_none() {
            model.error = "Named profile was not found".into();
        }
        Ok(model)
    }
    pub fn select(&mut self, index: usize) {
        if let Some(p) = self.profiles.profiles.get(index) {
            self.selected = Some(index);
            self.draft = p.clone();
        }
    }
    pub fn new_profile(&mut self) {
        self.selected = None;
        self.draft = Profile::default();
    }
    pub fn save(&mut self) -> anyhow::Result<()> {
        self.draft.validate()?;
        let mut next = self.profiles.clone();
        if let Some(i) = self.selected {
            let old = next.profiles[i].name.clone();
            next.profiles[i] = self.draft.clone();
            if next.default.as_ref() == Some(&old) {
                next.default = Some(self.draft.name.clone());
            }
        } else {
            next.profiles.push(self.draft.clone());
        }
        self.store.save(&mut next)?;
        self.selected = next.profiles.iter().position(|p| p.name == self.draft.name);
        self.profiles = next;
        Ok(())
    }
    pub fn delete(&mut self) -> anyhow::Result<()> {
        if let Some(i) = self.selected {
            let mut next = self.profiles.clone();
            let removed = next.profiles.remove(i);
            if next.default.as_ref() == Some(&removed.name) {
                next.default = None;
            }
            self.store.save(&mut next)?;
            self.profiles = next;
            self.new_profile();
        }
        Ok(())
    }
    pub fn make_default(&mut self) -> anyhow::Result<()> {
        let mut next = self.profiles.clone();
        next.default = self.selected.map(|i| next.profiles[i].name.clone());
        self.store.save(&mut next)?;
        self.profiles = next;
        Ok(())
    }
    pub fn reload(&mut self) -> anyhow::Result<()> {
        self.profiles = self.store.load()?;
        self.new_profile();
        self.error.clear();
        Ok(())
    }
}
