
use soroban_sdk::{contracttype, Env};

#[contracttype]
#[derive(Clone)]
pub struct Cached<T> {
    pub data: T,
    pub expires_at: u64,
}

impl<T> Cached<T> {
    pub fn new(data: T, expires_at: u64) -> Self {
        Self { data, expires_at }
    }

    pub fn is_expired(&self, env: &Env) -> bool {
        self.expires_at > 0 && self.expires_at <= env.ledger().timestamp()
    }
}