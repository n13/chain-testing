### Motivation

To have accounts for which all outgoing transactions are subject to a variable time during which they may be cancelled. The idea is this could be used to deter theft as well as correct mistakes.

## Design

Pallet uses `Scheduler` and `Preimages` pallets internally to handle scheduling and lookup calls, respectively. For every dispatchable call submitted by the reversible account, in order:
1. Preimage note is taken 
2. Stored in the pallet's `PendingDispatch` which maps the unique tx ID `((who, call).hash())` to `(origin, call)`. 
3. Schedules the `ReversibleTxs::execute_dispatch` with name `tx_id`, so that user is able to cancel the dispatch by the `tx_id`
4. At the execution block, scheduler calls the `execute_dispatch` which *takes* the call from `Preimage` and dispatches it, cleaning up the storage.

NOTE: failed dispatches are not retried, in this version

### Delay policy

Pallet currently offers two policies/ways for transaction delaying: `Explicit` and `Intercept`:

- `Explicit`: default behaviour, where reversible accounts need call delayed transactions through `pallet_reversible_txs::schedule_dispatch` extrinsic. Directly calling the transaction will be rejected by `ReversibleTransactionExtension` as invalid.
- `Intercept`: this is the superset of `Explicit`, and allows the `ReversibleTransactionExtension` to intercept delayed transactions in the validation phase and internally call `do_schedule_dispatch` function. The downside is, since we are delaying the call in validation level, we should reject the transaction as invalid, which is not really good for UX. In theory, it should be possible to introduce `Pending` state to `TransactionValidity` by forking crates, but that's not in the scope of this tx.

### Tracking

Pending/delayed transactions can be tracked at `PendingDispatches` storage and by subscribing to `ReversibleTxsEvent::TransactionScheduled{..}` event.

### Storages

- `ReversibleAccounts`: list of accounts that are `reversible` accounts. Accounts can call `ReversibleTxs::set_reversability` extrinsic to join this set.
- `PendingDispatches`: stores current pending dispatches for the user. Maps `tx_id` to `(caller, call)`. We store the caller so that we can validate the user who's canceling the dispatch.
- `AccountPendingIndex`: stores the current count of pending transactions for the user so that they don't exceed `MaxPendingPerAccount`

### Notes

- Transaction id is `((who, call).hash())` where `who` is the account that called the transaction and `call` is the call itself. This is used to identify the transaction in the scheduler and preimage. So, if user calls the same transaction twice, it will generate the same transaction ID and error with `AlreadyScheduled` error. To override this, someone can schedule `pallet_utility::batch` transaction with multiple identical calls instead.
