use crate::{
    mock::*, AssetCollection, AssetOwner, Assets, CollectionRoles, Collections,
    ContractHistory, ContractHistoryCount, ContractSignatures, Error, Event,
    FrozenAssets, FungibleBalances,
    pallet::{AssetType, CollectionRoleSet},
};
use frame_support::{assert_noop, assert_ok, BoundedVec};

// ─────────────────────────────────────────────────────────────────────────────
// Helpers
// ─────────────────────────────────────────────────────────────────────────────

fn valid_hash() -> [u8; 32] {
    [1u8; 32]
}

/// Mint a basic non-fungible asset as `origin` (no collection). Returns asset ID 0.
fn mint_default(origin: u64) -> u64 {
    let name: BoundedVec<u8, frame_support::traits::ConstU32<64>> =
        BoundedVec::try_from(b"Test Asset".to_vec()).unwrap();
    let uri: BoundedVec<u8, frame_support::traits::ConstU32<256>> =
        BoundedVec::try_from(b"ipfs://Qm123".to_vec()).unwrap();

    assert_ok!(AssetTokenization::mint_asset(
        RuntimeOrigin::signed(origin),
        name,
        AssetType::Digital,
        uri,
        valid_hash(),
        false,
        None,
        None, // no collection
    ));
    0
}

/// Create a collection as `origin`. Returns collection ID 0.
fn create_default_collection(origin: u64) -> u64 {
    let name: BoundedVec<u8, frame_support::traits::ConstU32<64>> =
        BoundedVec::try_from(b"Test Collection".to_vec()).unwrap();
    assert_ok!(AssetTokenization::create_collection(
        RuntimeOrigin::signed(origin),
        name,
    ));
    0
}

// ─────────────────────────────────────────────────────────────────────────────
// 1. Successful mint (no collection)
// ─────────────────────────────────────────────────────────────────────────────
#[test]
fn mint_asset_works() {
    new_test_ext().execute_with(|| {
        System::set_block_number(1);

        let asset_id = mint_default(1);

        assert!(Assets::<Test>::contains_key(asset_id));
        assert_eq!(AssetOwner::<Test>::get(asset_id), Some(1u64));

        System::assert_last_event(
            Event::AssetMinted { asset_id, owner: 1, contract_hash: valid_hash() }.into(),
        );
    });
}

// ─────────────────────────────────────────────────────────────────────────────
// 2. sign_contract records the signer and emits the event
// ─────────────────────────────────────────────────────────────────────────────
#[test]
fn sign_contract_works() {
    new_test_ext().execute_with(|| {
        System::set_block_number(1);
        let asset_id = mint_default(1);

        assert_ok!(AssetTokenization::sign_contract(RuntimeOrigin::signed(2), asset_id));

        assert!(ContractSignatures::<Test>::contains_key(asset_id, 2u64));

        System::assert_last_event(
            Event::ContractSigned { asset_id, signer: 2, block: 1 }.into(),
        );
    });
}

// ─────────────────────────────────────────────────────────────────────────────
// 3. Frozen asset prevents update_contract
// ─────────────────────────────────────────────────────────────────────────────
#[test]
fn freeze_prevents_update() {
    new_test_ext().execute_with(|| {
        System::set_block_number(1);
        let asset_id = mint_default(1);

        assert_ok!(AssetTokenization::freeze_asset(RuntimeOrigin::signed(1), asset_id));
        assert!(FrozenAssets::<Test>::get(asset_id));

        let new_uri: BoundedVec<u8, frame_support::traits::ConstU32<256>> =
            BoundedVec::try_from(b"ipfs://new".to_vec()).unwrap();
        assert_noop!(
            AssetTokenization::update_contract(
                RuntimeOrigin::signed(1),
                asset_id,
                new_uri,
                [2u8; 32],
            ),
            Error::<Test>::AssetIsFrozen
        );
    });
}

// ─────────────────────────────────────────────────────────────────────────────
// 4. transfer_asset changes the owner
// ─────────────────────────────────────────────────────────────────────────────
#[test]
fn transfer_changes_owner() {
    new_test_ext().execute_with(|| {
        System::set_block_number(1);
        let asset_id = mint_default(1);

        assert_ok!(AssetTokenization::sign_contract(RuntimeOrigin::signed(1), asset_id));

        assert_ok!(AssetTokenization::transfer_asset(
            RuntimeOrigin::signed(1),
            asset_id,
            2u64,
        ));

        assert_eq!(AssetOwner::<Test>::get(asset_id), Some(2u64));
        System::assert_last_event(
            Event::AssetTransferred { asset_id, from: 1, to: 2 }.into(),
        );
    });
}

// ─────────────────────────────────────────────────────────────────────────────
// 5. Only the owner can freeze
// ─────────────────────────────────────────────────────────────────────────────
#[test]
fn only_owner_can_freeze() {
    new_test_ext().execute_with(|| {
        System::set_block_number(1);
        let asset_id = mint_default(1);

        assert_noop!(
            AssetTokenization::freeze_asset(RuntimeOrigin::signed(99), asset_id),
            Error::<Test>::NotAssetOwner
        );

        assert_ok!(AssetTokenization::freeze_asset(RuntimeOrigin::signed(1), asset_id));
    });
}

// ─────────────────────────────────────────────────────────────────────────────
// 6. Signing twice is rejected
// ─────────────────────────────────────────────────────────────────────────────
#[test]
fn double_sign_rejected() {
    new_test_ext().execute_with(|| {
        System::set_block_number(1);
        let asset_id = mint_default(1);

        assert_ok!(AssetTokenization::sign_contract(RuntimeOrigin::signed(2), asset_id));
        assert_noop!(
            AssetTokenization::sign_contract(RuntimeOrigin::signed(2), asset_id),
            Error::<Test>::AlreadySigned
        );
    });
}

// ─────────────────────────────────────────────────────────────────────────────
// 7. Non-owner cannot transfer
// ─────────────────────────────────────────────────────────────────────────────
#[test]
fn non_owner_cannot_transfer() {
    new_test_ext().execute_with(|| {
        System::set_block_number(1);
        let asset_id = mint_default(1);

        assert_noop!(
            AssetTokenization::transfer_asset(RuntimeOrigin::signed(99), asset_id, 3u64),
            Error::<Test>::NotAssetOwner
        );
    });
}

// ─────────────────────────────────────────────────────────────────────────────
// 8. Transfer blocked until owner has signed the contract
// ─────────────────────────────────────────────────────────────────────────────
#[test]
fn transfer_blocked_until_owner_signs() {
    new_test_ext().execute_with(|| {
        System::set_block_number(1);
        let asset_id = mint_default(1);

        assert_noop!(
            AssetTokenization::transfer_asset(RuntimeOrigin::signed(1), asset_id, 2u64),
            Error::<Test>::ContractNotSigned
        );

        assert_ok!(AssetTokenization::sign_contract(RuntimeOrigin::signed(1), asset_id));
        assert_ok!(AssetTokenization::transfer_asset(
            RuntimeOrigin::signed(1),
            asset_id,
            2u64,
        ));
        assert_eq!(AssetOwner::<Test>::get(asset_id), Some(2u64));
    });
}

// ─────────────────────────────────────────────────────────────────────────────
// 9. Fungible supply consistency guard
// ─────────────────────────────────────────────────────────────────────────────
#[test]
fn inconsistent_fungible_supply_rejected() {
    new_test_ext().execute_with(|| {
        System::set_block_number(1);
        let name: BoundedVec<u8, frame_support::traits::ConstU32<64>> =
            BoundedVec::try_from(b"Bad Asset".to_vec()).unwrap();
        let uri: BoundedVec<u8, frame_support::traits::ConstU32<256>> =
            BoundedVec::try_from(b"ipfs://Qm123".to_vec()).unwrap();

        // is_fungible = true but no supply — invalid.
        assert_noop!(
            AssetTokenization::mint_asset(
                RuntimeOrigin::signed(1),
                name.clone(),
                AssetType::Digital,
                uri.clone(),
                valid_hash(),
                true,
                None,
                None,
            ),
            Error::<Test>::InconsistentFungibleSupply
        );

        // is_fungible = false but supply provided — invalid.
        assert_noop!(
            AssetTokenization::mint_asset(
                RuntimeOrigin::signed(1),
                name.clone(),
                AssetType::Physical,
                uri.clone(),
                valid_hash(),
                false,
                Some(1000),
                None,
            ),
            Error::<Test>::InconsistentFungibleSupply
        );

        // Consistent: fungible with supply.
        assert_ok!(AssetTokenization::mint_asset(
            RuntimeOrigin::signed(1),
            name.clone(),
            AssetType::Digital,
            uri.clone(),
            valid_hash(),
            true,
            Some(1_000_000),
            None,
        ));
        // Consistent: non-fungible with None.
        assert_ok!(AssetTokenization::mint_asset(
            RuntimeOrigin::signed(1),
            name,
            AssetType::Physical,
            uri,
            valid_hash(),
            false,
            None,
            None,
        ));
    });
}

// =============================================================================
// Collection tests
// =============================================================================

// ─────────────────────────────────────────────────────────────────────────────
// 10. create_collection stores metadata and emits event
// ─────────────────────────────────────────────────────────────────────────────
#[test]
fn create_collection_works() {
    new_test_ext().execute_with(|| {
        System::set_block_number(1);
        let coll_id = create_default_collection(1);

        let info = Collections::<Test>::get(coll_id).expect("collection should exist");
        assert_eq!(info.owner, 1u64);
        assert!(!info.is_frozen);

        System::assert_last_event(
            Event::CollectionCreated { collection_id: coll_id, owner: 1 }.into(),
        );
    });
}

// ─────────────────────────────────────────────────────────────────────────────
// 11. Collection owner can assign issuer and freezer roles
// ─────────────────────────────────────────────────────────────────────────────
#[test]
fn owner_can_set_collection_roles() {
    new_test_ext().execute_with(|| {
        System::set_block_number(1);
        let coll_id = create_default_collection(1);

        let roles = CollectionRoleSet { is_admin: false, is_issuer: true, is_freezer: true };
        assert_ok!(AssetTokenization::set_collection_roles(
            RuntimeOrigin::signed(1),
            coll_id,
            2u64,
            roles.clone(),
        ));

        assert_eq!(CollectionRoles::<Test>::get(coll_id, 2u64), Some(roles));
    });
}

// ─────────────────────────────────────────────────────────────────────────────
// 12. Admin can assign issuer/freezer but NOT the admin role
// ─────────────────────────────────────────────────────────────────────────────
#[test]
fn admin_cannot_grant_admin_role() {
    new_test_ext().execute_with(|| {
        System::set_block_number(1);
        let coll_id = create_default_collection(1);

        // Owner makes account 2 an admin.
        assert_ok!(AssetTokenization::set_collection_roles(
            RuntimeOrigin::signed(1),
            coll_id,
            2u64,
            CollectionRoleSet { is_admin: true, is_issuer: false, is_freezer: false },
        ));

        // Admin (2) can grant issuer to account 3.
        assert_ok!(AssetTokenization::set_collection_roles(
            RuntimeOrigin::signed(2),
            coll_id,
            3u64,
            CollectionRoleSet { is_admin: false, is_issuer: true, is_freezer: false },
        ));

        // Admin (2) cannot grant admin role to account 3.
        assert_noop!(
            AssetTokenization::set_collection_roles(
                RuntimeOrigin::signed(2),
                coll_id,
                3u64,
                CollectionRoleSet { is_admin: true, is_issuer: true, is_freezer: false },
            ),
            Error::<Test>::NotAuthorized
        );
    });
}

// ─────────────────────────────────────────────────────────────────────────────
// 13. Non-admin/owner cannot set roles
// ─────────────────────────────────────────────────────────────────────────────
#[test]
fn non_admin_cannot_set_roles() {
    new_test_ext().execute_with(|| {
        System::set_block_number(1);
        let coll_id = create_default_collection(1);

        assert_noop!(
            AssetTokenization::set_collection_roles(
                RuntimeOrigin::signed(99),
                coll_id,
                2u64,
                CollectionRoleSet { is_admin: false, is_issuer: true, is_freezer: false },
            ),
            Error::<Test>::NotAuthorized
        );
    });
}

// ─────────────────────────────────────────────────────────────────────────────
// 14. Minting into collection requires issuer role
// ─────────────────────────────────────────────────────────────────────────────
#[test]
fn mint_into_collection_requires_issuer() {
    new_test_ext().execute_with(|| {
        System::set_block_number(1);
        let coll_id = create_default_collection(1);

        let name: BoundedVec<u8, frame_support::traits::ConstU32<64>> =
            BoundedVec::try_from(b"Issued Asset".to_vec()).unwrap();
        let uri: BoundedVec<u8, frame_support::traits::ConstU32<256>> =
            BoundedVec::try_from(b"ipfs://Qm456".to_vec()).unwrap();

        // Account 2 has no role — should fail.
        assert_noop!(
            AssetTokenization::mint_asset(
                RuntimeOrigin::signed(2),
                name.clone(),
                AssetType::Digital,
                uri.clone(),
                valid_hash(),
                false,
                None,
                Some(coll_id),
            ),
            Error::<Test>::NotAuthorized
        );

        // Grant issuer role to account 2.
        assert_ok!(AssetTokenization::set_collection_roles(
            RuntimeOrigin::signed(1),
            coll_id,
            2u64,
            CollectionRoleSet { is_admin: false, is_issuer: true, is_freezer: false },
        ));

        // Now account 2 can mint.
        assert_ok!(AssetTokenization::mint_asset(
            RuntimeOrigin::signed(2),
            name,
            AssetType::Digital,
            uri,
            valid_hash(),
            false,
            None,
            Some(coll_id),
        ));

        assert_eq!(AssetCollection::<Test>::get(0u64), Some(coll_id));
    });
}

// ─────────────────────────────────────────────────────────────────────────────
// 15. Collection freezer can freeze an asset in that collection
// ─────────────────────────────────────────────────────────────────────────────
#[test]
fn collection_freezer_can_freeze_asset() {
    new_test_ext().execute_with(|| {
        System::set_block_number(1);
        let coll_id = create_default_collection(1); // owner = 1

        // Mint as owner into the collection.
        let name: BoundedVec<u8, frame_support::traits::ConstU32<64>> =
            BoundedVec::try_from(b"Freezable Asset".to_vec()).unwrap();
        let uri: BoundedVec<u8, frame_support::traits::ConstU32<256>> =
            BoundedVec::try_from(b"ipfs://QmFreeze".to_vec()).unwrap();
        assert_ok!(AssetTokenization::mint_asset(
            RuntimeOrigin::signed(1),
            name,
            AssetType::Physical,
            uri,
            valid_hash(),
            false,
            None,
            Some(coll_id),
        ));

        // Account 3 has no role — cannot freeze.
        assert_noop!(
            AssetTokenization::freeze_asset(RuntimeOrigin::signed(3), 0u64),
            Error::<Test>::NotAssetOwner
        );

        // Grant freezer role to account 3.
        assert_ok!(AssetTokenization::set_collection_roles(
            RuntimeOrigin::signed(1),
            coll_id,
            3u64,
            CollectionRoleSet { is_admin: false, is_issuer: false, is_freezer: true },
        ));

        // Now account 3 can freeze.
        assert_ok!(AssetTokenization::freeze_asset(RuntimeOrigin::signed(3), 0u64));
        assert!(FrozenAssets::<Test>::get(0u64));
    });
}

// =============================================================================
// Contract history tests
// =============================================================================

// ─────────────────────────────────────────────────────────────────────────────
// 16. update_contract archives previous hash and increments version
// ─────────────────────────────────────────────────────────────────────────────
#[test]
fn update_contract_records_history() {
    new_test_ext().execute_with(|| {
        System::set_block_number(1);
        let asset_id = mint_default(1);

        let new_uri: BoundedVec<u8, frame_support::traits::ConstU32<256>> =
            BoundedVec::try_from(b"ipfs://v2".to_vec()).unwrap();
        let new_hash = [2u8; 32];

        assert_ok!(AssetTokenization::update_contract(
            RuntimeOrigin::signed(1),
            asset_id,
            new_uri.clone(),
            new_hash,
        ));

        // Live hash updated.
        let info = Assets::<Test>::get(asset_id).unwrap();
        assert_eq!(info.contract_hash, new_hash);

        // Original hash archived at history index 0.
        assert_eq!(ContractHistory::<Test>::get(asset_id, 0u32), Some(valid_hash()));
        assert_eq!(ContractHistoryCount::<Test>::get(asset_id), 1u32);

        // Second update.
        let third_hash = [3u8; 32];
        let uri3: BoundedVec<u8, frame_support::traits::ConstU32<256>> =
            BoundedVec::try_from(b"ipfs://v3".to_vec()).unwrap();
        assert_ok!(AssetTokenization::update_contract(
            RuntimeOrigin::signed(1),
            asset_id,
            uri3,
            third_hash,
        ));

        // Second hash archived at index 1.
        assert_eq!(ContractHistory::<Test>::get(asset_id, 1u32), Some(new_hash));
        assert_eq!(ContractHistoryCount::<Test>::get(asset_id), 2u32);
    });
}

// =============================================================================
// Fungible token tests
// =============================================================================

// ─────────────────────────────────────────────────────────────────────────────
// 17. Fungible supply is credited to the minter on mint
// ─────────────────────────────────────────────────────────────────────────────
#[test]
fn fungible_supply_credited_on_mint() {
    new_test_ext().execute_with(|| {
        System::set_block_number(1);
        let supply: u128 = 1_000_000;
        let name: BoundedVec<u8, frame_support::traits::ConstU32<64>> =
            BoundedVec::try_from(b"Fungible Token".to_vec()).unwrap();
        let uri: BoundedVec<u8, frame_support::traits::ConstU32<256>> =
            BoundedVec::try_from(b"ipfs://QmFungible".to_vec()).unwrap();

        assert_ok!(AssetTokenization::mint_asset(
            RuntimeOrigin::signed(1),
            name,
            AssetType::Digital,
            uri,
            valid_hash(),
            true,
            Some(supply),
            None,
        ));

        assert_eq!(FungibleBalances::<Test>::get(0u64, 1u64), supply);
    });
}

// ─────────────────────────────────────────────────────────────────────────────
// 18. transfer_fungible moves the correct amount
// ─────────────────────────────────────────────────────────────────────────────
#[test]
fn transfer_fungible_works() {
    new_test_ext().execute_with(|| {
        System::set_block_number(1);
        let supply: u128 = 1_000_000;
        let name: BoundedVec<u8, frame_support::traits::ConstU32<64>> =
            BoundedVec::try_from(b"Fungible Token".to_vec()).unwrap();
        let uri: BoundedVec<u8, frame_support::traits::ConstU32<256>> =
            BoundedVec::try_from(b"ipfs://QmFungible".to_vec()).unwrap();

        assert_ok!(AssetTokenization::mint_asset(
            RuntimeOrigin::signed(1),
            name,
            AssetType::Digital,
            uri,
            valid_hash(),
            true,
            Some(supply),
            None,
        ));

        assert_ok!(AssetTokenization::transfer_fungible(
            RuntimeOrigin::signed(1),
            0u64,
            2u64,
            400_000u128,
        ));

        assert_eq!(FungibleBalances::<Test>::get(0u64, 1u64), 600_000u128);
        assert_eq!(FungibleBalances::<Test>::get(0u64, 2u64), 400_000u128);

        System::assert_last_event(
            Event::FungibleTransferred { asset_id: 0, from: 1, to: 2, amount: 400_000 }.into(),
        );
    });
}

// ─────────────────────────────────────────────────────────────────────────────
// 19. Transferring full balance removes the storage entry
// ─────────────────────────────────────────────────────────────────────────────
#[test]
fn transfer_fungible_full_balance_cleans_storage() {
    new_test_ext().execute_with(|| {
        System::set_block_number(1);
        let supply: u128 = 500;
        let name: BoundedVec<u8, frame_support::traits::ConstU32<64>> =
            BoundedVec::try_from(b"Token".to_vec()).unwrap();
        let uri: BoundedVec<u8, frame_support::traits::ConstU32<256>> =
            BoundedVec::try_from(b"ipfs://QmT".to_vec()).unwrap();

        assert_ok!(AssetTokenization::mint_asset(
            RuntimeOrigin::signed(1),
            name,
            AssetType::Digital,
            uri,
            valid_hash(),
            true,
            Some(supply),
            None,
        ));

        assert_ok!(AssetTokenization::transfer_fungible(
            RuntimeOrigin::signed(1),
            0u64,
            2u64,
            supply,
        ));

        // Zero-balance entry should be removed.
        assert_eq!(FungibleBalances::<Test>::get(0u64, 1u64), 0u128);
        assert_eq!(FungibleBalances::<Test>::get(0u64, 2u64), supply);
    });
}

// ─────────────────────────────────────────────────────────────────────────────
// 20. Insufficient balance is rejected
// ─────────────────────────────────────────────────────────────────────────────
#[test]
fn transfer_fungible_insufficient_balance_fails() {
    new_test_ext().execute_with(|| {
        System::set_block_number(1);
        let name: BoundedVec<u8, frame_support::traits::ConstU32<64>> =
            BoundedVec::try_from(b"Fungible Token".to_vec()).unwrap();
        let uri: BoundedVec<u8, frame_support::traits::ConstU32<256>> =
            BoundedVec::try_from(b"ipfs://QmFungible".to_vec()).unwrap();

        assert_ok!(AssetTokenization::mint_asset(
            RuntimeOrigin::signed(1),
            name,
            AssetType::Digital,
            uri,
            valid_hash(),
            true,
            Some(1_000u128),
            None,
        ));

        assert_noop!(
            AssetTokenization::transfer_fungible(
                RuntimeOrigin::signed(1),
                0u64,
                2u64,
                9_999u128, // more than supply
            ),
            Error::<Test>::InsufficientFungibleBalance
        );
    });
}

// ─────────────────────────────────────────────────────────────────────────────
// 21. transfer_fungible fails on a non-fungible asset
// ─────────────────────────────────────────────────────────────────────────────
#[test]
fn transfer_fungible_non_fungible_asset_fails() {
    new_test_ext().execute_with(|| {
        System::set_block_number(1);
        let asset_id = mint_default(1); // non-fungible

        assert_noop!(
            AssetTokenization::transfer_fungible(
                RuntimeOrigin::signed(1),
                asset_id,
                2u64,
                100u128,
            ),
            Error::<Test>::AssetNotFungible
        );
    });
}

// ─────────────────────────────────────────────────────────────────────────────
// 22. Frozen asset blocks fungible transfer
// ─────────────────────────────────────────────────────────────────────────────
#[test]
fn frozen_asset_blocks_fungible_transfer() {
    new_test_ext().execute_with(|| {
        System::set_block_number(1);
        let supply = 1_000u128;
        let name: BoundedVec<u8, frame_support::traits::ConstU32<64>> =
            BoundedVec::try_from(b"Locked Token".to_vec()).unwrap();
        let uri: BoundedVec<u8, frame_support::traits::ConstU32<256>> =
            BoundedVec::try_from(b"ipfs://QmLocked".to_vec()).unwrap();

        assert_ok!(AssetTokenization::mint_asset(
            RuntimeOrigin::signed(1),
            name,
            AssetType::Digital,
            uri,
            valid_hash(),
            true,
            Some(supply),
            None,
        ));

        assert_ok!(AssetTokenization::freeze_asset(RuntimeOrigin::signed(1), 0u64));

        assert_noop!(
            AssetTokenization::transfer_fungible(
                RuntimeOrigin::signed(1),
                0u64,
                2u64,
                100u128,
            ),
            Error::<Test>::AssetIsFrozen
        );
    });
}

// ─────────────────────────────────────────────────────────────────────────────
// 23. Clearing all roles removes the storage entry
// ─────────────────────────────────────────────────────────────────────────────
#[test]
fn clearing_roles_removes_storage_entry() {
    new_test_ext().execute_with(|| {
        System::set_block_number(1);
        let coll_id = create_default_collection(1);

        assert_ok!(AssetTokenization::set_collection_roles(
            RuntimeOrigin::signed(1),
            coll_id,
            2u64,
            CollectionRoleSet { is_admin: false, is_issuer: true, is_freezer: false },
        ));
        assert!(CollectionRoles::<Test>::contains_key(coll_id, 2u64));

        // Revoke all roles.
        assert_ok!(AssetTokenization::set_collection_roles(
            RuntimeOrigin::signed(1),
            coll_id,
            2u64,
            CollectionRoleSet::default(),
        ));
        assert!(!CollectionRoles::<Test>::contains_key(coll_id, 2u64));
    });
}

// ═════════════════════════════════════════════════════════════════════════════
// Collection Freezing Tests (to be added to tests.rs)
// ═════════════════════════════════════════════════════════════════════════════

// ─────────────────────────────────────────────────────────────────────────────
// 24. freeze_collection works and prevents minting
// ─────────────────────────────────────────────────────────────────────────────
#[test]
fn freeze_collection_works() {
    new_test_ext().execute_with(|| {
        System::set_block_number(1);
        let coll_id = create_default_collection(1);

        assert_ok!(AssetTokenization::freeze_collection(
            RuntimeOrigin::signed(1),
            coll_id,
        ));

        let info = Collections::<Test>::get(coll_id).unwrap();
        assert!(info.is_frozen);

        System::assert_last_event(
            Event::CollectionFrozen { collection_id: coll_id }.into(),
        );
    });
}

// ─────────────────────────────────────────────────────────────────────────────
// 25. Frozen collection prevents minting
// ─────────────────────────────────────────────────────────────────────────────
#[test]
fn frozen_collection_prevents_minting() {
    new_test_ext().execute_with(|| {
        System::set_block_number(1);
        let coll_id = create_default_collection(1);

        // Freeze the collection
        assert_ok!(AssetTokenization::freeze_collection(
            RuntimeOrigin::signed(1),
            coll_id,
        ));

        // Try to mint - should fail
        let name: BoundedVec<u8, frame_support::traits::ConstU32<64>> =
            BoundedVec::try_from(b"Asset".to_vec()).unwrap();
        let uri: BoundedVec<u8, frame_support::traits::ConstU32<256>> =
            BoundedVec::try_from(b"ipfs://QmTest".to_vec()).unwrap();

        assert_noop!(
            AssetTokenization::mint_asset(
                RuntimeOrigin::signed(1),
                name,
                AssetType::Digital,
                uri,
                valid_hash(),
                false,
                None,
                Some(coll_id),
            ),
            Error::<Test>::CollectionIsFrozen
        );
    });
}

// ─────────────────────────────────────────────────────────────────────────────
// 26. Only owner or admin can freeze collection
// ─────────────────────────────────────────────────────────────────────────────
#[test]
fn only_owner_or_admin_can_freeze_collection() {
    new_test_ext().execute_with(|| {
        System::set_block_number(1);
        let coll_id = create_default_collection(1);

        // Random account cannot freeze
        assert_noop!(
            AssetTokenization::freeze_collection(RuntimeOrigin::signed(99), coll_id),
            Error::<Test>::NotAuthorized
        );

        // Grant admin role to account 2
        assert_ok!(AssetTokenization::set_collection_roles(
            RuntimeOrigin::signed(1),
            coll_id,
            2u64,
            CollectionRoleSet { is_admin: true, is_issuer: false, is_freezer: false },
        ));

        // Admin can freeze
        assert_ok!(AssetTokenization::freeze_collection(
            RuntimeOrigin::signed(2),
            coll_id,
        ));

        let info = Collections::<Test>::get(coll_id).unwrap();
        assert!(info.is_frozen);
    });
}

// ─────────────────────────────────────────────────────────────────────────────
// 27. Cannot freeze already frozen collection
// ─────────────────────────────────────────────────────────────────────────────
#[test]
fn cannot_freeze_already_frozen_collection() {
    new_test_ext().execute_with(|| {
        System::set_block_number(1);
        let coll_id = create_default_collection(1);

        assert_ok!(AssetTokenization::freeze_collection(
            RuntimeOrigin::signed(1),
            coll_id,
        ));

        // Try to freeze again - should fail
        assert_noop!(
            AssetTokenization::freeze_collection(RuntimeOrigin::signed(1), coll_id),
            Error::<Test>::CollectionIsFrozen
        );
    });
}

// ─────────────────────────────────────────────────────────────────────────────
// 28. Issuer and freezer roles cannot freeze collection
// ─────────────────────────────────────────────────────────────────────────────
#[test]
fn issuer_freezer_roles_cannot_freeze_collection() {
    new_test_ext().execute_with(|| {
        System::set_block_number(1);
        let coll_id = create_default_collection(1);

        // Grant issuer role to account 2
        assert_ok!(AssetTokenization::set_collection_roles(
            RuntimeOrigin::signed(1),
            coll_id,
            2u64,
            CollectionRoleSet { is_admin: false, is_issuer: true, is_freezer: false },
        ));

        // Issuer cannot freeze collection
        assert_noop!(
            AssetTokenization::freeze_collection(RuntimeOrigin::signed(2), coll_id),
            Error::<Test>::NotAuthorized
        );

        // Grant freezer role to account 3
        assert_ok!(AssetTokenization::set_collection_roles(
            RuntimeOrigin::signed(1),
            coll_id,
            3u64,
            CollectionRoleSet { is_admin: false, is_issuer: false, is_freezer: true },
        ));

        // Freezer cannot freeze collection (can only freeze individual assets)
        assert_noop!(
            AssetTokenization::freeze_collection(RuntimeOrigin::signed(3), coll_id),
            Error::<Test>::NotAuthorized
        );
    });
}
