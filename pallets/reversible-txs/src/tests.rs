#![cfg(test)]

use super::*; // Import items from parent module (lib.rs)
use crate::mock::*; // Import mock runtime and types
use frame_support::traits::StorePreimage;
use frame_support::{assert_err, assert_ok};
use pallet_scheduler::Agenda;
use sp_core::H256;
use sp_runtime::traits::{BadOrigin, BlakeTwo256, Hash};

// Helper function to create a transfer call
fn transfer_call(dest: AccountId, amount: Balance) -> RuntimeCall {
    RuntimeCall::Balances(pallet_balances::Call::transfer_keep_alive {
        dest,
        value: amount,
    })
}

// Helper function to calculate TxId (matching the logic in schedule_dispatch)
fn calculate_tx_id(who: AccountId, call: &RuntimeCall) -> H256 {
    BlakeTwo256::hash_of(&(who, call).encode())
}

// Helper to run to the next block
fn run_to_block(n: u64) {
    while System::block_number() < n {
        // Finalize previous block
        Scheduler::on_finalize(System::block_number());
        System::finalize();
        // Set next block number
        System::set_block_number(System::block_number() + 1);
        // Initialize next block
        System::on_initialize(System::block_number());
        Scheduler::on_initialize(System::block_number());
    }
}

// Helper to create a bounded vec of task ids
fn bounded_vec(ids: Vec<H256>) -> BoundedVec<H256, MaxReversibleTxs> {
    ids.try_into()
        .unwrap_or_else(|_| panic!("Failed to convert to BoundedVec"))
}

#[test]
fn set_reversibility_works() {
    new_test_ext().execute_with(|| {
        System::set_block_number(1);
        let genesis_user = 1;

        // Check initial state
        assert_eq!(
            ReversibleTxs::is_reversible(&genesis_user),
            Some((10, DelayPolicy::Explicit))
        );

        // Set the delay
        let another_user = 3;
        let delay = 5u64;
        assert_ok!(ReversibleTxs::set_reversibility(
            RuntimeOrigin::signed(another_user),
            Some(delay),
            DelayPolicy::Intercept
        ));
        assert_eq!(
            ReversibleTxs::is_reversible(&another_user),
            Some((delay, DelayPolicy::Intercept))
        );
        System::assert_last_event(
            Event::ReversibilitySet {
                who: another_user,
                delay,
                policy: DelayPolicy::Intercept,
            }
            .into(),
        );

        // Use default delay
        assert_ok!(ReversibleTxs::set_reversibility(
            RuntimeOrigin::signed(another_user),
            None,
            DelayPolicy::Explicit
        ));
        assert_eq!(
            ReversibleTxs::is_reversible(&another_user),
            Some((DefaultDelay::get(), DelayPolicy::Explicit))
        );
        System::assert_last_event(
            Event::ReversibilitySet {
                who: another_user,
                delay: DefaultDelay::get(),
                policy: DelayPolicy::Explicit,
            }
            .into(),
        );

        // Too short delay
        let short_delay = MinDelayPeriod::get() - 1;
        assert_err!(
            ReversibleTxs::set_reversibility(
                RuntimeOrigin::signed(another_user),
                Some(short_delay),
                DelayPolicy::Explicit
            ),
            Error::<Test>::DelayTooShort
        );
        // stays unchanged
        assert_eq!(
            ReversibleTxs::is_reversible(&another_user),
            Some((DefaultDelay::get(), DelayPolicy::Explicit))
        );

        let new_user = 4;
        assert_err!(
            ReversibleTxs::set_reversibility(
                RuntimeOrigin::signed(new_user),
                Some(short_delay),
                DelayPolicy::Explicit
            ),
            Error::<Test>::DelayTooShort
        );
        assert_eq!(ReversibleTxs::is_reversible(&new_user), None);
    });
}

#[test]
fn set_reversibility_fails_delay_too_short() {
    new_test_ext().execute_with(|| {
        let user = 2; // User 2 is not reversible initially
        let short_delay = MinDelayPeriod::get() - 1;

        assert_err!(
            ReversibleTxs::set_reversibility(
                RuntimeOrigin::signed(user),
                Some(short_delay),
                DelayPolicy::Explicit
            ),
            Error::<Test>::DelayTooShort
        );
        assert_eq!(ReversibleTxs::is_reversible(&user), None);
    });
}

#[test]
fn schedule_dispatch_works() {
    new_test_ext().execute_with(|| {
        System::set_block_number(1);
        let user = 1; // Reversible from genesis
        let dest_user = 2;
        let amount = 100;
        let call = transfer_call(dest_user, amount);
        let tx_id = calculate_tx_id(user, &call);
        let (user_delay, _) = ReversibleTxs::is_reversible(&user).unwrap();
        let expected_block = System::block_number() + user_delay;
        let bounded = Preimage::bound(call.clone()).unwrap();

        assert!(Agenda::<Test>::get(expected_block).len() == 0);

        // Simulate call from SignedExtension
        assert_ok!(ReversibleTxs::schedule_dispatch(
            RuntimeOrigin::signed(user),
            Box::new(call.clone())
        ));

        // Check storage
        assert_eq!(PendingDispatches::<Test>::get(tx_id), Some((user, bounded)));
        assert_eq!(ReversibleTxs::account_pending_index(user), 1);

        // Check scheduler
        assert!(Agenda::<Test>::get(expected_block).len() > 0);
    });
}

#[test]
fn schedule_dispatch_fails_not_reversible() {
    new_test_ext().execute_with(|| {
        let user = 2; // Not reversible
        let call = transfer_call(3, 50);

        assert_err!(
            ReversibleTxs::schedule_dispatch(RuntimeOrigin::signed(user), Box::new(call)),
            Error::<Test>::AccountNotReversible
        );
    });
}

#[test]
fn schedule_already_dispatched_call_fails() {
    new_test_ext().execute_with(|| {
        let user = 1;
        let dest_user = 2;
        let amount = 100;
        let call = transfer_call(dest_user, amount);

        // Schedule first
        assert_ok!(ReversibleTxs::schedule_dispatch(
            RuntimeOrigin::signed(user),
            Box::new(call.clone())
        ));

        // Try to schedule the same call again
        assert_err!(
            ReversibleTxs::schedule_dispatch(RuntimeOrigin::signed(user), Box::new(call)),
            Error::<Test>::AlreadyScheduled
        );
    });
}

#[test]
fn schedule_dispatch_fails_too_many_pending() {
    new_test_ext().execute_with(|| {
        let user = 1;
        let max_pending = MaxReversibleTxs::get();

        // Fill up pending slots
        for i in 0..max_pending {
            let call = transfer_call(2, i as u128 + 1); // Unique call each time
            assert_ok!(ReversibleTxs::schedule_dispatch(
                RuntimeOrigin::signed(user),
                Box::new(call)
            ));
            // Max pending per block is 10, so we increment the block number
            // after every 10 calls
            if i % 10 == 9 {
                System::set_block_number(System::block_number() + 1);
            }
        }

        // Try to schedule one more
        let call = transfer_call(3, 100);
        assert_err!(
            ReversibleTxs::schedule_dispatch(RuntimeOrigin::signed(user), Box::new(call)),
            Error::<Test>::TooManyPendingTransactions
        );
    });
}

#[test]
fn cancel_dispatch_works() {
    new_test_ext().execute_with(|| {
        System::set_block_number(1);
        let user = 1;
        let call = transfer_call(2, 50);
        let tx_id = calculate_tx_id(user, &call);
        let (user_delay, _) = ReversibleTxs::is_reversible(&user).unwrap();
        let execute_block = System::block_number() + user_delay;

        assert_eq!(Agenda::<Test>::get(execute_block).len(), 0);

        // Schedule first
        assert_ok!(ReversibleTxs::schedule_dispatch(
            RuntimeOrigin::signed(user),
            Box::new(call.clone())
        ));
        assert!(ReversibleTxs::pending_dispatches(tx_id).is_some());
        assert!(!ReversibleTxs::account_pending_index(user).is_zero());

        // Check the expected block agendas count
        assert_eq!(Agenda::<Test>::get(execute_block).len(), 1);

        // Now cancel
        assert_ok!(ReversibleTxs::cancel(RuntimeOrigin::signed(user), tx_id));

        // Check state cleared
        assert!(ReversibleTxs::pending_dispatches(tx_id).is_none());
        assert!(ReversibleTxs::account_pending_index(user).is_zero());

        assert_eq!(Agenda::<Test>::get(execute_block).len(), 0);

        // Check event
        System::assert_last_event(Event::TransactionCancelled { who: user, tx_id }.into());
    });
}

#[test]
fn cancel_dispatch_fails_not_owner() {
    new_test_ext().execute_with(|| {
        let owner = 1;
        let attacker = 3;
        let call = transfer_call(2, 50);
        let tx_id = calculate_tx_id(owner, &call);

        // Schedule as owner
        assert_ok!(ReversibleTxs::schedule_dispatch(
            RuntimeOrigin::signed(owner),
            Box::new(call)
        ));

        // Attacker tries to cancel
        assert_err!(
            ReversibleTxs::cancel(RuntimeOrigin::signed(attacker), tx_id),
            Error::<Test>::NotOwner
        );

        // Check state not affected
        assert!(ReversibleTxs::pending_dispatches(tx_id).is_some());
    });
}

#[test]
fn cancel_dispatch_fails_not_found() {
    new_test_ext().execute_with(|| {
        let user = 1;
        let non_existent_tx_id = H256::random();

        assert_err!(
            ReversibleTxs::cancel(RuntimeOrigin::signed(user), non_existent_tx_id),
            Error::<Test>::PendingTxNotFound
        );
    });
}

#[test]
fn execute_dispatch_works() {
    new_test_ext().execute_with(|| {
        System::set_block_number(1);
        let user = 1; // Reversible, delay 10
        let dest = 2;
        let amount = 50;
        let call = transfer_call(dest, amount);
        let tx_id = calculate_tx_id(user, &call);
        let (delay, _) = ReversibleTxs::is_reversible(&user).unwrap();
        let execute_block = System::block_number() + delay;

        assert_ok!(ReversibleTxs::schedule_dispatch(
            RuntimeOrigin::signed(user),
            Box::new(call.clone())
        ));
        assert!(ReversibleTxs::pending_dispatches(tx_id).is_some());

        run_to_block(execute_block - 1);

        // Execute the dispatch as a normal user. This should fail
        // because the origin should be `Signed(PalletId::into_account())`
        assert_err!(
            ReversibleTxs::execute_dispatch(RuntimeOrigin::signed(user), tx_id),
            Error::<Test>::InvalidSchedulerOrigin,
        );

        // Check state cleared
        assert!(ReversibleTxs::pending_dispatches(tx_id).is_some());

        // Even root origin should fail
        assert_err!(
            ReversibleTxs::execute_dispatch(RuntimeOrigin::root(), tx_id),
            BadOrigin
        );
    });
}

#[test]
fn full_flow_execute_works() {
    new_test_ext().execute_with(|| {
        System::set_block_number(1);

        let user = 1; // Reversible, delay 10
        let dest = 2;
        let amount = 50;
        let initial_user_balance = Balances::free_balance(user);
        let initial_dest_balance = Balances::free_balance(dest);
        let call = transfer_call(dest, amount);
        let tx_id = calculate_tx_id(user, &call);
        let (delay, _) = ReversibleTxs::is_reversible(&user).unwrap();
        let start_block = System::block_number();
        let execute_block = start_block + delay;

        assert_ok!(ReversibleTxs::schedule_dispatch(
            RuntimeOrigin::signed(user),
            Box::new(call.clone())
        ));
        assert!(ReversibleTxs::pending_dispatches(tx_id).is_some());
        assert!(Agenda::<Test>::get(execute_block).len() > 0);
        assert_eq!(Balances::free_balance(user), initial_user_balance); // Not executed yet

        run_to_block(execute_block);

        // Event should be emitted by execute_dispatch called by scheduler
        let expected_event = Event::TransactionExecuted {
            tx_id,
            result: Ok(().into()).into(),
        };
        assert!(
            System::events()
                .iter()
                .any(|rec| rec.event == expected_event.clone().into()),
            "Execute event not found"
        );

        assert_eq!(Balances::free_balance(user), initial_user_balance - amount);
        assert_eq!(Balances::free_balance(dest), initial_dest_balance + amount);

        assert!(ReversibleTxs::pending_dispatches(tx_id).is_none());
        assert!(ReversibleTxs::account_pending_index(user).is_zero());
        assert_eq!(Agenda::<Test>::get(execute_block).len(), 0); // Task removed after execution
    });
}

#[test]
fn full_flow_cancel_prevents_execution() {
    new_test_ext().execute_with(|| {
        let user = 1;
        let dest = 2;
        let amount = 50;
        let initial_user_balance = Balances::free_balance(user);
        let initial_dest_balance = Balances::free_balance(dest);
        let call = transfer_call(dest, amount);
        let tx_id = calculate_tx_id(user, &call);
        let (delay, _) = ReversibleTxs::is_reversible(&user).unwrap();
        let start_block = System::block_number();
        let execute_block = start_block + delay;

        assert_ok!(ReversibleTxs::schedule_dispatch(
            RuntimeOrigin::signed(user),
            Box::new(call.clone())
        ));

        assert_ok!(ReversibleTxs::cancel(RuntimeOrigin::signed(user), tx_id));
        assert!(ReversibleTxs::pending_dispatches(tx_id).is_none());
        assert!(ReversibleTxs::account_pending_index(user).is_zero());

        // Run past the execution block
        run_to_block(execute_block + 1);

        // State is unchanged
        assert_eq!(Balances::free_balance(user), initial_user_balance);
        assert_eq!(Balances::free_balance(dest), initial_dest_balance);

        // No events were emitted
        let expected_event_pattern = |e: &RuntimeEvent| match e {
            RuntimeEvent::ReversibleTxs(Event::TransactionExecuted { tx_id: tid, .. })
                if *tid == tx_id =>
            {
                true
            }
            _ => false,
        };
        assert!(
            !System::events()
                .iter()
                .any(|rec| expected_event_pattern(&rec.event)),
            "TransactionExecuted event should not exist"
        );
    });
}
