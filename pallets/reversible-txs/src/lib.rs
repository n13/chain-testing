//! # Reversibility Core Pallet
//!
//! Provides the core logic for scheduling and cancelling reversible transactions.
//! It manages the state of accounts opting into reversibility and the pending
//! transactions associated with them. Transaction interception is handled
//! separately via a `SignedExtension`.

#![cfg_attr(not(feature = "std"), no_std)]

extern crate alloc;
pub use pallet::*;

#[cfg(test)]
mod mock;

#[cfg(test)]
mod tests;

// #[cfg(feature = "runtime-benchmarks")]
// mod benchmarking;
// pub mod weights; // Assume weights.rs exists or will be created
// pub use weights::WeightInfo;

use alloc::boxed::Box;
use alloc::vec::Vec;
use frame_support::{
    dispatch::{GetDispatchInfo, PostDispatchInfo},
    pallet_prelude::*,
    traits::schedule::DispatchTime,
};
use frame_system::pallet_prelude::*;

/// How to delay transactions
/// - `Explicit`: Only delay transactions explicitly using `schedule_dispatch`.
/// - `Intercept`: Intercept and delay transactions at the `TransactionExtension` level.
///
/// For example, for a reversible account with `DelayPolicy::Intercept`, the transaction
/// will be delayed even if the user doesn't explicitly call `schedule_dispatch`. And for `DelayPolicy::Explicit`,
/// the transaction will be delayed only if the user explicitly calls this pallet's `schedule_dispatch` extrinsic.
#[derive(Encode, Decode, MaxEncodedLen, Clone, Default, TypeInfo, Debug, PartialEq, Eq)]
pub enum DelayPolicy {
    /// Only explicitly delay transactions using `schedule_dispatch` call
    #[default]
    Explicit,
    /// Intercept and delay transactions at `TransactionExtension` level. This is not UX friendly
    /// since it will return `TransactionValidityError` to the caller, but will still manage
    /// to delay the transaction.
    ///
    /// This is an opt-in feature and will not be enabled by default.
    Intercept,
}

#[frame_support::pallet]
pub mod pallet {
    use super::*;
    use frame_support::traits::{Bounded, CallerTrait, QueryPreimage, StorePreimage};
    use frame_support::{
        traits::schedule::v3::{Named, TaskName},
        PalletId,
    };
    use sp_runtime::traits::AccountIdConversion;
    use sp_runtime::traits::Hash;
    use sp_runtime::traits::{BlockNumberProvider, Dispatchable};
    use sp_runtime::Saturating;

    #[pallet::pallet]
    pub struct Pallet<T>(_);

    #[pallet::config]
    pub trait Config: frame_system::Config {
        /// The overarching runtime event type.
        type RuntimeEvent: From<Event<Self>> + IsType<<Self as frame_system::Config>::RuntimeEvent>;

        /// The overarching runtime call type. Must be dispatchable and encodable.
        type RuntimeCall: Parameter
            + Dispatchable<RuntimeOrigin = Self::RuntimeOrigin, PostInfo = PostDispatchInfo>
            + GetDispatchInfo
            + From<Call<Self>>
            + Encode
            + Decode
            + Clone
            + Eq
            + PartialEq
            + IsType<<Self as frame_system::Config>::RuntimeCall>;

        /// Scheduler for the runtime. We use the Named scheduler for cancellability.
        type Scheduler: Named<
            BlockNumberFor<Self>,
            <Self as Config>::RuntimeCall,
            Self::SchedulerOrigin,
            Hasher = Self::Hashing,
        >;

        /// Scheduler origin
        type SchedulerOrigin: From<frame_system::RawOrigin<Self::AccountId>>
            + CallerTrait<Self::AccountId>
            + MaxEncodedLen;

        /// Block number provider for scheduling.
        type BlockNumberProvider: BlockNumberProvider<BlockNumber = BlockNumberFor<Self>>;

        /// Maximum pending reversible transactions allowed per account. Used for BoundedVec.
        #[pallet::constant]
        type MaxPendingPerAccount: Get<u32>;

        /// The default delay period for reversible transactions if none is specified.
        #[pallet::constant]
        type DefaultDelay: Get<BlockNumberFor<Self>>;

        /// The minimum delay period allowed for reversible transactions.
        #[pallet::constant]
        type MinDelayPeriod: Get<BlockNumberFor<Self>>;

        /// Pallet Id
        type PalletId: Get<PalletId>;

        /// The preimage provider with which we look up call hashes to get the call.
        type Preimages: QueryPreimage<H = Self::Hashing> + StorePreimage;

        // /// A type representing the weights required by the dispatchables of this pallet.
        // type WeightInfo: WeightInfo;
    }

    /// Maps accounts to their chosen reversibility delay period (in milliseconds).
    /// Accounts present in this map have reversibility enabled.
    #[pallet::storage]
    #[pallet::getter(fn reversible_accounts)]
    pub type ReversibleAccounts<T: Config> = StorageMap<
        _,
        Blake2_128Concat,
        T::AccountId,
        (BlockNumberFor<T>, DelayPolicy),
        OptionQuery,
    >;

    /// Stores the details of pending transactions scheduled for delayed execution.
    /// Keyed by the unique transaction ID.
    #[pallet::storage]
    #[pallet::getter(fn pending_dispatches)]
    pub type PendingDispatches<T: Config> = StorageMap<
        _,
        Blake2_128Concat,
        T::Hash,
        (
            T::AccountId,
            Bounded<<T as Config>::RuntimeCall, T::Hashing>,
        ),
        OptionQuery,
    >;

    /// Indexes pending transaction IDs per account for efficient lookup and cancellation.
    /// Also enforces the maximum pending transactions limit per account.
    #[pallet::storage]
    #[pallet::getter(fn account_pending_index)]
    pub type AccountPendingIndex<T: Config> =
        StorageMap<_, Blake2_128Concat, T::AccountId, u32, ValueQuery>;

    #[pallet::event]
    #[pallet::generate_deposit(pub(super) fn deposit_event)]
    pub enum Event<T: Config> {
        /// A user has enabled or updated their reversibility settings.
        /// [who, maybe_delay: None means disabled]
        ReversibilitySet {
            who: T::AccountId,
            delay: BlockNumberFor<T>,
            policy: DelayPolicy,
        },
        /// A transaction has been intercepted and scheduled for delayed execution.
        /// [who, tx_id, execute_at_moment]
        TransactionScheduled {
            who: T::AccountId,
            tx_id: T::Hash,
            execute_at: DispatchTime<BlockNumberFor<T>>,
        },
        /// A scheduled transaction has been successfully cancelled by the owner.
        /// [who, tx_id]
        TransactionCancelled { who: T::AccountId, tx_id: T::Hash },
        /// A scheduled transaction was executed by the scheduler.
        /// [tx_id, dispatch_result]
        TransactionExecuted {
            tx_id: T::Hash,
            result: DispatchResultWithPostInfo,
        },
    }

    #[pallet::error]
    pub enum Error<T> {
        /// The account attempting the action is not marked as reversible.
        AccountNotReversible,
        /// The specified pending transaction ID was not found.
        PendingTxNotFound,
        /// The caller is not the original submitter of the transaction they are trying to cancel.
        NotOwner,
        /// The account has reached the maximum number of pending reversible transactions.
        TooManyPendingTransactions,
        /// The specified delay period is below the configured minimum.
        DelayTooShort,
        /// Failed to schedule the transaction execution with the scheduler pallet.
        SchedulingFailed,
        /// Failed to cancel the scheduled task with the scheduler pallet.
        CancellationFailed,
        /// The provided transaction ID is already associated with a pending transaction.
        AlreadyScheduled, // Should be rare with good TxIdProvider but possible
        /// Failed to decode the OpaqueCall back into a RuntimeCall.
        CallDecodingFailed,
        /// Call is invalid.
        InvalidCall,
        /// Invalid scheduler origin
        InvalidSchedulerOrigin,
    }

    #[pallet::call]
    impl<T: Config> Pallet<T> {
        /// Enable reversibility for the calling account with a specified delay, or disable it.
        ///
        /// - `delay`: The time (in milliseconds) after submission before the transaction executes.
        ///   If `None`, reversibility is disabled for the account.
        ///   If `Some`, must be >= `MinDelayPeriod`.
        #[pallet::call_index(0)]
        #[pallet::weight(T::DbWeight::get().writes(1))]
        pub fn set_reversibility(
            origin: OriginFor<T>,
            delay: Option<BlockNumberFor<T>>,
            policy: DelayPolicy,
        ) -> DispatchResult {
            let who = ensure_signed(origin)?;

            let delay = delay.unwrap_or(T::DefaultDelay::get());

            ensure!(delay >= T::MinDelayPeriod::get(), Error::<T>::DelayTooShort);

            ReversibleAccounts::<T>::insert(&who, (delay, policy.clone()));
            Self::deposit_event(Event::ReversibilitySet { who, delay, policy });

            Ok(())
        }

        /// Cancel a pending reversible transaction scheduled by the caller.
        ///
        /// - `tx_id`: The unique identifier of the transaction to cancel.
        #[pallet::call_index(1)]
        #[pallet::weight(T::DbWeight::get().writes(1))]
        pub fn cancel(origin: OriginFor<T>, tx_id: T::Hash) -> DispatchResult {
            let who = ensure_signed(origin)?;
            Self::cancel_dispatch(&who, tx_id)
        }

        /// Called by the Scheduler to finalize the scheduled task/call
        ///
        /// - `tx_id`: The unique id of the transaction to finalize and dispatch.
        #[pallet::call_index(2)]
        #[pallet::weight(T::DbWeight::get().writes(1))]
        pub fn execute_dispatch(
            origin: OriginFor<T>,
            tx_id: T::Hash,
        ) -> DispatchResultWithPostInfo {
            let who = ensure_signed(origin)?;

            ensure!(
                who == Self::account_id(),
                Error::<T>::InvalidSchedulerOrigin
            );

            Self::do_execute_dispatch(&tx_id)
        }

        /// Schedule a transaction for delayed execution.
        #[pallet::call_index(3)]
        #[pallet::weight(T::DbWeight::get().writes(1))]
        pub fn schedule_dispatch(
            origin: OriginFor<T>,
            call: Box<<T as Config>::RuntimeCall>,
        ) -> DispatchResult {
            Self::do_schedule_dispatch(origin, *call)
        }
    }

    #[pallet::hooks]
    impl<T: Config> Hooks<BlockNumberFor<T>> for Pallet<T> {
        fn integrity_test() {
            assert!(
                T::MinDelayPeriod::get() > Zero::zero(),
                "`T::MinDelayPeriod` must be greater than 0"
            );
            assert!(
                T::MinDelayPeriod::get() <= T::DefaultDelay::get(),
                "`T::MinDelayPeriod` must be less or equal to `T::DefaultDelay`"
            );
        }
    }

    impl<T: Config> Pallet<T> {
        /// Check if an account has reversibility enabled and return its delay.
        pub fn is_reversible(who: &T::AccountId) -> Option<(BlockNumberFor<T>, DelayPolicy)> {
            ReversibleAccounts::<T>::get(who)
        }

        // Pallet account as origin
        pub fn account_id() -> T::AccountId {
            T::PalletId::get().into_account_truncating()
        }

        fn do_execute_dispatch(tx_id: &T::Hash) -> DispatchResultWithPostInfo {
            let (who, call) =
                PendingDispatches::<T>::take(tx_id).ok_or(Error::<T>::PendingTxNotFound)?;

            // get from preimages
            let (call, _) = T::Preimages::realize::<<T as Config>::RuntimeCall>(&call)
                .map_err(|_| Error::<T>::CallDecodingFailed)?;

            let post_info = call.dispatch(frame_system::RawOrigin::Signed(who.clone()).into());

            // Remove from account index
            AccountPendingIndex::<T>::mutate(&who, |current_count| {
                // Decrement the count of pending transactions for the account.
                *current_count = current_count.saturating_sub(1);
            });

            // Remove from main storage
            PendingDispatches::<T>::remove(tx_id);

            // Emit event
            Self::deposit_event(Event::TransactionExecuted {
                tx_id: tx_id.clone(),
                result: post_info.clone(),
            });

            post_info
        }

        /// Simply converts hash output to a `TaskName`
        pub fn make_schedule_id(tx_id: &T::Hash) -> Result<TaskName, DispatchError> {
            let task_name: TaskName = tx_id
                .clone()
                .as_ref()
                .try_into()
                .map_err(|_| Error::<T>::InvalidCall)?;

            Ok(task_name)
        }

        /// Schedules a runtime call for delayed execution.
        /// This is intended to be called by the `TransactionExtension`, NOT directly by users.
        pub fn do_schedule_dispatch(
            origin: T::RuntimeOrigin,
            call: <T as Config>::RuntimeCall,
        ) -> DispatchResult {
            let who = ensure_signed(origin)?;
            let (delay, _) =
                Self::reversible_accounts(&who).ok_or(Error::<T>::AccountNotReversible)?;

            let tx_id = T::Hashing::hash_of(&(who.clone(), call.clone()).encode());

            // Ensure this tx_id isn't already pending (should be rare)
            ensure!(
                !PendingDispatches::<T>::contains_key(&tx_id),
                Error::<T>::AlreadyScheduled
            );

            // Check if the account can accommodate another pending transaction
            AccountPendingIndex::<T>::mutate(&who, |current_count| -> Result<(), DispatchError> {
                ensure!(
                    *current_count < T::MaxPendingPerAccount::get(),
                    Error::<T>::TooManyPendingTransactions
                );
                *current_count = current_count.saturating_add(1);
                Ok(())
            })?;

            let dispatch_time = DispatchTime::At(
                T::BlockNumberProvider::current_block_number().saturating_add(delay),
            );

            let call = T::Preimages::bound(call)?;

            // Store details before scheduling
            PendingDispatches::<T>::insert(&tx_id, (who.clone(), Box::new(call.clone())));

            let schedule_id = Self::make_schedule_id(&tx_id)?;

            let bounded_call = T::Preimages::bound(Call::<T>::execute_dispatch { tx_id }.into())?;

            // Schedule the `do_execute` call
            T::Scheduler::schedule_named(
                schedule_id,
                dispatch_time,
                None,
                Default::default(),
                frame_support::dispatch::RawOrigin::Signed(Self::account_id()).into(),
                bounded_call,
            )
            .map_err(|e| {
                log::error!("Failed to schedule transaction: {:?}", e);
                Error::<T>::SchedulingFailed
            })?;

            Self::deposit_event(Event::TransactionScheduled {
                who,
                tx_id,
                execute_at: dispatch_time,
            });

            Ok(())
        }

        /// Cancels a previously scheduled transaction. Internal logic used by `cancel` extrinsic.
        fn cancel_dispatch(who: &T::AccountId, tx_id: T::Hash) -> DispatchResult {
            // Retrieve owner from storage to verify ownership
            let (owner, _) =
                PendingDispatches::<T>::get(&tx_id).ok_or(Error::<T>::PendingTxNotFound)?;

            ensure!(&owner == who, Error::<T>::NotOwner);

            // Remove from main storage
            PendingDispatches::<T>::remove(&tx_id);

            // Decrement account index
            AccountPendingIndex::<T>::mutate(&owner, |current_count| {
                // Decrement the count of pending transactions for the account.
                *current_count = current_count.saturating_sub(1);
            });

            let schedule_id = Self::make_schedule_id(&tx_id)?;

            // Cancel the scheduled task
            T::Scheduler::cancel_named(schedule_id).map_err(|_| Error::<T>::CancellationFailed)?;

            Self::deposit_event(Event::TransactionCancelled {
                who: who.clone(),
                tx_id,
            });
            Ok(())
        }
    }

    #[pallet::genesis_config]
    #[derive(frame_support::DefaultNoBound)]
    pub struct GenesisConfig<T: Config> {
        /// Configure initial reversible accounts. [AccountId, Delay]
        pub initial_reversible_accounts: Vec<(T::AccountId, BlockNumberFor<T>)>,
    }

    #[pallet::genesis_build]
    impl<T: Config> BuildGenesisConfig for GenesisConfig<T> {
        fn build(&self) {
            for (who, delay) in &self.initial_reversible_accounts {
                // Basic validation, ensure delay is reasonable if needed
                if *delay >= T::MinDelayPeriod::get() {
                    ReversibleAccounts::<T>::insert(who, (delay, DelayPolicy::Explicit));
                } else {
                    // Optionally log a warning during genesis build
                    log::warn!(
                        "Genesis config for account {:?} has delay {:?} below MinDelayPeriod {:?}, skipping.",
                        who, delay, T::MinDelayPeriod::get()
                     );
                }
            }
        }
    }
}
