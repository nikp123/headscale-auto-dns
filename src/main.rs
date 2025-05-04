mod headscale;
mod traefik;
mod processing;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut state = processing::Processing::new()?;

    state.update_servers()?;
    state.update_routers()?;
    state.generate_json()?;

    Ok(())
}
