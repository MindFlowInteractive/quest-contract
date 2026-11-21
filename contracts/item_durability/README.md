# Item Durability Contract

A modular Soroban smart contract for the Quest economy that implements game item durability lifecycle management across Stages 1, 2, and 3:

1. **Durability Tracking & Usage (Stage 1 - #399):**
   - Configurable `max_durability` and `current_durability`.
   - `use_item` function: decrements durability by 1 per use.
   - Enforces fail-closed rejection when durability reaches zero (`ZeroDurability`).
   - Marks item as broken upon reaching zero durability and emits `(broken, item_id)` event.

2. **Repair with Token Payment Sink (Stage 2 - #400):**
   - `repair_item`: owner can pay tokens to restore missing durability.
   - Cost is strictly proportional to repaired durability: `cost = units * cost_per_unit`.
   - Repair is capped at `max_durability`.
   - Payment is routed directly to the designated token `sink_address`.
   - Broken items (durability == 0) are blocked from being repaired.

3. **Break & Salvage Payout (Stage 3 - #401):**
   - `salvage_item`: owner can salvage broken items for configured material payout tokens.
   - Item record is destroyed upon salvage, preventing double-salvage.
   - Non-broken items cannot be salvaged.

---

## Functions

### Administration
- `initialize(env, admin)`: Sets contract administrator.
- `set_repair_config(env, admin, token, cost_per_unit, sink_address)`: Configures repair token, unit cost, and sink recipient.
- `set_salvage_config(env, admin, material_token, material_payout)`: Configures material token and payout per broken item.
- `register_item(env, admin, item_id, owner, max_durability)`: Registers a new item.
- `transfer_admin(env, current_admin, new_admin)`: Secure admin transfer.

### Gameplay & Economics
- `use_item(env, user, item_id)`: Decrements durability, marks broken if reaches 0.
- `repair_item(env, user, item_id, amount_to_repair)`: Restores durability via token payment to sink.
- `salvage_item(env, user, item_id)`: Destroys broken item and awards material tokens.

### Views
- `get_item(env, item_id)`: Returns `Option<Item>`.
- `get_durability(env, item_id)`: Returns current durability or error if not found.
- `is_broken(env, item_id)`: Returns boolean indicating broken status.
- `calculate_repair_cost(env, item_id, amount_to_repair)`: Simulates repair cost.
