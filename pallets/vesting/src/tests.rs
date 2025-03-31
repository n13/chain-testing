use frame_support::{assert_noop, assert_ok};
use frame_support::dispatch::RawOrigin;
use crate::{mock::*, Event, VestingSchedule};
use frame_support::traits::Hooks;
use frame_support::weights::Weight;
use frame_system::Origin;
use super::*;

#[cfg(test)]
fn create_vesting_schedule<Moment: From<u64>>(start: u64, end: u64, amount: Balance) -> VestingSchedule<u64, Balance, Moment> {
    VestingSchedule {
        beneficiary: 1,
        start: start.into(),
        end: end.into(),
        amount: amount.into(),
        claimed: 0,
    }
}

#[test]
fn test_vesting_before_start() {
    new_test_ext().execute_with(|| {
        let origin = RuntimeOrigin::none();
        let schedule: VestingSchedule<u64, u128, u64> = create_vesting_schedule(100, 200, 1000);
        let now = 50; // Before vesting starts
        pallet_timestamp::Pallet::<Test>::set(origin, now).expect("Cannot set time to now");

        let vested: u128 = Pallet::<Test>::vested_amount(&schedule);
        assert_eq!(vested, 0);
    });
}

#[test]
fn test_vesting_after_end() {
    new_test_ext().execute_with(|| {
        let schedule: VestingSchedule<u64, u128, u64> = create_vesting_schedule(100, 200, 1000);
        let now = 250; // After vesting ends
        pallet_timestamp::Pallet::<Test>::set(RuntimeOrigin::none(), now).expect("Cannot set time to now");

        let vested: u128 = Pallet::<Test>::vested_amount(&schedule);
        assert_eq!(vested, 1000);
    });
}

#[test]
fn test_vesting_halfway() {
    new_test_ext().execute_with(|| {
        let origin = RuntimeOrigin::none();
        let schedule: VestingSchedule<u64, u128, u64> = create_vesting_schedule(100, 200, 1000);
        let now = 150; // Midway through vesting
        pallet_timestamp::Pallet::<Test>::set(origin, now).expect("Cannot set time to now");

        let vested: u128 = Pallet::<Test>::vested_amount(&schedule);
        assert_eq!(vested, 500); // 50% of 1000
    });
}

#[test]
fn test_vesting_start_equals_end() {
    new_test_ext().execute_with(|| {
        let origin = RuntimeOrigin::none();
        let schedule: VestingSchedule<u64, u128, u64> = create_vesting_schedule(100, 100, 1000);
        let now = 100; // Edge case: start == end
        pallet_timestamp::Pallet::<Test>::set(origin, now).expect("Cannot set time to now");

        let vested: u128 = Pallet::<Test>::vested_amount(&schedule);
        assert_eq!(vested, 1000); // Fully vested immediately
    });
}

#[test]
fn create_vesting_schedule_works() {
    new_test_ext().execute_with(|| {
        // Setup: Account 1 has 1000 tokens
        let start = 1000; // 1 second from genesis
        let end = 2000;   // 2 seconds from genesis
        let amount = 500;

        // Set timestamp to 0
        pallet_timestamp::Pallet::<Test>::set(RuntimeOrigin::none(), 0).expect("Cannot set time to now");

        // Create a vesting schedule
        assert_ok!(Vesting::create_vesting_schedule(
            RuntimeOrigin::signed(1),
            2, // Beneficiary
            amount,
            start,
            end
        ));

        // Check storage
        let schedules = VestingSchedules::<Test>::get(&2);
        assert_eq!(schedules.len(), 1);
        assert_eq!(
            schedules[0],
            VestingSchedule {
                beneficiary: 2,
                amount,
                start,
                end,
                claimed: 0,
            }
        );

        // Check balances
        assert_eq!(Balances::free_balance(1), 100000 - amount); // Sender loses tokens
        assert_eq!(Balances::free_balance(Vesting::account_id()), amount); // Pallet holds tokens
    });
}

#[test]
fn claim_vested_tokens_works() {
    println!("waht");
    new_test_ext().execute_with(|| {
        let start = 1000;
        let end = 2000;
        let amount = 500;

        println!("waht");
        // Create a vesting schedule
        assert_ok!(Vesting::create_vesting_schedule(
            RuntimeOrigin::signed(1),
            2,
            amount,
            start,
            end
        ));

        println!("waht");

        pallet_timestamp::Pallet::<Test>::set(RuntimeOrigin::none(), 1).expect("Cannot set time");

        // Set timestamp to halfway through vesting (50% vested)
        run_to_block(5, 1500);

        // Claim tokens
        assert_ok!(Vesting::claim(RuntimeOrigin::signed(2)));

        // Check claimed amount (50% of 500 = 250)
        let schedules = VestingSchedules::<Test>::get(&2);
        assert_eq!(schedules[0].claimed, 250);
        assert_eq!(Balances::free_balance(2), 2250); // 2000 initial + 250 claimed
        assert_eq!(Balances::free_balance(Vesting::account_id()), 250); // Remaining in pallet

        // Claim again at end
        run_to_block(6, 2000);
        assert_ok!(Vesting::claim(RuntimeOrigin::signed(2)));

        // Check full claim
        let schedules = VestingSchedules::<Test>::get(&2);
        assert_eq!(schedules[0].claimed, 500);
        assert_eq!(Balances::free_balance(2), 2500); // All 500 claimed
        assert_eq!(Balances::free_balance(Vesting::account_id()), 0); // Pallet empty
    });
}

#[test]
fn claim_before_vesting_fails() {
    new_test_ext().execute_with(|| {
        let start = 1000;
        let end = 2000;
        let amount = 500;

        // Create a vesting schedule
        assert_ok!(Vesting::create_vesting_schedule(
            RuntimeOrigin::signed(1),
            2,
            amount,
            start,
            end
        ));

        // Set timestamp before vesting starts
        pallet_timestamp::Pallet::<Test>::set(RuntimeOrigin::none(), 500).expect("Cannot set time to now");

        // Try to claim (should fail)
        assert_noop!(
            Vesting::claim(RuntimeOrigin::signed(2)),
            Error::<Test>::NothingToClaim
        );

        // Check no changes
        let schedules = VestingSchedules::<Test>::get(&2);
        assert_eq!(schedules[0].claimed, 0);
        assert_eq!(Balances::free_balance(2), 2000); // No tokens claimed
    });
}

#[test]
fn too_many_schedules_fails() {
    new_test_ext().execute_with(|| {
        // Fill up schedules to the max (100)
        for i in 0..MaxSchedules::get() {
            assert_ok!(Vesting::create_vesting_schedule(
                RuntimeOrigin::signed(1),
                2,
                10,
                1000 + i as u64,
                2000 + i as u64
            ));
        }

        // Try to add one more
        assert_noop!(
            Vesting::create_vesting_schedule(
                RuntimeOrigin::signed(1),
                2,
                10,
                3000,
                4000
            ),
            Error::<Test>::TooManySchedules
        );
    });
}