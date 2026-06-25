#![no_std]
use soroban_sdk::{
    contract, contractimpl, contracttype, token, Address, Env, String, Vec, Map,
};

#[derive(Clone, Debug, Eq, PartialEq)]
#[contracttype]
pub enum DataKey {
    Admin,
    Token,
    Paused,
    Schedule(u64),
    NextScheduleId,
    BeneficiarySchedules(Address),
    VestingHistory(u64),
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
    pub created_at: u64,
    pub modified_at: Option<u64>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
#[contracttype]
pub struct VestingEvent {
    pub event_type: EventType,
    pub timestamp: u64,
    pub amount: i128,
    pub description: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
#[contracttype]
pub enum EventType {
    ScheduleCreated,
    TokensReleased,
    ScheduleRevoked,
    ScheduleModified,
}

#[derive(Clone, Debug, Eq, PartialEq)]
#[contracttype]
pub struct VestingStatus {
    pub schedule_id: u64,
    pub total_amount: i128,
    pub vested_amount: i128,
    pub released_amount: i128,
    pub releasable_amount: i128,
    pub cliff_end_time: u64,
    pub vesting_end_time: u64,
    pub is_revoked: bool,
    pub is_fully_vested: bool,
}

#[contract]
pub struct VestingScheduleContract;

#[contractimpl]
impl VestingScheduleContract {
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

        if cliff_duration >= vesting_duration {
            panic!("Cliff duration must be less than vesting duration");
        }

        let schedule_id = Self::get_next_schedule_id(&env);
        let current_time = env.ledger().timestamp();

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
            created_at: current_time,
            modified_at: None,
        };

        env.storage()
            .persistent()
            .set(&DataKey::Schedule(schedule_id), &schedule);

        // Add schedule to beneficiary's list
        let mut schedules: Vec<u64> = env
            .storage()
            .persistent()
            .get(&DataKey::BeneficiarySchedules(beneficiary.clone()))
            .unwrap_or(Vec::new(&env));
        schedules.push_back(schedule_id);
        env.storage()
            .persistent()
            .set(&DataKey::BeneficiarySchedules(beneficiary), &schedules);

        // Record history
        Self::record_history(
            &env,
            schedule_id,
            EventType::ScheduleCreated,
            total_amount,
            String::from_str(&env, "Schedule created"),
        );

        // Transfer tokens to contract
        let token_address: Address = env.storage().instance().get(&DataKey::Token).unwrap();
        let token_client = token::Client::new(&env, &token_address);
        token_client.transfer(&admin, &env.current_contract_address(), &total_amount);

        schedule_id
    }

    /// Release vested tokens to the beneficiary
    /// 
    /// # Arguments
    /// * `schedule_id` - ID of the vesting schedule
    /// 
    /// # Returns
    /// Amount of tokens released
    pub fn release(env: Env, schedule_id: u64) -> i128 {
        let mut schedule: VestingSchedule = env
            .storage()
            .persistent()
            .get(&DataKey::Schedule(schedule_id))
            .expect("No vesting schedule found");

        schedule.beneficiary.require_auth();
        Self::require_not_paused(&env);

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
            .set(&DataKey::Schedule(schedule_id), &schedule);

        // Transfer tokens to beneficiary
        let token_address: Address = env.storage().instance().get(&DataKey::Token).unwrap();
        let token_client = token::Client::new(&env, &token_address);
        token_client.transfer(&env.current_contract_address(), &schedule.beneficiary, &releasable);

        // Record history
        Self::record_history(
            &env,
            schedule_id,
            EventType::TokensReleased,
            releasable,
            String::from_str(&env, "Tokens released"),
        );

        releasable
    }

    /// Revoke a vesting schedule (admin only, only if revocable)
    /// Returns unvested tokens to admin
    /// 
    /// # Arguments
    /// * `schedule_id` - ID of the vesting schedule to revoke
    /// 
    /// # Returns
    /// Amount of unvested tokens returned to admin
    pub fn revoke_schedule(env: Env, schedule_id: u64) -> i128 {
        let admin: Address = env.storage().instance().get(&DataKey::Admin).unwrap();
        admin.require_auth();

        let mut schedule: VestingSchedule = env
            .storage()
            .persistent()
            .get(&DataKey::Schedule(schedule_id))
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
            .set(&DataKey::Schedule(schedule_id), &schedule);

        // Return unvested tokens to admin
        if unvested_amount > 0 {
            let token_address: Address = env.storage().instance().get(&DataKey::Token).unwrap();
            let token_client = token::Client::new(&env, &token_address);
            token_client.transfer(&env.current_contract_address(), &admin, &unvested_amount);
        }

        // Record history
        Self::record_history(
            &env,
            schedule_id,
            EventType::ScheduleRevoked,
            unvested_amount,
            String::from_str(&env, "Schedule revoked"),
        );

        unvested_amount
    }

    /// Modify vesting schedule parameters (admin only)
    /// Cannot reduce already vested amounts
    /// 
    /// # Arguments
    /// * `schedule_id` - ID of the vesting schedule to modify
    /// * `new_cliff_duration` - New cliff duration (0 to keep current)
    /// * `new_vesting_duration` - New vesting duration (0 to keep current)
    pub fn modify_schedule(
        env: Env,
        schedule_id: u64,
        new_cliff_duration: u64,
        new_vesting_duration: u64,
    ) {
        let admin: Address = env.storage().instance().get(&DataKey::Admin).unwrap();
        admin.require_auth();
        Self::require_not_paused(&env);

        let mut schedule: VestingSchedule = env
            .storage()
            .persistent()
            .get(&DataKey::Schedule(schedule_id))
            .expect("No vesting schedule found");

        if schedule.revoked {
            panic!("Cannot modify revoked schedule");
        }

        // Only allow modifications that don't reduce already vested amounts
        let current_vested = Self::calculate_vested_amount(&env, &schedule);
        
        if new_cliff_duration > 0 {
            if new_cliff_duration >= schedule.vesting_duration {
                panic!("Cliff duration must be less than vesting duration");
            }
            schedule.cliff_duration = new_cliff_duration;
        }

        if new_vesting_duration > 0 {
            if new_vesting_duration <= schedule.cliff_duration {
                panic!("Vesting duration must be greater than cliff duration");
            }
            schedule.vesting_duration = new_vesting_duration;
        }

        // Ensure modification doesn't invalidate already vested tokens
        let new_vested = Self::calculate_vested_amount(&env, &schedule);
        if new_vested < current_vested {
            panic!("Modification cannot reduce vested amount");
        }

        schedule.modified_at = Some(env.ledger().timestamp());
        env.storage()
            .persistent()
            .set(&DataKey::Schedule(schedule_id), &schedule);

        // Record history
        Self::record_history(
            &env,
            schedule_id,
            EventType::ScheduleModified,
            0,
            String::from_str(&env, "Schedule modified"),
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
    /// * `schedule_id` - ID of the vesting schedule
    /// 
    /// # Returns
    /// VestingSchedule struct with all details
    pub fn get_schedule(env: Env, schedule_id: u64) -> VestingSchedule {
        env.storage()
            .persistent()
            .get(&DataKey::Schedule(schedule_id))
            .expect("No vesting schedule found")
    }

    /// Get all schedule IDs for a beneficiary
    /// 
    /// # Arguments
    /// * `beneficiary` - Address to get schedules for
    /// 
    /// # Returns
    /// Vec of schedule IDs
    pub fn get_beneficiary_schedules(env: Env, beneficiary: Address) -> Vec<u64> {
        env.storage()
            .persistent()
            .get(&DataKey::BeneficiarySchedules(beneficiary))
            .unwrap_or(Vec::new(&env))
    }

    /// Get vesting status for a schedule
    /// 
    /// # Arguments
    /// * `schedule_id` - ID of the vesting schedule
    /// 
    /// # Returns
    /// VestingStatus with current vesting information
    pub fn get_vesting_status(env: Env, schedule_id: u64) -> VestingStatus {
        let schedule: VestingSchedule = env
            .storage()
            .persistent()
            .get(&DataKey::Schedule(schedule_id))
            .expect("No vesting schedule found");

        let vested_amount = if schedule.revoked {
            schedule.released_amount
        } else {
            Self::calculate_vested_amount(&env, &schedule)
        };

        let releasable_amount = vested_amount - schedule.released_amount;
        let cliff_end_time = schedule.start_time + schedule.cliff_duration;
        let vesting_end_time = schedule.start_time + schedule.vesting_duration;
        let is_fully_vested = vested_amount >= schedule.total_amount;

        VestingStatus {
            schedule_id,
            total_amount: schedule.total_amount,
            vested_amount,
            released_amount: schedule.released_amount,
            releasable_amount,
            cliff_end_time,
            vesting_end_time,
            is_revoked: schedule.revoked,
            is_fully_vested,
        }
    }

    /// Get amount of tokens currently releasable
    /// 
    /// # Arguments
    /// * `schedule_id` - ID of the vesting schedule
    /// 
    /// # Returns
    /// Amount of tokens that can be released now
    pub fn get_releasable_amount(env: Env, schedule_id: u64) -> i128 {
        let schedule: VestingSchedule = env
            .storage()
            .persistent()
            .get(&DataKey::Schedule(schedule_id))
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
    /// * `schedule_id` - ID of the vesting schedule
    /// 
    /// # Returns
    /// Total amount vested so far
    pub fn get_vested_amount(env: Env, schedule_id: u64) -> i128 {
        let schedule: VestingSchedule = env
            .storage()
            .persistent()
            .get(&DataKey::Schedule(schedule_id))
            .expect("No vesting schedule found");

        if schedule.revoked {
            return schedule.released_amount;
        }

        Self::calculate_vested_amount(&env, &schedule)
    }

    /// Get vesting history for a schedule
    /// 
    /// # Arguments
    /// * `schedule_id` - ID of the vesting schedule
    /// 
    /// # Returns
    /// Map of event index to VestingEvent
    pub fn get_vesting_history(env: Env, schedule_id: u64) -> Map<u32, VestingEvent> {
        let mut history = Map::<u32, VestingEvent>::new(&env);
        let mut index = 0u32;
        
        loop {
            let key = DataKey::VestingHistory(schedule_id * 1000 + index as u64);
            if let Some(event) = env.storage().persistent().get::<DataKey, VestingEvent>(&key) {
                history.set(index, event);
                index += 1;
            } else {
                break;
            }
        }
        
        history
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

        // Check cliff period - no vesting before cliff ends
        if current_time < schedule.start_time + schedule.cliff_duration {
            return 0;
        }

        // If vesting period is complete, all tokens are vested
        if current_time >= schedule.start_time + schedule.vesting_duration {
            return schedule.total_amount;
        }

        // Linear vesting calculation
        let elapsed_since_start = current_time - schedule.start_time;
        let elapsed_since_cliff = elapsed_since_start - schedule.cliff_duration;
        let vesting_period = schedule.vesting_duration - schedule.cliff_duration;
        
        // Calculate vested amount using linear vesting
        (schedule.total_amount * elapsed_since_cliff as i128) / vesting_period as i128
    }

    fn record_history(env: &Env, schedule_id: u64, event_type: EventType, amount: i128, description: String) {
        let mut index = 0u64;
        loop {
            let key = DataKey::VestingHistory(schedule_id * 1000 + index);
            if !env.storage().persistent().has(&key) {
                let event = VestingEvent {
                    event_type,
                    timestamp: env.ledger().timestamp(),
                    amount,
                    description,
                };
                env.storage().persistent().set(&key, &event);
                break;
            }
            index += 1;
        }
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
