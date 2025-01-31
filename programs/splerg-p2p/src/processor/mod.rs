use spl_token_2022::check_spl_token_program_account;

use {
    borsh::{BorshDeserialize, BorshSerialize},
    solana_program::{
        account_info::{next_account_info, AccountInfo},
        entrypoint::ProgramResult,
        program::{invoke, invoke_signed},
        program_error::ProgramError,
        program_pack::Pack,
        pubkey::Pubkey,
        system_instruction,
        sysvar::rent::Rent,
        sysvar::Sysvar,
    },
};

use crate::{
    error::SwapError,
    instruction::SwapInstruction,
    state::SwapOrder,
    validation::{
        get_order_pda, validate_authority, validate_init_amounts, validate_order_pda,
        validate_rent_sysvar, validate_signer, validate_system_program, validate_taker,
        validate_token_account, validate_token_mint, validate_token_program,
    },
};

pub struct Processor;

impl Processor {
    pub fn process(
        program_id: &Pubkey,
        accounts: &[AccountInfo],
        instruction_data: &[u8],
    ) -> ProgramResult {
        let instruction = SwapInstruction::try_from_slice(instruction_data)?;

        match instruction {
            SwapInstruction::InitializeOrder {
                maker_amount,
                taker_amount,
            } => Self::process_initialize_order(program_id, accounts, maker_amount, taker_amount),
            SwapInstruction::ChangeOrderAmounts {
                new_maker_amount,
                new_taker_amount,
            } => Self::process_change_order_amounts(
                program_id,
                accounts,
                new_maker_amount,
                new_taker_amount,
            ),
            SwapInstruction::ChangeTaker { new_taker } => {
                Self::process_change_taker(accounts, new_taker)
            }
            SwapInstruction::CompleteSwap => Self::process_complete_swap(program_id, accounts),
            SwapInstruction::CloseOrder => Self::process_close_order(program_id, accounts),
        }
    }

    fn process_initialize_order(
        program_id: &Pubkey,
        accounts: &[AccountInfo],
        maker_amount: u64,
        taker_amount: u64,
    ) -> ProgramResult {
        let account_info_iter = &mut accounts.iter();
        let maker_info = next_account_info(account_info_iter)?;
        let order_account_info = next_account_info(account_info_iter)?;
        let maker_mint_ata_info = next_account_info(account_info_iter)?;
        let order_maker_mint_ata_info = next_account_info(account_info_iter)?;
        let taker_info = next_account_info(account_info_iter)?;
        let maker_mint_info = next_account_info(account_info_iter)?;
        let taker_mint_info = next_account_info(account_info_iter)?;
        let system_program_info = next_account_info(account_info_iter)?;
        let rent_info = next_account_info(account_info_iter)?;
        let token_program = next_account_info(account_info_iter)?;

        validate_signer(maker_info)?;
        validate_init_amounts(maker_amount, taker_amount)?;
        validate_token_mint(maker_mint_info)?;
        validate_token_mint(taker_mint_info)?;
        check_spl_token_program_account(token_program.key)?;
        validate_token_program(maker_mint_info, token_program.key)?;
        validate_token_account(maker_mint_ata_info, maker_info.key, maker_mint_info.key)?;
        validate_system_program(system_program_info.key)?;
        validate_rent_sysvar(rent_info.key)?;
        validate_token_account(
            order_maker_mint_ata_info,
            order_account_info.key,
            maker_mint_info.key,
        )?;

        let (_, bump) = get_order_pda(
            program_id,
            maker_info.key,
            maker_mint_info.key,
            taker_mint_info.key,
        )?;

        let rent = Rent::from_account_info(rent_info)?;
        let space = SwapOrder::LEN;
        let rent_lamports = rent.minimum_balance(space);

        invoke_signed(
            &system_instruction::create_account(
                maker_info.key,
                order_account_info.key,
                rent_lamports,
                space as u64,
                program_id,
            ),
            &[
                maker_info.clone(),
                order_account_info.clone(),
                system_program_info.clone(),
            ],
            &[&[
                b"order",
                maker_info.key.as_ref(),
                maker_mint_info.key.as_ref(),
                taker_mint_info.key.as_ref(),
                &[bump],
            ]],
        )?;

        let transfer_instruction = if *token_program.key == spl_token::id() {
            spl_token::instruction::transfer(
                token_program.key,
                maker_mint_ata_info.key,
                order_maker_mint_ata_info.key,
                maker_info.key,
                &[],
                maker_amount,
            )?
        } else {
            let account_data = spl_token_2022::state::Mint::unpack(&maker_mint_info.data.borrow())?;
            spl_token_2022::instruction::transfer_checked(
                token_program.key,
                maker_mint_ata_info.key,
                maker_mint_info.key,
                order_maker_mint_ata_info.key,
                maker_info.key,
                &[],
                maker_amount,
                account_data.decimals,
            )?
        };

        invoke(
            &transfer_instruction,
            &[
                maker_mint_ata_info.clone(),
                order_maker_mint_ata_info.clone(),
                maker_info.clone(),
                token_program.clone(),
            ],
        )?;

        let order = SwapOrder::new(
            *maker_info.key,
            *taker_info.key,
            *maker_mint_info.key,
            *taker_mint_info.key,
            maker_amount,
            taker_amount,
            bump,
        );

        order.serialize(&mut *order_account_info.data.borrow_mut())?;

        Ok(())
    }

    fn process_change_order_amounts(
        program_id: &Pubkey,
        accounts: &[AccountInfo],
        new_maker_amount: u64,
        new_taker_amount: u64,
    ) -> ProgramResult {
        let account_info_iter = &mut accounts.iter();
        let maker_info = next_account_info(account_info_iter)?;
        let order_account_info = next_account_info(account_info_iter)?;
        let order_token_account = next_account_info(account_info_iter)?;
        let maker_token_account = next_account_info(account_info_iter)?;
        let token_program = next_account_info(account_info_iter)?;

        let (mut order, _) = validate_order_pda(program_id, order_account_info)?;

        validate_authority(maker_info, &order)?;
        check_spl_token_program_account(token_program.key)?;
        validate_token_account(
            order_token_account,
            order_account_info.key,
            &order.maker_token_mint,
        )?;
        validate_token_account(
            order_token_account,
            order_account_info.key,
            &order.maker_token_mint,
        )?;

        // Get current escrow balance
        let escrow_token_data =
            spl_token::state::Account::unpack(&order_token_account.data.borrow())?;
        let current_escrow_amount = escrow_token_data.amount;

        match new_maker_amount.cmp(&current_escrow_amount) {
            std::cmp::Ordering::Greater => {
                // Need to transfer additional tokens to escrow
                let additional_amount = new_maker_amount - current_escrow_amount;

                if *token_program.key == spl_token::id() {
                    invoke(
                        &spl_token::instruction::transfer(
                            token_program.key,
                            maker_token_account.key,
                            order_token_account.key,
                            maker_info.key,
                            &[],
                            additional_amount,
                        )?,
                        &[
                            maker_token_account.clone(),
                            order_token_account.clone(),
                            maker_info.clone(),
                            token_program.clone(),
                        ],
                    )?;
                } else {
                    invoke(
                        &spl_token_2022::instruction::transfer(
                            token_program.key,
                            maker_token_account.key,
                            order_token_account.key,
                            maker_info.key,
                            &[],
                            additional_amount,
                        )?,
                        &[
                            maker_token_account.clone(),
                            order_token_account.clone(),
                            maker_info.clone(),
                            token_program.clone(),
                        ],
                    )?;
                }
            }
            std::cmp::Ordering::Less => {
                // Need to refund tokens to maker
                let refund_amount = current_escrow_amount - new_maker_amount;

                if *token_program.key == spl_token::id() {
                    invoke_signed(
                        &spl_token::instruction::transfer(
                            token_program.key,
                            order_token_account.key,
                            maker_token_account.key,
                            order_account_info.key,
                            &[],
                            refund_amount,
                        )?,
                        &[
                            order_token_account.clone(),
                            maker_token_account.clone(),
                            order_account_info.clone(),
                            token_program.clone(),
                        ],
                        &[&[
                            b"order",
                            maker_info.key.as_ref(),
                            &order.maker_token_mint.to_bytes(),
                            &order.taker_token_mint.to_bytes(),
                            &[order.bump],
                        ]],
                    )?;
                } else {
                    invoke_signed(
                        &spl_token_2022::instruction::transfer(
                            token_program.key,
                            order_token_account.key,
                            maker_token_account.key,
                            order_account_info.key,
                            &[],
                            refund_amount,
                        )?,
                        &[
                            order_token_account.clone(),
                            maker_token_account.clone(),
                            order_account_info.clone(),
                            token_program.clone(),
                        ],
                        &[&[
                            b"order",
                            maker_info.key.as_ref(),
                            &order.maker_token_mint.to_bytes(),
                            &order.taker_token_mint.to_bytes(),
                            &[order.bump],
                        ]],
                    )?;
                }
            }
            std::cmp::Ordering::Equal => {} // No token transfer needed
        }

        order.maker_amount = new_maker_amount;
        order.taker_amount = new_taker_amount;
        order.serialize(&mut *order_account_info.data.borrow_mut())?;

        Ok(())
    }

    fn process_change_taker(accounts: &[AccountInfo], new_taker: [u8; 32]) -> ProgramResult {
        let account_info_iter = &mut accounts.iter();
        let maker_info = next_account_info(account_info_iter)?;
        let order_account_info = next_account_info(account_info_iter)?;
        let new_taker_info = next_account_info(account_info_iter)?;

        let mut order = SwapOrder::try_from_slice(&order_account_info.data.borrow())?;
        validate_authority(maker_info, &order)?;

        if Pubkey::new_from_array(new_taker) != *new_taker_info.key {
            return Err(ProgramError::InvalidArgument);
        }

        order.taker = Pubkey::new_from_array(new_taker);
        order.serialize(&mut *order_account_info.data.borrow_mut())?;

        Ok(())
    }

    fn process_complete_swap(program_id: &Pubkey, accounts: &[AccountInfo]) -> ProgramResult {
        let account_info_iter = &mut accounts.iter();
        let taker_info = next_account_info(account_info_iter)?;
        let order_account_info = next_account_info(account_info_iter)?;
        let maker_taker_mint_ata = next_account_info(account_info_iter)?;
        let taker_sending_ata = next_account_info(account_info_iter)?;
        let taker_maker_mint_ata = next_account_info(account_info_iter)?;
        let order_maker_token_ata = next_account_info(account_info_iter)?;
        let token_program = next_account_info(account_info_iter)?;

        let (order, _) = validate_order_pda(program_id, order_account_info)?;
        validate_taker(taker_info, &order)?;
        check_spl_token_program_account(token_program.key)?;
        validate_token_account(maker_taker_mint_ata, &order.maker, &order.taker_token_mint)?;
        validate_token_account(
            taker_maker_mint_ata,
            taker_info.key,
            &order.maker_token_mint,
        )?;
        validate_token_account(taker_sending_ata, taker_info.key, &order.taker_token_mint)?;
        validate_token_account(
            order_maker_token_ata,
            order_account_info.key,
            &order.maker_token_mint,
        )?;

        // Verify we have enough tokens in escrow
        let escrow_token_data =
            spl_token::state::Account::unpack(&order_maker_token_ata.data.borrow())?;
        if escrow_token_data.amount < order.maker_amount {
            return Err(SwapError::InsufficientFunds.into());
        }

        if *token_program.key == spl_token::id() {
            invoke(
                &spl_token::instruction::transfer(
                    token_program.key,
                    taker_sending_ata.key,
                    maker_taker_mint_ata.key,
                    taker_info.key,
                    &[],
                    order.taker_amount,
                )?,
                &[
                    taker_sending_ata.clone(),
                    maker_taker_mint_ata.clone(),
                    taker_info.clone(),
                    token_program.clone(),
                ],
            )?;
        } else {
            invoke(
                &spl_token_2022::instruction::transfer(
                    token_program.key,
                    taker_sending_ata.key,
                    maker_taker_mint_ata.key,
                    taker_info.key,
                    &[],
                    order.taker_amount,
                )?,
                &[
                    taker_sending_ata.clone(),
                    maker_taker_mint_ata.clone(),
                    taker_info.clone(),
                    token_program.clone(),
                ],
            )?;
        }

        if *token_program.key == spl_token::id() {
            invoke_signed(
                &spl_token::instruction::transfer(
                    token_program.key,
                    order_maker_token_ata.key,
                    taker_maker_mint_ata.key,
                    order_account_info.key,
                    &[],
                    order.maker_amount,
                )?,
                &[
                    order_maker_token_ata.clone(),
                    taker_maker_mint_ata.clone(),
                    order_account_info.clone(),
                    token_program.clone(),
                ],
                &[&[
                    b"order",
                    &order.maker.to_bytes(),
                    &order.maker_token_mint.to_bytes(),
                    &order.taker_token_mint.to_bytes(),
                    &[order.bump],
                ]],
            )?;
        } else {
            invoke_signed(
                &spl_token_2022::instruction::transfer(
                    token_program.key,
                    order_maker_token_ata.key,
                    taker_maker_mint_ata.key,
                    order_account_info.key,
                    &[],
                    order.maker_amount,
                )?,
                &[
                    order_maker_token_ata.clone(),
                    taker_maker_mint_ata.clone(),
                    order_account_info.clone(),
                    token_program.clone(),
                ],
                &[&[
                    b"order",
                    &order.maker.to_bytes(),
                    &order.maker_token_mint.to_bytes(),
                    &order.taker_token_mint.to_bytes(),
                    &[order.bump],
                ]],
            )?;
        }

        Ok(())
    }

    fn process_close_order(program_id: &Pubkey, accounts: &[AccountInfo]) -> ProgramResult {
        let account_info_iter = &mut accounts.iter();
        let authority_info = next_account_info(account_info_iter)?;
        let order_account_info = next_account_info(account_info_iter)?;
        let order_token_ata = next_account_info(account_info_iter)?;
        let maker_token_ata = next_account_info(account_info_iter)?;
        let token_program = next_account_info(account_info_iter)?;

        let (order, _) = validate_order_pda(program_id, order_account_info)?;
        validate_authority(authority_info, &order)?;
        check_spl_token_program_account(token_program.key)?;
        validate_token_account(
            order_token_ata,
            order_account_info.key,
            &order.maker_token_mint,
        )?;
        validate_token_account(maker_token_ata, &order.maker, &order.maker_token_mint)?;

        let token_data = spl_token::state::Account::unpack(&order_token_ata.data.borrow())?;
        if token_data.amount > 0 {
            if *token_program.key == spl_token::id() {
                invoke_signed(
                    &spl_token::instruction::transfer(
                        token_program.key,
                        order_token_ata.key,
                        maker_token_ata.key,
                        order_account_info.key,
                        &[],
                        token_data.amount,
                    )?,
                    &[
                        order_token_ata.clone(),
                        maker_token_ata.clone(),
                        order_account_info.clone(),
                        token_program.clone(),
                    ],
                    &[&[
                        b"order",
                        &order.maker.to_bytes(),
                        &order.maker_token_mint.to_bytes(),
                        &order.taker_token_mint.to_bytes(),
                        &[order.bump],
                    ]],
                )?;

                invoke_signed(
                    &spl_token::instruction::close_account(
                        token_program.key,
                        order_token_ata.key,
                        authority_info.key,
                        order_account_info.key,
                        &[],
                    )?,
                    &[
                        order_token_ata.clone(),
                        authority_info.clone(),
                        order_account_info.clone(),
                        token_program.clone(),
                    ],
                    &[&[
                        b"order",
                        &order.maker.to_bytes(),
                        &order.maker_token_mint.to_bytes(),
                        &order.taker_token_mint.to_bytes(),
                        &[order.bump],
                    ]],
                )?;
            } else {
                invoke_signed(
                    &spl_token_2022::instruction::transfer(
                        token_program.key,
                        order_token_ata.key,
                        maker_token_ata.key,
                        order_account_info.key,
                        &[],
                        token_data.amount,
                    )?,
                    &[
                        order_token_ata.clone(),
                        maker_token_ata.clone(),
                        order_account_info.clone(),
                        token_program.clone(),
                    ],
                    &[&[
                        b"order",
                        &order.maker.to_bytes(),
                        &order.maker_token_mint.to_bytes(),
                        &order.taker_token_mint.to_bytes(),
                        &[order.bump],
                    ]],
                )?;

                invoke_signed(
                    &spl_token_2022::instruction::close_account(
                        token_program.key,
                        order_token_ata.key,
                        authority_info.key,
                        order_account_info.key,
                        &[],
                    )?,
                    &[
                        order_token_ata.clone(),
                        authority_info.clone(),
                        order_account_info.clone(),
                        token_program.clone(),
                    ],
                    &[&[
                        b"order",
                        &order.maker.to_bytes(),
                        &order.maker_token_mint.to_bytes(),
                        &order.taker_token_mint.to_bytes(),
                        &[order.bump],
                    ]],
                )?;
            }
        }

        let rent_lamports = order_account_info.lamports();
        **order_account_info.lamports.borrow_mut() = 0;
        **authority_info.lamports.borrow_mut() += rent_lamports;

        order_account_info.data.borrow_mut().fill(0);

        Ok(())
    }
}
