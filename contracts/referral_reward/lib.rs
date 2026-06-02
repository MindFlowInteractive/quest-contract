use std::collections::HashMap;

pub struct ReferralContract {
    pub referrals: HashMap<String, ReferralRecord>, // key: referee
    pub stats: HashMap<String, (u64, u64)>, // referrer -> (count, total earned)
    pub reward_amount_referrer: u64,
    pub reward_amount_referee: u64,
    pub max_reward_per_user: Option<u64>,
}


impl ReferralContract {
    pub fn new(referrer_reward: u64, referee_reward: u64) -> Self {
        Self {
            referrals: HashMap::new(),
            stats: HashMap::new(),
            reward_amount_referrer: referrer_reward,
            reward_amount_referee: referee_reward,
            max_reward_per_user: None,
        }
    }
}

impl ReferralContract {
    pub fn register_referral(&mut self, referrer: String, referee: String) -> Result<(), String> {
        if self.referrals.contains_key(&referee) {
            return Err("Referral already registered".into());
        }

        let record = ReferralRecord {
            referrer: referrer.clone(),
            referee: referee.clone(),
            rewarded_at: None,
            reward_amount_referrer: self.reward_amount_referrer,
            reward_amount_referee: self.reward_amount_referee,
        };

        self.referrals.insert(referee, record);
        Ok(())
    }
}

impl ReferralContract {
    pub fn claim_referral_reward(&mut self, referee: String, now: u64) -> Result<(), String> {
        let record = self.referrals.get_mut(&referee).ok_or("Referral not found")?;

        if record.rewarded_at.is_some() {
            return Err("Reward already claimed".into());
        }

        let entry = self.stats.entry(record.referrer.clone()).or_insert((0, 0));
        let projected_total = entry.1 + record.reward_amount_referrer;

        if let Some(cap) = self.max_reward_per_user {
            if projected_total > cap {
                return Err("Reward cap exceeded".into());
            }
        }

        // Transfer tokens (mocked here)
        self.transfer(&record.referrer, record.reward_amount_referrer)?;
        self.transfer(&record.referee, record.reward_amount_referee)?;

        record.rewarded_at = Some(now);

        // Update stats (already fetched as entry, just need to update it directly)
        entry.0 += 1;
        entry.1 += record.reward_amount_referrer;

        // Emit event
        self.emit_referral_rewarded(&record.referrer, &record.referee, record.reward_amount_referrer + record.reward_amount_referee);

        Ok(())
    }

    fn transfer(&self, _to: &String, _amount: u64) -> Result<(), String> {
        // integrate with token runtime
        Ok(())
    }

    fn emit_referral_rewarded(&self, referrer: &String, referee: &String, amount: u64) {
        println!("ReferralRewarded: referrer={}, referee={}, amount={}", referrer, referee, amount);
    }
}

impl ReferralContract {
    pub fn update_reward_amounts(&mut self, referrer_amount: u64, referee_amount: u64) {
        self.reward_amount_referrer = referrer_amount;
        self.reward_amount_referee = referee_amount;
    }
}

impl ReferralContract {
    pub fn get_referral_stats(&self, referrer: String) -> (u64, u64) {
        self.stats.get(&referrer).cloned().unwrap_or((0, 0))
    }

    pub fn update_reward_cap(&mut self, cap: Option<u64>) {
        self.max_reward_per_user = cap;
        println!("RewardCapUpdated: new_cap={:?}", cap);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Mock struct since it wasn't in lib.rs directly, we provide it to make it compile
    pub struct ReferralRecord {
        pub referrer: String,
        pub referee: String,
        pub rewarded_at: Option<u64>,
        pub reward_amount_referrer: u64,
        pub reward_amount_referee: u64,
    }

    #[test]
    fn test_reward_cap_exceeded() {
        let mut contract = ReferralContract::new(50, 10);
        contract.update_reward_cap(Some(80));

        let ref1 = "user1".to_string();
        let ref2 = "user2".to_string();
        let referrer = "alice".to_string();

        // Register 2 referrals
        contract.referrals.insert(ref1.clone(), ReferralRecord {
            referrer: referrer.clone(), referee: ref1.clone(), rewarded_at: None,
            reward_amount_referrer: 50, reward_amount_referee: 10,
        });
        contract.referrals.insert(ref2.clone(), ReferralRecord {
            referrer: referrer.clone(), referee: ref2.clone(), rewarded_at: None,
            reward_amount_referrer: 50, reward_amount_referee: 10,
        });

        // First claim should succeed (50 <= 80)
        assert!(contract.claim_referral_reward(ref1, 100).is_ok());
        
        // Second claim should fail (50 + 50 = 100 > 80)
        let result = contract.claim_referral_reward(ref2, 101);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), "Reward cap exceeded");
    }

    #[test]
    fn test_cap_update_scenario() {
        let mut contract = ReferralContract::new(50, 10);
        let referrer = "alice".to_string();
        let ref1 = "user1".to_string();
        let ref2 = "user2".to_string();

        contract.referrals.insert(ref1.clone(), ReferralRecord {
            referrer: referrer.clone(), referee: ref1.clone(), rewarded_at: None,
            reward_amount_referrer: 50, reward_amount_referee: 10,
        });
        contract.referrals.insert(ref2.clone(), ReferralRecord {
            referrer: referrer.clone(), referee: ref2.clone(), rewarded_at: None,
            reward_amount_referrer: 50, reward_amount_referee: 10,
        });

        contract.update_reward_cap(Some(50));
        assert!(contract.claim_referral_reward(ref1, 100).is_ok());

        // Fails due to cap
        assert!(contract.claim_referral_reward(ref2.clone(), 101).is_err());

        // Increase cap
        contract.update_reward_cap(Some(100));
        assert!(contract.claim_referral_reward(ref2, 102).is_ok());
    }
}

