# Yield Farming Contract - Quick Reference

## 🚀 Quick Start

```bash
# Build
make build

# Test
make test

# Deploy
make deploy
```

## 📊 Pool Configuration Examples

### Conservative Pool (Low Risk)
- APY: 5% (500 basis points)
- Lock: 7 days
- Penalty: 2% (200 basis points)
- Multiplier: 1x (10000 basis points)

### Standard Pool (Medium Risk)
- APY: 10% (1000 basis points)
- Lock: 30 days
- Penalty: 5% (500 basis points)
- Multiplier: 1x (10000 basis points)

### Premium Pool (High Risk, High Reward)
- APY: 20% (2000 basis points)
- Lock: 90 days
- Penalty: 10% (1000 basis points)
- Multiplier: 1.5x (15000 basis points)

### NFT Pool (Ultra Premium)
- APY: 30% (3000 basis points)
- Lock: 180 days
- Penalty: 15% (1500 basis points)
- Multiplier: 2x (20000 basis points)
- Auto-compound: Enabled

## 💰 Reward Calculation Examples

### Example 1: Basic Token Staking
- Stake: 10,000 tokens
- APY: 10% (1000 bp)
- Multiplier: 1x (10000 bp)
- Time: 1 year
- **Reward: ~1,000 tokens**

### Example 2: With Multiplier
- Stake: 10,000 tokens
- APY: 10% (1000 bp)
- Multiplier: 1.5x (15000 bp)
- Time: 1 year
- **Reward: ~1,500 tokens**

### Example 3: Short Duration
- Stake: 10,000 tokens
- APY: 10% (1000 bp)
- Multiplier: 1x (10000 bp)
- Time: 30 days
- **Reward: ~82 tokens**

### Example 4: Early Withdrawal
- Stake: 10,000 tokens
- Lock: 30 days
- Penalty: 5% (500 bp)
- Unstake: Day 15 (early)
- **Return: 9,500 tokens (500 penalty)**

## 🔢 Basis Points Reference

| Percentage | Basis Points |
|------------|--------------|
| 1%         | 100          |
| 2%         | 200          |
| 5%         | 500          |
| 10%        | 1000         |
| 15%        | 1500         |
| 20%        | 2000         |
| 50%        | 5000         |
| 100%       | 10000        |
| 150%       | 15000        |
| 200%       | 20000        |

## 🎯 Common Use Cases

### 1. Long-term HODLers
```
Pool: Conservative with auto-compound
APY: 5-10%
Lock: 90-180 days
Benefit: Steady, compounding growth
```

### 2. Active Traders
```
Pool: Flexible with short lock
APY: 5-8%
Lock: 7-14 days
Benefit: Quick access to funds
```

### 3. NFT Collectors
```
Pool: NFT-specific with high APY
APY: 20-30%
Lock: 60-180 days
Benefit: Earn while holding rare NFTs
```

### 4. Guild Treasuries
```
Pool: High-value with multipliers
APY: 15-20%
Lock: 90+ days
Benefit: Maximize guild rewards
```

## ⚠️ Important Notes

1. **Lock Periods**: Cannot unstake before unlock time without penalty
2. **Penalties**: Applied to principal, not rewards
3. **Auto-Compound**: Rewards added to principal automatically
4. **Multipliers**: Applied to base APY rewards
5. **Time Calculation**: Rewards accrue per second
6. **Gas Costs**: Consider transaction fees when claiming small amounts

## 🔗 Integration Checklist

- [ ] Deploy reward token contract
- [ ] Deploy yield farming contract
- [ ] Initialize with admin and reward token
- [ ] Fund contract with reward tokens
- [ ] Create initial pools
- [ ] Test stake/unstake flow
- [ ] Monitor pool statistics
- [ ] Set up frontend integration
- [ ] Configure auto-compound pools
- [ ] Enable NFT staking (if needed)

## 📞 Support

For issues or questions:
1. Check README.md for detailed documentation
2. Review IMPLEMENTATION_SUMMARY.md for technical details
3. Run tests: `make test`
4. Check contract logs on Stellar Explorer

## 🎉 Success Metrics

Track these metrics for pool health:
- Total Value Locked (TVL)
- Number of active stakers
- Average stake duration
- Reward distribution rate
- Early withdrawal rate
- Auto-compound adoption

---

**Contract Version**: 0.1.0  
**Soroban SDK**: 21.0.0  
**Status**: Production Ready ✅
