#![cfg_attr(not(feature = "std"), no_std)]

pub use pallet::*;

#[cfg(test)]
mod mock;

#[cfg(test)]
mod tests;

#[cfg(feature = "runtime-benchmarks")]
mod benchmarking;
mod weights;
pub use weights::*;


#[frame_support::pallet]
pub mod pallet {
    use super::*;
    use frame_support::pallet_prelude::*;
    use frame_system::pallet_prelude::*;
    use codec::Decode;
    use sp_runtime::{
        traits::{AccountIdConversion, Saturating},
        ArithmeticError
    };
    use frame_support::traits::{
        Currency,
        ExistenceRequirement::{
            AllowDeath,
            KeepAlive
        },
        Get,
    };
    use frame_support::{
        parameter_types,
        BoundedVec,
        PalletId
    };
    use sp_std::convert::TryInto;

    #[derive(Encode, Decode, Clone, PartialEq, Eq, RuntimeDebug, TypeInfo, MaxEncodedLen)]
    pub struct VestingSchedule<AccountId, Balance, Moment> {
        pub id: u64,                   // Unique id
        pub creator: AccountId,        // Who created the scehdule
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

    #[pallet::storage]
    pub type ScheduleCounter<T: Config> = StorageValue<_, u64, ValueQuery>;

    #[pallet::config]
    pub trait Config: frame_system::Config + pallet_balances::Config + pallet_timestamp::Config {
        type RuntimeEvent: From<Event<Self>> + IsType<<Self as frame_system::Config>::RuntimeEvent>;
        type PalletId: Get<PalletId>;
        #[pallet::constant]
        type MaxSchedules: Get<u32>;
        type WeightInfo: WeightInfo;
    }

    #[pallet::event]
    #[pallet::generate_deposit(pub(super) fn deposit_event)]
    pub enum Event<T: Config> {
        VestingScheduleCreated(T::AccountId, T::Balance, T::Moment, T::Moment, u64),
        TokensClaimed(T::AccountId, T::Balance),
        VestingScheduleCancelled(T::AccountId, u64), // Creator, Schedule ID
    }

    #[pallet::error]
    pub enum Error<T> {
        NoVestingSchedule,      // No schedule exists for the caller
        InvalidSchedule,        // Start block >= end block
        TooManySchedules,       // Exceeded maximum number of schedules
        NotCreator,             // Caller isn’t the creator
        ScheduleNotFound,       // No schedule with that ID
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
            let who = ensure_signed(origin)?;

            ensure!(start < end, Error::<T>::InvalidSchedule);
            ensure!(amount > T::Balance::zero(), Error::<T>::InvalidSchedule);

            // Transfer tokens from caller to pallet and lock them
            pallet_balances::Pallet::<T>::transfer(&who, &Self::account_id(), amount, KeepAlive)?;

            // Generate unique ID
            let schedule_id = ScheduleCounter::<T>::get().wrapping_add(1);
            ScheduleCounter::<T>::put(schedule_id);

            // Add the schedule to storage
            let schedule = VestingSchedule {
                creator: who,
                beneficiary: beneficiary.clone(),
                amount,
                start,
                end,
                claimed: T::Balance::zero(),
                id: schedule_id,
            };
            VestingSchedules::<T>::try_mutate(&beneficiary, |schedules| {
                schedules.try_push(schedule).map_err(|_| Error::<T>::TooManySchedules)
            })?;

            Self::deposit_event(Event::VestingScheduleCreated(beneficiary, amount, start, end, schedule_id));
            Ok(())
        }

        // Claim vested tokens
        #[pallet::call_index(1)]
        #[pallet::weight(<T as Config>::WeightInfo::claim())]
        pub fn claim(
            _origin: OriginFor<T>,
            beneficiary: T::AccountId,
        ) -> DispatchResult {
            let schedules = VestingSchedules::<T>::get(&beneficiary);
            ensure!(!schedules.is_empty(), Error::<T>::NoVestingSchedule);

            // Collect vested amounts
            let mut vested_amounts: Vec<(usize, T::Balance)> = Vec::new();
            for (index, schedule) in schedules.iter().enumerate() {
                let vested = Self::vested_amount(&schedule)?;
                let claimable = vested.saturating_sub(schedule.claimed);
                if claimable > T::Balance::zero() {
                    vested_amounts.push((index, claimable));
                }
            }

            // Calculate total claimable
            let total_claimable = vested_amounts
                .iter()
                .fold(T::Balance::zero(), |acc, &(_, amount)| acc + amount);

            // Mutate schedules and transfer
            VestingSchedules::<T>::mutate(&beneficiary, |schedules| {
                for (index, claimable) in vested_amounts {
                    schedules[index].claimed += claimable;
                }
            });

            if total_claimable > T::Balance::zero() {
                // Transfer claimable tokens
                pallet_balances::Pallet::<T>::transfer(&Self::account_id(), &beneficiary, total_claimable, AllowDeath)?;

                Self::deposit_event(Event::TokensClaimed(beneficiary, total_claimable));
            }

            Ok(())
        }

        #[pallet::call_index(2)]
        #[pallet::weight(<T as Config>::WeightInfo::cancel_vesting_schedule())]
        pub fn cancel_vesting_schedule(
            origin: OriginFor<T>,
            beneficiary: T::AccountId,
            schedule_id: u64,
        ) -> DispatchResult {
            let who = ensure_signed(origin.clone())?;

            // Claim for beneficiary whatever they are currently owed
            Self::claim(origin, beneficiary.clone())?;

            // Get the beneficiary’s schedules
            let mut schedules = VestingSchedules::<T>::get(&beneficiary);
            ensure!(!schedules.is_empty(), Error::<T>::NoVestingSchedule);

            // Find the schedule by ID
            let index = schedules
                .iter()
                .position(|s| s.id == schedule_id)
                .ok_or(Error::<T>::ScheduleNotFound)?;

            // Ensure the caller is the creator
            let schedule = &schedules[index];
            ensure!(schedule.creator == who, Error::<T>::NotCreator);

            // Refund unclaimed amount to the creator
            let unclaimed = schedule.amount.saturating_sub(schedule.claimed);
            if unclaimed > T::Balance::zero() {
                pallet_balances::Pallet::<T>::transfer(
                    &Self::account_id(),
                    &who,
                    unclaimed,
                    AllowDeath,
                )?;
            }

            // Remove the schedule
            schedules.remove(index);
            if schedules.is_empty() {
                VestingSchedules::<T>::remove(&beneficiary);
            } else {
                VestingSchedules::<T>::insert(&beneficiary, schedules);
            }

            // Emit event
            Self::deposit_event(Event::VestingScheduleCancelled(who, schedule_id));
            Ok(())
        }
    }

    impl<T: Config> Pallet<T> {
        // Helper to calculate vested amount
        pub fn vested_amount(
            schedule: &VestingSchedule<T::AccountId, T::Balance, T::Moment>,
        ) -> Result<T::Balance, DispatchError> {
            let now = <pallet_timestamp::Pallet<T>>::get();
            // No need to convert now/start/end to u64 explicitly if T::Moment is u64-like
            if now < schedule.start {
                Ok(T::Balance::zero())
            } else if now >= schedule.end {
                Ok(schedule.amount)
            } else {
                let elapsed = now.saturating_sub(schedule.start);
                let duration = schedule.end.saturating_sub(schedule.start);

                // Convert amount to u64 for intermediate calculation
                let amount_u64: u64 = schedule.amount.try_into()
                    .map_err(|_| DispatchError::Other("Balance to u64 conversion failed"))?;

                // Perform calculation in u64 (T::Moment-like)
                let elapsed_u64: u64 = elapsed.try_into()
                    .map_err(|_| DispatchError::Other("Moment to u64 conversion failed"))?;
                let duration_u64: u64 = duration.try_into()
                    .map_err(|_| DispatchError::Other("Moment to u64 conversion failed"))?;
                let duration_safe: u64 = duration_u64.max(1);

                let vested_u64: u64 = amount_u64
                    .saturating_mul(elapsed_u64)
                    .checked_div(duration_safe)
                    .ok_or(DispatchError::Arithmetic(ArithmeticError::Underflow))?;

                // Convert back to T::Balance
                let vested = T::Balance::try_from(vested_u64)
                    .map_err(|_| DispatchError::Other("u64 to Balance conversion failed"))?;

                Ok(vested)
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
