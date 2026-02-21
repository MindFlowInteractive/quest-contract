#![no_std]
use soroban_sdk::{contract, contractimpl, contracttype, Address, Env, String, Vec, Map, token};

#[cfg(test)]
mod test;

#[contracttype]
#[derive(Clone)]
pub struct Charity {
    pub id: u32,
    pub name: String,
    pub wallet: Address,
    pub verified: bool,
    pub total_raised: i128,
    pub contributor_count: u32,
}

#[contracttype]
#[derive(Clone)]
pub struct Donation {
    pub donor: Address,
    pub charity_id: u32,
    pub amount: i128,
    pub timestamp: u64,
    pub recurring: bool,
}

#[contracttype]
#[derive(Clone)]
pub struct Receipt {
    pub id: u32,
    pub donor: Address,
    pub charity_id: u32,
    pub total_donated: i128,
}

#[contracttype]
pub enum DataKey {
    Admin,
    TokenAddr,
    MatchingPool,
    NextCharityId,
    NextReceiptId,
    Charity(u32),
    DonorTotal(Address, u32),
    DonorList(u32),
    RecurringDonation(Address, u32),
    Receipt(u32),
    Leaderboard,
}

#[contract]
pub struct CharityContract;

#[contractimpl]
impl CharityContract {
    pub fn initialize(env: Env, admin: Address, token: Address) {
        if env.storage().instance().has(&DataKey::Admin) {
            panic!("Already initialized");
        }
        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage().instance().set(&DataKey::TokenAddr, &token);
        env.storage().instance().set(&DataKey::MatchingPool, &0i128);
        env.storage().instance().set(&DataKey::NextCharityId, &1u32);
        env.storage().instance().set(&DataKey::NextReceiptId, &1u32);
        env.storage().instance().set(&DataKey::Leaderboard, &Vec::<(Address, i128)>::new(&env));
    }

    pub fn add_charity(env: Env, admin: Address, name: String, wallet: Address) -> u32 {
        admin.require_auth();
        let stored_admin: Address = env.storage().instance().get(&DataKey::Admin).unwrap();
        if admin != stored_admin {
            panic!("Unauthorized");
        }

        let id: u32 = env.storage().instance().get(&DataKey::NextCharityId).unwrap();
        let charity = Charity {
            id,
            name,
            wallet,
            verified: false,
            total_raised: 0,
            contributor_count: 0,
        };
        
        env.storage().persistent().set(&DataKey::Charity(id), &charity);
        env.storage().persistent().set(&DataKey::DonorList(id), &Vec::<Address>::new(&env));
        env.storage().instance().set(&DataKey::NextCharityId, &(id + 1));
        id
    }

    pub fn verify_charity(env: Env, admin: Address, charity_id: u32) {
        admin.require_auth();
        let stored_admin: Address = env.storage().instance().get(&DataKey::Admin).unwrap();
        if admin != stored_admin {
            panic!("Unauthorized");
        }

        let mut charity: Charity = env.storage().persistent().get(&DataKey::Charity(charity_id)).unwrap();
        charity.verified = true;
        env.storage().persistent().set(&DataKey::Charity(charity_id), &charity);
    }

    pub fn donate(env: Env, donor: Address, charity_id: u32, amount: i128) {
        donor.require_auth();
        if amount <= 0 {
            panic!("Invalid amount");
        }

        let mut charity: Charity = env.storage().persistent().get(&DataKey::Charity(charity_id)).unwrap();
        if !charity.verified {
            panic!("Charity not verified");
        }

        let token_addr: Address = env.storage().instance().get(&DataKey::TokenAddr).unwrap();
        let token = token::Client::new(&env, &token_addr);
        token.transfer(&donor, &charity.wallet, &amount);

        let donor_key = DataKey::DonorTotal(donor.clone(), charity_id);
        let prev_total: i128 = env.storage().persistent().get(&donor_key).unwrap_or(0);
        
        if prev_total == 0 {
            charity.contributor_count += 1;
            let mut donors: Vec<Address> = env.storage().persistent().get(&DataKey::DonorList(charity_id)).unwrap();
            donors.push_back(donor.clone());
            env.storage().persistent().set(&DataKey::DonorList(charity_id), &donors);
        }

        let new_total = prev_total + amount;
        env.storage().persistent().set(&donor_key, &new_total);
        charity.total_raised += amount;
        env.storage().persistent().set(&DataKey::Charity(charity_id), &charity);

        Self::update_leaderboard(env.clone(), donor.clone(), amount);
    }

    pub fn fund_matching_pool(env: Env, funder: Address, amount: i128) {
        funder.require_auth();
        let token_addr: Address = env.storage().instance().get(&DataKey::TokenAddr).unwrap();
        let token = token::Client::new(&env, &token_addr);
        token.transfer(&funder, &env.current_contract_address(), &amount);

        let pool: i128 = env.storage().instance().get(&DataKey::MatchingPool).unwrap();
        env.storage().instance().set(&DataKey::MatchingPool, &(pool + amount));
    }

    pub fn distribute_matching(env: Env, admin: Address, charity_id: u32) -> i128 {
        admin.require_auth();
        let stored_admin: Address = env.storage().instance().get(&DataKey::Admin).unwrap();
        if admin != stored_admin {
            panic!("Unauthorized");
        }

        let charity: Charity = env.storage().persistent().get(&DataKey::Charity(charity_id)).unwrap();
        let donors: Vec<Address> = env.storage().persistent().get(&DataKey::DonorList(charity_id)).unwrap();
        
        let mut sum_sqrt = 0i128;
        for donor in donors.iter() {
            let amount: i128 = env.storage().persistent().get(&DataKey::DonorTotal(donor.clone(), charity_id)).unwrap();
            sum_sqrt += Self::sqrt(amount);
        }
        
        let qf_amount = sum_sqrt * sum_sqrt - charity.total_raised;
        let pool: i128 = env.storage().instance().get(&DataKey::MatchingPool).unwrap();
        let payout = if qf_amount > pool { pool } else { qf_amount };

        if payout > 0 {
            let token_addr: Address = env.storage().instance().get(&DataKey::TokenAddr).unwrap();
            let token = token::Client::new(&env, &token_addr);
            token.transfer(&env.current_contract_address(), &charity.wallet, &payout);
            env.storage().instance().set(&DataKey::MatchingPool, &(pool - payout));
        }

        payout
    }

    pub fn issue_receipt(env: Env, donor: Address, charity_id: u32) -> u32 {
        donor.require_auth();
        let total: i128 = env.storage().persistent().get(&DataKey::DonorTotal(donor.clone(), charity_id)).unwrap_or(0);
        if total == 0 {
            panic!("No donations found");
        }

        let id: u32 = env.storage().instance().get(&DataKey::NextReceiptId).unwrap();
        let receipt = Receipt { id, donor: donor.clone(), charity_id, total_donated: total };
        env.storage().persistent().set(&DataKey::Receipt(id), &receipt);
        env.storage().instance().set(&DataKey::NextReceiptId, &(id + 1));
        id
    }

    pub fn set_recurring(env: Env, donor: Address, charity_id: u32, amount: i128, enabled: bool) {
        donor.require_auth();
        let key = DataKey::RecurringDonation(donor.clone(), charity_id);
        if enabled {
            env.storage().persistent().set(&key, &amount);
        } else {
            env.storage().persistent().remove(&key);
        }
    }

    pub fn get_charity(env: Env, charity_id: u32) -> Charity {
        env.storage().persistent().get(&DataKey::Charity(charity_id)).unwrap()
    }

    pub fn get_leaderboard(env: Env) -> Vec<(Address, i128)> {
        env.storage().instance().get(&DataKey::Leaderboard).unwrap()
    }

    pub fn get_donor_total(env: Env, donor: Address, charity_id: u32) -> i128 {
        env.storage().persistent().get(&DataKey::DonorTotal(donor, charity_id)).unwrap_or(0)
    }

    fn sqrt(n: i128) -> i128 {
        if n == 0 { return 0; }
        let mut x = n;
        let mut y = (x + 1) / 2;
        while y < x {
            x = y;
            y = (x + n / x) / 2;
        }
        x
    }

    fn update_leaderboard(env: Env, donor: Address, amount: i128) {
        let mut board: Vec<(Address, i128)> = env.storage().instance().get(&DataKey::Leaderboard).unwrap();
        let mut found = false;
        
        for i in 0..board.len() {
            if board.get(i).unwrap().0 == donor {
                let current = board.get(i).unwrap();
                board.set(i, (donor.clone(), current.1 + amount));
                found = true;
                break;
            }
        }
        
        if !found {
            board.push_back((donor, amount));
        }

        for i in 0..board.len() {
            for j in (i + 1)..board.len() {
                if board.get(j).unwrap().1 > board.get(i).unwrap().1 {
                    let temp = board.get(i).unwrap();
                    board.set(i, board.get(j).unwrap());
                    board.set(j, temp);
                }
            }
        }

        if board.len() > 10 {
            board.pop_back();
        }

        env.storage().instance().set(&DataKey::Leaderboard, &board);
    }
}
