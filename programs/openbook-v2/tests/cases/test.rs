use super::*;

#[tokio::test]
async fn test_simple_settle() -> Result<(), TransportError> {
    let context = TestContext::new().await;
    let solana = &context.solana.clone();

    let admin = TestKeypair::new();
    let owner = context.users[0].key;
    let payer = context.users[1].key;
    let mints = &context.mints[0..=2];

    let owner_token_0 = context.users[0].token_accounts[0];
    let owner_token_1 = context.users[0].token_accounts[1];

    let initial_token_deposit = 10_000;

    let tokens = Token::create(mints.to_vec(), solana, admin, payer).await;

    //
    // TEST: Create a market
    //

    let market = get_market_address_by_index(1);
    let base_vault = solana
        .create_associated_token_account(&market, mints[0].pubkey)
        .await;
    let quote_vault = solana
        .create_associated_token_account(&market, mints[1].pubkey)
        .await;

    let openbook_v2::accounts::CreateMarket {
        market,
        base_vault,
        quote_vault,
        ..
    } = send_tx(
        solana,
        CreateMarketInstruction {
            admin,
            payer,
            market_index: 1,
            quote_lot_size: 10,
            base_lot_size: 100,
            maker_fee: 0.0002,
            taker_fee: 0.000,
            base_mint: mints[0].pubkey,
            quote_mint: mints[1].pubkey,
            base_vault,
            quote_vault,
            ..CreateMarketInstruction::with_new_book_and_queue(&solana, &tokens[1]).await
        },
    )
    .await
    .unwrap();

    let settler =
        create_funded_account(&solana, owner, market, 251, &context.users[1], &[], 0).await;
    let settler_owner = owner.clone();

    let account_0 = create_funded_account(
        &solana,
        owner,
        market,
        0,
        &context.users[1],
        &mints[0..1],
        initial_token_deposit,
    )
    .await;

    let account_1 = create_funded_account(
        &solana,
        owner,
        market,
        1,
        &context.users[1],
        &mints[0..1],
        initial_token_deposit,
    )
    .await;

    //
    // TEST: Create another market
    //

    let market_2 = get_market_address_by_index(2);
    let base_vault_2 = solana
        .create_associated_token_account(&market_2, mints[0].pubkey)
        .await;
    let quote_vault_2 = solana
        .create_associated_token_account(&market_2, mints[1].pubkey)
        .await;

    let openbook_v2::accounts::CreateMarket {
        market: market_2, ..
    } = send_tx(
        solana,
        CreateMarketInstruction {
            admin,
            payer,
            market_index: 2,
            quote_lot_size: 10,
            base_lot_size: 100,
            maker_fee: 0.0002,
            taker_fee: 0.000,
            base_mint: mints[0].pubkey,
            quote_mint: mints[1].pubkey,
            base_vault: base_vault_2,
            quote_vault: quote_vault_2,
            ..CreateMarketInstruction::with_new_book_and_queue(&solana, &tokens[2]).await
        },
    )
    .await
    .unwrap();

    let price_lots = {
        let market = solana.get_account::<Market>(market).await;
        market.native_price_to_lot(I80F48::from(1000))
    };

    // Set the initial oracle price
    set_stub_oracle_price(solana, market, &tokens[1], admin, 1000.0).await;

    send_tx(
        solana,
        PlaceOrderInstruction {
            open_orders_account: account_0,
            market,
            owner,
            payer: owner_token_1,
            base_vault,
            quote_vault,
            side: Side::Bid,
            price_lots,
            max_base_lots: 1,
            max_quote_lots: 10000,
            reduce_only: false,
            client_order_id: 0,
            expiry_timestamp: 0,
        },
    )
    .await
    .unwrap();

    send_tx(
        solana,
        PlaceOrderInstruction {
            open_orders_account: account_1,
            market,
            owner,
            payer: owner_token_0,
            base_vault,
            quote_vault,
            side: Side::Ask,
            price_lots,
            max_base_lots: 1,
            max_quote_lots: 10000,
            reduce_only: false,
            client_order_id: 0,
            expiry_timestamp: 0,
        },
    )
    .await
    .unwrap();

    {
        let open_orders_account_0 = solana.get_account::<OpenOrdersAccount>(account_0).await;
        let open_orders_account_1 = solana.get_account::<OpenOrdersAccount>(account_1).await;

        assert_eq!(open_orders_account_0.position.base_position_lots(), 0);
        assert_eq!(open_orders_account_1.position.base_position_lots(), 0);
        assert_eq!(
            open_orders_account_0
                .position
                .quote_position_native()
                .round(),
            0
        );
        assert_eq!(open_orders_account_1.position.quote_position_native(), 0);
        assert_eq!(open_orders_account_0.position.bids_base_lots, 1);
        assert_eq!(open_orders_account_1.position.bids_base_lots, 0);
        assert_eq!(open_orders_account_0.position.asks_base_lots, 0);
        assert_eq!(open_orders_account_1.position.asks_base_lots, 0);
        assert_eq!(open_orders_account_0.position.taker_base_lots, 0);
        assert_eq!(open_orders_account_1.position.taker_quote_lots, 10000);
        assert_eq!(open_orders_account_0.position.base_free_lots, 0);
        assert_eq!(open_orders_account_1.position.base_free_lots, 0);
        assert_eq!(open_orders_account_0.position.quote_free_lots, 0);
        assert_eq!(open_orders_account_1.position.quote_free_lots, 0);
    }

    send_tx(
        solana,
        ConsumeEventsInstruction {
            market,
            open_orders_accounts: vec![account_0, account_1],
        },
    )
    .await
    .unwrap();

    {
        let open_orders_account_0 = solana.get_account::<OpenOrdersAccount>(account_0).await;
        let open_orders_account_1 = solana.get_account::<OpenOrdersAccount>(account_1).await;

        assert_eq!(open_orders_account_0.position.base_position_lots(), 1);
        assert_eq!(open_orders_account_1.position.base_position_lots(), -1);
        assert_eq!(
            open_orders_account_0
                .position
                .quote_position_native()
                .round(),
            -100_020
        );
        assert_eq!(
            open_orders_account_1.position.quote_position_native(),
            100_000
        );
        assert_eq!(open_orders_account_0.position.bids_base_lots, 0);
        assert_eq!(open_orders_account_1.position.bids_base_lots, 0);
        assert_eq!(open_orders_account_0.position.asks_base_lots, 0);
        assert_eq!(open_orders_account_1.position.asks_base_lots, 0);
        assert_eq!(open_orders_account_0.position.taker_base_lots, 0);
        assert_eq!(open_orders_account_1.position.taker_quote_lots, 0);
        assert_eq!(open_orders_account_0.position.base_free_lots, 1);
        assert_eq!(open_orders_account_1.position.base_free_lots, 0);
        assert_eq!(open_orders_account_0.position.quote_free_lots, 0);
        assert_eq!(open_orders_account_1.position.quote_free_lots, 10000);
    }

    send_tx(
        solana,
        SettleFundsInstruction {
            market,
            open_orders_account: account_0,
            base_vault,
            quote_vault,
            payer_base: owner_token_0,
            payer_quote: owner_token_1,
        },
    )
    .await
    .unwrap();

    {
        let open_orders_account_0 = solana.get_account::<OpenOrdersAccount>(account_0).await;
        let open_orders_account_1 = solana.get_account::<OpenOrdersAccount>(account_1).await;

        assert_eq!(open_orders_account_0.position.base_free_lots, 0);
        assert_eq!(open_orders_account_1.position.base_free_lots, 0);
        assert_eq!(open_orders_account_0.position.quote_free_lots, 0);
        assert_eq!(open_orders_account_1.position.quote_free_lots, 10000);
    }

    send_tx(
        solana,
        SettleFundsInstruction {
            market,
            open_orders_account: account_1,
            base_vault,
            quote_vault,
            payer_base: owner_token_0,
            payer_quote: owner_token_1,
        },
    )
    .await
    .unwrap();

    {
        let open_orders_account_0 = solana.get_account::<OpenOrdersAccount>(account_0).await;
        let open_orders_account_1 = solana.get_account::<OpenOrdersAccount>(account_1).await;

        assert_eq!(open_orders_account_0.position.base_free_lots, 0);
        assert_eq!(open_orders_account_1.position.base_free_lots, 0);
        assert_eq!(open_orders_account_0.position.quote_free_lots, 0);
        assert_eq!(open_orders_account_1.position.quote_free_lots, 0);
    }

    Ok(())
}

#[tokio::test]
async fn test_cancel_orders() -> Result<(), TransportError> {
    let context = TestContext::new().await;
    let solana = &context.solana.clone();

    let admin = TestKeypair::new();
    let owner = context.users[0].key;
    let payer = context.users[1].key;
    let mints = &context.mints[0..=2];

    let owner_token_0 = context.users[0].token_accounts[0];
    let owner_token_1 = context.users[0].token_accounts[1];

    let initial_token_deposit = 10_000;

    let tokens = Token::create(mints.to_vec(), solana, admin, payer).await;

    //
    // TEST: Create a market
    //

    let market = get_market_address_by_index(1);
    let base_vault = solana
        .create_associated_token_account(&market, mints[0].pubkey)
        .await;
    let quote_vault = solana
        .create_associated_token_account(&market, mints[1].pubkey)
        .await;

    let openbook_v2::accounts::CreateMarket {
        market,
        base_vault,
        quote_vault,
        ..
    } = send_tx(
        solana,
        CreateMarketInstruction {
            admin,
            payer,
            market_index: 1,
            quote_lot_size: 10,
            base_lot_size: 100,
            maker_fee: 0.0002,
            taker_fee: 0.000,
            base_mint: mints[0].pubkey,
            quote_mint: mints[1].pubkey,
            base_vault,
            quote_vault,
            ..CreateMarketInstruction::with_new_book_and_queue(&solana, &tokens[1]).await
        },
    )
    .await
    .unwrap();

    set_stub_oracle_price(solana, market, &tokens[1], admin, 1000.0).await;

    let settler =
        create_funded_account(&solana, owner, market, 251, &context.users[1], &[], 0).await;
    let settler_owner = owner.clone();

    let account_0 = create_funded_account(
        &solana,
        owner,
        market,
        0,
        &context.users[1],
        &mints[0..1],
        initial_token_deposit,
    )
    .await;

    let account_1 = create_funded_account(
        &solana,
        owner,
        market,
        1,
        &context.users[1],
        &mints[0..1],
        initial_token_deposit,
    )
    .await;

    let price_lots = {
        let market = solana.get_account::<Market>(market).await;
        market.native_price_to_lot(I80F48::from(1000))
    };

    send_tx(
        solana,
        PlaceOrderInstruction {
            open_orders_account: account_0,
            market,
            owner,
            payer: owner_token_1,
            base_vault,
            quote_vault,
            side: Side::Bid,
            price_lots,
            max_base_lots: 1,
            max_quote_lots: 10000,
            reduce_only: false,
            client_order_id: 0,
            expiry_timestamp: 0,
        },
    )
    .await
    .unwrap();

    send_tx(
        solana,
        PlaceOrderInstruction {
            open_orders_account: account_1,
            market,
            owner,
            payer: owner_token_0,
            base_vault,
            quote_vault,
            side: Side::Ask,
            price_lots,
            max_base_lots: 1,
            max_quote_lots: 10000,
            reduce_only: false,
            client_order_id: 0,
            expiry_timestamp: 0,
        },
    )
    .await
    .unwrap();

    {
        let open_orders_account_0 = solana.get_account::<OpenOrdersAccount>(account_0).await;
        let open_orders_account_1 = solana.get_account::<OpenOrdersAccount>(account_1).await;

        assert_eq!(open_orders_account_0.position.base_position_lots(), 0);
        assert_eq!(open_orders_account_1.position.base_position_lots(), 0);
        assert_eq!(
            open_orders_account_0
                .position
                .quote_position_native()
                .round(),
            0
        );
        assert_eq!(open_orders_account_1.position.quote_position_native(), 0);
        assert_eq!(open_orders_account_0.position.bids_base_lots, 1);
        assert_eq!(open_orders_account_1.position.bids_base_lots, 0);
        assert_eq!(open_orders_account_0.position.asks_base_lots, 0);
        assert_eq!(open_orders_account_1.position.asks_base_lots, 0);
        assert_eq!(open_orders_account_0.position.taker_base_lots, 0);
        assert_eq!(open_orders_account_1.position.taker_quote_lots, 10000);
        assert_eq!(open_orders_account_0.position.base_free_lots, 0);
        assert_eq!(open_orders_account_1.position.base_free_lots, 0);
        assert_eq!(open_orders_account_0.position.quote_free_lots, 0);
        assert_eq!(open_orders_account_1.position.quote_free_lots, 0);
    }

    send_tx(
        solana,
        ConsumeEventsInstruction {
            market,
            open_orders_accounts: vec![account_0, account_1],
        },
    )
    .await
    .unwrap();

    {
        let open_orders_account_0 = solana.get_account::<OpenOrdersAccount>(account_0).await;
        let open_orders_account_1 = solana.get_account::<OpenOrdersAccount>(account_1).await;

        assert_eq!(open_orders_account_0.position.base_position_lots(), 1);
        assert_eq!(open_orders_account_1.position.base_position_lots(), -1);
        assert_eq!(
            open_orders_account_0
                .position
                .quote_position_native()
                .round(),
            -100_020
        );
        assert_eq!(
            open_orders_account_1.position.quote_position_native(),
            100_000
        );
        assert_eq!(open_orders_account_0.position.bids_base_lots, 0);
        assert_eq!(open_orders_account_1.position.bids_base_lots, 0);
        assert_eq!(open_orders_account_0.position.asks_base_lots, 0);
        assert_eq!(open_orders_account_1.position.asks_base_lots, 0);
        assert_eq!(open_orders_account_0.position.taker_base_lots, 0);
        assert_eq!(open_orders_account_1.position.taker_quote_lots, 0);
        assert_eq!(open_orders_account_0.position.base_free_lots, 1);
        assert_eq!(open_orders_account_1.position.base_free_lots, 0);
        assert_eq!(open_orders_account_0.position.quote_free_lots, 0);
        assert_eq!(open_orders_account_1.position.quote_free_lots, 10000);
    }

    send_tx(
        solana,
        DepositInstruction {
            owner,
            market,
            open_orders_account: account_0,
            base_vault,
            quote_vault,
            payer_base: owner_token_0,
            payer_quote: owner_token_1,
            base_amount_lots: 100,
            quote_amount_lots: 0,
        },
    )
    .await
    .unwrap();

    {
        let open_orders_account_0 = solana.get_account::<OpenOrdersAccount>(account_0).await;
        let open_orders_account_1 = solana.get_account::<OpenOrdersAccount>(account_1).await;

        assert_eq!(open_orders_account_0.position.base_free_lots, 101);
        assert_eq!(open_orders_account_0.position.quote_free_lots, 0);
    }

    let balance = solana.token_account_balance(owner_token_0).await;

    // Assets should be free, let's post the opposite orders
    send_tx(
        solana,
        PlaceOrderInstruction {
            open_orders_account: account_0,
            market,
            owner,
            payer: owner_token_0,
            base_vault,
            quote_vault,
            side: Side::Ask,
            price_lots,
            max_base_lots: 1,
            max_quote_lots: 10000,
            reduce_only: false,
            client_order_id: 0,
            expiry_timestamp: 0,
        },
    )
    .await
    .unwrap();

    {
        assert_eq!(balance, solana.token_account_balance(owner_token_0).await);
        let open_orders_account_0 = solana.get_account::<OpenOrdersAccount>(account_0).await;
        let open_orders_account_1 = solana.get_account::<OpenOrdersAccount>(account_1).await;

        assert_eq!(open_orders_account_0.position.base_position_lots(), 1);
        assert_eq!(open_orders_account_1.position.base_position_lots(), -1);
        assert_eq!(
            open_orders_account_0
                .position
                .quote_position_native()
                .round(),
            -100_020
        );
        assert_eq!(
            open_orders_account_1.position.quote_position_native(),
            100_000
        );
        assert_eq!(open_orders_account_0.position.bids_base_lots, 0);
        assert_eq!(open_orders_account_1.position.bids_base_lots, 0);
        assert_eq!(open_orders_account_0.position.asks_base_lots, 1);
        assert_eq!(open_orders_account_1.position.asks_base_lots, 0);
        assert_eq!(open_orders_account_0.position.taker_base_lots, 0);
        assert_eq!(open_orders_account_1.position.taker_quote_lots, 0);
        assert_eq!(open_orders_account_0.position.base_free_lots, 100);
        assert_eq!(open_orders_account_1.position.base_free_lots, 0);
        assert_eq!(open_orders_account_0.position.quote_free_lots, 0);
        assert_eq!(open_orders_account_1.position.quote_free_lots, 10000);
    }

    send_tx(
        solana,
        SettleFundsInstruction {
            market,
            open_orders_account: account_0,
            base_vault,
            quote_vault,
            payer_base: owner_token_0,
            payer_quote: owner_token_1,
        },
    )
    .await
    .unwrap();

    {
        let open_orders_account_0 = solana.get_account::<OpenOrdersAccount>(account_0).await;
        let open_orders_account_1 = solana.get_account::<OpenOrdersAccount>(account_1).await;

        assert_eq!(open_orders_account_0.position.asks_base_lots, 1);
        assert_eq!(open_orders_account_0.position.base_free_lots, 0);
        assert_eq!(open_orders_account_1.position.base_free_lots, 0);
        assert_eq!(open_orders_account_0.position.quote_free_lots, 0);
        assert_eq!(open_orders_account_1.position.quote_free_lots, 10000);
    }

    let order_id_to_cancel = solana
        .get_account::<OpenOrdersAccount>(account_0)
        .await
        .open_orders[0]
        .id;

    send_tx(
        solana,
        CancelOrderInstruction {
            owner,
            market,
            open_orders_account: account_0,
            order_id: order_id_to_cancel,
        },
    )
    .await
    .unwrap();

    {
        let open_orders_account_0 = solana.get_account::<OpenOrdersAccount>(account_0).await;
        let open_orders_account_1 = solana.get_account::<OpenOrdersAccount>(account_1).await;

        assert_eq!(open_orders_account_0.position.asks_base_lots, 0);
        assert_eq!(open_orders_account_0.position.base_free_lots, 1);
        assert_eq!(open_orders_account_0.position.quote_free_lots, 0);
    }

    // Post and cancel Bid
    send_tx(
        solana,
        PlaceOrderInstruction {
            open_orders_account: account_0,
            market,
            owner,
            payer: owner_token_1,
            base_vault,
            quote_vault,
            side: Side::Bid,
            price_lots,
            max_base_lots: 1,
            max_quote_lots: 10000,
            reduce_only: false,
            client_order_id: 0,
            expiry_timestamp: 0,
        },
    )
    .await
    .unwrap();

    {
        let open_orders_account_0 = solana.get_account::<OpenOrdersAccount>(account_0).await;
        let open_orders_account_1 = solana.get_account::<OpenOrdersAccount>(account_1).await;

        assert_eq!(open_orders_account_0.position.bids_base_lots, 1);
        assert_eq!(open_orders_account_0.position.asks_base_lots, 0);
        assert_eq!(open_orders_account_0.position.base_free_lots, 1);
        assert_eq!(open_orders_account_0.position.quote_free_lots, 0);
    }

    let order_id_to_cancel = solana
        .get_account::<OpenOrdersAccount>(account_0)
        .await
        .open_orders[0]
        .id;

    send_tx(
        solana,
        CancelOrderInstruction {
            owner,
            market,
            open_orders_account: account_0,
            order_id: order_id_to_cancel,
        },
    )
    .await
    .unwrap();

    {
        let open_orders_account_0 = solana.get_account::<OpenOrdersAccount>(account_0).await;
        let open_orders_account_1 = solana.get_account::<OpenOrdersAccount>(account_1).await;

        assert_eq!(open_orders_account_0.position.base_free_lots, 1);
        assert_eq!(open_orders_account_0.position.quote_free_lots, 10000);
    }

    Ok(())
}

#[tokio::test]
async fn test_expired_orders() -> Result<(), TransportError> {
    let context = TestContext::new().await;
    let solana = &context.solana.clone();

    let admin = TestKeypair::new();
    let owner = context.users[0].key;
    let payer = context.users[1].key;
    let mints = &context.mints[0..=2];

    let owner_token_0 = context.users[0].token_accounts[0];
    let owner_token_1 = context.users[0].token_accounts[1];

    let initial_token_deposit = 10_000;

    let tokens = Token::create(mints.to_vec(), solana, admin, payer).await;

    //
    // TEST: Create a market
    //

    let market = get_market_address_by_index(1);
    let base_vault = solana
        .create_associated_token_account(&market, mints[0].pubkey)
        .await;
    let quote_vault = solana
        .create_associated_token_account(&market, mints[1].pubkey)
        .await;

    let openbook_v2::accounts::CreateMarket {
        market,
        base_vault,
        quote_vault,
        ..
    } = send_tx(
        solana,
        CreateMarketInstruction {
            admin,
            payer,
            market_index: 1,
            quote_lot_size: 10,
            base_lot_size: 100,
            maker_fee: 0.0002,
            taker_fee: 0.000,
            base_mint: mints[0].pubkey,
            quote_mint: mints[1].pubkey,
            base_vault,
            quote_vault,
            ..CreateMarketInstruction::with_new_book_and_queue(&solana, &tokens[1]).await
        },
    )
    .await
    .unwrap();

    set_stub_oracle_price(solana, market, &tokens[1], admin, 1000.0).await;

    let settler =
        create_funded_account(&solana, owner, market, 251, &context.users[1], &[], 0).await;
    let settler_owner = owner.clone();

    let account_0 = create_funded_account(
        &solana,
        owner,
        market,
        0,
        &context.users[1],
        &mints[0..1],
        initial_token_deposit,
    )
    .await;

    let account_1 = create_funded_account(
        &solana,
        owner,
        market,
        1,
        &context.users[1],
        &mints[0..1],
        initial_token_deposit,
    )
    .await;

    let price_lots = {
        let market = solana.get_account::<Market>(market).await;
        market.native_price_to_lot(I80F48::from(1000))
    };

    // Order with expiry time of 2s
    let now_ts: u64 = solana.get_clock().await.unix_timestamp as u64;
    send_tx(
        solana,
        PlaceOrderInstruction {
            open_orders_account: account_0,
            market,
            owner,
            payer: owner_token_1,
            base_vault,
            quote_vault,
            side: Side::Bid,
            price_lots,
            max_base_lots: 1,
            max_quote_lots: 10000,
            reduce_only: false,
            client_order_id: 0,
            expiry_timestamp: now_ts + 2,
        },
    )
    .await
    .unwrap();

    {
        let open_orders_account_0 = solana.get_account::<OpenOrdersAccount>(account_0).await;

        assert_eq!(open_orders_account_0.position.base_position_lots(), 0);
        assert_eq!(
            open_orders_account_0
                .position
                .quote_position_native()
                .round(),
            0
        );
        assert_eq!(open_orders_account_0.position.bids_base_lots, 1);
        assert_eq!(open_orders_account_0.position.asks_base_lots, 0);
        assert_eq!(open_orders_account_0.position.taker_base_lots, 0);
        assert_eq!(open_orders_account_0.position.base_free_lots, 0);
        assert_eq!(open_orders_account_0.position.quote_free_lots, 0);
    }

    // Advance clock
    solana.advance_clock(2).await;
    // Bid isn't available anymore, shouldn't be matched
    send_tx(
        solana,
        PlaceOrderInstruction {
            open_orders_account: account_1,
            market,
            owner,
            payer: owner_token_0,
            base_vault,
            quote_vault,
            side: Side::Ask,
            price_lots,
            max_base_lots: 1,
            max_quote_lots: 10000,
            reduce_only: false,
            client_order_id: 0,
            expiry_timestamp: 0,
        },
    )
    .await
    .unwrap();

    {
        let open_orders_account_0 = solana.get_account::<OpenOrdersAccount>(account_0).await;
        let open_orders_account_1 = solana.get_account::<OpenOrdersAccount>(account_1).await;

        assert_eq!(open_orders_account_0.position.bids_base_lots, 1);
        assert_eq!(open_orders_account_0.position.asks_base_lots, 0);
        assert_eq!(open_orders_account_0.position.taker_base_lots, 0);
        assert_eq!(open_orders_account_0.position.base_free_lots, 0);
        assert_eq!(open_orders_account_0.position.quote_free_lots, 0);
        assert_eq!(open_orders_account_1.position.bids_base_lots, 0);
        assert_eq!(open_orders_account_1.position.asks_base_lots, 1);
        assert_eq!(open_orders_account_1.position.taker_base_lots, 0);
        assert_eq!(open_orders_account_1.position.base_free_lots, 0);
        assert_eq!(open_orders_account_1.position.quote_free_lots, 0);
    }

    send_tx(
        solana,
        ConsumeEventsInstruction {
            market,
            open_orders_accounts: vec![account_0, account_1],
        },
    )
    .await
    .unwrap();

    // ConsumeEvents removes the bids_base_lots in the Out event
    {
        let open_orders_account_0 = solana.get_account::<OpenOrdersAccount>(account_0).await;
        let open_orders_account_1 = solana.get_account::<OpenOrdersAccount>(account_1).await;

        assert_eq!(open_orders_account_0.position.bids_base_lots, 0);
        assert_eq!(open_orders_account_0.position.asks_base_lots, 0);
        assert_eq!(open_orders_account_0.position.taker_base_lots, 0);
        assert_eq!(open_orders_account_0.position.base_free_lots, 0);
        assert_eq!(open_orders_account_0.position.quote_free_lots, 10000);
        assert_eq!(open_orders_account_1.position.bids_base_lots, 0);
        assert_eq!(open_orders_account_1.position.asks_base_lots, 1);
        assert_eq!(open_orders_account_1.position.taker_base_lots, 0);
        assert_eq!(open_orders_account_1.position.base_free_lots, 0);
        assert_eq!(open_orders_account_1.position.quote_free_lots, 0);
    }

    Ok(())
}
