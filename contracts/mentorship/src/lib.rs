#![no_std]

use soroban_sdk::{
    contract, contractimpl, contracttype, contracterror,
    panic_with_error, symbol_short,
    Address, Env, String, Symbol, Vec, token,
};

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum Error {
    MentorNotFound = 1,
    StudentNotFound = 2,
    SessionNotFound = 3,
    MilestoneNotFound = 4,
    UnauthorizedCaller = 5,
    InvalidRating = 6,
    SessionNotActive = 7,
    MilestoneAlreadyVerified = 8,
    MilestoneNotSubmitted = 9,
    PoolNotDeposited = 10,
    AlreadyRegistered = 11,
    ListingNotFound = 12,
    AlreadyReviewed = 13,
    SessionNotCompleted = 14,
    MilestonesIncomplete = 15,
    InvalidMilestoneCount = 16,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SessionStatus {
    Pending,
    Active,
    Completed,
    Cancelled,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MilestoneStatus {
    Pending,
    Submitted,
    Verified,
    Rejected,
}

#[contracttype]
#[derive(Clone)]
pub struct MilestoneInput {
    pub description: String,
    pub reward_amount: i128,
}

#[contracttype]
#[derive(Clone)]
pub struct Milestone {
    pub index: u32,
    pub description: String,
    pub reward_amount: i128,
    pub status: MilestoneStatus,
}

#[contracttype]
#[derive(Clone)]
pub struct MentorProfile {
    pub mentor: Address,
    pub expertise: String,
    pub hourly_rate: i128,
    pub total_sessions: u32,
    pub completed_sessions: u32,
    pub rating_sum: u32,
    pub rating_count: u32,
    pub active: bool,
}

#[contracttype]
#[derive(Clone)]
pub struct StudentProfile {
    pub student: Address,
    pub total_sessions: u32,
    pub completed_sessions: u32,
    pub certificates_earned: u32,
    pub active: bool,
}

#[contracttype]
#[derive(Clone)]
pub struct MentorshipSession {
    pub session_id: u64,
    pub mentor: Address,
    pub student: Address,
    pub start_time: u64,
    pub duration: u64,
    pub milestone_count: u32,
    pub milestones_verified: u32,
    pub reward_pool: i128,
    pub reward_distributed: i128,
    pub mentor_share: u32,
    pub pool_deposited: bool,
    pub status: SessionStatus,
    pub reviewed: bool,
}

#[contracttype]
#[derive(Clone)]
pub struct Certificate {
    pub session_id: u64,
    pub student: Address,
    pub mentor: Address,
    pub milestones_completed: u32,
    pub issued_at: u64,
}

#[contracttype]
#[derive(Clone)]
pub struct Review {
    pub review_id: u64,
    pub session_id: u64,
    pub student: Address,
    pub mentor: Address,
    pub rating: u32,
    pub comment: String,
    pub created_at: u64,
}

#[contracttype]
#[derive(Clone)]
pub struct MarketplaceListing {
    pub listing_id: u64,
    pub mentor: Address,
    pub expertise: String,
    pub duration: u64,
    pub milestone_count: u32,
    pub reward_pool: i128,
    pub mentor_share: u32,
    pub active: bool,
}

const SESSION_COUNTER: Symbol = symbol_short!("S_CNT");
const REVIEW_COUNTER: Symbol = symbol_short!("R_CNT");
const LISTING_COUNTER: Symbol = symbol_short!("L_CNT");
const ADMIN: Symbol = symbol_short!("ADMIN");
const TOKEN: Symbol = symbol_short!("TOKEN");

#[contracttype]
pub enum DataKey {
    MentorProfile(Address),
    StudentProfile(Address),
    Session(u64),
    Milestone(u64, u32),
    Certificate(u64),
    Review(u64),
    Listing(u64),
}

#[contract]
pub struct MentorshipContract;

#[contractimpl]
impl MentorshipContract {
    pub fn initialize(env: Env, admin: Address, token: Address) {
        env.storage().instance().set(&ADMIN, &admin);
        env.storage().instance().set(&TOKEN, &token);
        env.storage().instance().set(&SESSION_COUNTER, &0u64);
        env.storage().instance().set(&REVIEW_COUNTER, &0u64);
        env.storage().instance().set(&LISTING_COUNTER, &0u64);
    }

    pub fn register_mentor(env: Env, mentor: Address, expertise: String, hourly_rate: i128) {
        mentor.require_auth();

        if env
            .storage()
            .persistent()
            .has(&DataKey::MentorProfile(mentor.clone()))
        {
            panic_with_error!(&env, Error::AlreadyRegistered);
        }

        let profile = MentorProfile {
            mentor: mentor.clone(),
            expertise,
            hourly_rate,
            total_sessions: 0,
            completed_sessions: 0,
            rating_sum: 0,
            rating_count: 0,
            active: true,
        };

        env.storage()
            .persistent()
            .set(&DataKey::MentorProfile(mentor), &profile);
    }

    pub fn register_student(env: Env, student: Address) {
        student.require_auth();

        if env
            .storage()
            .persistent()
            .has(&DataKey::StudentProfile(student.clone()))
        {
            panic_with_error!(&env, Error::AlreadyRegistered);
        }

        let profile = StudentProfile {
            student: student.clone(),
            total_sessions: 0,
            completed_sessions: 0,
            certificates_earned: 0,
            active: true,
        };

        env.storage()
            .persistent()
            .set(&DataKey::StudentProfile(student), &profile);
    }

    pub fn create_session(
        env: Env,
        student: Address,
        mentor: Address,
        milestones: Vec<MilestoneInput>,
        duration: u64,
        mentor_share: u32,
    ) -> u64 {
        student.require_auth();

        if !env
            .storage()
            .persistent()
            .has(&DataKey::MentorProfile(mentor.clone()))
        {
            panic_with_error!(&env, Error::MentorNotFound);
        }
        if !env
            .storage()
            .persistent()
            .has(&DataKey::StudentProfile(student.clone()))
        {
            panic_with_error!(&env, Error::StudentNotFound);
        }
        if mentor_share > 100 {
            panic_with_error!(&env, Error::UnauthorizedCaller);
        }
        if milestones.is_empty() {
            panic_with_error!(&env, Error::InvalidMilestoneCount);
        }

        let session_id: u64 = env.storage().instance().get(&SESSION_COUNTER).unwrap_or(0) + 1;
        env.storage().instance().set(&SESSION_COUNTER, &session_id);

        let mut reward_pool: i128 = 0;
        let milestone_count = milestones.len();

        for (i, input) in milestones.iter().enumerate() {
            reward_pool += input.reward_amount;
            let milestone = Milestone {
                index: i as u32,
                description: input.description.clone(),
                reward_amount: input.reward_amount,
                status: MilestoneStatus::Pending,
            };
            env.storage()
                .persistent()
                .set(&DataKey::Milestone(session_id, i as u32), &milestone);
        }

        let session = MentorshipSession {
            session_id,
            mentor: mentor.clone(),
            student: student.clone(),
            start_time: 0,
            duration,
            milestone_count,
            milestones_verified: 0,
            reward_pool,
            reward_distributed: 0,
            mentor_share,
            pool_deposited: false,
            status: SessionStatus::Pending,
            reviewed: false,
        };

        env.storage()
            .persistent()
            .set(&DataKey::Session(session_id), &session);

        let mut mentor_profile: MentorProfile = env
            .storage()
            .persistent()
            .get(&DataKey::MentorProfile(mentor.clone()))
            .unwrap_or_else(|| panic_with_error!(&env, Error::MentorNotFound));
        mentor_profile.total_sessions += 1;
        env.storage()
            .persistent()
            .set(&DataKey::MentorProfile(mentor), &mentor_profile);

        let mut student_profile: StudentProfile = env
            .storage()
            .persistent()
            .get(&DataKey::StudentProfile(student.clone()))
            .unwrap_or_else(|| panic_with_error!(&env, Error::StudentNotFound));
        student_profile.total_sessions += 1;
        env.storage()
            .persistent()
            .set(&DataKey::StudentProfile(student), &student_profile);

        session_id
    }

    pub fn deposit_reward_pool(env: Env, session_id: u64, depositor: Address) {
        depositor.require_auth();

        let mut session: MentorshipSession = env
            .storage()
            .persistent()
            .get(&DataKey::Session(session_id))
            .unwrap_or_else(|| panic_with_error!(&env, Error::SessionNotFound));

        if session.pool_deposited {
            return;
        }

        let token_addr: Address = env.storage().instance().get(&TOKEN).unwrap();
        token::Client::new(&env, &token_addr).transfer(
            &depositor,
            &env.current_contract_address(),
            &session.reward_pool,
        );

        session.pool_deposited = true;
        env.storage()
            .persistent()
            .set(&DataKey::Session(session_id), &session);
    }

    pub fn start_session(env: Env, session_id: u64) {
        let mut session: MentorshipSession = env
            .storage()
            .persistent()
            .get(&DataKey::Session(session_id))
            .unwrap_or_else(|| panic_with_error!(&env, Error::SessionNotFound));

        session.mentor.require_auth();

        if session.status != SessionStatus::Pending {
            panic_with_error!(&env, Error::SessionNotActive);
        }
        if !session.pool_deposited {
            panic_with_error!(&env, Error::PoolNotDeposited);
        }

        session.start_time = env.ledger().timestamp();
        session.status = SessionStatus::Active;
        env.storage()
            .persistent()
            .set(&DataKey::Session(session_id), &session);
    }

    pub fn submit_milestone(env: Env, session_id: u64, milestone_idx: u32) {
        let session: MentorshipSession = env
            .storage()
            .persistent()
            .get(&DataKey::Session(session_id))
            .unwrap_or_else(|| panic_with_error!(&env, Error::SessionNotFound));

        session.student.require_auth();

        if session.status != SessionStatus::Active {
            panic_with_error!(&env, Error::SessionNotActive);
        }

        let mut milestone: Milestone = env
            .storage()
            .persistent()
            .get(&DataKey::Milestone(session_id, milestone_idx))
            .unwrap_or_else(|| panic_with_error!(&env, Error::MilestoneNotFound));

        if milestone.status == MilestoneStatus::Verified {
            panic_with_error!(&env, Error::MilestoneAlreadyVerified);
        }

        milestone.status = MilestoneStatus::Submitted;
        env.storage()
            .persistent()
            .set(&DataKey::Milestone(session_id, milestone_idx), &milestone);
    }

    pub fn verify_milestone(env: Env, session_id: u64, milestone_idx: u32) {
        let mut session: MentorshipSession = env
            .storage()
            .persistent()
            .get(&DataKey::Session(session_id))
            .unwrap_or_else(|| panic_with_error!(&env, Error::SessionNotFound));

        session.mentor.require_auth();

        if session.status != SessionStatus::Active {
            panic_with_error!(&env, Error::SessionNotActive);
        }

        let mut milestone: Milestone = env
            .storage()
            .persistent()
            .get(&DataKey::Milestone(session_id, milestone_idx))
            .unwrap_or_else(|| panic_with_error!(&env, Error::MilestoneNotFound));

        if milestone.status == MilestoneStatus::Verified {
            panic_with_error!(&env, Error::MilestoneAlreadyVerified);
        }
        if milestone.status != MilestoneStatus::Submitted {
            panic_with_error!(&env, Error::MilestoneNotSubmitted);
        }

        let token_addr: Address = env.storage().instance().get(&TOKEN).unwrap();
        let token_client = token::Client::new(&env, &token_addr);
        let contract_addr = env.current_contract_address();

        let mentor_amount = (milestone.reward_amount * session.mentor_share as i128) / 100;
        let student_amount = milestone.reward_amount - mentor_amount;
        let mentor = session.mentor.clone();
        let student = session.student.clone();

        if mentor_amount > 0 {
            token_client.transfer(&contract_addr, &mentor, &mentor_amount);
        }
        if student_amount > 0 {
            token_client.transfer(&contract_addr, &student, &student_amount);
        }

        milestone.status = MilestoneStatus::Verified;
        env.storage()
            .persistent()
            .set(&DataKey::Milestone(session_id, milestone_idx), &milestone);

        session.milestones_verified += 1;
        session.reward_distributed += milestone.reward_amount;
        env.storage()
            .persistent()
            .set(&DataKey::Session(session_id), &session);
    }

    pub fn reject_milestone(env: Env, session_id: u64, milestone_idx: u32) {
        let session: MentorshipSession = env
            .storage()
            .persistent()
            .get(&DataKey::Session(session_id))
            .unwrap_or_else(|| panic_with_error!(&env, Error::SessionNotFound));

        session.mentor.require_auth();

        if session.status != SessionStatus::Active {
            panic_with_error!(&env, Error::SessionNotActive);
        }

        let mut milestone: Milestone = env
            .storage()
            .persistent()
            .get(&DataKey::Milestone(session_id, milestone_idx))
            .unwrap_or_else(|| panic_with_error!(&env, Error::MilestoneNotFound));

        if milestone.status != MilestoneStatus::Submitted {
            panic_with_error!(&env, Error::MilestoneNotSubmitted);
        }

        milestone.status = MilestoneStatus::Rejected;
        env.storage()
            .persistent()
            .set(&DataKey::Milestone(session_id, milestone_idx), &milestone);
    }

    pub fn complete_session(env: Env, session_id: u64) {
        let mut session: MentorshipSession = env
            .storage()
            .persistent()
            .get(&DataKey::Session(session_id))
            .unwrap_or_else(|| panic_with_error!(&env, Error::SessionNotFound));

        if session.status != SessionStatus::Active {
            panic_with_error!(&env, Error::SessionNotActive);
        }
        if session.milestones_verified < session.milestone_count {
            panic_with_error!(&env, Error::MilestonesIncomplete);
        }

        let student = session.student.clone();
        let mentor = session.mentor.clone();

        let certificate = Certificate {
            session_id,
            student: student.clone(),
            mentor: mentor.clone(),
            milestones_completed: session.milestones_verified,
            issued_at: env.ledger().timestamp(),
        };
        env.storage()
            .persistent()
            .set(&DataKey::Certificate(session_id), &certificate);

        let mut mentor_profile: MentorProfile = env
            .storage()
            .persistent()
            .get(&DataKey::MentorProfile(mentor.clone()))
            .unwrap_or_else(|| panic_with_error!(&env, Error::MentorNotFound));
        mentor_profile.completed_sessions += 1;
        env.storage()
            .persistent()
            .set(&DataKey::MentorProfile(mentor), &mentor_profile);

        let mut student_profile: StudentProfile = env
            .storage()
            .persistent()
            .get(&DataKey::StudentProfile(student.clone()))
            .unwrap_or_else(|| panic_with_error!(&env, Error::StudentNotFound));
        student_profile.completed_sessions += 1;
        student_profile.certificates_earned += 1;
        env.storage()
            .persistent()
            .set(&DataKey::StudentProfile(student), &student_profile);

        session.status = SessionStatus::Completed;
        env.storage()
            .persistent()
            .set(&DataKey::Session(session_id), &session);
    }

    pub fn rate_mentor(
        env: Env,
        session_id: u64,
        rating: u32,
        comment: String,
    ) -> u64 {
        let mut session: MentorshipSession = env
            .storage()
            .persistent()
            .get(&DataKey::Session(session_id))
            .unwrap_or_else(|| panic_with_error!(&env, Error::SessionNotFound));

        session.student.require_auth();

        if session.status != SessionStatus::Completed {
            panic_with_error!(&env, Error::SessionNotCompleted);
        }
        if session.reviewed {
            panic_with_error!(&env, Error::AlreadyReviewed);
        }
        if rating < 1 || rating > 5 {
            panic_with_error!(&env, Error::InvalidRating);
        }

        let review_id: u64 = env.storage().instance().get(&REVIEW_COUNTER).unwrap_or(0) + 1;
        env.storage().instance().set(&REVIEW_COUNTER, &review_id);

        let review = Review {
            review_id,
            session_id,
            student: session.student.clone(),
            mentor: session.mentor.clone(),
            rating,
            comment,
            created_at: env.ledger().timestamp(),
        };
        env.storage()
            .persistent()
            .set(&DataKey::Review(review_id), &review);

        let mut mentor_profile: MentorProfile = env
            .storage()
            .persistent()
            .get(&DataKey::MentorProfile(session.mentor.clone()))
            .unwrap_or_else(|| panic_with_error!(&env, Error::MentorNotFound));
        mentor_profile.rating_sum += rating;
        mentor_profile.rating_count += 1;
        env.storage()
            .persistent()
            .set(&DataKey::MentorProfile(session.mentor.clone()), &mentor_profile);

        session.reviewed = true;
        env.storage()
            .persistent()
            .set(&DataKey::Session(session_id), &session);

        review_id
    }

    pub fn list_on_marketplace(
        env: Env,
        mentor: Address,
        expertise: String,
        duration: u64,
        milestone_count: u32,
        reward_pool: i128,
        mentor_share: u32,
    ) -> u64 {
        mentor.require_auth();

        if !env
            .storage()
            .persistent()
            .has(&DataKey::MentorProfile(mentor.clone()))
        {
            panic_with_error!(&env, Error::MentorNotFound);
        }
        if mentor_share > 100 {
            panic_with_error!(&env, Error::UnauthorizedCaller);
        }

        let listing_id: u64 = env.storage().instance().get(&LISTING_COUNTER).unwrap_or(0) + 1;
        env.storage().instance().set(&LISTING_COUNTER, &listing_id);

        let listing = MarketplaceListing {
            listing_id,
            mentor,
            expertise,
            duration,
            milestone_count,
            reward_pool,
            mentor_share,
            active: true,
        };
        env.storage()
            .persistent()
            .set(&DataKey::Listing(listing_id), &listing);

        listing_id
    }

    pub fn take_from_marketplace(
        env: Env,
        listing_id: u64,
        student: Address,
        milestones: Vec<MilestoneInput>,
    ) -> u64 {
        student.require_auth();

        if !env
            .storage()
            .persistent()
            .has(&DataKey::StudentProfile(student.clone()))
        {
            panic_with_error!(&env, Error::StudentNotFound);
        }

        let mut listing: MarketplaceListing = env
            .storage()
            .persistent()
            .get(&DataKey::Listing(listing_id))
            .unwrap_or_else(|| panic_with_error!(&env, Error::ListingNotFound));

        if !listing.active {
            panic_with_error!(&env, Error::ListingNotFound);
        }
        if milestones.len() != listing.milestone_count {
            panic_with_error!(&env, Error::InvalidMilestoneCount);
        }

        listing.active = false;
        env.storage()
            .persistent()
            .set(&DataKey::Listing(listing_id), &listing);

        let session_id: u64 = env.storage().instance().get(&SESSION_COUNTER).unwrap_or(0) + 1;
        env.storage().instance().set(&SESSION_COUNTER, &session_id);

        let mut reward_pool: i128 = 0;
        for (i, input) in milestones.iter().enumerate() {
            reward_pool += input.reward_amount;
            let milestone = Milestone {
                index: i as u32,
                description: input.description.clone(),
                reward_amount: input.reward_amount,
                status: MilestoneStatus::Pending,
            };
            env.storage()
                .persistent()
                .set(&DataKey::Milestone(session_id, i as u32), &milestone);
        }

        let session = MentorshipSession {
            session_id,
            mentor: listing.mentor.clone(),
            student: student.clone(),
            start_time: 0,
            duration: listing.duration,
            milestone_count: listing.milestone_count,
            milestones_verified: 0,
            reward_pool,
            reward_distributed: 0,
            mentor_share: listing.mentor_share,
            pool_deposited: false,
            status: SessionStatus::Pending,
            reviewed: false,
        };
        env.storage()
            .persistent()
            .set(&DataKey::Session(session_id), &session);

        let mut mentor_profile: MentorProfile = env
            .storage()
            .persistent()
            .get(&DataKey::MentorProfile(listing.mentor.clone()))
            .unwrap_or_else(|| panic_with_error!(&env, Error::MentorNotFound));
        mentor_profile.total_sessions += 1;
        env.storage()
            .persistent()
            .set(&DataKey::MentorProfile(listing.mentor), &mentor_profile);

        let mut student_profile: StudentProfile = env
            .storage()
            .persistent()
            .get(&DataKey::StudentProfile(student.clone()))
            .unwrap_or_else(|| panic_with_error!(&env, Error::StudentNotFound));
        student_profile.total_sessions += 1;
        env.storage()
            .persistent()
            .set(&DataKey::StudentProfile(student), &student_profile);

        session_id
    }

    pub fn get_mentor_profile(env: Env, mentor: Address) -> MentorProfile {
        env.storage()
            .persistent()
            .get(&DataKey::MentorProfile(mentor))
            .unwrap_or_else(|| panic_with_error!(&env, Error::MentorNotFound))
    }

    pub fn get_student_profile(env: Env, student: Address) -> StudentProfile {
        env.storage()
            .persistent()
            .get(&DataKey::StudentProfile(student))
            .unwrap_or_else(|| panic_with_error!(&env, Error::StudentNotFound))
    }

    pub fn get_session(env: Env, session_id: u64) -> MentorshipSession {
        env.storage()
            .persistent()
            .get(&DataKey::Session(session_id))
            .unwrap_or_else(|| panic_with_error!(&env, Error::SessionNotFound))
    }

    pub fn get_milestone(env: Env, session_id: u64, milestone_idx: u32) -> Milestone {
        env.storage()
            .persistent()
            .get(&DataKey::Milestone(session_id, milestone_idx))
            .unwrap_or_else(|| panic_with_error!(&env, Error::MilestoneNotFound))
    }

    pub fn get_certificate(env: Env, session_id: u64) -> Certificate {
        env.storage()
            .persistent()
            .get(&DataKey::Certificate(session_id))
            .unwrap_or_else(|| panic_with_error!(&env, Error::SessionNotFound))
    }

    pub fn get_review(env: Env, review_id: u64) -> Review {
        env.storage()
            .persistent()
            .get(&DataKey::Review(review_id))
            .unwrap_or_else(|| panic_with_error!(&env, Error::SessionNotFound))
    }

    pub fn get_listing(env: Env, listing_id: u64) -> MarketplaceListing {
        env.storage()
            .persistent()
            .get(&DataKey::Listing(listing_id))
            .unwrap_or_else(|| panic_with_error!(&env, Error::ListingNotFound))
    }

    pub fn get_mentor_rating(env: Env, mentor: Address) -> u32 {
        let profile: MentorProfile = env
            .storage()
            .persistent()
            .get(&DataKey::MentorProfile(mentor))
            .unwrap_or_else(|| panic_with_error!(&env, Error::MentorNotFound));

        if profile.rating_count == 0 {
            return 0;
        }
        (profile.rating_sum * 100) / profile.rating_count
    }
}

#[cfg(test)]
mod test;
