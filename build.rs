/// This build script uses `vergen` to expose build information at compile time.
use std::error::Error;

use vergen_gitcl::{BuildBuilder, Emitter, GitclBuilder, RustcBuilder, SysinfoBuilder};

fn main() -> Result<(), Box<dyn Error>> {
    bosion::gather();

    Emitter::default()
        .add_instructions(&BuildBuilder::all_build()?)?
        .add_instructions(&GitclBuilder::all_git()?)?
        .add_instructions(&RustcBuilder::all_rustc()?)?
        .add_instructions(&SysinfoBuilder::all_sysinfo()?)?
        .emit()?;

    Ok(())
}