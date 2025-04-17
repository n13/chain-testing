//! Custom signed extensions for the runtime.
use crate::*;
use codec::{Decode, Encode};
use core::{marker::PhantomData, u8};
use frame_support::pallet_prelude::{InvalidTransaction, ValidTransaction};
use frame_system::ensure_signed;
use pallet_reversible_txs::DelayPolicy;
use scale_info::TypeInfo;
use sp_runtime::{traits::TransactionExtension, Weight};

/// Transaction extension for reversible accounts
///
/// This extension is used to intercept delayed transactions for users that opted in
/// for reversible transactions. Based on the policy set by the user, the transaction
/// will either be denied or intercepted and delayed.
#[derive(Encode, Decode, Clone, Eq, PartialEq, Default, TypeInfo, Debug)]
#[scale_info(skip_type_params(T))]
pub struct ReversibleTransactionExtension<T: pallet_reversible_txs::Config>(PhantomData<T>);

impl<T: pallet_reversible_txs::Config + Send + Sync> ReversibleTransactionExtension<T> {
    /// Creates new `TransactionExtension` to check genesis hash.
    pub fn new() -> Self {
        Self(core::marker::PhantomData)
    }
}

impl<T: pallet_reversible_txs::Config + Send + Sync + alloc::fmt::Debug>
    TransactionExtension<RuntimeCall> for ReversibleTransactionExtension<T>
{
    type Pre = ();
    type Val = ();
    type Implicit = ();

    const IDENTIFIER: &'static str = "ReversibleTransactionExtension";

    fn weight(&self, _call: &RuntimeCall) -> Weight {
        Weight::zero()
    }

    fn prepare(
        self,
        _val: Self::Val,
        _origin: &sp_runtime::traits::DispatchOriginOf<RuntimeCall>,
        _call: &RuntimeCall,
        _info: &sp_runtime::traits::DispatchInfoOf<RuntimeCall>,
        _len: usize,
    ) -> Result<Self::Pre, frame_support::pallet_prelude::TransactionValidityError> {
        Ok(())
    }

    fn validate(
        &self,
        origin: sp_runtime::traits::DispatchOriginOf<RuntimeCall>,
        call: &RuntimeCall,
        _info: &sp_runtime::traits::DispatchInfoOf<RuntimeCall>,
        _len: usize,
        _self_implicit: Self::Implicit,
        _inherited_implication: &impl sp_runtime::traits::Implication,
        _source: frame_support::pallet_prelude::TransactionSource,
    ) -> sp_runtime::traits::ValidateResult<Self::Val, RuntimeCall> {
        let who = ensure_signed(origin.clone()).map_err(|_| {
            frame_support::pallet_prelude::TransactionValidityError::Invalid(
                InvalidTransaction::BadSigner,
            )
        })?;

        if let Some((_, policy)) = ReversibleTxs::is_reversible(&who) {
            match policy {
                DelayPolicy::Explicit => {
                    return Err(
                        frame_support::pallet_prelude::TransactionValidityError::Invalid(
                            InvalidTransaction::Custom(0),
                        ),
                    );
                }
                DelayPolicy::Intercept => {
                    // Only intercept `Balances` calls for now.
                    if matches!(call, RuntimeCall::Balances(_)) {
                        let _ = ReversibleTxs::schedule_dispatch(origin.clone(), call.clone())
                            .map_err(|_| {
                                frame_support::pallet_prelude::TransactionValidityError::Invalid(
                                    InvalidTransaction::Custom(1),
                                )
                            })?;

                        return Err(
                            frame_support::pallet_prelude::TransactionValidityError::Unknown(
                                frame_support::pallet_prelude::UnknownTransaction::Custom(u8::MAX),
                            ),
                        );
                    }
                }
            }
        }

        Ok((ValidTransaction::default(), (), origin))
    }
}

#[cfg(test)]
mod tests {
    use frame_support::pallet_prelude::{TransactionValidityError, UnknownTransaction};
    use pallet_reversible_txs::PendingDispatches;
    use sp_runtime::{traits::TxBaseImplication, AccountId32};

    use super::*;
    fn alice() -> AccountId {
        AccountId32::from([1; 32])
    }

    fn bob() -> AccountId {
        AccountId32::from([2; 32])
    }
    fn charlie() -> AccountId {
        AccountId32::from([3; 32])
    }

    // Build genesis storage according to the mock runtime.
    pub fn new_test_ext() -> sp_io::TestExternalities {
        let mut t = frame_system::GenesisConfig::<Runtime>::default()
            .build_storage()
            .unwrap();

        pallet_balances::GenesisConfig::<Runtime> {
            balances: vec![
                (alice(), EXISTENTIAL_DEPOSIT * 10000),
                (bob(), EXISTENTIAL_DEPOSIT * 2),
            ],
        }
        .assimilate_storage(&mut t)
        .unwrap();

        pallet_reversible_txs::GenesisConfig::<Runtime> {
            initial_reversible_accounts: vec![(alice(), 10)],
        }
        .assimilate_storage(&mut t)
        .unwrap();

        t.into()
    }

    #[test]
    fn test_reversible_transaction_extension() {
        new_test_ext().execute_with(|| {
            // Test the reversible transaction extension
            let ext = ReversibleTransactionExtension::<Runtime>::new();
            let call = RuntimeCall::Balances(pallet_balances::Call::transfer_keep_alive {
                dest: MultiAddress::Id(bob()),
                value: 10 * EXISTENTIAL_DEPOSIT,
            });
            let origin = RuntimeOrigin::signed(alice());

            // Test the prepare method
            let pre = ext
                .clone()
                .prepare((), &origin, &call, &Default::default(), 0)
                .unwrap();
            assert_eq!(pre, ());

            // Test the validate method
            let result = ext.validate(
                origin,
                &call,
                &Default::default(),
                0,
                (),
                &TxBaseImplication::<()>(()),
                frame_support::pallet_prelude::TransactionSource::External,
            );
            // we should fail here with `InvalidTransaction::Custom(0)` since default policy is
            // `DelayPolicy::Explicit`
            assert_eq!(
                result.unwrap_err(),
                TransactionValidityError::Invalid(InvalidTransaction::Custom(0))
            );
            // Pending transactions should be empty
            assert_eq!(PendingDispatches::<Runtime>::iter().count(), 0);

            // Charlie opts in for intercept
            ReversibleTxs::set_reversibility(
                RuntimeOrigin::signed(charlie()),
                None,
                DelayPolicy::Intercept,
            )
            .unwrap();

            // Charlie sends bob a transaction
            let call = RuntimeCall::Balances(pallet_balances::Call::transfer_keep_alive {
                dest: MultiAddress::Id(bob()),
                value: 10 * EXISTENTIAL_DEPOSIT,
            });

            let origin = RuntimeOrigin::signed(charlie());

            // Test the prepare method
            let pre = ext
                .clone()
                .prepare((), &origin, &call, &Default::default(), 0)
                .unwrap();

            assert_eq!(pre, ());

            // Test the validate method
            let result = ext.validate(
                origin,
                &call,
                &Default::default(),
                0,
                (),
                &TxBaseImplication::<()>(()),
                frame_support::pallet_prelude::TransactionSource::External,
            );
            // we should fail here with `UnknownTransaction::Custom(u8::MAX)` since default policy is
            // `DelayPolicy::Intercept`
            assert_eq!(
                result.unwrap_err(),
                TransactionValidityError::Unknown(UnknownTransaction::Custom(u8::MAX))
            );

            // Pending transactions should contain the transaction
            assert_eq!(PendingDispatches::<Runtime>::iter().count(), 1);

            // Other calls should not be intercepted
            let call = RuntimeCall::System(frame_system::Call::remark {
                remark: vec![1, 2, 3],
            });
            let origin = RuntimeOrigin::signed(charlie());
            let result = ext.validate(
                origin,
                &call,
                &Default::default(),
                0,
                (),
                &TxBaseImplication::<()>(()),
                frame_support::pallet_prelude::TransactionSource::External,
            );

            // we should not fail here
            assert!(result.is_ok());
        });
    }
}
