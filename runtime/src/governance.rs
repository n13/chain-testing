use crate::{
    AccountId, Balance, Balances, BlockNumber, Runtime, RuntimeOrigin, DAYS, HOURS, MICRO_UNIT,
    UNIT,
};
use alloc::vec::Vec;
use codec::{Decode, Encode, EncodeLike, MaxEncodedLen};
use frame_support::pallet_prelude::TypeInfo;
#[cfg(feature = "runtime-benchmarks")]
use frame_support::traits::Currency;
use frame_support::traits::{
    CallerTrait, Consideration, EnsureOrigin, EnsureOriginWithArg, Footprint, Get, OriginTrait,
    ReservableCurrency,
};
use pallet_ranked_collective::Rank;
use sp_core::crypto::AccountId32;
use sp_runtime::traits::{Convert, MaybeConvert};
use sp_runtime::{DispatchError, Perbill};
use sp_std::marker::PhantomData;

///Preimage pallet fee model

#[derive(Encode, Decode, Clone, PartialEq, Eq, TypeInfo, MaxEncodedLen, Debug)]
pub struct PreimageDeposit {
    amount: Balance,
}

impl Consideration<AccountId, Footprint> for PreimageDeposit {
    fn new(who: &AccountId, footprint: Footprint) -> Result<Self, DispatchError> {
        // Simple fee model: 0.1 UNIT + 0.0001 UNIT for one byte
        let base = UNIT / 10;
        let per_byte = MICRO_UNIT / 10;
        let size = (footprint.size as u128).saturating_add(footprint.count as u128);
        let amount = base.saturating_add(per_byte.saturating_mul(size));

        Balances::reserve(who, amount)?;
        Ok(Self { amount })
    }

    fn update(self, who: &AccountId, new_footprint: Footprint) -> Result<Self, DispatchError> {
        // Calculate new amount
        let base = UNIT / 10;
        let per_byte = MICRO_UNIT / 10;
        let size = (new_footprint.size as u128).saturating_add(new_footprint.count as u128);
        let new_amount = base.saturating_add(per_byte.saturating_mul(size));

        // Release old deposite
        Balances::unreserve(who, self.amount);

        // Take new deposite
        Balances::reserve(who, new_amount)?;

        Ok(Self { amount: new_amount })
    }

    fn drop(self, who: &AccountId) -> Result<(), DispatchError> {
        Balances::unreserve(who, self.amount);
        Ok(())
    }

    ///We will have to finally focus on fees, so weight and benchamrks will be important.
    /// For now, it's AI implementation

    #[cfg(feature = "runtime-benchmarks")]
    fn ensure_successful(who: &AccountId, footprint: Footprint) {
        let base = UNIT / 10;
        let per_byte = MICRO_UNIT / 10;
        let size = (footprint.size as u128).saturating_add(footprint.count as u128);
        let amount = base.saturating_add(per_byte.saturating_mul(size));

        // Check if user has enough coins
        if Balances::free_balance(who) < amount {
            Balances::make_free_balance_be(who, amount.saturating_mul(2));
        }
    }
}

// Define tracks for referenda
pub struct CommunityTracksInfo;
impl pallet_referenda::TracksInfo<Balance, BlockNumber> for CommunityTracksInfo {
    type Id = u16;
    type RuntimeOrigin = <RuntimeOrigin as frame_support::traits::OriginTrait>::PalletsOrigin;

    fn tracks() -> &'static [(Self::Id, pallet_referenda::TrackInfo<Balance, BlockNumber>)] {
        static TRACKS: [(u16, pallet_referenda::TrackInfo<Balance, BlockNumber>); 2] = [
            // Track 0: Signed Track (authenticated proposals)
            // - For proposals from authenticated users that require privileges
            // - Less stringent than root but still requires identity
            (
                0,
                pallet_referenda::TrackInfo {
                    name: "signed",
                    max_deciding: 5, // Allow several concurrent proposals
                    decision_deposit: 500 * UNIT, // Moderate deposit
                    prepare_period: 12 * HOURS, // Shorter preparation time
                    decision_period: 7 * DAYS, // 1 week voting period
                    confirm_period: 12 * HOURS, // 12 hours confirmation
                    min_enactment_period: 1 * DAYS, // 1 day until execution
                    min_approval: pallet_referenda::Curve::LinearDecreasing {
                        length: Perbill::from_percent(100),
                        floor: Perbill::from_percent(55), // Majority approval required
                        ceil: Perbill::from_percent(70),
                    },
                    min_support: pallet_referenda::Curve::LinearDecreasing {
                        length: Perbill::from_percent(100),
                        floor: Perbill::from_percent(5),
                        ceil: Perbill::from_percent(25),
                    },
                },
            ),
            // Track 1: Signaling Track (non-binding community opinions)
            // - For community sentiment and direction gathering
            (
                1,
                pallet_referenda::TrackInfo {
                    name: "signaling",
                    max_deciding: 20, // High throughput for community proposals
                    decision_deposit: 100 * UNIT, // Low deposit requirement
                    prepare_period: 6 * HOURS, // Short preparation time
                    decision_period: 5 * DAYS, // Standard voting period
                    confirm_period: 3 * HOURS, // Minimal confirmation period
                    min_enactment_period: 1, // 1 Block - immediate "execution" (just for record-keeping)
                    min_approval: pallet_referenda::Curve::LinearDecreasing {
                        length: Perbill::from_percent(100),
                        floor: Perbill::from_percent(50), // Simple majority approval
                        ceil: Perbill::from_percent(60),
                    },
                    min_support: pallet_referenda::Curve::LinearDecreasing {
                        length: Perbill::from_percent(100),
                        floor: Perbill::from_percent(1), // Very low support threshold
                        ceil: Perbill::from_percent(10),
                    },
                },
            ),
        ];
        &TRACKS
    }

    fn track_for(id: &Self::RuntimeOrigin) -> Result<Self::Id, ()> {
        // Check for system origins first
        if let Some(system_origin) = id.as_system_ref() {
            match system_origin {
                frame_system::RawOrigin::None => return Ok(1), // None origin uses track 1
                _ => {}
            }
        }

        if let Some(_signer) = id.as_signed() {
            return Ok(0); // Signed users use track 0
        }

        Err(())
    }

    fn info(id: Self::Id) -> Option<&'static pallet_referenda::TrackInfo<Balance, BlockNumber>> {
        Self::tracks()
            .iter()
            .find(|(track_id, _)| *track_id == id)
            .map(|(_, info)| info)
    }

    fn check_integrity() -> Result<(), &'static str> {
        // Basic check that all track IDs are unique
        let mut track_ids = Self::tracks().iter().map(|(id, _)| *id).collect::<Vec<_>>();
        track_ids.sort();
        track_ids.dedup();
        if track_ids.len() != Self::tracks().len() {
            return Err("Duplicate track IDs found");
        }
        Ok(())
    }
}

pub struct TechCollectiveTracksInfo;
impl pallet_referenda::TracksInfo<Balance, BlockNumber> for TechCollectiveTracksInfo {
    type Id = u16;
    type RuntimeOrigin = <RuntimeOrigin as frame_support::traits::OriginTrait>::PalletsOrigin;

    fn tracks() -> &'static [(Self::Id, pallet_referenda::TrackInfo<Balance, BlockNumber>)] {
        static TRACKS: [(u16, pallet_referenda::TrackInfo<Balance, BlockNumber>); 1] = [
            // Track 0: Root Track (major system changes)
            // - Highest privileges for critical protocol upgrades and parameter changes
            (
                0,
                pallet_referenda::TrackInfo {
                    name: "root",
                    max_deciding: 1, // Only 1 referendum can be in deciding phase at a time
                    decision_deposit: 1000 * UNIT, // Highest deposit requirement to prevent spam
                    prepare_period: 1 * DAYS, // 1 day preparation before voting begins
                    decision_period: 5 * DAYS, // 5 days for community to vote
                    confirm_period: 2 * DAYS, // 2 days confirmation period once passing
                    min_enactment_period: 2 * DAYS, // 2 day between approval and execution
                    min_approval: pallet_referenda::Curve::LinearDecreasing {
                        length: Perbill::from_percent(100),
                        floor: Perbill::from_percent(75), // Minimum 75% approval at end
                        ceil: Perbill::from_percent(100), // Requires 100% approval at start
                    },
                    min_support: pallet_referenda::Curve::LinearDecreasing {
                        length: Perbill::from_percent(0),
                        //In this way support param is off.
                        floor: Perbill::from_percent(0),
                        ceil: Perbill::from_percent(0),
                    },
                },
            ),
        ];
        &TRACKS
    }

    fn track_for(id: &Self::RuntimeOrigin) -> Result<Self::Id, ()> {
        // Check for system origins first
        if let Some(system_origin) = id.as_system_ref() {
            match system_origin {
                frame_system::RawOrigin::Root => return Ok(0), // Root can use track 0
                frame_system::RawOrigin::None => return Ok(2), // None origin uses track 2
                _ => {}
            }
        }

        // Check for signed origins - simplified version
        if let Some(_signer) = id.as_signed() {
            return Ok(1);
        }
        Err(())
    }

    fn info(id: Self::Id) -> Option<&'static pallet_referenda::TrackInfo<Balance, BlockNumber>> {
        Self::tracks()
            .iter()
            .find(|(track_id, _)| *track_id == id)
            .map(|(_, info)| info)
    }

    fn check_integrity() -> Result<(), &'static str> {
        // Basic check that all track IDs are unique
        let mut track_ids = Self::tracks().iter().map(|(id, _)| *id).collect::<Vec<_>>();
        track_ids.sort();
        track_ids.dedup();
        if track_ids.len() != Self::tracks().len() {
            return Err("Duplicate track IDs found");
        }
        Ok(())
    }
}

/// Converts a track ID to a minimum required rank for voting.
/// Currently, all tracks require rank 0 as the minimum rank.
/// In the future, this could be extended to support multiple ranks
/// where different tracks might require different minimum ranks.
/// For example:
/// - Track 1 might require rank 0
/// - Track 2 might require rank 1
/// - Track 3 might require rank 2
/// This would allow for a hierarchical voting system where higher-ranked
/// members can vote on more important proposals.
pub struct MinRankOfClassConverter<Delta>(PhantomData<Delta>);
impl<Delta: Get<u16>> Convert<u16, u16> for MinRankOfClassConverter<Delta> {
    fn convert(_a: u16) -> u16 {
        0 // Currently, all tracks require rank 0 as the minimum rank
    }
}

pub struct GlobalMaxMembers<MaxVal: Get<u32>>(PhantomData<MaxVal>);

impl<MaxVal: Get<u32>> MaybeConvert<u16, u32> for GlobalMaxMembers<MaxVal> {
    fn maybe_convert(_a: u16) -> Option<u32> {
        Some(MaxVal::get())
    }
}

pub struct RootOrMemberForCollectiveOriginImpl<Runtime, I>(PhantomData<(Runtime, I)>);

impl<Runtime, I> EnsureOrigin<Runtime::RuntimeOrigin>
    for RootOrMemberForCollectiveOriginImpl<Runtime, I>
where
    Runtime: pallet_ranked_collective::Config<I> + frame_system::Config,
    <Runtime as frame_system::Config>::RuntimeOrigin:
        OriginTrait<PalletsOrigin = crate::OriginCaller>,
    for<'a> &'a AccountId32: EncodeLike<<Runtime as frame_system::Config>::AccountId>,
    I: 'static,
{
    type Success = Rank;

    fn try_origin(o: Runtime::RuntimeOrigin) -> Result<Self::Success, Runtime::RuntimeOrigin> {
        if <frame_system::EnsureRoot<Runtime::AccountId> as EnsureOrigin<
            Runtime::RuntimeOrigin,
        >>::try_origin(o.clone())
        .is_ok()
        {
            return Ok(0);
        }

        let original_o_for_error = o.clone();
        let pallets_origin = o.into_caller();

        match pallets_origin {
            crate::OriginCaller::system(frame_system::RawOrigin::Signed(who)) => {
                if pallet_ranked_collective::Members::<Runtime, I>::contains_key(&who) {
                    Ok(0)
                } else {
                    Err(original_o_for_error)
                }
            }
            _ => Err(original_o_for_error),
        }
    }

    #[cfg(feature = "runtime-benchmarks")]
    fn try_successful_origin() -> Result<Runtime::RuntimeOrigin, ()> {
        Ok(frame_system::RawOrigin::<Runtime::AccountId>::Root.into())
    }
}

pub type RootOrMemberForCollectiveOrigin = RootOrMemberForCollectiveOriginImpl<Runtime, ()>;

pub struct RootOrMemberForTechReferendaOriginImpl<Runtime, I>(PhantomData<(Runtime, I)>);

impl<Runtime, I> EnsureOriginWithArg<Runtime::RuntimeOrigin, crate::OriginCaller>
    for RootOrMemberForTechReferendaOriginImpl<Runtime, I>
where
    Runtime: frame_system::Config<AccountId = AccountId32> + pallet_ranked_collective::Config<I>,
    <Runtime as frame_system::Config>::RuntimeOrigin:
        OriginTrait<PalletsOrigin = crate::OriginCaller>,
    I: 'static,
{
    type Success = Runtime::AccountId;

    fn try_origin(
        o: Runtime::RuntimeOrigin,
        _: &crate::OriginCaller,
    ) -> Result<Self::Success, Runtime::RuntimeOrigin> {
        let pallets_origin = o.clone().into_caller();

        if let crate::OriginCaller::system(frame_system::RawOrigin::Root) = pallets_origin {
            if let Ok(signer) = <frame_system::EnsureSigned<Runtime::AccountId> as EnsureOrigin<
                Runtime::RuntimeOrigin,
            >>::try_origin(o.clone())
            {
                return Ok(signer);
            }
        }

        let original_o_for_error = o.clone();
        let pallets_origin = o.into_caller();

        match pallets_origin {
            crate::OriginCaller::system(frame_system::RawOrigin::Signed(who)) => {
                if pallet_ranked_collective::Members::<Runtime, I>::contains_key(&who) {
                    Ok(who)
                } else {
                    Err(original_o_for_error)
                }
            }
            _ => Err(original_o_for_error),
        }
    }

    #[cfg(feature = "runtime-benchmarks")]
    fn try_successful_origin(_arg: &crate::OriginCaller) -> Result<Runtime::RuntimeOrigin, ()> {
        Ok(
            frame_system::RawOrigin::<Runtime::AccountId>::Signed(AccountId32::new([0u8; 32]))
                .into(),
        )
    }
}

pub type RootOrMemberForTechReferendaOrigin = RootOrMemberForTechReferendaOriginImpl<Runtime, ()>;
