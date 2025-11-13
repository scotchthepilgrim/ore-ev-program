# ORE EV Contract

A high-performance Solana program for optimal ORE mining deployment using Kelly criterion optimization. Built with Pinocchio for maximum efficiency (no heap allocations, no-std).

## What This Does

This program implements **Kelly-optimal ORE deployment** across multiple blocks in a single transaction. It:

1. **Analyzes the current ORE round** - Reads on-chain round data including block sizes, deployed amounts, and motherlode value
2. **Calculates optimal allocations** - Uses the Kelly criterion formula: `y* = √(V × O / C) - O` with iterative refinement
3. **Filters by EV threshold** - Only deploys to blocks meeting minimum expected value requirements
4. **Targets smallest blocks** - Focuses on 1-5 smallest blocks for best ROI
5. **Executes multi-block deployment** - Makes CPI calls to the ORE program to deploy optimally across selected blocks

### Key Features

- **Kelly Criterion Optimization**: Mathematically optimal position sizing accounting for pot impact
- **EV Threshold Filtering**: Configurable minimum EV in basis points (e.g., -500 bps = accept -5% EV)
- **Multi-Block Support**: Deploy to 1-5 smallest blocks in a single transaction
- **Dynamic Scaling**: Automatically scales deployments to fit within total budget
- **Zero Heap Allocations**: Ultra-efficient using Pinocchio (no-std, no allocator)

## Program Architecture

### Instructions

- **OreDeploy (discriminator: 1)** - Main instruction for optimal deployment
  - Parameters:
    - `total_amount` (u64) - Total SOL budget in lamports
    - `ore_price_lamports` (u64) - Current ORE price for EV calculations (e.g. 1 ORE = 1.6 * LAMPORTS_PER_SOL)
    - `min_ev_threshold_bps` (i16) - Minimum EV threshold in basis points
    - `num_blocks` (u8) - Number of smallest blocks to target (1-5)

### State

- **OreRound** - Deserialized ORE program round account containing:
  - Deployed amounts per block (25 blocks)
  - Total deployed, motherlode value
  - Round metadata

- **Utils** - Helper functions for safe data loading and serialization

## Build & Deploy

### Prerequisites

```bash
# Install Rust and Solana CLI
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
sh -c "$(curl -sSfL https://release.solana.com/stable/install)"
```

### Build Program

```bash
cd program
cargo build-sbf
```

### Get Program Address

```bash
solana address -k target/deploy/ore-ev-program-keypair.json
```

Update the program ID in `program/src/lib.rs` with the address above.

### Deploy

```bash
solana program deploy target/deploy/ore-ev-program.so
```

## Project Structure

```
program/
├── src/
│   ├── entrypoint.rs          # Program entrypoint (no-std, no allocator)
│   ├── lib.rs                 # Library root with program ID
│   ├── error.rs               # Custom error types
│   ├── instruction/
│   │   ├── mod.rs            # Instruction enum and routing
│   │   └── ore_deploy.rs     # ORE deployment logic with Kelly optimization
│   └── state/
│       ├── mod.rs            # State module exports
│       ├── ore_round.rs      # OreRound state structure
│       └── utils.rs          # Serialization/deserialization helpers
└── Cargo.toml               # Dependencies and features
```

## Algorithm Details

### Kelly Criterion Formula

For a single block deployment:

```
y* = √(V × O / C) - O

Where:
  V = Pot value if block wins (losing pool × 0.9 + ORE value)
  O = Current block size
  C = Kelly constant (24.2525)
  y* = Optimal deployment amount
```

### Iterative Refinement

The algorithm refines the optimal amount 5 times to account for pot impact:
1. Calculate initial `y*` with current pot
2. Recalculate pot assuming your deployment
3. Recalculate `y*` with adjusted pot
4. Repeat until convergence (< 100 lamports difference)

### EV Calculation

```
EV = (Expected Win) - (Expected Loss) - (Admin Fee)

Expected Win = (Pot × Your Share) / 25
Expected Loss = (Deployment × 24) / 25
Admin Fee = Deployment × 1.01%
```

## Performance

Built with Pinocchio for maximum efficiency:
- **No heap allocations** - Stack-only operations
- **No-std** - Minimal runtime overhead
- **Optimized serialization** - Zero-copy deserialization with bytemuck
- **Low compute units** - Efficient enough for multi-block deployments in one transaction
# ore-ev-program
