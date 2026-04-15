//! Benchmarking setup for pallet-assettokenization

use super::*;

#[allow(unused)]
use crate::Pallet as AssetTokenization;
use frame_benchmarking::v2::*;
use frame_support::BoundedVec;
use frame_system::RawOrigin;

fn valid_hash() -> [u8; 32] {
    [1u8; 32]
}

fn make_name() -> BoundedVec<u8, frame_support::traits::ConstU32<64>> {
    BoundedVec::try_from(b"Benchmark Asset".to_vec()).unwrap()
}

fn make_uri() -> BoundedVec<u8, frame_support::traits::ConstU32<256>> {
    BoundedVec::try_from(b"ipfs://QmBenchmark".to_vec()).unwrap()
}

#[benchmarks]
mod benchmarks {
    use super::*;
    use crate::pallet::AssetType;

    #[benchmark]
    fn mint_asset() {
        let caller: T::AccountId = whitelisted_caller();
        #[extrinsic_call]
        mint_asset(
            RawOrigin::Signed(caller.clone()),
            make_name(),
            AssetType::Digital,
            make_uri(),
            valid_hash(),
            false,
            None,
        );

        assert!(Assets::<T>::contains_key(0u64));
    }

    #[benchmark]
    fn sign_contract() {
        let owner: T::AccountId = whitelisted_caller();
        // Pre-mint an asset so there is something to sign.
        Assets::<T>::insert(
            0u64,
            crate::pallet::AssetInfo {
                name: make_name(),
                asset_type: AssetType::Digital,
                contract_uri: make_uri(),
                contract_hash: valid_hash(),
                is_fungible: false,
                fungible_supply: None,
                creator: owner.clone(),
                created_at: frame_system::Pallet::<T>::block_number(),
            },
        );
        AssetOwner::<T>::insert(0u64, owner.clone());
        NextAssetId::<T>::put(1u64);

        let signer: T::AccountId = account("signer", 0, 0);
        #[extrinsic_call]
        sign_contract(RawOrigin::Signed(signer.clone()), 0u64);

        assert!(ContractSignatures::<T>::contains_key(0u64, signer));
    }

    #[benchmark]
    fn transfer_asset() {
        let owner: T::AccountId = whitelisted_caller();
        Assets::<T>::insert(
            0u64,
            crate::pallet::AssetInfo {
                name: make_name(),
                asset_type: AssetType::Physical,
                contract_uri: make_uri(),
                contract_hash: valid_hash(),
                is_fungible: false,
                fungible_supply: None,
                creator: owner.clone(),
                created_at: frame_system::Pallet::<T>::block_number(),
            },
        );
        AssetOwner::<T>::insert(0u64, owner.clone());
        NextAssetId::<T>::put(1u64);

        let recipient: T::AccountId = account("recipient", 0, 0);
        #[extrinsic_call]
        transfer_asset(RawOrigin::Signed(owner), 0u64, recipient.clone());

        assert_eq!(AssetOwner::<T>::get(0u64), Some(recipient));
    }

    impl_benchmark_test_suite!(
        AssetTokenization,
        crate::mock::new_test_ext(),
        crate::mock::Test
    );
}