use anyhow::{Context, Result, bail};
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use std::io::Write;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::{
    fs::{self, OpenOptions},
    path::PathBuf,
};

fn default_true() -> bool {
    true
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Profile {
    pub name: String,
    pub host: String,
    pub port: u16,
    #[serde(default)]
    pub username: String,
    #[serde(default = "default_true")]
    pub map_ctrl_to_command: bool,
    #[serde(default)]
    pub fullscreen: bool,
    #[serde(default)]
    pub clipboard: bool,
}
impl Default for Profile {
    fn default() -> Self {
        Self {
            name: "Mac mini".into(),
            host: String::new(),
            port: 5900,
            username: String::new(),
            map_ctrl_to_command: true,
            fullscreen: false,
            clipboard: false,
        }
    }
}
impl Profile {
    pub fn validate(&self) -> Result<()> {
        if self.name.trim().is_empty() || self.name.len() > 128 {
            bail!("Enter a profile name (1–128 bytes)")
        }
        if self.host.is_empty()
            || self.host.len() > 253
            || self
                .host
                .chars()
                .any(|c| c.is_whitespace() || c.is_control() || "/\\@".contains(c))
            || self.host.starts_with('-')
        {
            bail!("Enter a hostname or IP address, without a URL or port")
        }
        if self.host.contains(':') && self.host.parse::<std::net::Ipv6Addr>().is_err() {
            bail!("Use an unbracketed IPv6 address, or put the port in its separate field")
        }
        if self.username.len() > 127 || self.username.contains('\0') {
            bail!("Username must be at most 127 bytes and contain no NUL")
        }
        if self.port == 0 {
            bail!("Port must be between 1 and 65535")
        }
        Ok(())
    }
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Profiles {
    pub version: u32,
    pub revision: u64,
    pub default: Option<String>,
    pub profiles: Vec<Profile>,
}
impl Default for Profiles {
    fn default() -> Self {
        Self {
            version: 1,
            revision: 0,
            default: None,
            profiles: vec![],
        }
    }
}
impl Profiles {
    pub fn validate(&self) -> Result<()> {
        if self.version != 1 {
            bail!("Unsupported profile format version {}", self.version)
        }
        for (i, p) in self.profiles.iter().enumerate() {
            p.validate()?;
            if self.profiles[..i].iter().any(|q| q.name == p.name) {
                bail!("Profile names must be unique")
            }
        }
        if let Some(n) = &self.default
            && !self.profiles.iter().any(|p| &p.name == n)
        {
            bail!("Default profile is missing")
        }
        Ok(())
    }
}
#[derive(Clone)]
pub struct Store {
    pub directory: PathBuf,
}
impl Store {
    pub fn discover() -> Result<Self> {
        let base = match std::env::var_os("XDG_CONFIG_HOME").filter(|v| !v.is_empty()) {
            Some(p) => PathBuf::from(p),
            None => {
                PathBuf::from(std::env::var_os("HOME").context("HOME is not set")?).join(".config")
            }
        };
        if !base.is_absolute() {
            bail!("Configuration directory must be absolute")
        }
        Ok(Self {
            directory: base.join("omarchy-vnc"),
        })
    }
    pub fn load(&self) -> Result<Profiles> {
        let path = self.directory.join("profiles.json");
        let result: Profiles = match fs::read(&path) {
            Ok(bytes) => serde_json::from_slice(&bytes)
                .context("Invalid profiles.json; original file preserved")?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Profiles::default(),
            Err(e) => return Err(e.into()),
        };
        result.validate()?;
        Ok(result)
    }
    /// Revision check and atomic save under an interprocess lock prevent lost updates.
    pub fn save(&self, profiles: &mut Profiles) -> Result<()> {
        profiles.validate()?;
        fs::create_dir_all(&self.directory)?;
        fs::set_permissions(&self.directory, fs::Permissions::from_mode(0o700))?;
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .open(self.directory.join("profiles.lock"))?;
        lock.lock_exclusive()?;
        let current = self.load()?;
        if current.revision != profiles.revision {
            bail!("Profiles changed in another window. Reload before saving.")
        }
        let mut next = profiles.clone();
        next.revision = next
            .revision
            .checked_add(1)
            .context("Profile revision overflow")?;
        let mut temp = tempfile::NamedTempFile::new_in(&self.directory)?;
        temp.write_all(&serde_json::to_vec_pretty(&next)?)?;
        temp.as_file().sync_all()?;
        temp.persist(self.directory.join("profiles.json"))?;
        std::fs::File::open(&self.directory)?.sync_all()?;
        *profiles = next;
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn conflicts_preserve_data() {
        let dir = tempfile::tempdir().unwrap();
        let s = Store {
            directory: dir.path().join("with spaces"),
        };
        let mut a = s.load().unwrap();
        let mut b = a.clone();
        a.profiles.push(Profile {
            host: "macmini.local".into(),
            ..Profile::default()
        });
        s.save(&mut a).unwrap();
        b.profiles.push(Profile {
            host: "other.local".into(),
            ..Profile::default()
        });
        assert!(s.save(&mut b).is_err());
        assert_eq!(s.load().unwrap(), a);
    }
    #[test]
    fn invalid_and_corrupt_profiles() {
        let dir = tempfile::tempdir().unwrap();
        let s = Store {
            directory: dir.path().into(),
        };
        fs::write(s.directory.join("profiles.json"), "broken").unwrap();
        assert!(s.load().is_err());
        assert!(Profile::default().validate().is_err());
        assert!(
            Profile {
                host: "localhost:5900".into(),
                ..Profile::default()
            }
            .validate()
            .is_err()
        );
        assert!(
            Profile {
                host: "::1".into(),
                ..Profile::default()
            }
            .validate()
            .is_ok()
        );
    }
}

#[cfg(test)]
mod model_tests {
    use super::*;
    use crate::Model;
    #[test]
    fn shared_model_round_trip_default_rename_and_stale_edit() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store {
            directory: dir.path().join("profiles"),
        };
        let mut a = Model {
            store: store.clone(),
            profiles: Profiles::default(),
            selected: None,
            draft: Profile {
                host: "example.local".into(),
                username: "sample".into(),
                ..Profile::default()
            },
            error: String::new(),
        };
        a.save().unwrap();
        a.make_default().unwrap();
        let loaded = store.load().unwrap();
        let mut b = Model {
            store,
            profiles: loaded,
            selected: Some(0),
            draft: a.draft.clone(),
            error: String::new(),
        };
        a.draft.name = "Renamed".into();
        a.save().unwrap();
        assert_eq!(a.profiles.default.as_deref(), Some("Renamed"));
        b.draft.host = "stale.example".into();
        assert!(b.save().is_err());
        b.reload().unwrap();
        b.select(0);
        assert_eq!(b.draft.name, "Renamed");
        assert_eq!(b.draft.username, "sample");
        b.delete().unwrap();
        assert!(b.profiles.profiles.is_empty());
        assert!(b.profiles.default.is_none());
    }
}
