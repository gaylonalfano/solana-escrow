// Defines the API of the program
// NOTE This module is responsible for decoding instruction_data.
use std::convert::TryInto;
use solana_program::program_error::ProgramError;

use crate::error::EscrowError::InvalidInstruction;


pub enum EscrowInstruction {

    /// Starts the trade by creating and populating an escrow account and 
    /// transferring ownership of the given temp token account to the PDA
    ///
    ///
    /// Accounts expected:
    ///
    /// 0. `[signer]` The account of the person initializing the escrow
    /// 1. `[writable]` Temporary token account that should be created prior to this instruction and owned by the initializer
    /// 2. `[]` The initializer's token account for the token they will receive should the trade go through
    /// 3. `[writable]` The escrow account, it will hold all necessary info about the trade.
    /// 4. `[]` The rent sysvar. NOTE sysvar can be accessed without passing into entrypoint as an account
    /// 5. `[]` The token program
    /// NOTE In the guide, InitEscrow is sometimes referred as an 'endpoint'.
    InitEscrow {
        /// The amount party A expects to receive of token Y from party B
        /// NOTE This amount is provided through the instruction_data (not through an account!)
        amount: u64
    }
}

impl EscrowInstruction {
    /// Unpacks a byte buffer into a [EscrowInstruction](enum.EscrowInstruction.html).
    pub fn unpack(input: &[u8]) -> Result<Self, ProgramError> {
        // NOTE unpack chooses which instruction to build and then builds and returns
        // that instruction.
        // NOTE The first byte is 'tag' and the rest of slice is 'rest'
        // 'tag' tells unpack how to decode the 'rest' of the slice.
        let (tag, rest) = input.split_first().ok_or(InvalidInstruction)?;

        Ok(match tag {
            0 => Self::InitEscrow {
                amount: Self::unpack_amount(rest)?,
            },
            _ => return Err(InvalidInstruction.into()),
        })
    }

    fn unpack_amount(input: &[u8]) -> Result<u64, ProgramError> {
        // Decodes the 'rest' of the slice to get a u64 representing amount
        let amount = input
            .get(..8)
            .and_then(|slice| slice.try_into().ok())
            .map(u64::from_le_bytes)
            .ok_or(InvalidInstruction)?;
        Ok(amount)
    }
}
