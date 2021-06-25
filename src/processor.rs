// Where the magic happens
use solana_program::{
    account_info::{next_account_info, AccountInfo},
    entrypoint::ProgramResult,
    msg,
    program::{invoke, invoke_signed},
    program_error::ProgramError,
    program_pack::{Pack, IsInitialized},
    pubkey::Pubkey,
    sysvar::{rent::Rent, Sysvar},
};


use spl_token::state::Account as TokenAccount;

// NOTE use crate -> refers to our local modules (crates?) we've made
// All crates must be registered inside Cargo.toml
use crate::{instruction::EscrowInstruction, error::EscrowError, state::Escrow};

pub struct Processor;
// Q: Is impl like making a class?
impl Processor {
    pub fn process(
        program_id: &Pubkey,
        accounts: &[AccountInfo],
        instruction_data: &[u8]
    ) -> ProgramResult {
        // Pass the reference to the slice holding the instruction_data from
        // entrypoint.rs into the unpack() fn
        // NOTE '?': Error values with the ? operator called on them go through
        // the 'from' function, defined in the From trait, which is used to convert
        // errors from one type into another: 
        // https://doc.rust-lang.org/book/ch09-02-recoverable-errors-with-result.html#a-shortcut-for-propagating-errors-the--operator
        let instruction = EscrowInstruction::unpack(instruction_data)?;

        // Use match to figure out which processing function to call (currently trivial,
        // since we don't have anything). msg! logs where we are going.
        match instruction {
            // tag = 0, we run the InitEscrow processing function
            EscrowInstruction::InitEscrow { amount } => {
                msg!("Instruction: InitEscrow");
                Self::process_init_escrow(accounts, amount, program_id)
            },
            // tag = 1, we run the Exchange processing function 
            EscrowInstruction::Exchange { amount } => {
                msg!("Instruction: Exchange");
                Self::process_exchange(accounts, amount, program_id)
            }
        }
    }

    fn process_init_escrow(
        accounts: &[AccountInfo],
        amount: u64,
        program_id: &Pubkey,
    ) -> ProgramResult {
        // Create an mutable iterator
        let account_info_iter = &mut accounts.iter();
        // IMPORTANT: The first account we expect - AS DEFINED IN instruction.rs -
        // is the escrow's initializer, i.e., Alice's MAIN ACCOUNT.
        // Recall the accounts defined in instruction.rs:
        /// 0. `[signer]` The account of the person initializing the escrow
        /// 1. `[writable]` Temporary token account that should be created prior to this instruction and owned by the initializer
        /// 2. `[]` The initializer's token account for the token they will receive should the trade go through
        /// 3. `[writable]` The escrow account, it will hold all necessary info about the trade.
        /// 4. `[]` The rent sysvar. NOTE sysvar can be accessed without passing into entrypoint as an account
        /// 5. `[]` The token program

        let initializer = next_account_info(account_info_iter)?;

        // Check that Alice is the signer via AccountInfo is_signer boolean field
        if !initializer.is_signer {
            return Err(ProgramError::MissingRequiredSignature);
        }

        // Grab the temporary token account that will eventually hold the transferred
        // tokens. NOTE It needs to be writable but we don't need to explicitly check,
        // because the transaction will fail automatically should Alice (initializer)
        // not mark the account as writable.
        let temp_token_account = next_account_info(account_info_iter)?;
        let token_to_receive_account = next_account_info(account_info_iter)?;

        // IMPORTANT:
        // Q: Why check token_to_receive_account.owner is actually owned by Token Program,
        // but don't do the same for temp_token_account?
        // A: Because later on in the function we will ask the Token Program to transfer
        // ownership of the temp_token_account to the PDA (Program Derived Address).
        // This transfer will fail if the temp_token_account is not owned by the Token Program,
        // because only PROGRAMS that own accounts may change accounts. Hence, there is no
        // need to add another check here.
        // We don't make any changes to the token_to_receive_account though (inside Alice's
        // transaction). We will just save it into the escrow data so that when Bob takes the
        // trade, the escrow will know where to send his asset Y. Thus, for this account, 
        // we should add a check.
        // NOTE Nothing terrible would happen if we didn't add this check. Instead, Bob's
        // transaction would fail because the Token Program will attempt to send the Y tokens
        // to Alice but not be the owner of the token_to_receive_account. That said, it seems
        // more reasonable explicitly specify which transaction failed/led to the invalid state.
        // NOTE spl_token is a crate and it's aka the token program
        if *token_to_receive_account.owner != spl_token::id() {
            // NOTE '*' means the ACTUAL value, NOT a reference. * is for de-referencing.
            return Err(ProgramError::IncorrectProgramId);
        }
        // TODO Check that token_to_receive_account is a TOKEN account, NOT a 
        // token MINT account!
        // if *token_to_receive_account.type != spl_token::

        let escrow_account = next_account_info(account_info_iter)?;
        let rent = &Rent::from_account_info(next_account_info(account_info_iter)?)?;

        // NOTE Most times you want your accounts to be rent-exempt, because if
        // balances go to zero, they DISAPPEAR (i.e., purged from memory at runtime)!
        // This is why we're checking whether escrow (state) account is exempt. 
        // If we didn't do this check, and Alice were to pass in a non-rent-exempt account,
        // the account balance might go to zero balance before Bob takes the trade.
        // With the account gone, Alice would have no way to recover her tokens.
        if !rent.is_exempt(escrow_account.lamports(), escrow_account.data_len()) {
            return Err(EscrowError::NotRentExempt.into());
        }

        // NOTE First time we access the account data ([u8]). We deserialize/decode it with
        // Escrow::unpack_unchecked() from state.rs (soon to create), which will return
        // an actual Escrow type (defined in state.rs) we can work with.
        // NOTE We're using 'mut' since we want to add data to this object
        let mut escrow_info = Escrow::unpack_unchecked(&escrow_account.data.borrow())?;
        if escrow_info.is_initialized() {
            return Err(ProgramError::AccountAlreadyInitialized);
        }

        // Now let's add the state serialization. We've already created the Escrow struct
        // instance (via unpack_unchecked) and checked that it is indeed uninitialized.
        // Time to populate the Escrow struct fields!
        // NOTE This literally is the data we'll be double-checking for the actual transfer
        // instruction (see Exchange below).
        escrow_info.is_initialized = true;
        escrow_info.initializer_pubkey = *initializer.key;
        escrow_info.temp_token_account_pubkey = *temp_token_account.key;
        escrow_info.initializer_token_to_receive_account_pubkey = *token_to_receive_account.key;
        escrow_info.expected_amount = amount;

        // Serialize our escrow_info object using 'pack' default function, which internally
        // calls our 'pack_into_slice' function.
        Escrow::pack(escrow_info, &mut escrow_account.data.borrow_mut())?;

        // Need to transfer (user space) ownership of the temporary token account to the PDA
        // NOTE We create a PDA by passing in an array of seeds and the program_id into the
        // find_program_address function. We get back a new pda and bump_seed (we won't need
        // the bump seed in Alice's tx) with a 1/(2^255) chance the function fails (2^255 is BIG).
        // In our case the seeds can be static (there are cases such as Associated Token
        // Account Program where seeds aren't static). We just need 1 PDA that can own N temp
        // token accounts for different escrows occurring at any and possibly the same point
        // in time.
        // NOTE A PDA are public keys that are derived from the program_id and the seeds as
        // well as having been pushed off the ed25519 standard eliptic curve by the 
        // bump seed (nonce)! (Solana key pairs use the ed25519 standard.)
        // Hence, PDAs do not lie on the ed25519 curve and therefore have no private keys
        // associated with them! A PDA is just a random array of bytes with the only defining
        // feature being that they are NOT on that curve. We'll later use the bump seed when
        // we look into signing messages with PDAs even without a private key (Bob's tx)!
        // https://paulx.dev/blog/2021/01/14/programming-on-solana-an-introduction/#pdas-part-2
        let (pda, _bump_seed) = Pubkey::find_program_address(&[b"escrow"], program_id);

        // To transfer ownership of temporary token account to PDA, we will call the
        // token program (spl_token) from our escrow program and create a new
        // INSTRUCTION! owner_change_ix is an Instruction!
        // NOTE This is called Cross-Program Invocation/Instruction and executed using either
        // invoke() or invoke_signed() functions.
        // Grab the token_program account
        // NOTE The program getting called through a CPI must be included as an account
        // in the 2nd argument of invoke() and invoke_signed() functions.
        let token_program = next_account_info(account_info_iter)?;
        // Create the instruction (CPI) that the token_program would expect were we executing
        // a normal call. The token program defines some helper functions inside its
        // instruction.rs that we can make use of (e.g., set_authority fn). 
        // set_authority() is a builder function to create such an instruction.
        // NOTE We're using the Signature Extension concept here:
        // https://docs.solana.com/developing/programming-model/calling-between-programs#instructions-that-require-privileges
        // "When including a signed account in a program call, in all CPIs including that
        // account made by the program inside the current instruction, the account will
        // also be signed, i.e., the signature is extended to the CPIs."
        // NOTE This means that since Alice signed the InitEscrow transaction, the program
        // can make the token program set_authority CPI and include her pubkey as a signer pubkey.
        // This is necessary because changing a token account's owner should of course require
        // the approval of the current owner.
        // IMPORTANT By token program we mean the spl_token program, which has its own
        // instruction.rs:
        // https://docs.rs/spl-token/2.0.4/src/spl_token/instruction.rs.html#538-550
        // NOTE Generally, before making a CPI, we should check that token_program is truly
        // the account of the token program. Otherwise, we might be calling a rogue program.
        // Thankfully, spl-token crate > 3.1.1 (which we're using) does this if we use
        // their instruction builder functions (i.e., spl_token::instruction::set_authority())
        let owner_change_ix = spl_token::instruction::set_authority(
            token_program.key, // token program id
            temp_token_account.key, // account whose authority we want to change
            Some(&pda), // account that's the new authority (PDA)
            spl_token::instruction::AuthorityType::AccountOwner, // type of authority change
            initializer.key,  // current account owner (Alice -> initializer.key)
            &[&initializer.key], // public keys signing the CPI
        )?;

        msg!("Calling the token program to transfer token account ownership...");
        invoke(
            &owner_change_ix, // The Cross-Program Instruction
            &[
                temp_token_account.clone(), // Accounts required by the CPI instruction
                initializer.clone(), // Accounts required by the CPI instruction
                token_program.clone(), // Account of the program we're calling
                // NOTE We can see params of spl_token set_authority() in its instruction.rs
                // https://docs.rs/spl-token/2.0.4/src/spl_token/instruction.rs.html#538-550
            ], 
        )?;

        Ok(())
    }


    fn process_exchange(
        accounts: &[AccountInfo],
        amount_expected_by_taker: u64,
        program_id: &Pubkey,
    ) -> ProgramResult {
        msg!("Calling process_exchange function");
        // Get an iterator from the accounts passed into the Exchange instruction
        let account_info_iter = &mut accounts.iter();
        /// IMPORTANT: This is Bob's Transaction. Alice has already created the Escrow,
        /// so now Bob needs to send the correct amount of Y tokens to the Escrow,
        /// then the Escrow will send him Alice's X tokens and Alice his Y tokens.
        ///
        ///
        /// 0. `[signer]` The account of the person taking the trade (Bob. Alice is the Initializer)
        /// 1. `[writable]` The taker's (Bob) token account for the token they send 
        /// 2. `[writable]` The taker's token account for the token they will receive should the trade go through
        /// 3. `[writable]` The PDA's temp token account to get tokens from and eventually close
        /// 4. `[writable]` The initializer's main account to send their rent fees to
        /// 5. `[writable]` The initializer's token account that will receive tokens
        /// 6. `[writable]` The escrow account holding the escrow info
        /// 7. `[]` The token program
        /// 8. `[]` The PDA account
        // Time to loop over the accounts and assign to variables
        // 0. Let's grab the taker account information
        let taker = next_account_info(account_info_iter)?;
        // Check that taker (Bob) is signer via AccountInfo is_signer boolean field
        if !taker.is_signer {
            return Err(ProgramError::MissingRequiredSignature);
        }

        // 1. Grab taker's sending token account (Y token)
        let takers_sending_token_account = next_account_info(account_info_iter)?;

        // 2. Grab taker's receiving token account (X token) if trade is successful
        let takers_receiving_token_account = next_account_info(account_info_iter)?;

        // 3. Grab Alice's temp X token account that's currently owned by
        // the Escrow Program's PDA
        let pdas_temp_token_account = next_account_info(account_info_iter)?;
        // Check that this temp account holds what taker (Bob) expects in X token
        // Q: The 'amount' information is stored in the account's data I think...
        // A: Yes, had the right idea. We're going to use TokenAccount from state.rs
        // to help us unpack this account data to double-check amounts are accurate
        let pdas_temp_token_account_info = TokenAccount::unpack(&pdas_temp_token_account.data.borrow())?;
        // Find the PDA address by using array of seeds and program_id.
        // NOTE We're going to use this pda when passing authority_pubkey in transfer ix
        let (pda, bump_seed) = Pubkey::find_program_address(&[b"escrow"], program_id);
        // Finally, check that temp X token account amount is equal to this process_exchange's
        // amount_expected_by_taker value.
        if amount_expected_by_taker != pdas_temp_token_account_info.amount {
            // Q: What does .into() do?
            return Err(EscrowError::ExpectedAmountMismatch.into());
        }

        // 4. Grab the initializer's main account information
        let initializers_main_account = next_account_info(account_info_iter)?;

        // 5. Grab initializer's Y token account that will receive Y tokens (Alice's Y token)
        let initializers_token_to_receive_account = next_account_info(account_info_iter)?;

        // 6. Grab the Escrow State Account that's holding all the escrow info
        let escrow_account = next_account_info(account_info_iter)?;

        // 6.1 Check that PDA's temp token account matches the same as the Escrow Account's
        // temp_token_account key. First need to unpack escrow_account data
        let escrow_info = Escrow::unpack(&escrow_account.data.borrow())?;
        // NOTE Need to dereference the borrow using '*' to match struct Pubkey
        if escrow_info.temp_token_account_pubkey != *pdas_temp_token_account.key {
            return Err(ProgramError::InvalidAccountData);
        }

        // 6.2 Check whether Escrow Accounts data/info for initializer pubkey matches
        // the initializers_main_account that was passed into accounts arg
        // NOTE Need to dereference the borrow using '*' to match struct Pubkey
        if escrow_info.initializer_pubkey != *initializers_main_account.key {
            return Err(ProgramError::InvalidAccountData);
        }

        // 6.3 Check whether the accounts for tokens to receive match between the
        // Escrow Account's data/info and the accounts arg. Basically checking
        // whether they both point to Alice's Y token account address.
        if escrow_info.initializer_token_to_receive_account_pubkey != *initializers_token_to_receive_account.key {
            return Err(ProgramError::InvalidAccountData);
        }

        // 7. Grab the Token Program account
        // NOTE Recall that even programs in Solana live inside an account, i.e., the program
        // in its binary form (e.g., helloworld.so, spl_token.so, etc.) is actually going to
        // be the data of some account. This is key!
        let token_program = next_account_info(account_info_iter)?;

        // Q: Check whether token_program account is the same as the program that initialized
        // the temp X token account? (NOT the user space owner property, as that is the Escrow
        // Program's PDA. Not sure if that's needed...)

        // Time to transfer Y tokens from Bob's account to Alice's Y token account
        // To do this, we're actually creating an Transfer Instruction.
        // NOTE To perform the actual transfer we use spl_token::instruction::transfer built-in
        // method, which is a CPI. We then will use invoke() to call this new instruction
        // and pass in this instruction along with the accounts involved.
        // NOTE This is using Signature Extension to make the token transfer to Alice's Y
        // token account on Bob's behalf.
        let transfer_to_initializer_ix = spl_token::instruction::transfer(
            token_program.key,
            takers_sending_token_account.key, // source (Bob's Y token account)
            initializers_token_to_receive_account.key, // destination (Alice's Y token account)
            taker.key, // authority_pubkey (Bob's main account since he's authorizing the trade)
            &[&taker.key], // signers array
            escrow_info.expected_amount, // This is the amount passed to InitEscrow, i.e., Alice's X token amount
            // NOTE Or, in other words, the agreed upon amount Bob expects to receive in X tokens for
            // his Y tokens he's going to transfer to Alice.
        )?;
        msg!("Calling the token program to transfer tokens to the escrow's initializer...");
        invoke(
            &transfer_to_initializer_ix, // CPI instruction
            &[
                takers_sending_token_account.clone(), // Bob's Y token account
                initializers_token_to_receive_account.clone(), // Alice's Y token account
                taker.clone(), // Bob's main account
                token_program.clone(), // Token Program AccountInfo.
                // NOTE The token program key (id) is the program id of this
                // transfer_to_initializer_ix
            ],
        )?;


        // 8. Time to transfer X tokens from temp X token account to Bob's main X token account
        // NOTE The PDA has authority on the temp X token account
        let pda_account = next_account_info(account_info_iter)?;

        // Create another Transfer Instruction
        let transfer_to_taker_ix = spl_token::instruction::transfer(
            token_program.key,
            pdas_temp_token_account.key, // source. Q: Why not from pda_account?
            takers_receiving_token_account.key, // destination (Bob's X token account).
            &pda, // authority_pubkey (retrieved from find_program_address() above)
            &[&pda],  // signers array. Again, the PDA is the signer
            pdas_temp_token_account_info.amount,// amount to transfer (inside PDA AccountInfo)
        )?;
        // Now time to invoke this instruction
        // NOTE This uses invoke_signed to allow the PDA to sign something. Recall that
        // a PDA is bumped off the Ed25519 elliptic curve. Hence, there is NO private key.
        // (And hence why we pass &pda instead of &pda.key I believe.)
        // Q: Can PDAs sign CPIs? No, but actually yes! The PDA isn't actually signing
        // the CPI in cryptographic fashion. In addition to the two args, the invoke_signed()
        // takes a third argument: the seeds that were used to create the PDA the CPI is
        // supposed to be "signed" with. Technically, the find_program_address() fn adds
        // the bump seed to make the PDA fall off the Ed25519 curve.
        //
        // NOTE When a program calls invoke_signed(), the runtime uses those seeds and the
        // program id of the calling program to recreate the PDA. If the PDA matches one
        // of the given accounts inside invoke_signed's arguments, that account's 'signed'
        // property will be set to true. This means that no other program (smart contract)
        // can fake this PDA because it is the runtime that sees which program is making the
        // invoke_signed call. In our case, only the Escrow program will have the program id
        // that will result in a PDA equal to one of the addresses in invoke_signed's
        // 'accounts' argument.
        //
        // Read more at:
        // https://paulx.dev/blog/2021/01/14/programming-on-solana-an-introduction/#processor-part-3-pdas-part-3
        msg!("Calling the token program to transfer tokens to the taker from pda temp account...");
        invoke_signed(
            &transfer_to_taker_ix, // CPI instruction
            &[
                pdas_temp_token_account.clone(),
                takers_receiving_token_account.clone(),
                pda_account.clone(),
                token_program.clone(),
            ],
            &[&[&b"escrow"[..], &[bump_seed]]], // seeds used to create the PDA
        )?;


        // 9. Need to tidy up and close the temp PDA account using invoke_signed fn
        // NOTE Accounts are required to have a min balance to be rent exempt. 
        // So, when we no longer need an account (ie close the account), we can recover
        // the balance by transferring it to a different account.
        // NOTE If an account has no balance left, it will be purged from memory by the
        // runtime after the transaction (you can see this via the explorer on closed accts).
        // NOTE Since the temp token account is owned by the Token Program, only the TP may
        // decrease the balance. And because this action requires permission of the
        // (user space) owner of the token account (i.e., PDA in this case), we use
        // invoked_signed() fn again.
        let close_pdas_temp_acc_ix = spl_token::instruction::close_account(
            token_program.key,
            pdas_temp_token_account.key,
            initializers_main_account.key,
            &pda,
            &[&pda]
        )?;
        msg!("Calling the token program to close PDA's temp account...");
        invoke_signed(
            &close_pdas_temp_acc_ix,
            &[
                pdas_temp_token_account.clone(),
                initializers_main_account.clone(),
                pda_account.clone(),
                token_program.clone(),
            ],
            &[&[&b"escrow"[..], &[bump_seed]]],
        )?;

        // 10. Time to close the Escrow (state) account to conclude this program
        msg!("Closing the escrow account...");
        // We can credit Alice's main account with remaining balance in escrow account
        // NOTE You can credit her account even though Escrow Program isn't the owner
        // of her (initializer's) account.
        **initializers_main_account.lamports.borrow_mut() = initializers_main_account
            .lamports()
            .checked_add(escrow_account.lamports())
            .ok_or(EscrowError::AmountOverflow)?; // Need to add this new error in error.rs
        **escrow_account.lamports.borrow_mut() = 0;
        *escrow_account.data.borrow_mut() = &mut []; // Set the 'data' field to empty slice
        // NOTE Even though the account will be purged at runtime, this is not the final
        // instruction in the transaction. Thus, a subsequent tx may read or even revive
        // the data completely by making the account rent-exempt again. Depending on your
        // program, forgetting to clear the data field can have dangerous consequences.
        // NOTE When you are intentionally closing an account by setting its lamports to zero
        // so it's removed from memory after the tx, make sure to either clear the 'data' field
        // or leave the data in a state that would be OK to be recovered by a subsequent
        // transaction.

        Ok(())
    }
}
