#![no_std]

use soroban_sdk::{
    contract, contractimpl, contracttype, symbol_short, Address, Env, String, Vec,
};

#[cfg(test)]
mod test;

// ─── Storage Keys ─────────────────────────────────────────────────────────────

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    /// Singleton admin address.
    Admin,
    /// Global token / event counters.
    Counters,
    /// Event metadata keyed by event_id.
    Event(u32),
    /// Attendance NFT metadata keyed by token_id.
    Token(u32),
    /// Set of token_ids owned by an address.
    OwnerTokens(Address),
    /// Guard: has the given (player, event_id) already checked in?
    CheckedIn(Address, u32),
}

// ─── Error Codes ──────────────────────────────────────────────────────────────

#[contracttype]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum AttendanceError {
    AlreadyInitialized  = 1,
    NotAdmin            = 2,
    EventNotFound       = 3,
    WindowNotOpen       = 4,
    AlreadyCheckedIn    = 5,
    TransferNotAllowed  = 6,
    TokenNotFound       = 7,
    InvalidWindow       = 8,
}

impl From<AttendanceError> for soroban_sdk::Error {
    fn from(e: AttendanceError) -> Self {
        soroban_sdk::Error::from_contract_error(e as u32)
    }
}

// ─── Data Structs ─────────────────────────────────────────────────────────────

/// Global counters kept in instance storage.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Counters {
    pub next_event_id: u32,
    pub next_token_id: u32,
    pub total_supply: u32,
}

/// An organizer-defined event with a check-in window.
#[contracttype]
#[derive(Clone, Debug)]
pub struct Event {
    pub event_id: u32,
    pub name: String,
    pub window_start: u64,
    pub window_end: u64,
    pub organizer: Address,
}

/// Non-transferable attendance NFT.
#[contracttype]
#[derive(Clone, Debug)]
pub struct AttendanceToken {
    pub token_id: u32,
    pub event_id: u32,
    pub owner: Address,
    pub checked_in_at: u64,
}

// ─── Contract ─────────────────────────────────────────────────────────────────

#[contract]
pub struct AttendanceNFT;

#[contractimpl]
impl AttendanceNFT {
    // ── Initialisation ────────────────────────────────────────────────────────

    /// Bootstrap the contract with an admin address.
    /// Can only be called once.
    pub fn initialize(env: Env, admin: Address) {
        if env.storage().instance().has(&DataKey::Admin) {
            panic!("Already initialized");
        }

        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage().instance().set(
            &DataKey::Counters,
            &Counters {
                next_event_id: 1,
                next_token_id: 1,
                total_supply: 0,
            },
        );

        env.events()
            .publish((symbol_short!("init"),), admin);
    }

    // ── Event management ──────────────────────────────────────────────────────

    /// Create a new event with a check-in window.
    /// Only the admin (organizer role) may call this.
    pub fn create_event(
        env: Env,
        name: String,
        window_start: u64,
        window_end: u64,
    ) -> u32 {
        let admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .expect("Not initialized");
        admin.require_auth();

        if window_start >= window_end {
            panic!("Invalid check-in window");
        }

        let mut counters: Counters = env
            .storage()
            .instance()
            .get(&DataKey::Counters)
            .expect("Not initialized");

        let event_id = counters.next_event_id;
        counters.next_event_id += 1;

        let event = Event {
            event_id,
            name: name.clone(),
            window_start,
            window_end,
            organizer: admin.clone(),
        };

        let key = DataKey::Event(event_id);
        env.storage().persistent().set(&key, &event);
        env.storage()
            .persistent()
            .extend_ttl(&key, 100_000, 500_000);

        env.storage().instance().set(&DataKey::Counters, &counters);

        env.events()
            .publish((symbol_short!("evt_new"),), (event_id, name));

        event_id
    }

    /// Return event metadata.
    pub fn get_event(env: Env, event_id: u32) -> Option<Event> {
        env.storage()
            .persistent()
            .get(&DataKey::Event(event_id))
    }

    // ── Check-in ──────────────────────────────────────────────────────────────

    /// Player calls this during the check-in window to receive an attendance
    /// NFT.  Panics if:
    ///  - the event does not exist
    ///  - the current ledger timestamp is outside the window
    ///  - the caller has already checked in to this event
    pub fn check_in(env: Env, player: Address, event_id: u32) -> u32 {
        player.require_auth();

        // Verify event exists
        let event: Event = env
            .storage()
            .persistent()
            .get(&DataKey::Event(event_id))
            .expect("Event not found");

        // Enforce check-in window
        let now = env.ledger().timestamp();
        if now < event.window_start || now > event.window_end {
            panic!("Check-in window is not open");
        }

        // Prevent double check-in
        let guard_key = DataKey::CheckedIn(player.clone(), event_id);
        if env.storage().persistent().has(&guard_key) {
            panic!("Already checked in");
        }

        // Record check-in guard
        env.storage().persistent().set(&guard_key, &true);
        env.storage()
            .persistent()
            .extend_ttl(&guard_key, 100_000, 500_000);

        // Mint the non-transferable attendance token
        let mut counters: Counters = env
            .storage()
            .instance()
            .get(&DataKey::Counters)
            .expect("Not initialized");

        let token_id = counters.next_token_id;
        counters.next_token_id += 1;
        counters.total_supply += 1;

        let token = AttendanceToken {
            token_id,
            event_id,
            owner: player.clone(),
            checked_in_at: now,
        };

        let token_key = DataKey::Token(token_id);
        env.storage().persistent().set(&token_key, &token);
        env.storage()
            .persistent()
            .extend_ttl(&token_key, 100_000, 500_000);

        // Update owner → token list
        let mut owned = Self::get_owned_internal(&env, player.clone());
        owned.push_back(token_id);
        let owned_key = DataKey::OwnerTokens(player.clone());
        env.storage().persistent().set(&owned_key, &owned);
        env.storage()
            .persistent()
            .extend_ttl(&owned_key, 100_000, 500_000);

        env.storage().instance().set(&DataKey::Counters, &counters);

        env.events()
            .publish((symbol_short!("checkin"),), (event_id, player, token_id));

        token_id
    }

    // ── Transfer — explicitly blocked ─────────────────────────────────────────

    /// Transfer is intentionally disabled; attendance tokens are soulbound.
    pub fn transfer(_env: Env, _from: Address, _to: Address, _token_id: u32) {
        panic!("Attendance tokens cannot be transferred");
    }

    // ── Query helpers ─────────────────────────────────────────────────────────

    /// Return the attendance token metadata.
    pub fn get_token(env: Env, token_id: u32) -> Option<AttendanceToken> {
        env.storage()
            .persistent()
            .get(&DataKey::Token(token_id))
    }

    /// Return the owner of a given token.
    pub fn owner_of(env: Env, token_id: u32) -> Address {
        let token: AttendanceToken = env
            .storage()
            .persistent()
            .get(&DataKey::Token(token_id))
            .expect("Token not found");
        token.owner
    }

    /// Return all token ids owned by an address.
    pub fn tokens_of(env: Env, owner: Address) -> Vec<u32> {
        Self::get_owned_internal(&env, owner)
    }

    /// Return whether a player has checked in to a specific event.
    pub fn has_checked_in(env: Env, player: Address, event_id: u32) -> bool {
        env.storage()
            .persistent()
            .has(&DataKey::CheckedIn(player, event_id))
    }

    /// Total number of attendance tokens minted across all events.
    pub fn total_supply(env: Env) -> u32 {
        let counters: Counters = env
            .storage()
            .instance()
            .get(&DataKey::Counters)
            .unwrap_or(Counters {
                next_event_id: 1,
                next_token_id: 1,
                total_supply: 0,
            });
        counters.total_supply
    }

    // ── Internal helpers ──────────────────────────────────────────────────────

    fn get_owned_internal(env: &Env, owner: Address) -> Vec<u32> {
        env.storage()
            .persistent()
            .get(&DataKey::OwnerTokens(owner))
            .unwrap_or(Vec::new(env))
    }
}
