#![no_std]

use soroban_sdk::{contract, contractimpl, contracttype, token, Address, Env, Vec};

#[contracttype]
#[derive(Clone, PartialEq)]
pub enum PoolStatus {
    Open,
    Distributed,
    Closed,
}

#[contracttype]
#[derive(Clone)]
pub struct Pool {
    pub id: u32,
    pub admin: Address,
    pub total: i128,
    pub min_threshold: i128,
    pub status: PoolStatus,
    pub created_at: u64,
    pub claim_period: u64,
}

#[contracttype]
#[derive(Clone)]
pub struct Contribution {
    pub contributor: Address,
    pub amount: i128,
}

#[contracttype]
#[derive(Clone)]
pub struct Distribution {
    pub pool_id: u32,
    pub winners: Vec<Address>,
    pub amounts: Vec<i128>,
    pub claimed: Vec<bool>,
    pub distributed_at: u64,
}

#[contracttype]
pub enum DataKey {
    Owner,
    Token,
    NextPoolId,
    Pool(u32),
    Contributions(u32),
    Distribution(u32),
    Stats,
}

#[contracttype]
#[derive(Clone)]
pub struct Stats {
    pub total_contributed: i128,
    pub num_contributors: u32,
    pub total_distributed: i128,
}

#[contract]
pub struct PrizePoolContract;

mod test;

#[contractimpl]
impl PrizePoolContract {
    pub fn init(e: Env, owner: Address, token: Address) {
        owner.require_auth();
        if e.storage().instance().has(&DataKey::Owner) {
            panic!("Already initialized");
        }
        e.storage().instance().set(&DataKey::Owner, &owner);
        e.storage().instance().set(&DataKey::Token, &token);
        e.storage().instance().set(&DataKey::NextPoolId, &1u32);
        let stats = Stats {
            total_contributed: 0,
            num_contributors: 0,
            total_distributed: 0,
        };
        e.storage().instance().set(&DataKey::Stats, &stats);
    }

    pub fn create_pool(e: Env, admin: Address, min_threshold: i128, claim_period: u64) -> u32 {
        let owner: Address = e.storage().instance().get(&DataKey::Owner).unwrap();
        owner.require_auth();

        let mut id: u32 = e.storage().instance().get(&DataKey::NextPoolId).unwrap();
        let now = e.ledger().timestamp();

        let pool = Pool {
            id,
            admin: admin.clone(),
            total: 0,
            min_threshold,
            status: PoolStatus::Open,
            created_at: now,
            claim_period,
        };

        e.storage().persistent().set(&DataKey::Pool(id), &pool);
        e.storage().persistent().set(&DataKey::Contributions(id), &Vec::<Contribution>::new(&e));
        id += 1;
        e.storage().instance().set(&DataKey::NextPoolId, &id);
        id - 1
    }

    pub fn contribute(e: Env, user: Address, pool_id: u32, amount: i128) {
        user.require_auth();
        if amount <= 0 {
            panic!("Invalid amount");
        }

        let mut pool: Pool = e.storage().persistent().get(&DataKey::Pool(pool_id)).unwrap();
        if pool.status != PoolStatus::Open {
            panic!("Pool not open");
        }

        let token: Address = e.storage().instance().get(&DataKey::Token).unwrap();
        let client = token::Client::new(&e, &token);
        client.transfer(&user, &e.current_contract_address(), &amount);

        let mut contributions: Vec<Contribution> = e.storage().persistent().get(&DataKey::Contributions(pool_id)).unwrap();
        contributions.push_back(Contribution { contributor: user.clone(), amount });
        e.storage().persistent().set(&DataKey::Contributions(pool_id), &contributions);

        pool.total += amount;
        e.storage().persistent().set(&DataKey::Pool(pool_id), &pool);

        let mut stats: Stats = e.storage().instance().get(&DataKey::Stats).unwrap();
        stats.total_contributed += amount;
        stats.num_contributors += 1;
        e.storage().instance().set(&DataKey::Stats, &stats);
    }

    pub fn distribute(e: Env, admin: Address, pool_id: u32, winners: Vec<Address>) {
        admin.require_auth();

        let mut pool: Pool = e.storage().persistent().get(&DataKey::Pool(pool_id)).unwrap();
        if pool.status != PoolStatus::Open {
            panic!("Pool not open for distribution");
        }

        if pool.total < pool.min_threshold {
            panic!("Pool below minimum threshold");
        }

        if winners.len() == 0 {
            panic!("No winners provided");
        }

        // Only pool admin can distribute
        if admin != pool.admin {
            panic!("Only pool admin can distribute");
        }

        let now = e.ledger().timestamp();
        // Equal split for simplicity
        let per = pool.total / (winners.len() as i128);
        let mut amounts: Vec<i128> = Vec::new(&e);
        for _ in 0..winners.len() {
            amounts.push_back(per);
        }

        // If there's remainder, keep in pool as rollover/unclaimed
        let distributed_sum = per * (winners.len() as i128);

        let claimed_vec: Vec<bool> = Vec::new(&e);

        let distr = Distribution {
            pool_id,
            winners: winners.clone(),
            amounts: amounts.clone(),
            claimed: claimed_vec.clone(),
            distributed_at: now,
        };

        e.storage().persistent().set(&DataKey::Distribution(pool_id), &distr);

        // Mark pool as Distributed but keep funds in contract for winners to claim
        pool.status = PoolStatus::Distributed;
        // reduce pool.total by distributed_sum (remaining is unclaimed/rollover)
        pool.total = pool.total - distributed_sum;
        e.storage().persistent().set(&DataKey::Pool(pool_id), &pool);
    }

    pub fn claim(e: Env, user: Address, pool_id: u32) {
        user.require_auth();

        let mut distr: Distribution = e.storage().persistent().get(&DataKey::Distribution(pool_id)).unwrap();

        // find user in winners (use u32 indices)
        let mut idx: Option<u32> = None;
        for i in 0..distr.winners.len() {
            if distr.winners.get(i).unwrap() == user.clone() {
                idx = Some(i);
                break;
            }
        }
        if idx.is_none() {
            panic!("Not a winner");
        }
        let idx_u = idx.unwrap();

        let mut claimed_vec = distr.claimed.clone();
        // ensure claimed_vec has same length as winners (initialize false if empty)
        if claimed_vec.len() < distr.winners.len() {
            for _j in claimed_vec.len()..distr.winners.len() {
                claimed_vec.push_back(false);
            }
        }

        if claimed_vec.get(idx_u).unwrap() {
            panic!("Already claimed");
        }

        let amount = distr.amounts.get(idx_u).unwrap();

        let token: Address = e.storage().instance().get(&DataKey::Token).unwrap();
        let client = token::Client::new(&e, &token);
        client.transfer(&e.current_contract_address(), &user, &amount);

        claimed_vec.set(idx_u, true);
        distr.claimed = claimed_vec.clone();
        e.storage().persistent().set(&DataKey::Distribution(pool_id), &distr);

        let mut stats: Stats = e.storage().instance().get(&DataKey::Stats).unwrap();
        stats.total_distributed += amount;
        e.storage().instance().set(&DataKey::Stats, &stats);
    }

    pub fn rollover_unclaimed(e: Env, owner: Address, pool_id: u32, target_pool_id: u32) {
        owner.require_auth();

        let pool: Pool = e.storage().persistent().get(&DataKey::Pool(pool_id)).unwrap();
        let mut target: Pool = e.storage().persistent().get(&DataKey::Pool(target_pool_id)).unwrap();

        let distr: Distribution = e.storage().persistent().get(&DataKey::Distribution(pool_id)).unwrap();

        let now = e.ledger().timestamp();

        // Only allow rollover after claim period
        if now < distr.distributed_at + pool.claim_period {
            panic!("Claim period not yet expired");
        }

        // calc unclaimed
        let mut unclaimed: i128 = 0;
        let mut claimed_vec = distr.claimed.clone();
        if claimed_vec.len() < distr.winners.len() {
            for _ in claimed_vec.len()..distr.winners.len() {
                claimed_vec.push_back(false);
            }
        }

        for i in 0..distr.winners.len() {
            if !claimed_vec.get(i).unwrap() {
                unclaimed += distr.amounts.get(i).unwrap();
                claimed_vec.set(i, true); // mark as processed
            }
        }

        if unclaimed == 0 {
            return;
        }

        // add unclaimed to target pool total
        target.total += unclaimed;
        e.storage().persistent().set(&DataKey::Pool(target_pool_id), &target);

        // update distribution record to mark all as claimed now
        let mut new_distr = distr.clone();
        new_distr.claimed = claimed_vec.clone();
        e.storage().persistent().set(&DataKey::Distribution(pool_id), &new_distr);
    }

    // Views
    pub fn get_pool(e: Env, pool_id: u32) -> Pool {
        e.storage().persistent().get(&DataKey::Pool(pool_id)).unwrap()
    }

    pub fn get_stats(e: Env) -> Stats {
        e.storage().instance().get(&DataKey::Stats).unwrap()
    }

    pub fn get_distribution(e: Env, pool_id: u32) -> Distribution {
        e.storage().persistent().get(&DataKey::Distribution(pool_id)).unwrap()
    }
}
