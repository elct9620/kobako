//! Thin CLI over `kobako_baker::bake` — Stage C invokes it as
//! `kobako-baker <input.wasm> <output.wasm>`.

use anyhow::{bail, Context, Result};

fn main() -> Result<()> {
    let mut args = std::env::args_os().skip(1);
    let (Some(input), Some(output), None) = (args.next(), args.next(), args.next()) else {
        bail!("usage: kobako-baker <input.wasm> <output.wasm>");
    };
    let wasm =
        std::fs::read(&input).with_context(|| format!("read {}", input.to_string_lossy()))?;
    let baked = kobako_baker::bake(&wasm)?;
    std::fs::write(&output, &baked)
        .with_context(|| format!("write {}", output.to_string_lossy()))?;
    Ok(())
}
