//! Last-note priority: the keys held, oldest first, the newest sounding.

/// The keys held down.
#[derive(Debug, Clone)]
pub struct HeldKeys {
    /// Oldest first. With none held, slot 0 keeps the last key released,
    /// so its pitch carries through the release.
    keys: [u8; HeldKeys::CAPACITY],
    len: usize,
}

impl Default for HeldKeys {
    fn default() -> Self {
        Self {
            keys: [0; Self::CAPACITY],
            len: 0,
        }
    }
}

impl HeldKeys {
    /// Keys remembered; past this the oldest is forgotten.
    pub const CAPACITY: usize = 16;

    /// `key` goes down and sounds.
    pub fn down(&mut self, key: u8) {
        self.remove(key);
        if self.len == Self::CAPACITY {
            self.keys.copy_within(1.., 0);
            self.len -= 1;
        }
        self.keys[self.len] = key;
        self.len += 1;
    }

    /// `key` comes up; the newest key still held sounds.
    pub fn up(&mut self, key: u8) {
        let was = self.len;
        self.remove(key);
        if self.len == 0 && was > 0 {
            self.keys[0] = key;
        }
    }

    /// Whether any key is held.
    pub fn any(&self) -> bool {
        self.len > 0
    }

    /// The newest key held, or the last released.
    pub fn sounding(&self) -> u8 {
        self.keys[self.len.saturating_sub(1)]
    }

    /// Every key let go.
    pub fn clear(&mut self) {
        self.len = 0;
    }

    fn remove(&mut self, key: u8) {
        if let Some(i) = self.keys[..self.len].iter().position(|&k| k == key) {
            self.keys.copy_within(i + 1..self.len, i);
            self.len -= 1;
        }
    }
}
