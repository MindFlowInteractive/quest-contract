
#![no_std]

use soroban_sdk::{
    contract, contractimpl, contracttype, symbol_short,
    Address, Env, Map, String, Symbol, Vec,
    log, panic_with_error,
};

// ─── Storage Keys ────────────────────────────────────────────────────────────

const ADMIN_KEY: Symbol          = symbol_short!("ADMIN");
const TOKEN_COUNT_KEY: Symbol    = symbol_short!("TOK_CNT");
const PAUSED_KEY: Symbol         = symbol_short!("PAUSED");

// ─── Error Codes ─────────────────────────────────────────────────────────────

#[contracttype]
#[derive(Copy, Clone, Debug, PartialEq)]
#[repr(u32)]
pub enum CertError {
    NotAdmin          = 1,
    NotOwner          = 2,
    CertNotFound      = 3,
    AlreadyMinted     = 4,
    TransferRestricted = 5,
    ContractPaused    = 6,
    InvalidInput      = 7,
    Unauthorized      = 8,
}

impl From<CertError> for soroban_sdk::Error {
    fn from(e: CertError) -> Self {
        soroban_sdk::Error::from_contract_error(e as u32)
    }
}

// ─── Rarity Tier ─────────────────────────────────────────────────────────────

#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub enum RarityTier {
    /// Completion time ≤ 60 s — legendary speed-run
    Legendary,
    /// 61 s – 300 s  — epic
    Epic,
    /// 301 s – 900 s — rare
    Rare,
    /// 901 s – 3600 s — uncommon
    Uncommon,
    /// > 3600 s       — common
    Common,
}

impl RarityTier {
    pub fn from_seconds(secs: u64) -> Self {
        match secs {
            0..=60      => RarityTier::Legendary,
            61..=300    => RarityTier::Epic,
            301..=900   => RarityTier::Rare,
            901..=3600  => RarityTier::Uncommon,
            _           => RarityTier::Common,
        }
    }

    pub fn weight(&self) -> u32 {
        match self {
            RarityTier::Legendary => 5,
            RarityTier::Epic      => 4,
            RarityTier::Rare      => 3,
            RarityTier::Uncommon  => 2,
            RarityTier::Common    => 1,
        }
    }
}

// ─── Certificate Metadata ────────────────────────────────────────────────────

#[contracttype]
#[derive(Clone, Debug)]
pub struct CertificateMetadata {
    /// Unique token id (auto-incremented).
    pub token_id: u64,
    /// Wallet address of the puzzle solver.
    pub owner: Address,
    /// Unique puzzle identifier.
    pub puzzle_id: String,
    /// Puzzle title for display purposes.
    pub puzzle_title: String,
    /// Ledger timestamp when the puzzle was completed.
    pub completed_at: u64,
    /// Time taken to solve the puzzle (seconds).
    pub completion_time_secs: u64,
    /// Solver's rank at time of completion (1 = first solver).
    pub rank: u64,
    /// Rarity tier derived from `completion_time_secs`.
    pub rarity: RarityTier,
    /// SHA-256 hex digest of the correct solution — used as proof embedding.
    pub solution_hash: String,
    /// Human-readable display URI (off-chain metadata JSON, IPFS encouraged).
    pub metadata_uri: String,
    /// Whether this certificate can be transferred.
    pub transferable: bool,
    /// Whether this certificate has been burned.
    pub burned: bool,
}

// ─── Verification Proof ──────────────────────────────────────────────────────

#[contracttype]
#[derive(Clone, Debug)]
pub struct VerificationProof {
    pub token_id: u64,
    pub owner: Address,
    pub puzzle_id: String,
    pub solution_hash: String,
    pub rarity: RarityTier,
    pub completed_at: u64,
    pub rank: u64,
    pub authentic: bool,
}

// ─── Storage Helpers ─────────────────────────────────────────────────────────

fn cert_key(token_id: u64) -> (Symbol, u64) {
    (symbol_short!("CERT"), token_id)
}

fn owner_certs_key(owner: &Address) -> (Symbol, Address) {
    (symbol_short!("OWN_CERT"), owner.clone())
}

fn puzzle_minted_key(puzzle_id: &String, owner: &Address) -> (Symbol, String, Address) {
    (symbol_short!("P_MINTED"), puzzle_id.clone(), owner.clone())
}

// ─── Contract ────────────────────────────────────────────────────────────────

#[contract]
pub struct CompletionCertificateContract;

#[contractimpl]
impl CompletionCertificateContract {

    // ── Initialisation ────────────────────────────────────────────────────

    pub fn initialize(env: Env, admin: Address) {
        if env.storage().instance().has(&ADMIN_KEY) {
            panic_with_error!(&env, CertError::Unauthorized);
        }
        admin.require_auth();
        env.storage().instance().set(&ADMIN_KEY, &admin);
        env.storage().instance().set(&TOKEN_COUNT_KEY, &0u64);
        env.storage().instance().set(&PAUSED_KEY, &false);
        log!(&env, "CompletionCertificate: initialized with admin {}", admin);
    }

    // ── Admin Utilities ───────────────────────────────────────────────────

    pub fn set_admin(env: Env, new_admin: Address) {
        Self::require_admin(&env);
        env.storage().instance().set(&ADMIN_KEY, &new_admin);
    }

    pub fn set_paused(env: Env, paused: bool) {
        Self::require_admin(&env);
        env.storage().instance().set(&PAUSED_KEY, &paused);
    }

    // ── Minting ───────────────────────────────────────────────────────────

    pub fn mint_certificate(
        env: Env,
        owner: Address,
        puzzle_id: String,
        puzzle_title: String,
        completion_time_secs: u64,
        rank: u64,
        solution_hash: String,
        metadata_uri: String,
        transferable: bool,
    ) -> u64 {
        Self::require_not_paused(&env);
        Self::require_admin(&env);

        let mint_key = puzzle_minted_key(&puzzle_id, &owner);
        if env.storage().persistent().has(&mint_key) {
            panic_with_error!(&env, CertError::AlreadyMinted);
        }

        let rarity = RarityTier::from_seconds(completion_time_secs);

        let token_id: u64 = env
            .storage()
            .instance()
            .get::<Symbol, u64>(&TOKEN_COUNT_KEY)
            .unwrap_or(0)
            + 1;
        env.storage().instance().set(&TOKEN_COUNT_KEY, &token_id);

        let completed_at = env.ledger().timestamp();

        let cert = CertificateMetadata {
            token_id,
            owner: owner.clone(),
            puzzle_id: puzzle_id.clone(),
            puzzle_title,
            completed_at,
            completion_time_secs,
            rank,
            rarity: rarity.clone(),
            solution_hash,
            metadata_uri,
            transferable,
            burned: false,
        };

        // Persist certificate.
        env.storage().persistent().set(&cert_key(token_id), &cert);
        // Mark as minted for this (puzzle, owner) pair.
        env.storage().persistent().set(&mint_key, &token_id);

        // Append to owner's list.
        let own_key = owner_certs_key(&owner);
        let mut owner_list: Vec<u64> = env
            .storage()
            .persistent()
            .get::<(Symbol, Address), Vec<u64>>(&own_key)
            .unwrap_or(Vec::new(&env));
        owner_list.push_back(token_id);
        env.storage().persistent().set(&own_key, &owner_list);

        log!(
            &env,
            "Minted certificate #{} for puzzle {} — rarity {:?}",
            token_id,
            puzzle_id,
            rarity
        );

        // Emit event.
        env.events().publish(
            (symbol_short!("mint"), symbol_short!("cert")),
            (token_id, owner, puzzle_id),
        );

        token_id
    }

    // ── Transfers ─────────────────────────────────────────────────────────

    pub fn transfer(env: Env, from: Address, to: Address, token_id: u64) {
        Self::require_not_paused(&env);
        from.require_auth();

        let key = cert_key(token_id);
        let mut cert: CertificateMetadata = env
            .storage()
            .persistent()
            .get::<(Symbol, u64), CertificateMetadata>(&key)
            .unwrap_or_else(|| panic_with_error!(&env, CertError::CertNotFound));

        if cert.burned {
            panic_with_error!(&env, CertError::CertNotFound);
        }
        if cert.owner != from {
            panic_with_error!(&env, CertError::NotOwner);
        }
        if !cert.transferable {
            panic_with_error!(&env, CertError::TransferRestricted);
        }

        // Remove from old owner's list.
        let old_key = owner_certs_key(&from);
        let mut old_list: Vec<u64> = env
            .storage()
            .persistent()
            .get::<(Symbol, Address), Vec<u64>>(&old_key)
            .unwrap_or(Vec::new(&env));
        if let Some(idx) = old_list.iter().position(|id| id == token_id) {
            old_list.remove(idx as u32);
        }
        env.storage().persistent().set(&old_key, &old_list);

        // Add to new owner's list.
        let new_key = owner_certs_key(&to);
        let mut new_list: Vec<u64> = env
            .storage()
            .persistent()
            .get::<(Symbol, Address), Vec<u64>>(&new_key)
            .unwrap_or(Vec::new(&env));
        new_list.push_back(token_id);
        env.storage().persistent().set(&new_key, &new_list);

        cert.owner = to.clone();
        env.storage().persistent().set(&key, &cert);

        env.events().publish(
            (symbol_short!("transfer"), symbol_short!("cert")),
            (token_id, from, to),
        );
    }

    // ── Burning ───────────────────────────────────────────────────────────

    pub fn burn(env: Env, owner: Address, token_id: u64) {
        owner.require_auth();

        let key = cert_key(token_id);
        let mut cert: CertificateMetadata = env
            .storage()
            .persistent()
            .get::<(Symbol, u64), CertificateMetadata>(&key)
            .unwrap_or_else(|| panic_with_error!(&env, CertError::CertNotFound));

        if cert.owner != owner {
            panic_with_error!(&env, CertError::NotOwner);
        }
        if cert.burned {
            panic_with_error!(&env, CertError::CertNotFound);
        }

        cert.burned = true;
        env.storage().persistent().set(&key, &cert);

        // Remove from owner list.
        let own_key = owner_certs_key(&owner);
        let mut list: Vec<u64> = env
            .storage()
            .persistent()
            .get::<(Symbol, Address), Vec<u64>>(&own_key)
            .unwrap_or(Vec::new(&env));
        if let Some(idx) = list.iter().position(|id| id == token_id) {
            list.remove(idx as u32);
        }
        env.storage().persistent().set(&own_key, &list);

        env.events().publish(
            (symbol_short!("burn"), symbol_short!("cert")),
            (token_id, owner),
        );
    }

    // ── Verification ──────────────────────────────────────────────────────

    pub fn verify_certificate(env: Env, token_id: u64) -> VerificationProof {
        let key = cert_key(token_id);
        match env
            .storage()
            .persistent()
            .get::<(Symbol, u64), CertificateMetadata>(&key)
        {
            Some(cert) if !cert.burned => VerificationProof {
                token_id:      cert.token_id,
                owner:         cert.owner,
                puzzle_id:     cert.puzzle_id,
                solution_hash: cert.solution_hash,
                rarity:        cert.rarity,
                completed_at:  cert.completed_at,
                rank:          cert.rank,
                authentic:     true,
            },
            _ => {

                let contract_addr = env.current_contract_address();
                VerificationProof {
                    token_id,
                    owner:         contract_addr,
                    puzzle_id:     String::from_str(&env, ""),
                    solution_hash: String::from_str(&env, ""),
                    rarity:        RarityTier::Common,
                    completed_at:  0,
                    rank:          0,
                    authentic:     false,
                }
            }
        }
    }

    // ── Queries ───────────────────────────────────────────────────────────

    /// Retrieve full certificate metadata by token id.
    pub fn get_certificate(env: Env, token_id: u64) -> CertificateMetadata {
        env.storage()
            .persistent()
            .get::<(Symbol, u64), CertificateMetadata>(&cert_key(token_id))
            .unwrap_or_else(|| panic_with_error!(&env, CertError::CertNotFound))
    }

    /// Return all token ids owned by a given address (the showcase/gallery).
    pub fn get_owner_certificates(env: Env, owner: Address) -> Vec<u64> {
        env.storage()
            .persistent()
            .get::<(Symbol, Address), Vec<u64>>(&owner_certs_key(&owner))
            .unwrap_or(Vec::new(&env))
    }

    /// Return the total number of certificates ever minted.
    pub fn total_supply(env: Env) -> u64 {
        env.storage()
            .instance()
            .get::<Symbol, u64>(&TOKEN_COUNT_KEY)
            .unwrap_or(0)
    }

    /// Check if a specific (puzzle_id, owner) pair has already been minted.
    pub fn is_minted(env: Env, puzzle_id: String, owner: Address) -> bool {
        env.storage()
            .persistent()
            .has(&puzzle_minted_key(&puzzle_id, &owner))
    }

    /// Return the current admin address.
    pub fn get_admin(env: Env) -> Address {
        env.storage()
            .instance()
            .get::<Symbol, Address>(&ADMIN_KEY)
            .unwrap_or_else(|| panic_with_error!(&env, CertError::Unauthorized))
    }

    /// Return whether the contract is currently paused.
    pub fn is_paused(env: Env) -> bool {
        env.storage()
            .instance()
            .get::<Symbol, bool>(&PAUSED_KEY)
            .unwrap_or(false)
    }

    // ── Showcase / Gallery ────────────────────────────────────────────────

    pub fn get_showcase(env: Env, owner: Address) -> Vec<CertificateMetadata> {
        let ids = Self::get_owner_certificates(env.clone(), owner);
        let mut certs: Vec<CertificateMetadata> = Vec::new(&env);

        for id in ids.iter() {
            if let Some(cert) = env
                .storage()
                .persistent()
                .get::<(Symbol, u64), CertificateMetadata>(&cert_key(id))
            {
                if !cert.burned {
                    certs.push_back(cert);
                }
            }
        }

        // Insertion sort by rarity weight (descending).
        let len = certs.len();
        for i in 1..len {
            let mut j = i;
            while j > 0 {
                let a = certs.get(j - 1).unwrap().rarity.weight();
                let b = certs.get(j).unwrap().rarity.weight();
                if a < b {
                    // Swap.
                    let tmp_a = certs.get(j - 1).unwrap();
                    let tmp_b = certs.get(j).unwrap();
                    certs.set(j - 1, tmp_b);
                    certs.set(j, tmp_a);
                    j -= 1;
                } else {
                    break;
                }
            }
        }

        certs
    }

    // ── Internal Helpers ──────────────────────────────────────────────────

    fn require_admin(env: &Env) {
        let admin: Address = env
            .storage()
            .instance()
            .get::<Symbol, Address>(&ADMIN_KEY)
            .unwrap_or_else(|| panic_with_error!(env, CertError::NotAdmin));
        admin.require_auth();
    }

    fn require_not_paused(env: &Env) {
        let paused: bool = env
            .storage()
            .instance()
            .get::<Symbol, bool>(&PAUSED_KEY)
            .unwrap_or(false);
        if paused {
            panic_with_error!(env, CertError::ContractPaused);
        }
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use soroban_sdk::{
        testutils::{Address as _, Ledger, LedgerInfo},
        Env, String,
    };

    /// Helper: deploy and initialise the contract, return (env, contract_id, admin).
    fn setup() -> (Env, Address, Address) {
        let env = Env::default();
        env.mock_all_auths();

        let admin = Address::generate(&env);
        let contract_id = env.register_contract(None, CompletionCertificateContract);
        let client = CompletionCertificateContractClient::new(&env, &contract_id);
        client.initialize(&admin);
        (env, contract_id, admin)
    }

    // ── Rarity Tier Tests ─────────────────────────────────────────────────

    #[test]
    fn test_rarity_legendary() {
        assert_eq!(RarityTier::from_seconds(0),  RarityTier::Legendary);
        assert_eq!(RarityTier::from_seconds(30), RarityTier::Legendary);
        assert_eq!(RarityTier::from_seconds(60), RarityTier::Legendary);
    }

    #[test]
    fn test_rarity_epic() {
        assert_eq!(RarityTier::from_seconds(61),  RarityTier::Epic);
        assert_eq!(RarityTier::from_seconds(300), RarityTier::Epic);
    }

    #[test]
    fn test_rarity_rare() {
        assert_eq!(RarityTier::from_seconds(301), RarityTier::Rare);
        assert_eq!(RarityTier::from_seconds(900), RarityTier::Rare);
    }

    #[test]
    fn test_rarity_uncommon() {
        assert_eq!(RarityTier::from_seconds(901),  RarityTier::Uncommon);
        assert_eq!(RarityTier::from_seconds(3600), RarityTier::Uncommon);
    }

    #[test]
    fn test_rarity_common() {
        assert_eq!(RarityTier::from_seconds(3601),  RarityTier::Common);
        assert_eq!(RarityTier::from_seconds(99999), RarityTier::Common);
    }

    #[test]
    fn test_rarity_weights_ordered() {
        assert!(RarityTier::Legendary.weight() > RarityTier::Epic.weight());
        assert!(RarityTier::Epic.weight()       > RarityTier::Rare.weight());
        assert!(RarityTier::Rare.weight()       > RarityTier::Uncommon.weight());
        assert!(RarityTier::Uncommon.weight()   > RarityTier::Common.weight());
    }

    // ── Initialisation Tests ──────────────────────────────────────────────

    #[test]
    fn test_initialize() {
        let (env, contract_id, admin) = setup();
        let client = CompletionCertificateContractClient::new(&env, &contract_id);
        assert_eq!(client.get_admin(), admin);
        assert_eq!(client.total_supply(), 0);
        assert!(!client.is_paused());
    }

    #[test]
    #[should_panic]
    fn test_double_initialize_panics() {
        let (env, contract_id, admin) = setup();
        let client = CompletionCertificateContractClient::new(&env, &contract_id);
        // Second initialise must panic.
        client.initialize(&admin);
    }

    // ── Mint Tests ────────────────────────────────────────────────────────

    #[test]
    fn test_mint_certificate_basic() {
        let (env, contract_id, _admin) = setup();
        let client = CompletionCertificateContractClient::new(&env, &contract_id);

        let owner   = Address::generate(&env);
        let p_id    = String::from_str(&env, "PUZZLE-001");
        let p_title = String::from_str(&env, "The Lost Labyrinth");
        let sol_hash = String::from_str(&env, "abc123def456");
        let uri      = String::from_str(&env, "ipfs://QmTest");

        let token_id = client.mint_certificate(
            &owner, &p_id, &p_title,
            &45u64,   // 45 s → Legendary
            &1u64,    // rank 1
            &sol_hash, &uri, &true,
        );

        assert_eq!(token_id, 1);
        assert_eq!(client.total_supply(), 1);

        let cert = client.get_certificate(&token_id);
        assert_eq!(cert.owner, owner);
        assert_eq!(cert.puzzle_id, p_id);
        assert_eq!(cert.rank, 1);
        assert_eq!(cert.rarity, RarityTier::Legendary);
        assert!(!cert.burned);
        assert!(cert.transferable);
    }

    #[test]
    fn test_mint_increments_token_id() {
        let (env, contract_id, _) = setup();
        let client = CompletionCertificateContractClient::new(&env, &contract_id);

        for i in 0u64..5 {
            let owner = Address::generate(&env);
            let p_id = String::from_str(&env, "PUZZLE-X");
            // Make each mint unique by using different owners.
            let id = client.mint_certificate(
                &owner,
                &p_id,
                &String::from_str(&env, "Test Puzzle"),
                &(i * 100 + 50),
                &(i + 1),
                &String::from_str(&env, "hash"),
                &String::from_str(&env, "uri"),
                &true,
            );
            assert_eq!(id, i + 1);
        }
        assert_eq!(client.total_supply(), 5);
    }

    #[test]
    #[should_panic]
    fn test_mint_duplicate_panics() {
        let (env, contract_id, _) = setup();
        let client = CompletionCertificateContractClient::new(&env, &contract_id);

        let owner = Address::generate(&env);
        let p_id = String::from_str(&env, "PUZZLE-DUP");
        let args = (
            &owner,
            &p_id,
            &String::from_str(&env, "Dup Test"),
            &200u64,
            &1u64,
            &String::from_str(&env, "hash"),
            &String::from_str(&env, "uri"),
            &true,
        );
        client.mint_certificate(
            args.0, args.1, args.2, args.3, args.4, args.5, args.6, args.7,
        );
        // Second mint for same (puzzle, owner) must panic.
        client.mint_certificate(
            args.0, args.1, args.2, args.3, args.4, args.5, args.6, args.7,
        );
    }

    // ── is_minted Tests ───────────────────────────────────────────────────

    #[test]
    fn test_is_minted() {
        let (env, contract_id, _) = setup();
        let client = CompletionCertificateContractClient::new(&env, &contract_id);

        let owner = Address::generate(&env);
        let p_id  = String::from_str(&env, "P-01");

        assert!(!client.is_minted(&p_id, &owner));

        client.mint_certificate(
            &owner, &p_id,
            &String::from_str(&env, "Puzzle"),
            &120u64, &2u64,
            &String::from_str(&env, "h"),
            &String::from_str(&env, "u"),
            &false,
        );

        assert!(client.is_minted(&p_id, &owner));
    }

    // ── Transfer Tests ────────────────────────────────────────────────────

    #[test]
    fn test_transfer_transferable_cert() {
        let (env, contract_id, _) = setup();
        let client = CompletionCertificateContractClient::new(&env, &contract_id);

        let owner = Address::generate(&env);
        let new_owner = Address::generate(&env);
        let p_id = String::from_str(&env, "P-T");

        let token_id = client.mint_certificate(
            &owner, &p_id,
            &String::from_str(&env, "Transferable"),
            &50u64, &1u64,
            &String::from_str(&env, "h"),
            &String::from_str(&env, "u"),
            &true, // transferable
        );

        client.transfer(&owner, &new_owner, &token_id);

        let cert = client.get_certificate(&token_id);
        assert_eq!(cert.owner, new_owner);

        // Old owner should have no certs.
        let old_list = client.get_owner_certificates(&owner);
        assert_eq!(old_list.len(), 0);

        // New owner should have the cert.
        let new_list = client.get_owner_certificates(&new_owner);
        assert_eq!(new_list.len(), 1);
    }

    #[test]
    #[should_panic]
    fn test_transfer_non_transferable_panics() {
        let (env, contract_id, _) = setup();
        let client = CompletionCertificateContractClient::new(&env, &contract_id);

        let owner = Address::generate(&env);
        let other = Address::generate(&env);
        let p_id  = String::from_str(&env, "P-NT");

        let token_id = client.mint_certificate(
            &owner, &p_id,
            &String::from_str(&env, "Soulbound"),
            &50u64, &1u64,
            &String::from_str(&env, "h"),
            &String::from_str(&env, "u"),
            &false, // NOT transferable
        );

        client.transfer(&owner, &other, &token_id);
    }

    // ── Burn Tests ────────────────────────────────────────────────────────

    #[test]
    fn test_burn_certificate() {
        let (env, contract_id, _) = setup();
        let client = CompletionCertificateContractClient::new(&env, &contract_id);

        let owner = Address::generate(&env);
        let p_id  = String::from_str(&env, "P-BURN");

        let token_id = client.mint_certificate(
            &owner, &p_id,
            &String::from_str(&env, "Burnable"),
            &100u64, &3u64,
            &String::from_str(&env, "h"),
            &String::from_str(&env, "u"),
            &true,
        );

        client.burn(&owner, &token_id);

        let cert = client.get_certificate(&token_id);
        assert!(cert.burned);

        // Owner list should be empty.
        assert_eq!(client.get_owner_certificates(&owner).len(), 0);

        // Proof should show not authentic.
        let proof = client.verify_certificate(&token_id);
        assert!(!proof.authentic);
    }

    #[test]
    #[should_panic]
    fn test_burn_by_non_owner_panics() {
        let (env, contract_id, _) = setup();
        let client = CompletionCertificateContractClient::new(&env, &contract_id);

        let owner = Address::generate(&env);
        let other = Address::generate(&env);
        let p_id  = String::from_str(&env, "P-BURN2");

        let token_id = client.mint_certificate(
            &owner, &p_id,
            &String::from_str(&env, "T"),
            &100u64, &1u64,
            &String::from_str(&env, "h"),
            &String::from_str(&env, "u"),
            &true,
        );

        client.burn(&other, &token_id); // must panic
    }

    // ── Verification Tests ────────────────────────────────────────────────

    #[test]
    fn test_verify_valid_certificate() {
        let (env, contract_id, _) = setup();
        let client = CompletionCertificateContractClient::new(&env, &contract_id);

        let owner    = Address::generate(&env);
        let p_id     = String::from_str(&env, "P-VER");
        let sol_hash = String::from_str(&env, "deadbeef");

        let token_id = client.mint_certificate(
            &owner, &p_id,
            &String::from_str(&env, "Verify Me"),
            &250u64, &5u64,
            &sol_hash,
            &String::from_str(&env, "uri"),
            &true,
        );

        let proof = client.verify_certificate(&token_id);
        assert!(proof.authentic);
        assert_eq!(proof.owner,         owner);
        assert_eq!(proof.puzzle_id,     p_id);
        assert_eq!(proof.solution_hash, sol_hash);
        assert_eq!(proof.rarity,        RarityTier::Epic); // 250 s → Epic
        assert_eq!(proof.rank,          5);
    }

    #[test]
    fn test_verify_nonexistent_returns_inauthentic() {
        let (env, contract_id, _) = setup();
        let client = CompletionCertificateContractClient::new(&env, &contract_id);
        let proof = client.verify_certificate(&999u64);
        assert!(!proof.authentic);
    }

    // ── Showcase Tests ────────────────────────────────────────────────────

    #[test]
    fn test_showcase_sorted_by_rarity() {
        let (env, contract_id, _) = setup();
        let client = CompletionCertificateContractClient::new(&env, &contract_id);

        let owner = Address::generate(&env);

        // Mint Common (5000 s), Epic (200 s), Rare (600 s).
        for (p, secs) in [("P-C", 5000u64), ("P-E", 200u64), ("P-R", 600u64)] {
            client.mint_certificate(
                &owner,
                &String::from_str(&env, p),
                &String::from_str(&env, "Title"),
                &secs, &1u64,
                &String::from_str(&env, "h"),
                &String::from_str(&env, "u"),
                &true,
            );
        }

        let showcase = client.get_showcase(&owner);
        assert_eq!(showcase.len(), 3);

        // First should be Epic (weight 4), then Rare (3), then Common (1).
        assert_eq!(showcase.get(0).unwrap().rarity, RarityTier::Epic);
        assert_eq!(showcase.get(1).unwrap().rarity, RarityTier::Rare);
        assert_eq!(showcase.get(2).unwrap().rarity, RarityTier::Common);
    }

    // ── Pause Tests ───────────────────────────────────────────────────────

    #[test]
    #[should_panic]
    fn test_mint_when_paused_panics() {
        let (env, contract_id, _) = setup();
        let client = CompletionCertificateContractClient::new(&env, &contract_id);

        client.set_paused(&true);
        assert!(client.is_paused());

        let owner = Address::generate(&env);
        client.mint_certificate(
            &owner,
            &String::from_str(&env, "P-PAUSE"),
            &String::from_str(&env, "T"),
            &100u64, &1u64,
            &String::from_str(&env, "h"),
            &String::from_str(&env, "u"),
            &true,
        );
    }

    #[test]
    fn test_unpause_allows_mint() {
        let (env, contract_id, _) = setup();
        let client = CompletionCertificateContractClient::new(&env, &contract_id);

        client.set_paused(&true);
        client.set_paused(&false);
        assert!(!client.is_paused());

        let owner = Address::generate(&env);
        let id = client.mint_certificate(
            &owner,
            &String::from_str(&env, "P-UNPAUSE"),
            &String::from_str(&env, "T"),
            &100u64, &1u64,
            &String::from_str(&env, "h"),
            &String::from_str(&env, "u"),
            &true,
        );
        assert_eq!(id, 1);
    }

    // ── Admin Transfer Tests ──────────────────────────────────────────────

    #[test]
    fn test_set_admin() {
        let (env, contract_id, _old_admin) = setup();
        let client  = CompletionCertificateContractClient::new(&env, &contract_id);
        let new_admin = Address::generate(&env);

        client.set_admin(&new_admin);
        assert_eq!(client.get_admin(), new_admin);
    }

    // ── Owner Gallery Tests ───────────────────────────────────────────────

    #[test]
    fn test_get_owner_certificates_empty() {
        let (env, contract_id, _) = setup();
        let client = CompletionCertificateContractClient::new(&env, &contract_id);
        let nobody = Address::generate(&env);
        assert_eq!(client.get_owner_certificates(&nobody).len(), 0);
    }

    #[test]
    fn test_get_owner_certificates_multiple() {
        let (env, contract_id, _) = setup();
        let client = CompletionCertificateContractClient::new(&env, &contract_id);
        let owner  = Address::generate(&env);

        for p in ["PA", "PB", "PC"] {
            client.mint_certificate(
                &owner,
                &String::from_str(&env, p),
                &String::from_str(&env, "Title"),
                &100u64, &1u64,
                &String::from_str(&env, "h"),
                &String::from_str(&env, "u"),
                &true,
            );
        }

        assert_eq!(client.get_owner_certificates(&owner).len(), 3);
    }
}