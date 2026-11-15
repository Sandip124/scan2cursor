// Paired phones. The QR code's token only pairs a new phone; from then on the phone uses its own
// key, kept here. Removing a phone deletes its key, so that one phone loses access while the others
// keep working.

use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
};

const MAX_DEVICES: usize = 64; // paired phones; more are refused until one is removed
const FILE: &str = ".devices.json"; // holds secrets: private file, never committed

#[derive(Serialize, Deserialize, Clone)]
pub struct Device {
    pub id: String,
    pub key: String,
    pub name: String,
    pub created: i64,   // ms since epoch
    pub last_seen: i64, // ms since epoch
}

pub struct Store {
    path: PathBuf,
    pub list: Vec<Device>,
}

impl Store {
    pub fn load(home: &Path) -> Self {
        let path = home.join(FILE);
        let list = fs::read_to_string(&path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        Store { path, list }
    }

    pub fn save(&self) {
        if let Ok(json) = serde_json::to_string(&self.list) {
            let _ = crate::write_private(&self.path, &json);
        }
    }

    /// The phone that owns this key, if any. Every key is compared, in constant time.
    pub fn by_key(&mut self, key: &str) -> Option<&mut Device> {
        let mut found = None;
        for (i, d) in self.list.iter().enumerate() {
            if crate::same_secret(&d.key, key) {
                found = Some(i);
            }
        }
        found.map(|i| &mut self.list[i])
    }

    pub fn get_mut(&mut self, id: &str) -> Option<&mut Device> {
        self.list.iter_mut().find(|d| d.id == id)
    }

    /// Pair a new phone: it gets an id (shown in lists) and a secret key (its login from now on).
    pub fn enroll(&mut self, name: String, now: i64) -> std::io::Result<Device> {
        if self.list.len() >= MAX_DEVICES {
            return Err(std::io::Error::other("too many paired phones"));
        }
        let device = Device { id: crate::random_hex(4)?, key: crate::random_hex(16)?, name, created: now, last_seen: now };
        self.list.push(device.clone());
        self.save();
        Ok(device)
    }

    pub fn remove(&mut self, id: &str) -> bool {
        let before = self.list.len();
        self.list.retain(|d| d.id != id);
        let removed = self.list.len() != before;
        if removed {
            self.save();
        }
        removed
    }
}

/// Forget every paired phone (used by `new-token`, the "start over" command).
pub fn clear(home: &Path) {
    let _ = fs::remove_file(home.join(FILE));
}
