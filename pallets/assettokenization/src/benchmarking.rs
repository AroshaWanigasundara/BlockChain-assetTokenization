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

fn make_collection_name() -> BoundedVec<u8, frame_support::traits::ConstU32<64>> {
    BoundedVec::try_from(b"Benchmark Collection".to_vec()).unwrap()
}

fn make_uri() -> BoundedVec<u8, frame_support::traits::ConstU32<256>> {
    BoundedVec::try_from(b"ipfs://QmBenchmark".to_vec()).unwrap()
}

fn make_updated_uri() -> BoundedVec<u8, frame_support::traits::ConstU32<256>> {
    BoundedVec::try_from(b"ipfs://QmBenchmarkUpdated".to_vec()).unwrap()
}

/// Insert a collection owned by `owner` at ID 0 and return the ID.
fn setup_collection<T: Config>(owner: &T::AccountId) -> u64 {
    Collections::<T>::insert(
        0u64,
        crate::pallet::CollectionInfo {
            name: make_collection_name(),
            owner: owner.clone(),
            is_frozen: false,
        },
    );
    NextCollectionId::<T>::put(1u64);
    0u64
}

/// Insert a basic non-fungible asset owned by `owner` at ID 0 and return the ID.
fn setup_asset<T: Config>(owner: &T::AccountId) -> u64 {
    Assets::<T>::insert(
        0u64,
        crate::pallet::AssetInfo {
            name: make_name(),
            asset_type: crate::pallet::AssetType::Digital,
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
    0u64
}

#[benchmarks]
mod benchmarks {
    use super::*;
    use crate::pallet::{AssetType, CollectionRoleSet};

    // ── mint_asset ────────────────────────────────────────────────────────────
    // Worst case: minting into a collection (includes collection + role reads).
    #[benchmark]
    fn mint_asset() {
        let caller: T::AccountId = whitelisted_caller();
        setup_collection::<T>(&caller);

        #[extrinsic_call]
        mint_asset(
            RawOrigin::Signed(caller.clone()),
            make_name(),
            AssetType::Digital,
            make_uri(),
            valid_hash(),
            false,
            None,
            Some(0u64),
        );

        assert!(Assets::<T>::contains_key(0u64));
        assert_eq!(AssetCollection::<T>::get(0u64), Some(0u64));
    }

    // ── sign_contract ─────────────────────────────────────────────────────────
    #[benchmark]
    fn sign_contract() {
        let owner: T::AccountId = whitelisted_caller();
        setup_asset::<T>(&owner);

        let signer: T::AccountId = account("signer", 0, 0);

        #[extrinsic_call]
        sign_contract(RawOrigin::Signed(signer.clone()), 0u64);

        assert!(ContractSignatures::<T>::contains_key(0u64, signer));
    }

    // ── update_contract ───────────────────────────────────────────────────────
    // Worst case: first update — writes one ContractHistory entry.
    #[benchmark]
    fn update_contract() {
        let owner: T::AccountId = whitelisted_caller();
        setup_asset::<T>(&owner);

        let new_hash = [2u8; 32];

        #[extrinsic_call]
        update_contract(RawOrigin::Signed(owner), 0u64, make_updated_uri(), new_hash);

        let info = Assets::<T>::get(0u64).unwrap();
        assert_eq!(info.contract_hash, new_hash);
        // History entry 0 should hold the original hash.
        assert_eq!(ContractHistory::<T>::get(0u64, 0u32), Some(valid_hash()));
        assert_eq!(ContractHistoryCount::<T>::get(0u64), 1u32);
    }

    // ── freeze_asset ──────────────────────────────────────────────────────────
    // Worst case: caller is the collection freezer (includes collection reads).
    #[benchmark]
    fn freeze_asset() {
        let owner: T::AccountId = whitelisted_caller();
        let coll_id = setup_collection::<T>(&owner);
        setup_asset::<T>(&owner);
        AssetCollection::<T>::insert(0u64, coll_id);

        #[extrinsic_call]
        freeze_asset(RawOrigin::Signed(owner), 0u64);

        assert!(FrozenAssets::<T>::get(0u64));
    }

    // ── transfer_asset ────────────────────────────────────────────────────────
    #[benchmark]
    fn transfer_asset() {
        let owner: T::AccountId = whitelisted_caller();
        setup_asset::<T>(&owner);
        // Owner must sign the contract first.
        ContractSignatures::<T>::insert(0u64, &owner, 0u64);

        let recipient: T::AccountId = account("recipient", 0, 0);

        #[extrinsic_call]
        transfer_asset(RawOrigin::Signed(owner), 0u64, recipient.clone());

        assert_eq!(AssetOwner::<T>::get(0u64), Some(recipient));
    }

    // ── create_collection ─────────────────────────────────────────────────────
    #[benchmark]
    fn create_collection() {
        let caller: T::AccountId = whitelisted_caller();

        #[extrinsic_call]
        create_collection(RawOrigin::Signed(caller.clone()), make_collection_name());

        assert!(Collections::<T>::contains_key(0u64));
    }

    // ── set_collection_roles ─────────────────────────────────────────────────
    // Worst case: owner grants all non-admin roles to another account.
    #[benchmark]
    fn set_collection_roles() {
        let owner: T::AccountId = whitelisted_caller();
        setup_collection::<T>(&owner);

        let target: T::AccountId = account("target", 0, 0);
        let roles = CollectionRoleSet { is_admin: false, is_issuer: true, is_freezer: true };

        #[extrinsic_call]
        set_collection_roles(RawOrigin::Signed(owner), 0u64, target.clone(), roles.clone());

        assert_eq!(CollectionRoles::<T>::get(0u64, target), Some(roles));
    }

    // ── transfer_fungible ─────────────────────────────────────────────────────
    #[benchmark]
    fn transfer_fungible() {
        let owner: T::AccountId = whitelisted_caller();
        let supply: u128 = 1_000_000;

        // Mint a fungible asset manually.
        Assets::<T>::insert(
            0u64,
            crate::pallet::AssetInfo {
                name: make_name(),
                asset_type: AssetType::Physical,
                contract_uri: make_uri(),
                contract_hash: valid_hash(),
                is_fungible: true,
                fungible_supply: Some(supply),
                creator: owner.clone(),
                created_at: frame_system::Pallet::<T>::block_number(),
            },
        );
        AssetOwner::<T>::insert(0u64, owner.clone());
        NextAssetId::<T>::put(1u64);
        FungibleBalances::<T>::insert(0u64, &owner, supply);

        let recipient: T::AccountId = account("recipient", 0, 0);

        #[extrinsic_call]
        transfer_fungible(RawOrigin::Signed(owner), 0u64, recipient.clone(), 500_000u128);

        assert_eq!(FungibleBalances::<T>::get(0u64, recipient), 500_000u128);
    }

    impl_benchmark_test_suite!(
        AssetTokenization,
        crate::mock::new_test_ext(),
        crate::mock::Test
    );
}
