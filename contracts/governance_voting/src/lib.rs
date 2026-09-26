use soroban_sdk::{contractimpl, Address, Env, Symbol};

#[derive(Clone)]
pub struct Proposal {
    pub id: u64,
    pub proposer: Address,
    pub voting_start: u64,
    pub voting_end: u64,
    pub votes_for: i128,
    pub votes_against: i128,
    pub quorum: i128,
    pub executed: bool,
}

pub struct GovernanceVotingContract;

const VOTING_DELAY: u64 = 86_400; // seconds before voting opens

#[contractimpl]
impl GovernanceVotingContract {
    pub fn create_proposal(env: Env, proposer: Address, quorum: i128) -> u64 {
        proposer.require_auth();
        let now = env.ledger().timestamp();
        let id: u64 = env.storage().instance().get(&Symbol::new(&env, "next_id")).unwrap_or(0);
        env.storage().instance().set(&Symbol::new(&env, "next_id"), &(id + 1));

        env.storage().persistent().set(&id, &Proposal {
            id,
            proposer,
            voting_start: now + VOTING_DELAY,
            voting_end: now + VOTING_DELAY + 604_800,
            votes_for: 0,
            votes_against: 0,
            quorum,
            executed: false,
        });
        id
    }

    pub fn cast_vote(env: Env, voter: Address, proposal_id: u64, weight: i128, support: bool) {
        voter.require_auth();
        let mut p: Proposal = env.storage().persistent().get(&proposal_id).unwrap();
        let now = env.ledger().timestamp();
        assert!(now >= p.voting_start && now <= p.voting_end, "not in voting window");
        if support { p.votes_for += weight } else { p.votes_against += weight }
        env.storage().persistent().set(&proposal_id, &p);
    }
}
