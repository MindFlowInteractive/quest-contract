# Subscription Contract

A comprehensive subscription contract for premium memberships with recurring benefits and exclusive content access on Soroban.

## Features

### ✅ Implemented Features

1. **Subscription Tiers** - Three tier levels (Basic, Premium, Enterprise)
2. **Subscription Purchase** - Users can purchase subscriptions with token payments
3. **Time-Based Validity** - Automatic expiry tracking based on ledger timestamps
4. **Auto-Renewal** - Automatic subscription renewal with token payment
5. **Benefits Tracking** - Track usage of subscription benefits per tier
6. **Subscription Cancellation** - Cancel auto-renewal while maintaining access until expiry
7. **Grace Period** - 3-day grace period for expired subscriptions
8. **Group Subscriptions** - Family/group plans with multiple members (2-10 members)
9. **Subscription Gifting** - Gift subscriptions to other users
10. **Tier Upgrades** - Upgrade to higher tiers mid-subscription

## Architecture

### Data Structures

**SubscriptionTier**
- Basic: Entry-level subscription (10 benefits/month)
- Premium: Mid-tier subscription (50 benefits/month)
- Enterprise: Top-tier subscription (unlimited benefits)

**Subscription**
```rust
{
    tier: SubscriptionTier,
    start_time: u64,
    expiry_time: u64,
    auto_renew: bool,
    is_active: bool,
    total_renewals: u32,
    benefits_used: u32,
    is_gifted: bool,
    gifted_by: Option<Address>,
}
```

**GroupSubscription**
```rust
{
    owner: Address,
    tier: SubscriptionTier,
    start_time: u64,
    expiry_time: u64,
    auto_renew: bool,
    is_active: bool,
    max_members: u32,
    total_renewals: u32,
}
```

## Usage Examples

### Initialize Contract
```rust
subscription.initialize(
    &admin,
    &payment_token,
    &1_000_000,   // Basic: 1 token
    &5_000_000,   // Premium: 5 tokens
    &20_000_000,  // Enterprise: 20 tokens
);
```

### Purchase Subscription
```rust
subscription.purchase_subscription(
    &user,
    &SubscriptionTier::Premium,
    &true  // auto_renew enabled
);
```

### Create Group Subscription
```rust
let group_id = subscription.create_group_subscription(
    &owner,
    &SubscriptionTier::Premium,
    &5,    // max 5 members
    &true  // auto_renew enabled
);
```

### Add Member to Group
```rust
subscription.add_group_member(&owner, &group_id, &member);
```

### Gift Subscription
```rust
subscription.gift_subscription(
    &gifter,
    &recipient,
    &SubscriptionTier::Premium
);
```

### Process Renewal
```rust
subscription.process_renewal(&user);  // Requires user authorization
```

### Check Subscription Status
```rust
let has_active = subscription.has_active_subscription(&user);
let tier = subscription.get_user_tier(&user);
let time_left = subscription.get_time_until_expiry(&user);
let in_grace = subscription.is_in_grace_period(&user);
```

## Admin Functions

### Update Pricing
```rust
subscription.update_pricing(
    &admin,
    &2_000_000,   // New Basic price
    &10_000_000,  // New Premium price
    &40_000_000   // New Enterprise price
);
```

### Pause/Unpause Contract
```rust
subscription.set_paused(&admin, &true);
```

### Withdraw Payments
```rust
subscription.withdraw(&admin, &amount);
```

## Time Constants

- **Subscription Period**: 30 days (2,592,000 seconds)
- **Grace Period**: 3 days (259,200 seconds)

## Testing

All features are fully tested with 28 comprehensive test cases covering:
- Basic subscription operations
- Subscription validity and grace periods
- Auto-renewal functionality
- Subscription management (cancel, upgrade, toggle auto-renew)
- Benefits tracking
- Group subscriptions
- Gifting functionality
- Admin operations
- Edge cases and error conditions

Run tests:
```bash
cargo test --package subscription
```

## Build & Deploy

### Build
```bash
# Build the contract
cargo build --package subscription --target wasm32-unknown-unknown --release

# Or use the workspace build
cd /path/to/quest-contract
soroban contract build
```

### Optimize
```bash
soroban contract optimize --wasm target/wasm32-unknown-unknown/release/subscription.wasm
```

### Deploy to Testnet
```bash
soroban contract deploy \
    --wasm target/wasm32-unknown-unknown/release/subscription.wasm \
    --source deployer \
    --network testnet
```

After deployment, initialize the contract:
```bash
soroban contract invoke \
    --id <CONTRACT_ID> \
    --source admin \
    --network testnet \
    -- initialize \
    --admin <ADMIN_ADDRESS> \
    --payment_token <TOKEN_ADDRESS> \
    --basic_price 1000000 \
    --premium_price 5000000 \
    --enterprise_price 20000000
```

## Security Considerations

1. **Authorization**: All state-changing operations require proper authentication
2. **Payment Safety**: All token transfers are atomic and revert on failure
3. **Admin Controls**: Critical operations are restricted to admin only
4. **Pause Mechanism**: Contract can be paused in emergencies
5. **Grace Period**: Users get 3-day buffer before losing access
6. **Group Limits**: Group size capped at 10 members to prevent abuse

## Future Enhancements

Potential future additions:
- Subscription discounts/promo codes
- Tiered benefits customization
- Subscription transfer functionality
- Refund mechanism for early cancellation
- Multi-token payment support
- Subscription stacking/extensions

## License

See project root for license information.
