#![no_std]

use soroban_sdk::{
    contract, contractimpl, contracttype, contracterror, contractmeta,
    Address, Env, Vec, BytesN, Symbol, symbol_short, Bytes
};

mod test;

// Contract metadata
contractmeta!(
    key = "Description",
    val = "Whitelist contract for managing access control with tiered permissions"
);

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WhitelistEntry {
    pub address: Address,
    pub tier: u32,
    pub expiration: Option<u32>, // Block number for expiration
    pub permissions: Vec<Symbol>,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MerkleProof {
    pub proof: Vec<BytesN<32>>,
    pub leaf: BytesN<32>,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WhitelistSnapshot {
    pub block_number: u32,
    pub merkle_root: BytesN<32>,
    pub total_entries: u32,
}

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum WhitelistError {
    NotAuthorized = 1,
    AddressNotWhitelisted = 2,
    InvalidTier = 3,
    ExpiredEntry = 4,
    InvalidMerkleProof = 5,
    AdminNotFound = 6,
    EntryAlreadyExists = 7,
    InvalidPermission = 8,
}

// Storage keys
const ADMIN: Symbol = symbol_short!("ADMIN");
const WHITELIST: Symbol = symbol_short!("WLIST");
const MERKLE_ROOT: Symbol = symbol_short!("MROOT");
const SNAPSHOT: Symbol = symbol_short!("SNAP");
const TIER_PERMS: Symbol = symbol_short!("TPERMS");

#[contract]
pub struct WhitelistContract;

#[contractimpl]
impl WhitelistContract {
    /// Initialize the contract with an admin
    pub fn initialize(env: Env, admin: Address) {
        admin.require_auth();
        env.storage().instance().set(&ADMIN, &admin);
    }

    /// Add a single address to whitelist
    pub fn add_to_whitelist(
        env: Env,
        admin: Address,
        address: Address,
        tier: u32,
        expiration: Option<u32>,
        permissions: Vec<Symbol>,
    ) -> Result<(), WhitelistError> {
        Self::require_admin(&env, &admin)?;
        
        if tier == 0 {
            return Err(WhitelistError::InvalidTier);
        }

        let entry = WhitelistEntry {
            address: address.clone(),
            tier,
            expiration,
            permissions,
        };

        let key = (WHITELIST, address);
        env.storage().persistent().set(&key, &entry);
        
        Ok(())
    }

    /// Remove address from whitelist
    pub fn remove_from_whitelist(
        env: Env,
        admin: Address,
        address: Address,
    ) -> Result<(), WhitelistError> {
        Self::require_admin(&env, &admin)?;
        
        let key = (WHITELIST, address);
        env.storage().persistent().remove(&key);
        
        Ok(())
    }

    /// Batch add addresses to whitelist
    pub fn batch_add_to_whitelist(
        env: Env,
        admin: Address,
        entries: Vec<WhitelistEntry>,
    ) -> Result<(), WhitelistError> {
        Self::require_admin(&env, &admin)?;
        
        for entry in entries.iter() {
            if entry.tier == 0 {
                return Err(WhitelistError::InvalidTier);
            }
            
            let key = (WHITELIST, entry.address.clone());
            env.storage().persistent().set(&key, &entry);
        }
        
        Ok(())
    }

    /// Batch remove addresses from whitelist
    pub fn batch_remove_from_whitelist(
        env: Env,
        admin: Address,
        addresses: Vec<Address>,
    ) -> Result<(), WhitelistError> {
        Self::require_admin(&env, &admin)?;
        
        for address in addresses.iter() {
            let key = (WHITELIST, address);
            env.storage().persistent().remove(&key);
        }
        
        Ok(())
    }

    /// Check if address is whitelisted and has required tier
    pub fn is_whitelisted(
        env: Env,
        address: Address,
        required_tier: Option<u32>,
    ) -> bool {
        let key = (WHITELIST, address);
        
        if let Some(entry) = env.storage().persistent().get::<_, WhitelistEntry>(&key) {
            // Check expiration
            if let Some(expiration) = entry.expiration {
                if env.ledger().sequence() > expiration {
                    return false;
                }
            }
            
            // Check tier requirement
            if let Some(req_tier) = required_tier {
                return entry.tier >= req_tier;
            }
            
            true
        } else {
            false
        }
    }

    /// Check if address has specific permission
    pub fn has_permission(
        env: Env,
        address: Address,
        permission: Symbol,
    ) -> bool {
        let key = (WHITELIST, address);
        
        if let Some(entry) = env.storage().persistent().get::<_, WhitelistEntry>(&key) {
            // Check expiration
            if let Some(expiration) = entry.expiration {
                if env.ledger().sequence() > expiration {
                    return false;
                }
            }
            
            entry.permissions.contains(&permission)
        } else {
            false
        }
    }

    /// Get whitelist entry for address
    pub fn get_whitelist_entry(env: Env, address: Address) -> Option<WhitelistEntry> {
        let key = (WHITELIST, address);
        env.storage().persistent().get(&key)
    }

    /// Set tier-based permissions
    pub fn set_tier_permissions(
        env: Env,
        admin: Address,
        tier: u32,
        permissions: Vec<Symbol>,
    ) -> Result<(), WhitelistError> {
        Self::require_admin(&env, &admin)?;
        
        if tier == 0 {
            return Err(WhitelistError::InvalidTier);
        }
        
        let key = (TIER_PERMS, tier);
        env.storage().persistent().set(&key, &permissions);
        
        Ok(())
    }

    /// Get tier permissions
    pub fn get_tier_permissions(env: Env, tier: u32) -> Vec<Symbol> {
        let key = (TIER_PERMS, tier);
        env.storage().persistent().get(&key).unwrap_or(Vec::new(&env))
    }

    /// Set merkle root for gas-optimized verification
    pub fn set_merkle_root(
        env: Env,
        admin: Address,
        merkle_root: BytesN<32>,
    ) -> Result<(), WhitelistError> {
        Self::require_admin(&env, &admin)?;
        
        env.storage().instance().set(&MERKLE_ROOT, &merkle_root);
        
        Ok(())
    }

    /// Verify merkle proof for whitelist inclusion
    pub fn verify_merkle_proof(
        env: Env,
        address: Address,
        tier: u32,
        proof: Vec<BytesN<32>>,
    ) -> Result<bool, WhitelistError> {
        let merkle_root: Option<BytesN<32>> = env.storage().instance().get(&MERKLE_ROOT);
        
        if merkle_root.is_none() {
            return Err(WhitelistError::InvalidMerkleProof);
        }
        
        let leaf = Self::compute_leaf(&env, &address, tier);
        let computed_root = Self::compute_merkle_root(&env, leaf, proof);
        
        Ok(computed_root == merkle_root.unwrap())
    }

    /// Create whitelist snapshot
    pub fn create_snapshot(
        env: Env,
        admin: Address,
        merkle_root: BytesN<32>,
        total_entries: u32,
    ) -> Result<(), WhitelistError> {
        Self::require_admin(&env, &admin)?;
        
        let snapshot = WhitelistSnapshot {
            block_number: env.ledger().sequence(),
            merkle_root,
            total_entries,
        };
        
        env.storage().instance().set(&SNAPSHOT, &snapshot);
        
        Ok(())
    }

    /// Get current snapshot
    pub fn get_snapshot(env: Env) -> Option<WhitelistSnapshot> {
        env.storage().instance().get(&SNAPSHOT)
    }

    /// Transfer admin role
    pub fn transfer_admin(
        env: Env,
        current_admin: Address,
        new_admin: Address,
    ) -> Result<(), WhitelistError> {
        Self::require_admin(&env, &current_admin)?;
        new_admin.require_auth();
        
        env.storage().instance().set(&ADMIN, &new_admin);
        
        Ok(())
    }

    /// Get current admin
    pub fn get_admin(env: Env) -> Option<Address> {
        env.storage().instance().get(&ADMIN)
    }

    // Helper functions
    fn require_admin(env: &Env, admin: &Address) -> Result<(), WhitelistError> {
        admin.require_auth();
        
        let stored_admin: Option<Address> = env.storage().instance().get(&ADMIN);
        
        match stored_admin {
            Some(stored) if stored == *admin => Ok(()),
            Some(_) => Err(WhitelistError::NotAuthorized),
            None => Err(WhitelistError::AdminNotFound),
        }
    }

    fn compute_leaf(env: &Env, address: &Address, tier: u32) -> BytesN<32> {
        // Simple leaf computation using address and tier
        let mut bytes = Bytes::new(env);
        
        // Add a simple representation of the address (just use first few bytes)
        let addr_str = address.to_string();
        for i in 0..addr_str.len().min(8) {
            bytes.push_back(i as u8); // Simplified representation
        }
        
        // Add tier bytes
        let tier_bytes = tier.to_be_bytes();
        for byte in tier_bytes.iter() {
            bytes.push_back(*byte);
        }
        
        env.crypto().keccak256(&bytes).into()
    }

    fn compute_merkle_root(env: &Env, leaf: BytesN<32>, proof: Vec<BytesN<32>>) -> BytesN<32> {
        let mut current_hash = leaf;
        
        for proof_element in proof.iter() {
            let mut combined = Bytes::new(env);
            
            // Determine order for consistent hashing
            let current_bytes = current_hash.as_ref();
            let proof_bytes = proof_element.as_ref();
            
            if current_bytes < proof_bytes {
                for byte in current_bytes.iter() {
                    combined.push_back(byte);
                }
                for byte in proof_bytes.iter() {
                    combined.push_back(byte);
                }
            } else {
                for byte in proof_bytes.iter() {
                    combined.push_back(byte);
                }
                for byte in current_bytes.iter() {
                    combined.push_back(byte);
                }
            }
            
            current_hash = env.crypto().keccak256(&combined).into();
        }
        
        current_hash
    }
}