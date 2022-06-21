use anchor_lang::{
    prelude::*,
    solana_program::{stake::state::StakeAuthorize, sysvar::SysvarId},
};
use anchor_spl::stake::{self, Authorize, Stake, StakeAccount};
use std::collections::HashSet;

use crate::{
    anchor_len::AnchorLen,
    errors::UnstakeError,
    state::{Pool, StakeAccountRecord},
};

#[derive(Accounts)]
pub struct Unstake<'info> {
    ///
    #[account(mut)]
    pub unstaker: Signer<'info>,

    ///
    pub pool_account: Account<'info, Pool>,

    /// pool's SOL reserves
    #[account(
        mut,
        seeds = [&pool_account.key().to_bytes()],
        bump,
    )]
    pub pool_sol_reserves: SystemAccount<'info>,

    ///
    #[account(
        mut,
        // TODO: constraint -> must be owned by the unstaker
        // TODO: constraint -> must not be locked (Deligated or Initialized)
    )]
    pub stake_account: Account<'info, StakeAccount>,

    /// (PDA)
    #[account(
        init,
        payer = unstaker,
        space = StakeAccountRecord::LEN,
    )]
    pub stake_account_record: Account<'info, StakeAccountRecord>,

    /// Solana native wallet pubkey to receive the unstaked amount
    /// CHECK: payment destination that can accept sol transfer
    pub destination: UncheckedAccount<'info>,

    #[account(
        // TODO: Do we need a check here? A new Error?
        constraint = Clock::check_id(clock.key),
    )]
    /// CHECK: need to check this
    pub clock: UncheckedAccount<'info>,
    pub stake_program: Program<'info, Stake>,
    pub system_program: Program<'info, System>,
}

impl<'info> Unstake<'info> {
    #[inline(always)]
    pub fn run(ctx: Context<Self>) -> Result<()> {
        let stake_account = &mut ctx.accounts.stake_account;
        let stake_program = &ctx.accounts.stake_program;
        let unstaker = &ctx.accounts.unstaker;
        let _pool_account = &ctx.accounts.pool_account;
        let pool_sol_reserves = &ctx.accounts.pool_sol_reserves;
        let stake_account_record = &mut ctx.accounts.stake_account_record;
        let clock = &ctx.accounts.clock;

        let authorized = stake_account
            .authorized()
            .ok_or(UnstakeError::StakeAccountAuthorizedNotRetrievable)?;
        // NOTE: check for withdrawer authority only since withdrawer can change both
        authorized
            .check(&HashSet::from([unstaker.key()]), StakeAuthorize::Withdrawer)
            .map_err(|_| UnstakeError::StakeAccountNotOwned)?;

        // cpi to stake::Authorize
        stake::authorize(
            CpiContext::new(
                stake_program.to_account_info(),
                Authorize {
                    stake: stake_account.to_account_info(),
                    authorized: unstaker.to_account_info(),
                    new_authorized: pool_sol_reserves.to_account_info(),
                    clock: clock.to_account_info(),
                },
            ),
            StakeAuthorize::Staker,
            None, // custodian
        )?;
        stake::authorize(
            CpiContext::new(
                stake_program.to_account_info(),
                Authorize {
                    stake: stake_account.to_account_info(),
                    authorized: unstaker.to_account_info(),
                    new_authorized: pool_sol_reserves.to_account_info(),
                    clock: clock.to_account_info(),
                },
            ),
            StakeAuthorize::Withdrawer,
            None, // custodian
        )?;

        // populate the stake_account_record
        // TODO: confirm if this value need to exclude rent exampt reserve
        //let meta = stake_account.meta();
        //meta.rent_exampt_reserve;
        stake_account_record.lamports_at_creation = stake_account.to_account_info().lamports();

        // TODO: pay-out from lp

        // TODO: update pool_account

        Ok(())
    }
}
