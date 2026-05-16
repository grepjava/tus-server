use std::{
    collections::HashMap,
    net::IpAddr,
    sync::Mutex,
    time::Instant,
};

struct Entry {
    count: u32,
    last: Instant,
}

pub struct LoginThrottle {
    by_ip: Mutex<HashMap<IpAddr, Entry>>,
    by_user: Mutex<HashMap<String, Entry>>,
    max_attempts: u32,
    lockout_secs: u64,
    /// Maximum entries per map. When reached, expired entries are evicted before
    /// inserting; if still at capacity the new key is skipped (not tracked).
    max_entries: usize,
}

impl LoginThrottle {
    pub fn new(max_attempts: u32, lockout_secs: u64) -> Self {
        Self {
            by_ip: Mutex::new(HashMap::new()),
            by_user: Mutex::new(HashMap::new()),
            max_attempts,
            lockout_secs,
            max_entries: 50_000,
        }
    }

    /// Returns `None` if the request is allowed, or `Some(seconds_remaining)` if
    /// either the source IP or the username is currently locked out.
    pub fn check(&self, ip: IpAddr, username: &str) -> Option<u64> {
        {
            let map = self.by_ip.lock().unwrap();
            if let Some(secs) = check_entry(map.get(&ip), self.max_attempts, self.lockout_secs) {
                return Some(secs);
            }
        }
        let map = self.by_user.lock().unwrap();
        check_entry(
            map.get(&username.to_lowercase()),
            self.max_attempts,
            self.lockout_secs,
        )
    }

    pub fn record_failure(&self, ip: IpAddr, username: &str) {
        increment(
            &mut self.by_ip.lock().unwrap(),
            ip,
            self.lockout_secs,
            self.max_entries,
        );
        increment(
            &mut self.by_user.lock().unwrap(),
            username.to_lowercase(),
            self.lockout_secs,
            self.max_entries,
        );
    }

    pub fn record_success(&self, ip: IpAddr, username: &str) {
        self.by_ip.lock().unwrap().remove(&ip);
        self.by_user.lock().unwrap().remove(&username.to_lowercase());
    }
}

fn check_entry(entry: Option<&Entry>, max_attempts: u32, lockout_secs: u64) -> Option<u64> {
    let e = entry?;
    let elapsed = e.last.elapsed().as_secs();
    if elapsed >= lockout_secs || e.count < max_attempts {
        return None;
    }
    Some(lockout_secs.saturating_sub(elapsed))
}

fn increment<K: Eq + std::hash::Hash>(
    map: &mut HashMap<K, Entry>,
    key: K,
    lockout_secs: u64,
    max_entries: usize,
) {
    if map.len() >= max_entries && !map.contains_key(&key) {
        map.retain(|_, e| e.last.elapsed().as_secs() < lockout_secs);
        if map.len() >= max_entries {
            return;
        }
    }
    let e = map
        .entry(key)
        .or_insert_with(|| Entry { count: 0, last: Instant::now() });
    e.count += 1;
    e.last = Instant::now();
}
