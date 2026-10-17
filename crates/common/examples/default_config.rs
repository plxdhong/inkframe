//! Prints the default configuration: `cargo run -p common --example default_config`.
fn main() -> Result<(), common::config::ConfigError> {
    print!("{}", common::Config::default().to_toml()?);
    Ok(())
}
