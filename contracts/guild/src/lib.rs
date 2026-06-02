#![no_std]

use soroban_sdk::{
    contract, contractimpl, contracttype,
    Address, Env, String, Vec, Map, Symbol, token,
};

//
// ──────────────────────────────────────────────────────────
// ROLES
// ──────────────────────────────────────────────────────────
//

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Role {
    Member = 0,
    Officer = 1,
    Leader = 2,
}

//
// ──────────────────────────────────────────────────────────
// DATA KEYS
// ──────────────────────────────────────────────────────────
//

#[contracttype]
pub enum DataKey {
    Config,                    // GuildConfig
    Member(Address),           // Role
    MembersList,               // Vec<Address>
    TreasuryToken,             // Address
    Resource(Symbol),          // i128
    Achievement(Symbol),       // u32
    Proposal(u32),             // Proposal
    ProposalCounter,           // u32
    Competition(u32),          // Competition
    WithdrawalProposal(u32),   // WithdrawalProposal
    WithdrawalProposalCounter, // u32
    WithdrawalVoted(u32, Address), // bool
}

//
// ──────────────────────────────────────────────────────────
// STRUCTS
// ──────────────────────────────────────────────────────────
//

#[contracttype]
#[derive(Clone, Debug)]
pub struct GuildConfig {
    pub name: String,
    pub disbanded: bool,
    pub withdrawal_threshold: i128,
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct Proposal {
    pub id: u32,
    pub yes: u32,
    pub no: u32,
    pub deadline: u64,
    pub executed: bool,
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct WithdrawalProposal {
    pub id: u32,
    pub officer: Address,
    pub amount: i128,
    pub yes: u32,
    pub no: u32,
    pub deadline: u64,
    pub executed: bool,
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct Competition {
    pub opponent: Address,
    pub reward: i128,
    pub won: bool,
}

//
// ──────────────────────────────────────────────────────────
// CONTRACT
// ──────────────────────────────────────────────────────────
//

#[contract]
pub struct GuildContract;

#[contractimpl]
impl GuildContract {

    // ───────────── INITIALIZATION ─────────────

    pub fn initialize(env: Env, leader: Address, name: String, token_address: Address) {
        leader.require_auth();

        if env.storage().persistent().has(&DataKey::Config) {
            panic!("Already initialized");
        }

        env.storage().persistent().set(
            &DataKey::Config,
            &GuildConfig { name, disbanded: false, withdrawal_threshold: 10_000 },
        );

        env.storage().instance().set(&DataKey::TreasuryToken, &token_address);
        Self::set_role_internal(&env, leader, Role::Leader);
    }

    // ───────────── MEMBERSHIP ─────────────

    pub fn join(env: Env, user: Address) {
        user.require_auth();
        Self::assert_active(&env);

        if Self::get_role(env.clone(), user.clone()).is_some() {
            panic!("Already a member");
        }

        Self::set_role_internal(&env, user, Role::Member);
    }

    pub fn set_role(env: Env, leader: Address, target: Address, role: Role) {
        leader.require_auth();
        Self::assert_leader(&env, &leader);
        Self::assert_active(&env);

        Self::set_role_internal(&env, target, role);
    }

    // ───────────── TREASURY ─────────────

    pub fn deposit(env: Env, member: Address, amount: i128) {
        member.require_auth();
        Self::assert_active(&env);

        let token_addr: Address =
            env.storage().instance().get(&DataKey::TreasuryToken).unwrap();
        let client = token::Client::new(&env, &token_addr);

        client.transfer(&member, &env.current_contract_address(), &amount);
    }

    pub fn withdraw(env: Env, officer: Address, amount: i128, deadline: Option<u64>) -> Option<u32> {
        officer.require_auth();
        Self::assert_officer_or_leader(&env, &officer);
        Self::assert_active(&env);

        let config: GuildConfig = env.storage().persistent().get(&DataKey::Config).unwrap();
        
        if amount > config.withdrawal_threshold {
            let mut id: u32 = env.storage().persistent().get(&DataKey::WithdrawalProposalCounter).unwrap_or(0);
            id += 1;
            let d = deadline.unwrap_or(env.ledger().timestamp() + 86400); // 24h default
            
            let wp = WithdrawalProposal {
                id, officer, amount, yes: 0, no: 0, deadline: d, executed: false
            };
            
            env.storage().persistent().set(&DataKey::WithdrawalProposal(id), &wp);
            env.storage().persistent().set(&DataKey::WithdrawalProposalCounter, &id);
            return Some(id);
        }

        let token_addr: Address =
            env.storage().instance().get(&DataKey::TreasuryToken).unwrap();
        let client = token::Client::new(&env, &token_addr);

        let balance = client.balance(&env.current_contract_address());
        if balance < amount {
            panic!("Insufficient funds");
        }

        client.transfer(&env.current_contract_address(), &officer, &amount);
        None
    }

    pub fn vote_withdrawal(env: Env, member: Address, proposal_id: u32, approve: bool) {
        member.require_auth();
        Self::assert_active(&env);

        if Self::get_role(env.clone(), member.clone()).is_none() {
            panic!("Not a member");
        }

        let voted_key = DataKey::WithdrawalVoted(proposal_id, member.clone());
        if env.storage().persistent().has(&voted_key) {
            panic!("Already voted");
        }

        let mut proposal: WithdrawalProposal = env.storage().persistent().get(&DataKey::WithdrawalProposal(proposal_id)).unwrap();

        if env.ledger().timestamp() > proposal.deadline {
            panic!("Voting closed");
        }

        if proposal.executed {
            panic!("Already executed");
        }

        if approve {
            proposal.yes += 1;
        } else {
            proposal.no += 1;
        }

        env.storage().persistent().set(&DataKey::WithdrawalProposal(proposal_id), &proposal);
        env.storage().persistent().set(&voted_key, &true);
    }

    pub fn execute_withdrawal(env: Env, executor: Address, proposal_id: u32) {
        executor.require_auth();
        Self::assert_active(&env);

        let mut proposal: WithdrawalProposal = env.storage().persistent().get(&DataKey::WithdrawalProposal(proposal_id)).unwrap();

        if env.ledger().timestamp() > proposal.deadline {
            panic!("Proposal expired");
        }

        if proposal.executed {
            panic!("Already executed");
        }

        let members: Vec<Address> = env.storage().persistent().get(&DataKey::MembersList).unwrap();
        let total_members = members.len() as u32;

        if proposal.yes <= total_members / 2 {
            panic!("Not enough approvals");
        }

        proposal.executed = true;
        env.storage().persistent().set(&DataKey::WithdrawalProposal(proposal_id), &proposal);

        let token_addr: Address = env.storage().instance().get(&DataKey::TreasuryToken).unwrap();
        let client = token::Client::new(&env, &token_addr);

        let balance = client.balance(&env.current_contract_address());
        if balance < proposal.amount {
            panic!("Insufficient funds");
        }

        client.transfer(&env.current_contract_address(), &proposal.officer, &proposal.amount);
    }

    // ───────────── SHARED RESOURCES ─────────────

    pub fn add_resource(env: Env, officer: Address, resource: Symbol, amount: i128) {
        officer.require_auth();
        Self::assert_officer_or_leader(&env, &officer);
        Self::assert_active(&env);

        let mut current: i128 =
            env.storage().persistent().get(&DataKey::Resource(resource.clone())).unwrap_or(0);

        current += amount;
        env.storage().persistent().set(&DataKey::Resource(resource), &current);
    }

    // ───────────── ACHIEVEMENTS ─────────────

    pub fn add_achievement(env: Env, officer: Address, achievement: Symbol) {
        officer.require_auth();
        Self::assert_officer_or_leader(&env, &officer);
        Self::assert_active(&env);

        let mut count: u32 =
            env.storage().persistent().get(&DataKey::Achievement(achievement.clone())).unwrap_or(0);

        count += 1;
        env.storage().persistent().set(&DataKey::Achievement(achievement), &count);
    }

    // ───────────── VOTING ─────────────

    pub fn create_proposal(env: Env, officer: Address, deadline: u64) -> u32 {
        officer.require_auth();
        Self::assert_officer_or_leader(&env, &officer);
        Self::assert_active(&env);

        let mut id: u32 =
            env.storage().persistent().get(&DataKey::ProposalCounter).unwrap_or(0);
        id += 1;

        let proposal = Proposal {
            id,
            yes: 0,
            no: 0,
            deadline,
            executed: false,
        };

        env.storage().persistent().set(&DataKey::Proposal(id), &proposal);
        env.storage().persistent().set(&DataKey::ProposalCounter, &id);

        id
    }

    pub fn vote(env: Env, member: Address, proposal_id: u32, approve: bool) {
        member.require_auth();
        Self::assert_active(&env);

        if Self::get_role(env.clone(), member).is_none() {
            panic!("Not a member");
        }

        let mut proposal: Proposal =
            env.storage().persistent().get(&DataKey::Proposal(proposal_id)).unwrap();

        if env.ledger().timestamp() > proposal.deadline {
            panic!("Voting closed");
        }

        if approve {
            proposal.yes += 1;
        } else {
            proposal.no += 1;
        }

        env.storage().persistent().set(&DataKey::Proposal(proposal_id), &proposal);
    }

    // ───────────── INTER-GUILD COMPETITION ─────────────

    pub fn record_competition(
        env: Env,
        leader: Address,
        opponent: Address,
        reward: i128,
        won: bool,
    ) {
        leader.require_auth();
        Self::assert_leader(&env, &leader);
        Self::assert_active(&env);

        let id = env.ledger().timestamp() as u32;

        env.storage().persistent().set(
            &DataKey::Competition(id),
            &Competition { opponent, reward, won },
        );
    }

    // ───────────── DISBAND ─────────────

    pub fn disband(env: Env, leader: Address) {
        leader.require_auth();
        Self::assert_leader(&env, &leader);

        let mut config: GuildConfig =
            env.storage().persistent().get(&DataKey::Config).unwrap();

        if config.disbanded {
            panic!("Already disbanded");
        }

        let token_addr: Address =
            env.storage().instance().get(&DataKey::TreasuryToken).unwrap();
        let client = token::Client::new(&env, &token_addr);

        let members: Vec<Address> =
            env.storage().persistent().get(&DataKey::MembersList).unwrap();

        let total = client.balance(&env.current_contract_address());
        let share = total / members.len() as i128;

        for m in members.iter() {
            client.transfer(&env.current_contract_address(), &m, &share);
        }

        config.disbanded = true;
        env.storage().persistent().set(&DataKey::Config, &config);
    }

    // ───────────── HELPERS ─────────────

    fn set_role_internal(env: &Env, user: Address, role: Role) {
        env.storage().persistent().set(&DataKey::Member(user.clone()), &role);

        let mut members: Vec<Address> =
            env.storage().persistent().get(&DataKey::MembersList).unwrap_or(Vec::new(env));

        if !members.contains(&user) {
            members.push_back(user);
            env.storage().persistent().set(&DataKey::MembersList, &members);
        }
    }

    pub fn get_role(env: Env, user: Address) -> Option<Role> {
        env.storage().persistent().get(&DataKey::Member(user))
    }

    fn assert_leader(env: &Env, user: &Address) {
        if Self::get_role(env.clone(), user.clone()) != Some(Role::Leader) {
            panic!("Leader only");
        }
    }

    fn assert_officer_or_leader(env: &Env, user: &Address) {
        match Self::get_role(env.clone(), user.clone()) {
            Some(Role::Leader) | Some(Role::Officer) => {}
            _ => panic!("Officer or Leader only"),
        }
    }

    fn assert_active(env: &Env) {
        let cfg: GuildConfig =
            env.storage().persistent().get(&DataKey::Config).unwrap();
        if cfg.disbanded {
            panic!("Guild disbanded");
        }
    }
}

mod test;