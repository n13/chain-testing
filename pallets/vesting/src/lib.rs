#![cfg_attr(not(feature = "std"), no_std)]

pub use pallet::*;

#[cfg(test)]
mod mock;

#[cfg(test)]
mod tests;

#[cfg(feature = "runtime-benchmarks")]
mod benchmarking;

#[frame_support::pallet]
pub mod pallet {
    use super::*;
    use frame_support::pallet_prelude::*;
    use frame_system::pallet_prelude::*;
    use codec::Decode;
    use sp_runtime::{
        traits::{Saturating, AccountIdConversion, CheckedDiv},
    };
    use frame_support::traits::{
        Currency,
        Get,
        ExistenceRequirement::{
            AllowDeath,
            KeepAlive
        },
    };
    use frame_support::{
        parameter_types,
        PalletId,
        BoundedVec
    };
    use sp_std::convert::TryInto;

    #[derive(Encode, Decode, Clone, PartialEq, Eq, RuntimeDebug, TypeInfo, MaxEncodedLen)]
    pub struct VestingSchedule<AccountId, Balance, Moment> {
        pub beneficiary: AccountId,    // Who gets the tokens
        pub amount: Balance,           // Total tokens to vest
        pub start: Moment,        // When vesting begins
        pub end: Moment,          // When vesting fully unlocks
        pub claimed: Balance,          // Tokens already claimed
    }
    
    #[pallet::storage]
    pub type VestingSchedules<T: Config> = StorageMap<
        _,
        Blake2_128Concat,
        T::AccountId,                  // Key: beneficiary address
        BoundedVec<VestingSchedule<T::AccountId, T::Balance, T::Moment>, T::MaxSchedules>,
        ValueQuery
    >;

    #[pallet::config]
    pub trait Config: frame_system::Config + pallet_balances::Config + pallet_timestamp::Config {
        type RuntimeEvent: From<Event<Self>> + IsType<<Self as frame_system::Config>::RuntimeEvent>;
        type PalletId: Get<PalletId>;
        #[pallet::constant]
        type MaxSchedules: Get<u32>;
        type WeightInfo: WeightInfo;
    }

    pub trait WeightInfo {
        fn create_vesting_schedule() -> Weight;
        fn claim() -> Weight;
    }

    pub struct DefaultWeightInfo;

    impl WeightInfo for DefaultWeightInfo {
        fn create_vesting_schedule() -> Weight {
            Weight::from_parts(10_000, 0)
        }
        fn claim() -> Weight {
            Weight::from_parts(10_000, 0)
        }
    }


    #[pallet::event]
    #[pallet::generate_deposit(pub(super) fn deposit_event)]
    pub enum Event<T: Config> {
        VestingScheduleCreated(T::AccountId, T::Balance, T::Moment, T::Moment),
        TokensClaimed(T::AccountId, T::Balance),
    }

    #[pallet::error]
    pub enum Error<T> {
        NoVestingSchedule,      // No schedule exists for the caller
        NothingToClaim,         // No tokens available to claim yet
        InvalidSchedule,        // Start block >= end block
        TooManySchedules,       // Exceeded maximum number of schedules
    }

    #[pallet::pallet]
    pub struct Pallet<T>(_);

    #[pallet::call]
    impl<T: Config> Pallet<T> {
        // Create a vesting schedule
        #[pallet::call_index(0)]
        #[pallet::weight(<T as Config>::WeightInfo::create_vesting_schedule())]
        pub fn create_vesting_schedule(
            origin: OriginFor<T>,
            beneficiary: T::AccountId,
            amount: T::Balance,
            start: T::Moment,
            end: T::Moment,
        ) -> DispatchResult {
            let who = ensure_signed(origin)?; // Caller (e.g., admin)

            ensure!(start < end, Error::<T>::InvalidSchedule);
            ensure!(amount > T::Balance::zero(), Error::<T>::InvalidSchedule);

            // Transfer tokens from caller to pallet and lock them
            pallet_balances::Pallet::<T>::transfer(&who, &Self::account_id(), amount, KeepAlive)?;

            // Add the schedule to storage
            let schedule = VestingSchedule {
                beneficiary: beneficiary.clone(),
                amount,
                start,
                end,
                claimed: T::Balance::zero(),
            };
            VestingSchedules::<T>::try_mutate(&beneficiary, |schedules| {
                schedules.try_push(schedule).map_err(|_| Error::<T>::TooManySchedules)
            })?;

            Self::deposit_event(Event::VestingScheduleCreated(beneficiary, amount, start, end));
            Ok(())
        }

        // Claim vested tokens
        #[pallet::call_index(1)]
        #[pallet::weight(<T as Config>::WeightInfo::claim())]
        pub fn claim(
            origin: OriginFor<T>,
        ) -> DispatchResult {
            let who = ensure_signed(origin)?;
            let schedules = VestingSchedules::<T>::get(&who);
            ensure!(!schedules.is_empty(), Error::<T>::NoVestingSchedule);

            let mut total_claimable = T::Balance::zero();

            // Calculate claimable amount and update claimed amounts in storage
            VestingSchedules::<T>::mutate(&who, |schedules| {
                for schedule in schedules.iter_mut() {
                    let vested = Self::vested_amount(&schedule);
                    let claimable = vested - schedule.claimed;
                    if claimable > T::Balance::zero() {
                        schedule.claimed += claimable.clone();
                        total_claimable += claimable;
                    }
                }
            });

            ensure!(total_claimable > T::Balance::zero(), Error::<T>::NothingToClaim);

            // Transfer claimable tokens
            pallet_balances::Pallet::<T>::transfer(&Self::account_id(), &who, total_claimable, AllowDeath)?;

            Self::deposit_event(Event::TokensClaimed(who, total_claimable));
            Ok(())
        }
    }

    impl<T: Config> Pallet<T> {
        // Helper to calculate vested amount
        pub fn vested_amount(
            schedule: &VestingSchedule<T::AccountId, T::Balance, T::Moment>,
        ) -> T::Balance {
            let now = <pallet_timestamp::Pallet<T>>::get();

            // Safely convert `T::Moment` to `u64`
            let now_u64: u64 = match now.try_into() {
                Ok(n) => n,
                Err(_) => return T::Balance::zero(),
            };
            let start_u64: u64 = match schedule.start.try_into() {
                Ok(s) => s,
                Err(_) => return T::Balance::zero(),
            };
            let end_u64: u64 = match schedule.end.try_into() {
                Ok(e) => e,
                Err(_) => return T::Balance::zero(),
            };

            if now_u64 < start_u64 {
                T::Balance::zero()
            } else if now_u64 >= end_u64 {
                schedule.amount
            } else {
                let elapsed = now_u64.saturating_sub(start_u64);
                let duration = end_u64.saturating_sub(start_u64).max(1);

                // Safely convert elapsed and duration to `T::Balance`
                let elapsed_balance: T::Balance = match elapsed.try_into() {
                    Ok(e) => e,
                    Err(_) => return T::Balance::zero(),
                };
                let duration_balance: T::Balance = match duration.try_into() {
                    Ok(d) => d,
                    Err(_) => return T::Balance::zero(),
                };

                // Linear vesting calculation with better precision
                schedule
                    .amount
                    .saturating_mul(elapsed_balance)
                    .checked_div(&duration_balance.max(T::Balance::one()))
                    .unwrap_or_else(T::Balance::zero)

            }
        }

        // Pallet account to "hold" tokens
        pub fn account_id() -> T::AccountId {
            T::PalletId::get().into_account_truncating()
        }
    }

    parameter_types! {
        pub const VestingPalletId: PalletId = PalletId(*b"vestingp");
    }
}
