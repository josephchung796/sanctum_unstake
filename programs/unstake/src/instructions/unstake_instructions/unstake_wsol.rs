use anchor_lang::prelude::*;
use anchor_spl::{
    stake::{Stake, StakeAccount},
    token::{self, SyncNative, Token, TokenAccount},
};

use crate::{
    anchor_len::AnchorLen,
    consts::WRAPPED_SOL_MINT,
    errors::UnstakeError,
    state::{Fee, Pool, StakeAccountRecord, FEE_SEED_SUFFIX},
};

use super::unstake_accounts::UnstakeAccounts;

#[derive(Accounts)]
pub struct UnstakeWSOL<'info> {
    /// pubkey paying for a new StakeAccountRecord account's rent
    #[account(mut)]
    pub payer: Signer<'info>,

    /// stake account owner
    pub unstaker: Signer<'info>,

    /// stake account to be unstaked
    /// rely on stake program CPI call to ensure owned by unstaker
    #[account(
        mut,
        // this also checks that a stake account is either
        // Initialized or Stake
        // NOTE: https://github.com/igneous-labs/unstake/issues/63
        //  - if lockup is not in force then the custodian cannot do anything
        //  - since the instruction updates both staker and withdrawer, lockup
        //    cannot be updated by the custodian or unstaker after the instruction
        //    resolves
        constraint = !stake_account.lockup()
            .ok_or(UnstakeError::StakeAccountLockupNotRetrievable)?
            .is_in_force(&clock, None)
            @ UnstakeError::StakeAccountLockupInForce,
    )]
    pub stake_account: Account<'info, StakeAccount>,

    /// Solana native wallet pubkey to receive the unstaked amount
    #[account(
        mut,
        constraint = destination.mint == WRAPPED_SOL_MINT @ UnstakeError::DestinationNotWSol
    )]
    pub destination: Account<'info, TokenAccount>,

    /// pool account that SOL reserves belong to
    #[account(mut)]
    pub pool_account: Account<'info, Pool>,

    /// pool's SOL reserves
    #[account(
        mut,
        seeds = [&pool_account.key().to_bytes()],
        bump,
    )]
    pub pool_sol_reserves: SystemAccount<'info>,

    /// pool's fee account
    #[account(
        seeds = [&pool_account.key().to_bytes(), FEE_SEED_SUFFIX],
        bump,
    )]
    pub fee_account: Account<'info, Fee>,

    /// stake account record to be created
    #[account(
        init,
        payer = payer,
        space = StakeAccountRecord::LEN,
        seeds = [&pool_account.key().to_bytes(), &stake_account.key().to_bytes()],
        bump,
    )]
    pub stake_account_record_account: Account<'info, StakeAccountRecord>,

    pub clock: Sysvar<'info, Clock>,
    pub stake_program: Program<'info, Stake>,
    pub system_program: Program<'info, System>,
    pub token_program: Program<'info, Token>,
}

impl_unstake_accounts!(UnstakeWSOL);

impl<'info> UnstakeWSOL<'info> {
    #[inline(always)]
    pub fn run(mut ctx: Context<Self>) -> Result<()> {
        let unstake_result = Self::run_unstake(&mut ctx)?;

        // sync native
        token::sync_native(CpiContext::new(
            ctx.accounts.token_program.to_account_info(),
            SyncNative {
                account: ctx.accounts.destination_account_info(),
            },
        ))?;

        // emit analytics log
        let (voter_pubkey, activation_epoch) =
            ctx.accounts.stake_account().delegation().map_or_else(
                || (String::from(""), String::from("")),
                |delegation| {
                    (
                        delegation.voter_pubkey.to_string(),
                        delegation.activation_epoch.to_string(),
                    )
                },
            );

        // Log Format:
        //  "unstake-log: [instruction, unstaker, stake_account_address, stake_account_voter, stake_account_activation_epoch, FEE, recorded_lamports, paid_lamports, fee_lamports]"
        //
        // Fee Format (see SPEC.md or fee.rs for details):
        //  "[fee_type; FEE_DETAILS]"
        msg!(
            "unstake-log: [2, {}, {}, {}, {}, {}, {}, {}, {}]",
            ctx.accounts.unstaker().key(),
            ctx.accounts.stake_account().key(),
            voter_pubkey,
            activation_epoch,
            ctx.accounts.fee_account().fee,
            unstake_result.stake_account_lamports,
            unstake_result.lamports_to_transfer,
            unstake_result.fee_lamports,
        );

        Ok(())
    }
}
