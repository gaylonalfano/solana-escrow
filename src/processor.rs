// Where the magic happens
use solana_program::{
    account_info::{next_account_info, AccountInfo},
    entrypoint::ProgramResult,
    msg,
    program::invoke,
    program_error::ProgramError,
    program_pack::{Pack, IsInitialized},
    pubkey::Pubkey,
    sysvar::{rent::Rent, Sysvar},
};

// use spl_token::state::Account as TokenAccount;

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
            EscrowInstruction::InitEscrow { amount } => {
                msg!("Instruction: InitEscrow");
                Self::process_init_escrow(accounts, amount, program_id)
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
        // balances go to zero, they DISAPPEAR!
        if !rent.is_exempt(escrow_account.lamports(), escrow_account.data_len()) {
            return Err(EscrowError::NotRentExempt.into());
        }

        // NOTE First time we access the account data ([u8]). We deserialize/decode it with
        // Escrow::unpack_unchecked() from state.rs (soon to create), which will return
        // an actual Escrow type (defined in state.rs) we can work with.
        let mut escrow_info = Escrow::unpack_unchecked(&escrow_account.data.borrow())?;
        if escrow_info.is_initialized() {
            return Err(ProgramError::AccountAlreadyInitialized);
        }

        // Now let's add the state serialization. We've already created the Escrow struct
        // instance (via unpack_unchecked) and checked that it is indeed uninitialized.
        // Time to populate the Escrow struct fields!
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
        // Create the instruction that the token_program would expect were we executing
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
}
