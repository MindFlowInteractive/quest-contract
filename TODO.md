# TODO - batch_processing contract

- [ ] Create new contract crate `contracts/batch_processing` (Cargo.toml + src/lib.rs)
- [ ] Implement batch data model (BatchStatus, Batch, BatchOperation, BatchOperationResult)
- [ ] Implement batch validation
- [ ] Implement batch execution with:
  - [ ] Atomic mode (rollback on failure)
  - [ ] Partial mode (continue after failures)
- [ ] Implement batch history + status tracking + query methods
- [ ] Implement queuing (queue_batch) + queue processing entrypoint
- [ ] Implement concurrency/execution limits (max batches per process call + execution guards)
- [ ] Add tests covering validation, order, failure handling, atomic vs partial, history, queue, limits
- [ ] Wire crate into workspace `Cargo.toml`
- [ ] Run `cargo test` for the new crate/workspace
