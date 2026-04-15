use crate::{
    mock::*, AssetOwner, Assets, ContractSignatures, Error, Event, FrozenAssets,
    pallet::AssetType,
};
use frame_support::{assert_noop, assert_ok, BoundedVec};

/// Helper: returns a valid 32-byte contract hash (non-zero).
fn valid_hash() -> [u8; 32] {
    [1u8; 32]
}

/// Helper: mint a basic asset as `origin` and return its ID (always 0 for the first call).
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
    ));
    // NextAssetId was 0 before the call, so the minted ID is 0.
    0
}

// ─────────────────────────────────────────────────────────────────────────────
// 1. Successful mint
// ─────────────────────────────────────────────────────────────────────────────
#[test]
fn mint_asset_works() {
    new_test_ext().execute_with(|| {
        System::set_block_number(1);

        let asset_id = mint_default(1);

        // Storage populated correctly.
        assert!(Assets::<Test>::contains_key(asset_id));
        assert_eq!(AssetOwner::<Test>::get(asset_id), Some(1u64));

        // Correct event emitted.
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

        // Signature recorded.
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

        // Owner freezes the asset.
        assert_ok!(AssetTokenization::freeze_asset(RuntimeOrigin::signed(1), asset_id));
        assert!(FrozenAssets::<Test>::get(asset_id));

        // Attempt to update the contract must fail.
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

        // Owner must sign the contract before transferring (design doc requirement).
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

        // Non-owner attempt must fail.
        assert_noop!(
            AssetTokenization::freeze_asset(RuntimeOrigin::signed(99), asset_id),
            Error::<Test>::NotAssetOwner
        );

        // Owner can freeze successfully.
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
// 8. Transfer blocked until owner has signed the contract (Gap 1)
// ─────────────────────────────────────────────────────────────────────────────
#[test]
fn transfer_blocked_until_owner_signs() {
    new_test_ext().execute_with(|| {
        System::set_block_number(1);
        let asset_id = mint_default(1);

        // Attempt to transfer before signing the contract must fail.
        assert_noop!(
            AssetTokenization::transfer_asset(RuntimeOrigin::signed(1), asset_id, 2u64),
            Error::<Test>::ContractNotSigned
        );

        // After the owner signs, the transfer succeeds.
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
// 9. Fungible supply consistency guard (Gap 3)
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
            ),
            Error::<Test>::InconsistentFungibleSupply
        );

        // Consistent cases: fungible with supply, and non-fungible with None.
        assert_ok!(AssetTokenization::mint_asset(
            RuntimeOrigin::signed(1),
            name.clone(),
            AssetType::Digital,
            uri.clone(),
            valid_hash(),
            true,
            Some(1_000_000),
        ));
        assert_ok!(AssetTokenization::mint_asset(
            RuntimeOrigin::signed(1),
            name,
            AssetType::Physical,
            uri,
            valid_hash(),
            false,
            None,
        ));
    });
}
