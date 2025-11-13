use bytemuck::{Pod, Zeroable};
use pinocchio::program_error::ProgramError;

use crate::state::DataLen;

#[repr(C)]
#[derive(Pod, Zeroable, Clone, Copy, Debug, PartialEq)]
pub struct OreRound {
    pub _disc: [u8; 8],

    /// The round number.
    pub id: u64,

    /// The amount of SOL deployed in each square.
    pub deployed: [u64; 25],

    /// The hash of the end slot, provided by solana, used for random number generation.
    pub slot_hash: [u8; 32],

    /// The count of miners on each square.
    pub count: [u64; 25],

    /// The slot at which claims for this round account end.
    pub expires_at: u64,

    /// The amount of ORE in the motherlode.
    pub motherlode: u64,

    /// The account to which rent should be returned when this account is closed.
    pub rent_payer: [u8; 32],

    /// The top miner of the round.
    pub top_miner: [u8; 32],

    /// The amount of ORE to distribute to the top miner.
    pub top_miner_reward: u64,

    /// The total amount of SOL deployed in the round.
    pub total_deployed: u64,

    /// The total amount of SOL put in the ORE vault.
    pub total_vaulted: u64,

    /// The total amount of SOL won by miners for the round.
    pub total_winnings: u64,
}

impl DataLen for OreRound {
    const LEN: usize = core::mem::size_of::<OreRound>();
}

/// Read bonding curve data from account
#[inline(always)]
pub fn read_ore_round_data(account_data: &[u8]) -> Result<&OreRound, ProgramError> {
    if account_data.len() < OreRound::LEN {
        return Err(ProgramError::InvalidAccountData);
    }
    Ok(bytemuck::from_bytes(&account_data[..OreRound::LEN]))
}

