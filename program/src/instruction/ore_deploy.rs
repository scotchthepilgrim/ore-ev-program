use pinocchio::{
    account_info::AccountInfo,
    cpi::slice_invoke,
    instruction::{AccountMeta, Instruction},
    program_error::ProgramError,
    ProgramResult,
};

use bytemuck::{Pod, Zeroable};
use pinocchio_log::log;

use crate::{
    error::MyProgramError,
    state::{
        read_ore_round_data,
        utils::{load_ix_data, DataLen},
        OreRound,
    },
};

pub const ORE_DEPLOY_IX_DISCRIMINATOR: u8 = 6;

#[repr(C)]
#[derive(Pod, Zeroable, Clone, Copy, Debug)]
pub struct OreDeployIxData {
    /// Total SOL budget (will be allocated optimally across blocks)
    pub total_amount: u64,

    /// ORE price in lamports (for calculating optimal deployment)
    pub ore_price_lamports: u64,

    /// Minimum EV threshold in basis points
    /// Examples: -500 = accept -5% EV, 0 = break-even+, 150 = +1.5%+
    pub min_ev_threshold_bps: i16,

    /// Number of smallest blocks to target (1-5)
    pub num_blocks: u8,

    /// Padding (5 bytes)
    pub _padding: [u8; 5],
}

impl DataLen for OreDeployIxData {
    const LEN: usize = core::mem::size_of::<OreDeployIxData>();
}

pub fn process_ore_deploy(accounts: &[AccountInfo], data: &[u8]) -> ProgramResult {
    let [ore_program, signer, authority, automation, board, miner, round, system_program, entropy_var, entropy_program] =
        accounts
    else {
        return Err(ProgramError::NotEnoughAccountKeys);
    };

    let round_data = read_round_data(round)?;
    let ix_data = unsafe { load_ix_data::<OreDeployIxData>(data)? };

    // Validate inputs
    if ix_data.num_blocks == 0 || ix_data.num_blocks > 5 {
        log!("Error: num_blocks must be between 1 and 5");
        return Err(ProgramError::InvalidInstructionData);
    }

    if ix_data.ore_price_lamports == 0 {
        log!("Error: ore_price_lamports required");
        return Err(ProgramError::InvalidInstructionData);
    }

    log!("═══ ORE OPTIMAL DEPLOYMENT ═══");
    log!(
        "Total budget: {}.{} SOL",
        ix_data.total_amount / 1_000_000_000,
        (ix_data.total_amount % 1_000_000_000) / 1_000_000
    );
    log!(
        "ORE price: {}.{} SOL",
        ix_data.ore_price_lamports / 1_000_000_000,
        (ix_data.ore_price_lamports % 1_000_000_000) / 1_000_000
    );
    log!(
        "EV threshold: {} bps ({}.{}%)",
        ix_data.min_ev_threshold_bps,
        ix_data.min_ev_threshold_bps / 100,
        ix_data.min_ev_threshold_bps.abs() % 100
    );
    log!("Target blocks: up to {}", ix_data.num_blocks);

    // Calculate optimal deployment for smallest blocks
    let (num_selected, amounts, indices, evs) = calculate_optimal_deployments(
        &round_data,
        ix_data.total_amount,
        ix_data.num_blocks,
        ix_data.ore_price_lamports,
        ix_data.min_ev_threshold_bps,
    )?;

    if num_selected == 0 {
        log!(
            "✗ No blocks meet EV threshold of {} bps",
            ix_data.min_ev_threshold_bps
        );
        return Err(MyProgramError::NoPositiveEvBlocks.into());
    }

    log!("Deploying to {} blocks with optimal sizing:", num_selected);

    // Execute deployments
    for i in 0..num_selected as usize {
        let block_size = round_data.deployed[indices[i] as usize];

        // Calculate EV percentage
        let ev_bps = (evs[i] * 10_000) / amounts[i] as i64;
        let is_positive = ev_bps >= 0;
        let abs_ev_bps = ev_bps.abs() as u64;

        log!(
            "  Block #{} (size: {} mSOL):",
            indices[i],
            block_size / 1_000_000
        );
        log!(
            "    → Deploying {} mSOL (EV: {}{}.{}%)",
            amounts[i] / 1_000_000,
            if is_positive { "+" } else { "-" },
            abs_ev_bps / 100,
            abs_ev_bps % 100
        );

        let mask = 1u32 << indices[i];
        execute_deploy(
            ore_program,
            signer,
            authority,
            automation,
            board,
            miner,
            round,
            system_program,
            entropy_var,
            entropy_program,
            amounts[i],
            mask,
        )?;
    }

    let total_deployed: u64 = amounts[..num_selected as usize].iter().sum();
    log!(
        "✓ Total deployed: {} SOL across {} blocks",
        total_deployed / 1_000_000_000,
        num_selected
    );

    Ok(())
}

/// Calculate optimal deployment amounts for smallest blocks
/// Returns (num_blocks, amounts[], block_indices[], evs[])
fn calculate_optimal_deployments(
    round: &OreRound,
    total_budget: u64,
    max_blocks: u8,
    ore_price_lamports: u64,
    min_ev_threshold_bps: i16,
) -> Result<(u8, [u64; 5], [u8; 5], [i64; 5]), ProgramError> {
    // Calculate ORE value (includes motherlode, after refining fee)
    let ore_value = {
        let base = (ore_price_lamports * 9) / 10; // 10% refining fee
        let motherlode_ev = (round.motherlode * 9) / 6250; // (motherlode/625) * 0.9
        base + motherlode_ev
    };

    // Sort blocks by size (smallest first)
    let mut blocks: [(u8, u64); 25] = [(0, 0); 25];
    for i in 0..25 {
        blocks[i] = (i as u8, round.deployed[i]);
    }

    // Bubble sort ascending
    for i in 0..24 {
        for j in 0..(24 - i) {
            if blocks[j].1 > blocks[j + 1].1 {
                blocks.swap(j, j + 1);
            }
        }
    }

    // Step 1: Calculate optimal deployment for each of the smallest blocks
    let mut optimal_amounts: [u64; 5] = [0; 5];
    let mut total_optimal = 0u64;

    for i in 0..max_blocks as usize {
        let (_, block_size) = blocks[i];

        // Calculate Kelly-optimal deployment: y* = √(V × O / C) - O
        let optimal = calculate_kelly_optimal(block_size, round.total_deployed, ore_value);

        optimal_amounts[i] = optimal;
        total_optimal = total_optimal.saturating_add(optimal);
    }

    // Step 2: Scale to fit within budget (if needed)
    let scale_factor = if total_optimal > total_budget && total_optimal > 0 {
        (total_budget * 1_000_000_000) / total_optimal
    } else {
        1_000_000_000 // No scaling needed
    };

    // Step 3: Apply scaling and filter by EV threshold
    let mut count: u8 = 0;
    let mut amounts: [u64; 5] = [0; 5];
    let mut indices: [u8; 5] = [255; 5];
    let mut evs: [i64; 5] = [0; 5];

    for i in 0..max_blocks as usize {
        if optimal_amounts[i] == 0 {
            continue;
        }

        let (block_idx, block_size) = blocks[i];

        // Apply scaling
        let scaled_amount = (optimal_amounts[i] * scale_factor) / 1_000_000_000;

        if scaled_amount == 0 {
            continue;
        }

        // Calculate EV with final amount
        let ev = calculate_ev(block_size, scaled_amount, round.total_deployed, ore_value);

        // Check EV threshold
        let min_ev_lamports = (scaled_amount as i64 * min_ev_threshold_bps as i64) / 10_000;

        if ev >= min_ev_lamports {
            amounts[count as usize] = scaled_amount;
            indices[count as usize] = block_idx;
            evs[count as usize] = ev;
            count += 1;
        } else {
            // Smallest blocks have best EV, so if one fails threshold, stop
            break;
        }
    }

    Ok((count, amounts, indices, evs))
}

/// Calculate Kelly-optimal deployment for a single block
/// Formula: y* = √(V × O / C) - O
/// With iterative refinement to account for pot impact
fn calculate_kelly_optimal(block_size: u64, total_pool: u64, ore_value: u64) -> u64 {
    const C_SCALED: u64 = 24_252_500_000; // C = 24.2525 * 1e9

    if block_size == 0 || total_pool <= block_size {
        return 0;
    }

    // Initial pot value if this block wins
    let losing_pool = total_pool.saturating_sub(block_size);
    let winnings = (losing_pool * 9000) / 10_000; // After protocol fee
    let v = winnings.saturating_add(ore_value);

    if v == 0 {
        return 0;
    }

    // Calculate y* = √(V × O / C) - O
    let mut y_star = {
        let product = v.saturating_mul(block_size);
        let scaled = product.saturating_mul(1_000_000_000) / C_SCALED;
        isqrt(scaled).saturating_sub(block_size)
    };

    // Iterative refinement (accounts for deployment reducing pot)
    for _ in 0..5 {
        if y_star == 0 {
            break;
        }

        // Recalculate V with your deployment factored in
        let adjusted_pool = losing_pool.saturating_sub(y_star);
        let adjusted_winnings = (adjusted_pool * 9000) / 10_000;
        let new_v = adjusted_winnings.saturating_add(ore_value);

        if new_v == 0 {
            return 0;
        }

        // Recalculate y*
        let product = new_v.saturating_mul(block_size);
        let scaled = product.saturating_mul(1_000_000_000) / C_SCALED;
        let new_y_star = isqrt(scaled).saturating_sub(block_size);

        // Check convergence (within 100 lamports)
        let diff = if new_y_star > y_star {
            new_y_star - y_star
        } else {
            y_star - new_y_star
        };

        if diff < 100 {
            y_star = new_y_star;
            break;
        }

        y_star = new_y_star;
    }

    y_star
}

/// Integer square root (Newton's method)
#[inline(always)]
fn isqrt(n: u64) -> u64 {
    if n == 0 {
        return 0;
    }
    if n <= 3 {
        return 1;
    }

    let mut x = n >> 1;
    let mut y = (x + n / x) >> 1;

    for _ in 0..6 {
        if y >= x {
            return x;
        }
        x = y;
        y = (x + n / x) >> 1;
    }

    x
}

/// Calculate expected value for a deployment
fn calculate_ev(block_size: u64, deploy_amount: u64, total_pool: u64, ore_value: u64) -> i64 {
    if deploy_amount == 0 || block_size == 0 {
        return i64::MIN;
    }

    let total_block = block_size.saturating_add(deploy_amount);
    if total_block == 0 {
        return i64::MIN;
    }

    // Your share (in basis points)
    let share_bps = (deploy_amount * 10_000) / total_block;

    // Pot value if you win
    let losing_pool = total_pool.saturating_sub(block_size);
    let winnings = (losing_pool * 9000) / 10_000; // Protocol fee
    let pot = winnings.saturating_add(ore_value);

    // EV calculation
    let expected_win = (pot * share_bps) / (25 * 10_000);
    let expected_loss = (deploy_amount * 24) / 25;
    let admin_fee = (deploy_amount * 101) / 10_000;

    (expected_win as i64)
        .saturating_sub(expected_loss as i64)
        .saturating_sub(admin_fee as i64)
}

fn execute_deploy(
    ore_program: &AccountInfo,
    signer: &AccountInfo,
    authority: &AccountInfo,
    automation: &AccountInfo,
    board: &AccountInfo,
    miner: &AccountInfo,
    round: &AccountInfo,
    system_program: &AccountInfo,
    entropy_var: &AccountInfo,
    entropy_program: &AccountInfo,
    sol_amount: u64,
    squares: u32,
) -> ProgramResult {
    let mut instruction_data = [0u8; 13];
    instruction_data[0..1].copy_from_slice(&ORE_DEPLOY_IX_DISCRIMINATOR.to_le_bytes());
    instruction_data[1..9].copy_from_slice(&sol_amount.to_le_bytes());
    instruction_data[9..13].copy_from_slice(&squares.to_le_bytes());

    let account_metas: [AccountMeta; 9] = [
        AccountMeta::writable_signer(signer.key()),
        AccountMeta::writable_signer(authority.key()),
        AccountMeta::writable(automation.key()),
        AccountMeta::writable(board.key()),
        AccountMeta::writable(miner.key()),
        AccountMeta::writable(round.key()),
        AccountMeta::readonly(system_program.key()),
        AccountMeta::writable(entropy_var.key()),
        AccountMeta::readonly(entropy_program.key()),
    ];

    let instruction = Instruction {
        program_id: ore_program.key(),
        accounts: &account_metas,
        data: &instruction_data,
    };

    let account_refs: [&AccountInfo; 9] = [
        signer,
        authority,
        automation,
        board,
        miner,
        round,
        system_program,
        entropy_var,
        entropy_program,
    ];

    slice_invoke(&instruction, &account_refs)?;

    Ok(())
}

fn read_round_data(round: &AccountInfo) -> Result<OreRound, ProgramError> {
    let data = round.try_borrow_data()?;
    let decoded_round = read_ore_round_data(&data)?;
    Ok(*decoded_round)
}

