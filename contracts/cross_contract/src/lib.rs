#![no_std]

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, symbol_short, Address, Bytes, Env,
    IntoVal, Symbol, Vec,
};

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum CrossContractError {
    InvalidConfig = 1,
    AlreadyInitialized = 2,
    NotInitialized = 3,
    Unauthorized = 4,
    RouteNotFound = 5,
    RouteDisabled = 6,
    InvalidCallbackConfig = 7,
    RateLimited = 8,
    QueueEmpty = 9,
    MessageNotFound = 10,
    MessageNotQueued = 11,
    TargetInvocationFailed = 12,
    CallbackInvocationFailed = 13,
    UnexpectedQueueState = 14,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum MessageStatus {
    Queued = 0,
    Delivered = 1,
    Failed = 2,
    CallbackFailed = 3,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum AuditAction {
    Enqueued = 0,
    Routed = 1,
    CallbackSucceeded = 2,
    Delivered = 3,
    Failed = 4,
    CallbackFailed = 5,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RateLimitConfig {
    pub window_secs: u64,
    pub max_messages: u32,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SenderWindow {
    pub window: u64,
    pub count: u32,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RouteConfig {
    pub key: Symbol,
    pub target_contract: Address,
    pub target_method: Symbol,
    pub default_callback_contract: Option<Address>,
    pub default_callback_method: Option<Symbol>,
    pub enabled: bool,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Message {
    pub id: u64,
    pub route: Symbol,
    pub sender: Address,
    pub payload: Bytes,
    pub atomic: bool,
    pub target_contract: Address,
    pub target_method: Symbol,
    pub callback_contract: Option<Address>,
    pub callback_method: Option<Symbol>,
    pub status: MessageStatus,
    pub queued_at: u64,
    pub processed_at: Option<u64>,
    pub response: Option<Bytes>,
    pub last_error: Option<u32>,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AuditEntry {
    pub timestamp: u64,
    pub action: AuditAction,
    pub status: MessageStatus,
    pub error: Option<u32>,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProcessOutcome {
    pub message_id: u64,
    pub status: MessageStatus,
    pub response: Option<Bytes>,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
enum DataKey {
    Admin,
    RateLimit,
    QueueHead,
    QueueTail,
    NextMessageId,
    Route(Symbol),
    Message(u64),
    Audit(u64),
    QueueSlot(u64),
    SenderWindow(Address),
}

#[contract]
pub struct CrossContractCommunication;

#[contractimpl]
impl CrossContractCommunication {
    pub fn initialize(
        env: Env,
        admin: Address,
        window_secs: u64,
        max_messages: u32,
    ) -> Result<(), CrossContractError> {
        admin.require_auth();
        if env.storage().instance().has(&DataKey::Admin) {
            return Err(CrossContractError::AlreadyInitialized);
        }

        let config = validate_rate_limit(window_secs, max_messages)?;
        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage().instance().set(&DataKey::RateLimit, &config);
        env.storage().instance().set(&DataKey::QueueHead, &0u64);
        env.storage().instance().set(&DataKey::QueueTail, &0u64);
        env.storage().instance().set(&DataKey::NextMessageId, &1u64);
        Ok(())
    }

    pub fn set_rate_limit(
        env: Env,
        admin: Address,
        window_secs: u64,
        max_messages: u32,
    ) -> Result<(), CrossContractError> {
        require_admin(&env, &admin)?;
        let config = validate_rate_limit(window_secs, max_messages)?;
        env.storage().instance().set(&DataKey::RateLimit, &config);
        Ok(())
    }

    pub fn register_route(
        env: Env,
        admin: Address,
        key: Symbol,
        target_contract: Address,
        target_method: Symbol,
        callback_contract: Option<Address>,
        callback_method: Option<Symbol>,
    ) -> Result<(), CrossContractError> {
        require_admin(&env, &admin)?;
        validate_callback_pair(&callback_contract, &callback_method)?;

        let route = RouteConfig {
            key: key.clone(),
            target_contract,
            target_method,
            default_callback_contract: callback_contract,
            default_callback_method: callback_method,
            enabled: true,
        };
        env.storage()
            .persistent()
            .set(&DataKey::Route(key), &route);
        Ok(())
    }

    pub fn set_route_enabled(
        env: Env,
        admin: Address,
        key: Symbol,
        enabled: bool,
    ) -> Result<(), CrossContractError> {
        require_admin(&env, &admin)?;
        let mut route = get_route_or_err(&env, &key)?;
        route.enabled = enabled;
        env.storage()
            .persistent()
            .set(&DataKey::Route(key), &route);
        Ok(())
    }

    pub fn queue_message(
        env: Env,
        sender: Address,
        route: Symbol,
        payload: Bytes,
        atomic: bool,
        callback_contract: Option<Address>,
        callback_method: Option<Symbol>,
    ) -> Result<u64, CrossContractError> {
        ensure_initialized(&env)?;
        sender.require_auth();

        let route_cfg = get_route_or_err(&env, &route)?;
        if !route_cfg.enabled {
            return Err(CrossContractError::RouteDisabled);
        }

        let (resolved_callback_contract, resolved_callback_method) = resolve_callback(
            &route_cfg,
            callback_contract,
            callback_method,
        )?;

        enforce_rate_limit(&env, &sender)?;

        let message_id = next_message_id(&env);
        let message = Message {
            id: message_id,
            route: route.clone(),
            sender: sender.clone(),
            payload,
            atomic,
            target_contract: route_cfg.target_contract,
            target_method: route_cfg.target_method,
            callback_contract: resolved_callback_contract,
            callback_method: resolved_callback_method,
            status: MessageStatus::Queued,
            queued_at: env.ledger().timestamp(),
            processed_at: None,
            response: None,
            last_error: None,
        };

        env.storage()
            .persistent()
            .set(&DataKey::Message(message_id), &message);
        push_queue(&env, message_id);
        append_audit(
            &env,
            message_id,
            AuditAction::Enqueued,
            MessageStatus::Queued,
            None,
        );
        env.events()
            .publish((symbol_short!("queued"), route), (message_id, sender));

        Ok(message_id)
    }

    pub fn process_next(env: Env) -> Result<ProcessOutcome, CrossContractError> {
        ensure_initialized(&env)?;
        let message_id = peek_queue(&env)?;
        let message = get_message_or_err(&env, message_id)?;

        if message.status != MessageStatus::Queued {
            return Err(CrossContractError::MessageNotQueued);
        }

        if message.atomic {
            Self::process_atomic(env, message)
        } else {
            Self::process_non_atomic(env, message)
        }
    }

    pub fn get_message(env: Env, message_id: u64) -> Option<Message> {
        env.storage().persistent().get(&DataKey::Message(message_id))
    }

    pub fn get_route(env: Env, key: Symbol) -> Option<RouteConfig> {
        env.storage().persistent().get(&DataKey::Route(key))
    }

    pub fn get_audit_trail(env: Env, message_id: u64) -> Vec<AuditEntry> {
        env.storage()
            .persistent()
            .get(&DataKey::Audit(message_id))
            .unwrap_or(Vec::new(&env))
    }

    pub fn get_queue_size(env: Env) -> Result<u64, CrossContractError> {
        ensure_initialized(&env)?;
        Ok(queue_size(&env))
    }

    pub fn get_rate_limit(env: Env) -> Result<RateLimitConfig, CrossContractError> {
        ensure_initialized(&env)?;
        get_rate_limit_config(&env)
    }

    pub fn get_sender_window(env: Env, sender: Address) -> Option<SenderWindow> {
        env.storage()
            .persistent()
            .get(&DataKey::SenderWindow(sender))
    }

    fn process_atomic(env: Env, mut message: Message) -> Result<ProcessOutcome, CrossContractError> {
        let response: Bytes = match env.try_invoke_contract::<Bytes, soroban_sdk::Error>(
            &message.target_contract,
            &message.target_method,
            build_target_args(&env, &message),
        ) {
            Ok(Ok(response)) => response,
            _ => return Err(CrossContractError::TargetInvocationFailed),
        };

        if let (Some(callback_contract), Some(callback_method)) =
            (message.callback_contract.clone(), message.callback_method.clone())
        {
            let accepted = match env.try_invoke_contract::<bool, soroban_sdk::Error>(
                &callback_contract,
                &callback_method,
                build_callback_args(&env, &message, &response),
            ) {
                Ok(Ok(accepted)) => accepted,
                _ => return Err(CrossContractError::CallbackInvocationFailed),
            };

            if !accepted {
                return Err(CrossContractError::CallbackInvocationFailed);
            }

            append_audit(
                &env,
                message.id,
                AuditAction::Routed,
                MessageStatus::Queued,
                None,
            );
            append_audit(
                &env,
                message.id,
                AuditAction::CallbackSucceeded,
                MessageStatus::Queued,
                None,
            );
        } else {
            append_audit(
                &env,
                message.id,
                AuditAction::Routed,
                MessageStatus::Queued,
                None,
            );
        }

        message.status = MessageStatus::Delivered;
        message.processed_at = Some(env.ledger().timestamp());
        message.response = Some(response.clone());
        message.last_error = None;

        env.storage()
            .persistent()
            .set(&DataKey::Message(message.id), &message);
        pop_queue(&env, message.id)?;
        append_audit(
            &env,
            message.id,
            AuditAction::Delivered,
            MessageStatus::Delivered,
            None,
        );
        env.events()
            .publish((symbol_short!("done"), message.route), message.id);

        Ok(ProcessOutcome {
            message_id: message.id,
            status: MessageStatus::Delivered,
            response: Some(response),
        })
    }

    fn process_non_atomic(
        env: Env,
        mut message: Message,
    ) -> Result<ProcessOutcome, CrossContractError> {
        let response: Bytes = match env.try_invoke_contract::<Bytes, soroban_sdk::Error>(
            &message.target_contract,
            &message.target_method,
            build_target_args(&env, &message),
        ) {
            Ok(Ok(response)) => response,
            _ => {
                message.status = MessageStatus::Failed;
                message.processed_at = Some(env.ledger().timestamp());
                message.last_error = Some(CrossContractError::TargetInvocationFailed as u32);
                env.storage()
                    .persistent()
                    .set(&DataKey::Message(message.id), &message);
                pop_queue(&env, message.id)?;
                append_audit(
                    &env,
                    message.id,
                    AuditAction::Failed,
                    MessageStatus::Failed,
                    Some(CrossContractError::TargetInvocationFailed as u32),
                );
                env.events()
                    .publish((symbol_short!("failed"), message.route), message.id);
                return Err(CrossContractError::TargetInvocationFailed);
            }
        };

        append_audit(
            &env,
            message.id,
            AuditAction::Routed,
            MessageStatus::Queued,
            None,
        );

        if let (Some(callback_contract), Some(callback_method)) =
            (message.callback_contract.clone(), message.callback_method.clone())
        {
            match env.try_invoke_contract::<bool, soroban_sdk::Error>(
                &callback_contract,
                &callback_method,
                build_callback_args(&env, &message, &response),
            ) {
                Ok(Ok(true)) => append_audit(
                    &env,
                    message.id,
                    AuditAction::CallbackSucceeded,
                    MessageStatus::Queued,
                    None,
                ),
                _ => {
                    message.status = MessageStatus::CallbackFailed;
                    message.processed_at = Some(env.ledger().timestamp());
                    message.response = Some(response.clone());
                    message.last_error = Some(CrossContractError::CallbackInvocationFailed as u32);
                    env.storage()
                        .persistent()
                        .set(&DataKey::Message(message.id), &message);
                    pop_queue(&env, message.id)?;
                    append_audit(
                        &env,
                        message.id,
                        AuditAction::CallbackFailed,
                        MessageStatus::CallbackFailed,
                        Some(CrossContractError::CallbackInvocationFailed as u32),
                    );
                    env.events()
                        .publish((symbol_short!("cbfail"), message.route), message.id);
                    return Err(CrossContractError::CallbackInvocationFailed);
                }
            }
        }

        message.status = MessageStatus::Delivered;
        message.processed_at = Some(env.ledger().timestamp());
        message.response = Some(response.clone());
        message.last_error = None;

        env.storage()
            .persistent()
            .set(&DataKey::Message(message.id), &message);
        pop_queue(&env, message.id)?;
        append_audit(
            &env,
            message.id,
            AuditAction::Delivered,
            MessageStatus::Delivered,
            None,
        );
        env.events()
            .publish((symbol_short!("done"), message.route), message.id);

        Ok(ProcessOutcome {
            message_id: message.id,
            status: MessageStatus::Delivered,
            response: Some(response),
        })
    }
}

fn ensure_initialized(env: &Env) -> Result<(), CrossContractError> {
    if !env.storage().instance().has(&DataKey::Admin) {
        return Err(CrossContractError::NotInitialized);
    }
    Ok(())
}

fn require_admin(env: &Env, admin: &Address) -> Result<(), CrossContractError> {
    ensure_initialized(env)?;
    admin.require_auth();
    let stored: Address = env
        .storage()
        .instance()
        .get(&DataKey::Admin)
        .ok_or(CrossContractError::NotInitialized)?;

    if stored != *admin {
        return Err(CrossContractError::Unauthorized);
    }
    Ok(())
}

fn validate_rate_limit(
    window_secs: u64,
    max_messages: u32,
) -> Result<RateLimitConfig, CrossContractError> {
    if window_secs == 0 || max_messages == 0 {
        return Err(CrossContractError::InvalidConfig);
    }

    Ok(RateLimitConfig {
        window_secs,
        max_messages,
    })
}

fn get_rate_limit_config(env: &Env) -> Result<RateLimitConfig, CrossContractError> {
    env.storage()
        .instance()
        .get(&DataKey::RateLimit)
        .ok_or(CrossContractError::NotInitialized)
}

fn get_route_or_err(env: &Env, key: &Symbol) -> Result<RouteConfig, CrossContractError> {
    env.storage()
        .persistent()
        .get(&DataKey::Route(key.clone()))
        .ok_or(CrossContractError::RouteNotFound)
}

fn get_message_or_err(env: &Env, message_id: u64) -> Result<Message, CrossContractError> {
    env.storage()
        .persistent()
        .get(&DataKey::Message(message_id))
        .ok_or(CrossContractError::MessageNotFound)
}

fn validate_callback_pair(
    callback_contract: &Option<Address>,
    callback_method: &Option<Symbol>,
) -> Result<(), CrossContractError> {
    match (callback_contract, callback_method) {
        (None, None) | (Some(_), Some(_)) => Ok(()),
        _ => Err(CrossContractError::InvalidCallbackConfig),
    }
}

fn resolve_callback(
    route: &RouteConfig,
    callback_contract: Option<Address>,
    callback_method: Option<Symbol>,
) -> Result<(Option<Address>, Option<Symbol>), CrossContractError> {
    validate_callback_pair(&callback_contract, &callback_method)?;
    validate_callback_pair(
        &route.default_callback_contract,
        &route.default_callback_method,
    )?;

    if callback_contract.is_some() {
        Ok((callback_contract, callback_method))
    } else {
        Ok((
            route.default_callback_contract.clone(),
            route.default_callback_method.clone(),
        ))
    }
}

fn next_message_id(env: &Env) -> u64 {
    let next: u64 = env.storage().instance().get(&DataKey::NextMessageId).unwrap_or(1);
    env.storage()
        .instance()
        .set(&DataKey::NextMessageId, &(next + 1));
    next
}

fn push_queue(env: &Env, message_id: u64) {
    let tail: u64 = env.storage().instance().get(&DataKey::QueueTail).unwrap_or(0);
    env.storage()
        .persistent()
        .set(&DataKey::QueueSlot(tail), &message_id);
    env.storage().instance().set(&DataKey::QueueTail, &(tail + 1));
}

fn peek_queue(env: &Env) -> Result<u64, CrossContractError> {
    let head: u64 = env.storage().instance().get(&DataKey::QueueHead).unwrap_or(0);
    let tail: u64 = env.storage().instance().get(&DataKey::QueueTail).unwrap_or(0);
    if head >= tail {
        return Err(CrossContractError::QueueEmpty);
    }

    env.storage()
        .persistent()
        .get(&DataKey::QueueSlot(head))
        .ok_or(CrossContractError::UnexpectedQueueState)
}

fn pop_queue(env: &Env, expected_message_id: u64) -> Result<(), CrossContractError> {
    let head: u64 = env.storage().instance().get(&DataKey::QueueHead).unwrap_or(0);
    let actual: u64 = env
        .storage()
        .persistent()
        .get(&DataKey::QueueSlot(head))
        .ok_or(CrossContractError::UnexpectedQueueState)?;

    if actual != expected_message_id {
        return Err(CrossContractError::UnexpectedQueueState);
    }

    env.storage().persistent().remove(&DataKey::QueueSlot(head));
    env.storage().instance().set(&DataKey::QueueHead, &(head + 1));
    Ok(())
}

fn queue_size(env: &Env) -> u64 {
    let head: u64 = env.storage().instance().get(&DataKey::QueueHead).unwrap_or(0);
    let tail: u64 = env.storage().instance().get(&DataKey::QueueTail).unwrap_or(0);
    tail.saturating_sub(head)
}

fn enforce_rate_limit(env: &Env, sender: &Address) -> Result<(), CrossContractError> {
    let config = get_rate_limit_config(env)?;
    let current_window = env.ledger().timestamp() / config.window_secs;
    let key = DataKey::SenderWindow(sender.clone());
    let usage = env
        .storage()
        .persistent()
        .get::<_, SenderWindow>(&key)
        .unwrap_or(SenderWindow {
            window: current_window,
            count: 0,
        });

    let next = if usage.window == current_window {
        if usage.count >= config.max_messages {
            return Err(CrossContractError::RateLimited);
        }
        SenderWindow {
            window: current_window,
            count: usage.count + 1,
        }
    } else {
        SenderWindow {
            window: current_window,
            count: 1,
        }
    };

    env.storage().persistent().set(&key, &next);
    Ok(())
}

fn build_target_args(env: &Env, message: &Message) -> Vec<soroban_sdk::Val> {
    soroban_sdk::vec![
        env,
        message.id.into_val(env),
        message.sender.clone().into_val(env),
        message.route.clone().into_val(env),
        message.payload.clone().into_val(env)
    ]
}

fn build_callback_args(env: &Env, message: &Message, response: &Bytes) -> Vec<soroban_sdk::Val> {
    soroban_sdk::vec![
        env,
        message.id.into_val(env),
        message.route.clone().into_val(env),
        response.clone().into_val(env),
        message.sender.clone().into_val(env)
    ]
}

fn append_audit(
    env: &Env,
    message_id: u64,
    action: AuditAction,
    status: MessageStatus,
    error: Option<u32>,
) {
    let key = DataKey::Audit(message_id);
    let mut trail: Vec<AuditEntry> = env
        .storage()
        .persistent()
        .get(&key)
        .unwrap_or(Vec::new(env));

    trail.push_back(AuditEntry {
        timestamp: env.ledger().timestamp(),
        action,
        status,
        error,
    });

    env.storage().persistent().set(&key, &trail);
}

fn panic_with_error(env: &Env, err: CrossContractError) -> ! {
    env.events().publish((symbol_short!("xerr"),), err as u32);
    panic!("cross contract error");
}

#[cfg(test)]
mod test;
