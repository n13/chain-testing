//! Substrate Node Template CLI library.
#![warn(missing_docs)]

mod benchmarking;
mod chain_spec;
mod cli;
mod command;
mod rpc;
mod service;
mod prometheus;
mod external_miner_client;
mod faucet;

fn main() -> sc_cli::Result<()> {
	command::run()
}
