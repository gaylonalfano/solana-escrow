// Responsible for defining state objects that the processor can use,
// and responsible for serializing/deserializing such objects from/into
// arrays of u8.
use solana_program::{
    program_pack::{IsInitialized, Pack, Sealed},
    program_error::ProgramError,
    pubkey::Pubkey,
};

use arrayref::{array_mut_ref, array_ref, array_refs, mut_array_refs};

pub struct Escrow {
    // Determine whether a given escrow account is already in use. This, serialization,
    // and deserialization are all standardized in the traits of the program_pack module.
    // NOTE We need to implement Sealed and IsInitialized
    pub is_initialized: bool,
    pub initializer_pubkey: Pubkey,
    // Save temp_token_account_pubkey so that when Bob takes the trade,
    // the escrow program can send tokens from the account at temp_token_account_pubkey
    // to Bob's account. I believe this is Alice's temp_token_account.
    // IMPORTANT:
    // Q: Why save this account address here?
    // A: Bob will have to pass in the account into his entrypoint call eventually,
    // so if we save its public key here, Bob can easily find the address of accounts
    // he needs to pass into the entrypoint. Otherwise, Alice would have to send
    // him not only the escrow account address but also all her account addresses.
    // Secondly, and more important for security, is that Bob could pass in a different
    // token account. Nothing stops him from doing so if we don't add a check requiring
    // him to pass in the account with temp_token_account_pubkey as its public key.
    // And to add that check later in the processor, we need the InitEscrow instruction
    // to save the temp_token_account_pubkey.
    // IMPORTANT Any account can be passed into the entrypoint, including different ones
    // than those defined in the API (inside instruction.rs). Therefore, it's the
    // program's responsibility to check that received_accounts == expected_accounts.
    pub temp_token_account_pubkey: Pubkey,
    // Save initializer_token_to_receive_account_pubkey so that when Bob takes the trade,
    // his tokens can be sent to this account (i.e., Alice's Y token account).
    pub initializer_token_to_receive_account_pubkey: Pubkey,
    // Save expected_amount so we can use it to check that Bob sends enough tokens.
    pub expected_amount: u64,
}

// Implement Sealed and IsInitialized from program_pack to help determine
// whether a given escrow account is already in use, and perform serializations
// and deserializations.
// NOTE Sealed is Solana's version of Rust's Sized trait
// NOTE Pack relies on Sealed and, in our case, also on IsInitialized being implemented.
impl Sealed for Escrow {}

impl IsInitialized for Escrow {
    fn is_initialized(&self) -> bool {
        self.is_initialized
    }
}

// NOTE When implementing Pack you have to implement all of its traits
// e.g., LEN, unpack_from_slice, pack_into_slice, etc.
// Q: 'impl' does want exactly?
// I believe it's adding/inheriting functionality from other traits...
// A: Yes! NOTE What's neat is that you traits can have default functions that
// may be overridden (e.g., like we're doing below with unpack_from_slice),
// but don't have to be! For example, Pack has unpack_unchecked, pack, etc. default
// functions that we are NOT overwriting.
// https://docs.rs/solana-program/1.7.1/src/solana_program/program_pack.rs.html#29-39
impl Pack for Escrow {
    // Define the escrow's length.
    // LEN is the size of our type (Escrow). We can calculate the length of
    // the struct by adding the sizes of the individual data types:
    // 1 (bool) + 3 * 32 (Pubkey) + 1 * 8 (u64) = 105
    // NOTE It's okay to use an entire u8 for the bool since it'll make our
    // coding easier and the cost of those extra wasted bits is infinitesimal.
    const LEN: usize = 105;
    // Let's DESERIALIZE STATE using unpack_from_slice(), a static constructor function.
    // unpack_from_slice turns an array of u8 into an instance of the Escrow struct.
    // NOTE arrayref library for getting references to SECTIONS of a slice.
    // Need to add as dependency inside Cargo.toml.
    // NOTE Self in this case is a new instance of an Escrow struct
    fn unpack_from_slice(src: &[u8]) -> Result<Self, ProgramError> {
        let src = array_ref![src, 0, Escrow::LEN];
        let (
            is_initialized,
            initializer_pubkey,
            temp_token_account_pubkey,
            initializer_token_to_receive_account_pubkey,
            expected_amount,
        ) = array_refs![src, 1, 32, 32, 32, 8];

        let is_initialized = match is_initialized {
            [0] => false,
            [1] => true,
            _ => return Err(ProgramError::InvalidAccountData),
        };

        Ok(Escrow {
            is_initialized,
            initializer_pubkey: Pubkey::new_from_array(*initializer_pubkey),
            temp_token_account_pubkey: Pubkey::new_from_array(*temp_token_account_pubkey),
            initializer_token_to_receive_account_pubkey: Pubkey::new_from_array(*initializer_token_to_receive_account_pubkey),
            expected_amount: u64::from_le_bytes(*expected_amount),
        })
    }

    // Let's SERIALIZE STATE using pack_into_slice()
    // NOTE &self is passed since we now have an instance of Escrow struct/type
    // thanks to unpack_from_slice(). We don't do this for unpack_from_slice
    // because there was no self yet!
    fn pack_into_slice(&self, dst: &mut [u8]) {
        let dst = array_mut_ref![dst, 0, Escrow::LEN];
        let (
            is_initialized_dst,
            initializer_pubkey_dst,
            temp_token_account_pubkey_dst,
            initializer_token_to_receive_account_pubkey_dst,
            expected_amount_dst,
        ) = mut_array_refs![dst, 1, 32, 32, 32, 8];

        let Escrow {
            is_initialized,
            initializer_pubkey,
            temp_token_account_pubkey,
            initializer_token_to_receive_account_pubkey,
            expected_amount,
        } = self;

        is_initialized_dst[0] = *is_initialized as u8;
        initializer_pubkey_dst.copy_from_slice(initializer_pubkey.as_ref());
        temp_token_account_pubkey_dst.copy_from_slice(temp_token_account_pubkey.as_ref());
        initializer_token_to_receive_account_pubkey_dst.copy_from_slice(initializer_token_to_receive_account_pubkey.as_ref());
        *expected_amount_dst = expected_amount.to_le_bytes();
    }
}
