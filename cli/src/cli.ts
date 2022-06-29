#!/usr/bin/env node

import { Address, AnchorProvider, Program } from "@project-serum/anchor";
import {
  IDL_JSON,
  Unstake,
  addLiquidityTx,
  createPoolTx,
} from "@soceanfi/unstake";
import { Keypair, PublicKey } from "@solana/web3.js";
import { hideBin } from "yargs/helpers";
import yargs from "yargs";
import { keypairFromFile, parsePosSolToLamports, readJsonFile } from "./utils";
import { FeeArg, toFeeChecked } from "./feeArgs";
import {
  createAssociatedTokenAccount,
  createAssociatedTokenAccountInstruction,
  getAccount,
  getAssociatedTokenAddress,
} from "@solana/spl-token";

function initProgram(
  cluster: string,
  wallet: string,
  program: Address
): Program<Unstake> {
  process.env.ANCHOR_PROVIDER_URL = cluster;
  process.env.ANCHOR_WALLET = wallet;
  return new Program(IDL_JSON as Unstake, program, AnchorProvider.env());
}

yargs(hideBin(process.argv))
  .strict()
  .help("h")
  .alias("h", "help")
  .option("cluster", {
    describe: "solana cluster",
    default: "http://127.0.0.1:8899",
    type: "string",
  })
  .option("wallet", {
    describe: "path to wallet keypair file",
    default: `${process.env.HOME}/.config/solana/id.json`,
    type: "string",
  })
  .option("program_id", {
    describe: "program pubkey",
    default: "6KBz9djJAH3gRHscq9ujMpyZ5bCK9a27o3ybDtJLXowz",
    type: "string",
  })
  .command(
    "create_pool <fee_path>",
    "create a new unstake liquidity pool",
    (y) =>
      y
        .positional("fee_path", {
          type: "string",
          description:
            "Path to JSON file defining liquidity pool's fee settings. Example contents:\n" +
            '{ "liquidityLinear": { "maxLiqRemaining": 0.003, "zeroLiqRemaining": 0.03 }}\n' +
            '{ "flat": 0.01 }',
        })
        .option("payer", {
          type: "string",
          description: "Path to keypair paying for the pool's rent and tx fees",
          defaultDescription: "wallet",
        })
        .option("fee_authority", {
          type: "string",
          description: "Path to keypair actings as the pool's fee authority",
          defaultDescription: "wallet",
        })
        .option("pool_account", {
          type: "string",
          description: "Path to keypair that will be the pool's address",
          defaultDescription: "randomly generated keypair",
        })
        .option("lp_mint", {
          type: "string",
          description:
            "Path to keypair that will be the pool's LP mint address",
          defaultDescription: "randomly generated keypair",
        }),
    async ({
      cluster,
      wallet,
      program_id,
      fee_path,
      payer: payerOption,
      fee_authority: feeAuthorityOption,
      pool_account: poolAccountOption,
      lp_mint: lpMintOption,
    }) => {
      const program = initProgram(cluster, wallet, program_id);
      const provider = program.provider as AnchorProvider;
      const fee = toFeeChecked(readJsonFile(fee_path!) as FeeArg);
      console.log("Fee:", JSON.stringify(fee));
      const poolAccountDefault = Keypair.generate();
      const lpMintDefault = Keypair.generate();
      const accounts = {
        feeAuthority: provider.wallet.publicKey,
        poolAccount: poolAccountDefault.publicKey,
        lpMint: lpMintDefault.publicKey,
        payer: provider.wallet.publicKey,
      };
      const signers = {
        poolAccount: poolAccountDefault,
        lpMint: lpMintDefault,
      };
      const accountKeyToKeypairPathOption = {
        feeAuthority: feeAuthorityOption,
        poolAccount: poolAccountOption,
        lpMint: lpMintOption,
        payer: payerOption,
      };
      Object.entries(accountKeyToKeypairPathOption).forEach(
        ([accountKey, option]) => {
          if (option) {
            const keypair = keypairFromFile(option);
            accounts[accountKey as keyof typeof accounts] = keypair.publicKey;
            signers[accountKey as keyof typeof signers] = keypair;
          }
        }
      );
      const tx = await createPoolTx(program, fee, accounts);
      const sig = await provider.sendAndConfirm(tx, Object.values(signers));
      console.log(
        "Liquidity pool initialized at",
        accounts.poolAccount.toString(),
        ", LP mint:",
        accounts.lpMint.toString(),
        ", fee authority:",
        accounts.feeAuthority.toString()
      );
      console.log("TX:", sig);
    }
  )
  .command(
    "add_liquidity <pool_account> <amount_sol>",
    "adds SOL liquidity to a liquidity pool",
    (y) =>
      y
        .positional("pool_account", {
          type: "string",
          description: "pubkey of the liquidity pool to add liquidity to",
        })
        .positional("amount_sol", {
          type: "number",
          description: "amount in SOL to add as liquidity",
        })
        .option("from", {
          type: "string",
          description: "Path to the SOL keypair to add liquidity from",
          defaultDescription: "wallet",
        })
        .option("mint_lp_tokens_to", {
          type: "string",
          description: "LP token account to mint LP tokens to",
          defaultDescription: "ATA of from",
        }),
    async ({
      cluster,
      wallet,
      program_id,
      pool_account,
      amount_sol,
      from: fromOption,
      mint_lp_tokens_to: mintLpTokensToOption,
    }) => {
      const program = initProgram(cluster, wallet, program_id);
      const provider = program.provider as AnchorProvider;
      const poolKey = new PublicKey(pool_account!);
      const pool = await program.account.pool.fetch(poolKey);
      const poolAccount = {
        publicKey: poolKey,
        account: pool,
      };
      const amountSol = amount_sol!;
      const amountLamports = parsePosSolToLamports(amountSol);
      let from = provider.wallet.publicKey;
      const signers = [];
      if (fromOption) {
        const fromKeypair = keypairFromFile(fromOption);
        signers.push(fromKeypair);
        from = fromKeypair.publicKey;
      }
      const fromAta = await getAssociatedTokenAddress(pool.lpMint, from);
      const mintLpTokensTo = mintLpTokensToOption ?? fromAta;
      const tx = await addLiquidityTx(program, amountLamports, {
        from,
        poolAccount,
        mintLpTokensTo,
      });
      try {
        await getAccount(provider.connection, new PublicKey(mintLpTokensTo));
      } catch (e) {
        if (mintLpTokensTo.toString() !== fromAta.toString()) {
          throw new Error(
            `LP token account ${mintLpTokensTo.toString()} does not exist`
          );
        }
        console.log(
          "LP token account",
          mintLpTokensTo.toString(),
          "does not exist, creating..."
        );
        tx.instructions.unshift(
          createAssociatedTokenAccountInstruction(
            provider.wallet.publicKey,
            new PublicKey(mintLpTokensTo),
            from,
            pool.lpMint
          )
        );
      }
      const sig = await provider.sendAndConfirm(tx, signers);
      console.log(
        amountSol,
        "SOL liquidity added to pool at",
        poolKey.toString()
      );
      console.log("TX:", sig);
    }
  ).argv;
