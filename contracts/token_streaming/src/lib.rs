#![no_std]

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, symbol_short, token, Address, Env, Symbol,
};

pub const BPS: i128 = 10_000;

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DataKey {
    NextStreamId,
    Stream(u64),
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Stream {
    pub id: u64,
    pub sender: Address,
    pub recipient: Address,
    pub token: Address,
    pub rate: i128,
    pub start: u64,
    pub end: u64,
    pub total: i128,
    pub withdrawn: i128,
    pub cancelled_at: Option<u64>,
}

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum StreamingError {
    StreamNotFound = 1,
    Unauthorized = 2,
    InvalidAmount = 3,
    InvalidSchedule = 4,
    NothingAccrued = 5,
    WithdrawalExceedsAccrued = 6,
    AlreadyCancelled = 7,
    InvalidSplit = 8,
}

const STREAM_CREATED: Symbol = symbol_short!("strm_new");
const WITHDRAWN: Symbol = symbol_short!("strm_wdr");
const RECIPIENT_CHANGED: Symbol = symbol_short!("strm_rcp");
const CANCELLED: Symbol = symbol_short!("strm_can");

#[contract]
pub struct TokenStreamingContract;

#[contractimpl]
impl TokenStreamingContract {
    /// Create a stream and fund it in full from the sender.
    pub fn create_stream(
        env: Env,
        sender: Address,
        recipient: Address,
        token: Address,
        rate: i128,
        start: u64,
        end: u64,
    ) -> Result<u64, StreamingError> {
        sender.require_auth();

        if rate <= 0 || end <= start {
            return Err(StreamingError::InvalidSchedule);
        }

        let total = rate
            .checked_mul((end - start) as i128)
            .ok_or(StreamingError::InvalidAmount)?;
        if total <= 0 {
            return Err(StreamingError::InvalidAmount);
        }

        let id: u64 = env
            .storage()
            .instance()
            .get(&DataKey::NextStreamId)
            .unwrap_or(1);

        let stream = Stream {
            id,
            sender: sender.clone(),
            recipient,
            token: token.clone(),
            rate,
            start,
            end,
            total,
            withdrawn: 0,
            cancelled_at: None,
        };

        token::Client::new(&env, &token).transfer(
            &sender,
            &env.current_contract_address(),
            &total,
        );

        env.storage()
            .persistent()
            .set(&DataKey::Stream(id), &stream);
        env.storage()
            .instance()
            .set(&DataKey::NextStreamId, &(id + 1));
        env.events().publish((STREAM_CREATED, id), sender);
        Ok(id)
    }

    /// Total amount streamed so far, capped at the funded total.
    pub fn streamed(env: Env, stream_id: u64) -> Result<i128, StreamingError> {
        let stream = Self::get_stream_or_err(&env, stream_id)?;
        Ok(Self::streamed_amount(&env, &stream))
    }

    /// Amount available for the recipient to withdraw right now.
    pub fn accrued(env: Env, stream_id: u64) -> Result<i128, StreamingError> {
        let stream = Self::get_stream_or_err(&env, stream_id)?;
        Ok(Self::accrued_amount(&env, &stream))
    }

    /// Withdraw part of the accrued balance. Never exceeds what has accrued.
    pub fn withdraw(
        env: Env,
        recipient: Address,
        stream_id: u64,
        amount: i128,
    ) -> Result<(), StreamingError> {
        recipient.require_auth();
        if amount <= 0 {
            return Err(StreamingError::InvalidAmount);
        }

        let key = DataKey::Stream(stream_id);
        let mut stream = Self::get_stream_or_err(&env, stream_id)?;
        if recipient != stream.recipient {
            return Err(StreamingError::Unauthorized);
        }

        let accrued = Self::accrued_amount(&env, &stream);
        if accrued <= 0 {
            return Err(StreamingError::NothingAccrued);
        }
        if amount > accrued {
            return Err(StreamingError::WithdrawalExceedsAccrued);
        }

        stream.withdrawn += amount;
        env.storage().persistent().set(&key, &stream);
        token::Client::new(&env, &stream.token).transfer(
            &env.current_contract_address(),
            &recipient,
            &amount,
        );
        env.events().publish((WITHDRAWN, stream_id), amount);
        Ok(())
    }

    /// Withdraw the entire accrued balance.
    pub fn withdraw_all(
        env: Env,
        recipient: Address,
        stream_id: u64,
    ) -> Result<i128, StreamingError> {
        let accrued = Self::accrued(env.clone(), stream_id)?;
        if accrued <= 0 {
            return Err(StreamingError::NothingAccrued);
        }
        Self::withdraw(env, recipient, stream_id, accrued)?;
        Ok(accrued)
    }

    /// Cancel a stream. The unstreamed remainder is split between the sender
    /// and recipient according to `recipient_bps` (basis points, 0..=10000).
    /// Already accrued funds remain withdrawable by the recipient.
    pub fn cancel(
        env: Env,
        sender: Address,
        stream_id: u64,
        recipient_bps: u32,
    ) -> Result<(i128, i128), StreamingError> {
        sender.require_auth();
        if recipient_bps > 10_000 {
            return Err(StreamingError::InvalidSplit);
        }

        let key = DataKey::Stream(stream_id);
        let mut stream = Self::get_stream_or_err(&env, stream_id)?;
        if sender != stream.sender {
            return Err(StreamingError::Unauthorized);
        }
        if stream.cancelled_at.is_some() {
            return Err(StreamingError::AlreadyCancelled);
        }

        let now = env.ledger().timestamp();
        let streamed = Self::streamed_amount(&env, &stream);
        let remainder = stream.total - streamed;

        let recipient_share = remainder * recipient_bps as i128 / BPS;
        let sender_share = remainder - recipient_share;

        stream.cancelled_at = Some(now);
        env.storage().persistent().set(&key, &stream);

        let token_client = token::Client::new(&env, &stream.token);
        if recipient_share > 0 {
            token_client.transfer(
                &env.current_contract_address(),
                &stream.recipient,
                &recipient_share,
            );
        }
        if sender_share > 0 {
            token_client.transfer(
                &env.current_contract_address(),
                &stream.sender,
                &sender_share,
            );
        }

        env.events()
            .publish((CANCELLED, stream_id), (sender_share, recipient_share));
        Ok((sender_share, recipient_share))
    }

    /// Change the recipient. Only the current recipient may hand off a stream.
    pub fn change_recipient(
        env: Env,
        recipient: Address,
        stream_id: u64,
        new_recipient: Address,
    ) -> Result<(), StreamingError> {
        recipient.require_auth();

        let key = DataKey::Stream(stream_id);
        let mut stream = Self::get_stream_or_err(&env, stream_id)?;
        if recipient != stream.recipient {
            return Err(StreamingError::Unauthorized);
        }

        stream.recipient = new_recipient.clone();
        env.storage().persistent().set(&key, &stream);
        env.events()
            .publish((RECIPIENT_CHANGED, stream_id), new_recipient);
        Ok(())
    }

    pub fn get_stream(env: Env, stream_id: u64) -> Option<Stream> {
        env.storage().persistent().get(&DataKey::Stream(stream_id))
    }

    fn get_stream_or_err(env: &Env, stream_id: u64) -> Result<Stream, StreamingError> {
        env.storage()
            .persistent()
            .get(&DataKey::Stream(stream_id))
            .ok_or(StreamingError::StreamNotFound)
    }

    fn streamed_amount(env: &Env, stream: &Stream) -> i128 {
        let mut cutoff = stream.end;
        if let Some(cancelled_at) = stream.cancelled_at {
            if cancelled_at < cutoff {
                cutoff = cancelled_at;
            }
        }

        let now = env.ledger().timestamp();
        if now < cutoff {
            cutoff = now;
        }
        if cutoff <= stream.start {
            return 0;
        }

        let amount = stream.rate * (cutoff - stream.start) as i128;
        if amount > stream.total {
            stream.total
        } else {
            amount
        }
    }

    fn accrued_amount(env: &Env, stream: &Stream) -> i128 {
        Self::streamed_amount(env, stream) - stream.withdrawn
    }
}

#[cfg(test)]
mod test;
