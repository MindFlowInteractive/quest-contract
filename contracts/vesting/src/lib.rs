#![no_std]
use soroban_sdk::{
    contract, contractimpl, contracttype, token, Address, Env, String, Vec,
};

#[derive(Clone, Debug, Eq, PartialEq)]
#[contracttype]
pub enum DataKey {
    Admin,
    Token,
    Paused,
    VestingSchedule(Address),
    NextScheduleId,
}

#[derive(Clone, Debug, Eq, PartialEq)]
#[contracttype]
pub struct VestingSchedule {
    pub schedule_id: u64,
    pub beneficiary: Address,
    pub total_amount: i128,
    pub released_amount: i128,
    pub start_time: u64,
    pub cliff_duration: u64,
    pub vesting_duration: u64,
    pub revocable: bool,
    pub revoked: bool,
    pub milestones: Vec<Milestone>,
    pub vesting_type: VestingType,
}

#[derive(Clone, Debug, Eq, PartialEq)]
#[contracttype]
pub struct Milestone {
    pub id: u32,
    pub name: String,
    pub percentage: u32,
    pub completed: bool,
    pub completion_time: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
#[contracttype]
pub enum VestingType {
    TimeBased,
    MilestoneBased,
    Hybrid,
}

#[derive(Clone, Debug, Eq, PartialEq)]
#[contracttype]
pub enum VestingEvent {
    ScheduleCreated(u64, Address, i128), // schedule_id, beneficiary, total_amount
    TokensReleased(u64, Address, i128),  // schedule_id, beneficiary, amount
    MilestoneCompleted(u64, u32),       // schedule_id, milestone_id
    ScheduleRevoked(u64, i128),         // schedule_id, unvested_amount
    ScheduleModified(u64),              // schedule_id
    ContractPaused,
    ContractUnpaused,
}

#[contract]
pub struct VestingContract;

#[contractimpl]
impl VestingContract {
    /// Initialize the vesting contract
    /// 
    /// # Arguments
    /// * `admin` - Address that will have admin privileges
    /// * `token` - Address of the token contract to be vested
    pub fn initialize(env: Env, admin: Address, token: Address) {
        if env.storage().instance().has(&DataKey::Admin) {
            panic!("Already initialized");
        }

        admin.require_auth();

        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage().instance().set(&DataKey::Token, &token);
        env.storage().instance().set(&DataKey::Paused, &false);
        env.storage().instance().set(&DataKey::NextScheduleId, &0u64);
    }

    /// Create a new vesting schedule
    /// 
    /// # Arguments
    /// * `beneficiary` - Address that will receive vested tokens
    /// * `total_amount` - Total amount of tokens to vest
    /// * `start_time` - Unix timestamp when vesting starts
    /// * `cliff_duration` - Duration (seconds) before any tokens vest
    /// * `vesting_duration` - Total duration (seconds) for full vesting
    /// * `revocable` - Whether admin can revoke this schedule
    /// * `vesting_type` - Type of vesting (TimeBased, MilestoneBased, or Hybrid)
    /// * `milestones` - List of milestones (required for MilestoneBased/Hybrid)
    /// 
    /// # Returns
    /// The schedule ID
    pub fn create_schedule(
        env: Env,
        beneficiary: Address,
        total_amount: i128,
        start_time: u64,
        cliff_duration: u64,
        vesting_duration: u64,
        revocable: bool,
        vesting_type: VestingType,
        milestones: Vec<Milestone>,
    ) -> u64 {
        let admin: Address = env.storage().instance().get(&DataKey::Admin).unwrap();
        admin.require_auth();

        Self::require_not_paused(&env);

        if total_amount <= 0 {
            panic!("Total amount must be positive");
        }

        if vesting_duration == 0 {
            panic!("Vesting duration must be positive");
        }

        // Validate milestones if provided
        if !milestones.is_empty() {
            let mut total_percentage = 0u32;
            for milestone in milestones.iter() {
                total_percentage += milestone.percentage;
            }
            if total_percentage != 10000 {
                panic!("Milestone percentages must sum to 100%");
            }
        }

        // Validate milestone requirement for milestone-based vesting
        match vesting_type {
            VestingType::MilestoneBased | VestingType::Hybrid => {
                if milestones.is_empty() {
                    panic!("Milestones required for milestone-based vesting");
                }
            }
            _ => {}
        }

        let schedule_id = Self::get_next_schedule_id(&env);

        let schedule = VestingSchedule {
            schedule_id,
            beneficiary: beneficiary.clone(),
            total_amount,
            released_amount: 0,
            start_time,
            cliff_duration,
            vesting_duration,
            revocable,
            revoked: false,
            milestones,
            vesting_type,
        };

        env.storage()
            .persistent()
            .set(&DataKey::VestingSchedule(beneficiary.clone()), &schedule);

        // Transfer tokens to contract
        let token_address: Address = env.storage().instance().get(&DataKey::Token).unwrap();
        let token_client = token::Client::new(&env, &token_address);
        token_client.transfer(&admin, &env.current_contract_address(), &total_amount);

        env.events().publish(
            (String::from_str(&env, "schedule_created"), schedule_id),
            VestingEvent::ScheduleCreated(schedule_id, beneficiary, total_amount),
        );

        schedule_id
    }

    /// Release vested tokens to the beneficiary
    /// 
    /// # Arguments
    /// * `beneficiary` - Address to release tokens to
    /// 
    /// # Returns
    /// Amount of tokens released
    pub fn release(env: Env, beneficiary: Address) -> i128 {
        beneficiary.require_auth();
        Self::require_not_paused(&env);

        let mut schedule: VestingSchedule = env
            .storage()
            .persistent()
            .get(&DataKey::VestingSchedule(beneficiary.clone()))
            .expect("No vesting schedule found");

        if schedule.revoked {
            panic!("Schedule has been revoked");
        }

        let vested_amount = Self::calculate_vested_amount(&env, &schedule);
        let releasable = vested_amount - schedule.released_amount;

        if releasable <= 0 {
            panic!("No tokens available for release");
        }

        schedule.released_amount += releasable;
        env.storage()
            .persistent()
            .set(&DataKey::VestingSchedule(beneficiary.clone()), &schedule);

        // Transfer tokens to beneficiary
        let token_address: Address = env.storage().instance().get(&DataKey::Token).unwrap();
        let token_client = token::Client::new(&env, &token_address);
        token_client.transfer(&env.current_contract_address(), &beneficiary, &releasable);

        env.events().publish(
            (String::from_str(&env, "tokens_released"), schedule.schedule_id),
            VestingEvent::TokensReleased(schedule.schedule_id, beneficiary, releasable),
        );

        releasable
    }

    /// Complete a milestone (admin only)
    /// 
    /// # Arguments
    /// * `beneficiary` - Address whose milestone to complete
    /// * `milestone_id` - ID of the milestone to mark as completed
    pub fn complete_milestone(env: Env, beneficiary: Address, milestone_id: u32) {
        let admin: Address = env.storage().instance().get(&DataKey::Admin).unwrap();
        admin.require_auth();
        Self::require_not_paused(&env);

        let mut schedule: VestingSchedule = env
            .storage()
            .persistent()
            .get(&DataKey::VestingSchedule(beneficiary.clone()))
            .expect("No vesting schedule found");

        if schedule.revoked {
            panic!("Schedule has been revoked");
        }

        let mut milestone_found = false;
        let mut updated_milestones = Vec::new(&env);

        for mut milestone in schedule.milestones.iter() {
            if milestone.id == milestone_id {
                if milestone.completed {
                    panic!("Milestone already completed");
                }
                milestone.completed = true;
                milestone.completion_time = env.ledger().timestamp();
                milestone_found = true;
            }
            updated_milestones.push_back(milestone);
        }

        if !milestone_found {
            panic!("Milestone not found");
        }

        schedule.milestones = updated_milestones;
        env.storage()
            .persistent()
            .set(&DataKey::VestingSchedule(beneficiary), &schedule);

        env.events().publish(
            (String::from_str(&env, "milestone_completed"), schedule.schedule_id),
            VestingEvent::MilestoneCompleted(schedule.schedule_id, milestone_id),
        );
    }

    /// Revoke a vesting schedule (admin only, only if revocable)
    /// Returns unvested tokens to admin
    /// 
    /// # Arguments
    /// * `beneficiary` - Address whose schedule to revoke
    /// 
    /// # Returns
    /// Amount of unvested tokens returned to admin
    pub fn revoke_schedule(env: Env, beneficiary: Address) -> i128 {
        let admin: Address = env.storage().instance().get(&DataKey::Admin).unwrap();
        admin.require_auth();

        let mut schedule: VestingSchedule = env
            .storage()
            .persistent()
            .get(&DataKey::VestingSchedule(beneficiary.clone()))
            .expect("No vesting schedule found");

        if !schedule.revocable {
            panic!("Schedule is not revocable");
        }

        if schedule.revoked {
            panic!("Schedule already revoked");
        }

        let vested_amount = Self::calculate_vested_amount(&env, &schedule);
        let unvested_amount = schedule.total_amount - vested_amount;

        schedule.revoked = true;
        env.storage()
            .persistent()
            .set(&DataKey::VestingSchedule(beneficiary.clone()), &schedule);

        // Return unvested tokens to admin
        if unvested_amount > 0 {
            let token_address: Address = env.storage().instance().get(&DataKey::Token).unwrap();
            let token_client = token::Client::new(&env, &token_address);
            token_client.transfer(&env.current_contract_address(), &admin, &unvested_amount);
        }

        env.events().publish(
            (String::from_str(&env, "schedule_revoked"), schedule.schedule_id),
            VestingEvent::ScheduleRevoked(schedule.schedule_id, unvested_amount),
        );

        unvested_amount
    }

    /// Modify vesting schedule parameters (admin only)
    /// Cannot reduce already vested amounts
    /// 
    /// # Arguments
    /// * `beneficiary` - Address whose schedule to modify
    /// * `new_vesting_duration` - New vesting duration (0 to keep current)
    /// * `new_milestones` - New milestones (empty to keep current)
    pub fn modify_schedule(
        env: Env,
        beneficiary: Address,
        new_vesting_duration: u64,
        new_milestones: Vec<Milestone>,
    ) {
        let admin: Address = env.storage().instance().get(&DataKey::Admin).unwrap();
        admin.require_auth();
        Self::require_not_paused(&env);

        let mut schedule: VestingSchedule = env
            .storage()
            .persistent()
            .get(&DataKey::VestingSchedule(beneficiary.clone()))
            .expect("No vesting schedule found");

        if schedule.revoked {
            panic!("Cannot modify revoked schedule");
        }

        // Only allow modifications that don't reduce already vested amounts
        let current_vested = Self::calculate_vested_amount(&env, &schedule);
        
        if new_vesting_duration > 0 {
            schedule.vesting_duration = new_vesting_duration;
        }

        if !new_milestones.is_empty() {
            // Validate new milestones
            let mut total_percentage = 0u32;
            for milestone in new_milestones.iter() {
                total_percentage += milestone.percentage;
            }
            if total_percentage != 10000 {
                panic!("Milestone percentages must sum to 100%");
            }
            schedule.milestones = new_milestones;
        }

        // Ensure modification doesn't invalidate already vested tokens
        let new_vested = Self::calculate_vested_amount(&env, &schedule);
        if new_vested < current_vested {
            panic!("Modification cannot reduce vested amount");
        }

        env.storage()
            .persistent()
            .set(&DataKey::VestingSchedule(beneficiary), &schedule);

        env.events().publish(
            (String::from_str(&env, "schedule_modified"), schedule.schedule_id),
            VestingEvent::ScheduleModified(schedule.schedule_id),
        );
    }

    /// Pause all vesting operations (admin only)
    pub fn pause(env: Env) {
        let admin: Address = env.storage().instance().get(&DataKey::Admin).unwrap();
        admin.require_auth();
        
        let paused: bool = env.storage().instance().get(&DataKey::Paused).unwrap_or(false);
        if paused {
            panic!("Already paused");
        }
        
        env.storage().instance().set(&DataKey::Paused, &true);
        
        env.events().publish(
            (String::from_str(&env, "contract_paused"),),
            VestingEvent::ContractPaused,
        );
    }

    /// Unpause vesting operations (admin only)
    pub fn unpause(env: Env) {
        let admin: Address = env.storage().instance().get(&DataKey::Admin).unwrap();
        admin.require_auth();
        
        let paused: bool = env.storage().instance().get(&DataKey::Paused).unwrap_or(false);
        if !paused {
            panic!("Already unpaused");
        }
        
        env.storage().instance().set(&DataKey::Paused, &false);
        
        env.events().publish(
            (String::from_str(&env, "contract_unpaused"),),
            VestingEvent::ContractUnpaused,
        );
    }

    /// Check if contract is paused
    /// 
    /// # Returns
    /// true if paused, false otherwise
    pub fn is_paused(env: Env) -> bool {
        env.storage().instance().get(&DataKey::Paused).unwrap_or(false)
    }

    /// Get vesting schedule details
    /// 
    /// # Arguments
    /// * `beneficiary` - Address to get schedule for
    /// 
    /// # Returns
    /// VestingSchedule struct with all details
    pub fn get_schedule(env: Env, beneficiary: Address) -> VestingSchedule {
        env.storage()
            .persistent()
            .get(&DataKey::VestingSchedule(beneficiary))
            .expect("No vesting schedule found")
    }

    /// Get amount of tokens currently releasable
    /// 
    /// # Arguments
    /// * `beneficiary` - Address to check
    /// 
    /// # Returns
    /// Amount of tokens that can be released now
    pub fn get_releasable_amount(env: Env, beneficiary: Address) -> i128 {
        let schedule: VestingSchedule = env
            .storage()
            .persistent()
            .get(&DataKey::VestingSchedule(beneficiary))
            .expect("No vesting schedule found");

        if schedule.revoked {
            return 0;
        }

        let vested = Self::calculate_vested_amount(&env, &schedule);
        vested - schedule.released_amount
    }

    /// Get total vested amount (including already released)
    /// 
    /// # Arguments
    /// * `beneficiary` - Address to check
    /// 
    /// # Returns
    /// Total amount vested so far
    pub fn get_vested_amount(env: Env, beneficiary: Address) -> i128 {
        let schedule: VestingSchedule = env
            .storage()
            .persistent()
            .get(&DataKey::VestingSchedule(beneficiary))
            .expect("No vesting schedule found");

        if schedule.revoked {
            return schedule.released_amount;
        }

        Self::calculate_vested_amount(&env, &schedule)
    }

    /// Get admin address
    /// 
    /// # Returns
    /// Address of the contract admin
    pub fn get_admin(env: Env) -> Address {
        env.storage().instance().get(&DataKey::Admin).unwrap()
    }

    /// Get token address
    /// 
    /// # Returns
    /// Address of the token being vested
    pub fn get_token(env: Env) -> Address {
        env.storage().instance().get(&DataKey::Token).unwrap()
    }

    // Internal helper functions
    fn calculate_vested_amount(env: &Env, schedule: &VestingSchedule) -> i128 {
        let current_time = env.ledger().timestamp();

        // Check cliff period
        if current_time < schedule.start_time + schedule.cliff_duration {
            return 0;
        }

        match schedule.vesting_type {
            VestingType::TimeBased => Self::calculate_time_based_vesting(schedule, current_time),
            VestingType::MilestoneBased => Self::calculate_milestone_based_vesting(schedule),
            VestingType::Hybrid => {
                let time_vested = Self::calculate_time_based_vesting(schedule, current_time);
                let milestone_vested = Self::calculate_milestone_based_vesting(schedule);
                time_vested.min(milestone_vested)
            }
        }
    }

    fn calculate_time_based_vesting(schedule: &VestingSchedule, current_time: u64) -> i128 {
        if current_time >= schedule.start_time + schedule.vesting_duration {
            return schedule.total_amount;
        }

        let elapsed = current_time - schedule.start_time;
        (schedule.total_amount * elapsed as i128) / schedule.vesting_duration as i128
    }

    fn calculate_milestone_based_vesting(schedule: &VestingSchedule) -> i128 {
        let mut vested = 0i128;

        for milestone in schedule.milestones.iter() {
            if milestone.completed {
                vested += (schedule.total_amount * milestone.percentage as i128) / 10000;
            }
        }

        vested
    }


    fn require_not_paused(env: &Env) {
        let paused: bool = env.storage().instance().get(&DataKey::Paused).unwrap_or(false);
        if paused {
            panic!("Contract is paused");
        }
    }

    fn get_next_schedule_id(env: &Env) -> u64 {
        let id: u64 = env
            .storage()
            .instance()
            .get(&DataKey::NextScheduleId)
            .unwrap_or(0);
        env.storage().instance().set(&DataKey::NextScheduleId, &(id + 1));
        id
    }
}

#[cfg(test)]
mod test;